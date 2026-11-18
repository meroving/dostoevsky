use gpui_kit::{font, px, Font, FontStyle, FontWeight, MenuItem, WindowAppearance};

use crate::actions::{AppearanceLight, AppearanceSystem};
use crate::prefs::{Appearance, Effects, Prefs};
use crate::theme;
use crate::ui_tests::Ui;

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n";

fn prefs(ui: &Ui) -> Prefs {
    ui.read(|_, cx| Prefs::get(cx).clone())
}

#[test]
fn pref_1_every_setting_can_be_changed() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    assert_eq!(ui.sheet(), Some("prefs"));

    // General: the window, the files, the keys.
    ui.click_id(("effects", 2usize));
    assert_eq!(ui.element("tap-shift").checked(), Some(false), "Tap ⇧ starts off");
    ui.click_id("tap-shift");
    assert_eq!(ui.element("tap-shift").checked(), Some(true));
    ui.click_id("backups");
    ui.click_id(("autosave", 1usize));
    let p = prefs(&ui);
    assert!(!p.backups);
    assert_eq!(p.autosave, crate::prefs::Autosave::Blur);
    assert!(ui.has_element("open-keymap"));
    assert!(ui.has_element("show-shortcuts"));

    // Page: the typography.
    ui.click_id("nav-page");
    ui.click_id(("font", 2usize));
    ui.click_id(("size", 3usize));
    ui.click_id(("column", 2usize));
    ui.click_id(("spacing", 1usize));
    let p = prefs(&ui);
    assert_eq!(
        (p.font_family.as_str(), p.font_size, p.column_chars, p.line_spacing, p.effects),
        (crate::theme::QUATTRO, 21., 74, 2.35, Effects::Off)
    );

    ui.click_id("nav-writing");
    for (id, on) in [("zen", false), ("typewriter", false), ("smart", true)] {
        assert_eq!(ui.element(id).checked(), Some(on), "{id} starts {on}");
        ui.click_id(id);
        assert_eq!(ui.element(id).checked(), Some(!on), "{id} switches");
    }
    assert_eq!(ui.element(("spelling", 0usize)).selected(), Some(true), "spelling starts Always");
    ui.click_id(("spelling", 1usize));
    let p = prefs(&ui);
    assert!(p.zen_writing && p.typewriter && !p.smart_punctuation);
    assert_eq!(p.spell_check, crate::prefs::SpellCheck::Editing);
    assert!(!ui.has_element("tap-shift"), "Tap ⇧ lives in General");

    ui.click_id("nav-editing");
    for id in ["hover-label", "memory", "sounds"] {
        assert_eq!(ui.element(id).checked(), Some(true), "{id} starts on");
        ui.click_id(id);
        assert_eq!(ui.element(id).checked(), Some(false), "{id} turns off");
    }
    let p = prefs(&ui);
    assert!(!p.sounds && !p.hover_label && !p.remember_pairings);
    assert!(!ui.has_element("count-ghosted"), "cuts are never counted (CUT-13)");
    assert_eq!(ui.element(("cut-look", 0usize)).selected(), Some(true), "faded text by default");
    ui.click_id(("cut-look", 1usize));
    assert_eq!(prefs(&ui).cut_look, crate::prefs::CutLook::Mark);
    assert_eq!(ui.read(|ws, cx| ws.model.read(cx).doc.presentation().look), doc_model::Look::Mark);
    assert!(!ui.has_element("ghost"), "a mark has no opacity");
    ui.click_id(("cut-look", 0usize));
    assert!(ui.has_element("ghost"), "the slider is the faded text's");
    // Ghost opacity: a click three quarters along the slider (5–30 %).
    let b = ui.element("ghost").bounds();
    ui.click(gpui_kit::point(b.left() + b.size.width * 0.75, b.center().y), gpui_kit::MouseButton::Left);
    let opacity = prefs(&ui).ghost_opacity;
    assert!((opacity - 0.24).abs() < 0.015, "{opacity}");

    ui.click_id("nav-ai");
    assert!(ui.has_element("ai-connect"));

    let saved = std::fs::read_to_string(Prefs::path().unwrap()).unwrap();
    let saved: Prefs = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved, prefs(&ui));
}

