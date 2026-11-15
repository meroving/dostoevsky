use doc_model::Scope;
use gpui_kit::px;

use crate::actions::ToggleLayer;
use crate::prefs::{Effects, WheelSwitch};
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early. You pick a word and move on.\n\n\
Editors have always known this. They write a suggestion in the margin.\n";

fn starts(ui: &Ui, prefix: &str) -> bool {
    ui.text().starts_with(prefix)
}

#[test]
fn alt_1_layer_toggles_and_is_saved_with_the_document() {
    let mut ui = Ui::open(DOC);
    assert!(!ui.read(|ws, cx| ws.model.read(cx).layer), "documents open for writing");
    let g = ui.make_group("friction", &["tension"]);
    assert!(ui.read(|ws, cx| ws.model.read(cx).layer), "making a group goes to editing (WIN-17)");
    ui.keys("cmd-d");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).layer));
    ui.hover_group(g);
    assert_eq!(ui.hovered_group(), None);
    assert!(!ui.saved().contains(r#""layer""#), "{}", ui.saved());

    ui.keys("cmd-d");
    ui.hover_group(g);
    assert_eq!(ui.hovered_group(), Some(g));
    assert!(ui.saved().contains(r#""layer":true"#), "{}", ui.saved());

    let reopened = Ui::open(&ui.saved());
    assert!(reopened.read(|ws, cx| ws.model.read(cx).layer), "the document remembers the layer is on");
    ui.action(ToggleLayer);
    let reopened = Ui::open(&ui.saved());
    assert!(!reopened.read(|ws, cx| ws.model.read(cx).layer));
}

#[test]
fn alt_6_hover_label() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.hover_group(g);
    assert_eq!(ui.hover_label().as_deref(), Some("version 3 of 3 ›"));
    ui.keys("down");
    assert_eq!(ui.hover_label().as_deref(), Some("original · 1 of 3 ›"));

    ui.set_prefs(|p| p.hover_label = false);
    ui.hover_group(g);
    assert_eq!(ui.hover_label(), None);
}

#[test]
fn alt_6_hover_label_under_the_dots_opens_the_panel() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.keys("escape");
    assert!(!ui.sidebar_open());
    ui.hover_group(g);
    let label = ui.hover_label_bounds().expect("the label shows");
    let (dots, _) = ui.dots(g);
    let word = ui.text_bounds(ui.visible_group(g).range)[0];
    assert!(label.top() > word.top(), "the label is below the word's line top: {label:?} vs {word:?}");
    assert!((label.top() - dots.y).abs() < px(10.), "the label touches the dots: {label:?} vs {dots:?}");

    ui.click(label.center(), gpui_kit::MouseButton::Left);
    assert_eq!(ui.sidebar(), Some(g), "a click on the label opens the panel on the group");
    ui.hover_group(g);
    assert_eq!(ui.hover_label(), None, "the panel already shows this group");
}

#[test]
fn alt_6_hover_label_can_be_reached() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension"]);
    let other = ui.make_group("suggestion", &["note"]);
    ui.keys("escape");
    ui.hover_group(g);
    let label = ui.hover_label_bounds().expect("the label shows");
    let (dots, _) = ui.dots(g);
    ui.move_pointer(dots);
    assert!(ui.hover_label().is_some(), "over the dots");
    ui.move_pointer(label.center());
    assert!(ui.hover_label().is_some(), "over the label");

    ui.hover_group(other);
    assert_eq!(ui.read(|ws, cx| ws.page.read(cx).hover_chip(cx).map(|(g, _)| g.id)), Some(g), "the grace keeps it");
    ui.advance(std::time::Duration::from_millis(350));
    assert_eq!(ui.read(|ws, cx| ws.page.read(cx).hover_chip(cx).map(|(g, _)| g.id)), Some(other));

    ui.unhover();
    assert!(ui.hover_label().is_some(), "still there just after leaving");
    ui.advance(std::time::Duration::from_millis(350));
    assert_eq!(ui.hover_label(), None);
}

