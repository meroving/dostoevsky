use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ai::{ModelRef, Service, ServiceKind};

use crate::ui_tests::{Ui, key_label};

pub const DOC: &str = "# Keep the Words\n\nMost of the friction in writing comes from choosing too early. You pick teh word and the other candidates quietly disappear.\n\nIt is, in a way, basically a kind of a one-way street, really.\n";

/// Answers each request with the next reply (the last one repeats) and keeps what it was sent.
pub struct Fake {
    pub base: String,
    pub requests: Arc<Mutex<Vec<String>>>,
}

pub fn sse(pieces: &[&str]) -> String {
    let mut body = String::new();
    for p in pieces {
        let v = serde_json::json!({"choices": [{"delta": {"content": p}}]});
        body.push_str(&format!("data: {v}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

pub fn http(status: &str, body: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

pub fn fake(replies: Vec<String>) -> Fake {
    fake_slow(replies, 0)
}

/// Every reply waits `ms` first: time to type while a pass runs.
pub fn fake_slow(replies: Vec<String>, ms: u64) -> Fake {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", l.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    std::thread::spawn(move || {
        let mut i = 0;
        for stream in l.incoming() {
            let Ok(mut s) = stream else { break };
            let mut req = Vec::new();
            let mut buf = vec![0u8; 65536];
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                req.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&req).to_string();
                if let Some(h) = text.find("\r\n\r\n") {
                    let len = text[..h]
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if req.len() >= h + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&req).to_string();
            let reply = if text.starts_with("GET /v1/models") {
                http("200 OK", "{\"data\":[{\"id\":\"test-small:1b\"},{\"id\":\"test-large:70b\"}]}")
            } else {
                seen.lock().unwrap().push(text);
                std::thread::sleep(std::time::Duration::from_millis(ms));
                let r = replies[i.min(replies.len() - 1)].clone();
                i += 1;
                r
            };
            let _ = s.write_all(reply.as_bytes());
        }
    });
    Fake { base, requests }
}

pub fn use_fake(ui: &mut Ui, base: &str) {
    let base = base.to_string();
    ui.with_app(|cx| {
        crate::sheets::set_prefs(cx, |p| {
            p.ai.services = vec![Service { kind: ServiceKind::Custom, base_url: base.clone() }];
            let m = ModelRef { service: ServiceKind::Custom, model: "test".into() };
            p.ai.quick = Some(m.clone());
            p.ai.deep = Some(m);
            // Allowed already; consent has tests of its own.
            p.ai.allowed = vec![ServiceKind::Custom];
        })
    });
}

/// A Custom address nobody answers; the local passes never ask it.
pub fn connect(ui: &mut Ui) {
    use_fake(ui, "http://127.0.0.1:9/v1");
}

fn ideas_running(ui: &Ui) -> bool {
    ui.read(|ws, _| ws.ideas.as_ref().is_some_and(|r| r.running))
}

fn ideas_error(ui: &Ui) -> Option<String> {
    ui.read(|ws, _| ws.ideas.as_ref().and_then(|r| r.error.as_ref().map(|e| e.message.clone())))
}

#[test]
fn the_first_request_to_a_service_asks_first() {
    let f = fake(vec![sse(&["tension\ndrag\nresistance\nstrain\npull\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.set_prefs(|p| p.ai.allowed.clear());
    ui.select_text("friction");
    ui.keys("alt-cmd-a");
    let g = ui.sidebar().expect("the group is open");
    assert!(ui.app.has_pending_prompt(), "asks before sending");
    ui.answer_prompt("Don't Allow");
    assert_eq!(ui.status().as_deref(), Some("Nothing was sent"));
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(f.requests.lock().unwrap().is_empty(), "nothing went out");
    assert!(ui.read(|_, cx| !crate::prefs::Prefs::get(cx).ai.allowed.contains(&ServiceKind::Custom)));

    ui.keys("alt-cmd-a");
    assert!(ui.app.has_pending_prompt(), "asks again");
    ui.answer_prompt("Allow");
    assert!(ui.wait_for(5., |ui| !ideas_running(ui) && ui.variants(g).len() == 6), "the request goes once allowed");
    assert!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.allowed.contains(&ServiceKind::Custom)));

    run(&mut ui, ai::lab::Pass::Tone);
    assert!(!ui.app.has_pending_prompt());
}

#[test]
fn consent_is_per_service_and_forgotten_on_remove() {
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", silent.local_addr().unwrap());
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &base);
    ui.set_prefs(|p| p.ai.allowed.clear());
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.app.has_pending_prompt(), "a pass asks");
    assert!(!lab_running(&ui), "and waits");
    ui.answer_prompt("Allow");
    assert!(ui.wait_for(2., lab_running), "then runs");
    ui.keys("escape");
    ui.with_app(|cx| {
        assert!(!crate::assist::needs_consent(ServiceKind::Custom, cx));
        assert!(!crate::assist::needs_consent(ServiceKind::Ollama, cx), "local: never asks");
        assert!(!crate::assist::needs_consent(ServiceKind::LmStudio, cx));
        assert!(crate::assist::needs_consent(ServiceKind::OpenRouter, cx), "each service asks for itself");
        crate::assist::remove_service(ServiceKind::Custom, cx);
        assert!(crate::assist::needs_consent(ServiceKind::Custom, cx), "added again, it asks again");
    });
    drop(silent);
}

#[test]
fn cmd_shift_g_without_ai_says_how_to_set_it_up() {
    let mut ui = Ui::open(DOC);
    ui.select_text("friction");
    ui.keys("alt-cmd-a");
    let g = ui.sidebar().expect("the group is open");
    assert_eq!(ui.variants(g), ["friction"]);
    assert_eq!(ideas_error(&ui).as_deref(), Some("No AI is set up yet."));
    ui.click_id("ideas-setup");
    assert_eq!(ui.sheet(), Some("prefs"));
    assert!(ui.has_element("ai-connect"), "Preferences opens on AI");
}

#[test]
fn cmd_shift_g_adds_five_ideas_that_undo_together() {
    let f = fake(vec![sse(&["1. tension\n", "drag\n- resistance\nFriction\n", "\"hesitation\"\nstrain\nextra\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.keys("alt-cmd-a");
    let g = ui.sidebar().unwrap();
    assert!(ui.wait_for(5., |ui| !ideas_running(ui)), "ideas never finished");
    assert_eq!(ui.variants(g), ["friction", "tension", "drag", "resistance", "hesitation", "strain"], "numbering, quotes and repeats are cleaned; five only");
    assert!(ui.text().contains("Most of the friction in writing"), "nothing is chosen for you");
    let info = ui.read(|ws, cx| ws.model.read(cx).doc.group(g).unwrap());
    assert!(info.variants[1..].iter().all(|v| v.by == doc_model::Source::Ai));
    let req = f.requests.lock().unwrap()[0].clone();
    assert!(req.contains("\\u27e6friction\\u27e7") || req.contains("⟦friction⟧"), "{req}");
    assert!(req.contains("Title: Keep the Words"));
    ui.keys("cmd-z");
    assert_eq!(ui.variants(g), ["friction"], "one ⌘Z removes the batch");
}

#[test]
fn a_words_ideas_leave_out_the_words_around_it() {
    let f = fake(vec![sse(&["the drag\n", "Of the pull in\n", "tension\n", "the friction\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.keys("alt-cmd-a");
    let g = ui.sidebar().unwrap();
    assert!(ui.wait_for(5., |ui| !ideas_running(ui)), "ideas never finished");
    assert_eq!(ui.variants(g), ["friction", "drag", "pull", "tension"], "“Most of the ⟦…⟧ in” is already there; a repeat of the original goes");
    assert!(f.requests.lock().unwrap()[0].contains("never repeat the words just before or after it"));
}

#[test]
fn ideas_per_ask_sets_how_many_come() {
    let f = fake(vec![sse(&["tension\n", "drag\n", "resistance\n", "strain\n", "pull\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    assert_eq!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.ideas), 5, "five by default");
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    ui.click_id(("ai-ideas", 0usize));
    assert_eq!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.ideas), 3);
    ui.keys("escape");
    ui.select_text("friction");
    ui.keys("alt-cmd-a");
    let g = ui.sidebar().unwrap();
    assert_eq!(ui.read(|ws, _| ws.ideas.as_ref().map(|r| r.count)), Some(3));
    assert!(ui.wait_for(5., |ui| !ideas_running(ui)), "ideas never finished");
    assert_eq!(ui.variants(g), ["friction", "tension", "drag", "resistance"], "three only");
    let req = f.requests.lock().unwrap()[0].clone();
    assert!(req.contains("exactly 3 suggestions"), "{req}");
    ui.type_text("??");
    ui.draw();
    assert!(ui.has_element("enter-ideas"));
    assert_eq!(ui.read(|_, cx| crate::ideas::per_ask(cx)), 3, "the footer's ↩ 3 ideas");
    let p: crate::prefs::Prefs = serde_json::from_str(r#"{"ai": {"ideas": 100}}"#).unwrap();
    assert_eq!(p.ai.ideas(), 10);
}

#[test]
fn question_marks_steer_more_ideas() {
    let f = fake(vec![sse(&["tension\ndrag\nresistance\nstrain\npull\n"]), sse(&["rub\ncatch\nsnag\nbind\nhitch\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.keys("alt-cmd-a");
    let g = ui.sidebar().unwrap();
    assert!(ui.wait_for(5., |ui| !ideas_running(ui)));
    ui.type_text("?? plainer");
    ui.keys("enter");
    assert!(ui.wait_for(5., |ui| !ideas_running(ui) && ui.variants(g).len() == 11));
    assert_eq!(ui.variants(g)[6..], ["rub", "catch", "snag", "bind", "hitch"]);
    let req = f.requests.lock().unwrap()[1].clone();
    assert!(req.contains("Make the suggestions: plainer"));
    assert!(req.contains("- tension"), "the versions already there are sent");
}

fn add_field(ui: &Ui) -> (String, String) {
    ui.read(|ws, cx| {
        let i = ws.sidebar.as_ref().unwrap().input.read(cx);
        (i.field_text().to_string(), i.hint_after.to_string())
    })
}

#[test]
fn footer_follows_the_add_field() {
    let f = fake(vec![sse(&["tension\ndrag\nresistance\nstrain\npull\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.action(crate::actions::AlternativesForSelection);
    let g = ui.sidebar().unwrap();
    ui.draw();
    assert!(ui.has_element("ideas-key") && !ui.has_element("enter-ideas") && !ui.has_element("enter-add"));
    ui.click_id("ideas-key");
    ui.draw();
    assert_eq!(add_field(&ui), ("?? ".to_string(), format!("plainer {}", key_label("⇥"))), "a word's first default steer, which ⇥ takes");
    assert!(ui.has_element("enter-ideas") && !ui.has_element("ideas-key"));
    ui.type_text("plainer");
    ui.draw();
    assert_eq!(add_field(&ui), ("?? plainer".to_string(), String::new()), "the hint goes once a steer is typed");
    assert!(ui.has_element("enter-ideas"));
    ui.keys("enter");
    assert!(ui.wait_for(5., |ui| !ideas_running(ui) && ui.variants(g).len() == 6), "{:?}", ui.variants(g));
    assert!(f.requests.lock().unwrap()[0].contains("Make the suggestions: plainer"));
    ui.draw();
    assert!(ui.has_element("ideas-key"), "the field is empty again");
    ui.type_text("toil");
    ui.draw();
    assert!(ui.has_element("enter-add") && !ui.has_element("ideas-key"));
    ui.with_app(|cx| crate::assist::set_on(false, cx));
    ui.draw();
    assert!(!ui.has_element("ideas-key") && !ui.has_element("enter-add") && !ui.has_element("enter-ideas"), "AI off: the footer as before");
}

#[test]
fn steers_are_remembered() {
    let f = fake(vec![sse(&["tension\ndrag\nresistance\nstrain\npull\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.action(crate::actions::AlternativesForSelection);
    let g = ui.sidebar().unwrap();
    ui.type_text("?? more concrete");
    ui.keys("enter");
    assert!(ui.wait_for(5., |ui| !ideas_running(ui) && ui.variants(g).len() == 6), "{:?}", ui.variants(g));
    assert_eq!(ui.read(|_, cx| crate::memory::Memory::steers(cx)), ["more concrete"], "remembered once its ideas came");

    ui.type_text("?? ");
    ui.draw();
    assert_eq!(add_field(&ui).1, format!("more concrete {}", key_label("⇥")), "yours first");
    ui.keys("tab");
    assert_eq!(add_field(&ui).0, "?? more concrete");
    assert_eq!(ui.read(|ws, cx| ws.sidebar.as_ref().unwrap().input.read(cx).selection()), 3..16, "selected, so typing replaces it");
    ui.keys("tab");
    assert_eq!(add_field(&ui).0, "?? plainer", "⇥ again: the next");
    ui.type_text("w");
    ui.draw();
    assert_eq!(add_field(&ui), ("?? w".to_string(), format!("armer {}", key_label("⇥"))), "part of one: the rest of it");
    ui.keys("tab");
    assert_eq!(add_field(&ui).0, "?? warmer");

    ui.with_app(crate::memory::Memory::clear);
    assert!(ui.read(|_, cx| crate::memory::Memory::steers(cx)).is_empty(), "Forget… forgets the steers too");
}

#[test]
fn question_marks_need_no_space() {
    let f = fake(vec![sse(&["tension\ndrag\nresistance\nstrain\npull\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.action(crate::actions::AlternativesForSelection);
    let g = ui.sidebar().unwrap();
    ui.type_text("??");
    ui.draw();
    assert_eq!(add_field(&ui).1, format!(" plainer {}", key_label("⇥")), "a space before the hint when none was typed");
    ui.type_text("plainer");
    ui.keys("enter");
    assert!(ui.wait_for(5., |ui| !ideas_running(ui) && ui.variants(g).len() == 6), "{:?}", ui.variants(g));
    assert!(!ui.variants(g).iter().any(|v| v.starts_with("??")));
    assert!(f.requests.lock().unwrap()[0].contains("Make the suggestions: plainer"));
}

#[test]
fn a_refusal_is_one_quiet_line() {
    let body = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\ndata: [DONE]\n\n";
    let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.keys("alt-cmd-a");
    assert!(ui.wait_for(5., |ui| !ideas_running(ui)));
    assert_eq!(ideas_error(&ui).as_deref(), Some("The model declined this passage."));
    assert!(ui.has_element("ideas-setup"));
    assert_eq!(ui.variants(ui.sidebar().unwrap()), ["friction"]);
}

#[test]
fn prefs_services_and_models() {
    let f = fake(vec![http("200 OK", "{}")]);
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    assert!(ui.has_element("ai-connect"));
    assert!(ui.has_element("ai-quick"));
    let base = f.base.clone();
    ui.with_app(|cx| {
        crate::assist::add_service(ServiceKind::Custom, cx);
        crate::sheets::set_prefs(cx, |p| p.ai.services.iter_mut().find(|s| s.kind == ServiceKind::Custom).unwrap().base_url = base.clone());
        crate::assist::refresh_models(ServiceKind::Custom, cx);
    });
    let done = ui.wait_for(5., |ui| ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.quick.is_some()));
    assert!(done, "Quick was never filled");
    let (q, d) = ui.read(|_, cx| {
        let p = &crate::prefs::Prefs::get(cx).ai;
        (p.quick.clone().unwrap().model, p.deep.clone().unwrap().model)
    });
    assert_eq!((q.as_str(), d.as_str()), ("test-small:1b", "test-large:70b"));
    ui.click_id("ai-deep");
    let menu = ui.read(|_, cx| crate::assist::get(cx).and_then(|a| a.menu));
    assert_eq!(menu, Some(crate::assist::PrefsMenu::Model(ai::cost::Slot::Deep)));
    assert!(ui.has_element(("ai-model-menu", 1usize)));
    // Models are listed by name: test-large:70b (1), test-small:1b (2).
    ui.click_id(("ai-model", 2usize));
    assert_eq!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.deep.clone().unwrap().model), "test-small:1b");
    ui.click_id(("ai-remove", ServiceKind::Custom as usize));
    assert!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.quick.is_none()));
}

#[test]
fn a_keyed_service_from_the_add_menu() {
    let f = fake(vec![http("200 OK", "{}")]);
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    ui.click_id("ai-add-service");
    ui.click_id(("ai-add", ServiceKind::Gemini as usize));
    assert!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.service(ServiceKind::Gemini).is_some()));
    assert!(ui.has_element("ai-paste-key"));
    assert!(ui.has_element(("ai-key-page", ServiceKind::Gemini as usize)), "where to make a key");
    let base = f.base.clone();
    ui.set_prefs(move |p| p.ai.services.iter_mut().find(|s| s.kind == ServiceKind::Gemini).unwrap().base_url = base.clone());
    ui.with_app(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("AIza-test\n".into())));
    ui.click_id("ai-paste-key");
    assert_eq!(ai::secrets::get("gemini").as_deref(), Some("AIza-test"));
    assert!(!ui.has_element(("ai-key-page", ServiceKind::Gemini as usize)), "saved: no link");
    let done = ui.wait_for(5., |ui| ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.quick.as_ref().is_some_and(|m| m.service == ServiceKind::Gemini)));
    assert!(done, "Quick was never filled from Gemini");
    ui.click_id(("ai-remove", ServiceKind::Gemini as usize));
    assert_eq!(ai::secrets::get("gemini"), None);
    assert!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.service(ServiceKind::Gemini).is_none()));
}

#[test]
fn prefs_ai_menu_closes_on_click_outside() {
    use gpui_kit::{point, px, MouseButton};
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    let menu = |ui: &Ui| ui.read(|_, cx| crate::assist::get(cx).and_then(|a| a.menu));
    ui.click_id("ai-add-service");
    assert_eq!(menu(&ui), Some(crate::assist::PrefsMenu::AddService));
    assert!(ui.has_element("ai-add-menu"));
    // Inside the sheet, away from the menu: on the Writing tab, which must not switch.
    ui.click_id("nav-writing");
    assert_eq!(menu(&ui), None);
    assert!(!ui.has_element("ai-add-menu"));
    assert!(!ui.has_element("ai-menu-backdrop"));
    assert!(ui.has_element("ai-add-service"), "still on AI");
    ui.click_id("ai-add-service");
    assert!(ui.has_element("ai-add-menu"));
    ui.click(point(px(4.), px(300.)), MouseButton::Left);
    assert_eq!(menu(&ui), None);
    assert!(ui.read(|ws, _| ws.sheet.is_some()), "the sheet stays");
    ui.click_id("ai-add-service");
    ui.keys("escape");
    assert_eq!(menu(&ui), None);
    assert!(ui.read(|ws, _| ws.sheet.is_some()));
    ui.keys("escape");
    assert!(ui.read(|ws, _| ws.sheet.is_none()));
}

#[test]
fn openrouter_connected_and_disconnected() {
    let mut ui = Ui::open(DOC);
    ui.with_app(|cx| crate::assist::set_key_for_tests(ServiceKind::OpenRouter, "sk-or-test", cx));
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    assert!(!ui.has_element("ai-connect"));
    ui.click_id(("ai-remove", ServiceKind::OpenRouter as usize));
    assert!(ui.has_element("ai-connect"), "back to Connect");
    assert_eq!(ai::secrets::get("openrouter"), None);
}

#[test]
fn openrouter_takes_a_pasted_key() {
    let mut ui = Ui::open(DOC);
    // Nobody answers here: the model list isn't fetched from the real openrouter.ai.
    ui.set_prefs(|p| p.ai.services.iter_mut().find(|s| s.kind == ServiceKind::OpenRouter).unwrap().base_url = "http://127.0.0.1:9/v1".into());
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    ui.with_app(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(" sk-or-v1-review \n".into())));
    ui.click_id("ai-paste-or-key");
    assert!(!ui.has_element("ai-connect"), "connected");
    assert!(!ui.has_element("ai-paste-or-key"));
    assert_eq!(ai::secrets::get("openrouter").as_deref(), Some("sk-or-v1-review"));
}

fn lab_running(ui: &Ui) -> bool {
    ui.read(|ws, _| ws.lab.running.is_some())
}

fn marks(ui: &Ui) -> Vec<String> {
    ui.read(|ws, cx| {
        let text: Vec<char> = ws.model.read(cx).doc.text().chars().collect();
        ws.lab.marks.as_ref().map(|m| m.items.iter().map(|k| text[k.range.clone()].iter().collect()).collect()).unwrap_or_default()
    })
}

fn json_reply(v: serde_json::Value) -> String {
    sse(&[&v.to_string()])
}

/// Opens the Review tab, focused, whatever was open.
fn open_review(ui: &mut Ui) {
    ui.win.update(|ws, w, cx| ws.open_review(w, cx));
    ui.draw();
}

fn focus_page(ui: &mut Ui) {
    ui.win.update(|ws, w, cx| ws.focus_page(w, cx));
}

/// Chooses a pass's row in the tab and presses its Run.
fn run(ui: &mut Ui, pass: ai::lab::Pass) {
    open_review(ui);
    if ui.read(|ws, _| ws.lab.showing_results()) {
        ui.click_id("review-back");
    }
    ui.click_id(("lab-pass", crate::lab::pass_row(pass)));
    ui.click_id("lab-run");
}

/// Trim at the amount on chip `i` (1 is −10%, 4 is −50%).
fn run_trim(ui: &mut Ui, i: usize) {
    open_review(ui);
    if ui.read(|ws, _| ws.lab.showing_results()) {
        ui.click_id("review-back");
    }
    ui.click_id(("lab-trim", i));
    ui.click_id("lab-run");
}

fn inside_panel(ui: &mut Ui, ids: Vec<gpui_kit::ElementId>, at: &str) {
    use gpui_kit::px;
    let panel = ui.element("review").bounds();
    for id in ids {
        let b = ui.element(id.clone()).bounds();
        assert!(b.left() >= panel.left() - px(0.5) && b.right() <= panel.right() + px(0.5), "{at}: {id:?} at {b:?} is outside the panel {panel:?}");
    }
}

#[test]
fn lab_hard_to_read_shows_long_sentences_at_once() {
    let long = format!("This sentence {} goes on.", "and on ".repeat(20));
    let mut ui = Ui::open_editing(&format!("Short one. {long}\n\nAnother short one.\n"));
    connect(&mut ui);
    let before = ui.text();
    ui.keys("alt-cmd-l");
    assert!(ui.read(|ws, _| ws.lab.open) && ui.has_element("review"), "⌥⌘L opens the Review tab");
    assert!(!ui.has_element("lab-run") || ui.read(|ws, _| ws.lab.at) == 0, "Run only in the chosen row");
    ui.click_id(("lab-pass", ai::lab::Pass::HardToRead as usize));
    assert!(!lab_running(&ui) && marks(&ui).is_empty(), "choosing a row runs nothing");
    ui.click_id("lab-run");
    assert_eq!(marks(&ui), std::slice::from_ref(&long));
    assert!(ui.read(|ws, cx| ws.page.read(cx).lab.marks.len()) == 1, "the page draws the mark");
    assert!(ui.has_element("mark-kind") && ui.has_element(("review-row", 0usize)), "the list, not a busy line");
    // The model's half fails on the refused port; Windows retries a refused connection for a
    // couple of seconds, and esc during a request stops it rather than going back.
    assert!(ui.wait_for(10., |ui| !lab_running(ui)), "the model's half has ended");
    ui.keys("escape");
    assert!(!ui.read(|ws, _| ws.lab.showing_results()), "esc goes back to the passes");
    assert_eq!(marks(&ui), std::slice::from_ref(&long), "and keeps the results");
    assert!(ui.has_element(("lab-open", ai::lab::Pass::HardToRead as usize)), "the pass's row leads back to them");
    ui.click_id(("lab-open", ai::lab::Pass::HardToRead as usize));
    ui.click_id("review-close");
    assert!(marks(&ui).is_empty());
    assert!(!ui.read(|ws, _| ws.lab.showing_results()));
    assert_eq!(ui.text(), before);
}

#[test]
fn review_is_a_tab_in_editing() {
    use gpui_kit::px;
    let mut ui = Ui::open(DOC);
    connect(&mut ui);
    ui.keys("cmd-d");
    assert!(!ui.has_element("lab"), "no REVIEW button in the header");
    let mode = ui.element("layer").bounds();
    assert!((mode.center().x - px(590.)).abs() < px(2.), "the mode button keeps the centre: {mode:?}");
    ui.keys("alt-cmd-l");
    let panel = ui.element("review").bounds();
    assert!(panel.left() <= px(0.5) && panel.top() >= px(crate::workspace::HEADER_H - 0.5), "the left panel: {panel:?}");
    assert!(ui.has_element("switch-review") && ui.has_element("switch-alternatives") && ui.has_element("switch-project"));
    assert_eq!(ui.element("layer").bounds(), mode, "the header doesn't move");
    ui.keys("alt-cmd-l");
    assert!(!ui.has_element("review"), "⌥⌘L again closes it");
    ui.keys("alt-cmd-l");
    ui.keys("cmd-d");
    assert!(!ui.has_element("review") && !ui.read(|ws, _| ws.lab.open), "writing closes it");
}

#[test]
fn lab_key_in_writing_switches_to_editing() {
    let mut ui = Ui::open(DOC);
    connect(&mut ui);
    ui.keys("alt-cmd-l");
    assert!(ui.read(|ws, cx| ws.model.read(cx).layer), "editing");
    assert!(ui.has_element("review"), "the tab is open");
}

#[test]
fn lab_marks_wait_while_writing() {
    let long = format!("This sentence {} goes on.", "and on ".repeat(20));
    let mut ui = Ui::open_editing(&format!("Short one. {long}\n\nAnother short one.\n"));
    connect(&mut ui);
    assert!(!ui.has_element("lab-waiting"));
    run(&mut ui, ai::lab::Pass::HardToRead);
    assert_eq!(ui.element("review-count").label(), Some("1 open"), "the tab counts what's open");
    let painted = |ui: &Ui| ui.read(|ws, cx| ws.page.read(cx).lab.marks.len());
    assert_eq!(painted(&ui), 1);

    ui.keys("cmd-d");
    assert_eq!(painted(&ui), 0, "the page shows only the text");
    assert!(!ui.has_element("review"));
    assert!(ui.has_element("lab-waiting"), "the dot says something waits");
    assert_eq!(marks(&ui), std::slice::from_ref(&long), "kept");

    ui.keys("cmd-d");
    assert_eq!(painted(&ui), 1, "back in editing");
    assert!(!ui.has_element("lab-waiting"));
    ui.keys("alt-cmd-l");
    assert!(ui.has_element(("review-row", 0usize)), "the tab opens on the results");
}

#[test]
fn lab_spins_while_a_pass_runs() {
    // A server that takes the connection and never answers: the pass stays running.
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", silent.local_addr().unwrap());
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(lab_running(&ui));
    assert!(ui.has_element("review-busy") && ui.has_element("review-stop"), "the tab spins, with Stop");
    ui.set_prefs(|p| p.effects = crate::prefs::Effects::Off);
    assert!(ui.has_element("review-busy"), "still there with Effects off");
    ui.keys("escape");
    assert!(!lab_running(&ui));
    assert!(!ui.has_element("review-busy"));
    drop(silent);
}

#[test]
fn lab_marks_from_the_model() {
    let reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 4, "quote": "It is, in a way, basically a kind of a one-way street, really.", "note": "Four hedges in nine words."},
        {"sentence": 99, "quote": "not in the text", "note": "dropped: no such sentence"}
    ]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::HardToRead);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["It is, in a way, basically a kind of a one-way street, really."]);
    let req = f.requests.lock().unwrap()[0].clone();
    assert!(req.contains("[4] It is, in a way"), "sentences are numbered: {req}");
    assert!(req.contains("response_format"));
    focus_page(&mut ui);
    ui.caret_at(0);
    ui.type_text("A ");
    assert_eq!(marks(&ui), ["It is, in a way, basically a kind of a one-way street, really."]);
    ui.click_id("review-close");
    assert!(marks(&ui).is_empty());
}

#[test]
fn lab_tone_suggests_alternatives() {
    let marks_reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 3, "quote": "quietly", "note": "too soft for the rest"},
        {"sentence": 4, "quote": "basically", "note": "too casual"}
    ]}));
    // `candidates at once disappear`: the words either side of `quietly` came along.
    // One reply for both asks (they run at once): each word keeps the versions of its sentence.
    let s3 = |w: &str| format!("You pick teh word and the other candidates {w} disappear.");
    let s4 = |w: &str| format!("It is, in a way, {w} a one-way street, really.");
    let ideas = json_reply(serde_json::json!({"versions": [
        {"sentence": s3("silently")},
        {"sentence": s4("silently a kind of")},
        {"sentence": s3("simply")},
        {"sentence": s4("simply")},
        {"sentence": s3("quietly")},
        {"sentence": s4("quietly a kind of")},
        {"sentence": s3("at once")},
        {"sentence": s3("for good")}
    ]}));
    let f = fake(vec![marks_reply, ideas]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    let before = ui.text();
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["quietly", "basically"]);
    assert!(!ui.has_element("mark-kind"), "off-tone words need no kind");
    assert_eq!(ui.element("suggest-all").label(), Some("Suggest alternatives for all 2"), "the head's button says how many");
    ui.click_id("suggest-all");
    assert!(ui.wait_for(5., |ui| !lab_running(ui) && !ui.groups().is_empty()));
    assert_eq!(ui.text(), before, "the originals still show");
    let groups = ui.groups();
    assert_eq!(groups.len(), 2);
    assert_eq!(ui.variants(groups[0].id), ["quietly", "silently", "simply", "at once"], "what changed in each sentence, not the original, three at most");
    assert_eq!(ui.variants(groups[1].id), ["basically a kind of", "silently a kind of", "simply"], "one stretch for all, the widest; “quietly” would echo the paragraph before");
    assert!(marks(&ui).is_empty(), "the marks became groups");
    let reqs = f.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 3, "one pass, one ask per word");
    assert!(reqs[1..].iter().any(|r| r.contains("⟦basically⟧") && r.contains("too casual")), "each ask has its word and note");
    assert!(reqs[1..].iter().all(|r| r.contains("The sentence: ") && r.contains("agreement")), "a word may take the words beside it along");
    ui.keys("cmd-z");
    assert!(ui.groups().is_empty(), "one ⌘Z");
    assert_eq!(ui.text(), before);
}

#[test]
fn lab_suggest_for_one_mark() {
    let marks_reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 3, "quote": "quietly", "note": "too soft"},
        {"sentence": 4, "quote": "basically", "note": "too casual"}
    ]}));
    let ideas = json_reply(serde_json::json!({"versions": [
        {"sentence": "It is, in a way, silently a kind of a one-way street, really."},
        {"sentence": "It is, in a way, simply a kind of a one-way street, really."}
    ]}));
    let f = fake(vec![marks_reply, ideas]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    ui.click_id(("review-row", 1usize));
    ui.click_id("row-suggest");
    assert!(ui.wait_for(5., |ui| !lab_running(ui) && !ui.groups().is_empty()));
    assert_eq!(f.requests.lock().unwrap().len(), 2, "one ask, for the one mark");
    assert_eq!(ui.variants(ui.groups()[0].id)[0], "basically");
    assert_eq!(marks(&ui), ["quietly"], "the other mark stays");
    // The group is the current row: a version tried from the list, then used.
    ui.click_id(("row-variant", 1usize));
    assert!(ui.text().contains("silently a kind of"), "{}", ui.text());
    assert_eq!(ui.element("row-settle").label(), Some("this"), "“silently” · “basically” don't fit beside Settle on at 262 px");
    assert_eq!(ui.element("row-mine").label(), Some("mine"));
    assert_eq!(ui.element("row-keep").label(), Some("Keep all 3"));
    ui.click_id("row-settle");
    assert!(ui.groups().is_empty());
    assert!(ui.text().contains("silently a kind of"));
    assert!(ui.has_element(("review-done", 0usize)), "what was used stays in the list, dimmed");
}

#[test]
fn lab_mistakes_become_alternatives() {
    let reply = json_reply(serde_json::json!({"marks": [{"sentence": 3, "quote": "teh", "fix": "the", "note": "teh → the"}]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Mistakes);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert!(ui.text().contains("You pick the word"), "the fix shows straight away");
    let g = ui.groups()[0].id;
    assert_eq!(ui.variants(g), ["teh", "the"]);
    assert!(ui.read(|ws, cx| ws.model.read(cx).layer), "the layer turns on so the groups show");
    assert!(!ui.has_element("settle-all"), "one fix: no button for all of them");
    assert_eq!(ui.element("row-settle").label(), Some("“the”"));
    assert_eq!(ui.element("row-mine").label(), Some("“teh”"));
    ui.click_id("row-mine");
    assert!(ui.groups().is_empty());
    assert!(ui.text().contains("You pick teh word"), "mine, back");
    assert!(!ui.read(|ws, _| ws.lab.showing_results()), "nothing left: the passes again");
    ui.undo();
    assert!(ui.text().contains("You pick the word") && ui.groups().len() == 1, "one ⌘Z");
}

#[test]
fn lab_fix_buttons_name_only_the_changed_words() {
    // The model quotes a stretch; the row and its buttons are about the one word that goes.
    let reply = json_reply(serde_json::json!({"marks": [{"sentence": 1, "quote": "into an a quiet room", "fix": "into a quiet room", "note": "doubled article"}]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open("She walked into an a quiet room and sat down.\n");
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Mistakes);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert!(ui.text().contains("walked into a quiet room"), "the fix shows");
    assert_eq!(ui.element("row-settle").label(), Some("no “an”"));
    assert_eq!(ui.element("row-mine").label(), Some("“an”"));
    assert_eq!(ui.element("row-keep").label(), Some("Keep both"), "an article, not a typo: it can stay as an alternative");
    let notes = ui.read(|ws, _| ws.lab.made.as_ref().unwrap().notes.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>());
    assert_eq!(notes, ["doubled article"], "the note names the mistake");
    let reqs = f.requests.lock().unwrap().clone();
    assert!(reqs[0].contains("names the mistake in one to four words"), "the prompt asks for a name, not the change");
}

/// Mockup review-settle: Keep both leaves the group in the text with the fix showing.
#[test]
fn review_keep_both_leaves_the_alternatives() {
    let reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 1, "quote": "and lets you", "fix": "and let you", "note": "agreement"},
        {"sentence": 1, "quote": "teh", "fix": "the", "note": "spelling"}
    ]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open("They sit and lets you choose teh word.\n");
    ui.use_dictionary("they sit and lets let you choose the word");
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Mistakes);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(ui.groups().len(), 2);
    assert!(!ui.has_element("keep-all"), "one of the two is a typo: nothing to keep all of");
    // The list starts on the first change in the text: lets → let.
    assert_eq!(ui.element("row-settle").label(), Some("“let”"));
    assert_eq!(ui.element("row-keep").label(), Some("Keep both"));
    ui.click_id("row-keep");
    assert_eq!(ui.groups().len(), 2, "the group stays in the text");
    assert!(ui.text().contains("and let you"), "the fix shows");
    assert_eq!(ui.read(|ws, _| ws.lab.made.as_ref().map(|t| t.groups.len())), Some(1), "and leaves the list");
    assert!(ui.has_element(("review-done", 0usize)), "dimmed, as kept");
    // The typo: settle only.
    assert_eq!(ui.element("row-settle").label(), Some("“the”"));
    assert!(!ui.has_element("row-keep"), "no Keep both for a typo");
    ui.win.update(|ws, w, cx| w.focus(&ws.lab.focus, cx));
    ui.keys("k");
    assert_eq!(ui.read(|ws, _| ws.lab.made.as_ref().map(|t| t.groups.len())), Some(1), "K does nothing on a typo");
    ui.keys("enter");
    assert_eq!(ui.groups().len(), 1, "↩ settles the typo; the kept one stays");
    assert!(ui.text().contains("choose the word"));
}

/// Mockup review-settle: versions keep on ↩; Keep all N for a section; Close leaves the groups.
#[test]
fn review_versions_keep_and_close() {
    let marks_reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 3, "quote": "quietly", "note": "too soft"},
        {"sentence": 4, "quote": "basically", "note": "too casual"}
    ]}));
    let ideas = json_reply(serde_json::json!({"versions": [
        {"sentence": "You pick teh word and the other candidates silently disappear."},
        {"sentence": "It is, in a way, simply a kind of a one-way street, really."},
        {"sentence": "You pick teh word and the other candidates simply disappear."},
        {"sentence": "It is, in a way, silently a kind of a one-way street, really."}
    ]}));
    let f = fake(vec![marks_reply, ideas.clone(), ideas]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    ui.click_id("suggest-all");
    assert!(ui.wait_for(5., |ui| !lab_running(ui) && ui.groups().len() == 2));
    assert_eq!(ui.element("keep-all").label(), Some("Keep all 2"));
    assert_eq!(ui.element("settle-all").label(), Some("chosen"));
    ui.win.update(|ws, w, cx| w.focus(&ws.lab.focus, cx));
    ui.keys("enter");
    assert_eq!(ui.groups().len(), 2, "↩ keeps versions");
    assert_eq!(ui.read(|ws, _| ws.lab.made.as_ref().map(|t| t.groups.len())), Some(1));
    ui.click_id("review-close");
    assert_eq!(ui.groups().len(), 2, "Close leaves the groups and their alternatives");
    assert!(!ui.read(|ws, _| ws.lab.showing_results()), "the list goes");
    assert!(ui.read(|ws, _| ws.lab.made.is_none()));
}

