use crate::prefs::Effects;
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early. You pick a word and move on.\n\n\
Editors have always known this. They write a suggestion in the margin.\n";

const SENTENCE: &str = "They write a suggestion in the margin.";

fn stash(ui: &mut Ui, needle: &str) {
    ui.select_text(needle);
    ui.keys("cmd-shift-x");
}

#[test]
fn cut_9_toggle_the_drawer() {
    let mut ui = Ui::open(DOC);
    ui.keys("ctrl-cmd-c");
    assert!(ui.drawer_open());
    ui.keys("ctrl-cmd-c");
    assert!(!ui.drawer_open());
    ui.caret_at(0);
    ui.type_text("X");
    assert!(ui.text().starts_with("XMost"));
}

#[test]
fn cut_9_the_panel_names_its_file_and_counts_cuts() {
    let mut ui = Ui::open(DOC);
    ui.keys("ctrl-cmd-c");
    assert_eq!(ui.element("overflow-whose").label(), Some("untitled · 0 cuts"));
    assert!(ui.has_element("cuts-empty"));
    assert!(!ui.has_element("new-note"), "nothing is typed into the panel");
    assert!(!ui.has_element("cuts-scope"), "a single file has no scope switch");
    stash(&mut ui, SENTENCE);
    assert_eq!(ui.element("overflow-whose").label(), Some("untitled · 1 cut"));
    assert!(!ui.has_element("cuts-empty"));
}

#[test]
fn cut_9_double_click_a_note_to_edit_it() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, SENTENCE);
    ui.click_note(0);
    assert_eq!((ui.selected_note(), ui.editing_note()), (Some(0), None), "one click only selects");
    ui.double_click_note(0);
    assert_eq!(ui.editing_note(), Some(0));
    ui.keys("cmd-down");
    ui.type_text(" Always.");
    assert_eq!(ui.notes(), [format!("{SENTENCE} Always.")]);
}

#[test]
fn cut_9_edit_from_the_menu() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, SENTENCE);
    ui.cut_menu(0, "Edit Text");
    assert_eq!(ui.editing_note(), Some(0));
    ui.keys("cmd-down");
    ui.type_text("!");
    assert_eq!(ui.notes(), [format!("{SENTENCE}!")]);
}

#[test]
fn cut_9_click_a_note_to_see_its_place() {
    let paras: Vec<String> = (1..=60).map(|i| format!("Paragraph {i} goes on for a while so the page is long.")).collect();
    let doc = paras.join("\n\n") + "\n";
    let mut ui = Ui::open_editing(&doc);
    ui.select_text("Paragraph 45 goes");
    ui.keys("cmd-x");
    let at = ui.find("Paragraph 45 goes").start;
    ui.keys("ctrl-cmd-c");
    ui.caret_at(0);
    ui.draw();
    assert!(ui.off_centre_at(0) < 0., "the page is at the top");
    ui.click_note(0);
    assert!(ui.off_centre_at(at).abs() < 30., "its faded text is in the middle: {}", ui.off_centre_at(at));
    assert_eq!(ui.selection(), 0..0, "the caret stays");
    assert_eq!(ui.editing_note(), None);
}

#[test]
fn cut_5_stash_moves_text_to_a_new_top_note() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, SENTENCE);
    assert!(!ui.text().contains(SENTENCE));
    assert!(ui.drawer_open());
    assert_eq!(ui.notes(), [SENTENCE]);
    assert_eq!(ui.flights(), [Effects::On]);

    stash(&mut ui, "choosing too early");
    assert_eq!(ui.notes(), ["choosing too early", SENTENCE]);
}

#[test]
fn cut_5_no_flight_without_motion_and_nothing_to_stash() {
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| p.effects = Effects::Off);
    stash(&mut ui, SENTENCE);
    assert_eq!(ui.notes().len(), 1);
    assert!(ui.flights().is_empty());

    ui.caret_at(0);
    ui.keys("cmd-shift-x");
    assert_eq!(ui.notes().len(), 1);
    assert_eq!(ui.status().as_deref(), Some("Select some text to cut it"));
}

