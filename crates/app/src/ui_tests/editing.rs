use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n\nEditors have always known this.\n";

#[test]
fn ed_1_font_and_line_height() {
    let mut ui = Ui::open(DOC);
    let p = ui.read(|_, cx| crate::prefs::Prefs::get(cx).clone());
    assert_eq!((p.font_family.as_str(), p.font_size, p.line_spacing), ("IBM Plex Mono", 19., 1.8));
    let lh = f32::from(ui.line_height());
    assert!((lh - 19. * 1.8).abs() < 1.5, "line height {lh}");

    ui.keys("cmd-,");
    ui.click_id("nav-page");
    ui.click_id(("font", 0usize));
    assert_eq!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).font_family.clone()), crate::theme::MONO);
    assert_eq!(ui.element(("font", 0usize)).selected(), Some(true));
}

#[test]
fn ed_1_return_starts_a_paragraph() {
    let mut ui = Ui::open("# Title\n\nOne. Two.\n");
    ui.caret_at(ui.find(" Two").start);
    ui.keys("enter");
    assert_eq!(ui.text(), "# Title\n\nOne.\n\n Two.\n");
    let gap = ui.point_for(ui.find(" Two").start).y - ui.point_for(ui.find("One").start).y;
    assert_eq!(gap, ui.line_height() + (ui.line_height() * (2. / 3.)).round(), "a gap ⅔ of a line between them (ED-13)");
    ui.keys("backspace");
    assert_eq!(ui.text(), "# Title\n\nOne. Two.\n", "one Backspace joins them");

    ui.keys("shift-enter");
    assert_eq!(ui.text(), "# Title\n\nOne.\n Two.\n");
    ui.keys("backspace");
    assert_eq!(ui.text(), "# Title\n\nOne. Two.\n");

    ui.caret_at(ui.find("Title").end);
    ui.keys("enter");
    ui.type_text("Lede.");
    assert_eq!(ui.text(), "# Title\n\nLede.\n\nOne. Two.\n", "after a heading too");

    ui.caret_at(ui.find("Two.").end);
    ui.keys("enter backspace");
    assert_eq!(ui.text(), "# Title\n\nLede.\n\nOne. Two.\n");

    ui.caret_at(ui.find("One").start);
    ui.keys("enter");
    assert_eq!(ui.text(), "# Title\n\nLede.\n\n\n\nOne. Two.\n");
    assert_eq!(ui.selection(), ui.find("One").start..ui.find("One").start);
    ui.keys("enter");
    assert_eq!(ui.text(), "# Title\n\nLede.\n\n\n\nOne. Two.\n");
    ui.keys("backspace");
    assert_eq!(ui.text(), "# Title\n\nLede.\n\nOne. Two.\n");
    let mut ui = Ui::open("One.\nTwo.\n");
    ui.caret_at(ui.find("Two").start);
    ui.keys("enter");
    assert_eq!(ui.text(), "One.\n\nTwo.\n");
    let mut ui = Ui::open("One.\nTwo.\n");
    ui.caret_at(ui.find("One.").end);
    ui.keys("enter");
    ui.type_text("X");
    assert_eq!(ui.text(), "One.\n\nXTwo.\n");

    let mut ui = Ui::open("- one\n");
    ui.caret_at(ui.find("one").end);
    ui.keys("enter");
    assert_eq!(ui.text(), "- one\n- \n");
    let mut ui = Ui::open("- one\n");
    ui.caret_at(0);
    ui.keys("enter");
    assert_eq!(ui.text(), "\n- one\n");
}

#[test]
fn ed_2_heading_marker_hides_off_the_caret_line() {
    let mut ui = Ui::open("# A Title\n\nBody text.\n");
    ui.caret_at(ui.find("Body").start);
    let hidden = ui.heading_shift(0);
    assert!(f32::from(hidden) < 0., "marker left of the column");
    ui.caret_at(3);
    assert_eq!(ui.heading_shift(0), hidden, "the title stays put on the caret line");
}

#[test]
fn ed_2_heading_lines_up_with_the_body_in_every_font() {
    for family in [crate::theme::MONO, crate::theme::PLEX, crate::theme::QUATTRO, crate::theme::LITERATA] {
        let mut ui = Ui::open("# Вайб-кодинг\n\nЧем отличается умелый человек от новичка?\n");
        ui.set_prefs(|p| p.font_family = family.into());
        ui.caret_at(ui.find("новичка").start);
        let (title, body) = (ui.point_for(2).x, ui.point_for(ui.find("Чем").start).x);
        assert!((f32::from(title) - f32::from(body)).abs() < 0.5, "{family}: title at {title:?}, body at {body:?}");
    }
}

