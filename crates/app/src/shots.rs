//! Scripted runs (`--features shots`): `DOSTOEVSKY_SCRIPT=steps.jsonl` runs one JSON step per
//! line against the first window, then quits. A failed `expect` exits 1; the last stderr line
//! is `shots: done` when every step passed (`open` drops the exit status). The steps may be
//! inline as `hex:<UTF-8 in hex>`: a sandboxed app can't read the file, `open --env` drops spaces.
//! Places are named by text (first occurrence) or by group (page order), never by pixels:
//!
//! ```text
//! {"type": "hello"}                       insert text at the caret
//! {"comment": "Pick one."}               type into the comment being written (⌥⌘M first)
//! {"field": "?? "}                      the Alternatives panel's add field holds this, focused
//! {"select": "friction"}                  select text; or [3, 9] (display chars), or
//! {"select": {"text": "Editors", "at": "start"}}   a caret at its start / end
//! {"alt": "friction", "scope": "word", "variants": ["tension"]}   make a group directly
//! {"keys": "cmd-shift-a down"}            dispatch keystrokes
//! {"action": "page::GhostToggle"}
//! {"hover": {"group": 0}}                 move the pointer over a group, or {"text": "…"}
//! {"click": {"text": "suggestion"}, "count": 2, "button": "right"}   or [x, y]
//! {"move": [640, 300]}                    move the pointer (window coordinates)
//! {"drag_note": 0, "to": {"text": "Editors"}, "alt": false}   drag a cut from the panel onto the page
//! {"cuts_scope": "book"}                  the Cuts panel's scope: "page" or "book"
//! {"drag": {"text": "a suggestion"}, "to": {"text": "You"}, "hold": true}   press and drag
//!                                         (selected text); `hold` keeps the button down
//! {"release": [640, 300]}                 let go of a held drag there
//! {"drag": {"tab": "alternatives"}, "to": {"tab": "comments", "at": "start"}}   a panel tab
//!                                         to a header (`at` start: before that tab)
//! {"first_open": "confirm"}               the first-open sheet's “Open as …”
//! {"new_project_place": "target/x"}      where New Project makes the folder
//! {"expect": {"text_contains": "…", "group": {"index": 0, "active": 1}, …}}   see `expect`
//! {"wait": 400}                           milliseconds
//! {"wait_for": {"text_contains": "…"}, "timeout": 5000}   poll an `expect` until it holds;
//!                                         fails like `expect` once the timeout (ms) is up
//! {"say": "ready"}                        print `shots: say ready` on stderr (for a driver)
//! {"prefs": {"autosave": "off"}}          change settings for this run (not saved)
//! {"export": {"format": "docx", "style": "book", "whole_book": true, "text_open": true, "page": 2}}
//!                                          set the open Export sheet's choices
//! {"shot": "target/x.png"}                  render the window to a PNG
//! {"shot": "target/x.png", "window": "about"}   render the About window instead (app::About first)
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::cell::RefCell;
use std::time::{Duration, Instant};

use gpui_kit::*;
use serde_json::Value;

use crate::workspace::Workspace;

static FAILED: AtomicBool = AtomicBool::new(false);

pub fn run(ws: WeakEntity<Workspace>, window: AnyWindowHandle, cx: &mut App) {
    let Ok(path) = std::env::var("DOSTOEVSKY_SCRIPT") else { return };
    let script = if let Some(hex) = path.strip_prefix("hex:") {
        let bytes: Option<Vec<u8>> =
            (0..hex.len()).step_by(2).map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok())).collect();
        match bytes.and_then(|b| String::from_utf8(b).ok()) {
            Some(s) => s,
            None => {
                eprintln!("shots: DOSTOEVSKY_SCRIPT: bad hex");
                return;
            }
        }
    } else {
        match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("shots: cannot read {path}: {e}");
                return;
            }
        }
    };
    let steps: Vec<Value> = script
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .filter_map(|l| serde_json::from_str(l).map_err(|e| eprintln!("shots: {e}: {l}")).ok())
        .collect();
    #[cfg(target_os = "macos")]
    let _ = window.update(cx, |_, window, _| crate::platform::ignore_real_pointer(window));
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(700)).await;
        for step in steps {
            let wait = step.get("wait").and_then(Value::as_u64).unwrap_or(0);
            if wait > 0 {
                cx.background_executor().timer(Duration::from_millis(wait)).await;
                continue;
            }
            if let Some(cond) = step.get("wait_for") {
                let timeout = Duration::from_millis(step.get("timeout").and_then(Value::as_u64).unwrap_or(5000));
                let start = Instant::now();
                loop {
                    let ws = ws.clone();
                    let r = window.update(cx, |_, window, cx| {
                        window.refresh();
                        let _ = window.draw(cx);
                        ws.upgrade().map(|ws| expect(cond, &ws, cx))
                    });
                    let Ok(Some(errors)) = r else { break };
                    if errors.is_empty() {
                        break;
                    }
                    if start.elapsed() > timeout {
                        for e in errors {
                            fail(format!("after {} ms: {e}", timeout.as_millis()));
                        }
                        break;
                    }
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                }
                continue;
            }
            let ws = ws.clone();
            let r = window.update(cx, |_, window, cx| step_once(&step, &ws, window, cx));
            if !matches!(r, Ok(true)) {
                break;
            }
            // Let the frame settle before the next step.
            cx.background_executor().timer(Duration::from_millis(120)).await;
        }
        if FAILED.load(Ordering::Relaxed) {
            std::process::exit(1);
        }
        eprintln!("shots: done");
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn fail(what: impl std::fmt::Display) {
    eprintln!("shots: expect failed: {what}");
    FAILED.store(true, Ordering::Relaxed);
}

