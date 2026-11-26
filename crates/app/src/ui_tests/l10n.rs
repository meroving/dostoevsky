use gpui_kit::MenuItem;
use l10n::Lang;

use crate::prefs::Prefs;
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n";

struct Speak;

impl Speak {
    fn in_(lang: Lang) -> Speak {
        l10n::set_for_thread(Some(lang));
        Speak
    }
}

impl Drop for Speak {
    fn drop(&mut self) {
        l10n::set_for_thread(None);
    }
}

fn titles(menus: &[gpui_kit::Menu]) -> Vec<String> {
    menus.iter().map(|m| m.name.to_string()).collect()
}

fn item_names(menus: &[gpui_kit::Menu]) -> Vec<String> {
    fn walk(items: &[MenuItem], out: &mut Vec<String>) {
        for it in items {
            match it {
                MenuItem::Action { name, .. } => out.push(name.to_string()),
                MenuItem::Submenu(m) => {
                    out.push(m.name.to_string());
                    walk(&m.items, out);
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for m in menus {
        walk(&m.items, &mut out);
    }
    out
}

#[test]
fn the_menu_bar_speaks_every_language() {
    let p = Prefs::default();
    let all = |menus: &[gpui_kit::Menu]| {
        let mut out = item_names(menus);
        out.extend(titles(menus).into_iter().skip(1)); // the first is the app's name
        out
    };
    let english = all(&crate::actions::menus(&[], &p, Some(crate::project::Kind::Book), false));
    for lang in Lang::ALL.into_iter().filter(|l| *l != Lang::En) {
        let _s = Speak::in_(lang);
        let menus = crate::actions::menus(&[], &p, Some(crate::project::Kind::Book), false);
        assert!(titles(&menus).contains(&l10n::tr("Window").to_string()), "{lang:?}");
        let names = all(&menus);
        assert_eq!(names.len(), english.len());
        // Theme names (View → Theme) are names, the same in every language.
        let theme_names: Vec<String> = crate::theme::all().into_iter().map(|(_, t, _)| t.name).collect();
        let same: Vec<_> = english
            .iter()
            .zip(&names)
            .filter(|(a, b)| a == b && a.chars().any(char::is_alphabetic) && !theme_names.contains(a))
            .map(|(a, _)| a)
            .collect();
        // Some words match in a few languages ("Zoom", "File"); a missing catalog leaves them all.
        assert!(same.len() <= 3, "{lang:?}: untranslated {same:?}");
    }
}

#[test]
fn russian_menus_read_russian() {
    let _s = Speak::in_(Lang::Ru);
    let menus = crate::actions::menus(&[], &Prefs::default(), None, false);
    assert_eq!(&titles(&menus)[1..3], ["Файл", "Правка"]);
    assert_eq!([1, 3, 25].map(|n| l10n::trn!("{n} word", "{n} words", n)), ["1 слово", "3 слова", "25 слов"]);
}

#[test]
fn the_page_menu_follows_the_language() {
    let mut ui = Ui::open(DOC);
    ui.right_click_text("friction");
    let english = ui.menu_labels();
    ui.keys("escape");
    let _s = Speak::in_(Lang::Ru);
    ui.right_click_text("friction");
    let russian = ui.menu_labels();
    assert_eq!(english.len(), russian.len());
    for (en, ru) in english.iter().zip(&russian) {
        assert!(en == ru || !ru.is_ascii(), "{en:?} → {ru:?}");
    }
    assert!(russian.iter().any(|l| l.chars().any(|c| ('а'..='я').contains(&c))), "{russian:?}");
    assert_ne!(l10n::trc!("variant", "Edit"), l10n::tr!("Edit"));
}

#[test]
fn choosing_a_language_in_settings() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    // Language is General's first row.
    let ru = 1 + Lang::ALL.iter().position(|l| *l == Lang::Ru).unwrap();
    assert_eq!(ui.element("language").label(), Some(crate::os_text::match_system()), "the system's own to begin with");
    assert!(!ui.has_element(("language", 0usize)), "the list is closed");

    ui.click_id("language");
    assert_eq!(ui.element("language").expanded(), Some(true));
    assert_eq!(ui.element(("language", 0usize)).selected(), Some(true));
    ui.keys("escape");
    assert!(!ui.has_element(("language", 0usize)));
    assert_eq!(ui.sheet(), Some("prefs"));

    ui.click_id("language");
    ui.click_id(("language", ru));
    let _s = Speak; // put the thread back to English when the test ends
    assert_eq!(ui.read(|_, cx| Prefs::get(cx).language.clone()).as_deref(), Some("ru"));
    assert_eq!(l10n::current(), Lang::Ru);
    assert!(!ui.has_element(("language", ru)), "choosing closes the list");
    assert_eq!(ui.element("language").label(), Some("Русский"));
    assert_eq!(ui.element(("appearance", 0usize)).label(), Some("Тёмное"));

    ui.click_id("language");
    ui.click_id(("language", 0usize));
    assert_eq!(ui.read(|_, cx| Prefs::get(cx).language.clone()), None);
}