#[test]
fn lab_mistakes_fix_and_rebuild() {
    let doc = "# Night\n\nShe walked to teh door. Hovering over the step, the dark was quiet.\n";
    let marks_reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 3, "quote": "Hovering over the step, the dark was quiet.", "fix": "", "note": "Dangling: the dark doesn't hover."},
        {"sentence": 2, "quote": "teh", "fix": "the", "note": "teh → the"}
    ]}));
    let ideas = sse(&["1. Hovering over the step, she found the dark quiet.\n"]);
    let f = fake(vec![marks_reply, ideas]);
    let mut ui = Ui::open(doc);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Mistakes);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert!(ui.text().contains("walked to the door"), "the fix shows");
    assert_eq!(marks(&ui), ["Hovering over the step, the dark was quiet."], "the sentence to rebuild is marked");
    let seq = |ui: &Ui| ui.read(|ws, _| (ws.lab.seq(), ws.lab.at_mark()));
    assert_eq!(seq(&ui), ((0, 2), false), "on the fix first");
    assert!(ui.has_element("row-settle") && !ui.has_element("row-suggest"));
    assert!(ui.has_element("review-changes") && ui.has_element("review-marked"), "a section each");
    ui.keys("alt-cmd-]");
    assert_eq!(seq(&ui), ((1, 2), true), "then the mark");
    assert!(ui.has_element("row-suggest") && ui.has_element("row-dismiss"));
    ui.click_id("row-suggest");
    assert!(ui.wait_for(5., |ui| !lab_running(ui) && ui.read(|ws, _| ws.lab.marks.is_none())));
    let (groups, fixed, current) = ui.read(|ws, _| {
        let t = ws.lab.made.as_ref().unwrap();
        (t.groups.len(), t.fixed, t.current)
    });
    assert_eq!((groups, fixed, current), (2, 1, 1), "the versions join the fix, the list on them");
    let reqs = f.requests.lock().unwrap().clone();
    assert!(reqs[0].contains("tense the passage is told in") && reqs[0].contains("leave \\\"fix\\\" empty"), "one pass asks for fixes and rebuilds");
    assert!(reqs[1].contains("tense the passage is told in") && reqs[1].contains("Dangling"), "the version asks with the note");
    assert_eq!(ui.element("settle-all").label(), Some("chosen"), "a fix and versions: each settles on what it shows");
    ui.click_id("settle-all");
    assert!(ui.groups().is_empty());
    assert!(ui.text().contains("walked to the door. Hovering over the step, the dark was quiet."), "the original sentence, as nothing else was picked");
}

