use crate::actions::ToggleLayer;
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n";

/// ALT-12, ALT-15, ALT-7
#[test]
fn make_a_group_and_cycle_it_by_hovering() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    let g = ui.only_group();
    assert!(ui.sidebar_open());

    ui.type_text("tension");
    ui.keys("enter");
    assert_eq!(ui.variants(g), ["friction", "tension"]);
    assert_eq!(ui.active(g), 1);
    assert!(ui.text().starts_with("Most of the tension in writing"));

    ui.keys("escape escape");
    assert!(!ui.sidebar_open());
    ui.hover_group(g);
    assert_eq!(ui.hovered_group(), Some(g));

    ui.keys("down");
    assert_eq!(ui.active(g), 0, "wraps back to the original");
    assert!(ui.text().starts_with("Most of the friction in writing"));

    ui.keys("down");
    assert_eq!(ui.active(g), 1);
}

/// ALT-7
#[test]
fn arrows_move_the_caret_when_not_hovering_a_group() {
    let mut ui = Ui::open("Most of the friction in writing.\n\nA second paragraph.\n");
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    let g = ui.only_group();
    ui.type_text("tension");
    ui.keys("enter escape escape");

    ui.select(0..0);
    ui.unhover();
    assert_eq!(ui.hovered_group(), None);
    ui.keys("down");
    assert_eq!(ui.active(g), 1, "the group did not cycle");
    assert!(ui.selection().start > 0, "the caret moved down");
}

/// ALT-1
#[test]
fn layer_off_arrows_never_cycle() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    let g = ui.only_group();
    ui.type_text("tension");
    ui.keys("enter escape");

    ui.action(ToggleLayer);
    ui.hover_group(g);
    assert_eq!(ui.hovered_group(), None);
    ui.keys("down");
    assert_eq!(ui.active(g), 1);
}
