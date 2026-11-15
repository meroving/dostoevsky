use gpui_kit::{px, MenuItem};

use crate::prefs::Prefs;
use crate::ui_tests::{TempDir, Ui};

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n";

#[test]
fn win_1_title_shows_name_and_edited() {
    let dir = TempDir::new("win1");
    let mut ui = Ui::open_file(&dir.file("essay.md", DOC));
    assert_eq!(ui.title(), "essay.md");
    ui.caret_at(0);
    ui.type_text("X");
    assert_eq!(ui.title(), "essay.md — Edited");
    ui.keys("cmd-s");
    assert_eq!(ui.title(), "essay.md");
    assert_eq!(Ui::open(DOC).title(), "Untitled");
}

#[test]
fn win_1_edited_gives_way_in_a_narrow_window() {
    const NAME: &str = "a-very-long-file-name-for-a-narrow-window.md";
    let dir = TempDir::new("win1narrow");
    let path = dir.file(NAME, DOC);
    let edited = |w: f32| {
        let mut ui = Ui::open_file_sized(&path, w, 800.);
        ui.caret_at(0);
        ui.type_text("X");
        ui
    };

    assert_eq!(edited(1180.).header_title(), format!("{NAME} — Edited"));
    let mut narrow = edited(520.);
    assert_eq!(narrow.header_title(), NAME);
    assert_eq!(narrow.title(), format!("{NAME} — Edited"));
    assert!(narrow.dirty());
}

#[test]
fn win_2_column_width_and_position() {
    let mut ui = Ui::open(DOC);
    let (origin, w66) = ui.column();
    assert!((180.0..=210.0).contains(&f32::from(origin.y)), "top {origin:?}");
    let centre = origin.x + w66 / 2.;
    assert!((centre - px(590.)).abs() < px(2.), "centred in the 1180 px window: {centre:?}");
    let char_w = w66 / 66.;
    assert!((px(10.)..=px(12.5)).contains(&char_w), "66 mono chars at 19 px: {char_w:?}");

    ui.set_prefs(|p| p.column_chars = 74);
    let (_, w74) = ui.column();
    assert!((w74 / char_w - 74.).abs() < 0.5, "{w74:?}");
    ui.set_prefs(|p| p.column_chars = 90);
    let (_, w90) = ui.column();
    assert!((w90 / char_w - 90.).abs() < 0.5, "{w90:?}");
    ui.set_prefs(|p| p.column_chars = 58);
    let (_, w58) = ui.column();
    assert!((w58 / char_w - 58.).abs() < 0.5, "{w58:?}");
}

#[test]
fn win_2_column_counts_letters_in_a_proportional_face() {
    let mut ui = Ui::open(DOC);
    let (_, mono) = ui.column();
    ui.set_prefs(|p| p.font_family = crate::theme::QUATTRO.into());
    let (origin, w) = ui.column();
    let letter = w / 66.;
    assert!((px(9.)..=px(11.5)).contains(&letter), "an average letter at 19 px, not an m (17 px): {letter:?}");
    assert!(w < mono, "Quattro's letters are narrower than Plex Mono's: {w:?} vs {mono:?}");
    assert!((origin.x + w / 2. - px(590.)).abs() < px(2.), "still centred: {origin:?}");
}

#[test]
fn win_3_counter() {
    let mut ui = Ui::open(DOC);
    assert_eq!(ui.counter(), "11 words 62 chars");
    ui.select_text("friction in writing");
    assert_eq!(ui.counter(), "3 words 19 chars selected");
    ui.keys("cmd-/");
    ui.caret_at(0);
    assert_eq!(ui.counter(), "8 words 43 chars", "cuts are never counted (CUT-13)");

    ui.click_id("counter");
    assert!(ui.has_element("count"), "Count opens");
    assert_eq!(ui.element("counter").checked(), Some(true), "the counter shows it is open");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).layer), "the layer stays as it was");
    ui.keys("escape");
    assert!(!ui.has_element("count"), "Esc closes Count");
}

