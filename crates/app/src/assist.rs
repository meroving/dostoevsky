use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ai::cost::Slot;
use ai::{AiError, Endpoint, ModelInfo, ModelRef, Service, ServiceKind};
use gpui_kit::{App, Global};
use l10n::tr;

use crate::prefs::Prefs;
use crate::sheets::set_prefs;

#[derive(Clone)]
pub enum Models {
    Loading,
    Ready(Rc<Vec<ModelInfo>>),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefsMenu {
    AddService,
    Model(Slot),
    Language,
}

#[derive(Default)]
pub struct Assist {
    pub models: HashMap<ServiceKind, Models>,
    /// Cached so the Keychain is asked once per run.
    keys: HashMap<ServiceKind, Option<String>>,
    /// Set to stop waiting for the browser.
    pub connecting: Option<Arc<AtomicBool>>,
    /// The PKCE verifier that goes with a pasted code.
    pub awaiting_paste: Option<String>,
    pub connect_error: Option<String>,
    pub testing: bool,
    pub test: Option<Result<String, String>>,
    /// Local servers that answered.
    pub found: HashSet<ServiceKind>,
    pub menu: Option<PrefsMenu>,
    /// Services whose whole list the model menu shows ("All … models").
    pub all_models: HashSet<ServiceKind>,
}

impl Global for Assist {}

/// Requests block (ureq), so each runs on a thread of its own.
pub fn off_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> impl std::future::Future<Output = Option<T>> {
    let (tx, rx) = smol::channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(f());
    });
    async move { rx.recv().await.ok() }
}

fn state(cx: &mut App) -> &mut Assist {
    cx.default_global::<Assist>()
}

pub fn get(cx: &App) -> Option<&Assist> {
    cx.try_global::<Assist>()
}

pub fn menu_open(cx: &App) -> bool {
    get(cx).is_some_and(|a| a.menu.is_some())
}

/// False if no menu was open.
pub fn close_menu(cx: &mut App) -> bool {
    let open = menu_open(cx);
    if open {
        let a = state(cx);
        a.menu = None;
        a.all_models.clear();
        cx.refresh_windows();
    }
    open
}

pub fn key(kind: ServiceKind, cx: &mut App) -> Option<String> {
    if let Some(k) = state(cx).keys.get(&kind) {
        return k.clone();
    }
    let k = ai::secrets::get(kind.slug()).filter(|k| !k.is_empty());
    state(cx).keys.insert(kind, k.clone());
    k
}

fn store_key(kind: ServiceKind, k: Option<String>, cx: &mut App) {
    match &k {
        Some(k) => {
            if let Err(e) = ai::secrets::set(kind.slug(), k) {
                state(cx).connect_error = Some(tr!("Couldn't save the key: {e}", e = e));
            }
        }
        None => ai::secrets::delete(kind.slug()),
    }
    state(cx).keys.insert(kind, k);
}

pub fn on(cx: &App) -> bool {
    Prefs::get(cx).ai.enabled
}

/// Reads Preferences only, not the Keychain: the header mustn't wait on it.
pub fn connected(cx: &App) -> bool {
    let p = &Prefs::get(cx).ai;
    p.enabled && [&p.quick, &p.deep].into_iter().flatten().any(|m| p.service(m.service).is_some_and(|s| !s.base().is_empty()))
}

pub fn set_on(enabled: bool, cx: &mut App) {
    if !enabled {
        cancel_connect(cx);
        let a = state(cx);
        a.menu = None;
        a.all_models.clear();
        a.connect_error = None;
        a.test = None;
    }
    set_prefs(cx, |p| p.ai.enabled = enabled);
    if enabled {
        on_prefs_open(cx);
    }
}

pub fn ready(kind: ServiceKind, cx: &mut App) -> bool {
    let added = Prefs::get(cx).ai.service(kind).is_some();
    added && (!kind.needs_key() || key(kind, cx).is_some())
}

