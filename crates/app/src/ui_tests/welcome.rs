use gpui_kit::{px, size};

use crate::actions::ToggleLayer;
use crate::ui_tests::Ui;

fn open_welcome() -> Ui {
    let mut app = Ui::app();
    let (model, caret) = app.update(|cx| crate::welcome::model(cx));
    let mut ui = Ui::window(app, model, size(px(1180.), px(860.)));
    ui.caret_at(caret);
    ui
}

fn drawn(ui: &Ui) -> Vec<bool> {
    ui.read(|ws, cx| ws.page.read(cx).comment_rects.borrow().iter().map(|(_, _, tick)| *tick).collect())
}

/// COM-2
#[test]
fn win_19_opens_as_an_unedited_copy_in_writing() {
    let mut ui = open_welcome();
    ui.read(|ws, cx| {
        let m = ws.model.read(cx);
        assert_eq!(m.title(), "Welcome");
        assert!(m.path.is_none() && !m.dirty() && !m.layer);
        assert_eq!(m.suggested.as_ref().and_then(|p| p.file_name()).unwrap(), "Welcome.md");
        assert!(!m.doc.can_undo(), "nothing to undo: the page was built, not typed");
    });
    let text = ui.text();
    let intro_end = text.find("appear in the margin.").unwrap() + "appear in the margin.".len();
    assert_eq!(ui.selection(), text[..intro_end].chars().count()..text[..intro_end].chars().count());
    assert!(!text.contains("[key:"), "keys are filled in");
    assert!(text.contains("⌘2") && text.contains("⌥⌘N"), "the default keys, as the keymap has them");
    ui.draw();
    assert!(drawn(&ui).is_empty(), "no comments in writing");
}

#[test]
fn win_19_editing_shows_five_notes_in_the_margin() {
    let mut ui = open_welcome();
    ui.action(ToggleLayer);
    ui.draw();
    assert_eq!(drawn(&ui), vec![false; 5], "five notes, all in the margin");
    assert_eq!(ui.groups().len(), 2);
    assert!(!ui.read(|ws, cx| ws.model.read(cx).dirty()), "switching modes isn't an edit");
}

#[test]
fn win_19_trying_things_edits_the_copy() {
    let mut ui = open_welcome();
    ui.action(ToggleLayer);
    let g = ui.groups()[0].id;
    ui.hover_group(g);
    ui.keys("down");
    ui.read(|ws, cx| {
        let m = ws.model.read(cx);
        assert!(m.dirty());
        assert_eq!(crate::workspace::window_title(m), "Welcome — Edited");
    });
    assert!(ui.text().contains("closet"), "the word's next version");
}

#[test]
fn win_19_in_the_help_menu() {
    let p = crate::prefs::Prefs::default();
    let help = crate::actions::menus(&[], &p, None, false).into_iter().find(|m| m.name.as_ref() == "Help").unwrap();
    let names: Vec<String> = help
        .items
        .into_iter()
        .filter_map(|it| match it {
            gpui_kit::MenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Welcome to Dostoevsky", "Keyboard Shortcuts"]);
}

#[test]
fn win_19_only_the_first_launch() {
    let mut ui = Ui::open("Text.\n");
    ui.set_prefs(|p| p.welcome_shown = true);
    assert!(!ui.read(|_, cx| crate::welcome::due(cx)));
}

#[test]
fn win_19_in_the_ui_language() {
    l10n::set_for_thread(Some(l10n::Lang::Ru));
    let mut ui = open_welcome();
    l10n::set_for_thread(None);
    assert!(ui.text().starts_with("# Добро пожаловать в Dostoevsky"));
    assert!(ui.text().contains("каморки"));
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).title()), "Добро пожаловать");
    ui.action(ToggleLayer);
    ui.draw();
    assert_eq!(drawn(&ui).len(), 5);
}