#[test]
fn ed_2_headings_in_the_hand_font() {
    let mut ui = Ui::open("# One\n\n## Two\n\n### Three\n\nBody.\n");
    let sizes = |ui: &Ui| [0, 2, 4, 6].map(|l| f32::from(ui.line_font_size(l)));
    let lh = f32::from(ui.line_height());
    let [h1, h2, h3, body] = sizes(&ui);
    assert!(h1 > h2 && h2 > h3 && h1 > body, "{h1} {h2} {h3} {body}");
    assert!(h1 < lh, "# fits its row: {h1} in {lh}");

    ui.keys("cmd-,");
    ui.click_id(("hand-font", 1usize));
    assert_eq!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).hand_font), crate::prefs::HandFont::Caveat);
    assert!(sizes(&ui)[0] > h1, "Caveat is set larger than Bad Script");
    assert!(sizes(&ui)[0] < lh);

    ui.click_id("nav-page");
    ui.click_id(("hand-headings", 0usize));
    assert!(!ui.read(|_, cx| crate::prefs::Prefs::get(cx).hand_headings));
    assert_eq!(sizes(&ui), [body; 4], "page font, page size");
}

#[test]
fn ed_3_bold_and_italic_wrap_the_selection() {
    let mut ui = Ui::open(DOC);
    ui.select_text("friction");
    ui.keys("cmd-b");
    assert!(ui.text().starts_with("Most of the **friction** in"), "{}", ui.text());
    ui.select_text("writing");
    ui.keys("cmd-i");
    assert!(ui.text().contains("in *writing* comes"), "{}", ui.text());
}

#[test]
fn ed_4_selection_gestures() {
    let mut ui = Ui::open(DOC);
    let p = ui.point_for(ui.find("friction").start + 3);
    ui.move_pointer(p);
    ui.click_n(p, 2);
    assert_eq!(ui.selection(), ui.find("friction"));
    ui.click_n(p, 3);
    let para = ui.find("Most of the friction in writing comes from choosing too early.");
    assert!(ui.selection().start == para.start && ui.selection().end >= para.end, "{:?}", ui.selection());

    ui.caret_at(0);
    ui.keys("alt-right");
    assert_eq!(ui.selection(), 4..4, "end of 'Most'");
    ui.keys("cmd-right");
    assert_eq!(ui.selection().start, para.end, "end of the line");
    ui.keys("cmd-down");
    assert_eq!(ui.selection().start, ui.text().chars().count(), "end of the document");
    ui.keys("cmd-up");
    assert_eq!(ui.selection(), 0..0);
    ui.keys("shift-alt-right");
    assert_eq!(ui.selection(), 0..4);
}

#[test]
fn ed_4_input_method_composition() {
    let mut ui = Ui::open(DOC);
    ui.caret_at(0);
    ui.ime_mark("かん");
    assert!(ui.text().starts_with("かんMost"));
    assert_eq!(ui.marked(), Some(0..2));
    ui.ime_mark("かんじ");
    assert!(ui.text().starts_with("かんじMost"));
    ui.ime_commit("漢字");
    assert!(ui.text().starts_with("漢字Most"), "{}", ui.text());
    assert_eq!(ui.marked(), None);
    ui.ime_mark("´");
    ui.ime_commit("é");
    assert!(ui.text().starts_with("漢字éMost"), "dead keys: {}", ui.text());
}

#[test]
fn ed_5_undo_groups_typing_and_is_unlimited() {
    let mut ui = Ui::open(DOC);
    ui.caret_at(0);
    ui.type_text("A quick run of typing. ");
    ui.undo();
    assert_eq!(ui.text(), DOC, "one step for a run of typing");
    ui.redo();
    assert!(ui.text().starts_with("A quick run of typing. Most"));

    let mut ui = Ui::open(DOC);
    for i in 0..150 {
        ui.caret_at(i % 20);
        ui.keys("cmd-b");
    }
    for _ in 0..150 {
        ui.undo();
    }
    assert_eq!(ui.text(), DOC, "150 steps back");
    for _ in 0..150 {
        ui.redo();
    }
    assert_eq!(ui.text().matches("****").count(), 150, "and forward again");
}