pub fn endpoint(kind: ServiceKind, cx: &mut App) -> Result<Endpoint, AiError> {
    let service = Prefs::get(cx).ai.service(kind).cloned().ok_or(AiError::NotSetUp)?;
    let key = key(kind, cx);
    if kind.needs_key() && key.is_none() {
        return Err(AiError::NotSetUp);
    }
    if service.base().is_empty() {
        return Err(AiError::NotSetUp);
    }
    Ok(Endpoint { kind, base: service.base().to_string(), key })
}

pub fn for_slot(slot: Slot, cx: &mut App) -> Result<(Endpoint, ModelRef), AiError> {
    let m = Prefs::get(cx).ai.slot(slot).cloned().ok_or(AiError::NotSetUp)?;
    Ok((endpoint(m.service, cx)?, m))
}

pub fn models_of(kind: ServiceKind, cx: &App) -> Rc<Vec<ModelInfo>> {
    match get(cx).and_then(|a| a.models.get(&kind)) {
        Some(Models::Ready(m)) => m.clone(),
        _ => Rc::new(Vec::new()),
    }
}

pub fn model_label(r: &ModelRef, cx: &App) -> String {
    ai::service::display_name(r, &models_of(r.service, cx))
}

/// Also fills empty slots with its defaults once the models arrive.
pub fn refresh_models(kind: ServiceKind, cx: &mut App) {
    let Ok(ep) = endpoint(kind, cx) else { return };
    if matches!(state(cx).models.get(&kind), Some(Models::Loading)) {
        return;
    }
    state(cx).models.insert(kind, Models::Loading);
    let job = off_thread(move || ai::client::list_models(&ep));
    cx.spawn(async move |cx| {
        let r = job.await;
        cx.update(|cx| {
            match r {
                Some(Ok(m)) => {
                    state(cx).models.insert(kind, Models::Ready(Rc::new(m)));
                    fill_empty_slots(kind, cx);
                }
                Some(Err(e)) => {
                    state(cx).models.insert(kind, Models::Failed(e.message(kind)));
                    if matches!(e, AiError::LocalNotRunning) {
                        state(cx).found.remove(&kind);
                    }
                }
                None => {
                    state(cx).models.remove(&kind);
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

pub fn fill_empty_slots(kind: ServiceKind, cx: &mut App) {
    let models = models_of(kind, cx);
    let (mut q, mut d) = ai::service::default_models(kind, &models);
    // OpenRouter renamed a model: fall back to its family, then to anything.
    let family = |f: &str| models.iter().find(|m| m.id.contains(f)).map(|m| m.id.clone());
    if q.is_none() && kind == ServiceKind::OpenRouter {
        q = family("claude-sonnet").or_else(|| models.first().map(|m| m.id.clone()));
    }
    if d.is_none() && kind == ServiceKind::OpenRouter {
        d = family("claude-opus").or_else(|| q.clone());
    }
    let ai_prefs = &Prefs::get(cx).ai;
    let (need_q, need_d) = (ai_prefs.quick.is_none(), ai_prefs.deep.is_none());
    if (need_q && q.is_some()) || (need_d && d.is_some()) {
        set_prefs(cx, |p| {
            if need_q {
                p.ai.quick = q.map(|model| ModelRef { service: kind, model });
            }
            if need_d {
                p.ai.deep = d.map(|model| ModelRef { service: kind, model });
            }
        });
    }
}

pub fn on_prefs_open(cx: &mut App) {
    if !on(cx) {
        return;
    }
    let kinds: Vec<ServiceKind> = Prefs::get(cx).ai.services.iter().map(|s| s.kind).collect();
    for k in kinds {
        if ready(k, cx) && !matches!(get(cx).and_then(|a| a.models.get(&k)), Some(Models::Ready(_) | Models::Loading)) {
            refresh_models(k, cx);
        }
    }
    probe_local(cx);
}

pub fn probe_local(cx: &mut App) {
    let bases: Vec<(ServiceKind, String)> = [ServiceKind::Ollama, ServiceKind::LmStudio]
        .into_iter()
        .map(|k| (k, Prefs::get(cx).ai.service(k).cloned().unwrap_or_else(|| Service::new(k)).base().to_string()))
        .collect();
    let job = off_thread(move || bases.into_iter().filter(|(_, b)| ai::client::local_running(b)).map(|(k, _)| k).collect::<HashSet<_>>());
    cx.spawn(async move |cx| {
        if let Some(found) = job.await {
            cx.update(|cx| {
                if state(cx).found != found {
                    state(cx).found = found;
                    cx.refresh_windows();
                }
            });
        }
    })
    .detach();
}

pub fn add_service(kind: ServiceKind, cx: &mut App) {
    state(cx).menu = None;
    if Prefs::get(cx).ai.service(kind).is_none() {
        set_prefs(cx, |p| p.ai.services.push(Service::new(kind)));
    }
    if ready(kind, cx) {
        refresh_models(kind, cx);
    }
    cx.refresh_windows();
}

/// OpenRouter keeps its row (Disconnect); other services go.
pub fn remove_service(kind: ServiceKind, cx: &mut App) {
    store_key(kind, None, cx);
    state(cx).models.remove(&kind);
    state(cx).test = None;
    set_prefs(cx, |p| {
        if kind != ServiceKind::OpenRouter {
            p.ai.services.retain(|s| s.kind != kind);
        }
        // Added again, it asks again.
        p.ai.allowed.retain(|k| *k != kind);
        if p.ai.quick.as_ref().is_some_and(|m| m.service == kind) {
            p.ai.quick = None;
        }
        if p.ai.deep.as_ref().is_some_and(|m| m.service == kind) {
            p.ai.deep = None;
        }
    });
    // Another service may fill the empty jobs.
    let others: Vec<ServiceKind> = Prefs::get(cx).ai.services.iter().map(|s| s.kind).filter(|k| *k != kind).collect();
    for k in others {
        if ready(k, cx) {
            fill_empty_slots(k, cx);
        }
    }
    cx.refresh_windows();
}

pub fn paste_key(kind: ServiceKind, cx: &mut App) {
    let Some(k) = cx.read_from_clipboard().and_then(|c| c.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && !t.contains(char::is_whitespace)) else {
        state(cx).connect_error = Some(tr!("Copy the key first, then press Paste key.").into());
        cx.refresh_windows();
        return;
    };
    state(cx).connect_error = None;
    store_key(kind, Some(k), cx);
    state(cx).models.remove(&kind);
    refresh_models(kind, cx);
    cx.refresh_windows();
}

pub fn paste_address(cx: &mut App) {
    let Some(url) = cx.read_from_clipboard().and_then(|c| c.text()).map(|t| t.trim().to_string()).filter(|t| t.starts_with("http://") || t.starts_with("https://")) else {
        state(cx).connect_error = Some(tr!("Copy an address starting with http:// or https:// first.").into());
        cx.refresh_windows();
        return;
    };
    state(cx).connect_error = None;
    set_prefs(cx, |p| {
        if let Some(s) = p.ai.services.iter_mut().find(|s| s.kind == ServiceKind::Custom) {
            s.base_url = url;
        }
        // A new address may be someone else's server: ask before sending it text.
        p.ai.allowed.retain(|k| *k != ServiceKind::Custom);
    });
    state(cx).models.remove(&ServiceKind::Custom);
    refresh_models(ServiceKind::Custom, cx);
}

pub fn connect_openrouter(cx: &mut App) {
    cancel_connect(cx);
    let pk = ai::oauth::pkce();
    let cb = match ai::oauth::Callback::listen() {
        Ok(cb) => cb,
        Err(_) => return connect_with_code(cx),
    };
    let url = ai::oauth::auth_url(Some(&cb.url()), &pk.challenge);
    // NSWorkspace, not a spawned `open`: the sandboxed build may not run other programs.
    cx.open_url(&url);
    let cancel = Arc::new(AtomicBool::new(false));
    let st = state(cx);
    st.connecting = Some(cancel.clone());
    st.connect_error = None;
    let verifier = pk.verifier;
    let job = off_thread(move || cb.wait(&cancel, Duration::from_secs(600)).and_then(|code| ai::oauth::exchange(&code, &verifier)));
    cx.spawn(async move |cx| {
        let r = job.await;
        cx.update(|cx| finish_connect(r, cx));
    })
    .detach();
    cx.refresh_windows();
}

pub fn connect_with_code(cx: &mut App) {
    cancel_connect(cx);
    let pk = ai::oauth::pkce();
    cx.open_url(&ai::oauth::auth_url(None, &pk.challenge));
    let st = state(cx);
    st.awaiting_paste = Some(pk.verifier);
    st.connect_error = None;
    cx.refresh_windows();
}

pub fn paste_code(cx: &mut App) {
    let Some(verifier) = state(cx).awaiting_paste.clone() else { return };
    let Some(code) = cx.read_from_clipboard().and_then(|c| c.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else {
        state(cx).connect_error = Some(tr!("Copy the code OpenRouter shows, then press Paste code.").into());
        cx.refresh_windows();
        return;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    state(cx).connecting = Some(cancel);
    let job = off_thread(move || ai::oauth::exchange(&code, &verifier));
    cx.spawn(async move |cx| {
        let r = job.await;
        cx.update(|cx| finish_connect(r, cx));
    })
    .detach();
    cx.refresh_windows();
}

pub fn cancel_connect(cx: &mut App) {
    let st = state(cx);
    if let Some(c) = st.connecting.take() {
        c.store(true, Ordering::Relaxed);
    }
    st.awaiting_paste = None;
    cx.refresh_windows();
}

fn finish_connect(r: Option<Result<String, AiError>>, cx: &mut App) {
    let st = state(cx);
    st.connecting = None;
    match r {
        Some(Ok(key)) => {
            st.awaiting_paste = None;
            st.connect_error = None;
            store_key(ServiceKind::OpenRouter, Some(key), cx);
            if Prefs::get(cx).ai.service(ServiceKind::OpenRouter).is_none() {
                set_prefs(cx, |p| p.ai.services.insert(0, Service::new(ServiceKind::OpenRouter)));
            }
            // The suggested models straight away; the list follows.
            fill_empty_slots(ServiceKind::OpenRouter, cx);
            state(cx).models.remove(&ServiceKind::OpenRouter);
            refresh_models(ServiceKind::OpenRouter, cx);
            cx.activate(true);
        }
        Some(Err(AiError::Cancelled)) | None => {}
        Some(Err(e)) => st.connect_error = Some(e.message(ServiceKind::OpenRouter)),
    }
    cx.refresh_windows();
}

#[cfg(any(test, feature = "shots"))]
pub fn set_key_for_tests(kind: ServiceKind, k: &str, cx: &mut App) {
    store_key(kind, Some(k.to_string()), cx);
}

pub fn test_connection(cx: &mut App) {
    let mut jobs = Vec::new();
    for slot in [Slot::Quick, Slot::Deep] {
        if let Ok((ep, m)) = for_slot(slot, cx) {
            if !jobs.iter().any(|(_, e, mm): &(Slot, Endpoint, ModelRef)| e.base == ep.base && mm.model == m.model) {
                jobs.push((slot, ep, m));
            }
        }
    }
    if jobs.is_empty() {
        state(cx).test = Some(Err(tr!("Pick a model first.").into()));
        cx.refresh_windows();
        return;
    }
    let st = state(cx);
    st.testing = true;
    st.test = None;
    let n = jobs.len();
    let job = off_thread(move || jobs.into_iter().map(|(_, ep, m)| (ep.kind, ai::client::ping(&ep, &m.model))).collect::<Vec<_>>());
    cx.spawn(async move |cx| {
        let r = job.await.unwrap_or_default();
        cx.update(|cx| {
            let st = state(cx);
            st.testing = false;
            let failed = r.iter().find_map(|(k, r)| r.as_ref().err().map(|e| e.message(*k)));
            st.test = Some(match failed {
                Some(e) => Err(e),
                None if n == 1 => Ok(tr!("✓ answered in {secs} s", secs = format!("{:.1}", r[0].1.as_ref().map(|d| d.as_secs_f32()).unwrap_or(0.)))),
                None => Ok(tr!("✓ both answered").into()),
            });
            cx.refresh_windows();
        });
    })
    .detach();
    cx.refresh_windows();
}

pub fn privacy_line(cx: &App) -> &'static str {
    let p = &Prefs::get(cx).ai;
    let local = |m: &Option<ModelRef>| m.as_ref().map(|m| m.service.local());
    match (local(&p.quick), local(&p.deep)) {
        (Some(true), Some(true)) => tr!("Both models run on this Mac. Nothing leaves it."),
        (Some(true), Some(false)) => tr!("Quick stays on this Mac. Deep sends text out when you ask."),
        (Some(false), Some(true)) => tr!("Deep stays on this Mac. Quick sends text out when you ask."),
        _ => tr!("Text leaves this Mac only when you ask for something."),
    }
}

/// Text goes off this Mac only to a service the writer has allowed.
/// Ollama and LM Studio run here and never ask.
pub fn needs_consent(kind: ServiceKind, cx: &App) -> bool {
    !kind.local() && !Prefs::get(cx).ai.allowed.contains(&kind)
}

fn set_allowed(kind: ServiceKind, allowed: bool, cx: &mut App) {
    set_prefs(cx, |p| {
        p.ai.allowed.retain(|k| *k != kind);
        if allowed {
            p.ai.allowed.push(kind);
        }
    });
}

fn consent_name(kind: ServiceKind, cx: &App) -> String {
    if kind != ServiceKind::Custom {
        return kind.label().to_string();
    }
    let base = Prefs::get(cx).ai.service(kind).map(|s| s.base().to_string()).unwrap_or_default();
    base.split("://").nth(1).and_then(|h| h.split('/').next()).filter(|h| !h.is_empty()).map(str::to_string).unwrap_or_else(|| kind.label().to_string())
}

impl crate::workspace::Workspace {
    /// True when a request may go to `kind` now; otherwise asks once and, on Allow, runs `then`.
    pub(crate) fn ai_allowed(&mut self, kind: ServiceKind, cx: &mut gpui_kit::Context<Self>, then: impl FnOnce(&mut Self, &mut gpui_kit::Context<Self>) + 'static) -> bool {
        if !needs_consent(kind, cx) {
            return true;
        }
        let name = consent_name(kind, cx);
        let msg = tr!("Send your text to {service}?", service = name);
        let detail = tr!(
            "To answer, Dostoevsky sends {service} the passage you asked about, the text around it and the document’s title. {service} handles it under its own privacy policy; Dostoevsky itself receives nothing.",
            service = name
        );
        let handle = self.window;
        cx.spawn(async move |this, cx| {
            let Ok(rx) = handle.update(cx, |_, window, cx| window.prompt(gpui_kit::PromptLevel::Info, &msg, Some(&detail), &[tr!("Allow"), tr!("Don't Allow")], cx)) else {
                return;
            };
            let Ok(answer) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                if answer == 0 {
                    set_allowed(kind, true, cx);
                    then(this, cx);
                } else {
                    this.flash_status(tr!("Nothing was sent"), cx);
                }
            });
        })
        .detach();
        false
    }
}