#[test]
fn win_3_count() {
    let mut ui = Ui::open(DOC);
    let cols = ui.read(|ws, cx| ws.count_columns(cx));
    assert_eq!(cols.len(), 1);
    assert_eq!((cols[0].label, cols[0].words, cols[0].chars, cols[0].paragraphs), ("document", 11, 62, 1));

    ui.select_text("friction in writing");
    let cols = ui.read(|ws, cx| ws.count_columns(cx));
    assert_eq!(cols.iter().map(|c| (c.label, c.words)).collect::<Vec<_>>(), [("selection", 3), ("document", 11)]);

    ui.click_id("counter");
    ui.click_id("counter-shows");
    assert_eq!(ui.read(|_, cx| Prefs::get(cx).counter_shows), crate::prefs::CounterShows::Words);
    assert_eq!(ui.counter(), "3 words selected");
    assert!(!ui.has_element("count-ghosted-here"), "cuts are never counted (CUT-13)");
}

#[test]
fn win_3_reading_ease() {
    let ui = Ui::open(DOC);
    assert_eq!(ui.read(|ws, cx| ws.count_columns(cx))[0].ease(), None, "too short to say");

    let page = "The cat sat on the mat. It was a warm day and the sun was out. ".repeat(8);
    let mut ui = Ui::open(&page);
    let cols = ui.read(|ws, cx| ws.count_columns(cx));
    let ease = cols[0].ease().expect("over 100 words of English");
    assert!(ease >= 90, "short words in short sentences read very easily: {ease}");
    assert_eq!(crate::count::band_label(ease), "very easy");
    assert_eq!(ui.read(|ws, cx| ws.lab_ease(cx)), Some(ease), "the AI menu reads the same number");
    ui.click_id("counter");
    assert!(ui.has_element("reading-ease"));

    let russian = "Я человек больной. Я злой человек. Непривлекательный я человек. Я думаю, что у меня болит печень. ".repeat(8);
    let ui = Ui::open(&russian);
    let cols = ui.read(|ws, cx| ws.count_columns(cx));
    assert_eq!(cols[0].reading.map(|r| r.0), Some(prose::Lang::Ru), "read as Russian");
    assert!(cols[0].ease().is_some());
}

#[test]
fn win_3_counter_gives_way_in_a_narrow_window() {
    let doc = format!("{}\n", "Most of the friction in writing comes from choosing too early. ".repeat(40));
    let ghosted = |w: f32| {
        let mut ui = Ui::open_sized(&doc, w, 800.);
        ui.select_text("friction in writing");
        ui.keys("cmd-/");
        ui.caret_at(0);
        ui
    };

    let mut wide = ghosted(1180.);
    let full = wide.counter();
    assert!(!full.contains('·'), "cuts are never counted (CUT-13): {full}");
    assert_eq!(wide.header_counter(), full);
    let plain = full.as_str();

    let mut narrow = ghosted(520.);
    assert_eq!(narrow.header_counter(), format!("{} words", plain.split(' ').next().unwrap()));
    narrow.select_text("Most of the");
    // REVIEW takes the slot in editing; the narrowest form drops "selected", which the counter's
    // brighter colour says instead.
    assert_eq!(narrow.header_counter(), "3 words");
    assert_eq!(ghosted(1180.).counter(), full, "a wide window says it all");
}

#[test]
fn win_4_mode_switch() {
    let mut ui = Ui::open(DOC);
    let editing = |ui: &Ui| ui.read(|ws, cx| ws.model.read(cx).layer);
    let selected = |ui: &mut Ui| (ui.element("mode-writing").selected(), ui.element("mode-editing").selected());
    assert_eq!(selected(&mut ui), (Some(true), Some(false)), "writing is lit");
    ui.click_id("mode-writing");
    assert!(!editing(&ui), "picking the mode you're in does nothing");
    ui.click_id("mode-editing");
    assert!(editing(&ui));
    assert_eq!(selected(&mut ui), (Some(false), Some(true)), "editing is lit");
    ui.click_id("mode-editing");
    assert!(editing(&ui), "still editing: a segment picks, it doesn't flip");
    ui.click_id("mode-writing");
    assert!(!editing(&ui));
    assert!(!ui.has_element("source"), "M↓ is gone; View Source is in the document menu");
}

