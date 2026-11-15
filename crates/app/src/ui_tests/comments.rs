use doc_model::CommentId;
use gpui_kit::{px, Bounds, MouseButton, Pixels};

use crate::actions::{DeleteAllComments, ToggleOverflow};
use crate::ui_tests::Ui;

const DOC: &str = "# The Garret\n\nOn an exceptionally hot evening early in July a young man came out of the garret in which he lodged.\n\nHe had successfully avoided meeting his landlady on the staircase. His garret was more like a cupboard than a room.\n";

fn comments(ui: &Ui) -> Vec<(String, String)> {
    ui.read(|ws, cx| {
        let d = &ws.model.read(cx).doc;
        d.comments().into_iter().map(|c| (d.text_range(c.range), c.text)).collect()
    })
}

fn notes(ui: &Ui) -> Vec<(CommentId, Bounds<Pixels>, bool)> {
    ui.read(|ws, cx| ws.page.read(cx).comment_rects.borrow().clone())
}

fn writing(ui: &Ui) -> Option<CommentId> {
    ui.read(|ws, _| ws.comment_edit.as_ref().map(|e| e.id))
}

fn comment(ui: &mut Ui, words: &str, text: &str) {
    ui.select_text(words);
    ui.keys("alt-cmd-m");
    ui.type_text(text);
    ui.keys("enter");
}

#[test]
fn com_1_a_comment_on_the_selection() {
    let mut ui = Ui::open_editing(DOC);
    ui.select_text("more like a cupboard");
    ui.keys("alt-cmd-m");
    // The field goes where the page painted the note: the frame after.
    ui.draw();
    assert!(writing(&ui).is_some(), "its note is open for writing");
    assert!(ui.has_element("comment-field"), "in the margin");
    ui.type_text("Cupboard or closet?");
    ui.keys("enter");
    assert_eq!(writing(&ui), None);
    assert_eq!(comments(&ui), [("more like a cupboard".to_string(), "Cupboard or closet?".to_string())]);
    assert_eq!(ui.text(), DOC, "the text is as it was");
    ui.undo();
    assert!(comments(&ui).is_empty(), "one step");
}

#[test]
fn com_1_the_caret_sentence_an_empty_note_and_writing() {
    let mut ui = Ui::open(DOC);
    let at = ui.find("staircase").start;
    ui.caret_at(at);
    ui.keys("alt-cmd-m");
    assert!(ui.read(|ws, cx| ws.model.read(cx).layer), "editing");
    ui.type_text("Set it up sooner?");
    ui.keys("enter");
    assert_eq!(comments(&ui)[0].0, "He had successfully avoided meeting his landlady on the staircase.");
    ui.undo();
    ui.select_text("garret");
    ui.keys("alt-cmd-m");
    ui.keys("escape");
    assert!(comments(&ui).is_empty(), "an empty note took its comment with it");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).doc.can_undo()), "and left no step");
}

/// The field is placed from the page's last paint, and the page scrolls after the window has
/// laid out its fields: it has to move in the same frame, not land on the next note.
#[test]
fn com_1_the_field_scrolls_with_its_note() {
    let long = format!("{DOC}{}", "\nAnother paragraph, so the page has somewhere to scroll.\n".repeat(40));
    let mut ui = Ui::open_editing(&long);
    ui.set_prefs(|p| p.effects = crate::prefs::Effects::Off);
    ui.select_text("more like a cupboard");
    ui.keys("alt-cmd-m");
    ui.draw();
    let id = writing(&ui).expect("writing");
    let note = |ui: &Ui| notes(ui).into_iter().find(|n| n.0 == id).expect("its note").1.top();
    let before = (note(&ui), ui.element("comment-field").bounds().top());
    let r = ui.find("staircase");
    // Exactly one frame (the update draws it): the page scrolls in it. Another frame would hide it.
    ui.win.update(|ws, _, cx| ws.page.update(cx, |p, cx| p.centre_on(r.start + 300..r.start + 301, cx)));
    let after = (note(&ui), ui.element("comment-field").bounds().top());
    assert!(after.0 < before.0 - px(100.), "the page scrolled: {before:?} → {after:?}");
    assert_eq!(after.1 - after.0, before.1 - before.0, "the field sits on its note: {before:?} → {after:?}");
    assert_eq!(writing(&ui), Some(id), "still writing");
}

#[test]
fn com_2_writing_shows_none() {
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "garret", "Attic?");
    assert_eq!(notes(&ui).len(), 1);
    ui.keys("cmd-d");
    assert!(ui.read(|ws, cx| ws.page.read(cx).comments.is_empty()));
    assert!(notes(&ui).is_empty());
    assert_eq!(comments(&ui).len(), 1, "still there");
}

#[test]
fn com_3_the_note_in_the_margin() {
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "more like a cupboard", "Cupboard or closet?");
    let (id, note, tick) = notes(&ui)[0];
    assert!(!tick);
    let words = ui.text_bounds(ui.find("more like"))[0];
    let (origin, width) = ui.column();
    assert!(note.left() > origin.x + width, "right of the column");
    assert!((note.top() - words.top()).abs() < gpui_kit::px(4.), "level with its line: {note:?} {words:?}");
    ui.hover_text("cupboard");
    assert_eq!(ui.read(|ws, cx| ws.page.read(cx).hovered_comment), Some(id));
    ui.click(note.center(), MouseButton::Left);
    assert_eq!(writing(&ui), Some(id));
    ui.type_text(" Pick one.");
    ui.keys("enter");
    assert_eq!(comments(&ui)[0].1, "Cupboard or closet? Pick one.");
}

