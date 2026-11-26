use gpui_kit::MouseButton;

use crate::prefs::{Appearance, Prefs};
use crate::sheets::Sheet;
use crate::theme::{self, HexColor, Theme};
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n";

fn prefs(ui: &Ui) -> Prefs {
    ui.read(|_, cx| Prefs::get(cx).clone())
}

fn editor<R>(ui: &Ui, f: impl FnOnce(&crate::theme_ui::ThemeEditor) -> R) -> R {
    ui.read(|ws, _| match &ws.sheet {
        Some(Sheet::ThemeEditor(editor)) => f(editor),
        _ => panic!("theme editor is closed"),
    })
}

fn replace_seed(ui: &mut Ui, seed: u8, text: &str) {
    ui.read_window(|ws, window, cx| {
        let Some(Sheet::ThemeEditor(editor)) = &ws.sheet else { panic!("theme editor is closed") };
        let field = editor.seeds[&(editor.half, seed)].clone();
        let focus = field.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    ui.keys("cmd-a");
    ui.type_text(text);
}

fn right_click(ui: &mut Ui, id: &str) {
    let b = ui.element(gpui_kit::SharedString::from(id.to_string())).bounds();
    ui.click(b.center(), MouseButton::Right);
}

fn install(ui: &mut Ui, id: &str, name: &str) {
    let name = name.to_string();
    let id = id.to_string();
    ui.with_app(move |_| {
        let mut theme = theme::builtin("aurora").unwrap();
        theme.name = name;
        theme::save(&id, &theme).unwrap();
    });
}

#[test]
fn general_picks_a_theme_and_appearance_picks_its_half() {
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| p.appearance = Appearance::Dark);
    ui.keys("cmd-,");
    assert!(ui.has_element(("appearance", 1usize)), "Appearance is in General");
    for id in ["theme-dostoevsky", "theme-petersburg", "theme-manuscript", "theme-white-nights", "theme-aurora", "new-theme"] {
        assert!(ui.has_element(id), "{id}");
    }

    ui.click_id("theme-manuscript");
    assert_eq!((prefs(&ui).theme.as_str(), prefs(&ui).appearance), ("manuscript", Appearance::Dark), "a tile picks the pair, not the half");
    assert_eq!(theme::palette().bg, theme::builtin("manuscript").unwrap().dark.paper.0);

    ui.click_id(("appearance", 1usize));
    assert_eq!(prefs(&ui).theme, "manuscript");
    assert_eq!(theme::palette().bg, theme::builtin("manuscript").unwrap().light.paper.0);
}

#[test]
fn a_builtin_offers_only_a_copy() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    let at = ui.element("theme-petersburg").bounds().center();
    ui.click_n(at, 1);
    ui.click_n(at, 2);
    assert_eq!(ui.sheet(), Some("prefs"), "double-clicking a built-in never edits it");

    right_click(&mut ui, "theme-petersburg");
    assert_eq!(ui.menu_labels(), ["Edit a Copy…"]);
    ui.menu("Edit a Copy…");
    assert_eq!(ui.sheet(), Some("theme-editor"));
    let (name, id) = ui.read(|ws, cx| match &ws.sheet {
        Some(Sheet::ThemeEditor(editor)) => (editor.name.read(cx).field_text().to_string(), editor.id.clone()),
        _ => panic!("theme editor is closed"),
    });
    assert_eq!((name.as_str(), id), ("Petersburg copy", None), "a copy never overwrites the built-in");

    let panel = |ui: &Ui| editor(ui, |e| e.draft.light.palette(true).bg_panel);
    ui.click_id("theme-half-light");
    let before = panel(&ui);
    replace_seed(&mut ui, 0, "#202830");
    assert_ne!(panel(&ui), before, "the copy's colours follow its seeds");
}

#[test]
fn new_theme_takes_three_colours_a_half() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("new-theme");
    assert_eq!(ui.sheet(), Some("theme-editor"));

    ui.click_id("theme-save");
    assert_eq!(ui.sheet(), Some("theme-editor"), "an unnamed theme cannot be saved");
    ui.click_id("theme-name");
    ui.type_text("Blue Hour");
    ui.click_id("theme-half-light");
    replace_seed(&mut ui, 0, "#fff7ed");
    ui.click_id("theme-half-dark");
    replace_seed(&mut ui, 0, "#101827");
    replace_seed(&mut ui, 1, "#e6d5c3");
    ui.click_id("theme-save");

    assert_eq!(ui.sheet(), None);
    assert_eq!(prefs(&ui).theme, "blue-hour");
    let path = crate::files::support_dir().unwrap().join("Themes/blue-hour.json");
    let saved: Theme = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved.name, "Blue Hour");
    assert_eq!((saved.light.paper.0, saved.dark.paper.0, saved.dark.ink.0), (0xfff7ed, 0x101827, 0xe6d5c3));
    ui.keys("cmd-,");
    assert!(ui.has_element("theme-blue-hour"));
}