#[test]
fn win_4_mode_keys_pick_and_flip() {
    let mut ui = Ui::open(DOC);
    let editing = |ui: &Ui| ui.read(|ws, cx| ws.model.read(cx).layer);
    ui.keys("cmd-2");
    assert!(editing(&ui), "⌘2 is editing");
    ui.keys("cmd-2");
    assert!(editing(&ui), "and stays editing");
    ui.keys("cmd-1");
    assert!(!editing(&ui), "⌘1 is writing");
    ui.keys("cmd-1");
    assert!(!editing(&ui));
    ui.keys("cmd-d");
    assert!(editing(&ui), "⌘D flips");
    ui.keys("cmd-d");
    assert!(!editing(&ui));
}

#[test]
fn win_4_view_menu_checks_the_mode() {
    let view = |editing: bool| {
        let menus = crate::actions::menus(&[], &crate::prefs::Prefs::default(), None, editing);
        let view = menus.into_iter().find(|m| m.name.as_ref() == "View").expect("a View menu");
        view.items
            .into_iter()
            .filter_map(|i| match i {
                gpui_kit::MenuItem::Action { name, checked, .. } if name.as_ref() == "Writing" || name.as_ref() == "Editing" => Some((name.to_string(), checked)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(view(false), vec![("Writing".into(), true), ("Editing".into(), false)]);
    assert_eq!(view(true), vec![("Writing".into(), false), ("Editing".into(), true)]);
}

#[test]
fn win_5_document_menu_opens_shortcuts() {
    let mut ui = Ui::open(DOC);
    assert!(!ui.has_element("keys"), "no keyboard icon in the header");
    ui.click_id("title");
    assert_eq!(ui.menu_labels().last().map(String::as_str), Some("Keyboard Shortcuts"));
    ui.menu("Keyboard Shortcuts");
    assert_eq!(ui.sheet(), Some("shortcuts"));
}

#[test]
fn win_6_panel_button() {
    let mut ui = Ui::open(DOC);
    assert_eq!(ui.element("panel").checked(), Some(false));
    ui.click_id("panel");
    assert!(ui.has_element("file-panel") && !ui.sidebar_open());
    assert_eq!(ui.element("switch-project").selected(), Some(true));
    assert_eq!(ui.element("panel").checked(), Some(true));
    ui.click_id("switch-alternatives");
    assert!(ui.sidebar_open());
    assert_eq!(ui.sidebar(), None, "the list of every group");
    assert!(ui.has_element("alt-empty"), "with none yet, it says how to make one");
    ui.click_id("panel");
    assert!(!ui.sidebar_open() && !ui.has_element("file-panel"));

    let id = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(id).range.start + 2);
    ui.click_id("panel");
    assert_eq!((ui.sidebar(), ui.sidebar_cursor()), (None, Some(id)));
    ui.click_id("switch-project");
    assert!(ui.has_element("file-panel") && !ui.sidebar_open());
    assert!(!ui.has_element("save") && !ui.has_element("open") && !ui.has_element("xyz"), "nothing floats in the corners");
}

#[test]
fn win_7_document_menu() {
    let dir = TempDir::new("win7");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.set_prefs(|p| p.autosave = crate::prefs::Autosave::Off);
    ui.caret_at(0);
    ui.type_text("X");
    ui.click_id("title");
    assert_eq!(
        ui.menu_labels(),
        ["Save", "Save As…", "Open…", "Copy Final Text", "Export…", "Print…", "Show in Finder", "Settings…", "Keyboard Shortcuts"]
    );
    assert_eq!(ui.read(|ws, _| ws.context_menu.as_ref().and_then(|m| m.heading.clone())).map(|h| h.0.to_string()).as_deref(), Some("unsaved changes · autosave is off"));
    ui.menu("Save");
    assert!(!ui.dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("X{DOC}"));
    ui.click_id("title");
    assert!(ui.read(|ws, cx| ws.save_state(cx).0).starts_with("saved"));
    ui.menu("Export…");
    assert_eq!(ui.sheet(), Some("export"));
}

/// WIN-1
#[test]
fn win_7_unsaved_dot_when_narrow() {
    let dir = TempDir::new("win7dot");
    let path = dir.file("a-very-long-file-name-for-a-narrow-window.md", DOC);
    let mut ui = Ui::open_file_sized(&path, 520., 600.);
    assert!(!ui.has_element("unsaved-dot"));
    ui.caret_at(0);
    ui.type_text("X");
    ui.draw();
    assert!(ui.has_element("unsaved-dot"));
    let mut wide = Ui::open_file_sized(&path, 1180., 800.);
    wide.caret_at(0);
    wide.type_text("X");
    wide.draw();
    assert!(!wide.has_element("unsaved-dot"), "“— Edited” fits");
}

#[test]
fn cut_9_the_cuts_button() {
    let mut ui = Ui::open(DOC);
    assert_eq!(ui.element("overflow-button").checked(), Some(false));
    assert_eq!(ui.element("overflow-button").label(), Some("Cuts"));
    ui.click_id("overflow-button");
    assert!(ui.drawer_open());
    assert_eq!(ui.element("overflow-button").checked(), Some(true), "accent-coloured while open");
    ui.click_id("overflow-button");
    assert!(!ui.drawer_open());
    ui.select_text("friction");
    ui.keys("cmd-shift-x");
    ui.draw();
    assert_eq!(ui.element("overflow-button").label(), Some("Cuts, 1 cut"));
}

#[test]
fn win_9_chrome_fades_while_typing() {
    let mut ui = Ui::open(DOC);
    assert!(!ui.chrome_dimmed());
    ui.caret_at(0);
    ui.type_text("X");
    assert!(ui.chrome_dimmed());
    ui.hover_text("writing");
    assert!(!ui.chrome_dimmed());
}

#[test]
fn win_10_full_screen_and_tabs() {
    let mut ui = Ui::open(DOC);
    ui.keys("ctrl-cmd-f");
    assert!(ui.is_fullscreen());
    ui.keys("ctrl-cmd-f");
    assert!(!ui.is_fullscreen());
    let tabs = ui.read(|_, cx| crate::window_options(cx).tabbing_identifier);
    assert_eq!(tabs.as_deref(), Some("dostoevsky-document"));
}

#[test]
fn win_11_menus_cover_every_command() {
    // In a book, which is when the sheet's project commands apply.
    let menus = crate::actions::menus(&[], &crate::prefs::Prefs::default(), Some(crate::project::Kind::Book), false);
    let names: Vec<String> = menus.iter().map(|m| m.name.to_string()).collect();
    for want in ["Dostoevsky", "File", "Edit", "Alternatives", "View", "Window", "Help"] {
        assert!(names.iter().any(|n| n == want), "no {want} menu in {names:?}");
    }
    fn actions(items: &[MenuItem], out: &mut Vec<Box<dyn gpui_kit::Action>>) {
        for it in items {
            match it {
                MenuItem::Action { action, .. } => out.push(action.boxed_clone()),
                MenuItem::Submenu(m) => actions(&m.items, out),
                _ => {}
            }
        }
    }
    let mut in_menus = Vec::new();
    for m in &menus {
        actions(&m.items, &mut in_menus);
    }
    let missing: Vec<String> = crate::actions::shortcut_groups()
        .into_iter()
        .flat_map(|(_, items)| items)
        // Keys rather than commands: ↑/↓ while hovering, Esc stepping back in the panel, ⇥ after ??,
        // ⇧ with the sentence keys, ⌘⌫ in a comment's note.
        .filter(|(label, _)| {
            !matches!(
                *label,
                "Cycle under pointer" | "Back / close" | "A remembered steer, after ??" | "Select to the previous sentence" | "Select to the next sentence" | "Delete Comment"
            )
        })
        .filter(|(_, a)| !in_menus.iter().any(|m| m.partial_eq(a.as_ref())))
        .map(|(label, _)| label.to_string())
        .collect();
    assert!(missing.is_empty(), "on the shortcuts sheet but in no menu: {missing:?}");
}

const ZEN_DOC: &str = "# Keep the Words\n\nMost of the friction in writing comes from choosing too early.\n\nEditors have always known this.\nA good editor doesn't erase your sentence.\n\n- one item\n";

fn zen_alpha(ui: &Ui, needle: &str) -> f32 {
    let at = ui.find(needle).start;
    ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").alpha_at(at))
}

fn zen_alphas(ui: &Ui) -> [f32; 4] {
    ["Keep", "friction", "always known", "one item"].map(|n| zen_alpha(ui, n))
}

#[test]
fn win_12_zen_dims_all_but_the_current_paragraph() {
    use crate::view::ZEN_DIM;
    let mut ui = Ui::open(ZEN_DOC);
    ui.set_prefs(|p| p.effects = crate::prefs::Effects::Off);
    assert!(!ui.read(|ws, cx| ws.model.read(cx).layer), "documents open for writing");
    let c = ui.find("always known").start;
    ui.caret_at(c);
    assert_eq!(zen_alphas(&ui), [1.; 4], "off by default");

    ui.keys("ctrl-cmd-z");
    assert!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).zen_writing));
    // Two source lines with no blank between them are one paragraph.
    assert_eq!(zen_alphas(&ui), [ZEN_DIM, ZEN_DIM, 1., ZEN_DIM]);
    assert_eq!(zen_alpha(&ui, "erase your"), 1.);

    ui.type_text("X");
    assert_eq!(zen_alphas(&ui), [ZEN_DIM, ZEN_DIM, 1., ZEN_DIM]);
    ui.caret_at(ui.find("friction").start);
    assert_eq!(zen_alphas(&ui), [ZEN_DIM, 1., ZEN_DIM, ZEN_DIM]);

    // A selection lights every paragraph it touches, but not the line it ends at the start of.
    let (a, b) = (ui.find("writing comes").start, ui.find("always").end);
    ui.select(a..b);
    assert_eq!(zen_alphas(&ui), [ZEN_DIM, 1., 1., ZEN_DIM]);
    let p = ui.find("Most of the friction in writing comes from choosing too early.");
    ui.select(p.start..p.end + 1);
    assert_eq!(zen_alphas(&ui), [ZEN_DIM, 1., ZEN_DIM, ZEN_DIM]);

    ui.keys("cmd-d");
    assert_eq!(zen_alphas(&ui), [1.; 4]);
    ui.keys("cmd-d");
    assert_eq!(zen_alphas(&ui), [ZEN_DIM, 1., ZEN_DIM, ZEN_DIM]);

    ui.keys("cmd-,");
    ui.click_id("nav-writing");
    assert_eq!(ui.element("zen").checked(), Some(true));
    ui.click_id("zen");
    assert!(!ui.read(|_, cx| crate::prefs::Prefs::get(cx).zen_writing));
    ui.keys("escape");
    assert_eq!(zen_alphas(&ui), [1.; 4]);
}