#[test]
fn lab_keep_all_mine() {
    let reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 3, "quote": "teh", "fix": "the", "note": "teh → the"},
        {"sentence": 4, "quote": "basically", "fix": "basicaly", "note": "spelling"}
    ]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    let before = ui.text();
    run(&mut ui, ai::lab::Pass::Mistakes);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(ui.groups().len(), 2);
    assert_eq!(ui.element("settle-all").label(), Some("fixes"));
    ui.click_id("settle-all-mine");
    assert!(ui.groups().is_empty());
    assert_eq!(ui.text(), before, "every original back");
}

/// CUT-15
#[test]
fn lab_trim_slider_keep_and_make_the_cuts() {
    let reply = json_reply(serde_json::json!({"cuts": [
        {"sentence": 4, "quote": "basically"},
        {"sentence": 4, "quote": "really"},
        {"sentence": 3, "quote": "quietly"},
        {"sentence": 2, "quote": "Most of the"},
        {"sentence": 4, "quote": "in a way,"}
    ]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run_trim(&mut ui, 1);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    let cuts = |ui: &Ui| ui.read(|ws, cx| ws.page.read(cx).lab.cuts.len());
    let percent = |ui: &Ui| ui.read(|ws, _| ws.lab.trim.as_ref().unwrap().percent);
    assert_eq!(percent(&ui), 10);
    assert!(cuts(&ui) >= 1);
    let at_10 = cuts(&ui);
    ui.click_id(("trim-tick", 4usize));
    assert_eq!(percent(&ui), 50);
    assert!(cuts(&ui) > at_10, "more cuts at −50%");
    ui.win.update(|ws, w, cx| w.focus(&ws.lab.focus, cx));
    ui.keys("left");
    assert_eq!(percent(&ui), 30, "← to the level before");
    ui.keys("alt-left alt-left alt-left");
    assert_eq!(percent(&ui), 27, "⌥← a percent at a time");
    ui.keys("right");
    assert_eq!(percent(&ui), 30);
    assert_eq!(f.requests.lock().unwrap().len(), 1, "the slider doesn't ask again");
    let first = ui.read(|ws, cx| ws.page.read(cx).lab.cuts[0].0.clone());
    ui.click(ui.point_for(first.start), gpui_kit::MouseButton::Left);
    assert!(ui.read(|ws, _| ws.lab.trim.as_ref().unwrap().kept.len()) == 1);
    let showing = cuts(&ui);
    let words = ui.read(|ws, _| ws.lab.trim.as_ref().unwrap().words_cut());
    assert_eq!(ui.element("cut-words").label().map(str::to_string), Some(format!("Cut {words} word{}", if words == 1 { "" } else { "s" })));
    ui.click_id("cut-words");
    let ghosts = ui.read(|ws, cx| ws.model.read(cx).doc.ghost_ranges().len());
    assert!(ghosts >= 1 && ghosts <= showing, "{ghosts} ghosts for {showing} cuts");
    assert!(ui.read(|ws, _| ws.lab.trim.is_none()));
    let n = ui.read(|ws, cx| ws.model.read(cx).doc.cuts().len());
    assert_eq!(ui.notes(), [format!("Trim −30% · {n} cut{}", if n == 1 { "" } else { "s" })]);
    ui.keys("ctrl-cmd-c");
    ui.click_note(0);
    assert_eq!(ui.notes().len(), 1 + n, "unfolded: {:?}", ui.notes());
    ui.cut_menu(0, "Put Back");
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.ghost_ranges().len()), 0);
    assert!(ui.notes().is_empty());
    ui.keys("cmd-z");
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.ghost_ranges().len()), ghosts);
    ui.keys("cmd-z");
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.ghost_ranges().len()), 0);
}