#[test]
fn alt_28_selection_buttons_make_a_group() {
    let mut ui = Ui::open_editing(DOC);
    ui.mouse_select("choosing too early");
    assert_eq!(ui.selection_buttons(), Some(ui.find("choosing too early")));
    assert!(ui.has_element("selection-buttons"));
    let bar = ui.element("selection-buttons").bounds();
    let line = ui.text_bounds(ui.find("choosing too early"))[0];
    assert!(bar.bottom() <= line.top(), "above the selection: {bar:?} vs {line:?}");

    ui.click_id("sel-alternatives");
    let g = ui.only_group();
    assert_eq!(ui.sidebar(), Some(g), "the panel opens on the new group");
    assert_eq!(ui.selection_buttons(), None);

    ui.mouse_select("pick a word");
    assert!(ui.selection_buttons().is_some());
    ui.keys("escape");
    assert_eq!(ui.selection_buttons(), None, "Esc hides them");
    ui.mouse_select("move on");
    ui.type_text("x");
    assert_eq!(ui.selection_buttons(), None, "typing hides them");
}

#[test]
fn alt_28_selection_buttons_when_not() {
    let mut ui = Ui::open(DOC);
    ui.mouse_select("choosing too early");
    assert_eq!(ui.selection_buttons(), None, "writing stays a plain page");

    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    assert_eq!(ui.selection_buttons(), None, "a keyboard selection");

    // The same three buttons with AI or without: nothing on the bar needs a model.
    for ai in [false, true] {
        ui.set_prefs(|p| p.ai.enabled = ai);
        ui.caret_at(0);
        ui.mouse_select("choosing too early");
        for id in ["sel-alternatives", "sel-comment", "sel-cut"] {
            assert!(ui.has_element(id), "{id}, AI {ai}");
        }
        assert!(!ui.has_element("sel-ai-ideas") && !ui.has_element("sel-put-back"));
    }

    ui.set_prefs(|p| p.selection_buttons = false);
    assert_eq!(ui.selection_buttons(), None, "turned off");
}

#[test]
fn alt_28_selection_buttons_comment_and_cut() {
    let mut ui = Ui::open_editing(DOC);
    ui.mouse_select("choosing too early");
    ui.click_id("sel-comment");
    let comments = ui.read(|ws, cx| ws.model.read(cx).doc.comments());
    assert_eq!(comments.len(), 1, "a comment, as ⌥⌘M makes");
    assert_eq!(comments[0].range, ui.find("choosing too early"));
    assert!(ui.read(|ws, _| ws.comment_edit.is_some()), "its note opens to type in");
    assert_eq!(ui.selection_buttons(), None);

    ui.keys("escape");
    ui.caret_at(0);
    ui.mouse_select("pick a word");
    ui.click_id("sel-cut");
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.cuts().len()), 1, "a cut, as ⌘/ makes");
    assert_eq!(ui.selection_buttons(), None);

    ui.mouse_select("pick a word");
    assert!(ui.has_element("sel-put-back"), "on cut words, only Put Back");
    assert!(!ui.has_element("sel-alternatives") && !ui.has_element("sel-comment") && !ui.has_element("sel-cut"));
    ui.click_id("sel-put-back");
    assert!(ui.read(|ws, cx| ws.model.read(cx).doc.traces().is_empty()), "put back");
}

#[test]
fn alt_7_hover_arrows_step_both_ways_and_wrap() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.hover_group(g);
    assert_eq!(ui.active(g), 2);
    ui.keys("down");
    assert_eq!(ui.active(g), 0);
    ui.keys("up");
    assert_eq!(ui.active(g), 2);
    ui.keys("up");
    assert_eq!(ui.active(g), 1);
    assert!(starts(&ui, "Most of the tension in writing"));
}

#[test]
fn alt_7_wheel_over_a_group_cycles_it() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    let p = ui.point_for(ui.find("struggle").start + 2);
    let top = ui.point_for(0).y;
    ui.wheel_notch(p, 1.);
    assert_eq!(ui.active(g), 0);
    assert!(starts(&ui, "Most of the friction in writing"));
    ui.wheel_notch(p, -1.);
    assert_eq!(ui.active(g), 2, "wraps back");
    assert_eq!(ui.point_for(0).y, top, "the page did not scroll");

    ui.action(ToggleLayer);
    ui.wheel_notch(p, 1.);
    assert_eq!(ui.active(g), 2, "layer off: the wheel never cycles");
}