#[test]
fn win_12_zen_fades_between_paragraphs() {
    let mut ui = Ui::open(ZEN_DOC);
    ui.set_prefs(|p| p.zen_writing = true);
    ui.caret_at(ui.find("friction").start);
    ui.caret_at(ui.find("always known").start);
    assert!(ui.read(|ws, cx| ws.page.read(cx).zen_fading()));
    let mid = zen_alpha(&ui, "always known");
    assert!((crate::view::ZEN_DIM..1.).contains(&mid), "fading in: {mid}");
}

/// "needle" is in its 30th paragraph.
fn long_doc() -> String {
    let mut s = "# Keep the Words\n\n".to_string();
    for i in 0..60 {
        s += &format!("Paragraph {i} of filler, long enough to make the page scroll{}.\n\n", if i == 30 { " needle" } else { "" });
    }
    s
}

fn caret_y(ui: &Ui) -> f32 {
    f32::from(ui.point_for(ui.selection().end).y)
}

fn middle(ui: &Ui) -> f32 {
    ui.read(|ws, cx| f32::from(ws.page.read(cx).layout.as_ref().expect("laid out").bounds().center().y))
}

#[test]
fn win_2_page_follows_the_caret_both_ways() {
    let mut ui = Ui::open(&long_doc());
    let top = ui.point_for(0).y;
    ui.caret_at(0);
    ui.keys("cmd-down");
    let y = caret_y(&ui);
    assert!(y < 860. && ui.point_for(0).y < px(0.), "scrolled to the end: caret at {y}");
    ui.keys("cmd-up");
    let y = ui.point_for(0).y;
    assert!(px(0.) < y && y <= top, "back in view: {y:?}");
}