#[test]
fn lab_trim_list_keeps_and_cuts() {
    let reply = json_reply(serde_json::json!({"cuts": [{"sentence": 4, "quote": "basically"}, {"sentence": 4, "quote": "really"}, {"sentence": 3, "quote": "quietly"}]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run_trim(&mut ui, 4);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    let state = |ui: &Ui| ui.read(|ws, _| {
        let t = ws.lab.trim.as_ref().unwrap();
        (t.at, t.kept.len(), t.showing().len(), t.list().len())
    });
    assert_eq!(state(&ui), (0, 0, 3, 3), "the list is the walk: every cut, in page order");
    ui.keys("down k");
    assert_eq!(state(&ui), (1, 1, 2, 3), "K keeps the one you're on; it stays in the list");
    ui.keys("k");
    assert_eq!(state(&ui), (1, 0, 3, 3), "and K again cuts it");
    ui.click_id(("review-cut", 2usize));
    assert_eq!(ui.element("cut-keep").label(), Some("Keep it"));
    ui.click_id("cut-keep");
    assert_eq!(state(&ui), (2, 1, 2, 3));
    assert_eq!(ui.element("cut-keep").label(), Some("Cut it"));
}

#[test]
fn review_keys() {
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", silent.local_addr().unwrap());
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &base);
    ui.keys("alt-cmd-l");
    let at = |ui: &Ui| ui.read(|ws, _| (ws.lab.at, ws.lab.trim_pc));
    assert_eq!(at(&ui), (0, 10));
    ui.keys("down down");
    assert_eq!(at(&ui), (2, 10));
    ui.keys("right");
    assert_eq!(at(&ui), (2, 10), "← → only on Trim's row");
    ui.keys("down right right");
    assert_eq!(at(&ui), (3, 30), "Trim's amount");
    assert!(!lab_running(&ui), "choosing runs nothing");
    ui.keys("down");
    assert_eq!(at(&ui), (0, 30), "round to the top");
    ui.keys("escape");
    assert!(!ui.read(|ws, _| ws.lab.open), "esc closes the tab");
    ui.keys("alt-cmd-l");
    ui.keys("2");
    assert!(lab_running(&ui), "a number runs its row");
    assert!(ui.read(|ws, _| ws.lab.showing_results()));
    assert_eq!(ui.read(|ws, _| ws.lab.running.as_ref().map(|r| r.label.clone())).as_deref(), Some("Looking for tangled sentences…"));
    ui.keys("escape");
    assert!(!lab_running(&ui), "esc stops it");
    drop(silent);
}

#[test]
fn each_pass_keeps_its_results() {
    let tone = json_reply(serde_json::json!({"marks": [
        {"sentence": 4, "quote": "basically", "note": "casual"},
        {"sentence": 4, "quote": "really", "note": "casual"}
    ]}));
    let hard = json_reply(serde_json::json!({"marks": [{"sentence": 4, "quote": "It is, in a way, basically a kind of a one-way street, really.", "note": "Hedges."}]}));
    let f = fake(vec![tone.clone(), hard, tone]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    run(&mut ui, ai::lab::Pass::HardToRead);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["It is, in a way, basically a kind of a one-way street, really."], "the page shows the pass you're in");
    assert_eq!(ui.read(|ws, _| ws.lab.open_total()), 3, "and the tab counts all of them");
    ui.click_id("review-back");
    assert_eq!(ui.element(("lab-open", ai::lab::Pass::Tone as usize)).label(), None);
    assert!(ui.has_element(("lab-open", ai::lab::Pass::Tone as usize)) && ui.has_element(("lab-open", ai::lab::Pass::HardToRead as usize)));
    ui.click_id(("lab-open", ai::lab::Pass::Tone as usize));
    assert_eq!(marks(&ui), ["basically", "really"], "Word choice's, as they were left");
    ui.click_id("row-dismiss");
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["basically", "really"], "Run again starts its own list again");
    assert_eq!(ui.read(|ws, _| ws.lab.open_in(crate::lab::pass_row(ai::lab::Pass::HardToRead))), 1, "the other pass's are untouched");
}

#[test]
fn lab_pass_survives_typing() {
    let reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 4, "quote": "basically", "note": "casual"},
        {"sentence": 4, "quote": "really", "note": "casual"}
    ]}));
    let f = fake_slow(vec![reply], 700);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(lab_running(&ui));
    focus_page(&mut ui);
    let at = ui.find("Most").start;
    ui.caret_at(at);
    ui.type_text("Truly. ");
    assert!(lab_running(&ui), "typing doesn't cancel it");
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["basically", "really"], "the marks moved with their words");
}

