use gpui_kit::Modifiers;

use crate::ui_tests::Ui;

const DOC: &str = "# The Garret\n\nHe lodged in S. Place and walked. His garret was a cupboard. The landlady lived below.\n\nAnd each time he passed, he felt sick. He was in debt.\n";

#[test]
fn ed_10_walk_by_sentence() {
    let mut ui = Ui::open(DOC);
    ui.caret_at(ui.find("lodged").start);
    ui.keys("ctrl-cmd-right");
    assert_eq!(ui.selection().start, ui.find("His garret").start, "past “S. Place”");
    ui.keys("ctrl-cmd-right ctrl-cmd-right");
    let and = ui.find("And each").start;
    assert_eq!(ui.selection(), and..and, "into the next paragraph");
    ui.keys("ctrl-cmd-left");
    assert_eq!(ui.selection().start, ui.find("The landlady").start);
    ui.caret_at(ui.find("cupboard").start);
    ui.keys("ctrl-cmd-left");
    assert_eq!(ui.selection().start, ui.find("His garret").start, "the start of this sentence first");

    ui.caret_at(ui.find("His garret").start);
    if cfg!(target_os = "macos") {
        ui.keys("ctrl-shift-cmd-right ctrl-shift-cmd-right");
    } else {
        // No key off the Mac: Ctrl+Alt+Shift+arrows move windows between GNOME's workspaces.
        ui.action(crate::actions::SelectNextSentence);
        ui.action(crate::actions::SelectNextSentence);
    }
    assert_eq!(ui.selection(), ui.find("His garret").start..and);
}

#[test]
fn ed_11_grow_and_shrink() {
    let mut ui = Ui::open(DOC);
    let c = ui.find("garret was").start + 2;
    ui.caret_at(c);
    ui.keys("ctrl-cmd-up");
    assert_eq!(ui.selection(), ui.find("garret was").start..ui.find("garret was").start + 6);
    ui.keys("ctrl-cmd-up");
    assert_eq!(ui.selection(), ui.find("His garret was a cupboard."));
    ui.keys("ctrl-cmd-up");
    let para = ui.find("He lodged in S. Place and walked. His garret was a cupboard. The landlady lived below.");
    assert_eq!(ui.selection(), para);
    ui.keys("ctrl-cmd-up");
    assert_eq!(ui.selection(), para, "stops at the paragraph");
    ui.keys("ctrl-cmd-down ctrl-cmd-down");
    assert_eq!(ui.selection(), ui.find("garret was").start..ui.find("garret was").start + 6);
    ui.keys("ctrl-cmd-down");
    assert_eq!(ui.selection(), c..c, "back where it started");
    ui.keys("ctrl-cmd-down");
    assert_eq!(ui.selection(), c..c, "nothing left to retrace");
}

#[test]
fn ed_11_grow_steps_through_a_group() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("a cupboard", &["a closet"]);
    let r = ui.visible_group(g).range;
    ui.caret_at(ui.find("closet").start + 1);
    ui.keys("ctrl-cmd-up");
    assert_eq!(ui.selection(), ui.find("closet"));
    ui.keys("ctrl-cmd-up");
    assert_eq!(ui.selection(), r);
    ui.keys("ctrl-cmd-up");
    assert_eq!(ui.selection(), ui.find("His garret was a closet."));
}

#[test]
fn ed_11_tap_shift() {
    let mut ui = Ui::open(DOC);
    let c = ui.find("cupboard").start + 2;
    ui.caret_at(c);
    let shift = Modifiers { shift: true, ..Default::default() };
    ui.modifiers(shift);
    ui.modifiers(Modifiers::default());
    assert_eq!(ui.selection(), c..c, "off by default");

    ui.set_prefs(|p| p.tap_shift = true);
    ui.modifiers(shift);
    ui.modifiers(Modifiers::default());
    assert_eq!(ui.selection(), ui.find("cupboard"));
    ui.modifiers(shift);
    ui.modifiers(Modifiers::default());
    assert_eq!(ui.selection(), ui.find("His garret was a cupboard."));

    ui.caret_at(c);
    ui.modifiers(shift);
    ui.keys("shift-right");
    ui.modifiers(Modifiers::default());
    assert_eq!(ui.selection(), c..c + 1, "⇧ with an arrow selects a character, nothing more");
}

#[test]
fn ed_12_move_a_sentence() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("His garret was a cupboard.");
    ui.keys("alt-down");
    assert!(ui.text().contains("walked. The landlady lived below. His garret was a cupboard.\n\nAnd"), "{}", ui.text());
    assert_eq!(ui.selection(), ui.find("His garret was a cupboard."));
    ui.keys("alt-down");
    assert!(ui.text().contains("lived below.\n\nHis garret was a cupboard. And each"), "{}", ui.text());
    assert_eq!(ui.selection(), ui.find("His garret was a cupboard."));
    ui.undo();
    ui.undo();
    assert_eq!(ui.text(), DOC);
    ui.select_text("He lodged in S. Place and walked.");
    ui.keys("alt-up");
    assert_eq!(ui.text(), DOC);
}

#[test]
fn ed_12_move_a_paragraph_with_its_group() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("felt sick", &["felt ill"]);
    ui.select_text("And each time he passed, he felt ill. He was in debt.");
    ui.keys("alt-up");
    assert!(ui.text().starts_with("# The Garret\n\nAnd each time he passed, he felt ill. He was in debt.\n\nHe lodged"), "{}", ui.text());
    assert_eq!(ui.visible_group(g).range, ui.find("felt ill"));
    assert_eq!(ui.variants(g), ["felt sick", "felt ill"]);
}

#[test]
fn ed_12_otherwise_alt_arrows_are_unchanged() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("garret was");
    ui.keys("alt-down");
    assert_eq!(ui.text(), DOC);
    let end = ui.find("lived below.").end;
    assert_eq!(ui.selection(), end..end);

    let mut ui = Ui::open(DOC);
    ui.select_text("His garret was a cupboard.");
    ui.keys("alt-down");
    assert_eq!(ui.text(), DOC, "writing mode doesn't move text");
}