#[test]
fn win_13_typewriter_keeps_the_caret_line_in_the_middle() {
    let mut ui = Ui::open(&long_doc());
    ui.set_prefs(|p| p.effects = crate::prefs::Effects::Off);
    let top = ui.point_for(0).y;
    ui.keys("ctrl-cmd-t");
    assert!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).typewriter));
    let mid = middle(&ui);

    ui.caret_at(0);
    ui.keys("down");
    assert_eq!(ui.point_for(0).y, top, "nothing is added above the title");
    for _ in 0..20 {
        ui.keys("down");
    }
    assert!((caret_y(&ui) - mid).abs() < 1., "{} vs {mid}", caret_y(&ui));
    ui.type_text("Typing ");
    assert!((caret_y(&ui) - mid).abs() < 1.);
    ui.keys("enter enter");
    assert!((caret_y(&ui) - mid).abs() < 1., "a new paragraph comes to the middle too");

    ui.keys("cmd-down");
    assert!((caret_y(&ui) - mid).abs() < 1., "last line: {} vs {mid}", caret_y(&ui));

    let first = ui.point_for(0).y;
    let p = gpui_kit::point(ui.column().0.x + px(20.), px(mid - 120.));
    ui.click(p, gpui_kit::MouseButton::Left);
    assert_eq!(ui.point_for(0).y, first, "a click doesn't scroll");
    assert!(ui.selection().end < ui.text().chars().count() - 100, "the caret moved");
    ui.wheel_notch(p, -5.);
    assert!(ui.point_for(0).y != first, "the wheel scrolls");
    ui.type_text("x");
    assert!((caret_y(&ui) - mid).abs() < 1., "the next keystroke brings the line back");

    ui.keys("cmd-f");
    ui.type_text("needle");
    ui.keys("enter");
    assert_eq!(ui.selection(), ui.find("needle"));
    assert!((caret_y(&ui) - mid).abs() < 1., "the match is in the middle");
    ui.keys("escape");

    let before = ui.point_for(0).y;
    ui.keys("cmd-d");
    assert_eq!(ui.point_for(0).y, before);
    ui.keys("down");
    assert_eq!(ui.point_for(0).y, before, "the caret was in view, so the page stays");
    ui.keys("cmd-d");
    assert_eq!(ui.point_for(0).y, before);

    ui.keys("cmd-,");
    ui.click_id("nav-writing");
    assert_eq!(ui.element("typewriter").checked(), Some(true));
    ui.click_id("typewriter");
    assert!(!ui.read(|_, cx| crate::prefs::Prefs::get(cx).typewriter));
}