fn point_of(v: &Value) -> Option<Point<Pixels>> {
    let a = v.as_array()?;
    Some(point(px(a.first()?.as_f64()? as f32), px(a.get(1)?.as_f64()? as f32)))
}

/// Display range of the first occurrence of `needle` on the page.
fn find(ws: &Entity<Workspace>, needle: &str, cx: &App) -> Option<std::ops::Range<usize>> {
    let text = ws.read(cx).model.read(cx).doc.text();
    let byte = text.find(needle)?;
    let start = text[..byte].chars().count();
    Some(start..start + needle.chars().count())
}

/// A display range named by `[a, b]`, `"text"` or `{"text": …, "at": "start" | "end"}`.
fn range_of(v: &Value, ws: &Entity<Workspace>, cx: &App) -> Option<std::ops::Range<usize>> {
    if let Some(a) = v.as_array() {
        let n = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(0) as usize;
        return Some(n(0)..n(1));
    }
    let s = v.as_str().or_else(|| v.get("text")?.as_str())?;
    let Some(r) = find(ws, s, cx) else {
        fail(format!("{s:?} is not on the page"));
        return None;
    };
    Some(match v.get("at").and_then(Value::as_str) {
        Some("start") => r.start..r.start,
        Some("end") => r.end..r.end,
        _ => r,
    })
}

/// A window position named by `[x, y]`, `{"group": n}` or `{"text": …}`: inside the glyph
/// cell at the middle of the group or text.
fn target(v: &Value, ws: &Entity<Workspace>, cx: &App) -> Option<Point<Pixels>> {
    if let Some(p) = point_of(v) {
        return Some(p);
    }
    // A panel tab's label; `"at": "start"` aims just inside its left edge (a drop lands before it).
    if let Some(name) = v.get("tab").and_then(Value::as_str) {
        let Some(b) = crate::panels::PanelTab::named(name).and_then(|t| ws.read(cx).tab_geometry.borrow().label(t)) else {
            fail(format!("no {name:?} tab on screen"));
            return None;
        };
        return Some(match v.get("at").and_then(Value::as_str) {
            Some("start") => point(b.left() + px(3.), b.center().y),
            _ => b.center(),
        });
    }
    let r = if let Some(n) = v.get("group").and_then(Value::as_u64) {
        let groups = ws.read(cx).model.read(cx).doc.visible_groups();
        match groups.get(n as usize) {
            Some(g) => g.range.clone(),
            None => {
                fail(format!("no group {n} (there are {})", groups.len()));
                return None;
            }
        }
    } else {
        range_of(v, ws, cx)?
    };
    // A target that isn't on the page fails the run: skipping it would let the next steps
    // act on whatever is under the old pointer position.
    let Some(p) = ws.read(cx).page.read(cx).layout.as_ref().and_then(|l| Some((l.point_for((r.start + r.end) / 2)?, l.line_height()))) else {
        fail(format!("{v} is not laid out on the page"));
        return None;
    };
    Some(point(p.0.x + px(2.), p.0.y + p.1 / 2.))
}

fn mouse(window: &mut Window, cx: &mut App, p: Point<Pixels>, button: MouseButton, count: usize, modifiers: Modifiers) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent { button, position: p, modifiers, click_count: count, first_mouse: false }),
        cx,
    );
    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position: p, modifiers, click_count: count }), cx);
}

