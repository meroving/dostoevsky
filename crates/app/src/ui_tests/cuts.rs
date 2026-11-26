use crate::ui_tests::{Ui, key_label};

const DOC: &str = "Most of the friction in writing comes from choosing too early. You pick a word and move on.\n\n\
Editors have always known this. They write a suggestion in the margin.\n";

#[test]
fn cut_1_cmd_x_fades_in_place() {
    let mut ui = Ui::open_editing(DOC);
    let r = ui.find("choosing too early");
    ui.select_text("choosing too early");
    ui.keys("cmd-x");
    assert_eq!(ui.text(), DOC, "the page doesn't reflow");
    assert_eq!(ui.ghosts(), std::slice::from_ref(&r));
    assert!(ui.fades().iter().any(|(f, _)| *f == r), "{:?}", ui.fades());
    assert_eq!(ui.clipboard().as_deref(), Some("choosing too early"));
    let status = format!("Cut · {} moves it · esc puts it back", key_label("⌘V"));
    assert_eq!(ui.status().as_deref(), Some(status.as_str()));
    assert_eq!(ui.selection(), r.end..r.end);
    assert!(ui.read(|ws, cx| ws.model.read(cx).doc.clean_text()).contains("comes from. You"));
    ui.keys("right");
    assert_eq!(ui.status(), None);
}

#[test]
fn cut_2_cmd_v_moves_then_copies() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-x");
    let at = ui.find("margin").start;
    ui.caret_at(at);
    ui.keys("cmd-v");
    assert!(ui.text().contains("in the choosing too early margin"), "{}", ui.text());
    assert!(ui.text().contains("comes from. You"), "the trace went and its space was tidied: {}", ui.text());
    assert!(ui.ghosts().is_empty());
    let status = format!("Moved · {} puts it back", key_label("⌘Z"));
    assert_eq!(ui.status().as_deref(), Some(status.as_str()));
    assert!(ui.read(|ws, cx| ws.model.read(cx).doc.cuts()).is_empty(), "the cut left the panel");

    ui.keys("cmd-v");
    assert_eq!(ui.text().matches("choosing too early").count(), 2, "a second ⌘V pastes a copy");
    ui.keys("cmd-z");
    ui.keys("cmd-z");
    assert_eq!(ui.text(), DOC, "one ⌘Z undid the move");
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.cuts()).len(), 1);
}

#[test]
fn cut_2_a_later_copy_wins() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-x");
    ui.select_text("Editors");
    ui.keys("cmd-c");
    ui.caret_at(0);
    ui.keys("cmd-v");
    assert!(ui.text().starts_with("EditorsMost"), "{}", ui.text());
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.cuts()).len(), 1, "the cut stays");
}

#[test]
fn cut_3_esc_straight_after_puts_it_back() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-x");
    ui.keys("escape");
    assert!(ui.ghosts().is_empty());
    assert!(ui.read(|ws, cx| ws.model.read(cx).doc.cuts()).is_empty());
    assert_eq!(ui.text(), DOC);

    ui.select_text("choosing too early");
    ui.keys("cmd-x");
    ui.keys("left");
    ui.keys("escape");
    assert_eq!(ui.ghosts().len(), 1, "too late: the cut stays");
}

#[test]
fn cut_4_cmd_slash_cuts_and_puts_back() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("Editors");
    ui.keys("cmd-c");
    let r = ui.find("choosing too early");
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    assert_eq!(ui.ghosts(), std::slice::from_ref(&r));
    assert_eq!(ui.clipboard().as_deref(), Some("Editors"), "the clipboard is untouched");
    ui.caret_at(r.start + 3);
    ui.keys("cmd-/");
    assert!(ui.ghosts().is_empty());
    assert!(ui.fades().contains(&(r, 1.0)), "{:?}", ui.fades());
    ui.keys("cmd-z");
    assert_eq!(ui.ghosts().len(), 1, "putting back is one ⌘Z");
}

/// WIN-4
#[test]
fn cut_6_traces_look_the_same_in_both_modes() {
    let mut ui = Ui::open_editing(DOC);
    let r = ui.find("choosing too early");
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    let editing = (ui.text(), ui.ghosts(), ui.point_for(ui.find("Editors").start));
    ui.keys("cmd-d");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).layer), "writing");
    assert_eq!((ui.text(), ui.ghosts(), ui.point_for(ui.find("Editors").start)), editing, "nothing moves");
    let e = ui.find("Editors");
    ui.select_text("Editors");
    ui.keys("cmd-x");
    assert_eq!(ui.ghosts(), [r, e], "a cut made in writing fades in place");
    let writing = (ui.text(), ui.ghosts());
    ui.keys("cmd-d");
    assert_eq!((ui.text(), ui.ghosts()), writing, "back in editing nothing changes");
}

