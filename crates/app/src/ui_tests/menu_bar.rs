//! The menu bar drawn in the header off the Mac (dsky-fbso.9).

use gpui_kit::{px, Modifiers, MouseButton};

use crate::menu_bar::Row;
use crate::ui_tests::{key_label, Ui};

const DOC: &str = "Most of the friction in writing comes from choosing too early. You pick a word and move on.\n\n\
Editors have always known this. They write a suggestion in the margin.\n";

fn shown(ui: &Ui) -> (Option<usize>, Vec<Option<usize>>) {
    ui.read(|ws, _| ws.menu_bar.shown())
}

fn panels(ui: &mut Ui) -> Vec<Vec<Row>> {
    ui.read_window(|ws, window, cx| ws.menu_panels(window, cx).into_iter().map(|p| p.rows).collect())
}

fn row<'a>(rows: &'a [Row], label: &str) -> (usize, &'a Row) {
    rows.iter().enumerate().find(|(_, r)| r.label == label).unwrap_or_else(|| panic!("no row {label:?} in {:?}", rows.iter().map(|r| &r.label).collect::<Vec<_>>()))
}

fn title(i: usize) -> (&'static str, usize) {
    ("menu-title", i)
}

fn tap_alt(ui: &mut Ui) {
    ui.modifiers(Modifiers { alt: true, ..Default::default() });
    ui.modifiers(Modifiers::default());
}

#[test]
fn the_menus_start_the_header_and_open_on_a_click() {
    let mut ui = Ui::open(DOC);
    let names: Vec<_> = (0..5).map(|i| ui.element(title(i)).bounds()).collect();
    for pair in names.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "side by side: {pair:?}");
    }
    let panel = ui.element("panel").bounds();
    assert!(names[4].right() < panel.left(), "before the panel button: {:?} {panel:?}", names[4]);
    assert!((names[0].center().y - px(18.)).abs() < px(1.5), "on the header's line: {:?}", names[0]);

    ui.click_id(title(0));
    assert_eq!(shown(&ui), (Some(0), vec![None]), "File opens with no row lit");
    let file = &panels(&mut ui)[0];
    assert_eq!(row(file, "Save").1.keys.as_deref(), Some(key_label("⌘S").as_str()), "keys as this system writes them");
    assert_eq!(row(file, crate::os_text::quit()).1.keys.as_deref(), Some(key_label("⌘Q").as_str()));
    let menu = ui.element(("menu-panel", 0usize)).bounds();
    assert!((menu.left() - names[0].left()).abs() < px(0.5) && menu.top() >= names[0].bottom(), "under its name: {menu:?}");

    // The pointer moves to another name: its menu opens instead.
    let view = ui.element(title(3)).bounds().center();
    ui.move_pointer(view);
    assert_eq!(shown(&ui).0, Some(3));
    // A click on the open menu's name closes it, and the keys go back to the page.
    ui.click_id(title(3));
    assert_eq!(shown(&ui), (None, vec![]));
    ui.caret_at(0);
    ui.type_text("X");
    assert!(ui.text().starts_with('X'), "typing reaches the page");
}

#[test]
fn checks_and_greyed_items_follow_the_window() {
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| p.typewriter = true);
    ui.click_id(title(3));
    let view = &panels(&mut ui)[0];
    assert!(row(view, "Typewriter Scrolling").1.checked);
    assert!(!row(view, "Zen Writing").1.checked);
    assert!(row(view, "Writing").1.checked, "this window's mode");
    ui.click_id(title(3));
    ui.click_id(title(0));
    let file = &panels(&mut ui)[0];
    // A document never saved has no folder.
    assert!(!row(file, "Open Enclosing Folder").1.enabled);
    assert!(row(file, "Save").1.enabled);
    assert!(row(file, "Preferences…").1.enabled, "the app's own actions are always there");
    assert!(file.iter().all(|r| r.label != "Hide Dostoevsky" && r.label != "Services"), "nothing of the Mac's app menu");
}

#[test]
fn an_item_runs_where_the_keys_were() {
    let mut ui = Ui::open(DOC);
    ui.select_text("friction in writing");
    let sel = ui.selection();
    ui.click_id(title(1));
    let edit = panels(&mut ui).remove(0);
    let (i, copy) = row(&edit, "Copy");
    assert!(copy.enabled);
    ui.click_id(format!("menu-row-0-{i}"));
    assert_eq!(shown(&ui), (None, vec![]), "the menu closes");
    assert_eq!(ui.clipboard().as_deref(), Some("friction in writing"));
    assert_eq!(ui.selection(), sel, "the selection stays");
}

