use crate::prefs::SpellCheck;
use crate::ui_tests::Ui;

const WORDS: &str = "most of the friction in writing comes from choosing too early you pick a word";

fn ui(text: &str) -> Ui {
    let mut ui = Ui::open(text);
    ui.use_dictionary(WORDS);
    ui.draw();
    ui
}

#[test]
fn ed_4_misspelled_words_are_underlined() {
    let mut ui = ui("Most of the frction in writng.\n");
    assert_eq!(ui.misspelled(), [ui.find("frction"), ui.find("writng")]);
    ui.set_prefs(|p| p.spell_check = SpellCheck::Off);
    assert!(ui.misspelled().is_empty());
}

#[test]
fn ed_4_spelling_in_editing_mode_only() {
    let mut ui = ui("Most of the frction in writing.\n");
    ui.set_prefs(|p| p.spell_check = SpellCheck::Editing);
    assert!(ui.misspelled().is_empty(), "writing mode");
    ui.keys("cmd-d");
    ui.draw();
    assert_eq!(ui.misspelled(), [ui.find("frction")], "editing mode");
    ui.keys("cmd-d");
    ui.draw();
    assert!(ui.misspelled().is_empty(), "back to writing");
}

#[test]
fn ed_4_the_word_being_typed_is_left_alone() {
    let mut ui = ui("Most of the friction.\n");
    ui.caret_at(ui.find(".").start);
    ui.type_text(" wrds");
    assert!(ui.misspelled().is_empty(), "still typing it");
    ui.type_text(" ");
    assert_eq!(ui.misspelled(), [ui.find("wrds")]);
}

#[test]
fn ctx_1_spelling_suggestions() {
    let mut ui = ui("Most of the frction in writing.\n");
    ui.right_click_text("frction");
    let labels = ui.menu_labels();
    assert_eq!(labels[..3], ["friction", "Learn Spelling", "Ignore Spelling"], "{labels:?}");
    assert!(labels.iter().any(|l| l == "Alternatives for “frction”"));
    ui.menu("friction");
    assert!(ui.text().starts_with("Most of the friction in"));
    assert!(ui.misspelled().is_empty());
    ui.undo();
    assert!(ui.text().starts_with("Most of the frction in"), "undoable");
}

#[test]
fn ctx_1_learn_spelling_and_no_guesses() {
    let mut ui = ui("Zyx and the zyx again.\n");
    assert_eq!(ui.misspelled().len(), 4, "Zyx, and, zyx, again");
    ui.right_click_text("zyx");
    let items = ui.menu_items();
    assert_eq!((items[0].label.as_str(), items[0].enabled), ("No Guesses Found", false));
    ui.menu("Learn Spelling");
    assert_eq!(ui.misspelled(), [ui.find("and"), ui.find("again")], "both zyx are accepted");
}

#[test]
fn ctx_1_look_up() {
    let mut ui = ui("Most of the friction in writing.\n");
    ui.right_click_text("friction");
    ui.menu("Look Up “friction”");
    assert_eq!(ui.looked_up(), ["friction"]);

    ui.select_text("friction in writing");
    ui.right_click_text("in");
    ui.menu("Look Up “friction in writing”");
    assert_eq!(ui.looked_up(), ["friction", "friction in writing"]);
}