#[test]
fn lab_mistakes_survive_typing() {
    let reply = json_reply(serde_json::json!({"marks": [{"sentence": 3, "quote": "teh", "fix": "the", "note": "teh → the"}]}));
    let f = fake_slow(vec![reply], 700);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Mistakes);
    focus_page(&mut ui);
    let at = ui.find("Most").start;
    ui.caret_at(at);
    ui.type_text("Truly. ");
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    let text = ui.text();
    assert!(text.contains("Truly. Most") && text.contains("You pick the word"), "{text}");
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "no word tagger: dsky-fbso.18")]
fn com_10_keep_as_comment() {
    let doc = "# Keep Them\n\nMost of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear. A week later you can’t remember those words, only that one of them was better.\n";
    let mut ui = Ui::open(doc);
    ui.keys("alt-cmd-l");
    ui.keys("3");
    assert_eq!(marks(&ui), ["words"], "the echo, found on the Mac");
    let note = ui.read(|ws, _| ws.lab.marks.as_ref().unwrap().items[0].note.clone());
    ui.click_id("row-comment");
    let kept = ui.read(|ws, cx| {
        let d = &ws.model.read(cx).doc;
        d.comments().into_iter().map(|c| (d.text_range(c.range), c.text)).collect::<Vec<_>>()
    });
    assert_eq!(kept, [("words".to_string(), note)]);
    assert!(ui.read(|ws, _| ws.lab.marks.is_none()), "the mark went");
    ui.undo();
    assert!(ui.read(|ws, cx| !ws.model.read(cx).doc.has_comments()), "⌘Z takes it back");
}