#[test]
fn win_13_typewriter_glides() {
    let mut ui = Ui::open(&long_doc());
    ui.set_prefs(|p| p.typewriter = true);
    ui.caret_at(ui.find("Paragraph 20").start);
    ui.keys("down");
    assert!(ui.read(|ws, cx| ws.page.read(cx).gliding()));
    let y = caret_y(&ui);
    assert!(y > middle(&ui) + 1., "on its way up: {y}");
}

#[test]
fn win_7_document_menu_opens_settings() {
    let mut ui = Ui::open(DOC);
    ui.click_id("title");
    ui.menu("Settings…");
    assert_eq!(ui.sheet(), Some("prefs"));
}

#[test]
fn settings_available_without_focus() {
    let mut ui = Ui::open(DOC);
    ui.win.update(|_, window, cx| window.blur(cx));
    ui.draw();
    assert!(ui.app.update(|cx| cx.is_action_available(&crate::actions::ShowPreferences)));
    ui.app.update(|cx| cx.dispatch_action(&crate::actions::ShowPreferences));
    ui.draw();
    assert_eq!(ui.sheet(), Some("prefs"));
}

#[test]
fn win_16_writing_closes_every_panel() {
    let mut ui = Ui::open_editing("Most of the friction in writing comes from choosing too early.\n\nEditors know this.\n");
    let g = ui.make_group("friction", &["tension"]);
    ui.caret_at(ui.visible_group(g).range.start + 2);
    ui.keys("cmd-shift-a");
    assert!(ui.sidebar_open());
    ui.keys("ctrl-cmd-c");
    assert!(ui.drawer_open());
    ui.keys("cmd-f");
    assert!(ui.read(|ws, _| ws.find_bar().is_some()));

    ui.action(crate::actions::ToggleLayer);
    assert!(!ui.sidebar_open(), "the alternatives panel closed");
    assert!(!ui.drawer_open(), "the drawer closed");
    assert!(ui.read(|ws, _| ws.find_bar().is_none()), "the find bar closed");
    ui.caret_at(0);
    ui.type_text("X");
    assert!(ui.text().starts_with("XMost"), "the keyboard is on the page");

    ui.action(crate::actions::ToggleLayer);
    assert!(ui.sidebar_open() && ui.drawer_open(), "editing puts back what it had");
    ui.action(crate::actions::ToggleLayer);
    assert!(!ui.sidebar_open() && !ui.drawer_open(), "writing closes them again");
}

