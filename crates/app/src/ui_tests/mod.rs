mod about;
pub(crate) mod ai;
mod alternatives;
mod comments;
mod context_menu;
mod cuts;
mod drag;
mod editing;
mod files;
mod l10n;
mod layout;
#[cfg(not(target_os = "macos"))]
mod menu_bar;
mod ghosts;
mod overflow;
mod panel_foot;
mod panels;
mod paragraphs;
mod perf;
mod prefs;
mod private;
mod project;
mod sentences;
mod smoke;
mod spelling;
mod themes;
mod welcome;
mod window;
mod zen;

use std::ops::Range;
use std::sync::Arc;

use doc_model::{GroupId, GroupInfo, VisibleGroup};
use gpui_kit::{
    point, px, size, Action, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, TestApp, TestAppWindow, TouchPhase, WindowBounds, WindowOptions,
};

use crate::model::DocModel;
use crate::prefs::{Effects, Prefs};
use crate::view::TextView;
use crate::workspace::Workspace;

/// A shortcut label as the Mac shows it ("⇧⌘A"), as this system shows it ("Ctrl+Shift+A").
pub fn key_label(mac: &str) -> String {
    crate::os_text::key(mac)
}

#[derive(Debug, PartialEq)]
pub struct MenuItem {
    pub label: String,
    pub enabled: bool,
    pub badge: Option<&'static str>,
    pub shortcut: Option<String>,
}

pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dostoevsky-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Canonical, so it matches paths the app canonicalizes (/var → /private/var).
        TempDir(dir.canonicalize().unwrap())
    }

    pub fn file(&self, name: &str, text: &str) -> std::path::PathBuf {
        let p = self.0.join(name);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Ui {
    pub(crate) app: TestApp,
    win: TestAppWindow<Workspace>,
}

impl Ui {
    /// In a 1180×860 window; a trailer may carry groups.
    pub fn open(md: &str) -> Ui {
        Ui::open_timed(md).0
    }

    pub fn open_editing(md: &str) -> Ui {
        let mut ui = Ui::open(md);
        ui.action(crate::actions::ToggleLayer);
        assert!(ui.read(|ws, cx| ws.model.read(cx).layer));
        ui
    }

    /// And how long the document took from parsing to its first frame.
    pub fn open_timed(md: &str) -> (Ui, std::time::Duration) {
        Ui::open_sized_timed(md, size(px(1180.), px(860.)))
    }

    pub fn open_sized(md: &str, w: f32, h: f32) -> Ui {
        Ui::open_sized_timed(md, size(px(w), px(h))).0
    }

    fn open_sized_timed(md: &str, window_size: gpui_kit::Size<Pixels>) -> (Ui, std::time::Duration) {
        let app = Ui::app();
        let t = std::time::Instant::now();
        let loaded = crate::format::load(md);
        let mut model = DocModel::new(loaded.doc);
        model.layer = loaded.layer;
        model.present();
        model.kept = loaded.kept;
        let ui = Ui::window(app, model, window_size);
        let took = t.elapsed();
        (ui, took)
    }

    pub fn open_file(path: &std::path::Path) -> Ui {
        Ui::open_file_sized(path, 1180., 860.)
    }

    /// The file it would show, then the project.
    pub fn open_folder(root: &std::path::Path) -> Ui {
        let tree = crate::project::open(root);
        let memory = crate::files::project_memory(root);
        let first = tree.opening_file(memory.last.as_deref(), crate::project::Day::today());
        let model = first
            .and_then(|p| crate::project_ui::load_model(&p).ok())
            .unwrap_or_else(|| DocModel::new(doc_model::Document::new()));
        let mut ui = Ui::window(Ui::app(), model, size(px(1180.), px(860.)));
        ui.win.update(|ws, window, cx| ws.attach_project(root.to_path_buf(), window, cx));
        ui.draw();
        ui
    }

    pub fn open_file_sized(path: &std::path::Path, w: f32, h: f32) -> Ui {
        let model = crate::read_model(path).expect("readable file");
        Ui::window(Ui::app(), model, size(px(w), px(h)))
    }

    fn app() -> TestApp {
        // Keys never touch the real Keychain in tests (ai::secrets keeps them per thread).
        std::env::set_var("DOSTOEVSKY_SECRETS", "memory");
        #[cfg(target_os = "macos")]
        let text_system = Arc::new(gpui_apple::AppleTextSystem::new());
        // The bundled fonts are loaded below; Quattro stands in for a missing system UI font.
        #[cfg(not(target_os = "macos"))]
        let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new(crate::theme::QUATTRO));
        let mut app = TestApp::with_text_system_and_assets(text_system, Arc::new(crate::theme::Assets));
        app.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::load_fonts(cx);
            // No Sounds global, so nothing is audible; tones are recorded (`played`) instead.
            // The look the flow baselines were approved in; tests pick light themselves.
            // Zen writing and typewriter are on for new installs, and editing opens on Review;
            // tests turn them on themselves.
            let panels = crate::prefs::ModePanels { editing: crate::prefs::Panels::default(), ..Default::default() };
            cx.set_global(Prefs { appearance: crate::prefs::Appearance::Dark, zen_writing: false, typewriter: false, panels, ..Prefs::default() });
            crate::theme::apply(cx);
            cx.set_global(crate::memory::Memory::default());
            crate::keymap::apply(cx);
            crate::register_actions(cx);
        });
        app
    }

    fn window(mut app: TestApp, model: DocModel, window_size: gpui_kit::Size<Pixels>) -> Ui {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), window_size))),
            ..Default::default()
        };
        let win = app.open_window_with_options(options, |window, cx| Workspace::new(model, window, cx));
        let (handle, ws) = (win.handle(), win.root());
        app.update(|cx| cx.default_global::<crate::Windows>().0.push((handle.into(), ws.downgrade())));
        crate::sound::PLAYED.with(|p| p.borrow_mut().clear());
        let mut ui = Ui { app, win };
        ui.draw();
        ui
    }

    pub fn draw(&mut self) {
        self.win.draw();
        // Saves on the writer thread land (and say how they went) before the test looks.
        crate::files::wait_for_writes(None);
        self.app.run_until_parked();
    }

    pub fn read<R>(&self, f: impl FnOnce(&Workspace, &gpui_kit::App) -> R) -> R {
        self.win.read(f)
    }

    pub fn read_window<R>(&mut self, f: impl FnOnce(&Workspace, &mut gpui_kit::Window, &mut gpui_kit::App) -> R) -> R {
        self.win.update(|ws, window, cx| f(ws, window, cx))
    }

    fn page(&self) -> Entity<TextView> {
        self.read(|ws, _| ws.page.clone())
    }

    fn model(&self) -> Entity<DocModel> {
        self.read(|ws, _| ws.model.clone())
    }

    /// Space-separated, as in a keymap: `"cmd-shift-a"`, `"alt-cmd-down down"`. Mac keys; off the
    /// Mac each goes through `actions::off_mac`, as the default bindings do.
    pub fn keys(&mut self, keys: &str) {
        for k in keys.split_whitespace() {
            let k = if cfg!(target_os = "macos") { k.to_string() } else { crate::actions::off_mac(k).unwrap_or_else(|| panic!("{k} has no binding off the Mac")) };
            self.win.simulate_keystroke(&k);
            self.draw();
        }
    }

    pub fn type_text(&mut self, text: &str) {
        self.win.simulate_input(text);
        self.draw();
    }

    /// ⇧ alone, then none, is a tap.
    pub fn modifiers(&mut self, m: gpui_kit::Modifiers) {
        self.win.simulate_event(gpui_kit::ModifiersChangedEvent { modifiers: m, capslock: Default::default() });
        self.draw();
    }

    pub fn action(&mut self, action: impl Action) {
        self.win.update(|_, window, cx| window.dispatch_action(action.boxed_clone(), cx));
        self.draw();
    }

    pub fn set_prefs(&mut self, f: impl FnOnce(&mut Prefs)) {
        let cuts = self.app.update(|cx| {
            f(cx.global_mut::<Prefs>());
            crate::prefs::publish(cx.global::<Prefs>())
        });
        if cuts {
            self.win.update(|ws, _, cx| ws.represent_cuts(cx));
        }
        self.draw();
    }

    pub fn undo(&mut self) {
        self.keys("cmd-z");
    }

    pub fn redo(&mut self) {
        self.keys("cmd-shift-z");
    }

    pub fn caret_at(&mut self, c: usize) {
        self.select(c..c);
    }

    pub fn select(&mut self, r: Range<usize>) {
        let page = self.page();
        self.win.update(|_, _, cx| page.update(cx, |p, cx| p.select(r, cx)));
        self.draw();
    }

    pub fn select_text(&mut self, needle: &str) {
        let r = self.find(needle);
        self.select(r);
    }

    /// The last alternative ends up active.
    pub fn make_group(&mut self, needle: &str, alternatives: &[&str]) -> GroupId {
        self.select_text(needle);
        self.keys("cmd-shift-a");
        let id = self.sidebar();
        let id = id.unwrap_or_else(|| panic!("⌘⇧A on {needle:?} made no group; status: {:?}", self.status()));
        for a in alternatives {
            self.type_text(a);
            self.keys("enter");
        }
        // Esc goes back a step at a time: the versions, then the list, then out.
        self.keys("escape");
        if self.sidebar_open() {
            self.keys("escape");
        }
        id
    }

    pub fn hover_group(&mut self, id: GroupId) {
        let g = self.visible_group(id);
        let mid = g.range.start + (g.range.end - g.range.start) / 2;
        let p = self.point_for(mid);
        self.move_pointer(p);
    }

    pub fn unhover(&mut self) {
        self.move_pointer(point(px(5.), px(845.)));
    }

    pub fn click(&mut self, position: Point<Pixels>, button: MouseButton) {
        self.move_pointer(position);
        let modifiers = Modifiers::default();
        self.win.simulate_event(MouseDownEvent { position, button, modifiers, click_count: 1, first_mouse: false });
        self.win.simulate_event(MouseUpEvent { position, button, modifiers, click_count: 1 });
        self.draw();
    }

    pub fn click_dots(&mut self, id: GroupId) {
        let g = self.visible_group(id);
        let page = self.page();
        let b = self
            .read(|_, cx| crate::view::dots_bounds(page.read(cx).layout.as_ref()?, &g))
            .unwrap_or_else(|| panic!("group {id:?} has no dots on screen"));
        self.click(b.center(), MouseButton::Left);
    }

    pub fn right_click_text(&mut self, needle: &str) {
        let r = self.find(needle);
        let p = self.point_for((r.start + r.end) / 2);
        self.click(p, MouseButton::Right);
    }

    pub fn hover_text(&mut self, needle: &str) {
        let r = self.find(needle);
        let p = self.point_for((r.start + r.end) / 2);
        self.move_pointer(p);
    }

    pub fn click_note(&mut self, i: usize) {
        let b = self.note_card(i);
        self.click(b.center(), MouseButton::Left);
    }

    pub fn double_click_note(&mut self, i: usize) {
        let b = self.note_card(i);
        self.move_pointer(b.center());
        self.click_n(b.center(), 1);
        self.click_n(b.center(), 2);
    }

    /// The cut clicked once, by its place in the panel's order.
    pub fn selected_note(&self) -> Option<usize> {
        self.read(|ws, cx| {
            let at = ws.drawer.as_ref().and_then(|d| d.selected.clone())?;
            ws.panel_items(cx).iter().position(|i| i.at == at)
        })
    }

    /// `alt` copies.
    pub fn drag_note(&mut self, i: usize, to: Point<Pixels>, alt: bool) {
        let from = self.note_card(i).center();
        let modifiers = Modifiers { alt, ..Default::default() };
        let left = MouseButton::Left;
        self.win.simulate_event(MouseDownEvent { position: from, button: left, modifiers, click_count: 1, first_mouse: false });
        for k in 1..=4 {
            let t = k as f32 / 4.;
            let position = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
            self.win.simulate_event(MouseMoveEvent { position, modifiers, pressed_button: Some(left) });
            self.draw();
        }
        self.win.simulate_event(MouseUpEvent { position: to, button: left, modifiers, click_count: 1 });
        self.draw();
    }

    /// `alt` held throughout copies; `before_release` runs with the button still down.
    pub fn drag_path(&mut self, from: Point<Pixels>, path: &[Point<Pixels>], alt: bool, before_release: impl FnOnce(&mut Ui)) {
        let modifiers = Modifiers { alt, ..Default::default() };
        let left = MouseButton::Left;
        self.move_pointer(from);
        self.win.simulate_event(MouseDownEvent { position: from, button: left, modifiers, click_count: 1, first_mouse: false });
        self.draw();
        let mut last = from;
        for &to in path {
            for k in 1..=4 {
                let t = k as f32 / 4.;
                let position = point(last.x + (to.x - last.x) * t, last.y + (to.y - last.y) * t);
                self.win.simulate_event(MouseMoveEvent { position, modifiers, pressed_button: Some(left) });
                self.draw();
            }
            last = to;
        }
        before_release(self);
        self.win.simulate_event(MouseUpEvent { position: last, button: left, modifiers, click_count: 1 });
        self.draw();
    }

    pub fn drag_selection(&mut self, to: Point<Pixels>, alt: bool) {
        let sel = self.selection();
        let from = self.point_for((sel.start + sel.end) / 2);
        self.drag_path(from, &[to], alt, |_| {});
    }

    /// The landing place shown on the page while dragging.
    pub fn drop_mark(&self) -> Option<doc_model::Place> {
        self.read(|ws, cx| ws.page.read(cx).drop_mark)
    }

    pub fn drag_id(&mut self, id: impl Into<gpui_kit::ElementId>, dx: f32) {
        let from = self.element(id).bounds().center();
        let (left, modifiers) = (MouseButton::Left, Modifiers::default());
        self.win.simulate_event(MouseDownEvent { position: from, button: left, modifiers, click_count: 1, first_mouse: false });
        for k in 1..=4 {
            let position = point(from.x + px(dx * k as f32 / 4.), from.y);
            self.win.simulate_event(MouseMoveEvent { position, modifiers, pressed_button: Some(left) });
            self.draw();
        }
        let position = point(from.x + px(dx), from.y);
        self.win.simulate_event(MouseUpEvent { position, button: left, modifiers, click_count: 1 });
        self.draw();
    }

    pub fn drag_row(&mut self, from: &str, to: &str, above: bool) {
        let start = self.element(gpui_kit::SharedString::from(from.to_string())).bounds().center();
        let b = self.element(gpui_kit::SharedString::from(to.to_string())).bounds();
        let end = point(b.center().x, if above { b.top() + px(3.) } else { b.bottom() - px(3.) });
        let (left, modifiers) = (MouseButton::Left, Modifiers::default());
        self.win.simulate_event(MouseDownEvent { position: start, button: left, modifiers, click_count: 1, first_mouse: false });
        for k in 1..=4 {
            let t = k as f32 / 4.;
            let position = point(start.x + (end.x - start.x) * t, start.y + (end.y - start.y) * t);
            self.win.simulate_event(MouseMoveEvent { position, modifiers, pressed_button: Some(left) });
            self.draw();
        }
        self.win.simulate_event(MouseUpEvent { position: end, button: left, modifiers, click_count: 1 });
        self.draw();
    }

    fn note_card(&self, i: usize) -> Bounds<Pixels> {
        let cards = self.read(|ws, _| ws.drawer.as_ref().expect("the drawer is closed").card_bounds());
        *cards.get(i).unwrap_or_else(|| panic!("no note card {i}; cards: {cards:?}"))
    }

    pub fn answer_prompt(&mut self, label: &str) {
        assert!(self.app.has_pending_prompt(), "no prompt is open");
        self.app.simulate_prompt_answer(label);
        self.draw();
    }

    /// The element must opt in with `.test_support()`.
    pub fn element(&mut self, id: impl Into<gpui_kit::ElementId>) -> gpui_kit::test::ElementSnapshot {
        use gpui_kit::test::TestWindowExt;
        let id = id.into();
        self.win.update(|_, window, _| window.try_find(id.clone()).unwrap_or_else(|| panic!("no element {id:?} in the last frame")))
    }

    pub fn has_element(&mut self, id: impl Into<gpui_kit::ElementId>) -> bool {
        use gpui_kit::test::TestWindowExt;
        let id = id.into();
        self.win.update(|_, window, _| window.try_find(id).is_some())
    }

    pub fn click_id(&mut self, id: impl Into<gpui_kit::ElementId>) {
        let b = self.element(id).bounds();
        self.click(b.center(), MouseButton::Left);
    }

    pub fn click_n(&mut self, p: Point<Pixels>, n: usize) {
        let (button, modifiers) = (MouseButton::Left, Modifiers::default());
        self.win.simulate_event(MouseDownEvent { position: p, button, modifiers, click_count: n, first_mouse: false });
        self.win.simulate_event(MouseUpEvent { position: p, button, modifiers, click_count: n });
        self.draw();
    }

    pub fn click_variant(&mut self, i: usize) {
        let b = self.variant_row(i);
        self.click(b.center(), MouseButton::Left);
    }

    pub fn double_click_variant(&mut self, i: usize) {
        let b = self.variant_row(i);
        self.click_n(b.center(), 1);
        self.click_n(b.center(), 2);
    }

    pub fn variant_field(&self) -> Option<(usize, String)> {
        self.read(|ws, cx| {
            let e = ws.sidebar.as_ref()?.editing.as_ref()?;
            let i = ws.model.read(cx).doc.group(e.group)?.variants.iter().position(|v| v.id == e.variant)?;
            Some((i, e.field.read(cx).field_text().to_string()))
        })
    }

    pub fn sidebar_field(&self) -> String {
        self.read(|ws, cx| ws.sidebar.as_ref().expect("sidebar is closed").input.read(cx).field_text().to_string())
    }

    pub fn right_click_variant(&mut self, i: usize) {
        let b = self.variant_row(i);
        self.click(b.center(), MouseButton::Right);
    }

    fn variant_row(&self, i: usize) -> Bounds<Pixels> {
        let rows = self.read(|ws, _| ws.sidebar.as_ref().expect("sidebar is closed").row_bounds());
        *rows.get(i).unwrap_or_else(|| panic!("no variant row {i}; rows: {rows:?}"))
    }

    pub fn menu(&mut self, label: &str) {
        self.win.update(|ws, window, cx| ws.run_menu_label(label, window, cx));
        self.draw();
    }

    pub fn move_pointer(&mut self, position: Point<Pixels>) {
        self.win.simulate_event(MouseMoveEvent { position, modifiers: Default::default(), pressed_button: None });
        self.draw();
    }

    /// `lines` > 0 is towards the end of the document.
    pub fn wheel_notch(&mut self, position: Point<Pixels>, lines: f32) {
        let delta = ScrollDelta::Lines(point(0., -lines));
        self.wheel(ScrollWheelEvent { position, delta, touch_phase: TouchPhase::Moved, ..Default::default() });
    }

    pub fn wheel_notch_alt(&mut self, position: Point<Pixels>, lines: f32) {
        let delta = ScrollDelta::Lines(point(0., -lines));
        let modifiers = Modifiers { alt: true, ..Default::default() };
        self.wheel(ScrollWheelEvent { position, delta, modifiers, touch_phase: TouchPhase::Moved });
    }

    /// `dy` > 0 is towards the end of the document.
    pub fn swipe(&mut self, position: Point<Pixels>, dy: f32, touch_phase: TouchPhase) {
        let delta = ScrollDelta::Pixels(point(px(0.), px(-dy)));
        self.wheel(ScrollWheelEvent { position, delta, touch_phase, ..Default::default() });
    }

    /// A trackpad moved sideways: `dx` > 0 is fingers moving left (the next version).
    pub fn swipe_sideways(&mut self, position: Point<Pixels>, dx: f32, dy: f32, touch_phase: TouchPhase) {
        let delta = ScrollDelta::Pixels(point(px(-dx), px(-dy)));
        self.wheel(ScrollWheelEvent { position, delta, touch_phase, ..Default::default() });
    }

    fn wheel(&mut self, e: ScrollWheelEvent) {
        self.move_pointer(e.position);
        self.win.simulate_event(e);
        self.draw();
    }

    pub fn point_for(&self, c: usize) -> Point<Pixels> {
        self.read(|ws, cx| {
            let layout = ws.page.read(cx).layout.clone().expect("page was never laid out");
            let p = layout.point_for(c).unwrap_or_else(|| panic!("char {c} is not laid out"));
            point(p.x + px(2.), p.y + layout.line_height() / 2.)
        })
    }

    /// As displayed: active variants, ghosts included.
    pub fn text(&self) -> String {
        self.read(|ws, cx| ws.model.read(cx).doc.text())
    }

    pub fn find(&self, needle: &str) -> Range<usize> {
        let text = self.text();
        let byte = text.find(needle).unwrap_or_else(|| panic!("{needle:?} not on the page:\n{text}"));
        let start = text[..byte].chars().count();
        start..start + needle.chars().count()
    }

    pub fn groups(&self) -> Vec<VisibleGroup> {
        self.read(|ws, cx| ws.model.read(cx).doc.visible_groups())
    }

    pub fn visible_group(&self, id: GroupId) -> VisibleGroup {
        self.groups().into_iter().find(|g| g.id == id).unwrap_or_else(|| panic!("group {id:?} is not on the page"))
    }

    pub fn only_group(&self) -> GroupId {
        let gs = self.groups();
        assert_eq!(gs.len(), 1, "expected one group, found {gs:?}");
        gs[0].id
    }

    pub fn group(&self, id: GroupId) -> GroupInfo {
        let model = self.model();
        self.app.read(|cx| model.read(cx).doc.group(id)).expect("no such group")
    }

    /// Original first.
    pub fn variants(&self, id: GroupId) -> Vec<String> {
        self.group(id).variants.into_iter().map(|v| v.text).collect()
    }

    pub fn active(&self, id: GroupId) -> usize {
        self.group(id).active
    }

    pub fn hovered_group(&self) -> Option<GroupId> {
        self.read(|ws, cx| ws.page.read(cx).hovered_group)
    }

    pub fn selection(&self) -> Range<usize> {
        self.read(|ws, cx| ws.page.read(cx).selection())
    }

    pub fn sidebar_open(&self) -> bool {
        self.read(|ws, _| ws.sidebar.is_some())
    }

    /// `None` while the panel shows the list of every group.
    pub fn sidebar(&self) -> Option<GroupId> {
        self.read(|ws, _| ws.sidebar.as_ref().expect("sidebar is closed").group)
    }

    pub fn sidebar_cursor(&self) -> Option<GroupId> {
        self.read(|ws, _| ws.sidebar.as_ref().expect("sidebar is closed").cursor)
    }

    /// The group washed on the page for the alternatives panel.
    pub fn spotlight(&self) -> Option<GroupId> {
        self.read(|ws, cx| ws.page.read(cx).spotlight)
    }

    /// Pixels from group `id`'s first line to the middle of the page.
    pub fn off_centre(&self, id: GroupId) -> f32 {
        self.off_centre_at(self.visible_group(id).range.start)
    }

    /// Pixels from the line holding display offset `c` to the middle of the page.
    pub fn off_centre_at(&self, c: usize) -> f32 {
        let (y, mid) = self.read(|ws, cx| {
            let p = ws.page.read(cx);
            let l = p.layout.as_ref().expect("laid out");
            let lh = f32::from(l.line_height());
            (f32::from(l.point_for(c).expect("on the page").y) + lh / 2., f32::from(l.bounds().center().y))
        });
        y - mid
    }


    /// The part of the page left undimmed.
    pub fn focus_range(&self) -> Option<Range<usize>> {
        self.read(|ws, cx| ws.page.read(cx).focus_range.clone())
    }

    pub fn hover_label(&self) -> Option<String> {
        self.read(|ws, cx| ws.page.read(cx).hover_chip(cx).map(|(_, label)| label))
    }

    pub fn hover_label_bounds(&self) -> Option<gpui_kit::Bounds<Pixels>> {
        self.read(|ws, cx| ws.page.read(cx).chip_bounds(cx))
    }

    /// The selection the buttons above it are for, while they show.
    pub fn selection_buttons(&self) -> Option<Range<usize>> {
        self.read(|ws, cx| ws.page.read(cx).selection_buttons(cx))
    }

    pub fn mouse_select(&mut self, needle: &str) {
        let r = self.find(needle);
        let from = self.point_for(r.start);
        let to = self.point_for(r.end);
        self.drag_path(from, &[to], false, |_| {});
    }

    pub fn status(&self) -> Option<String> {
        self.read(|ws, _| ws.status_text())
    }

    pub fn menu_labels(&self) -> Vec<String> {
        self.read(|ws, _| {
            let menu = ws.context_menu.as_ref().expect("no context menu is open");
            menu.items.iter().flatten().map(|it| it.label.to_string()).collect()
        })
    }

    pub fn drawer_open(&self) -> bool {
        self.read(|ws, _| ws.drawer.is_some())
    }

    /// The cut being edited, by its place in the panel's order.
    pub fn editing_note(&self) -> Option<usize> {
        self.read(|ws, cx| {
            let at = ws.drawer.as_ref().and_then(|d| d.editing.clone())?;
            ws.panel_items(cx).iter().position(|i| i.at == at)
        })
    }

    /// The Cuts panel's cuts in its scope and order, folded ones left out, open or not.
    pub fn notes(&self) -> Vec<String> {
        self.read(|ws, cx| ws.panel_items(cx).into_iter().map(|i| i.text).collect())
    }

    pub fn marks(&mut self) -> Vec<Bounds<Pixels>> {
        self.draw();
        self.read(|ws, cx| {
            let p = ws.page.read(cx);
            p.marks.iter().filter_map(|&(_, at, para)| p.mark_rect(at, para, cx)).collect()
        })
    }

    pub fn no_trace(&self) -> Vec<String> {
        self.read(|ws, cx| {
            ws.panel_items(cx).into_iter().filter(|i| matches!(i.trace, crate::cuts_panel::Trace::Untraced | crate::cuts_panel::Trace::Gone)).map(|i| i.text).collect()
        })
    }

    pub fn cut_line(&mut self, i: usize) -> String {
        self.draw();
        self.element(gpui_kit::ElementId::NamedInteger("note".into(), i as u64)).label().map(String::from).unwrap_or_default()
    }

    pub fn chip(&self, i: usize) -> Option<String> {
        self.read(|ws, cx| ws.panel_items(cx).get(i).and_then(|it| it.chip.clone()))
    }

    /// Cuts kept inside the page's own file (a chapter's from before its project kept them).
    pub fn file_notes(&self) -> Vec<String> {
        self.read(|ws, cx| ws.model.read(cx).doc.cuts().into_iter().filter(|c| matches!(c.place, doc_model::CutPlace::Note(_)) && c.source.is_none()).map(|c| c.text).collect())
    }

    pub fn origin_number(&self, i: usize) -> Option<usize> {
        self.read(|ws, cx| {
            let at = ws.panel_items(cx).get(i)?.at.clone();
            let quote = match at {
                crate::cuts_panel::CutRef::Page(id) => ws.model.read(cx).doc.cut_info(id)?.origin?,
                crate::cuts_panel::CutRef::Drawer(id) => ws.project.as_ref()?.drawer.model.read(cx).doc.cut_info(id)?.origin?,
                _ => return None,
            };
            ws.origin_paragraph(&quote, cx).map(|(n, _)| n)
        })
    }

    pub fn cut_menu(&mut self, i: usize, label: &str) {
        let b = self.note_card(i);
        self.click(b.center(), MouseButton::Right);
        self.menu(label);
    }

    pub fn flash(&self) -> Option<Range<usize>> {
        self.read(|ws, cx| ws.page.read(cx).flash.as_ref().map(|(r, _)| r.clone()))
    }

    pub fn flights(&self) -> Vec<Effects> {
        self.read(|ws, _| ws.overlay.flights())
    }

    pub fn title(&self) -> String {
        self.read(|ws, cx| crate::workspace::window_title(ws.model.read(cx)))
    }

    /// Top-left in the window, and width.
    pub fn column(&self) -> (Point<Pixels>, Pixels) {
        self.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").column())
    }

    /// In window coordinates, one per row.
    pub fn text_bounds(&self, range: Range<usize>) -> Vec<gpui_kit::Bounds<Pixels>> {
        self.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").text_bounds(range))
    }

    /// The first dot's centre, and whether the dots run across.
    pub fn dots(&self, id: GroupId) -> (Point<Pixels>, bool) {
        let g = self.visible_group(id);
        self.read(|ws, cx| crate::view::dots_start(ws.page.read(cx).layout.as_ref().expect("laid out"), &g)).expect("dots on the page")
    }

    /// From and to, in window coordinates.
    pub fn caret_slide(&self) -> Option<(Point<Pixels>, Point<Pixels>)> {
        self.read(|ws, cx| ws.page.read(cx).caret_slide())
    }

    pub fn line_height(&self) -> Pixels {
        self.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").line_height())
    }

    pub fn chrome_dimmed(&self) -> bool {
        self.read(|ws, _| ws.chrome_dimmed())
    }

    pub fn is_fullscreen(&mut self) -> bool {
        self.win.update(|_, window, _| window.is_fullscreen())
    }

    pub fn dirty(&self) -> bool {
        self.read(|ws, cx| ws.model.read(cx).dirty())
    }

    pub fn path(&self) -> Option<std::path::PathBuf> {
        self.read(|ws, cx| ws.model.read(cx).path.clone())
    }

    pub fn advance(&mut self, d: std::time::Duration) {
        self.app.advance_clock(d);
        self.draw();
    }

    pub fn choose_save_path(&mut self, path: &std::path::Path) {
        assert!(self.app.did_prompt_for_new_path(), "no save panel is open");
        let path = path.to_path_buf();
        self.app.simulate_new_path_selection(move |_| Some(path));
        self.draw();
    }

    pub fn drop_files(&mut self, paths: &[std::path::PathBuf], position: Point<Pixels>) {
        use gpui_kit::{ExternalPaths, FileDropEvent};
        let paths = ExternalPaths(paths.iter().cloned().collect());
        self.win.simulate_event(FileDropEvent::Entered { position, paths });
        self.win.simulate_event(FileDropEvent::Pending { position });
        self.win.simulate_event(FileDropEvent::Submit { position });
        self.draw();
    }

    pub fn window_count(&self) -> usize {
        self.app.windows().len()
    }

    pub fn documents(&self) -> Vec<(String, Option<std::path::PathBuf>)> {
        self.app.read(|cx| {
            let open = cx.windows();
            cx.try_global::<crate::Windows>()
                .map(|w| {
                    w.0.iter()
                        .filter(|(h, _)| open.contains(h))
                        .filter_map(|(_, ws)| ws.upgrade())
                        .map(|ws| {
                            let m = ws.read(cx).model.read(cx);
                            (m.doc.text(), m.path.clone())
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    pub fn update_window<R>(&mut self, f: impl FnOnce(&mut Workspace, &mut gpui_kit::Window, &mut gpui_kit::Context<Workspace>) -> R) -> R {
        let r = self.win.update(f);
        self.draw();
        r
    }

    pub fn with_app<R>(&mut self, f: impl FnOnce(&mut gpui_kit::App) -> R) -> R {
        let r = self.app.update(f);
        self.draw();
        r
    }

    /// Real seconds: file events arrive from another thread.
    pub fn wait_for(&mut self, secs: f32, mut done: impl FnMut(&Ui) -> bool) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
        while std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
            self.app.advance_clock(std::time::Duration::from_millis(400));
            self.draw();
            if done(self) {
                return true;
            }
        }
        false
    }

    pub fn sheet(&self) -> Option<&'static str> {
        use crate::sheets::Sheet;
        self.read(|ws, _| {
            ws.sheet.as_ref().map(|s| match s {
                Sheet::Source => "source",
                Sheet::Export(_) => "export",
                Sheet::Shortcuts => "shortcuts",
                Sheet::Prefs { .. } => "prefs",
                Sheet::ThemeEditor(_) => "theme-editor",
                Sheet::Detached => "detached",
                Sheet::NewProject(_) => "new-project",
                Sheet::ProjectType(_) => "project-type",
            })
        })
    }

    /// A heading's `# ` sits left of the column.
    pub fn heading_shift(&self, line: usize) -> Pixels {
        self.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").x_shift(line).expect("line laid out"))
    }

    pub fn line_font_size(&self, line: usize) -> Pixels {
        self.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").font_size_of(line).expect("line laid out"))
    }

    pub fn ime_mark(&mut self, marked: &str) {
        use gpui_kit::EntityInputHandler;
        let page = self.page();
        self.win.update(|_, window, cx| page.update(cx, |p, cx| p.replace_and_mark_text_in_range(None, marked, None, window, cx)));
        self.draw();
    }

    pub fn ime_commit(&mut self, text: &str) {
        use gpui_kit::EntityInputHandler;
        let page = self.page();
        self.win.update(|_, window, cx| page.update(cx, |p, cx| p.replace_text_in_range(None, text, window, cx)));
        self.draw();
    }

    /// In UTF-16, as the input method sees it.
    pub fn marked(&mut self) -> Option<Range<usize>> {
        use gpui_kit::EntityInputHandler;
        let page = self.page();
        self.win.update(|_, window, cx| page.update(cx, |p, cx| p.marked_text_range(window, cx)))
    }

    pub fn find_state(&self) -> Option<(usize, Option<usize>, usize)> {
        self.read(|ws, cx| ws.find_bar().map(|b| {
            let b = b.read(cx);
            (b.spots.len(), b.current, b.hidden_hits)
        }))
    }

    pub fn set_replacement(&mut self, text: &str) {
        let bar = self.read(|ws, _| ws.find_bar().cloned()).expect("the find bar is open");
        self.win.update(|_, _, cx| {
            let field = bar.read(cx).replacement.clone();
            field.update(cx, |f, cx| f.set_field_text(text, cx));
        });
        self.draw();
    }

    /// Tests have no dictionary by default.
    pub fn use_dictionary(&mut self, words: &str) {
        let list = crate::spell::WordList::new(words);
        self.with_app(|cx| crate::spell::Spelling::set(cx, Box::new(list)));
    }

    pub fn misspelled(&self) -> Vec<Range<usize>> {
        self.read(|ws, cx| ws.page.read(cx).misspelled.to_vec())
    }

    pub fn looked_up(&self) -> Vec<String> {
        crate::platform::LOOKED_UP.with(|l| l.borrow().iter().map(|(t, _)| t.clone()).collect())
    }

    pub fn ghosts(&self) -> Vec<Range<usize>> {
        self.read(|ws, cx| ws.model.read(cx).doc.ghost_ranges())
    }

    pub fn hovered_ghost(&self) -> Option<Range<usize>> {
        self.read(|ws, cx| ws.page.read(cx).hovered_ghost())
    }

    /// (range, target opacity).
    pub fn fades(&self) -> Vec<(Range<usize>, f32)> {
        self.read(|ws, cx| ws.page.read(cx).fades())
    }

    pub fn counter(&self) -> String {
        self.read(|ws, cx| ws.counter(f32::MAX, cx))
    }

    pub fn header_counter(&mut self) -> String {
        self.win.update(|ws, window, cx| {
            ws.counter(crate::workspace::counter_room(window.viewport_size().width.into(), crate::workspace::HeaderEdges::of(window)), cx)
        })
    }

    pub fn header_title(&mut self) -> String {
        self.win.update(|ws, window, cx| {
            let room = crate::workspace::title_room(window.viewport_size().width.into(), crate::workspace::HeaderEdges::of(window));
            crate::workspace::header_title(ws.model.read(cx), room)
        })
    }

    pub fn clipboard(&self) -> Option<String> {
        self.app.read_from_clipboard().and_then(|item| item.text())
    }

    /// The file as saved.
    pub fn source_view(&self) -> Option<String> {
        self.read(|ws, cx| match &ws.sheet {
            Some(crate::sheets::Sheet::Source) => Some(ws.source_text(cx)),
            _ => None,
        })
    }

    pub fn export_form<R>(&self, f: impl FnOnce(&crate::export_ui::ExportForm) -> R) -> Option<R> {
        self.read(|ws, _| match &ws.sheet {
            Some(crate::sheets::Sheet::Export(form)) => Some(f(form)),
            _ => None,
        })
    }

    /// The Export preview waits a moment, then lays out pages off the UI thread.
    pub fn settle(&mut self) {
        for _ in 0..6 {
            self.app.advance_clock(std::time::Duration::from_millis(300));
            self.draw();
        }
    }

    pub fn menu_items(&self) -> Vec<MenuItem> {
        self.read(|ws, cx| {
            let menu = ws.context_menu.as_ref().expect("no context menu is open");
            menu.items
                .iter()
                .flatten()
                .map(|it| MenuItem {
                    label: it.label.to_string(),
                    enabled: it.command.is_some(),
                    badge: it.badge,
                    shortcut: it.shortcut.as_ref().and_then(|a| crate::context_menu::shortcut_label(a.as_ref(), cx)),
                })
                .collect()
        })
    }

    pub fn menu_open(&self) -> bool {
        self.read(|ws, _| ws.context_menu.is_some())
    }

    pub fn bursts(&self) -> Vec<Effects> {
        self.read(|ws, _| ws.overlay.bursts())
    }

    /// `true` = the original's; forgets them.
    pub fn take_tones(&mut self) -> Vec<bool> {
        crate::sound::PLAYED.with(|p| std::mem::take(&mut *p.borrow_mut()))
    }

    pub fn saved(&self) -> String {
        self.read(|ws, cx| crate::format::serialize(ws.model.read(cx)))
    }
}