#[test]
fn lab_dismiss_one_and_all() {
    let reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 4, "quote": "basically", "note": "casual"},
        {"sentence": 4, "quote": "really", "note": "casual"},
        {"sentence": 3, "quote": "quietly", "note": "soft"}
    ]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["quietly", "basically", "really"]);
    ui.click_id("row-dismiss");
    assert_eq!(marks(&ui), ["basically", "really"]);
    assert!(ui.has_element(("review-done", 0usize)), "dismissed, dimmed in the list");
    ui.click_id("dismiss-all");
    assert!(marks(&ui).is_empty());
    assert!(!ui.read(|ws, _| ws.lab.showing_results()), "nothing left: the passes again");
}

#[test]
fn review_without_a_model() {
    let long = format!("This sentence {} goes on.", "and on ".repeat(20));
    let mut ui = Ui::open(&format!("Short one. {long}\n\nAnother short one.\n"));
    assert_eq!(ui.read(|_, cx| crate::lab::AiState::of(cx)), crate::lab::AiState::NoModel);
    ui.keys("alt-cmd-l");
    assert!(ui.read(|ws, cx| ws.model.read(cx).layer), "editing");
    assert!(ui.has_element("review"), "the tab");
    assert_eq!(ui.read(|ws, _| ws.lab.at), 1, "the first row that runs is chosen");
    assert!(ui.has_element("lab-ai-settings"));
    ui.click_id(("lab-pass", ai::lab::Pass::Mistakes as usize));
    assert_eq!(ui.read(|ws, _| ws.lab.at), 1, "a faded row can't be chosen");
    ui.keys("down");
    assert_eq!(ui.read(|ws, _| ws.lab.at), 2, "↓ goes to Word choice");
    ui.keys("down");
    assert_eq!(ui.read(|ws, _| ws.lab.at), 1, "and round, skipping Trim and Fix mistakes");
    ui.keys("2");
    assert_eq!(marks(&ui), std::slice::from_ref(&long), "the long sentence, found on the Mac");
    assert!(!lab_running(&ui), "nothing asked");
    assert!(ui.has_element(("review-row", 0usize)), "the list");
    assert!(ui.has_element("row-suggest-off"), "Suggest versions, greyed: it needs a model");

    ui.click_id("review-back");
    ui.click_id("lab-ai-settings");
    assert_eq!(ui.sheet(), Some("prefs"), "it opens Preferences → AI");
}

#[test]
fn lab_without_a_deep_model() {
    let mut ui = Ui::open(DOC);
    connect(&mut ui);
    ui.with_app(|cx| crate::sheets::set_prefs(cx, |p| p.ai.deep = None));
    run(&mut ui, ai::lab::Pass::Tone);
    assert_eq!(ui.read(|ws, _| ws.lab.error.clone().map(|e| e.0)).as_deref(), Some("No AI is set up yet."));
    assert!(ui.has_element("review-retry"));
    ui.click_id("lab-setup");
    assert_eq!(ui.sheet(), Some("prefs"));
}

#[test]
fn review_and_alternatives_share_the_left_panel() {
    let long = format!("This sentence {} goes on.", "and on ".repeat(20));
    let doc = format!("Short one. {long}\n\nAnother short one.\n");
    let mut ui = Ui::open_sized(&doc, 1440., 900.);
    ui.action(crate::actions::ToggleLayer);
    connect(&mut ui);
    run(&mut ui, ai::lab::Pass::HardToRead);
    let panel = ui.element("review").bounds();
    ui.select_text("Another");
    ui.keys("cmd-shift-a");
    assert!(ui.sidebar_open() && !ui.has_element("review"), "Alternatives takes the panel");
    assert_eq!(marks(&ui), std::slice::from_ref(&long), "the results stay");
    ui.click_id("switch-review");
    assert!(!ui.sidebar_open() && ui.has_element(("review-row", 0usize)), "and Review takes it back, on the results");
    assert_eq!(ui.element("review").bounds(), panel);
    ui.action(crate::actions::ToggleOverflow);
    assert!(ui.drawer_open());
    assert_eq!(ui.element("review").bounds(), panel, "the Drawer doesn't move it");
    ui.click_id("panel");
    assert!(!ui.has_element("review"));
    ui.click_id("panel");
    assert!(ui.has_element("review"), "the panel button brings back the tab it showed");
}