#[test]
fn editing_keeps_colours_set_in_the_file() {
    let mut ui = Ui::open(DOC);
    ui.with_app(|_| {
        let mut theme = theme::builtin("petersburg").unwrap();
        theme.name = "Tuned".into();
        theme.light.colors.insert("bg-panel".into(), HexColor(0xf0e5d8));
        theme::save("tuned", &theme).unwrap();
    });
    ui.keys("cmd-,");
    let at = ui.element("theme-tuned").bounds().center();
    ui.click_n(at, 1);
    ui.click_n(at, 2);
    assert_eq!(editor(&ui, |e| e.id.clone()), Some("tuned".into()), "double-click edits a theme of one's own");
    ui.click_id("theme-half-light");
    replace_seed(&mut ui, 2, "#336699");
    ui.click_id("theme-save");

    let saved = theme::find("tuned").unwrap();
    assert_eq!(saved.light.accent.0, 0x336699);
    assert_eq!(saved.light.colors["bg-panel"].0, 0xf0e5d8, "a colour set in the file survives the editor");
}

#[test]
fn the_colour_well_follows_the_colour_panel() {
    let mut ui = Ui::open(DOC);
    ui.action(crate::actions::CreateThemeSheet);
    ui.click_id("theme-half-dark");
    let accent = editor(&ui, |e| e.draft.dark.accent.0);
    ui.click_id("theme-well-2");
    assert_eq!(crate::platform::colour_panel_colour(), Some(accent), "the panel opens on the seed");

    crate::platform::show_colour_panel(0x2d3f6b);
    assert!(ui.wait_for(3., |ui| editor(ui, |e| e.draft.dark.accent.0) == 0x2d3f6b), "the seed takes the panel's colour");
    let field = ui.read(|ws, cx| match &ws.sheet {
        Some(Sheet::ThemeEditor(e)) => e.seeds[&(false, 2)].read(cx).field_text().to_string(),
        _ => panic!("theme editor is closed"),
    });
    assert_eq!(field, "#2d3f6b");
    assert!(ui.has_element("theme-hard-2"), "a dim accent on a dark paper is flagged");
    assert!(!ui.has_element("theme-hard-1"));

    ui.click_id("theme-cancel");
    assert!(ui.wait_for(3., |_| crate::platform::colour_panel_colour().is_none()), "closing the sheet closes the panel");
}

#[test]
fn importing_a_vscode_file_saves_and_takes_it() {
    let mut ui = Ui::open(DOC);
    ui.action(crate::actions::CreateThemeSheet);
    let night = crate::theme_import::from_json(r##"{"name":"Night Owl","type":"dark","colors":{"editor.background":"#011627","editor.foreground":"#d6deeb"}}"##, "x");
    ui.update_window(|ws, window, cx| ws.take_imported(vec![night], window, cx));
    assert_eq!(ui.sheet(), None);
    assert_eq!(prefs(&ui).theme, "night-owl");
    assert_eq!(theme::find("night-owl").unwrap().dark.paper.0, 0x011627);

    ui.action(crate::actions::CreateThemeSheet);
    ui.update_window(|ws, window, cx| ws.take_imported(vec![Err("Not a theme file.".into())], window, cx));
    assert_eq!(ui.sheet(), Some("theme-editor"), "a bad file leaves the sheet open with the reason");
}

#[test]
fn deleting_a_theme_of_ones_own_restores_dostoevsky() {
    let mut ui = Ui::open(DOC);
    install(&mut ui, "temporary", "Temporary");
    ui.keys("cmd-,");
    ui.click_id("theme-temporary");
    assert_eq!(prefs(&ui).theme, "temporary");
    let path = crate::files::support_dir().unwrap().join("Themes/temporary.json");

    right_click(&mut ui, "theme-temporary");
    assert_eq!(ui.menu_labels(), ["Edit…", "Duplicate", crate::os_text::show_in_file_manager(), "Delete…"]);
    ui.menu("Delete…");
    ui.answer_prompt("Keep");
    assert!(path.is_file());
    right_click(&mut ui, "theme-temporary");
    ui.menu("Delete…");
    ui.answer_prompt("Delete");

    assert!(!path.exists());
    assert_eq!(prefs(&ui).theme, "dostoevsky");
    assert_eq!(ui.text(), DOC);
    assert!(!ui.has_element("theme-temporary"));
}

#[test]
fn view_menu_offers_a_new_theme() {
    let menus = crate::actions::menus(&[], &Prefs::default(), None, false);
    let view = menus.into_iter().find(|m| m.name.as_ref() == "View").unwrap();
    let themes = view
        .items
        .iter()
        .find_map(|it| match it {
            gpui_kit::MenuItem::Submenu(m) if m.name.as_ref() == "Theme" => Some(m),
            _ => None,
        })
        .expect("View → Theme");
    let names: Vec<String> = themes
        .items
        .iter()
        .filter_map(|it| match it {
            gpui_kit::MenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Dostoevsky", "Petersburg", "Manuscript", "White Nights", "Aurora", "New Theme…"]);
}