#[test]
fn pref_1_literata() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("nav-page");
    ui.click_id(("font", 3usize));
    assert_eq!(prefs(&ui).font_family, crate::theme::LITERATA);
    let (listed, ids, m) = ui.read(|_, cx| {
        let ts = cx.text_system();
        let ids = [(FontWeight::NORMAL, FontStyle::Normal), (FontWeight::BOLD, FontStyle::Normal), (FontWeight::NORMAL, FontStyle::Italic), (FontWeight::BOLD, FontStyle::Italic)]
            .map(|(weight, style)| ts.resolve_font(&Font { weight, style, ..font(crate::theme::LITERATA) }));
        let m = ts.advance(ids[0], px(19.), 'm').map(|a| f32::from(a.width)).unwrap_or(0.);
        (ts.all_font_names().iter().any(|n| n == crate::theme::LITERATA), ids, m)
    });
    assert!(listed, "Literata is registered");
    for i in 0..4 {
        for j in i + 1..4 {
            assert_ne!(ids[i], ids[j], "regular, bold, italic and bold italic are four faces, none standing in for another");
        }
    }
    assert!((m - 0.969 * 19.).abs() < 0.3, "the page is in Literata, not a fallback: m is {m} px");
}

#[test]
fn prefs_fit_a_narrow_window() {
    for w in [520., 700., 908., 1180.] {
        let mut ui = Ui::open_sized(DOC, w, 860.);
        ui.keys("cmd-,");
        let sheet = ui.element("sheet").bounds();
        // General: the window's rows, the files rows, the shortcuts row.
        for id in [("appearance", 2usize), ("hand-font", 2usize), ("effects", 2usize)] {
            let b = ui.element(id).bounds();
            assert!(b.right() <= sheet.right() && b.left() >= sheet.left(), "{id:?} outside the sheet at {w}px: {b:?} vs {sheet:?}");
        }
        for id in ["choose-folder", "open-keymap"] {
            let b = ui.element(id).bounds();
            assert!(b.right() <= sheet.right(), "{id} outside the sheet at {w}px: {b:?} vs {sheet:?}");
        }
        // Page: the sample and the typography rows.
        ui.click_id("nav-page");
        for id in [("font", 3usize), ("size", 3usize), ("column", 4usize), ("spacing", 2usize)] {
            let b = ui.element(id).bounds();
            assert!(b.right() <= sheet.right(), "{id:?} outside the sheet at {w}px: {b:?} vs {sheet:?}");
        }
        // Editing: the cuts slider inside its choice, and the volume one.
        ui.click_id("nav-editing");
        for id in ["ghost", "volume"] {
            let b = ui.element(id).bounds();
            assert!(b.right() <= sheet.right(), "{id} outside the sheet at {w}px: {b:?} vs {sheet:?}");
        }
        // Page's last row shows without scrolling in a full-size window.
        ui.click_id("nav-page");
        let b = ui.element(("paragraphs", 1usize)).bounds();
        assert!(b.right() <= sheet.right(), "paragraphs outside the sheet at {w}px: {b:?} vs {sheet:?}");
        if w >= 908. {
            assert!(b.bottom() <= sheet.bottom(), "paragraphs below the fold at {w}px: {b:?} vs {sheet:?}");
        }
    }
}

/// PREF-1: Page draws the open document above the rows, with the page's own settings.
#[test]
fn pref_1_page_shows_a_live_sample() {
    let mut ui = Ui::open("# Keep the words\n\nEvery word, sentence and paragraph can hold several versions, and the page tries each in place.\n");
    ui.keys("cmd-,");
    ui.click_id("nav-page");
    assert!(ui.has_element("page-sample"));
    assert_eq!(ui.element("page-sample-caption").label(), Some("THE OPEN DOCUMENT"));
    let before = ui.element("page-sample").bounds();
    ui.click_id(("size", 3usize));
    let after = ui.element("page-sample").bounds();
    assert!(after.size.height > before.size.height, "the sample grows with the text size: {before:?} → {after:?}");
    // Hyphenate sits on the Alignment row and stays whichever way it points.
    assert_eq!(ui.element("hyphenation").checked(), Some(false));
    ui.click_id("hyphenation");
    assert!(prefs(&ui).hyphenation);
    ui.click_id(("alignment", 1usize));
    assert_eq!(prefs(&ui).alignment, crate::prefs::Alignment::Justified);
    assert_eq!(ui.element("hyphenation").checked(), Some(true), "the switch stays with Left or Justified");
}