#[test]
fn win_17_alternatives_commands_go_to_editing() {
    let mut ui = Ui::open("Most of the friction in writing comes from choosing too early.\n\nEditors know this.\n");
    let editing = |ui: &Ui| ui.read(|ws, cx| ws.model.read(cx).layer);
    let writing = |ui: &mut Ui| {
        if editing(ui) {
            ui.action(crate::actions::ToggleLayer);
        }
        assert!(!editing(ui));
    };

    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    assert!(editing(&ui) && ui.sidebar_open());
    assert!(ui.read(|ws, _| ws.mode_pulsed()), "the mode button pulses");
    ui.type_text("tension");
    ui.keys("enter");
    let g = ui.sidebar().unwrap();

    writing(&mut ui);
    ui.action(crate::actions::AllAlternatives);
    assert!(editing(&ui) && ui.sidebar_open());

    writing(&mut ui);
    ui.caret_at(ui.visible_group(g).range.start + 1);
    let before = ui.active(g);
    ui.keys("alt-cmd-down");
    assert!(editing(&ui));
    assert_ne!(ui.active(g), before, "the command still ran");

    writing(&mut ui);
    ui.caret_at(0);
    ui.keys("alt-cmd-right");
    assert!(editing(&ui));

    writing(&mut ui);
    let at = ui.find("Editors").start;
    ui.caret_at(at);
    ui.keys("alt-cmd-down");
    assert!(!editing(&ui), "nothing to act on, nothing switches");

    ui.caret_at(ui.visible_group(g).range.start + 1);
    ui.keys("alt-cmd-enter");
    assert!(editing(&ui));
    assert!(ui.groups().is_empty());
}

#[test]
fn win_17_panel_button_in_writing_opens_the_project_tab() {
    let mut ui = Ui::open_editing("Most of the friction in writing comes from choosing too early.\n");
    ui.keys("cmd-shift-a");
    assert!(ui.sidebar_open());
    ui.click_id("panel");
    assert!(!ui.sidebar_open());
    ui.action(crate::actions::ToggleLayer);
    ui.click_id("panel");
    assert!(!ui.sidebar_open() && ui.read(|ws, _| ws.panel_open()), "the Project tab");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).layer), "still writing");
}

#[test]
fn win_17_mode_switch_pulses() {
    let mut ui = Ui::open("Most of the friction.\n");
    assert!(!ui.read(|ws, _| ws.mode_pulsed()));
    ui.click_id("mode-editing");
    assert!(ui.read(|ws, _| ws.mode_pulsed()));
    ui.click_id("mode-writing");
    ui.draw();
    assert!(!ui.read(|ws, _| ws.mode_pulsed()), "writing has no pulse");
}