#[test]
fn alt_7_wheel_switching_follows_the_setting() {
    let filler = "A line of filler to make the page long enough to scroll.\n\n".repeat(40);
    let mut ui = Ui::open_editing(&format!("Most of the friction in writing.\n\n{filler}"));
    let g = ui.make_group("friction", &["tension", "struggle"]);
    let over = |ui: &Ui| ui.point_for(ui.visible_group(g).range.start + 2);

    ui.set_prefs(|p| p.wheel = WheelSwitch::Off);
    let top = ui.point_for(0).y;
    ui.wheel_notch(over(&ui), 1.);
    assert_eq!(ui.active(g), 2, "off: the wheel did not cycle");
    assert!(ui.point_for(0).y < top, "off: the wheel scrolled the page");
    ui.wheel_notch_alt(over(&ui), 1.);
    assert_eq!(ui.active(g), 2, "off: not with ⌥ either");

    ui.set_prefs(|p| p.wheel = WheelSwitch::WithOption);
    ui.wheel_notch(over(&ui), 1.);
    assert_eq!(ui.active(g), 2, "with ⌥: a plain scroll does not cycle");
    let top = ui.point_for(0).y;
    ui.wheel_notch_alt(over(&ui), 1.);
    assert_eq!(ui.active(g), 0, "with ⌥: ⌥ + scroll cycles");
    assert_eq!(ui.point_for(0).y, top, "and the page stays put");

    ui.set_prefs(|p| p.wheel = WheelSwitch::Always);
    ui.wheel_notch(over(&ui), 1.);
    assert_eq!(ui.active(g), 1, "always: a plain scroll cycles");
}

#[test]
fn alt_7_trackpad_swipe_steps_and_ignores_momentum() {
    use gpui_kit::TouchPhase::{Ended, Moved, Started};
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    let p = ui.point_for(ui.find("struggle").start + 2);
    ui.swipe(p, 0., Started);
    ui.swipe(p, 20., Moved);
    assert_eq!(ui.active(g), 2, "not far enough yet");
    ui.swipe(p, 40., Moved);
    assert_eq!(ui.active(g), 0);
    ui.swipe(p, 0., Ended);
    for _ in 0..5 {
        ui.swipe(p, 60., Moved);
    }
    assert_eq!(ui.active(g), 0, "momentum is swallowed");
}

#[test]
fn alt_7_sideways_swipe_cycles_whatever_the_wheel_setting() {
    use gpui_kit::TouchPhase::{Ended, Moved, Started};
    let filler = "A line of filler to make the page long enough to scroll.\n\n".repeat(40);
    let mut ui = Ui::open_editing(&format!("Most of the friction in writing.\n\n{filler}"));
    let g = ui.make_group("friction", &["tension", "struggle"]);
    let p = ui.point_for(ui.visible_group(g).range.start + 2);
    for wheel in [WheelSwitch::Off, WheelSwitch::WithOption, WheelSwitch::Always] {
        ui.set_prefs(|p| p.wheel = wheel);
        let (before, top) = (ui.active(g), ui.point_for(0).y);
        ui.swipe_sideways(p, 0., 0., Started);
        ui.swipe_sideways(p, 30., 4., Moved);
        ui.swipe_sideways(p, 30., 4., Moved);
        assert_eq!(ui.active(g), (before + 1) % 3, "{wheel:?}: left is the next version");
        assert_eq!(ui.point_for(0).y, top, "{wheel:?}: the page stays put");
        ui.swipe_sideways(p, 0., 0., Ended);
        for _ in 0..5 {
            ui.swipe_sideways(p, 60., 0., Moved);
        }
        assert_eq!(ui.active(g), (before + 1) % 3, "{wheel:?}: momentum is swallowed");
        ui.swipe_sideways(p, 0., 0., Started);
        ui.swipe_sideways(p, -50., 0., Moved);
        assert_eq!(ui.active(g), before, "{wheel:?}: right is the previous one");
        ui.swipe_sideways(p, 0., 0., Ended);
    }

    ui.set_prefs(|p| p.wheel = WheelSwitch::Off);
    let top = ui.point_for(0).y;
    ui.swipe_sideways(p, 0., 0., Started);
    ui.swipe_sideways(p, 4., 30., Moved);
    ui.swipe_sideways(p, 60., 30., Moved);
    assert_eq!(ui.active(g), 2, "a vertical gesture stays vertical");
    assert!(ui.point_for(0).y < top, "and scrolls the page");
    ui.swipe_sideways(p, 0., 0., Ended);

    ui.action(ToggleLayer);
    ui.swipe_sideways(p, 0., 0., Started);
    ui.swipe_sideways(p, 60., 0., Moved);
    assert_eq!(ui.active(g), 2, "layer off: a swipe never cycles");
}