/// PREF-1: a document too empty or too short to show is stood in for by Dostoevsky.
#[test]
fn pref_1_a_short_document_shows_dostoevsky() {
    for doc in ["", "todo\n", "# Just a title\n\nOne line.\n"] {
        let mut ui = Ui::open(doc);
        ui.keys("cmd-,");
        ui.click_id("nav-page");
        assert!(ui.has_element("page-sample"), "{doc:?}: the sample is always there");
        assert_eq!(ui.element("page-sample-caption").label(), Some("A PASSAGE FROM DOSTOEVSKY"), "{doc:?}");
        // Enough text to wrap: the rows below still have something to move.
        let b = ui.element("page-sample").bounds();
        assert!(b.size.height > px(60.), "{doc:?}: {b:?}");
        // Once the document has a real paragraph, it takes over again.
        if doc.is_empty() {
            ui.keys("escape");
            ui.caret_at(0);
            ui.type_text("Most of the friction in writing comes from choosing too early, and the sentence moves on without the words you dropped.\n");
            ui.keys("cmd-,");
            ui.click_id("nav-page");
            assert_eq!(ui.element("page-sample-caption").label(), Some("THE OPEN DOCUMENT"));
        }
    }
}

/// ALT-24
#[test]
fn pref_1_forget_remembered_words_asks_first() {
    let mut ui = Ui::open(DOC);
    let count = |ui: &Ui| ui.read(|_, cx| crate::memory::Memory::count(cx));
    ui.with_app(crate::memory::Memory::clear);
    ui.keys("cmd-,");
    ui.click_id("nav-editing");
    assert!(!ui.has_element("forget-memory"), "nothing to forget");
    ui.with_app(|cx| {
        crate::memory::Memory::record(cx, "friction", "drag");
        crate::memory::Memory::record(cx, "choosing", "picking");
    });
    ui.draw();
    assert_eq!(count(&ui), 2);
    ui.click_id("forget-memory");
    ui.answer_prompt("Cancel");
    assert_eq!(count(&ui), 2, "Cancel keeps them");
    ui.click_id("forget-memory");
    ui.answer_prompt("Forget");
    assert_eq!(count(&ui), 0);
    assert!(!ui.has_element("forget-memory"));
}

#[test]
fn pref_1_changes_apply_live() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("nav-writing");
    ui.click_id("smart");
    assert!(!prefs(&ui).smart_punctuation);
    ui.keys("escape");
    ui.caret_at(0);
    ui.type_text("\"");
    assert!(ui.text().starts_with('"'), "{}", ui.text());
}

/// WIN-14
#[test]
fn pref_1_appearance_switches_the_theme() {
    let mut ui = Ui::open(DOC);
    assert!(!theme::is_light());
    assert_eq!(theme::bg(), theme::DARK.bg);

    ui.keys("cmd-,");
    ui.click_id(("appearance", 1usize));
    assert_eq!(prefs(&ui).appearance, Appearance::Light);
    assert!(theme::is_light());
    assert_eq!((theme::bg(), theme::text(), theme::accent()), (theme::LIGHT.bg, theme::LIGHT.text, theme::LIGHT.accent));
    let saved: Prefs = serde_json::from_str(&std::fs::read_to_string(Prefs::path().unwrap()).unwrap()).unwrap();
    assert_eq!(saved.appearance, Appearance::Light);

    ui.keys("escape");
    ui.keys("ctrl-cmd-l");
    assert_eq!(prefs(&ui).appearance, Appearance::Dark);
    assert!(!theme::is_light());
    ui.action(AppearanceLight);
    assert!(theme::is_light());
}

/// WIN-14
#[test]
fn appearance_match_macos_follows_the_system() {
    let mut ui = Ui::open(DOC);
    ui.action(AppearanceSystem);
    // The test platform reports a light system.
    assert!(theme::is_light());
    ui.app.update(|cx| theme::apply_for(WindowAppearance::Dark, cx));
    assert!(!theme::is_light());
    ui.app.update(|cx| theme::apply_for(WindowAppearance::VibrantLight, cx));
    assert!(theme::is_light());

    ui.keys("ctrl-cmd-l");
    assert_eq!(prefs(&ui).appearance, Appearance::Dark);
    ui.app.update(|cx| theme::apply_for(WindowAppearance::Light, cx));
    assert!(!theme::is_light(), "a chosen theme stays when the system changes");
}

