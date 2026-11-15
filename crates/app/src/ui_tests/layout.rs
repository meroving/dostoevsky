use doc_model::Scope;
use gpui_kit::px;

use crate::actions::ToggleOverflow;
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early. You pick a word and move on.\n\n\
Editors have always known this. They write a suggestion in the margin.\n";

#[test]
fn toolbar_centre_holds_while_the_counter_changes() {
    let mut ui = Ui::open(DOC);
    let at = |ui: &mut Ui| (ui.element("layer").bounds(), ui.element("overflow-button").bounds(), ui.element("title").bounds().origin);
    let before = at(&mut ui);
    let (layer, _, _) = before;
    let centre = layer.center().x;
    assert!((centre - px(590.)).abs() < px(2.), "centred in the 1180 px window: {centre:?}");
    let counter = ui.element("counter").bounds().size.width;

    ui.select_text("friction in writing");
    assert_eq!(at(&mut ui), before, "with a selection");
    ui.caret_at(0);
    ui.type_text(&"word ".repeat(300));
    assert!(ui.element("counter").bounds().size.width > counter, "the counter got wider");
    assert_eq!(at(&mut ui), before, "with a longer count");
}

/// WIN-1
#[test]
fn header_is_one_line() {
    let dir = crate::ui_tests::TempDir::new("header");
    let path = dir.file("a-very-long-file-name-for-the-header-of-a-small-window.md", DOC);

    for (w, h) in [(1180., 800.), (900., 600.), (680., 500.), (520., 360.)] {
        let mut ui = Ui::open_file_sized(&path, w, h);
        crate::ui_tests::ai::connect(&mut ui);
        ui.select_text("friction in writing");
        ui.keys("cmd-/");
        ui.select_text("Most of");
        ui.type_text("X");
        ui.select_text("Editors have always known this");

        // The same line in writing and in editing.
        let ids = ["panel", "title", "layer", "counter", "overflow-button"];
        for editing in [false, true] {
            if editing {
                ui.keys("cmd-d");
            }
            let boxes: Vec<_> = ids.iter().map(|id| ui.element(*id).bounds()).collect();
            for (id, b) in ids.iter().zip(&boxes) {
                assert!(b.bottom() <= px(crate::workspace::HEADER_H), "{id} is in the header at {w}: {b:?}");
                assert!((b.center().y - px(18.)).abs() < px(1.5), "{id} is on the line at {w}: {b:?}");
            }
            for pair in boxes.windows(2) {
                assert!(pair[0].right() <= pair[1].left(), "in order and clear at {w}: {ids:?} {pair:?}");
            }
            assert!(boxes[boxes.len() - 1].right() <= px(w), "inside the window at {w}");
            assert!(boxes[0].left() >= px(74.), "clear of the traffic lights at {w}");
            assert!((boxes[2].center().x - px(w / 2.)).abs() < px(2.), "the middle stays centred at {w}");
        }
    }
}

/// WIN-3: the test window panics on a move, so button tests know a click stays with the button.
#[test]
#[should_panic(expected = "not implemented")]
fn header_moves_the_window() {
    let mut ui = Ui::open(DOC);
    let (title, layer) = (ui.element("title").bounds(), ui.element("layer").bounds());
    ui.click(gpui_kit::point((title.right() + layer.left()) / 2., title.center().y), gpui_kit::MouseButton::Left);
}

#[test]
fn header_starts_at_the_edge_in_full_screen() {
    let mut ui = Ui::open(DOC);
    let beside_the_lights = ui.element("panel").bounds().left();
    ui.keys("ctrl-cmd-f");
    ui.draw();
    assert!(ui.element("panel").bounds().left() < beside_the_lights);
}

#[test]
fn panels_start_under_the_header() {
    let mut ui = Ui::open(DOC);
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    ui.action(ToggleOverflow);
    assert_eq!(ui.element("sidebar").bounds().top(), px(crate::workspace::HEADER_H));
    assert_eq!(ui.element("overflow").bounds().top(), px(crate::workspace::HEADER_H));
}