fn move_to(window: &mut Window, cx: &mut App, p: Point<Pixels>, pressed: Option<MouseButton>, modifiers: Modifiers) {
    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, pressed_button: pressed, modifiers }), cx);
}

fn step_once(step: &Value, ws: &WeakEntity<Workspace>, window: &mut Window, cx: &mut App) -> bool {
    let Some(ws) = ws.upgrade() else { return false };
    let page = ws.read(cx).page.clone();
    if let Some(t) = step.get("type").and_then(Value::as_str) {
        page.update(cx, |p, cx| p.insert_text(t, cx));
    } else if let Some(t) = step.get("comment").and_then(Value::as_str) {
        let Some(editor) = ws.read(cx).comment_edit.as_ref().map(|e| e.editor.clone()) else {
            fail("comment: no comment is being written");
            return true;
        };
        editor.update(cx, |v, cx| v.insert_text(t, cx));
    } else if let Some(t) = step.get("field").and_then(Value::as_str) {
        let Some(input) = ws.read(cx).sidebar.as_ref().map(|s| s.input.clone()) else {
            fail("field: the Alternatives panel isn't open on a group");
            return true;
        };
        input.update(cx, |i, cx| i.set_field_text(t, cx));
        let f = input.read(cx).focus.clone();
        window.focus(&f, cx);
        ws.update(cx, |_, cx| cx.notify());
    } else if let Some(v) = step.get("select") {
        if let Some(r) = range_of(v, &ws, cx) {
            page.update(cx, |p, cx| p.select(r, cx));
        }
    } else if let Some(v) = step.get("alt") {
        let Some(r) = range_of(v, &ws, cx) else { return true };
        let scope = match step.get("scope").and_then(Value::as_str) {
            Some("sentence") => doc_model::Scope::Sentence,
            Some("paragraph") => doc_model::Scope::Paragraph,
            _ => doc_model::Scope::Word,
        };
        let variants: Vec<String> = step
            .get("variants")
            .and_then(Value::as_array)
            .map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let model = ws.read(cx).model.clone();
        model.update(cx, |m, cx| match m.create_group(cx, r, scope) {
            Ok(id) => {
                for v in &variants {
                    m.add_variant(cx, id, v);
                }
            }
            Err(e) => fail(format!("alt: {e}")),
        });
    } else if let Some(k) = step.get("keys").and_then(Value::as_str) {
        for ks in k.split_whitespace() {
            if let Ok(ks) = Keystroke::parse(ks) {
                window.dispatch_keystroke(ks, cx);
            }
        }
    } else if let Some(a) = step.get("action").and_then(Value::as_str) {
        match cx.build_action(a, None) {
            Ok(action) => window.dispatch_action(action, cx),
            Err(e) => eprintln!("shots: unknown action {a}: {e}"),
        }
    } else if let Some(v) = step.get("cuts_scope").and_then(Value::as_str) {
        ws.update(cx, |ws, cx| {
            ws.cuts_scope = if v == "book" { crate::cuts_panel::Scope::Book } else { crate::cuts_panel::Scope::Page };
            cx.notify();
        });
    } else if let Some(p) = step.get("move").and_then(point_of) {
        move_to(window, cx, p, None, Modifiers::default());
    } else if let Some(v) = step.get("hover") {
        if let Some(p) = target(v, &ws, cx) {
            move_to(window, cx, p, None, Modifiers::default());
        }
    } else if let Some(v) = step.get("click") {
        let count = step.get("count").and_then(Value::as_u64).unwrap_or(1) as usize;
        let button = match step.get("button").and_then(Value::as_str) {
            Some("right") => MouseButton::Right,
            _ => MouseButton::Left,
        };
        if let Some(p) = target(v, &ws, cx) {
            move_to(window, cx, p, None, Modifiers::default());
            mouse(window, cx, p, button, count, Modifiers::default());
        }
    } else if let Some(i) = step.get("drag_note").and_then(Value::as_u64) {
        let card = ws.read(cx).drawer.as_ref().and_then(|d| d.card_bounds().get(i as usize).copied());
        let to = step.get("to").and_then(|v| target(v, &ws, cx));
        let (Some(card), Some(to)) = (card, to) else {
            fail(format!("drag_note {i}: no such note card or target"));
            return true;
        };
        let modifiers = Modifiers { alt: step.get("alt").and_then(Value::as_bool).unwrap_or(false), ..Default::default() };
        let from = card.center();
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: from, modifiers, click_count: 1, first_mouse: false }),
            cx,
        );
        for k in 1..=4 {
            let t = k as f32 / 4.;
            move_to(window, cx, point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t), Some(MouseButton::Left), modifiers);
            window.refresh();
            let _ = window.draw(cx);
        }
        window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: to, modifiers, click_count: 1 }), cx);
    } else if let Some(v) = step.get("drag") {
        let (Some(from), Some(to)) = (target(v, &ws, cx), step.get("to").and_then(|v| target(v, &ws, cx))) else {
            fail("drag: no such place".to_string());
            return true;
        };
        let modifiers = Modifiers { alt: step.get("alt").and_then(Value::as_bool).unwrap_or(false), ..Default::default() };
        move_to(window, cx, from, None, modifiers);
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: from, modifiers, click_count: 1, first_mouse: false }),
            cx,
        );
        window.refresh();
        let _ = window.draw(cx);
        for k in 1..=6 {
            let t = k as f32 / 6.;
            move_to(window, cx, point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t), Some(MouseButton::Left), modifiers);
            window.refresh();
            let _ = window.draw(cx);
        }
        if !step.get("hold").and_then(Value::as_bool).unwrap_or(false) {
            window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: to, modifiers, click_count: 1 }), cx);
        }
    } else if let Some(v) = step.get("release") {
        let Some(to) = target(v, &ws, cx) else { return true };
        window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: to, modifiers: Modifiers::default(), click_count: 1 }), cx);
    } else if let Some(e) = step.get("export") {
        ws.update(cx, |w, cx| w.script_export(e, cx));
    } else if step.get("first_open").is_some() {
        ws.update(cx, |w, cx| w.confirm_first_open(window, cx));
    } else if let Some(place) = step.get("new_project_place").and_then(Value::as_str) {
        ws.update(cx, |w, cx| w.set_new_project_place(std::path::PathBuf::from(place), cx));
    } else if let Some(e) = step.get("expect") {
        // Expectations see the state after the last frame.
        window.refresh();
        let _ = window.draw(cx);
        for err in expect(e, &ws, cx) {
            fail(err);
        }
    } else if let Some(s) = step.get("say").and_then(Value::as_str) {
        eprintln!("shots: say {s}");
    } else if let Some(Value::Object(changes)) = step.get("prefs") {
        let mut prefs = serde_json::to_value(crate::prefs::Prefs::get(cx)).unwrap_or_default();
        if let Value::Object(p) = &mut prefs {
            p.extend(changes.clone());
        }
        match serde_json::from_value::<crate::prefs::Prefs>(prefs) {
            Ok(p) => {
                cx.set_global(p);
                crate::theme::apply(cx);
                // As Preferences does: a new Show cuts as redraws the page's cuts.
                if crate::prefs::publish(cx.global::<crate::prefs::Prefs>()) {
                    ws.update(cx, |ws, cx| ws.represent_cuts(cx));
                }
            }
            Err(e) => fail(format!("prefs: {e}")),
        }
    } else if let Some(path) = step.get("shot").and_then(Value::as_str).filter(|_| step.get("window").and_then(Value::as_str) == Some("about")) {
        let Some(about) = crate::about::window(cx) else {
            fail("shot: no About window (dispatch app::About first)");
            return true;
        };
        let saved = about.update(cx, |_, w, cx| {
            w.refresh();
            let _ = w.draw(cx);
            w.render_to_image().map_err(|e| e.to_string()).and_then(|img| img.save(path).map_err(|e| e.to_string()))
        });
        if let Ok(Err(e)) | Err(e) = saved.map_err(|e| e.to_string()) {
            eprintln!("shots: about shot failed: {e}");
        }
    } else if let Some(path) = step.get("shot").and_then(Value::as_str) {
        // Occluded windows get no display-link frames; draw synchronously.
        window.refresh();
        let _ = window.draw(cx);
        match window.render_to_image() {
            Ok(img) => {
                if let Err(e) = img.save(path) {
                    eprintln!("shots: cannot save {path}: {e}");
                }
            }
            Err(e) => eprintln!("shots: render failed: {e}"),
        }
    } else if step.get("quit").is_some() {
        return false;
    }
    // Offscreen windows get no display-link frames: draw now so hover and key contexts are current.
    window.refresh();
    let _ = window.draw(cx);
    true
}