#[test]
fn alt_7_page_scroll_runs_past_groups() {
    use gpui_kit::TouchPhase::{Moved, Started};
    let filler = "A line of filler to make the page long enough to scroll.\n\n".repeat(40);
    let mut ui = Ui::open(&format!("A first line.\n\nMost of the friction in writing.\n\n{filler}"));
    let g = ui.make_group("friction", &["tension"]);
    let away = ui.point_for(ui.find("first").start);
    let top = ui.point_for(0).y;
    ui.swipe(away, 0., Started);
    ui.swipe(away, 30., Moved);
    for _ in 0..5 {
        let over = ui.point_for(ui.visible_group(g).range.start + 2);
        ui.swipe(over, 30., Moved);
    }
    assert_eq!(ui.active(g), 1, "the group did not cycle");
    assert!(ui.point_for(0).y < top - px(100.), "the page scrolled: {:?} → {:?}", top, ui.point_for(0).y);
}

#[test]
fn alt_8_keyboard_only_path() {
    let mut ui = Ui::open(DOC);
    let a = ui.make_group("friction", &["tension"]);
    let b = ui.make_group("suggestion", &["note"]);

    ui.caret_at(0);
    ui.keys("alt-cmd-right");
    assert_eq!(ui.selection(), ui.visible_group(a).range.start..ui.visible_group(a).range.start);
    ui.keys("alt-cmd-down");
    assert_eq!(ui.active(a), 0);
    ui.keys("alt-cmd-up");
    assert_eq!(ui.active(a), 1);
    ui.keys("alt-cmd-0");
    assert_eq!(ui.active(a), 0);

    ui.keys("alt-cmd-right");
    let b_start = ui.visible_group(b).range.start;
    assert_eq!(ui.selection(), b_start..b_start);
    ui.keys("alt-cmd-left");
    let a_start = ui.visible_group(a).range.start;
    assert_eq!(ui.selection(), a_start..a_start);
}

#[test]
fn alt_9_two_tones() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.take_tones();
    ui.hover_group(g);
    ui.keys("down down down");
    assert_eq!(ui.take_tones(), [true, false, false], "original, tension, struggle");

    ui.set_prefs(|p| p.sounds = false);
    ui.keys("down");
    assert_eq!(ui.take_tones(), Vec::<bool>::new());
}

#[test]
fn alt_11_cycling_keeps_the_selection_elsewhere() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.select_text("writing");
    ui.hover_group(g);
    ui.keys("down");
    assert!(starts(&ui, "Most of the friction"));
    assert_eq!(ui.selection(), ui.find("writing"), "selection still on 'writing'");
    ui.keys("down");
    assert_eq!(ui.selection(), ui.find("writing"));
}

#[test]
fn alt_12_scope_detection() {
    let mut ui = Ui::open("# A Title\n\nMost of the friction in writing comes early. You pick a word.\n\nWhole paragraph here.\n");
    let w = ui.make_group("the friction", &[]);
    assert_eq!(ui.group(w).scope, Scope::Word, "up to 4 words with no break inside");
    let short = ui.make_group("You pick a word.", &[]);
    assert_eq!(ui.group(short).scope, Scope::Sentence, "exactly one whole sentence, however short");
    let s = ui.make_group("Most of the friction in writing comes early.", &[]);
    assert_eq!(ui.group(s).scope, Scope::Sentence);
    let p = ui.make_group("Whole paragraph here.", &[]);
    assert_eq!(ui.group(p).scope, Scope::Paragraph);
    let h = ui.make_group("# A Title", &[]);
    assert_eq!(ui.group(h).scope, Scope::Sentence, "headings count as sentences");
    assert_eq!(ui.variants(h), ["A Title"], "the # marker stays outside the group");
}

#[test]
fn alt_12_no_selection_opens_the_sidebar_where_you_are() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start + 2);
    ui.keys("cmd-shift-a");
    assert_eq!(ui.sidebar(), Some(g));
    ui.keys("escape escape");
    assert!(!ui.sidebar_open());

    ui.caret_at(ui.find("Editors").start);
    ui.keys("cmd-shift-a");
    assert_eq!(ui.sidebar(), None, "the list of every group");
}

