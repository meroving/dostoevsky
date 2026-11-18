use std::time::Duration;

use doc_model::{Place, Scope};
use gpui_kit::{point, px, Modifiers, MouseButton, MouseMoveEvent};

use crate::ui_tests::{TempDir, Ui};

const DOC: &str = "Most of the friction in writing comes from choosing too early. You pick a word and move on.\n\n\
Editors have always known this. They write a suggestion in the margin.\n\n\
Software forgot this.\n";

const SENTENCE: &str = "They write a suggestion in the margin.";

/// ED-9
#[test]
fn a_sentence_moves_between_words() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text(SENTENCE);
    let to = ui.point_for(ui.find("You pick").start);
    ui.drag_selection(to, false);
    assert!(ui.text().contains(&format!("too early. {SENTENCE} You pick")), "{}", ui.text());
    assert!(ui.text().contains("known this.\n\nSoftware"), "{}", ui.text());
    assert_eq!(ui.selection(), ui.find(SENTENCE));
    assert_eq!(ui.flash(), Some(ui.find(SENTENCE)));
    ui.undo();
    assert_eq!(ui.text(), DOC);
}

/// ED-9
#[test]
fn the_landing_place_shows_while_dragging() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text(SENTENCE);
    let sel = ui.selection();
    let from = ui.point_for((sel.start + sel.end) / 2);
    let pick = ui.find("pick");
    let to = ui.point_for(pick.start + 1);
    ui.drag_path(from, &[to], false, |ui| {
        assert_eq!(ui.drop_mark(), Some(Place::Inline(pick.start)));
        let carrying = ui.read(|ws, cx| ws.page.read(cx).carrying.clone());
        assert_eq!(carrying, Some(sel.clone()));
    });
    assert_eq!(ui.drop_mark(), None);
    assert!(ui.text().contains(&format!("You {SENTENCE} pick")), "{}", ui.text());
}

/// ED-9
#[test]
fn a_click_in_the_selection_places_the_caret() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text(SENTENCE);
    let c = ui.find("suggestion").start + 2;
    ui.click(ui.point_for(c), MouseButton::Left);
    assert_eq!(ui.selection(), c..c);
    assert_eq!(ui.text(), DOC);
}

/// ED-9
#[test]
fn writing_mode_does_not_drag() {
    let mut ui = Ui::open(DOC);
    ui.select_text(SENTENCE);
    let to = ui.point_for(ui.find("You pick").start);
    ui.drag_selection(to, false);
    assert_eq!(ui.text(), DOC);
    assert_ne!(ui.selection(), ui.find(SENTENCE));
}

/// ED-9
#[test]
fn a_paragraph_moves_between_paragraphs() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("Software forgot this.");
    let to = ui.point_for(ui.find("Most").start);
    ui.drag_selection(to, false);
    assert!(ui.text().starts_with("Software forgot this.\n\nMost of the friction"), "{}", ui.text());
    assert!(ui.text().ends_with("in the margin.\n"), "{}", ui.text());
}

/// ED-9
#[test]
fn alt_drag_copies() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("a suggestion");
    let to = ui.point_for(ui.find("forgot").start);
    ui.drag_selection(to, true);
    assert!(ui.text().contains("write a suggestion in"), "{}", ui.text());
    assert!(ui.text().contains("Software a suggestion forgot this."), "{}", ui.text());
}

/// ED-9
#[test]
fn escape_cancels_a_drag() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text(SENTENCE);
    let sel = ui.selection();
    let from = ui.point_for((sel.start + sel.end) / 2);
    let to = ui.point_for(ui.find("You pick").start);
    ui.drag_path(from, &[to], false, |ui| ui.keys("escape"));
    assert_eq!(ui.text(), DOC);
    assert_eq!(ui.drop_mark(), None);
}