#[test]
fn ed_6_find_and_replace() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-f");
    ui.type_text("in");
    let (n, _, _) = ui.find_state().expect("find bar open");
    assert_eq!(n, ui.text().matches("in").count());
    ui.keys("enter");
    assert_eq!(&ui.text()[ui.selection()], "in");
    let first = ui.selection();
    ui.click_id("next");
    assert!(ui.selection().start > first.start, "{:?} after {first:?}", ui.selection());
    ui.keys("enter");
    assert_eq!(ui.find_state().unwrap().0, 3, "Return still goes to the find field, not the page");
    ui.keys("escape");
    assert!(ui.find_state().is_none());

    ui.keys("alt-cmd-f");
    ui.type_text("Editors");
    ui.set_replacement("Writers");
    ui.click_id("replace-all");
    assert!(ui.text().contains("Writers have always"));
    assert_eq!(ui.status().as_deref(), Some("Replaced 1"));
    ui.keys("escape");
    ui.undo();
    assert!(ui.text().contains("Editors have always"), "replace all is one undo step");
}

#[test]
fn ed_6_find_next_and_previous() {
    let mut ui = Ui::open(DOC);
    ui.caret_at(0);
    ui.keys("cmd-f");
    ui.type_text("in");
    ui.keys("enter");
    let first = ui.selection();
    assert_eq!(first.start, ui.text().find("in").unwrap());
    ui.keys("cmd-g");
    assert!(ui.selection().start > first.start);
    ui.keys("cmd-shift-g");
    assert_eq!(ui.selection(), first);
    ui.keys("shift-enter");
    assert_eq!(ui.selection().start, ui.text().rfind("in").unwrap(), "wraps to the last");
    assert_eq!(ui.find_state().unwrap().0, ui.text().matches("in").count(), "⇧Return types nothing");
    ui.keys("escape");
    assert!(ui.find_state().is_none());

    ui.caret_at(0);
    ui.keys("cmd-g");
    assert!(ui.find_state().is_some(), "the bar comes back");
    assert_eq!(ui.selection(), first, "with the last query");

    ui.select_text("Editors");
    ui.keys("cmd-e");
    ui.caret_at(0);
    ui.keys("cmd-g");
    assert_eq!(ui.selection(), ui.find("Editors"));
}

#[test]
fn ed_3_cocoa_text_keys() {
    let mut ui = Ui::open(DOC);
    let second = ui.find("Editors").start;
    ui.caret_at(second + 3);
    ui.keys("alt-up");
    assert_eq!(ui.selection(), second..second);
    ui.keys("alt-up");
    assert_eq!(ui.selection(), 0..0, "over the blank line, to the paragraph before");
    ui.keys("alt-down");
    let end1 = ui.find("early.").end;
    assert_eq!(ui.selection(), end1..end1);
    ui.keys("alt-down");
    let end2 = ui.find("this.").end;
    assert_eq!(ui.selection(), end2..end2);
    ui.keys("alt-shift-up");
    assert_eq!(ui.selection(), second..end2);

    ui.caret_at(second + 2);
    ui.keys("ctrl-t");
    assert!(ui.text().contains("\nEidtors have"), "{}", ui.text());
    assert_eq!(ui.selection(), second + 3..second + 3);
    ui.undo();

    ui.caret_at(ui.find("always").start);
    ui.keys("ctrl-k");
    assert!(ui.text().ends_with("\nEditors have \n"), "{:?}", ui.text());
    ui.keys("ctrl-k");
    assert!(ui.text().ends_with("\nEditors have "), "then the line break: {:?}", ui.text());
}

#[test]
fn ed_6_find_reaches_hidden_versions() {
    let mut ui = Ui::open(DOC);
    let id = ui.make_group("friction", &["tension"]);
    ui.keys("cmd-f");
    ui.type_text("friction");
    assert_eq!(ui.find_state().unwrap(), (1, Some(0), 1), "one match, out of sight");
    assert_eq!(ui.element("find-summary").label(), Some("1 in this file · 1 out of sight"));
    ui.keys("enter");
    assert_eq!(ui.sidebar(), Some(id));
    assert_eq!(ui.read(|ws, _| ws.sidebar.as_ref().and_then(|s| s.found)), Some((id, 0)));
    assert!(ui.text().contains("tension"), "the page keeps its version");
    ui.click_id("opt-alternatives");
    assert_eq!(ui.element("opt-alternatives").checked(), Some(false));
    assert_eq!(ui.find_state().unwrap().0, 0);
    ui.keys("escape");
    assert!(!ui.sidebar_open(), "find puts the panel away");
}

