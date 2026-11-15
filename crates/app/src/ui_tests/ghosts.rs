use crate::actions::RemoveAllGhosted;
use crate::prefs::Effects;
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early. You pick a word and move on.\n\n\
Editors have always known this. They write a suggestion in the margin.\n";

fn ghosted() -> Ui {
    let mut ui = Ui::open(DOC);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui
}

#[test]
fn cut_4_cmd_slash_ghosts_the_selection_with_a_fade() {
    let ui = ghosted();
    let r = ui.find("choosing too early");
    assert_eq!(ui.ghosts(), std::slice::from_ref(&r));
    assert_eq!(ui.fades(), [(r, 0.12)]);
    assert!(ui.text().contains("choosing too early"), "the text stays in place");
}

#[test]
fn cut_4_no_fade_without_motion() {
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| p.effects = Effects::Off);
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    assert_eq!(ui.ghosts().len(), 1);
    assert!(ui.fades().is_empty());
}

#[test]
fn cut_13_cut_words_are_not_counted() {
    let mut ui = Ui::open(DOC);
    assert!(ui.counter().starts_with("30 words"), "{}", ui.counter());
    ui.select_text("choosing too early");
    ui.keys("cmd-/");
    ui.caret_at(0);
    assert!(ui.counter().starts_with("27 words"), "{}", ui.counter());
    assert!(!ui.counter().contains('·'), "{}", ui.counter());
}

#[test]
fn cut_4_ghosted_text_can_be_edited() {
    let mut ui = ghosted();
    ui.caret_at(ui.find("too early").start + 3);
    ui.type_text("o");
    assert_eq!(ui.ghosts(), [ui.find("choosing tooo early")]);
}

#[test]
fn cut_6_hovering_ghosted_text() {
    let mut ui = ghosted();
    ui.hover_text("too");
    assert_eq!(ui.hovered_ghost(), Some(ui.find("choosing too early")));
    ui.hover_text("Editors");
    assert_eq!(ui.hovered_ghost(), None);
}

#[test]
fn cut_4_revive_at_the_caret() {
    let mut ui = ghosted();
    let r = ui.find("choosing too early");
    ui.caret_at(r.start + 3);
    ui.keys("cmd-/");
    assert!(ui.ghosts().is_empty());
    assert!(ui.fades().contains(&(r, 1.0)), "{:?}", ui.fades());
}

#[test]
fn cut_4_put_back_from_part_of_it() {
    let mut ui = ghosted();
    ui.select_text("too");
    ui.keys("cmd-/");
    assert!(ui.ghosts().is_empty(), "{:?}", ui.ghosts());
}

#[test]
fn cut_1_partial_group_selection_takes_the_whole_group() {
    let mut ui = Ui::open(DOC);
    ui.make_group("friction", &["tension"]);
    ui.select_text("the tens");
    ui.keys("cmd-/");
    assert_eq!(ui.ghosts(), [ui.find("the tension")]);
    assert!(ui.groups()[0].ghosted);
}

#[test]
fn cut_1_across_paragraphs() {
    let mut ui = Ui::open(DOC);
    let start = ui.find("move on.").start;
    let end = ui.find("Editors have").end;
    ui.select(start..end);
    ui.keys("cmd-/");
    let span = start..end;
    assert_eq!(ui.ghosts(), std::slice::from_ref(&span));
    let reopened = Ui::open(&ui.saved());
    assert_eq!(reopened.ghosts(), std::slice::from_ref(&span));
}

#[test]
fn cut_13_final_markdown_leaves_ghosts_out() {
    let mut ui = ghosted();
    ui.keys("cmd-shift-c");
    let copied = ui.clipboard().expect("copied");
    assert!(copied.starts_with("Most of the friction in writing comes from"), "{copied}");
    assert!(!copied.contains("choosing"), "{copied}");
    assert_eq!(ui.status().as_deref(), Some("Copied the final text · 27 words"));
    let html = crate::clipboard::RICH_HTML.with(|r| r.borrow().clone()).expect("formatted too");
    assert!(html.starts_with("<p>Most of the friction") && !html.contains("choosing"), "{html}");

    ui.keys("alt-cmd-u");
    let saved = ui.source_view().expect("source view is open");
    let body = &saved[..saved.find("<!-- dostoevsky:").unwrap()];
    assert!(!body.contains("choosing too early") && saved.contains("choosing too early") && saved.contains(r#""trace":true"#), "{saved}");
    ui.keys("alt-cmd-u");

    ui.keys("cmd-shift-e");
    ui.click_id(("export-format", crate::export_ui::Format::Markdown as usize));
    ui.settle();
    let shown = ui.export_form(|f| f.preview.text.clone()).flatten().expect("the Markdown preview");
    assert_eq!(shown, copied.trim_end());
}

#[test]
fn cut_16_delete_all_cuts_on_this_page() {
    let mut ui = ghosted();
    ui.select_text("suggestion");
    ui.keys("cmd-/");
    assert_eq!(ui.ghosts().len(), 2);

    ui.action(RemoveAllGhosted);
    ui.answer_prompt("Cancel");
    assert_eq!(ui.ghosts().len(), 2);

    ui.action(RemoveAllGhosted);
    ui.answer_prompt("Delete");
    assert!(ui.ghosts().is_empty());
    assert!(ui.notes().is_empty());
    assert!(!ui.text().contains("choosing") && !ui.text().contains("suggestion"), "{}", ui.text());

    ui.undo();
    assert_eq!(ui.ghosts().len(), 2);
    assert!(ui.text().contains("choosing too early"));
}

#[test]
fn ghosts_survive_save_and_reopen() {
    let mut ui = ghosted();
    ui.make_group("suggestion", &["note"]);
    ui.select_text("note");
    ui.keys("cmd-/");
    let before = ui.ghosts();
    assert_eq!(before.len(), 2);
    let mut reopened = Ui::open(&ui.saved());
    if !reopened.read(|ws, cx| ws.model.read(cx).layer) {
        reopened.keys("cmd-d");
    }
    assert_eq!(reopened.ghosts(), before);
    assert!(reopened.groups()[0].ghosted);
}