#[test]
fn cut_10_cmd_enter_puts_a_note_back_at_the_caret() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, SENTENCE);
    ui.caret_at(0);
    ui.click_note(0);
    ui.keys("cmd-enter");
    assert!(ui.text().starts_with(&format!("{SENTENCE} Most of the friction")), "{}", ui.text());
    assert!(ui.notes().is_empty());
    let back = 0..SENTENCE.chars().count();
    assert_eq!(ui.selection(), back.clone());
    assert_eq!(ui.flash(), Some(back));
    ui.type_text("Z");
    assert!(ui.text().starts_with("Z Most"), "focus is back on the page: {}", ui.text());
}

#[test]
fn cut_10_drag_a_note_onto_the_page() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, "choosing too early");
    let to = ui.point_for(ui.find("Editors").start + 3);
    ui.drag_note(0, to, false);
    assert!(ui.notes().is_empty());
    assert!(ui.text().contains("\n\nchoosing too early Editors have"), "{}", ui.text());
    assert_eq!(ui.selection(), ui.find("choosing too early"));
}

#[test]
fn cut_10_alt_drag_copies() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, "choosing too early");
    let to = ui.point_for(ui.find("Editors").start + 3);
    ui.drag_note(0, to, true);
    assert_eq!(ui.notes(), ["choosing too early"]);
    assert!(ui.text().contains("choosing too early Editors have"), "{}", ui.text());
}

#[test]
fn cut_10_a_paragraph_scrap_goes_between_paragraphs() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, "Editors have always known this. They write a suggestion in the margin.");
    let to = ui.point_for(ui.find("choosing").start);
    ui.drag_note(0, to, false);
    assert!(ui.text().starts_with("Editors have always known this. They write a suggestion in the margin.\n\nMost of"), "{}", ui.text());
}

#[test]
fn cut_10_drag_a_note_into_a_paragraph_gap() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, "choosing too early");
    let gap = ui.find("Editors").start - 1;
    let to = ui.point_for(gap);
    ui.drag_note(0, to, false);
    assert!(ui.text().contains("move on.\n\nchoosing too early\n\nEditors have"), "{}", ui.text());
}

#[test]
fn cut_11_origin_link() {
    let mut ui = Ui::open(DOC);
    stash(&mut ui, SENTENCE);
    assert_eq!(ui.origin_number(0), Some(2));

    ui.caret_at(0);
    ui.type_text("A new first paragraph.");
    ui.keys("enter enter");
    assert_eq!(ui.origin_number(0), Some(3), "the link follows its paragraph");

    ui.click_id(("origin", 0usize));
    let editors = ui.find("Editors have always known this.");
    let flashed = ui.flash().expect("the paragraph flashes");
    assert_eq!(flashed.start, editors.start);

    let reopened = Ui::open(&ui.saved());
    assert_eq!(reopened.origin_number(0), Some(3));
}

#[test]
fn cut_12_groups_travel_with_the_note() {
    let mut ui = Ui::open(DOC);
    ui.make_group("suggestion", &["note"]);
    stash(&mut ui, "They write a note in the margin.");
    assert!(ui.groups().is_empty());
    assert_eq!(ui.notes(), ["They write a note in the margin."]);

    ui.caret_at(0);
    ui.click_note(0);
    ui.keys("cmd-enter");
    let g = ui.only_group();
    assert_eq!(ui.variants(g), ["suggestion", "note"]);
    assert_eq!(ui.active(g), 1);
}

#[test]
fn cut_14_saved_and_counted_apart() {
    let mut ui = Ui::open(DOC);
    assert!(ui.counter().starts_with("30 words"), "{}", ui.counter());
    stash(&mut ui, SENTENCE);
    ui.caret_at(0);
    assert!(ui.counter().starts_with("23 words"), "{}", ui.counter());
    assert!(ui.saved().contains("<!-- dostoevsky:overflow"), "{}", ui.saved());
    let reopened = Ui::open(&ui.saved());
    assert_eq!(reopened.notes(), [SENTENCE]);
    assert_eq!(reopened.text(), ui.text());
}

#[test]
fn cut_11_whole_paragraph_keeps_a_link() {
    let mut ui = Ui::open("First.\n\nSecond paragraph, all of it.\n\nThird.\n");
    stash(&mut ui, "Second paragraph, all of it.");
    assert_eq!(ui.origin_number(0), Some(2), "¶2: Third now stands where it was");
    ui.click_id(("origin", 0usize));
    assert_eq!(ui.flash().map(|r| r.start), Some(ui.find("Third.").start));
}