#[test]
fn review_fits_its_panel() {
    use gpui_kit::ElementId;
    for (w, h, drawer) in [(520., 700., false), (900., 860., true), (1180., 860., false)] {
        let at = format!("{w}×{h}{}", if drawer { " + Drawer" } else { "" });
        let marks_reply = json_reply(serde_json::json!({"marks": [
            {"sentence": 3, "quote": "quietly", "note": "Too soft for the rest of the paragraph, which is plain and direct and doesn't hedge anywhere else."},
            {"sentence": 4, "quote": "basically", "note": "too casual"}
        ]}));
        let versions: Vec<serde_json::Value> = ["silently", "simply", "at once"]
            .iter()
            .flat_map(|t| {
                [
                    serde_json::json!({"sentence": format!("You pick teh word and the other candidates {t} disappear.")}),
                    serde_json::json!({"sentence": format!("It is, in a way, {t} a kind of a one-way street, really.")}),
                ]
            })
            .collect();
        let ideas = json_reply(serde_json::json!({ "versions": versions }));
        let cuts = json_reply(serde_json::json!({"cuts": [{"sentence": 4, "quote": "It is, in a way, basically a kind of a one-way street"}, {"sentence": 4, "quote": "really"}, {"sentence": 3, "quote": "quietly"}]}));
        let f = fake(vec![marks_reply, ideas.clone(), ideas, cuts]);
        let mut ui = Ui::open_sized(DOC, w, h);
        use_fake(&mut ui, &f.base);
        if drawer {
            ui.action(crate::actions::ToggleOverflow);
            assert!(ui.drawer_open());
        }
        ui.keys("alt-cmd-l");
        let ids: Vec<ElementId> = ["switch-project", "switch-alternatives", "switch-review", "lab-run", "lab-scope"].into_iter().map(Into::into).collect();
        inside_panel(&mut ui, ids, &format!("{at}, passes"));
        run(&mut ui, ai::lab::Pass::Tone);
        assert!(ui.wait_for(5., |ui| !lab_running(ui)), "{at}");
        let mut ids: Vec<ElementId> = ["review-back", "review-title", "review-sum", "suggest-all", "dismiss-all", "row-suggest", "row-comment", "row-dismiss", "review-close"].into_iter().map(Into::into).collect();
        ids.push(("review-row", 0usize).into());
        inside_panel(&mut ui, ids, &format!("{at}, marks"));

        ui.click_id("suggest-all");
        assert!(ui.wait_for(5., |ui| !lab_running(ui) && ui.read(|ws, _| ws.lab.made.is_some())), "{at}");
        ui.click_id(("row-variant", 1usize));
        let ids: Vec<ElementId> = ["settle-all", "settle-all-mine", "keep-all", "row-settle", "row-mine", "row-keep"].into_iter().map(Into::into).collect();
        inside_panel(&mut ui, ids, &format!("{at}, made"));

        run_trim(&mut ui, 4);
        assert!(ui.wait_for(5., |ui| !lab_running(ui) && ui.read(|ws, _| ws.lab.trim.is_some())), "{at}");
        let mut ids: Vec<ElementId> = ["review-sum", "trim-slider", "trim-track", "cut-words", "cut-keep"].into_iter().map(Into::into).collect();
        ids.extend((0..ai::trim::LEVELS.len()).map(|i| ElementId::from(("trim-tick", i))));
        ids.push(("review-cut", 0usize).into());
        inside_panel(&mut ui, ids, &format!("{at}, trim"));
    }
}

#[test]
fn review_list_steps_with_the_page() {
    let reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 3, "quote": "quietly", "note": "too soft"},
        {"sentence": 4, "quote": "basically", "note": "too casual"}
    ]}));
    let f = fake(vec![reply]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    let current = |ui: &Ui| ui.read(|ws, _| ws.lab.marks.as_ref().unwrap().current);
    let painted = |ui: &Ui| ui.read(|ws, cx| ws.page.read(cx).lab.marks.iter().position(|m| m.current));
    assert_eq!((current(&ui), painted(&ui)), (0, Some(0)));
    ui.click_id(("review-row", 1usize));
    assert_eq!((current(&ui), painted(&ui)), (1, Some(1)), "a row goes there, and the page follows");
    ui.keys("down");
    assert_eq!(current(&ui), 0, "↓ wraps");
    ui.keys("up");
    assert_eq!(current(&ui), 1);
    ui.keys("alt-cmd-[");
    assert_eq!(current(&ui), 0, "⌥⌘[ ] step it too");
}

#[test]
fn question_marks_with_ai_off() {
    let mut ui = Ui::open(DOC);
    ui.with_app(|cx| crate::assist::set_on(false, cx));
    ui.select_text("friction");
    ui.action(crate::actions::AlternativesForSelection);
    assert!(ui.sidebar().is_some());
    ui.type_text("??");
    ui.keys("enter");
    assert!(ui.read(|ws, _| ws.ideas.is_none()), "no ideas asked for");
    assert_eq!(ui.status().as_deref(), Some("AI is off (Preferences → AI)"));
    assert_eq!(ui.variants(ui.sidebar().unwrap()), ["friction"], "?? isn't added as a version");
}


#[test]
fn ideas_button_by_the_add_field() {
    let f = fake(vec![sse(&["tension\ndrag\nresistance\nstrain\npull\n"])]);
    let mut ui = Ui::open(DOC);
    use_fake(&mut ui, &f.base);
    ui.select_text("friction");
    ui.action(crate::actions::AlternativesForSelection);
    let g = ui.sidebar().unwrap();
    ui.click_id("ideas-button");
    assert!(ui.wait_for(5., |ui| !ideas_running(ui) && ui.variants(g).len() == 6), "{:?}", ui.variants(g));
    ui.with_app(|cx| crate::assist::set_on(false, cx));
    ui.draw();
    assert!(!ui.has_element("ideas-button"), "no AI, no button");
}


#[test]
fn old_prefs_keep_ai_on() {
    let p: crate::prefs::Prefs = serde_json::from_str(r#"{"ai": {"services": []}}"#).unwrap();
    assert!(p.ai.enabled);
}


#[test]
fn the_ai_switch_turns_ai_off() {
    let long = format!("This sentence {} goes on.", "and on ".repeat(20));
    let mut ui = Ui::open_editing(&format!("Short one. {long}\n\nAnother short one.\n"));
    connect(&mut ui);
    run(&mut ui, ai::lab::Pass::HardToRead);
    assert_eq!(marks(&ui).len(), 1);

    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    assert!(ui.has_element("ai-quick"));
    ui.click_id("ai-on");
    assert!(!ui.read(|_, cx| cx.global::<crate::prefs::Prefs>().ai.enabled));
    assert!(!ui.has_element("ai-quick") && !ui.has_element("ai-add-service"), "only the switch is left");
    ui.keys("escape");
    assert_eq!(ui.sheet(), None);

    assert!(ui.has_element("review"), "Review stays: it finds what the Mac can");
    assert!(marks(&ui).is_empty(), "the marks go with it");
    assert!(ui.read(|ws, cx| ws.page.read(cx).lab.marks.is_empty()));
    open_review(&mut ui);
    assert!(ui.has_element("review-passes"));
    assert!(ui.has_element(("lab-pass", ai::lab::Pass::HardToRead as usize)) && ui.has_element(("lab-pass", ai::lab::Pass::Tone as usize)));
    assert!(!ui.has_element(("lab-pass", ai::lab::Pass::Mistakes as usize)) && !ui.has_element("lab-trim-row"), "only what runs on the Mac");
    assert!(!ui.has_element("lab-ai-settings"), "nothing about AI");
    ui.keys("1");
    assert_eq!(marks(&ui).len(), 1, "Hard to read's long sentence, with no model");
    assert!(!ui.has_element("row-suggest") && !ui.has_element("row-suggest-off"), "no Suggest with AI off");
    ui.keys("escape");

    ui.select_text("Short");
    ui.keys("alt-cmd-a");
    assert!(!ui.sidebar_open(), "⌥⌘A makes no group");
    assert_eq!(ui.status().as_deref(), Some("AI is off (Preferences → AI)"));
    ui.right_click_text("Short");
    assert!(!ui.menu_labels().iter().any(|l| l.starts_with("AI ")), "{:?}", ui.menu_labels());
    ui.keys("escape");

    let alternatives = |ai: bool| {
        let p = crate::prefs::Prefs { ai: crate::prefs::AiPrefs { enabled: ai, ..Default::default() }, ..Default::default() };
        let menus = crate::actions::menus(&[], &p, None, false);
        let m = menus.into_iter().find(|m| m.name.as_ref() == "Alternatives").unwrap();
        m.items
            .into_iter()
            .filter_map(|it| match it {
                gpui_kit::MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let (on, off) = (alternatives(true), alternatives(false));
    assert!(on.iter().any(|n| n == "Review…") && on.iter().any(|n| n.starts_with("AI ")));
    assert!(off.iter().any(|n| n == "Review…") && !off.iter().any(|n| n.starts_with("AI ")), "Review stays, AI goes: {off:?}");

    ui.with_app(|cx| crate::assist::set_on(true, cx));
    open_review(&mut ui);
    assert!(ui.has_element(("lab-pass", ai::lab::Pass::Mistakes as usize)), "on again, every pass");
}


#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "no word tagger: dsky-fbso.18")]
fn lab_hard_to_read_marks_passive_sentences() {
    let doc = "# Editors\n\nEditors have always known this. Your sentence isn’t erased by a good editor. They write a suggestion in the margin and let you decide, and the page holds both versions until you are ready.\n";
    let f = fake_slow(vec![json_reply(serde_json::json!({"marks": []}))], 300);
    let mut ui = Ui::open(doc);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::HardToRead);
    assert_eq!(marks(&ui), ["Your sentence isn’t erased by a good editor."], "found at once, before the model answers");
    assert_eq!(ui.read(|ws, _| ws.lab.marks.as_ref().unwrap().items[0].found), ai::lab::Found::Passive);
    assert!(ui.has_element("mark-kind"));
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui).len(), 1, "the passive mark stays when the model finds nothing");
    assert!(ai::lab::Pass::HardToRead.steer("", ai::lab::Found::Passive).contains("active voice"));

    let ru = "Мы долго шли молча и не говорили ни слова. Письмо было написано ещё вчера. Я держал его в кармане и не знал, что с ним делать.";
    let sentences = ai::segment::sentences(ru, 0..ru.chars().count());
    let found = crate::lab::passive_sentences(ru, &sentences, &[]);
    let quotes: Vec<String> = found.iter().map(|m| ru.chars().skip(m.range.start).take(m.range.len()).collect()).collect();
    // macOS 15 (the CI runners) can't tag Russian: nothing is found there, nothing wrong is found.
    let want: &[&str] = if crate::tagger::reads(prose::Lang::Ru, ru) { &["Письмо было написано ещё вчера."] } else { &[] };
    assert_eq!(quotes, want);

    let fr = "Nous avons marché longtemps sans rien dire. La lettre était écrite depuis la veille, et il est allé la poster.";
    let sentences = ai::segment::sentences(fr, 0..fr.chars().count());
    assert!(crate::lab::passive_sentences(fr, &sentences, &[]).is_empty(), "no rule for French");
}


#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "no word tagger: dsky-fbso.18")]
fn lab_word_choice_marks_echoes() {
    assert_eq!(ai::lab::Pass::Tone.menu_label(), "Word choice");
    let doc = "# Keep Them\n\nMost of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear. A week later you can’t remember those words, only that one of them was better. At the end of the day you keep them, and keep them close.\n";
    let reply = json_reply(serde_json::json!({"marks": [{"sentence": 5, "quote": "At the end of the day", "note": "A cliché."}]}));
    let f = fake_slow(vec![reply], 300);
    let mut ui = Ui::open(doc);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert_eq!(marks(&ui), ["words"], "the echo at once; \"keep\" is in the title");
    let (found, note) = ui.read(|ws, _| {
        let k = &ws.lab.marks.as_ref().unwrap().items[0];
        (k.found, k.note.clone())
    });
    assert_eq!(found, ai::lab::Found::Echo);
    assert_eq!(note, "“words” again, 19 words after “word”.");
    assert!(ui.has_element("mark-kind"), "the list says echo");
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["words", "At the end of the day"], "the model's mark joins, in page order");
    assert!(ai::lab::Pass::Tone.steer("“words” again", ai::lab::Found::Echo).contains("repetition"));
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "no word tagger: dsky-fbso.18")]
fn an_echos_versions_leave_no_echo() {
    let doc = "# Keep Them\n\nMost of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear. A week later you can’t remember those words, only that one of them was better.\n";
    let versions = json_reply(serde_json::json!({"versions": [
        {"sentence": "A week later you can’t remember those word choices, only that one of them was better."},
        {"sentence": "A week later you can’t remember those candidates, only that one of them was better."},
        {"sentence": "A week later you can’t remember them, only that one of them was better."},
        {"sentence": "A week later you can’t remember the ones you lost, only that one of them was better."}
    ]}));
    let f = fake(vec![json_reply(serde_json::json!({"marks": []})), versions]);
    let mut ui = Ui::open(doc);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::Tone);
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["words"]);
    ui.click_id(("review-row", 0usize));
    ui.click_id("row-suggest");
    assert!(ui.wait_for(5., |ui| !lab_running(ui) && !ui.groups().is_empty()));
    let g = ui.groups()[0].id;
    assert_eq!(ui.variants(g), ["those words", "them", "the ones you lost"], "“word choices” still repeats “word”, “candidates” repeats the sentence before");
    let reqs = f.requests.lock().unwrap().clone();
    assert!(reqs[1].contains("They repeat \\\"word\\\"") && reqs[1].contains("You pick a word"), "the model sees the word repeated and where: {}", reqs[1]);
}