/// CUT-5, ED-9
#[test]
fn drop_on_overflow_stashes() {
    let mut ui = Ui::open_editing(DOC);
    ui.keys("ctrl-cmd-c");
    let drawer = ui.element("overflow").bounds().center();
    ui.select_text("a suggestion");
    ui.drag_selection(drawer, false);
    assert_eq!(ui.notes(), ["a suggestion"]);
    assert!(!ui.text().contains("a suggestion"), "{}", ui.text());

    ui.select_text("Software forgot this.");
    ui.drag_selection(drawer, true);
    assert_eq!(ui.notes(), ["Software forgot this.", "a suggestion"]);
    assert!(ui.text().contains("Software forgot this."));
}

/// CUT-5
#[test]
fn the_drawer_shows_a_slot() {
    let mut ui = Ui::open_editing(DOC);
    ui.keys("ctrl-cmd-c");
    let drawer = ui.element("overflow").bounds().center();
    ui.select_text("a suggestion");
    let sel = ui.selection();
    let from = ui.point_for((sel.start + sel.end) / 2);
    ui.drag_path(from, &[drawer], false, |ui| {
        assert!(ui.has_element("overflow-slot"));
        assert_eq!(ui.drop_mark(), None, "no landing place on the page while over the drawer");
    });
    assert!(!ui.has_element("overflow-slot"));
}

/// Wide enough that the page holds when the drawer opens; writing, alternatives layer on.
fn wide() -> Ui {
    let mut ui = Ui::open_sized(DOC, 1440., 900.);
    ui.action(crate::actions::ToggleLayer);
    ui
}

/// CUT-5
#[test]
fn picking_up_text_opens_the_drawer() {
    let mut ui = wide();
    assert!(!ui.drawer_open());
    let column = ui.column();
    ui.select_text("a suggestion");
    let sel = ui.selection();
    let from = ui.point_for((sel.start + sel.end) / 2);
    let nearby = point(from.x, from.y + px(20.));
    ui.drag_path(from, &[nearby], false, |ui| {
        assert!(ui.drawer_open(), "the drawer opens as the text is picked up");
        assert_eq!(ui.column(), column, "the page stays where it was");
        let into = ui.element("overflow").bounds().center();
        let modifiers = Modifiers::default();
        for k in 1..=4 {
            let t = k as f32 / 4.;
            let position = point(nearby.x + (into.x - nearby.x) * t, nearby.y + (into.y - nearby.y) * t);
            ui.win.simulate_event(MouseMoveEvent { position, modifiers, pressed_button: Some(MouseButton::Left) });
            ui.draw();
        }
        ui.win.simulate_event(gpui_kit::MouseUpEvent { position: into, button: MouseButton::Left, modifiers, click_count: 1 });
        ui.draw();
    });
    assert_eq!(ui.notes(), ["a suggestion"]);
    assert!(ui.drawer_open(), "it stays open to show the scrap");
}

/// CUT-5
#[test]
fn the_drawer_a_drag_opened_closes_after_it() {
    let mut ui = wide();
    ui.select_text(SENTENCE);
    let to = ui.point_for(ui.find("You pick").start);
    ui.drag_selection(to, false);
    assert!(ui.text().contains(&format!("too early. {SENTENCE} You pick")), "{}", ui.text());
    assert!(!ui.drawer_open());

    ui.select_text(SENTENCE);
    let sel = ui.selection();
    let from = ui.point_for((sel.start + sel.end) / 2);
    let to = ui.point_for(ui.find("Software").start);
    ui.drag_path(from, &[to], false, |ui| ui.keys("escape"));
    assert!(!ui.drawer_open());

    ui.keys("ctrl-cmd-c");
    ui.select_text(SENTENCE);
    let to = ui.point_for(ui.find("Software").start);
    ui.drag_selection(to, false);
    assert!(ui.drawer_open());
}

/// CUT-5
#[test]
fn a_narrow_window_keeps_the_drawer_closed() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text(SENTENCE);
    let sel = ui.selection();
    let from = ui.point_for((sel.start + sel.end) / 2);
    let to = ui.point_for(ui.find("You pick").start);
    ui.drag_path(from, &[to], false, |ui| assert!(!ui.drawer_open()));
}