#[test]
fn a_submenu_opens_beside_its_row() {
    let mut ui = Ui::open(DOC);
    ui.click_id(title(3));
    let (i, _) = row(&panels(&mut ui)[0], "Appearance");
    let at = ui.element(format!("menu-row-0-{i}")).bounds();
    ui.move_pointer(at.center());
    assert_eq!(shown(&ui).1, vec![Some(i), None]);
    let sub = ui.element(("menu-panel", 1usize)).bounds();
    let menu = ui.element(("menu-panel", 0usize)).bounds();
    assert!(sub.left() >= menu.right() - px(4.), "beside it: {sub:?} {menu:?}");
    assert!((sub.top() - (at.top() - px(5.))).abs() < px(1.), "level with its row: {sub:?} {at:?}");
    let (light, _) = row(&panels(&mut ui)[1], "Light");
    ui.click_id(format!("menu-row-1-{light}"));
    assert_eq!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).appearance), crate::prefs::Appearance::Light);
}

#[test]
fn alt_puts_the_keys_on_the_bar() {
    let mut ui = Ui::open(DOC);
    tap_alt(&mut ui);
    assert_eq!(shown(&ui), (Some(0), vec![]), "File lit, nothing open");
    ui.keys("down");
    assert_eq!(shown(&ui), (Some(0), vec![Some(0)]), "File opens with its first row lit");
    ui.keys("down");
    assert_eq!(shown(&ui).1, vec![Some(1)]);
    ui.keys("right");
    assert_eq!(shown(&ui), (Some(1), vec![Some(0)]), "Edit, its first row lit");
    ui.keys("left left");
    assert_eq!(shown(&ui).0, Some(4), "round to Help");
    ui.keys("up");
    let help = &panels(&mut ui)[0];
    assert_eq!(help[shown(&ui).1[0].unwrap()].label, "About Dostoevsky", "up from the top is the last row");
    ui.keys("escape");
    assert_eq!(shown(&ui), (Some(4), vec![]), "Esc closes the menu, the bar keeps the keys");
    ui.keys("escape");
    assert_eq!(shown(&ui), (None, vec![]));
    ui.caret_at(0);
    ui.type_text("X");
    assert!(ui.text().starts_with('X'), "the keys are back on the page");
    // Alt again lets go of the bar.
    tap_alt(&mut ui);
    tap_alt(&mut ui);
    assert_eq!(shown(&ui), (None, vec![]));
}

#[test]
fn enter_runs_the_lit_item() {
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| p.typewriter = false);
    tap_alt(&mut ui);
    ui.keys("right right right down");
    let view = panels(&mut ui).remove(0);
    let (i, _) = row(&view, "Typewriter Scrolling");
    while shown(&ui).1[0] != Some(i) {
        ui.keys("down");
    }
    ui.keys("enter");
    assert!(ui.read(|_, cx| crate::prefs::Prefs::get(cx).typewriter));
    assert_eq!(shown(&ui), (None, vec![]));
}

/// Alt+letter opens a menu by its access key; letters Alt already presses something with are
/// skipped (Alt+A is AI alternatives off the Mac).
#[test]
fn alt_and_a_letter_open_a_menu() {
    let mut ui = Ui::open(DOC);
    ui.keys("alt-v");
    assert_eq!(shown(&ui), (Some(3), vec![Some(0)]));
    ui.keys("escape escape");
    let alternatives = crate::actions::menu_titles()[2];
    let key = ui.read(|ws, cx| {
        let _ = ws;
        let taken: Vec<char> = cx
            .key_bindings()
            .borrow()
            .bindings()
            .filter_map(|b| match b.keystrokes() {
                [k] if k.modifiers().alt && !k.modifiers().control && !k.modifiers().shift => k.key().chars().next(),
                _ => None,
            })
            .collect();
        crate::menu_bar::mnemonics(&crate::actions::menu_titles(), |c| taken.contains(&c))[2]
    });
    let letter = alternatives.chars().nth(key.expect("Alternatives has a key")).unwrap().to_lowercase().to_string();
    assert!(letter != "a", "Alt+A stays AI alternatives");
    ui.keys(&format!("alt-{letter}"));
    assert_eq!(shown(&ui).0, Some(2));
}

#[test]
fn a_click_outside_closes_the_menu() {
    let mut ui = Ui::open(DOC);
    ui.click_id(title(0));
    assert!(ui.has_element(("menu-panel", 0usize)));
    // Beside the open menu, on the page.
    let menu = ui.element(("menu-panel", 0usize)).bounds();
    ui.click(gpui_kit::point(menu.right() + px(200.), px(300.)), MouseButton::Left);
    assert_eq!(shown(&ui), (None, vec![]));
    assert!(!ui.has_element(("menu-panel", 0usize)));
}

/// A window too narrow for the names: one ≡ holds the menus.
#[test]
fn a_narrow_window_folds_the_menus() {
    let mut ui = Ui::open_sized(DOC, 800., 600.);
    assert!(!ui.has_element(title(1)));
    let fold = ui.element(title(0)).bounds();
    assert!(fold.size.width < px(40.), "{fold:?}");
    ui.click_id(title(0));
    let names: Vec<String> = panels(&mut ui)[0].iter().map(|r| r.label.clone()).collect();
    assert_eq!(names, crate::actions::menu_titles());
    assert!(panels(&mut ui)[0].iter().all(|r| r.submenu));
    // The title keeps its room.
    let panel = ui.element("panel").bounds();
    assert!(fold.right() < panel.left());
}