#[test]
fn cut_13_saved_and_reopened() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-x");
    let saved = ui.saved();
    let body = &saved[..saved.find("<!-- dostoevsky:").unwrap()];
    assert!(!body.contains("choosing"), "{saved}");
    let reopened = Ui::open(&saved);
    assert!(reopened.read(|ws, cx| ws.model.read(cx).layer), "saved in editing");
    assert_eq!(reopened.text(), DOC);
    assert_eq!(reopened.ghosts().len(), 1);
}

#[test]
fn ctx_1_cut_items() {
    let mut ui = Ui::open_editing(DOC);
    ui.right_click_text("friction");
    let labels = ui.menu_labels();
    let cut = labels.iter().position(|l| l == "Cut").expect("Cut");
    assert_eq!(labels[cut + 1], "Cut Without a Trace", "{labels:?}");
    assert!(!labels.iter().any(|l| l.contains("Ghost") || l.contains("Drawer")), "{labels:?}");
    ui.menu("Cut");
    assert_eq!(ui.ghosts(), [ui.find("friction")]);

    ui.right_click_text("friction");
    assert!(ui.menu_labels().iter().any(|l| l == "Put Back"), "{:?}", ui.menu_labels());
    ui.menu("Put Back");
    assert!(ui.ghosts().is_empty());

    let mut ui = Ui::open(DOC);
    ui.right_click_text("friction");
    let labels = ui.menu_labels();
    assert!(labels.iter().any(|l| l == "Cut") && !labels.iter().any(|l| l == "Cut Without a Trace" || l == "Put Back"), "{labels:?}");
}

#[test]
fn cut_9_this_page_in_text_order_then_no_trace() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("They write a suggestion in the margin.");
    ui.keys("cmd-shift-x");
    ui.select_text("You pick a word");
    ui.keys("cmd-/");
    ui.select_text("friction");
    ui.keys("cmd-/");
    assert_eq!(ui.notes(), ["friction", "You pick a word", "They write a suggestion in the margin."]);
    assert_eq!(ui.chip(0).as_deref(), Some("¶1"));
    assert_eq!(ui.chip(2).as_deref(), Some("from ¶2"));
    assert!(ui.cut_line(0).ends_with("faded · just now"), "{}", ui.cut_line(0));
    assert!(ui.cut_line(2).ends_with("no trace"), "{}", ui.cut_line(2));
    assert_eq!(ui.no_trace(), ["They write a suggestion in the margin."]);
    assert_eq!(ui.element("overflow-button").label(), Some("Cuts, 3 cuts"));
}

/// ED-9
#[test]
fn cut_10_drag_moves_and_alt_copies() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui.keys("ctrl-cmd-c");
    let to = ui.point_for(ui.find("margin").start);
    ui.drag_note(0, to, true);
    assert!(ui.text().contains("in the choosing too early margin"), "{}", ui.text());
    assert_eq!(ui.notes(), ["choosing too early"], "⌥ keeps the cut");
    let to = ui.point_for(ui.find("Editors").start);
    ui.drag_note(0, to, false);
    assert!(ui.notes().is_empty());
    assert!(ui.ghosts().is_empty(), "its trace went with it");
    assert!(ui.text().contains("choosing too early Editors"), "{}", ui.text());
    ui.undo();
    assert_eq!(ui.notes(), ["choosing too early"], "one ⌘Z brings it back");
}

#[test]
fn cut_10_a_cuts_menu() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui.select_text("They write a suggestion in the margin.");
    ui.keys("cmd-shift-x");
    ui.caret_at(0);
    ui.cut_menu(0, "Copy");
    assert_eq!(ui.clipboard().as_deref(), Some("choosing too early"));
    ui.cut_menu(0, "Go to Its Place");
    assert_eq!(ui.flash(), Some(ui.find("choosing too early")));
    let b = ui.read(|ws, _| ws.drawer.as_ref().unwrap().card_bounds()[1]);
    ui.click(b.center(), gpui_kit::MouseButton::Right);
    assert!(!ui.menu_labels().iter().any(|l| l == "Put Back"), "{:?}", ui.menu_labels());
    ui.keys("escape");
    ui.cut_menu(0, "Put Back");
    assert!(ui.ghosts().is_empty());
    assert!(ui.read(|ws, cx| ws.model.read(cx).doc.clean_text()).contains("comes from choosing too early."));
    assert_eq!(ui.notes(), ["They write a suggestion in the margin."]);
    ui.cut_menu(0, "Delete for Good");
    assert!(ui.notes().is_empty());
    ui.undo();
    assert_eq!(ui.notes(), ["They write a suggestion in the margin."]);
}

