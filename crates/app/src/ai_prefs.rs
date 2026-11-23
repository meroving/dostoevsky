use ai::cost::Slot;
use ai::{ModelRef, ServiceKind};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::tr;

use crate::assist::{self, Models, PrefsMenu};
use crate::prefs::Prefs;
use crate::prefs::AiPrefs;
use crate::sheets::{button, heading, row, segmented, set_prefs, toggle_with};
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::Workspace;

fn muted(t: impl Into<SharedString>) -> Div {
    div().text_size(px(11.5)).text_color(c(theme::muted())).child(t.into())
}

fn ok_mark() -> Div {
    div().text_color(c(theme::success())).child("✓")
}

fn spinner() -> Div {
    div().size(px(9.)).rounded_full().border_2().border_color(ca(theme::accent(), 0.7))
}

impl Workspace {
    pub(crate) fn render_ai_prefs(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let on = assist::on(cx);
        let mut rows: Vec<AnyElement> = vec![row(tr!("Use AI"), None, toggle_with("ai-on", on, |cx, v| assist::set_on(v, cx))).into_any_element()];
        if !on {
            rows.push(div().pt_3().text_size(px(11.5)).text_color(c(theme::muted())).child(tr!("AI is off. Nothing leaves this Mac.")).into_any_element());
            return rows;
        }
        let services: Vec<ServiceKind> = Prefs::get(cx).ai.services.iter().map(|s| s.kind).collect();
        rows.push(heading(tr!("SERVICES")));
        for kind in &services {
            rows.push(self.service_row(*kind, cx));
        }
        rows.push(self.add_service(&services, cx));
        if let Some(e) = assist::get(cx).and_then(|a| a.connect_error.clone()) {
            rows.push(div().py_1().text_size(px(12.)).text_color(c(theme::danger())).child(e).into_any_element());
        }
        let any_ready = services.iter().any(|k| assist::ready(*k, cx));
        rows.push(self.models_heading(any_ready, cx));
        rows.push(self.model_row(Slot::Quick, tr!("Quick model"), tr!("ideas for alternatives"), any_ready, cx));
        rows.push(self.model_row(Slot::Deep, tr!("Deep model"), tr!("AI marks and trims"), any_ready, cx));
        rows.push(heading(tr!("ALTERNATIVES")));
        let ideas = Prefs::get(cx).ai.ideas();
        rows.push(
            row(
                tr!("Ideas per ask"),
                None,
                segmented("ai-ideas", AiPrefs::IDEAS.iter().map(|n| (*n, n.to_string())).collect(), ideas, |p, v| p.ai.ideas = v),
            )
            .into_any_element(),
        );
        rows.push(div().pt_3().text_size(px(11.5)).text_color(c(theme::muted())).child(assist::privacy_line(cx)).into_any_element());
        rows
    }

    /// The MODELS heading carries Test connection: it tests the models listed under it,
    /// and its asking state and result show in the same place.
    fn models_heading(&mut self, any_ready: bool, cx: &mut Context<Self>) -> AnyElement {
        let right = any_ready.then(|| {
            let (testing, test) = assist::get(cx).map(|a| (a.testing, a.test.clone())).unwrap_or_default();
            match (testing, test) {
                (true, _) => div().flex().items_center().gap_2().child(spinner()).child(muted(tr!("asking…"))).into_any_element(),
                (_, Some(Ok(t))) => div().text_size(px(12.)).text_color(c(theme::success())).child(t).into_any_element(),
                (_, Some(Err(e))) => div().text_size(px(12.)).text_color(c(theme::danger())).child(e).into_any_element(),
                _ => div()
                    .id("ai-test")
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(c(theme::accent()))
                    .hover(|d| d.text_color(c(theme::text())))
                    .on_click(|_, _, cx| assist::test_connection(cx))
                    .child(tr!("Test connection"))
                    .test_support()
                    .into_any_element(),
            }
        });
        div()
            .pt_4()
            .pb_1()
            .flex()
            .items_center()
            .gap_3()
            .child(div().text_size(px(10.5)).text_color(c(theme::faint())).child(tr!("MODELS")))
            .child(div().flex_1())
            .children(right)
            .into_any_element()
    }