#[test]
fn alt_13_versions_dim_the_rest_of_the_page() {
    let mut ui = Ui::open(DOC);
    ui.make_group("friction", &["tension"]);
    let g = ui.make_group("early", &["soon"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    assert_eq!(ui.element("group-position").label(), Some("2 of 2"));
    let para = ui.focus_range().expect("the page is dimmed outside the paragraph");
    assert_eq!(para.start, 0);
    assert!(para.end < ui.find("Editors").start, "the second paragraph is dimmed");
    ui.keys("escape");
    assert_eq!(ui.focus_range(), None, "the list leaves the page undimmed");
}

#[test]
fn alt_14_click_or_arrow_in_the_sidebar() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.take_tones();

    ui.click_variant(1);
    assert_eq!(ui.active(g), 1);
    assert!(starts(&ui, "Most of the tension"));
    assert_eq!(ui.take_tones(), [false]);

    ui.keys("down");
    assert_eq!(ui.active(g), 2);
    ui.keys("up up");
    assert_eq!(ui.active(g), 0);
    assert_eq!(ui.take_tones(), [false, false, true]);
}

/// ALT-15
#[test]
fn alt_14_original_label_and_key_hints() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.draw();
    assert!(ui.has_element("original-label"));
    assert!(ui.has_element("sidebar-keys"));
    ui.click_id("settle");
    assert!(ui.groups().is_empty(), "settle, from the footer");
}

#[test]
fn alt_15_escape_returns_focus_to_the_page() {
    let mut ui = Ui::open(DOC);
    ui.make_group("friction", &["tension"]);
    ui.caret_at(0);
    ui.type_text("X");
    assert!(starts(&ui, "XMost of the tension"), "{}", ui.text());
}

#[test]
fn alt_16_cmd_backspace_removes_with_a_burst_and_undo_restores() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.keys("cmd-backspace");
    assert_eq!(ui.variants(g), ["friction", "tension"]);
    assert_eq!(ui.bursts(), [Effects::On]);

    ui.keys("escape");
    ui.undo();
    assert_eq!(ui.variants(g), ["friction", "tension", "struggle"], "⌘Z brings it back in place");
    assert_eq!(ui.active(g), 2);
}

#[test]
fn alt_16_backspace_in_the_empty_field_removes_nothing() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.type_text("ab");
    ui.keys("backspace backspace backspace backspace");
    assert_eq!(ui.variants(g), ["friction", "tension", "struggle"]);
    assert!(ui.bursts().is_empty());

    ui.type_text("grip");
    ui.keys("cmd-backspace");
    assert_eq!(ui.variants(g), ["friction", "tension", "struggle"], "⌘⌫ only cleared the field");
    ui.keys("cmd-backspace");
    assert_eq!(ui.variants(g), ["friction", "tension"]);
}

#[test]
fn alt_16_undo_from_the_sidebar_restores_a_removed_variant() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a cmd-backspace");
    ui.undo();
    assert_eq!(ui.variants(g), ["friction", "tension", "struggle"]);
    assert!(ui.sidebar_open(), "the sidebar stays on the group");
    ui.redo();
    assert_eq!(ui.variants(g), ["friction", "tension"]);
}

#[test]
fn alt_16_burst_becomes_a_fade_without_motion() {
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| p.effects = Effects::Off);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a cmd-backspace");
    assert_eq!(ui.bursts(), [Effects::Off]);
}

#[test]
fn alt_16_removing_the_original_promotes_the_next() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.hover_group(g);
    ui.keys("down");
    assert_eq!(ui.active(g), 0);
    ui.keys("cmd-shift-a cmd-backspace");
    assert_eq!(ui.variants(g), ["tension", "struggle"]);
    assert!(ui.status().is_some_and(|s| s.contains("Original removed")), "{:?}", ui.status());
}

#[test]
fn alt_17_settle_on_this() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("alt-cmd-enter");
    assert!(ui.groups().is_empty());
    assert!(starts(&ui, "Most of the tension"));
    assert!(ui.status().is_some_and(|s| s.starts_with("Settled")));
    ui.undo();
    assert_eq!(ui.variants(ui.only_group()), ["friction", "tension"]);
}

#[test]
fn alt_18_last_alternative_dissolves_the_group() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a cmd-backspace");
    assert!(ui.groups().is_empty());
    assert!(starts(&ui, "Most of the friction"));
    assert!(ui.status().is_some_and(|s| s.starts_with("Removed. ⌘Z")), "{:?}", ui.status());
}