#[test]
fn page_holds_when_panels_open() {
    let mut ui = Ui::open_sized(DOC, 1440., 900.);
    let column = ui.column();
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    assert!(ui.sidebar_open());
    assert_eq!(ui.column(), column, "sidebar open");
    ui.action(ToggleOverflow);
    assert!(ui.drawer_open());
    assert_eq!(ui.column(), column, "sidebar and Overflow open");
}

#[test]
fn panels_never_cover_the_page() {
    let mut ui = Ui::open(DOC);
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    ui.action(ToggleOverflow);
    let sidebar = ui.element("sidebar").bounds();
    let drawer = ui.element("overflow").bounds();
    let (origin, width) = ui.column();
    assert!(origin.x >= sidebar.right(), "page {origin:?} under the sidebar {sidebar:?}");
    assert!(origin.x + width <= drawer.left(), "page ends {:?} under Overflow {drawer:?}", origin.x + width);
}

#[test]
fn dots_sit_with_their_group() {
    let mut ui = Ui::open(DOC);
    let word = ui.make_group("friction", &["drag", "grit"]);
    let sentence = ui.make_group("You pick a word and move on.", &["You choose and go."]);
    let para = ui.make_group("Editors have always known this. They write a suggestion in the margin.", &["Editors knew."]);
    assert_eq!(ui.visible_group(word).scope, Scope::Word);
    assert_eq!(ui.visible_group(sentence).scope, Scope::Sentence);
    assert_eq!(ui.visible_group(para).scope, Scope::Paragraph);
    let pitch = px(4.5);

    let (start, across) = ui.dots(word);
    let w = *ui.text_bounds(ui.visible_group(word).range).last().unwrap();
    let mid = start.x + pitch * (ui.variants(word).len() - 1) as f32 / 2.;
    assert!(across);
    assert!((mid - w.center().x).abs() < px(0.5), "word dots centred {mid:?} vs {:?}", w.center().x);
    assert!(start.y > w.bottom() - px(4.) && start.y < w.bottom() + ui.line_height() / 2., "word dots just under it: {start:?} {w:?}");

    let (start, across) = ui.dots(sentence);
    let s = *ui.text_bounds(ui.visible_group(sentence).range).last().unwrap();
    assert!(across);
    let end = start.x + pitch * (ui.variants(sentence).len() - 1) as f32 + px(1.4);
    assert!((end - s.right()).abs() < px(0.5), "sentence dots end under its last character: {end:?} vs {:?}", s.right());
    assert!(start.x > s.left() + px(12.), "inside the sentence, with line before them: {start:?} {s:?}");
    assert!(start.y > s.top() && start.y < s.bottom() + px(6.), "on its last line: {start:?} {s:?}");

    let (start, across) = ui.dots(para);
    let rows = ui.text_bounds(ui.visible_group(para).range);
    let (top, bottom) = (rows[0].top(), rows.last().unwrap().bottom());
    let mid = start.y + pitch * (ui.variants(para).len() - 1) as f32 / 2.;
    assert!(!across);
    assert!(start.x < ui.column().0.x - px(20.), "beside the rule, left of the text: {start:?}");
    assert!((mid - (top + bottom) / 2.).abs() < px(1.), "centred on the paragraph: {mid:?} vs {top:?}..{bottom:?}");
}

#[test]
fn dots_follow_their_word() {
    let mut ui = Ui::open(DOC);
    let word = ui.make_group("margin", &["edge"]);
    let before = ui.dots(word).0;
    let start = ui.find("Editors").start;
    ui.caret_at(start);
    ui.type_text("Good ");
    let after = ui.dots(word).0;
    let w = *ui.text_bounds(ui.visible_group(word).range).last().unwrap();
    assert!(after.x > before.x, "moved with the word: {before:?} → {after:?}");
    let mid = after.x + px(4.5) * (ui.variants(word).len() - 1) as f32 / 2.;
    assert!((mid - w.center().x).abs() < px(0.5), "still centred under it");
}