#[test]
fn com_4_ticks_and_the_peek() {
    let mut ui = Ui::open_sized(DOC, 760., 700.);
    ui.keys("cmd-d");
    comment(&mut ui, "garret", "Attic?");
    let (id, tick, is_tick) = notes(&ui)[0];
    assert!(is_tick, "a tick");
    ui.move_pointer(tick.center());
    ui.draw();
    assert_eq!(ui.read(|ws, _| ws.comment_peek), Some(id));
    assert!(ui.has_element("comment-peek"));
    ui.click(tick.center(), MouseButton::Left);
    ui.draw();
    assert_eq!(writing(&ui), Some(id), "a click writes it, in a card by its tick");
    assert!(ui.has_element("comment-card"));
}

/// Preferences → Editing → Comments: ticks with a roomy margin, and a note's menu switches back.
#[test]
fn com_4_ticks_by_choice() {
    use crate::prefs::CommentLook;
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "garret", "Attic?");
    assert!(!notes(&ui)[0].2, "a note by default");
    ui.set_prefs(|p| p.comment_look = CommentLook::Ticks);
    let (id, tick, is_tick) = notes(&ui)[0];
    assert!(is_tick, "a tick though the margin has room");
    ui.move_pointer(tick.center());
    ui.draw();
    assert_eq!(ui.read(|ws, _| ws.comment_peek), Some(id), "its peek shows the note");
    ui.click(tick.center(), MouseButton::Right);
    ui.menu("Show Comments as Notes");
    assert_eq!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).comment_look), CommentLook::Notes);
    assert!(!notes(&ui)[0].2, "back in the margin");
    let note = notes(&ui)[0].1;
    ui.click(note.center(), MouseButton::Right);
    ui.menu("Show Comments as Ticks");
    assert!(notes(&ui)[0].2);
}

#[test]
fn com_6_changing_and_deleting() {
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "garret", "Attic?");
    comment(&mut ui, "landlady", "Name her?");
    let note = notes(&ui)[0].1;
    ui.click(note.center(), MouseButton::Right);
    assert_eq!(ui.menu_labels(), ["Edit Comment", "Copy Comment", "Show in Comments", "Show Comments as Ticks", "Delete Comment"]);
    ui.menu("Delete Comment");
    assert_eq!(comments(&ui).len(), 1);
    ui.undo();
    assert_eq!(comments(&ui).len(), 2);
    ui.right_click_text("landlady");
    assert!(ui.menu_labels().contains(&"Edit Comment".to_string()), "on its words too");
    ui.keys("escape");
    let note = notes(&ui)[1].1;
    ui.click(note.center(), MouseButton::Left);
    assert!(writing(&ui).is_some());
    ui.keys("cmd-backspace");
    assert_eq!(comments(&ui), [("garret".to_string(), "Attic?".to_string())]);
    ui.action(DeleteAllComments);
    ui.answer_prompt("Delete");
    assert!(comments(&ui).is_empty());
}

#[test]
fn com_5_the_comments_tab() {
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "garret", "Attic?");
    comment(&mut ui, "landlady", "Name her?");
    ui.action(ToggleOverflow);
    assert!(ui.has_element("tab-comments"));
    ui.click_id("tab-comments");
    assert!(ui.has_element("comments-panel"));
    assert_eq!(ui.read(|ws, cx| ws.panel_count(cx)), 2, "the header counts the open tab");
    assert!(notes(&ui).iter().all(|n| n.2), "ticks while the list has the notes");
    ui.click_id(("comment-row", 1usize));
    assert_eq!(writing(&ui).map(|_| ()), Some(()));
    assert_eq!(ui.read(|ws, _| ws.comment_found), writing(&ui));
    ui.keys("escape");
    ui.click_id("tab-cuts");
    assert!(ui.has_element("overflow"));
}

/// The panel's edge keeps the open tab while it moves, and the panel button closes Comments too.
#[test]
fn com_5_resizing_and_closing_the_comments_tab() {
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "garret", "Attic?");
    ui.action(ToggleOverflow);
    ui.click_id("tab-comments");
    let edge = ui.element("overflow-edge").bounds().center();
    ui.drag_path(edge, &[gpui_kit::point(edge.x - gpui_kit::px(40.), edge.y)], false, |ui| {
        assert!(ui.has_element("comments-panel"), "Comments stays while its edge moves");
        assert!(!ui.has_element("overflow"), "not Cuts");
    });
    assert!(ui.has_element("comments-panel"));
    ui.click_id("overflow-button");
    assert!(!ui.has_element("comments-panel"), "the panel button closes it");
    assert!(notes(&ui).iter().all(|n| !n.2), "the notes are back in the margin");
    ui.click_id("overflow-button");
    assert!(ui.has_element("comments-panel"), "and opens it on its tab");
}

#[test]
fn com_9_a_cut_says_it_carries_a_comment() {
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "cupboard", "Closet?");
    ui.select_text("His garret was more like a cupboard than a room.");
    ui.keys("cmd-x");
    assert!(comments(&ui).is_empty());
    ui.action(ToggleOverflow);
    assert!(ui.has_element(("cut-comments", 0usize)));
}

#[test]
fn com_11_word_gets_my_comments_when_asked() {
    let mut ui = Ui::open_editing(DOC);
    comment(&mut ui, "garret", "Attic?");
    let book = ui.read(|ws, cx| ws.export_page_book_for_test(cx));
    let notes: Vec<_> = book.chapters[0].notes.iter().filter(|n| n.comment.is_some()).collect();
    assert_eq!(notes.len(), 1);
    let md = &book.chapters[0].markdown;
    let r = notes[0].range.clone();
    assert_eq!(md.chars().skip(r.start).take(r.len()).collect::<String>(), "garret");
}