    fn service_row(&mut self, kind: ServiceKind, cx: &mut Context<Self>) -> AnyElement {
        let remove = |label: &'static str| {
            button(("ai-remove", kind as usize), label, false).on_click(move |_, _, cx| assist::remove_service(kind, cx)).test_support()
        };
        let control: AnyElement = match kind {
            ServiceKind::OpenRouter => {
                let (connecting, pasting) = assist::get(cx).map(|a| (a.connecting.is_some(), a.awaiting_paste.is_some())).unwrap_or_default();
                if assist::key(kind, cx).is_some() {
                    div().flex().items_center().gap_2().child(ok_mark()).child(div().text_color(c(theme::text())).child(tr!("Connected"))).child(muted(tr!("key in Keychain")).mr_2()).child(remove(tr!("Disconnect"))).into_any_element()
                } else if pasting {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(connecting, |d| d.child(spinner()))
                        .child(muted(tr!("Copy the code OpenRouter shows, then")))
                        .child(button("ai-paste-code", tr!("Paste code"), true).on_click(|_, _, cx| assist::paste_code(cx)).test_support())
                        .child(button("ai-cancel", tr!("Cancel"), false).on_click(|_, _, cx| assist::cancel_connect(cx)).test_support())
                        .into_any_element()
                } else if connecting {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(spinner())
                        .child(div().text_color(c(theme::text_2())).child(tr!("Waiting for openrouter.ai…")))
                        .child(button("ai-cancel", tr!("Cancel"), false).on_click(|_, _, cx| assist::cancel_connect(cx)).test_support())
                        .child(
                            div()
                                .id("ai-paste-instead")
                                .cursor_pointer()
                                .text_size(px(11.5))
                                .text_color(c(theme::accent()))
                                .on_click(|_, _, cx| assist::connect_with_code(cx))
                                .child(tr!("paste a code"))
                                .test_support(),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(button("ai-connect", tr!("Connect OpenRouter"), true).on_click(|_, _, cx| assist::connect_openrouter(cx)).test_support())
                        .child(muted(tr!("opens your browser")))
                        .child(
                            div()
                                .id("ai-paste-or-key")
                                .cursor_pointer()
                                .text_size(px(11.5))
                                .text_color(c(theme::accent()))
                                .on_click(move |_, _, cx| assist::paste_key(kind, cx))
                                .child(tr!("or paste a key"))
                                .test_support(),
                        )
                        .into_any_element()
                }
            }
            ServiceKind::OpenAi | ServiceKind::Gemini | ServiceKind::Mistral | ServiceKind::Groq => {
                let has = assist::key(kind, cx).is_some();
                let page = kind.key_page();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(has, |d| d.child(ok_mark()).child(div().text_color(c(theme::text())).child(tr!("Key saved"))).child(muted(tr!("in Keychain")).mr_2()))
                    .when(!has, |d| d.child(button("ai-paste-key", tr!("Paste key"), true).on_click(move |_, _, cx| assist::paste_key(kind, cx)).test_support()).child(muted(tr!("copy it first"))))
                    .when_some(page.filter(|_| !has), |d, url| {
                        d.child(
                            div()
                                .id(("ai-key-page", kind as usize))
                                .mr_2()
                                .cursor_pointer()
                                .text_size(px(11.5))
                                .text_color(c(theme::accent()))
                                .on_click(move |_, _, cx| cx.open_url(url))
                                .child(tr!("get a key"))
                                .test_support(),
                        )
                    })
                    .child(remove(tr!("Remove")))
                    .into_any_element()
            }
            ServiceKind::Ollama | ServiceKind::LmStudio => {
                let address = Prefs::get(cx).ai.service(kind).map(|s| s.short_address()).unwrap_or_default();
                let models = assist::get(cx).and_then(|a| a.models.get(&kind).cloned());
                let running = assist::get(cx).is_some_and(|a| a.found.contains(&kind)) || matches!(models, Some(Models::Ready(_)));
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(running, |d| d.child(ok_mark()).child(div().text_color(c(theme::text())).child(tr!("Running"))))
                    .when(!running, |d| d.child(div().text_color(c(theme::muted())).child(tr!("Not running"))))
                    .child(muted(address).mr_2())
                    .child(remove(tr!("Remove")))
                    .into_any_element()
            }
            ServiceKind::Custom => {
                let address = Prefs::get(cx).ai.service(kind).map(|s| s.base().to_string()).unwrap_or_default();
                let has_key = assist::key(kind, cx).is_some();
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .when(address.is_empty(), |d| d.child(button("ai-paste-address", tr!("Paste address"), true).on_click(|_, _, cx| assist::paste_address(cx)).test_support()))
                    .when(!address.is_empty(), |d| {
                        d.child(div().text_color(c(theme::text())).child(address))
                            .when(!has_key, |d| d.child(button("ai-paste-key", tr!("Paste key"), false).on_click(move |_, _, cx| assist::paste_key(kind, cx)).test_support()))
                            .when(has_key, |d| d.child(muted(tr!("key in Keychain"))))
                    })
                    .child(remove(tr!("Remove")))
                    .into_any_element()
            }
        };
        row(kind.label(), Some(kind.hint()), control).into_any_element()
    }