#[test]
fn cut_6_show_cuts_as_a_mark() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui.select_text("Editors have always known this. They write a suggestion in the margin.");
    ui.keys("cmd-/");
    let step = ui.read(|ws, cx| ws.model.read(cx).doc.undo_tag());
    ui.set_prefs(|p| p.cut_look = crate::prefs::CutLook::Mark);
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.undo_tag()), step, "not an edit");
    assert!(ui.ghosts().is_empty());
    assert!(!ui.text().contains("choosing too early"), "{}", ui.text());
    let marks = ui.marks();
    assert_eq!(marks.len(), 2, "a hairline and a margin rule: {marks:?}");
    assert!(marks[0].size.height > marks[0].size.width, "inline: a hairline");
    assert!(marks[1].size.width > marks[1].size.height, "paragraphs: a rule in the margin");
    ui.set_prefs(|p| p.cut_look = crate::prefs::CutLook::Text);
    assert_eq!(ui.ghosts().len(), 2);
    assert!(ui.marks().is_empty());
}

#[test]
fn cut_6_marks_show_in_writing_too() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui.set_prefs(|p| p.cut_look = crate::prefs::CutLook::Mark);
    let marks = ui.marks();
    assert_eq!(marks.len(), 1);
    ui.keys("cmd-d");
    assert_eq!(ui.marks(), marks, "the same mark in writing");

    let e = ui.find("Editors");
    ui.select_text("Editors");
    ui.keys("cmd-x");
    assert_eq!(ui.ghosts(), std::slice::from_ref(&e), "fresh: faded whatever the setting");
    ui.keys("cmd-d");
    assert_eq!(ui.ghosts(), [e], "still faded after ⌘E");
    ui.set_prefs(|p| p.cut_look = crate::prefs::CutLook::Mark);
    assert_eq!(ui.marks().len(), 1, "unchanged setting: nothing folds");
}

#[test]
fn cut_6_a_mark_peeks_at_its_text() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui.set_prefs(|p| p.cut_look = crate::prefs::CutLook::Mark);
    let m = ui.marks()[0];
    ui.move_pointer(m.center());
    assert!(ui.has_element("mark-peek"));
    ui.click_id("peek-show-text");
    assert_eq!(ui.ghosts().len(), 1, "this cut shows its text again");
    assert!(!ui.has_element("mark-peek"));
    let at = ui.find("choosing").start + 2;
    ui.caret_at(at);
    let said = ui.read(|ws, cx| ws.page.read(cx).a11y_description(cx).to_string());
    assert!(said.contains("cut: choosing too early"), "{said}");
}

#[test]
fn cut_7_a_traces_menu() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui.right_click_text("too early");
    let labels = ui.menu_labels();
    for l in ["Put Back", "✓ Show the Text", "Show in Cuts", "Forget This Spot", "Delete for Good"] {
        assert!(labels.iter().any(|x| x.trim() == l), "{l} in {labels:?}");
    }
    ui.menu("   Show as a Mark");
    assert!(ui.ghosts().is_empty());
    assert_eq!(ui.marks().len(), 1);
    let m = ui.marks()[0];
    ui.click(m.center(), gpui_kit::MouseButton::Left);
    assert!(ui.menu_labels().iter().any(|l| l == "✓ Show as a Mark"), "{:?}", ui.menu_labels());
    ui.menu("Show in Cuts");
    assert!(ui.drawer_open());
    // The page made room for the panel: the mark moved with it.
    let m = ui.marks()[0];
    ui.click(m.center(), gpui_kit::MouseButton::Left);
    ui.menu("Forget This Spot");
    assert!(ui.marks().is_empty());
    assert_eq!(ui.no_trace(), ["choosing too early"]);
    ui.undo();
    assert_eq!(ui.marks().len(), 1);
    let m = ui.marks()[0];
    ui.click(m.center(), gpui_kit::MouseButton::Left);
    ui.menu("Delete for Good");
    assert!(ui.notes().is_empty());
    ui.undo();
    assert_eq!(ui.notes(), ["choosing too early"]);
}

#[test]
fn cut_8_editing_a_faded_cut_in_place() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    let r = ui.find("choosing too early");
    ui.caret_at(r.start + 8);
    assert_eq!(ui.read_window(|ws, w, cx| ws.page.read(cx).cut_being_edited(w)), Some(r.clone()), "bright, with a wash");
    ui.type_text("X");
    assert_eq!(ui.notes(), ["choosingX too early"]);
    assert!(!ui.read(|ws, cx| ws.model.read(cx).doc.clean_text()).contains("choosingX"), "it is the cut's text, not the page's");
    let r = ui.find("choosingX too early");
    ui.caret_at(r.end);
    assert_eq!(ui.read_window(|ws, w, cx| ws.page.read(cx).cut_being_edited(w)), None);
    ui.type_text("Y");
    assert!(ui.read(|ws, cx| ws.model.read(cx).doc.clean_text()).contains("Y"), "at its edge it goes into the page");
    assert_eq!(ui.notes(), ["choosingX too early"]);
}