#[test]
fn alt_18_undo_after_the_group_dissolves() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a cmd-backspace escape");
    assert!(!ui.sidebar_open());
    ui.undo();
    assert_eq!(ui.variants(ui.only_group()), ["friction", "tension"]);
}

#[test]
fn alt_19_empty_list_says_how_to_add() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-shift-a");
    assert_eq!(ui.sidebar(), None);
    assert!(ui.has_element("alt-empty"));
    assert!(!ui.has_element("alt-count"), "the Alternatives tab counts nothing yet");
    ui.keys("escape");
    ui.select_text("Editors have always known this. They write a suggestion in the margin.");
    ui.keys("cmd-shift-a");
    let g = ui.sidebar().expect("a paragraph group");
    assert_eq!(ui.visible_group(g).scope, Scope::Paragraph);
}

const LONG: &str = "# One\n\nThe first word here.\n\nFiller line.\n\n# Two\n\nA second word there.\n\nA third word everywhere.\n";

fn long_page() -> (Ui, [doc_model::GroupId; 3]) {
    let mut text = LONG.replace("Filler line.", &"Filler line to push things apart.\n\n".repeat(30));
    text.push_str(&"More filler at the end.\n\n".repeat(30));
    let mut ui = Ui::open(&text);
    ui.set_prefs(|p| p.effects = Effects::Off);
    let a = ui.make_group("first", &["opening"]);
    let b = ui.make_group("second", &["next"]);
    let c = ui.make_group("third", &["last"]);
    (ui, [a, b, c])
}

#[test]
fn alt_27_list_walks_the_page() {
    let (mut ui, [a, b, c]) = long_page();
    ui.caret_at(0);
    ui.action(crate::actions::AllAlternatives);
    assert_eq!(ui.sidebar(), None);
    assert!(ui.has_element(("alt-row", 0usize)) && ui.has_element(("alt-row", 2usize)) && !ui.has_element(("alt-row", 3usize)));
    assert_eq!(ui.element("alt-count").label(), Some("3 in this file"));

    ui.keys("down");
    assert_eq!((ui.sidebar(), ui.sidebar_cursor(), ui.spotlight()), (None, Some(a), Some(a)));
    ui.keys("down");
    assert_eq!(ui.sidebar_cursor(), Some(b));
    assert!(ui.off_centre(b).abs() < 30., "in the middle: {}", ui.off_centre(b));
    ui.keys("down down");
    assert_eq!(ui.sidebar_cursor(), Some(c), "the last row stays put");
    assert!(ui.off_centre(c).abs() < 30., "{}", ui.off_centre(c));
    assert_eq!(ui.focus_range(), None, "nothing is dimmed while you look");
    ui.keys("up");
    assert_eq!(ui.sidebar_cursor(), Some(b));
}

#[test]
fn alt_27_open_and_step_back() {
    let (mut ui, [_, b, c]) = long_page();
    ui.caret_at(0);
    ui.action(crate::actions::AllAlternatives);
    ui.keys("down down enter");
    assert_eq!(ui.sidebar(), Some(b));
    assert!(ui.has_element("to-overview"));
    ui.keys("escape");
    assert_eq!((ui.sidebar(), ui.sidebar_cursor()), (None, Some(b)));
    ui.click_id(("alt-row", 2usize));
    assert_eq!(ui.sidebar(), Some(c));
    assert!(ui.off_centre(c).abs() < 30., "{}", ui.off_centre(c));
    ui.click_id("to-overview");
    assert_eq!(ui.sidebar(), None);
    ui.keys("escape");
    assert!(!ui.sidebar_open());
}

#[test]
fn win_6_cmd_p_toggles_the_panel_in_a_single_file() {
    let (mut ui, _) = long_page();
    ui.keys("cmd-p");
    assert!(ui.has_element("file-panel") && !ui.sidebar_open(), "the Project tab first");
    ui.click_id("switch-alternatives");
    assert!(ui.sidebar_open());
    assert_eq!(ui.sidebar(), None, "the list, not a group");
    ui.keys("cmd-p");
    assert!(!ui.sidebar_open() && !ui.has_element("file-panel"));
    ui.keys("cmd-p");
    assert!(ui.sidebar_open(), "the tab used last");
    ui.keys("cmd-p");
    assert!(!ui.sidebar_open());
}