/// WIN-14
#[test]
fn appearance_defaults() {
    assert_eq!(Prefs::default().appearance, Appearance::System);
    let old: Prefs = serde_json::from_str(r#"{"font_size": 17.0}"#).unwrap();
    assert_eq!((old.appearance, old.font_size), (Appearance::Dark, 17.));
    let light: Prefs = serde_json::from_str(r#"{"appearance": "light"}"#).unwrap();
    assert_eq!(light.appearance, Appearance::Light);
}

/// WIN-12, WIN-13
#[test]
fn zen_and_typewriter_default_on() {
    let p = Prefs::default();
    assert!(p.zen_writing && p.typewriter);
    let old: Prefs = serde_json::from_str(r#"{"zen_writing": false, "typewriter": false}"#).unwrap();
    assert!(!old.zen_writing && !old.typewriter);
}

/// WIN-14, WIN-12, WIN-13
#[test]
fn view_menu_checks_the_current_settings() {
    let p = Prefs { appearance: Appearance::Light, zen_writing: true, typewriter: false, ..Prefs::default() };
    let menus = crate::actions::menus(&[], &p, None, false);
    let view = menus.iter().find(|m| m.name.as_ref() == "View").unwrap();
    let checked = |items: &[MenuItem], name: &str| {
        items.iter().find_map(|it| match it {
            MenuItem::Action { name: n, checked, .. } if n.as_ref() == name => Some(*checked),
            _ => None,
        })
    };
    assert_eq!(checked(&view.items, "Zen Writing"), Some(true));
    assert_eq!(checked(&view.items, "Typewriter Scrolling"), Some(false));
    let appearance = view
        .items
        .iter()
        .find_map(|it| match it {
            MenuItem::Submenu(m) if m.name.as_ref() == "Appearance" => Some(m),
            _ => None,
        })
        .expect("View → Appearance");
    let marks: Vec<_> = ["Dark", "Light", "Match macOS"].iter().map(|n| checked(&appearance.items, n)).collect();
    assert_eq!(marks, [Some(false), Some(true), Some(false)]);
}

#[test]
fn kb_2_user_keymap() {
    let mut ui = Ui::open(DOC);
    let errors = ui.with_app(|cx| {
        crate::keymap::apply_text(
            r#"// my keys
            [
              { "context": "Page", "bindings": { "cmd-k": "page::GhostToggle", "cmd-/": null } },
            ]"#,
            cx,
        )
    });
    assert!(errors.is_empty(), "{errors:?}");
    ui.select_text("friction");
    ui.keys("cmd-/");
    assert!(ui.ghosts().is_empty(), "the removed default does nothing");
    ui.keys("cmd-k");
    assert_eq!(ui.ghosts(), [ui.find("friction")]);
    let label = ui.read(|_, cx| crate::context_menu::shortcut_label(&crate::actions::GhostToggle, cx));
    assert_eq!(label.as_deref(), Some("⌘K"));
}

#[test]
fn kb_2_bad_entries_are_reported() {
    let mut ui = Ui::open(DOC);
    let errors = ui.with_app(|cx| {
        crate::keymap::apply_text(r#"[{ "bindings": { "cmd-k": "page::NoSuchAction", "cmd-j": "page::GhostToggle" } }]"#, cx)
    });
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("cmd-k"), "{errors:?}");
    ui.select_text("friction");
    ui.keys("cmd-j");
    assert_eq!(ui.ghosts().len(), 1);
}

/// WIN-15
#[test]
fn background_opacity_makes_the_window_glass() {
    use gpui_kit::WindowBackgroundAppearance;
    let mut ui = Ui::open(DOC);
    let before = ui.point_for(5);
    assert!(!theme::glass());
    ui.keys("cmd-,");
    let b = ui.element("opacity").bounds();
    ui.click(gpui_kit::point(b.left() + b.size.width * 0.5, b.center().y), gpui_kit::MouseButton::Left);
    let opacity = prefs(&ui).background_opacity;
    assert!((opacity - 0.75).abs() < 0.015, "{opacity}");
    assert!(theme::glass());
    assert!((theme::surface(theme::bg()).a - opacity).abs() < 1e-6);
    assert_eq!(theme::window_background(opacity), WindowBackgroundAppearance::Blurred);
    ui.keys("escape");
    ui.draw();
    assert_eq!(ui.point_for(5), before, "the page doesn't move on a see-through window");

    ui.app.update(|cx| crate::sheets::set_prefs(cx, |p| p.background_opacity = 1.));
    assert!(!theme::glass());
    assert_eq!(theme::window_background(1.), WindowBackgroundAppearance::Opaque);
    // Old prefs files have no opacity: opaque.
    let old: Prefs = serde_json::from_str(r#"{"font_size": 17.0}"#).unwrap();
    assert_eq!(old.background_opacity, 1.);
}

/// dsky-nke1.1: ⌘= and ⌘− step the same four sizes Preferences → Page offers, and ⌘0 is the default.
#[test]
fn text_size_keys() {
    let mut ui = Ui::open(DOC);
    let size = |ui: &Ui| prefs(ui).font_size;
    let laid_out = |ui: &Ui| ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").font_size_of(0));

    ui.keys("cmd-= cmd-=");
    assert_eq!((size(&ui), laid_out(&ui)), (21., Some(px(21.))), "19 → 21 in two steps, and the page re-lays out");
    ui.keys("cmd-=");
    assert_eq!(size(&ui), 21., "the top of the four");

    ui.keys("cmd-0");
    assert_eq!(size(&ui), 19., "⌘0 returns to the default");

    ui.keys("cmd-- cmd-- cmd--");
    assert_eq!(size(&ui), 15.);
    ui.keys("cmd--");
    assert_eq!(size(&ui), 15., "the bottom of the four");

    // ⇧⌘= reaches the app as ⌘+.
    ui.keys("cmd-+");
    assert_eq!(size(&ui), 17.);
}

/// dsky-nke1.1: View → Text carries the three items, greyed out at the ends.
#[test]
fn text_size_menu_items() {
    fn with_text_menu(p: &Prefs, f: impl FnOnce(&[MenuItem])) {
        let menus = crate::actions::menus(&[], p, None, false);
        let m = menus
            .iter()
            .find(|m| m.name.as_ref() == "View")
            .expect("View menu")
            .items
            .iter()
            .find_map(|it| match it {
                MenuItem::Submenu(m) if m.name.as_ref() == "Text" => Some(m),
                _ => None,
            })
            .expect("View → Text");
        f(&m.items)
    }
    let state = |items: &[MenuItem], name: &str| {
        items.iter().find_map(|it| match it {
            MenuItem::Action { name: n, disabled, .. } if n.as_ref() == name => Some(*disabled),
            _ => None,
        })
    };

    with_text_menu(&Prefs::default(), |items| {
        let names: Vec<_> = items.iter().filter_map(|it| match it {
            MenuItem::Action { name, .. } => Some(name.as_ref().to_string()),
            _ => None,
        }).collect();
        assert_eq!(
            names.iter().take(3).map(String::as_str).collect::<Vec<_>>(),
            ["Bigger", "Smaller", "Default Size"],
            "the size items sit above alignment"
        );
        assert_eq!((state(items, "Bigger"), state(items, "Smaller")), (Some(false), Some(false)));
    });

    with_text_menu(&Prefs { font_size: 21., ..Prefs::default() }, |items| {
        assert_eq!((state(items, "Bigger"), state(items, "Smaller")), (Some(true), Some(false)), "21 is the top");
    });

    with_text_menu(&Prefs { font_size: 15., ..Prefs::default() }, |items| {
        assert_eq!((state(items, "Bigger"), state(items, "Smaller")), (Some(false), Some(true)), "15 is the bottom");
    });

    // A hand-edited size outside the four still steps back in.
    let mut p = Prefs { font_size: 18., ..Prefs::default() };
    assert!(p.step_text_size(1) && p.font_size == 19.);
    assert!(p.step_text_size(-1) && p.step_text_size(-1) && p.font_size == 15.);
    assert!(!p.step_text_size(-1));
}

/// dsky-nke1.1: the shortcuts sheet lists the three, with ⌘= as the label (the last binding).
#[test]
fn text_size_shortcut_rows() {
    let ui = Ui::open(DOC);
    let (bigger, smaller, default) = ui.read(|_, cx| {
        (
            crate::context_menu::shortcut_label(&crate::actions::TextBigger, cx),
            crate::context_menu::shortcut_label(&crate::actions::TextSmaller, cx),
            crate::context_menu::shortcut_label(&crate::actions::TextDefaultSize, cx),
        )
    });
    assert_eq!(bigger.as_deref(), Some("⌘="));
    assert_eq!(smaller.as_deref(), Some("⌘−"));
    assert_eq!(default.as_deref(), Some("⌘0"));

    let app = crate::actions::shortcut_groups()
        .into_iter()
        .find(|(name, _)| *name == "App")
        .expect("the App group");
    let labels: Vec<&str> = app.1.iter().map(|(l, _)| l.as_ref()).collect();
    for wanted in ["Bigger text", "Smaller text", "Default text size"] {
        assert!(labels.contains(&wanted), "{labels:?} is missing {wanted}");
    }
}