/// Checks the window against `e`; each key is one check:
///
/// - `text_contains` / `text_lacks`: a string or a list of strings, on the page text
/// - `groups`, `ghosts`, `notes`: how many are on the page / in Overflow
/// - `group`: `{"index": n, "active": k, "variants": […], "scope": "word"}` (page order)
/// - `sidebar`, `drawer`: whether they are open
/// - `hover_label`: the label under the hovered group's dots
/// - `status`: text in the status toast
/// - `origin`: `{"note": i, "paragraph": n}`, the note's `↩ ¶n` link
/// - `clipboard_contains`: text on the clipboard
/// - `dirty`: whether the document has unsaved changes
/// - `path_ends`: the end of the document's path (`"part-1/03-the-visit.md"`)
/// - `project`: whether the window is on a folder
fn expect(e: &Value, ws: &Entity<Workspace>, cx: &mut App) -> Vec<String> {
    let errors = RefCell::new(Vec::new());
    let fail = |what: String| errors.borrow_mut().push(what);
    let w = ws.read(cx);
    let doc = &w.model.read(cx).doc;
    let text = doc.text();
    let strings = |v: &Value| -> Vec<String> {
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        }
    };
    let count = |key: &str, got: usize| {
        if let Some(want) = e.get(key).and_then(Value::as_u64) {
            if got != want as usize {
                fail(format!("{key}: want {want}, got {got}"));
            }
        }
    };
    let flag = |key: &str, got: bool| {
        if let Some(want) = e.get(key).and_then(Value::as_bool) {
            if got != want {
                fail(format!("{key}: want {want}, got {got}"));
            }
        }
    };
    for s in e.get("text_contains").map(strings).unwrap_or_default() {
        if !text.contains(&s) {
            fail(format!("text_contains {s:?}; the page is:\n{text}"));
        }
    }
    for s in e.get("text_lacks").map(strings).unwrap_or_default() {
        if text.contains(&s) {
            fail(format!("text_lacks {s:?}"));
        }
    }
    let groups = doc.visible_groups();
    count("groups", groups.len());
    count("ghosts", doc.ghost_ranges().len());
    count("notes", doc.notes().len());
    flag("sidebar", w.sidebar.is_some());
    flag("drawer", w.drawer.is_some());
    if let Some(g) = e.get("group") {
        let n = g.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        match groups.get(n).and_then(|vg| doc.group(vg.id)) {
            None => fail(format!("group {n}: there are {} groups", groups.len())),
            Some(info) => {
                if let Some(want) = g.get("active").and_then(Value::as_u64) {
                    if info.active != want as usize {
                        fail(format!("group {n} active: want {want}, got {}", info.active));
                    }
                }
                if let Some(want) = g.get("variants") {
                    let got: Vec<String> = info.variants.iter().map(|v| v.text.clone()).collect();
                    if got != strings(want) {
                        fail(format!("group {n} variants: want {:?}, got {got:?}", strings(want)));
                    }
                }
                if let Some(want) = g.get("scope").and_then(Value::as_str) {
                    let got = crate::sidebar::scope_label(info.scope).to_lowercase();
                    if got != want {
                        fail(format!("group {n} scope: want {want}, got {got}"));
                    }
                }
            }
        }
    }
    if let Some(want) = e.get("hover_label").and_then(Value::as_str) {
        let got = w.page.read(cx).hover_chip(cx).map(|(_, l)| l);
        if got.as_deref() != Some(want) {
            fail(format!("hover_label: want {want:?}, got {got:?}"));
        }
    }
    if let Some(want) = e.get("status").and_then(Value::as_str) {
        let got = w.status_text();
        if !got.as_deref().is_some_and(|s| s.contains(want)) {
            fail(format!("status: want {want:?}, got {got:?}"));
        }
    }
    if let Some(o) = e.get("origin") {
        let i = o.get("note").and_then(Value::as_u64).unwrap_or(0) as usize;
        let want = o.get("paragraph").and_then(Value::as_u64).map(|n| n as usize);
        let got = doc.notes().get(i).and_then(|n| n.origin.clone()).and_then(|q| w.origin_paragraph(&q, cx)).map(|(n, _)| n);
        if got != want {
            fail(format!("origin of note {i}: want ¶{want:?}, got ¶{got:?}"));
        }
    }
    if let Some(want) = e.get("clipboard_contains").and_then(Value::as_str) {
        let got = cx.read_from_clipboard().and_then(|c| c.text());
        if !got.as_deref().is_some_and(|s| s.contains(want)) {
            fail(format!("clipboard_contains {want:?}; clipboard: {got:?}"));
        }
    }
    let m = w.model.read(cx);
    flag("dirty", m.dirty());
    if let Some(want) = e.get("path_ends").and_then(Value::as_str) {
        if !m.path.as_ref().is_some_and(|p| p.ends_with(want)) {
            fail(format!("path_ends {want:?}; path: {:?}", m.path));
        }
    }
    flag("project", w.project_root().is_some());
    errors.into_inner()
}