#[test]
fn alt_27_previous_and_next_group() {
    let (mut ui, [a, b, c]) = long_page();
    ui.caret_at(ui.visible_group(a).range.start);
    ui.keys("cmd-shift-a");
    assert_eq!(ui.element("group-position").label(), Some("1 of 3"));
    ui.keys("alt-down");
    assert_eq!(ui.sidebar(), Some(b));
    assert!(ui.off_centre(b).abs() < 30., "{}", ui.off_centre(b));
    ui.click_id("next-group");
    assert_eq!(ui.sidebar(), Some(c));
    assert_eq!(ui.element("group-position").label(), Some("3 of 3"));
    ui.keys("alt-down");
    assert_eq!(ui.sidebar(), Some(a), "wraps round");
    ui.keys("alt-up");
    assert_eq!(ui.sidebar(), Some(c));
    ui.click_id("prev-group");
    assert_eq!(ui.sidebar(), Some(b));
    assert_eq!(ui.active(c), 1, "stepping changes no group");
}

/// ALT-17
#[test]
fn alt_27_settling_shows_the_list() {
    let (mut ui, [a, b, _]) = long_page();
    ui.caret_at(ui.visible_group(a).range.start);
    ui.keys("cmd-shift-a alt-cmd-enter");
    assert_eq!(ui.sidebar(), None);
    ui.keys("down");
    assert_eq!(ui.sidebar_cursor(), Some(b), "the list has the keyboard");
}

#[test]
fn alt_20_cycling_runs_undo_as_one_step() {
    // Reopened, so the history starts empty and the group is the first thing touched.
    let saved = {
        let mut ui = Ui::open_editing(DOC);
        ui.make_group("friction", &["tension", "struggle"]);
        ui.saved()
    };
    let mut ui = Ui::open(&saved);
    let g = ui.only_group();
    assert_eq!(ui.active(g), 2);
    ui.hover_group(g);
    ui.keys("down down");
    assert_eq!(ui.active(g), 1);
    ui.undo();
    assert_eq!(ui.active(g), 2, "back to where the run started");

    ui.redo();
    ui.hover_group(g);
    ui.keys("down down down");
    ui.undo();
    assert_eq!(ui.active(g), 2, "the full circle left no step of its own");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).doc.can_undo()));
}

#[test]
fn alt_20_browsing_after_adding_a_variant_keeps_it() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.hover_group(g);
    ui.keys("down down");
    ui.undo();
    assert_eq!(ui.variants(g), ["friction", "tension", "struggle"]);
    assert_eq!(ui.active(g), 2);
}

#[test]
fn alt_20_type_cycle_undo() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.hover_group(g);
    ui.keys("up");
    assert_eq!(ui.active(g), 1);
    ui.caret_at(ui.visible_group(g).range.end);
    ui.type_text("s");
    assert!(starts(&ui, "Most of the tensions"));
    ui.hover_group(g);
    ui.keys("down");
    assert!(starts(&ui, "Most of the struggle"));
    ui.undo();
    assert!(starts(&ui, "Most of the tensions"), "{}", ui.text());
    ui.undo();
    assert!(starts(&ui, "Most of the tension "), "{}", ui.text());
}

#[test]
fn alt_21_typing_at_the_edges() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.end);
    ui.type_text("s");
    assert_eq!(ui.variants(g), ["friction", "tensions"]);
    ui.type_text(",");
    assert_eq!(ui.variants(g), ["friction", "tensions"]);
    assert!(starts(&ui, "Most of the tensions, in"));
}

#[test]
fn alt_21_replace_and_delete_a_whole_span() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.select(ui.visible_group(g).range);
    ui.type_text("drag");
    assert_eq!(ui.variants(g), ["friction", "drag"]);

    ui.select(ui.visible_group(g).range);
    ui.keys("backspace");
    assert!(ui.groups().is_empty());
    assert!(starts(&ui, "Most of the  in writing"), "{}", ui.text());
    ui.undo();
    assert_eq!(ui.variants(ui.only_group()), ["friction", "drag"]);
}

#[test]
fn alt_21_return_inside_a_group_keeps_it() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("Editors have always known this. They write a suggestion in the margin.", &[]);
    ui.caret_at(ui.find(" They write").start);
    ui.keys("enter");
    assert_eq!(ui.only_group(), g);
    assert!(ui.variants(g)[0].contains("this.\n\n They write"), "a paragraph break (D6): {:?}", ui.variants(g));
}