/// ALT-2
#[test]
fn underline_rows_end_at_the_last_character() {
    let long = "Most of the friction in writing comes from choosing too early, before you know what the sentence is for, who it talks to, or where the paragraph around it is going next.";
    let mut ui = Ui::open(&format!("{long} Then you move on.\n"));
    let other = "Most of the friction in writing comes from choosing a word too early, long before you know what the sentence is for, who it talks to, or where it goes.";
    let g = ui.make_group(long, &[other]);
    assert_eq!(ui.visible_group(g).scope, Scope::Sentence);
    let range = ui.visible_group(g).range;
    let (ink, rows) = ui.read(|ws, cx| {
        let l = ws.page.read(cx).layout.as_ref().expect("laid out");
        (l.ink_rects(range.clone()), l.rects(range.clone(), false))
    });
    assert!(ink.len() > 1 && ink.len() == rows.len(), "wraps: {ink:?} {rows:?}");
    for (i, (a, b)) in ink.iter().zip(&rows).enumerate() {
        assert_eq!(a.left(), b.left());
        if i + 1 < rows.len() {
            assert!(a.right() < b.right() - px(4.), "row {i} stops before the space at the wrap: {a:?} {b:?}");
        } else {
            assert_eq!(a.right(), b.right(), "the last row ends at the period");
        }
    }
}

#[test]
fn panels_resize_by_their_inner_edge() {
    let mut ui = Ui::open(DOC);
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    ui.action(ToggleOverflow);
    let sidebar = ui.element("sidebar").bounds();
    let drawer = ui.element("overflow").bounds();
    let settle = ui.element("settle").bounds();
    assert!(settle.right() <= sidebar.right(), "the hint fits the sidebar: {settle:?} in {sidebar:?}");

    ui.drag_id("sidebar-edge", 60.);
    assert_eq!(ui.element("sidebar").bounds().size.width, sidebar.size.width + px(60.));
    ui.drag_id("overflow-edge", -40.);
    let wider = ui.element("overflow").bounds();
    assert_eq!(wider.size.width, drawer.size.width + px(40.));
    assert_eq!(wider.right(), drawer.right(), "Overflow stays on the right edge");
    let (origin, width) = ui.column();
    assert!(origin.x >= ui.element("sidebar").bounds().right() && origin.x + width <= wider.left());

    ui.drag_id("sidebar-edge", -400.);
    assert_eq!(ui.element("sidebar").bounds().size.width, px(crate::workspace::SIDEBAR_MIN), "no narrower than the minimum");
    ui.drag_id("overflow-edge", 400.);
    assert_eq!(ui.element("overflow").bounds().size.width, px(crate::workspace::OVERFLOW_MIN), "no narrower than the minimum");
    ui.drag_id("overflow-edge", -2000.);
    assert!(ui.element("overflow").bounds().size.width <= px(crate::workspace::PANEL_MAX));
    let (origin, width) = ui.column();
    assert!(origin.x + width <= ui.element("overflow").bounds().left(), "the page still clears Overflow");
}

#[test]
fn panel_headers_fit_at_the_minimum() {
    let mut ui = Ui::open("One friction here. A word there. This one is long enough here. That one is long enough too.\n");
    ui.make_group("friction", &["drag"]);
    ui.make_group("word", &["term"]);
    ui.make_group("This one is long enough here.", &["Brief one is it here."]);
    ui.make_group("That one is long enough too.", &["Brief two is it here."]);
    ui.select_text("drag");
    ui.keys("cmd-shift-a");
    ui.action(ToggleOverflow);
    ui.drag_id("sidebar-edge", -400.);
    ui.drag_id("overflow-edge", 400.);

    let sidebar = ui.element("sidebar").bounds();
    let pager = ui.element("next-group").bounds();
    assert!(pager.right() <= sidebar.right() - px(16.), "‹ n of N › fits: {pager:?} in {sidebar:?}");
    assert!(ui.element("settle").bounds().right() <= sidebar.right() - px(20.), "the hint fits");
    let drawer = ui.element("overflow").bounds();
    let whose = ui.element("overflow-whose").bounds();
    assert!(whose.right() <= drawer.right() - px(13.), "the file's name and count fit: {whose:?} in {drawer:?}");
}