/// CUT-5
#[test]
fn holding_over_the_overflow_button_opens_it() {
    let mut ui = Ui::open_editing(DOC);
    assert!(!ui.drawer_open());
    let button = ui.element("overflow-button").bounds().center();
    ui.select_text("a suggestion");
    let sel = ui.selection();
    let from = ui.point_for((sel.start + sel.end) / 2);
    ui.drag_path(from, &[button], false, |ui| {
        ui.advance(Duration::from_millis(600));
        assert!(ui.drawer_open(), "the drawer opens after half a second");
        let into = ui.element("overflow").bounds().center();
        let modifiers = Modifiers::default();
        for k in 1..=4 {
            let t = k as f32 / 4.;
            let position = point(button.x + (into.x - button.x) * t, button.y + (into.y - button.y) * t);
            ui.win.simulate_event(MouseMoveEvent { position, modifiers, pressed_button: Some(MouseButton::Left) });
            ui.draw();
        }
        ui.win.simulate_event(gpui_kit::MouseUpEvent { position: into, button: MouseButton::Left, modifiers, click_count: 1 });
        ui.draw();
    });
    assert_eq!(ui.notes(), ["a suggestion"]);
}

/// ED-9, WIN-9
#[test]
fn a_drop_keeps_the_chrome() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text(SENTENCE);
    let to = ui.point_for(ui.find("You pick").start);
    ui.drag_selection(to, false);
    assert!(!ui.read(|ws, _| ws.chrome_dimmed()));
    assert!(ui.dirty());
}

/// ALT-23
#[test]
fn dragging_a_variant_reorders_it_and_keeps_the_active_variant() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "unease"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    assert_eq!(ui.variants(g), ["friction", "tension", "unease"]);
    assert_eq!(ui.active(g), 2);

    let from = ui.element(("variant-text", 1usize)).bounds().center();
    let row = ui.element(("variant-text", 2usize)).bounds();
    let to = point(row.center().x, row.bottom() - px(2.));
    ui.drag_path(from, &[to], false, |ui| {
        assert_eq!(ui.read(|ws, _| ws.sidebar.as_ref().and_then(|s| s.drop_at)), Some(3));
        assert!(ui.has_element(("variant-drop", 3usize)));
    });
    assert_eq!(ui.variants(g), ["friction", "unease", "tension"]);
    assert_eq!(ui.active(g), 1);
    assert!(ui.text().starts_with("Most of the unease"), "the same variant stays active");
    assert!(!ui.has_element(("variant-drop", 3usize)));

    ui.undo();
    assert_eq!(ui.variants(g), ["friction", "tension", "unease"]);
    assert_eq!(ui.active(g), 2);
}

/// ALT-23
#[test]
fn the_original_and_a_cancelled_variant_drag_change_nothing() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "unease"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    let before = ui.variants(g);

    let original = ui.element(("variant-text", 0usize)).bounds().center();
    let last = ui.element(("variant-text", 2usize)).bounds();
    let below = point(last.center().x, last.bottom() - px(2.));
    ui.drag_path(original, &[below], false, |_| {});
    assert_eq!(ui.variants(g), before);
    assert_eq!(ui.active(g), 2);

    let from = ui.element(("variant-text", 1usize)).bounds().center();
    ui.drag_path(from, &[below], false, |ui| {
        assert!(ui.has_element(("variant-drop", 3usize)));
        ui.keys("escape");
        assert_eq!(ui.read(|ws, _| ws.sidebar.as_ref().and_then(|s| s.drop_at)), None);
    });
    assert_eq!(ui.variants(g), before);
    assert_eq!(ui.active(g), 2);

    let from = ui.element(("variant-text", 1usize)).bounds().center();
    let elsewhere = ui.point_for(ui.find("Software").start);
    ui.drag_path(from, &[elsewhere], false, |_| {});
    assert_eq!(ui.variants(g), before, "a drop outside the list does nothing");
    assert_eq!(ui.active(g), 2);
}