#[test]
fn ed_6_match_case_and_whole_words() {
    let mut ui = Ui::open("Her mother, her there.\n");
    ui.keys("cmd-f");
    ui.type_text("her");
    assert_eq!(ui.find_state().unwrap().0, 4);
    ui.keys("alt-cmd-w");
    assert_eq!(ui.element("opt-words").checked(), Some(true));
    assert_eq!(ui.find_state().unwrap().0, 2, "Her, her");
    assert!(ui.element("find-summary").label().unwrap_or_default().ends_with("2 more inside other words"));
    ui.keys("alt-cmd-c");
    assert_eq!(ui.find_state().unwrap().0, 1, "her");
    ui.keys("escape cmd-f");
    assert_eq!(ui.element("opt-case").checked(), Some(true), "the window remembers");
}

/// CUT-6
#[test]
fn ed_6_find_raises_a_ghost() {
    let mut ui = Ui::open(DOC);
    ui.select_text("Editors");
    ui.keys("cmd-/");
    ui.caret_at(0);
    ui.keys("cmd-f");
    ui.type_text("editors");
    ui.keys("enter");
    let ghost = ui.find("Editors");
    assert!(ui.read(|ws, cx| ws.page.read(cx).raised_ghost.clone()).is_some_and(|r| r.start <= ghost.start && ghost.end <= r.end));
    ui.click_id("opt-cuts");
    assert_eq!(ui.find_state().unwrap().0, 0);
}

/// WIN-1
#[test]
fn ed_6_the_header_has_a_find_button() {
    let mut ui = Ui::open(DOC);
    assert_eq!(ui.element("find-button").checked(), Some(false));
    ui.click_id("find-button");
    assert!(ui.find_state().is_some());
    assert_eq!(ui.element("find-button").checked(), Some(true));
    assert!(!ui.has_element("scope-file"), "a single file has no scope to choose");
    ui.click_id("find-button");
    assert!(ui.find_state().is_none());
}

#[test]
fn ed_7_smart_punctuation() {
    let mut ui = Ui::open("\n");
    ui.caret_at(0);
    ui.type_text("\"Hi,\" it's a--b");
    assert_eq!(ui.text(), "“Hi,” it’s a—b\n");

    ui.set_prefs(|p| p.smart_punctuation = false);
    ui.keys("cmd-a backspace");
    ui.type_text("\"Hi\" a--b");
    assert_eq!(ui.text(), "\"Hi\" a--b");
}


#[test]
fn caret_slides_while_typing() {
    let mut ui = Ui::open(DOC);
    let start = ui.find("friction").start;
    ui.caret_at(start);
    // A new slide leaves from wherever the caret is drawn: let this one finish.
    std::thread::sleep(std::time::Duration::from_millis(120));
    let at = |ui: &Ui, c: usize| ui.point_for(c).x - gpui_kit::px(2.);

    ui.type_text("x");
    let (from, to) = ui.caret_slide().expect("a caret");
    assert_eq!(from.x, at(&ui, start), "it leaves from where it was");
    assert_eq!(to.x, at(&ui, start + 1), "and goes to after the letter");
    assert_eq!(from.y, to.y);

    ui.keys("right");
    let (from, to) = ui.caret_slide().unwrap();
    assert!(from.x < to.x && to.x == at(&ui, start + 2), "→ slides too");

    let far = ui.find("Editors").start;
    ui.click(ui.point_for(far), gpui_kit::MouseButton::Left);
    let (from, to) = ui.caret_slide().unwrap();
    assert_eq!(from, to, "a click jumps");

    ui.set_prefs(|p| p.effects = crate::prefs::Effects::Off);
    ui.type_text("y");
    let (from, to) = ui.caret_slide().unwrap();
    assert_eq!(from, to, "no slide with Effects off");
}

#[test]
fn tab_inserts_nothing_on_the_page() {
    let mut ui = Ui::open("Britain fulfilled it.\n");
    ui.caret_at(ui.find("filled").start);
    ui.keys("tab shift-tab");
    ui.type_text("\u{19}");
    assert_eq!(ui.text(), "Britain fulfilled it.\n");

    let ui = Ui::open("ful\tfilled\n");
    let gap = ui.point_for(ui.find("filled").start).x - ui.point_for(ui.find("\t").start).x;
    let space = {
        let ui = Ui::open("ful filled\n");
        ui.point_for(ui.find("filled").start).x - ui.point_for(ui.find(" ").start).x
    };
    assert!(gap > gpui_kit::px(1.), "a tab has a width");
    assert_eq!(gap, space, "as wide as a space");
}