#[test]
fn sidebar_fields_wrap_inside_the_panel() {
    let mut ui = Ui::open(DOC);
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    ui.type_text("a much longer alternative that cannot possibly fit on one row of the panel");
    let sidebar = ui.element("sidebar").bounds();
    let add = ui.element("add-field").bounds();
    assert!(add.right() <= sidebar.right() - px(20.), "the add field fits: {add:?} in {sidebar:?}");
    assert!(add.size.height > px(30.), "it wrapped: {add:?}");

    ui.keys("enter");
    ui.double_click_variant(1);
    let edit = ui.element("variant-field").bounds();
    assert!(edit.right() <= sidebar.right() - px(20.), "the edited variant fits: {edit:?} in {sidebar:?}");
}

#[test]
fn find_fits_a_narrow_window() {
    let mut ui = Ui::open_sized("Some text to find.\n", 520., 600.);
    ui.keys("cmd-f");
    let bar = ui.element("find-bar").bounds();
    assert!(bar.left() >= px(0.) && bar.right() <= px(520.), "the find bar {bar:?} in a 520 window");
    for id in ["prev", "next", "opt-words", "close", "opt-cuts"] {
        let b = ui.element(id).bounds();
        assert!(b.left() >= bar.left() && b.right() <= bar.right(), "{id} {b:?} inside {bar:?}");
    }
}

/// WIN-18
#[test]
fn justified_and_hyphenated_paragraphs() {
    use crate::prefs::Alignment;
    const BOOK: &str = "# Chapter One\n\nHe was hopelessly in debt to his landlady, and was afraid of meeting her. \
This was not because he was cowardly and abject, quite the contrary; but for some time past he had been in \
an overstrained irritable condition, verging on hypochondria.\n";
    let mut ui = Ui::open(BOOK);
    let line = 2;
    let ragged = ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().row_ends(line));
    let heading = ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().row_ends(0));
    let (origin, width) = ui.column();
    let right = origin.x + width;
    assert!(ragged.len() >= 3, "{ragged:?}");
    assert!(ragged.iter().all(|(x, h)| *x <= right && !h), "ragged, no hyphens by default: {ragged:?}");
    assert!(ragged[..ragged.len() - 1].iter().any(|(x, _)| right - *x > px(4.)), "some row falls short: {ragged:?}");

    ui.app.update(|cx| {
        crate::sheets::set_prefs(cx, |p| {
            p.alignment = Alignment::Justified;
            p.hyphenation = true;
        })
    });
    ui.draw();
    let rows = ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().row_ends(line));
    for (x, _) in &rows[..rows.len() - 1] {
        assert!((*x - right).abs() < px(0.5), "row reaches the edge {right:?}: {rows:?}");
    }
    assert!(rows.iter().any(|(_, h)| *h), "some word is broken: {rows:?}");
    assert!(rows.last().unwrap().0 < right - px(4.), "the last row stays left: {rows:?}");
    assert_eq!(ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().row_ends(0)), heading, "the heading is untouched");
    // The hyphen is drawn, not typed.
    assert_eq!(ui.text(), BOOK);
    ui.select_text("overstrained");
    ui.keys("cmd-c");
    assert_eq!(ui.app.read_from_clipboard().and_then(|c| c.text()).as_deref(), Some("overstrained"));

    // Back to Left (hyphenation alone smooths the edge): the rows are as they were.
    ui.app.update(|cx| crate::sheets::set_prefs(cx, |p| p.alignment = Alignment::Left));
    ui.draw();
    let left = ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().row_ends(line));
    assert!(left.iter().all(|(x, _)| *x <= right + px(0.5)), "{left:?}");
    ui.app.update(|cx| crate::sheets::set_prefs(cx, |p| p.hyphenation = false));
    ui.draw();
    assert_eq!(ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().row_ends(line)), ragged);
}