const DOUBLED: &str = "You pick a word, the the sentence moves on.\n\nAnd so on.\n";

/// The second `the`, as a word group with its panel open.
fn doubled(ui: &mut Ui) -> doc_model::GroupId {
    let at = ui.text().match_indices("the").nth(1).unwrap().0;
    ui.select(at..at + 3);
    ui.keys("cmd-shift-a");
    ui.sidebar().expect("the group is open")
}

/// ALT-29
#[test]
fn a_version_without_the_words_is_a_cut_while_it_shows() {
    let mut ui = Ui::open_editing(DOUBLED);
    let g = doubled(&mut ui);
    ui.right_click_variant(0);
    assert!(ui.menu_labels().iter().any(|l| l == "Add a version without it"), "{:?}", ui.menu_labels());
    ui.menu("Add a version without it");
    let info = ui.group(g);
    assert_eq!((info.variants.len(), info.active), (2, 1), "added and shown");
    assert!(info.variants[1].cut);
    assert_eq!(ui.element(gpui_kit::ElementId::NamedInteger("variant-text".into(), 1)).label(), Some("without “the”"));
    // Shown, it is a cut: faded, left out of the final text, listed as a version.
    assert_eq!(ui.ghosts(), [ui.visible_group(g).range]);
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.clean_text()), "You pick a word, the sentence moves on.\n\nAnd so on.\n");
    assert_eq!(ui.notes(), ["the"]);
    // No second one, no editing it, never the original.
    ui.right_click_variant(0);
    assert!(!ui.menu_labels().iter().any(|l| l == "Add a version without it"));
    ui.menu("Copy text");
    ui.right_click_variant(1);
    assert_eq!(ui.menu_labels(), ["Copy text", "Remove"]);
    ui.menu("Copy text");
    ui.double_click_variant(1);
    assert_eq!(ui.variant_field(), None);
    // Another version: out of the panel.
    ui.click_variant(0);
    assert!(ui.notes().is_empty(), "{:?}", ui.notes());
    assert!(ui.ghosts().is_empty());
    ui.draw();
    assert_eq!(ui.element("overflow-button").label(), Some("Cuts"));
    ui.click_variant(1);
    assert_eq!(ui.notes(), ["the"]);
    ui.keys("ctrl-cmd-c");
    assert!(ui.cut_line(0).contains("a version"), "{}", ui.cut_line(0));
    assert_eq!(ui.element("overflow-button").label(), Some("Cuts, 1 cut"));
    // As a mark it takes no room, and the dots stay where the word was.
    ui.set_prefs(|p| p.cut_look = crate::prefs::CutLook::Mark);
    assert!(ui.visible_group(g).range.is_empty());
    assert_eq!(ui.marks().len(), 1);
    ui.click_dots(g);
    ui.set_prefs(|p| p.cut_look = crate::prefs::CutLook::Text);
}

/// ALT-29
#[test]
fn a_version_without_the_words_puts_back_the_original_and_settles_to_a_cut() {
    let mut ui = Ui::open_editing(DOUBLED);
    let g = doubled(&mut ui);
    ui.right_click_variant(0);
    ui.menu("Add a version without it");
    // Its own menu: no spot to forget; Put Back shows the original.
    let r = ui.visible_group(g).range;
    let p = ui.point_for(r.start + 1);
    ui.click(p, gpui_kit::MouseButton::Right);
    let labels = ui.menu_labels();
    assert!(labels.iter().any(|l| l == "Put Back"), "{labels:?}");
    assert!(!labels.iter().any(|l| l == "Forget This Spot"), "{labels:?}");
    ui.menu("Put Back");
    assert_eq!(ui.active(g), 0);
    assert_eq!(ui.group(g).variants.len(), 2, "the version stays");
    // Settled while it shows: an ordinary cut.
    ui.click_variant(1);
    ui.keys("alt-cmd-enter");
    assert!(ui.groups().is_empty());
    assert_eq!(ui.notes(), ["the"]);
    ui.keys("ctrl-cmd-c");
    assert!(!ui.cut_line(0).contains("a version"), "{}", ui.cut_line(0));
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.clean_text()), "You pick a word, the sentence moves on.\n\nAnd so on.\n");
    ui.undo();
    assert_eq!(ui.active(g), 1);
}