    fn add_service(&mut self, added: &[ServiceKind], cx: &mut Context<Self>) -> AnyElement {
        let open = assist::get(cx).is_some_and(|a| a.menu == Some(PrefsMenu::AddService));
        let missing: Vec<ServiceKind> = ServiceKind::ALL.into_iter().filter(|k| !added.contains(k)).collect();
        if missing.is_empty() {
            return Empty.into_any_element();
        }
        let found = assist::get(cx).map(|a| a.found.clone()).unwrap_or_default();
        let menu = open.then(|| {
            deferred(
                anchored().snap_to_window().child(
                    div()
                        .id("ai-add-menu")
                        .occlude()
                        .mt_1()
                        .w(px(360.))
                        .p_1()
                        .rounded_lg()
                        .border_1()
                        .border_color(c(theme::line()))
                        .bg(c(theme::bg_raised()))
                        .shadow_lg()
                        .children(missing.iter().map(|&k| {
                            div()
                                .id(("ai-add", k as usize))
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_3()
                                .py_1p5()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|d| d.bg(ca(theme::accent(), 0.18)))
                                .on_click(move |_, _, cx| assist::add_service(k, cx))
                                .child(div().flex().flex_col().child(div().text_color(c(theme::text())).child(k.label())).child(muted(k.hint())))
                                .when(found.contains(&k), |d| d.child(div().text_size(px(11.)).text_color(c(theme::success())).child(tr!("found"))))
                                .test_support()
                        }))
                        .test_support(),
                ),
            )
            .with_priority(2)
        });
        div()
            .flex()
            .flex_col()
            .py_3()
            .border_b_1()
            .border_color(c(theme::line()))
            .child(
                div()
                    .id("ai-add-service")
                    .cursor_pointer()
                    .text_color(c(theme::accent()))
                    .on_click(move |_, _, cx| {
                        let a = cx.default_global::<assist::Assist>();
                        a.menu = if open { None } else { Some(PrefsMenu::AddService) };
                        if !open {
                            assist::probe_local(cx);
                        }
                        cx.refresh_windows();
                    })
                    .child(tr!("+ Add a service ▾"))
                    .test_support(),
            )
            .children(menu)
            .into_any_element()
    }

    fn model_row(&mut self, slot: Slot, label: &'static str, hint: &'static str, enabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let current = Prefs::get(cx).ai.slot(slot).cloned();
        let open = enabled && assist::get(cx).is_some_and(|a| a.menu == Some(PrefsMenu::Model(slot)));
        let (name, service) = match &current {
            Some(m) => (assist::model_label(m, cx), m.service.label()),
            None => (tr!("Pick a model").to_string(), ""),
        };
        let id = match slot {
            Slot::Quick => "ai-quick",
            Slot::Deep => "ai-deep",
        };
        let picker = div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .w(px(320.))
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(if open { c(theme::accent()) } else { c(theme::line()) })
            .when(enabled, |d| d.cursor_pointer().on_click(cx.listener(move |this, _, window, cx| this.toggle_model_menu(slot, window, cx))))
            .child(div().text_color(if enabled { c(theme::text()) } else { c(theme::muted()) }).child(name))
            .child(muted(format!("{service} ▾")))
            .test_support();
        // In a box of its own under the picker: `anchored` is absolute, and would otherwise open over it.
        let menu = open.then(|| div().child(self.model_menu(slot, current.clone(), cx)));
        let control = div().flex().flex_col().child(picker).children(menu);
        div().when(!enabled, |d| d.opacity(0.45)).child(row(label, Some(hint), control)).into_any_element()
    }

    /// Opens a model menu with the caret in its search field, or closes it.
    fn toggle_model_menu(&mut self, slot: Slot, window: &mut Window, cx: &mut Context<Self>) {
        let open = assist::get(cx).is_some_and(|a| a.menu == Some(PrefsMenu::Model(slot)));
        if open {
            assist::close_menu(cx);
            self.model_search = None;
            window.focus(&self.sheet_focus, cx);
            return;
        }
        let a = cx.default_global::<assist::Assist>();
        a.menu = Some(PrefsMenu::Model(slot));
        a.all_models = false;
        let model = self.model.clone();
        let field = cx.new(|cx| {
            let mut f = TextView::new(Mode::Field, model, cx);
            f.placeholder = tr!("Search models").into();
            f
        });
        let sub = cx.subscribe_in(&field, window, Self::on_model_search);
        let focus = field.read(cx).focus.clone();
        self.model_search = Some(ModelSearch { slot, field, at: None, scroll: ScrollHandle::new(), _sub: sub });
        window.focus(&focus, cx);
        cx.refresh_windows();
    }

    /// The search field of a menu that has closed (a pick, a click outside) goes, and the sheet gets the keys back.
    pub(crate) fn tidy_model_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = &self.model_search else { return };
        if assist::get(cx).is_some_and(|a| a.menu == Some(PrefsMenu::Model(s.slot))) && self.sheet.is_some() {
            return;
        }
        let focused = s.field.read(cx).focus.is_focused(window);
        self.model_search = None;
        if focused && self.sheet.is_some() {
            window.focus(&self.sheet_focus, cx);
        }
    }

    fn model_query(&self, cx: &App) -> String {
        self.model_search.as_ref().map(|s| s.field.read(cx).field_text().trim().to_string()).unwrap_or_default()
    }

    fn on_model_search(&mut self, _: &Entity<TextView>, e: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.model_search.as_ref().map(|s| s.slot) else { return };
        let query = self.model_query(cx);
        let picks: Vec<ModelRef> = menu_parts(slot, &query, cx)
            .into_iter()
            .filter_map(|p| match p {
                Part::Model(m) => Some(m.r),
                _ => None,
            })
            .collect();
        let Some(s) = self.model_search.as_mut() else { return };
        match e {
            ViewEvent::Edited => {
                s.at = (!query.is_empty() && !picks.is_empty()).then_some(0);
                s.scroll.scroll_to_item(0);
                cx.refresh_windows();
            }
            ViewEvent::Arrow(d) if !picks.is_empty() => {
                let last = picks.len() as isize - 1;
                let at = match s.at {
                    Some(i) => (i as isize + d).clamp(0, last),
                    None if *d > 0 => 0,
                    None => last,
                };
                s.at = Some(at as usize);
                cx.refresh_windows();
            }
            ViewEvent::Confirm => {
                let at = s.at.or((!query.is_empty()).then_some(0));
                if let Some(r) = at.and_then(|i| picks.get(i)).cloned() {
                    pick_model(slot, r, cx);
                    self.model_search = None;
                    window.focus(&self.sheet_focus, cx);
                }
            }
            ViewEvent::Cancel if !query.is_empty() => {
                s.at = None;
                s.field.update(cx, |f, cx| f.set_field_text("", cx));
                cx.refresh_windows();
            }
            ViewEvent::Cancel => {
                assist::close_menu(cx);
                self.model_search = None;
                window.focus(&self.sheet_focus, cx);
            }
            _ => {}
        }
    }

    fn model_menu(&mut self, slot: Slot, current: Option<ModelRef>, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.model_query(cx);
        let (field, at, scroll) = match &self.model_search {
            Some(s) if s.slot == slot => (Some(s.field.clone()), s.at, Some(s.scroll.clone())),
            _ => (None, None, None),
        };
        let lit = c(theme::accent());
        let mut items: Vec<AnyElement> = Vec::new();
        // Which child of the list the keyboard is on, so it can be scrolled to.
        let mut at_child = None;
        let mut n = 0usize;
        for part in menu_parts(slot, &query, cx) {
            match part {
                Part::Header(title, count) => items.push(
                    div()
                        .flex()
                        .justify_between()
                        .px_3()
                        .pt_2()
                        .pb_1()
                        .text_size(px(10.5))
                        .text_color(c(theme::faint()))
                        .child(title)
                        .children(count.map(|t| div().text_color(c(theme::muted())).child(t)))
                        .into_any_element(),
                ),
                Part::Model(m) => {
                    let on = current.as_ref().is_some_and(|r| *r == m.r);
                    let kb = at == Some(n);
                    if kb {
                        at_child = Some(items.len());
                    }
                    n += 1;
                    let r = m.r.clone();
                    let lit_text = |t: String| {
                        let marks: Vec<_> = ai::service::query_ranges(&t, &query).into_iter().map(|r| (r, HighlightStyle { color: Some(lit), ..Default::default() })).collect();
                        StyledText::new(t).with_highlights(marks)
                    };
                    items.push(
                        div()
                            .id(("ai-model", n))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .when(on, |d| d.bg(ca(theme::accent(), 0.22)))
                            .when(kb && !on, |d| d.bg(ca(theme::accent(), 0.16)))
                            .hover(|d| d.bg(ca(theme::accent(), 0.16)))
                            .on_click(move |_, _, cx| pick_model(slot, r.clone(), cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .child(div().text_color(c(theme::text())).child(lit_text(m.name)))
                                    .children(m.blurb.map(|b| div().text_size(px(11.5)).text_color(c(theme::muted())).child(lit_text(b)))),
                            )
                            .children(m.cost.map(|c| muted(c).flex_none()))
                            .test_support()
                            .into_any_element(),
                    );
                }
                Part::All(kind, count) => items.push(
                    div()
                        .id(("ai-all-models", kind as usize))
                        .mx_2()
                        .my_1()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(c(theme::line()))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(c(theme::muted()))
                        .on_click(|_, _, cx| {
                            cx.default_global::<assist::Assist>().all_models = true;
                            cx.refresh_windows();
                        })
                        .child(tr!("All {service} models ({n})…", service = kind.label(), n = count))
                        .test_support()
                        .into_any_element(),
                ),
                Part::Note(t) => items.push(div().px_3().py_2().child(muted(t)).into_any_element()),
                Part::Error(e) => items.push(div().px_3().py_1().text_size(px(12.)).text_color(c(theme::danger())).child(e).into_any_element()),
            }
        }
        if let (Some(i), Some(s)) = (at_child, &scroll) {
            s.scroll_to_item(i);
        }
        let search = field.map(|f| {
            div()
                .id("ai-model-search")
                .flex()
                .items_center()
                .gap_2()
                .mb_1()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(c(theme::bg()))
                .border_1()
                .border_color(c(theme::accent()))
                .child(svg().path("chrome/find.svg").size(px(12.)).flex_none().text_color(c(theme::muted())))
                .child(div().flex_1().min_w_0().child(f))
                .test_support()
        });
        let list = div().id(("ai-model-list", slot as usize)).flex().flex_col().min_h_0().overflow_y_scroll().children(items);
        let list = match &scroll {
            Some(s) => list.track_scroll(s),
            None => list,
        };
        deferred(
            anchored().snap_to_window().child(
                div()
                    .id(("ai-model-menu", slot as usize))
                    .occlude()
                    .mt_1()
                    .w(px(430.))
                    .max_h(px(460.))
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(c(theme::line()))
                    .bg(c(theme::bg_raised()))
                    .shadow_lg()
                    .children(search)
                    .child(list)
                    .test_support(),
            ),
        )
        .with_priority(2)
    }
}