/// FILE-6
#[test]
fn a_detached_drop_targets_the_groups_scope() {
    let ui = Ui::open_editing(DOC);
    let at = ui.point_for(ui.find("suggestion").start + 2);
    let target = |scope| ui.read(|ws, cx| ws.page.read(cx).drop_span_at(at, scope));
    assert_eq!(target(Scope::Word), Some(ui.find("suggestion")));
    assert_eq!(target(Scope::Sentence), Some(ui.find(SENTENCE)));
    assert_eq!(target(Scope::Paragraph), Some(ui.find("Editors have always known this. They write a suggestion in the margin.")));
}

fn detached_ui(name: &str) -> (TempDir, Ui) {
    let saved = {
        let mut ui = Ui::open(DOC);
        ui.make_group("friction", &["tension"]);
        ui.saved()
    };
    let edited = saved.replacen("Most of the tension in writing comes from choosing too early.", "A different opening line entirely.", 1);
    let dir = TempDir::new(name);
    let mut ui = Ui::open_file(&dir.file("essay.md", &edited));
    ui.click_id("detached-banner");
    assert_eq!(ui.sheet(), Some("detached"));
    (dir, ui)
}

/// FILE-6
#[test]
fn dragging_a_detached_group_onto_text_attaches_it() {
    let (_dir, mut ui) = detached_ui("drag-detached");
    let from = ui.element(("detached", 0usize)).bounds().center();
    let target = ui.find("different");
    let to = ui.point_for(target.start + 2);
    ui.drag_path(from, &[to], false, |ui| {
        assert_eq!(ui.sheet(), None, "picking the group up closes the sheet");
        assert_eq!(ui.read(|ws, cx| ws.page.read(cx).drop_span.clone()), Some(target.clone()));
    });
    assert_eq!(ui.groups().len(), 1);
    let g = ui.only_group();
    assert_eq!(ui.group(g).scope, Scope::Word);
    assert_eq!(ui.variants(g), ["different", "friction", "tension"]);
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).kept.detached.len()), 0);
    assert_eq!(ui.sheet(), None, "the last attached group leaves the page open");
}

/// FILE-6
#[test]
fn cancelling_a_detached_drag_brings_the_sheet_back() {
    let (_dir, mut ui) = detached_ui("cancel-detached-drag");
    let from = ui.element(("detached", 0usize)).bounds().center();
    let to = ui.point_for(ui.find("different").start + 2);
    ui.drag_path(from, &[to], false, |ui| {
        assert!(ui.read(|ws, cx| ws.page.read(cx).drop_span.is_some()));
        ui.keys("escape");
        assert_eq!(ui.sheet(), Some("detached"));
        assert!(ui.read(|ws, cx| ws.page.read(cx).drop_span.is_none()));
    });
    assert!(ui.groups().is_empty());
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).kept.detached.len()), 1);
    assert_eq!(ui.sheet(), Some("detached"));
}

/// FILE-6
#[test]
fn a_detached_drop_returns_to_the_remaining_groups() {
    let saved = {
        let mut ui = Ui::open(DOC);
        ui.make_group("friction", &["tension"]);
        ui.make_group("writing", &["editing"]);
        ui.saved()
    };
    let edited = saved.replacen("Most of the tension in editing comes from choosing too early.", "A different opening line entirely.", 1);
    let dir = TempDir::new("drag-detached-remaining");
    let mut ui = Ui::open_file(&dir.file("essay.md", &edited));
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).kept.detached.len()), 2);
    ui.click_id("detached-banner");
    let from = ui.element(("detached", 0usize)).bounds().center();
    let to = ui.point_for(ui.find("different").start + 2);
    ui.drag_path(from, &[to], false, |_| {});
    assert_eq!(ui.groups().len(), 1);
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).kept.detached.len()), 1);
    assert_eq!(ui.sheet(), Some("detached"));
    assert!(ui.has_element(("detached", 0usize)));
}