/// CUT-6, ED-13
#[test]
fn win_4_switching_modes_never_moves_text() {
    use crate::prefs::{CutLook, ParagraphLook};
    const BOOK: &str = "# Chapter One\n\nHe was hopelessly in debt to his landlady, and was afraid of meeting her. \
This was not because he was cowardly and abject, quite the contrary.\n\n\
He had become so completely absorbed in himself that he dreaded meeting anyone at all.\n\n\
- a list item\n\nHe was crushed by poverty, but the anxieties of his position had ceased to weigh upon him.\n";
    for look in [ParagraphLook::Spaced, ParagraphLook::Indented] {
        let mut ui = Ui::open_editing(BOOK);
        ui.set_prefs(|p| p.paragraphs = look);
        ui.make_group("landlady", &["hostess"]);
        ui.select_text("quite the contrary");
        ui.keys("cmd-x");
        ui.set_prefs(|p| p.cut_look = CutLook::Mark);
        ui.select_text("so completely");
        ui.keys("cmd-x");
        ui.select_text("cowardly");
        ui.keys("alt-cmd-m");
        ui.type_text("too strong?");
        ui.keys("enter");
        let starts = |ui: &Ui| {
            let text = ui.text();
            let lines: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| text[..i + 1].chars().count())).filter(|c| *c < text.chars().count()).collect();
            let words: Vec<usize> = ["debt", "cowardly", "absorbed", "crushed", "anxieties"].iter().map(|w| ui.find(w).start).collect();
            let at = |c: usize| ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").point_for(c));
            (text.clone(), lines.iter().chain(&words).map(|c| at(*c)).collect::<Vec<_>>(), ui.read(|ws, cx| ws.page.read(cx).marks.len()))
        };
        let editing = starts(&ui);
        ui.keys("cmd-d");
        assert!(!ui.read(|ws, cx| ws.model.read(cx).layer), "writing");
        assert_eq!(starts(&ui), editing, "writing moved text ({look:?})");
        ui.keys("cmd-d");
        assert_eq!(starts(&ui), editing, "editing again moved text ({look:?})");
    }
}

/// WIN-18: a book face's space is a third of a mono one's, and its rows justify all the same.
#[test]
fn justified_rows_reach_the_edge_in_every_font() {
    use crate::prefs::Alignment;
    const BOOK: &str = "He was hopelessly in debt to his landlady, and was afraid of meeting her. \
This was not because he was cowardly and abject, quite the contrary; but for some time past he had been in \
an overstrained irritable condition, verging on hypochondria. He had become so completely absorbed in himself, \
and isolated from his fellows that he dreaded meeting, not only his landlady, but anyone at all. He was crushed \
by poverty, but the anxieties of his position had of late ceased to weigh upon him.\n";
    for family in [crate::theme::PLEX, crate::theme::LITERATA, crate::theme::QUATTRO] {
        let mut ui = Ui::open(BOOK);
        ui.set_prefs(|p| {
            p.font_family = family.into();
            p.alignment = Alignment::Justified;
        });
        let (origin, width) = ui.column();
        let right = origin.x + width;
        let rows = ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().row_ends(0));
        assert!(rows.len() >= 4, "{family}: {rows:?}");
        for (x, _) in &rows[..rows.len() - 1] {
            assert!((*x - right).abs() < px(0.5), "{family}: every row but the last reaches {right:?}: {rows:?}");
        }
    }
}