#[test]
fn writing_clicked_in_writing_closes_every_panel() {
    let dir = TempDir::new("writing-again");
    dir.file("01-one.md", DOC);
    dir.file("02-two.md", DOC);
    let mut ui = Ui::open_folder(&dir.0);
    ui.win.update(|ws, w, cx| ws.open_drawer(w, cx));
    ui.draw();
    let open = |ui: &Ui| ui.read(|ws, _| (ws.panel_open(), ws.drawer.is_some()));
    assert_eq!(open(&ui), (true, true), "the list and the Cuts");
    ui.click_id("mode-writing");
    assert_eq!(open(&ui), (false, false), "a second click on writing clears the page");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).layer), "still writing");
    ui.action(crate::actions::ToggleLayer);
    ui.action(crate::actions::ToggleLayer);
    assert_eq!(open(&ui), (false, false), "writing comes back as it was left");
}

#[test]
fn win_16_writing_puts_back_its_project_list() {
    let dir = TempDir::new("win16");
    dir.file("01-one.md", DOC);
    dir.file("02-two.md", DOC);
    let mut ui = Ui::open_folder(&dir.0);
    let open = |ui: &Ui| ui.read(|ws, _| ws.panel_open());
    assert!(open(&ui), "a project opens on its list");
    ui.action(crate::actions::ToggleLayer);
    assert!(open(&ui), "editing doesn't close the list you were using");
    ui.click_id("panel");
    assert!(!open(&ui));
    ui.action(crate::actions::ToggleLayer);
    assert!(open(&ui), "writing's list is back");
    ui.click_id("panel");
    ui.action(crate::actions::ToggleLayer);
    ui.click_id("panel");
    assert!(open(&ui));
    ui.action(crate::actions::ToggleLayer);
    assert!(!open(&ui), "the project list closed: writing had it closed");
}

#[test]
fn each_mode_puts_back_its_own_panels() {
    use crate::prefs::{LeftTab, ModePanels};
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| p.panels = ModePanels::default());
    let page_focused = |ui: &mut Ui| ui.read_window(|ws, w, cx| ws.page.read(cx).focus.is_focused(w));
    let editing_left = |ui: &Ui| ui.read(|_, cx| Prefs::get(cx).panels.editing.left);

    ui.keys("cmd-2");
    assert!(ui.has_element("review"), "editing opens on Review the first time");
    assert!(page_focused(&mut ui), "the keyboard stays on the page");
    ui.keys("cmd-1");
    assert!(!ui.has_element("review") && !ui.has_element("file-panel"), "writing starts with nothing open");
    assert_eq!(editing_left(&ui), Some(LeftTab::Review));

    // Editing, closed: it stays closed.
    ui.keys("cmd-2 cmd-p");
    assert!(!ui.has_element("review"));
    ui.keys("cmd-1 cmd-2");
    assert!(!ui.has_element("review") && !ui.sidebar_open(), "closed, as it was left");
    assert_eq!(editing_left(&ui), None);

    // Alternatives and Cuts in editing, the Project tab in writing: each comes back to its own.
    ui.keys("cmd-p");
    ui.click_id("switch-alternatives");
    ui.keys("ctrl-cmd-c cmd-1");
    assert!(!ui.sidebar_open() && !ui.has_element("overflow"), "writing closes editing's panels");
    ui.keys("cmd-p");
    assert!(ui.has_element("file-panel"));
    ui.keys("cmd-2");
    assert!(ui.sidebar_open() && ui.has_element("overflow") && !ui.has_element("file-panel"), "editing's panels are back");
    assert!(page_focused(&mut ui));
    ui.keys("cmd-1");
    assert!(ui.has_element("file-panel") && !ui.sidebar_open() && !ui.has_element("overflow"), "writing's Project tab is back");

    // Editing left closed only adds nothing: writing's Project tab stays.
    ui.keys("cmd-2 cmd-p ctrl-cmd-c cmd-1");
    assert!(ui.has_element("file-panel"));
    ui.keys("cmd-2");
    assert!(ui.has_element("file-panel") && !ui.has_element("overflow"));

    // A command that goes to editing on its way brings only its own panel.
    ui.keys("cmd-1 alt-cmd-l");
    assert!(ui.has_element("review") && !ui.has_element("overflow"));
}
