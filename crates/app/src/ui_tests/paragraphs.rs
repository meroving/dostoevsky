use gpui_kit::{MouseButton, Pixels, Point, point, px};

use crate::prefs::ParagraphLook;
use crate::ui_tests::Ui;

const DOC: &str = "One two.\n\nThree four.\n\nFive.\n";

fn slot(ui: &Ui, c: usize) -> Point<Pixels> {
    ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").point_for(c).expect("laid out"))
}

fn gap_height(ui: &Ui, line: usize) -> Option<Pixels> {
    ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").gap_height(line))
}

#[test]
fn ed_13_the_break_is_one_step() {
    let mut ui = Ui::open(DOC);
    let lh = ui.line_height();
    assert_eq!(gap_height(&ui, 1), Some((lh * (2. / 3.)).round()));
    assert_eq!(slot(&ui, ui.find("Three").start).y - slot(&ui, 0).y, lh + (lh * (2. / 3.)).round());

    let (end, next) = (ui.find("two.").end, ui.find("Three").start);
    assert_eq!(next, end + 2);
    ui.caret_at(end);
    ui.keys("right");
    assert_eq!(ui.selection(), next..next, "→ crosses the break");
    ui.keys("left");
    assert_eq!(ui.selection(), end..end, "← too");
    ui.keys("shift-right");
    assert_eq!(ui.selection(), end..next, "⇧→ selects the whole break");

    ui.caret_at(ui.find("two").start);
    ui.keys("down");
    let c = ui.selection().start;
    assert!(ui.find("Three four.").contains(&c), "↓ lands on the next paragraph: {c}");
    assert_eq!(slot(&ui, c).x, slot(&ui, ui.find("two").start).x);
    ui.keys("up");
    assert_eq!(ui.selection().start, ui.find("two").start);
    ui.caret_at(ui.find("Three").start);
    ui.keys("up");
    assert!(ui.selection().start <= end, "↑ from a paragraph's start lands on the one above");

    let gap = gap_height(&ui, 1).unwrap();
    let x = slot(&ui, 3).x;
    let top = slot(&ui, end).y + lh;
    ui.click(point(x, top + gap * 0.25), MouseButton::Left);
    assert_eq!(ui.selection(), end..end);
    ui.click(point(x, top + gap * 0.75), MouseButton::Left);
    assert_eq!(ui.selection(), next..next);

    ui.caret_at(end);
    ui.keys("delete");
    assert_eq!(ui.text(), "One two.Three four.\n\nFive.\n");
    ui.undo();
    assert_eq!(ui.text(), DOC);
    ui.caret_at(next);
    ui.keys("backspace");
    assert_eq!(ui.text(), "One two.Three four.\n\nFive.\n");
}

#[test]
fn ed_13_return_on_an_empty_paragraph_adds_nothing() {
    let mut ui = Ui::open(DOC);
    ui.caret_at(ui.find("Five.").end);
    ui.keys("enter");
    let text = ui.text();
    assert_eq!(text, "One two.\n\nThree four.\n\nFive.\n\n\n");
    let caret = ui.selection();
    ui.keys("enter");
    assert_eq!(ui.text(), text, "a second Return adds nothing");
    assert_eq!(ui.selection(), caret);
    assert_eq!(ui.status().as_deref(), Some("--- for a scene break"));
    ui.type_text("Six.");
    assert_eq!(ui.text(), "One two.\n\nThree four.\n\nFive.\n\nSix.\n");

    let mut ui = Ui::open("One.\n\n\nTwo.\n");
    let row = ui.find("One.").end + 2;
    ui.caret_at(row);
    assert_eq!(ui.selection(), row..row);
    assert_eq!(gap_height(&ui, 2), None, "the extra line is a row");
}

#[test]
fn ed_13_indented_paragraphs() {
    const BOOK: &str = "# Title\n\nFirst paragraph.\n\nSecond paragraph.\n\n> A quote.\n\nAfter the quote.\n";
    let mut ui = Ui::open(BOOK);
    ui.set_prefs(|p| p.paragraphs = ParagraphLook::Indented);
    let lh = ui.line_height();
    let (origin, _) = ui.column();
    let indent = (px(19.) * 1.5).round();
    let first = slot(&ui, ui.find("First").start);
    let second = slot(&ui, ui.find("Second").start);
    assert_eq!(first.x, origin.x, "flush after the heading");
    assert_eq!(second.x, origin.x + indent);
    assert_eq!(second.y - first.y, lh, "no gap between them");
    assert_eq!(gap_height(&ui, 3), Some(px(0.)));
    assert_eq!(slot(&ui, ui.find("After").start).x, origin.x, "flush after a quote");
    assert_eq!(slot(&ui, ui.find("A quote").start).y - second.y, lh * 2., "a quote keeps a line around it");
    assert_eq!(ui.text(), BOOK);

    let end = ui.find("First paragraph.").end;
    ui.caret_at(end);
    ui.keys("right");
    assert_eq!(ui.selection().start, ui.find("Second").start);

    ui.caret_at(ui.find("Second paragraph.").end);
    ui.keys("enter");
    assert_eq!(slot(&ui, ui.selection().start).x, origin.x + indent);

    const RU: &str = "# Глава I\n\nВ начале июля, в чрезвычайно жаркое время, под вечер, один молодой человек вышел из своей каморки, которую нанимал от жильцов в С—м переулке, на улицу и медленно, как бы в нерешимости, отправился к К—ну мосту.\n\nОн благополучно избегнул встречи с своею хозяйкой на лестнице. Каморка его приходилась под самою кровлей высокого пятиэтажного дома и походила более на шкаф, чем на квартиру.\n";
    let mut ui = Ui::open(RU);
    ui.set_prefs(|p| p.paragraphs = ParagraphLook::Indented);
    let (origin, _) = ui.column();
    assert_eq!(slot(&ui, ui.find("В начале").start).x, origin.x + indent);
    assert_eq!(slot(&ui, ui.find("Он благополучно").start).x, origin.x + indent);
}

#[test]
fn ed_13_the_mode_never_moves_text() {
    const BOOK: &str = "# Title\n\nFirst paragraph, long enough to wrap onto a second row of the column at the default width of the page.\n\nSecond paragraph.\n\n- a list\n\nThird.\n";
    for look in [ParagraphLook::Spaced, ParagraphLook::Indented] {
        let mut ui = Ui::open(BOOK);
        ui.set_prefs(|p| p.paragraphs = look);
        let starts: Vec<usize> = ["Title", "First", "Second", "a list", "Third"].iter().map(|w| ui.find(w).start).collect();
        let writing: Vec<_> = starts.iter().map(|c| slot(&ui, *c)).collect();
        ui.action(crate::actions::ToggleLayer);
        ui.draw();
        let editing: Vec<_> = starts.iter().map(|c| slot(&ui, *c)).collect();
        assert_eq!(writing, editing, "{look:?}");
    }
}