#[test]
fn alt_22_nested_groups_follow_their_variant() {
    let mut ui = Ui::open_editing(DOC);
    let p = ui.make_group("Editors have always known this. They write a suggestion in the margin.", &[
        "Editors know this. They leave a note in the margin.",
    ]);
    let w = ui.make_group("note", &["mark"]);
    assert_eq!(ui.visible_group(w).depth, 1);
    ui.hover_group(p);
    ui.keys("down");
    assert_eq!(ui.active(p), 0);
    assert!(!ui.groups().iter().any(|g| g.id == w), "hidden with its variant");
    ui.keys("down");
    assert_eq!(ui.active(w), 1, "and back, as it was");
    assert!(ui.text().contains("They leave a mark in the margin."));
}

#[test]
fn alt_22_partial_overlap_is_explained() {
    let mut ui = Ui::open(DOC);
    ui.make_group("friction in", &["drag on"]);
    let r = ui.find("on writing");
    ui.select(r);
    ui.keys("cmd-shift-a");
    assert_eq!(ui.groups().len(), 1);
    assert_eq!(ui.status().as_deref(), Some("Overlaps another group"));
}

/// CTX-2
#[test]
fn alt_23_make_original_from_the_variant_menu() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.right_click_variant(1);
    assert_eq!(ui.menu_labels(), ["Edit", "Make original", "Copy text", "Remove"]);
    ui.menu("Make original");
    assert_eq!(ui.variants(g)[0], "tension");
}

#[test]
fn alt_26_double_click_edits_a_variant_in_place() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.double_click_variant(2);
    assert_eq!(ui.variant_field(), Some((2, "struggle".into())));
    assert_eq!(ui.active(g), 2, "the first click of the two shows it");

    ui.type_text("s");
    ui.keys("enter");
    assert_eq!(ui.variant_field(), None);
    assert_eq!(ui.variants(g), ["friction", "tension", "struggles"]);
    assert!(starts(&ui, "Most of the struggles"), "{}", ui.text());
    ui.type_text("drag");
    assert_eq!(ui.sidebar_field(), "drag", "typing goes back to the add field");

    ui.click_variant(0);
    ui.double_click_variant(1);
    ui.keys("cmd-a");
    ui.type_text("strain");
    ui.keys("enter");
    assert_eq!(ui.variants(g), ["friction", "strain", "struggles"]);
    ui.undo();
    assert_eq!(ui.variants(g), ["friction", "tension", "struggles"], "one step per edit");
}

#[test]
fn alt_26_escape_or_empty_changes_nothing() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.double_click_variant(0);
    ui.type_text("XYZ");
    ui.keys("escape");
    assert_eq!(ui.variant_field(), None);
    assert!(ui.sidebar_open());
    assert_eq!(ui.variants(g), ["friction", "tension"]);

    ui.double_click_variant(0);
    ui.keys("cmd-a backspace enter");
    assert_eq!(ui.variants(g), ["friction", "tension"]);
}

#[test]
fn alt_26_return_on_empty_field_or_menu_edits() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.keys("enter");
    assert_eq!(ui.variant_field(), Some((1, "tension".into())));
    ui.keys("escape");

    ui.right_click_variant(0);
    ui.menu("Edit");
    assert_eq!(ui.variant_field(), Some((0, "friction".into())));
}

#[test]
fn alt_26_clicking_away_saves() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.double_click_variant(0);
    ui.type_text("s");
    ui.click(ui.point_for(ui.find("move on").start), gpui_kit::MouseButton::Left);
    assert_eq!(ui.variant_field(), None);
    assert_eq!(ui.variants(g), ["frictions", "tension"]);
}

#[test]
fn alt_24_pairings_are_remembered() {
    let mut ui = Ui::open("The friction here.\n\nMore friction there.\n");
    ui.make_group("friction", &["tension"]);
    let r = ui.find("friction there");
    ui.select(r.start..r.start + "friction".len());
    ui.keys("cmd-shift-a");
    let offered = ui.read(|_, cx| crate::memory::Memory::suggestions(cx, "friction", &["friction".into()]));
    assert_eq!(offered, ["tension"]);
}

#[test]
fn alt_25_question_marks_ask_for_ideas() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &[]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");
    ui.type_text("??");
    ui.keys("enter");
    assert!(ui.has_element("ideas-setup"));
    assert_eq!(ui.variants(g), ["friction"]);
}