#[test]
fn lab_hard_to_read_joins_tangled_sentences() {
    let long = format!("This sentence {} goes on.", "and on ".repeat(20));
    let doc = format!("# T\n\nThe thing that the man who the dog bit saw ran. {long}\n");
    let reply = json_reply(serde_json::json!({"marks": [
        {"sentence": 2, "quote": "The thing that the man who the dog bit saw ran.", "note": "Nested clauses."},
        {"sentence": 3, "quote": "This sentence and on", "note": "also long"}
    ]}));
    let f = fake_slow(vec![reply], 500);
    let mut ui = Ui::open(&doc);
    use_fake(&mut ui, &f.base);
    run(&mut ui, ai::lab::Pass::HardToRead);
    assert_eq!(marks(&ui), std::slice::from_ref(&long), "the long one at once");
    assert!(lab_running(&ui) && ui.has_element("review-busy") && ui.has_element(("review-row", 0usize)), "looking, beside the list");
    assert!(ui.wait_for(5., |ui| !lab_running(ui)));
    assert_eq!(marks(&ui), ["The thing that the man who the dog bit saw ran.".to_string(), long.clone()]);
    let (current, long_flags) = ui.read(|ws, _| {
        let m = ws.lab.marks.as_ref().unwrap();
        (m.current, m.items.iter().map(|k| k.found == ai::lab::Found::Long).collect::<Vec<_>>())
    });
    assert_eq!(current, 1, "still on the long one");
    assert_eq!(long_flags, [false, true], "the long one keeps its count");
    let req = f.requests.lock().unwrap()[0].clone();
    assert!(req.contains("Sentences over 32 words"), "the model isn't asked about length: {req}");
}


#[test]
fn prefs_model_menu_search() {
    let f = fake(vec![http("200 OK", "{}")]);
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    let base = f.base.clone();
    ui.with_app(|cx| {
        crate::assist::add_service(ServiceKind::Custom, cx);
        crate::sheets::set_prefs(cx, |p| p.ai.services.iter_mut().find(|s| s.kind == ServiceKind::Custom).unwrap().base_url = base.clone());
        crate::assist::refresh_models(ServiceKind::Custom, cx);
    });
    assert!(ui.wait_for(5., |ui| ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.quick.is_some())));
    let menu = |ui: &Ui| ui.read(|_, cx| crate::assist::get(cx).and_then(|a| a.menu));
    let deep = |ui: &Ui| ui.read(|_, cx| crate::prefs::Prefs::get(cx).ai.deep.clone().unwrap().model);
    let sheet = |ui: &Ui| ui.read(|ws, _| ws.sheet.is_some());

    // The menu opens with the caret in its field; typing narrows it, ↩ picks the first.
    ui.click_id("ai-deep");
    assert!(ui.has_element("ai-model-search"));
    ui.type_text("LARGE");
    assert!(ui.has_element(("ai-model", 1usize)) && !ui.has_element(("ai-model", 2usize)), "only test-large:70b is left");
    ui.keys("enter");
    assert_eq!(deep(&ui), "test-large:70b");
    assert_eq!(menu(&ui), None);
    assert!(!ui.has_element("ai-model-search"));

    // Nothing found; esc empties the field, a second esc closes the menu, a third the sheet.
    ui.click_id("ai-deep");
    ui.type_text("nothing");
    assert!(!ui.has_element(("ai-model", 1usize)));
    ui.keys("escape");
    assert!(ui.has_element(("ai-model", 2usize)), "the whole list is back");
    assert!(menu(&ui).is_some());
    ui.keys("escape");
    assert_eq!(menu(&ui), None);
    assert!(sheet(&ui));
    ui.keys("escape");
    assert!(!sheet(&ui));

    // ↓↓ ↩ with an empty field walks the list.
    ui.keys("cmd-,");
    ui.click_id("nav-ai");
    ui.click_id("ai-deep");
    ui.keys("down down enter");
    assert_eq!(deep(&ui), "test-small:1b");

    // Words in any order, against the id.
    ui.click_id("ai-deep");
    ui.type_text("1b test");
    assert!(ui.has_element(("ai-model", 1usize)) && !ui.has_element(("ai-model", 2usize)));
    ui.keys("escape escape");
    assert_eq!(menu(&ui), None);
}

/// ALT-29
#[test]
fn an_idea_can_be_no_word() {
    let f = fake(vec![sse(&["∅\nwhole\n", "∅\nnext\nrest of the\nnew\n"])]);
    let mut ui = Ui::open("You pick a word, the the sentence moves on.\n");
    use_fake(&mut ui, &f.base);
    let at = ui.text().match_indices("the").nth(1).unwrap().0;
    ui.select(at..at + 3);
    ui.keys("alt-cmd-a");
    let g = ui.sidebar().unwrap();
    assert!(ui.wait_for(5., |ui| !ideas_running(ui)), "ideas never finished");
    let info = ui.group(g);
    let texts: Vec<(&str, bool)> = info.variants.iter().map(|v| (v.text.as_str(), v.cut)).collect();
    assert_eq!(texts, [("the", false), ("the", true), ("whole", false), ("next", false), ("rest of the", false), ("new", false)], "∅ is the version without it, once");
    assert_eq!(info.active, 0, "nothing is chosen for you");
    assert!(f.requests.lock().unwrap()[0].contains("may be just ∅"));
    ui.keys("cmd-z");
    assert_eq!(ui.variants(g), ["the"], "one ⌘Z removes the batch, the no-word one too");
}