/// The model menu's search field (Preferences → AI).
pub(crate) struct ModelSearch {
    pub slot: Slot,
    pub field: Entity<TextView>,
    /// The row ↑↓ moved to, counting models only.
    pub at: Option<usize>,
    scroll: ScrollHandle,
    _sub: Subscription,
}

struct MenuModel {
    r: ModelRef,
    name: String,
    blurb: Option<String>,
    cost: Option<String>,
}

enum Part {
    Header(String, Option<String>),
    Model(MenuModel),
    All(ServiceKind, usize),
    Note(String),
    Error(String),
}

fn pick_model(slot: Slot, r: ModelRef, cx: &mut App) {
    set_prefs(cx, |p| p.ai.set_slot(slot, Some(r)));
    let a = cx.default_global::<assist::Assist>();
    a.menu = None;
    a.all_models = false;
    a.test = None;
    cx.refresh_windows();
}

/// What the menu lists, in order. With a search, every service's models whose name or id has each word;
/// without, the suggested ones and a free one (or all of them, after "All … models").
fn menu_parts(slot: Slot, query: &str, cx: &mut App) -> Vec<Part> {
    let added: Vec<ServiceKind> = Prefs::get(cx).ai.services.iter().map(|s| s.kind).collect();
    let services: Vec<ServiceKind> = added.into_iter().filter(|k| assist::ready(*k, cx)).collect();
    let all = assist::get(cx).is_some_and(|a| a.all_models);
    let ideas = Prefs::get(cx).ai.ideas();
    let mut parts = Vec::new();
    let mut found = 0usize;
    for kind in services {
        let title = if kind.local() { tr!("{service} · ON THIS MAC", service = kind.label().to_uppercase()) } else { kind.label().to_uppercase() };
        let models = assist::models_of(kind, cx);
        let state = assist::get(cx).and_then(|a| a.models.get(&kind));
        let loading = matches!(state, Some(Models::Loading) | None);
        let failed = match state {
            Some(Models::Failed(e)) => Some(e.clone()),
            _ => None,
        };
        let cost = |id: &str| models.iter().find(|m| m.id == id).and_then(|m| ai::cost::per_use_with(m, slot, ideas));
        let listed = |m: &ai::service::ModelInfo| {
            let blurb = if kind.local() { Some(tr!("installed").to_string()) } else { (m.name != m.id).then(|| m.id.clone()) };
            let cost = if kind.local() { Some(tr!("free").to_string()) } else { ai::cost::per_use_with(m, slot, ideas) };
            MenuModel { r: ModelRef { service: kind, model: m.id.clone() }, name: m.name.clone(), blurb, cost }
        };
        let suggested = ai::service::suggested(kind);
        let suggested_row = |s: &ai::service::Suggested| MenuModel { r: ModelRef { service: kind, model: s.id.into() }, name: s.name.into(), blurb: Some(s.blurb().into()), cost: cost(s.id) };
        if !query.is_empty() {
            let mut hits: Vec<MenuModel> = suggested.iter().filter(|s| ai::service::matches_query(s.name, s.id, query)).map(suggested_row).collect();
            hits.extend(models.iter().filter(|m| !suggested.iter().any(|s| s.id == m.id) && ai::service::matches_query(&m.name, &m.id, query)).map(listed));
            if !hits.is_empty() {
                let total = models.len().max(suggested.len());
                parts.push(Part::Header(title, Some(tr!("{at} of {n}", at = hits.len(), n = total))));
                found += hits.len();
                parts.extend(hits.into_iter().map(Part::Model));
            }
            if let Some(e) = failed {
                parts.push(Part::Error(e));
            }
            continue;
        }
        parts.push(Part::Header(title, None));
        if !suggested.is_empty() && !all {
            parts.extend(suggested.iter().map(|s| Part::Model(suggested_row(s))));
            if let Some(f) = ai::service::free_model(&models) {
                parts.push(Part::Model(MenuModel {
                    r: ModelRef { service: kind, model: f.id.clone() },
                    name: tr!("A free model").into(),
                    blurb: Some(tr!("{model} · limited uses a day", model = f.name)),
                    cost: Some(tr!("free").into()),
                }));
            }
            if !models.is_empty() {
                parts.push(Part::All(kind, models.len()));
            }
        } else {
            parts.extend(models.iter().map(|m| Part::Model(listed(m))));
        }
        if let Some(e) = failed {
            parts.push(Part::Error(e));
        } else if loading && models.is_empty() && suggested.is_empty() {
            parts.push(Part::Note(tr!("loading…").into()));
        }
    }
    if !query.is_empty() && found == 0 {
        parts.push(Part::Note(tr!("No model matches “{query}”.", query = query)));
    }
    parts
}
