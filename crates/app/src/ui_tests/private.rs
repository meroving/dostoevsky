//! Private mode (⇧⌘H): writing mode draws the page's text as dots, bars, solid bars or blurred
//! ink, the live word or sentence left readable, and a selection reads while the window has focus. Paint only: the text, the layout and every
//! command that reads the document are the same with it on.

use std::ops::Range;

use gpui_kit::{px, MenuItem, Pixels, Point};

use crate::actions::{CopyFinalMarkdown, Hide, TogglePrivate};
use crate::prefs::{HiddenLook, Prefs, SpellCheck, StaysVisible};
use crate::ui_tests::Ui;
use crate::view::MaskShape;

const DOC: &str = "Most of the friction in writing.\n";
const LONG: &str = "# Keep the Words\n\nMost of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear.\n\nEditors have always known this. A good editor doesn't erase your sentence.\n";

fn prefs(ui: &Ui) -> Prefs {
    ui.read(|_, cx| Prefs::get(cx).clone())
}

/// `DOC` with the caret after “friction” and Private mode on in `look`.
fn masked(look: HiddenLook) -> Ui {
    let mut ui = Ui::open(DOC);
    ui.caret_at(ui.find("friction").end);
    ui.set_prefs(|p| {
        p.private_mode = true;
        p.hidden_look = look;
    });
    ui.draw();
    ui
}

/// What the page hides and how: `None` when it shows its text.
fn mask(ui: &Ui) -> Option<(Range<usize>, Vec<MaskShape>)> {
    ui.read(|ws, cx| {
        let page = ws.page.read(cx);
        let (look, live) = page.private_mask(cx)?;
        Some((live.clone(), page.layout.as_ref().expect("laid out").mask(look, std::slice::from_ref(&live))))
    })
}

/// What the page paints as text, and the shapes for the rest, as drawn in the window.
fn shown(ui: &mut Ui) -> Option<(Vec<Range<usize>>, Vec<MaskShape>)> {
    ui.read_window(|ws, window, cx| {
        let page = ws.page.read(cx);
        let (look, shown) = page.private_shown(window, cx)?;
        let shapes = page.layout.as_ref().expect("laid out").mask(look, &shown);
        Some((shown, shapes))
    })
}

/// The chars a mask covers, in order: everything but spaces and `live`.
fn hidden(text: &str, live: &Range<usize>) -> Vec<(usize, char)> {
    text.chars().enumerate().filter(|(i, c)| !c.is_whitespace() && !live.contains(i)).collect()
}

/// The caret slot's top-left for `c` (the `Ui` helper's point is a click target inside it).
fn at(ui: &Ui, c: usize) -> Point<Pixels> {
    ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").point_for(c).expect("laid out"))
}

fn positions(ui: &Ui) -> Vec<Point<Pixels>> {
    (0..=ui.text().chars().count()).map(|c| at(ui, c)).collect()
}

fn quad(s: &MaskShape) -> gpui_kit::Bounds<Pixels> {
    match s {
        MaskShape::Quad { bounds, .. } => *bounds,
        other => panic!("a quad, not {other:?}"),
    }
}

fn view_items(p: &Prefs) -> Vec<(String, bool)> {
    let menus = crate::actions::menus(&[], p, None, false);
    let view = menus.iter().find(|m| m.name.as_ref() == l10n::tr!("View")).expect("View");
    view.items
        .iter()
        .filter_map(|it| match it {
            MenuItem::Action { name, checked, .. } => Some((name.to_string(), *checked)),
            _ => None,
        })
        .collect()
}

#[test]
fn off_until_turned_on() {
    let p = Prefs::default();
    assert!(!p.private_mode);
    assert_eq!((p.hidden_look, p.stays_visible), (HiddenLook::Dots, StaysVisible::Word));
    let old: Prefs = serde_json::from_str(r#"{"typewriter": false}"#).unwrap();
    assert!(!old.private_mode && old.hidden_look == HiddenLook::Dots && old.stays_visible == StaysVisible::Word, "prefs saved before it");
    let saved = serde_json::to_string(&Prefs { hidden_look: HiddenLook::Blur, stays_visible: StaysVisible::Sentence, ..Prefs::default() }).unwrap();
    assert!(saved.contains(r#""hidden_look":"blur""#) && saved.contains(r#""stays_visible":"sentence""#), "{saved}");
    assert!(mask(&Ui::open(DOC)).is_none());
}

#[test]
fn shift_cmd_h_toggles_it_and_the_view_menu_follows() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-shift-h");
    assert!(prefs(&ui).private_mode);
    assert!(mask(&ui).is_some());
    let items = view_items(&prefs(&ui));
    let at = items.iter().position(|(n, _)| n == "Private Mode").expect("View → Private Mode");
    assert_eq!(items[at - 1].0, "Typewriter Scrolling", "after Typewriter Scrolling");
    assert!(items[at].1, "checked");
    ui.action(TogglePrivate);
    assert!(!prefs(&ui).private_mode);
    assert!(!view_items(&prefs(&ui))[at].1, "unchecked");
    assert!(mask(&ui).is_none());

    let label = |ui: &Ui, a: &dyn gpui_kit::Action| ui.read(|_, cx| crate::context_menu::shortcut_label(a, cx));
    assert_eq!(label(&ui, &TogglePrivate).as_deref(), Some("⇧⌘H"));
    assert_eq!(label(&ui, &Hide).as_deref(), Some("⌘H"), "⌘H stays Hide");
    let sheet = crate::actions::shortcut_groups();
    let (_, items) = sheet.iter().find(|(_, items)| items.iter().any(|(_, a)| a.partial_eq(&TogglePrivate))).expect("on the shortcuts sheet");
    let i = items.iter().position(|(_, a)| a.partial_eq(&TogglePrivate)).unwrap();
    assert_eq!((items[i - 1].0, items[i].0), ("Typewriter scrolling", "Private mode"));
}

#[test]
fn preferences_writing_has_the_three_rows() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-,");
    ui.click_id("nav-writing");
    assert_eq!(ui.element("private").checked(), Some(false));
    ui.click_id("private");
    assert!(prefs(&ui).private_mode);
    assert_eq!(ui.element("private").checked(), Some(true));

    assert_eq!(ui.element(("hidden-look", 0usize)).selected(), Some(true), "dots by default");
    ui.click_id(("hidden-look", 3usize));
    assert_eq!(prefs(&ui).hidden_look, HiddenLook::Blur);
    assert_eq!(ui.element(("stays-visible", 0usize)).selected(), Some(true), "the live word by default");
    ui.click_id(("stays-visible", 1usize));
    assert_eq!(prefs(&ui).stays_visible, StaysVisible::Sentence);

    // The choices stay set and settable with the mode off.
    ui.click_id("private");
    assert!(!prefs(&ui).private_mode);
    assert_eq!(ui.element(("hidden-look", 3usize)).selected(), Some(true));
    ui.click_id(("hidden-look", 1usize));
    ui.click_id(("stays-visible", 2usize));
    let p = prefs(&ui);
    assert_eq!((p.private_mode, p.hidden_look, p.stays_visible), (false, HiddenLook::Bars, StaysVisible::Nothing));
}

#[test]
fn dots_sit_where_the_hidden_characters_were() {
    let ui = masked(HiddenLook::Dots);
    let (live, shapes) = mask(&ui).expect("masked");
    assert_eq!(live, ui.find("friction"), "the word the caret is in stays");
    let chars = hidden(&ui.text(), &live);
    assert_eq!(shapes.len(), chars.len(), "one dot a character");
    let lh = ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().unwrap().line_height());
    for ((c, ch), s) in chars.iter().zip(&shapes) {
        let dot = quad(s);
        let (a, b) = (at(&ui, *c), at(&ui, c + 1));
        assert!(a.x <= dot.center().x && dot.center().x <= b.x, "{ch:?}: {dot:?} between {a:?} and {b:?}");
        assert!(a.y < dot.center().y && dot.center().y < a.y + lh, "{ch:?} on its row");
        assert!(dot.size.width < b.x - a.x, "{ch:?}: inside its advance");
    }
}

#[test]
fn bars_and_solid_cover_whole_words() {
    let bars = masked(HiddenLook::Bars);
    let (live, shapes) = mask(&bars).expect("masked");
    let words: Vec<&str> = DOC.split_whitespace().filter(|w| *w != "friction").collect();
    assert_eq!(shapes.len(), words.len(), "one bar a word: {words:?}");
    for (w, s) in words.iter().zip(&shapes) {
        let r = bars.find(w);
        assert!(r.end <= live.start || r.start >= live.end, "{w} is hidden");
        let bar = quad(s);
        let (a, b) = (at(&bars, r.start), at(&bars, r.end));
        assert!((bar.left() - a.x).abs() < px(0.5) && (bar.right() - b.x).abs() < px(0.5), "{w}: {bar:?} from {a:?} to {b:?}");
    }
    let solid = masked(HiddenLook::Solid);
    let (_, tall) = mask(&solid).expect("masked");
    assert_eq!(tall.len(), words.len());
    assert!(quad(&tall[0]).size.height > quad(&shapes[0]).size.height * 2., "solid is full height");
}

#[test]
fn blurred_ink_keeps_each_hidden_letter() {
    let ui = masked(HiddenLook::Blur);
    let (live, shapes) = mask(&ui).expect("masked");
    let chars = hidden(&ui.text(), &live);
    assert_eq!(shapes.len(), chars.len());
    for ((c, ch), s) in chars.iter().zip(&shapes) {
        match s {
            MaskShape::Ink { origin, ch: drawn, .. } => {
                assert_eq!(drawn, ch);
                assert!((origin.x - at(&ui, *c).x).abs() < px(0.5), "{ch:?} where it was");
            }
            other => panic!("ink, not {other:?}"),
        }
    }
}

#[test]
fn what_stays_visible() {
    let mut ui = Ui::open(LONG);
    ui.set_prefs(|p| p.private_mode = true);
    ui.caret_at(ui.find("pick").start + 2);
    ui.draw();
    assert_eq!(mask(&ui).unwrap().0, ui.find("pick"), "the live word");
    ui.caret_at(ui.find("early.").end);
    assert_eq!(mask(&ui).unwrap().0, ui.find("early."), "with its punctuation");

    ui.set_prefs(|p| p.stays_visible = StaysVisible::Sentence);
    ui.caret_at(ui.find("pick").start);
    assert_eq!(mask(&ui).unwrap().0, ui.find("You pick a word, the sentence moves on, and the other candidates quietly disappear."));
    ui.caret_at(ui.find("Editors").start + 3);
    assert_eq!(mask(&ui).unwrap().0, ui.find("Editors have always known this."), "the caret's sentence");

    ui.set_prefs(|p| p.stays_visible = StaysVisible::Nothing);
    let (live, shapes) = mask(&ui).unwrap();
    assert!(live.is_empty());
    let all = hidden(&ui.text(), &live).len();
    // `# ` is hidden off the caret line anyway: no shapes for it.
    assert_eq!(shapes.len(), all - 1, "every character but the heading's marker");
}

#[test]
fn the_word_dries_when_the_caret_leaves_it() {
    let mut ui = masked(HiddenLook::Dots);
    ui.type_text("ly");
    assert_eq!(mask(&ui).unwrap().0, ui.find("frictionly"), "the word being typed");
    ui.type_text(" ");
    let (live, shapes) = mask(&ui).unwrap();
    assert!(live.is_empty(), "{live:?}");
    assert_eq!(shapes.len(), hidden(&ui.text(), &live).len(), "the last word is dots too");
}

#[test]
fn nothing_moves() {
    let mut ui = Ui::open(LONG);
    ui.caret_at(ui.find("friction").end);
    ui.draw();
    let before = positions(&ui);
    for look in HiddenLook::ALL {
        ui.set_prefs(|p| {
            p.private_mode = true;
            p.hidden_look = look;
        });
        ui.draw();
        assert!(mask(&ui).is_some());
        assert_eq!(positions(&ui), before, "{look:?}");
    }
    ui.keys("cmd-shift-h");
    ui.draw();
    assert_eq!(positions(&ui), before, "off again");
}

#[test]
fn editing_shows_the_text() {
    let mut ui = masked(HiddenLook::Dots);
    ui.keys("cmd-d");
    ui.draw();
    assert!(mask(&ui).is_none(), "editing");
    ui.keys("cmd-d");
    ui.draw();
    assert!(mask(&ui).is_some(), "writing again");
}

#[test]
fn spelling_marks_and_the_prompt_are_not_drawn() {
    let mut ui = Ui::open("Most of the frction in writing.\n");
    ui.use_dictionary("most of the friction in writing");
    ui.draw();
    let spelling = |ui: &Ui| ui.read(|ws, cx| ws.page.read(cx).spelling_to_draw(cx).to_vec());
    assert_eq!(spelling(&ui), [ui.find("frction")]);
    ui.keys("cmd-shift-h");
    ui.draw();
    assert!(spelling(&ui).is_empty(), "not drawn");
    assert_eq!(ui.misspelled(), [ui.find("frction")], "still checked");
    assert_eq!(prefs(&ui).spell_check, SpellCheck::Always, "the preference is the writer's");
    ui.keys("cmd-shift-h");
    ui.draw();
    assert_eq!(spelling(&ui), [ui.find("frction")]);

    let mut empty = Ui::open("");
    let prompt = |ui: &Ui| ui.read(|ws, cx| ws.page.read(cx).shows_prompt(cx));
    assert!(prompt(&empty));
    empty.keys("cmd-shift-h");
    assert!(!prompt(&empty), "no placeholder over a hidden page");
}

#[test]
fn typing_undo_keys_and_final_text_are_untouched() {
    let mut plain = Ui::open(LONG);
    plain.action(CopyFinalMarkdown);
    let final_text = plain.clipboard().expect("copied");

    let mut ui = Ui::open(LONG);
    ui.keys("cmd-shift-h");
    ui.caret_at(ui.find("friction").end);
    ui.type_text(" quietly");
    assert!(ui.text().contains("the friction quietly in"));
    ui.undo();
    assert_eq!(ui.text(), LONG);
    ui.keys("ctrl-cmd-right");
    let after_key = ui.selection();
    plain.caret_at(plain.find("friction").end);
    plain.keys("ctrl-cmd-right");
    assert_eq!(after_key, plain.selection(), "sentence keys");
    ui.action(CopyFinalMarkdown);
    assert_eq!(ui.clipboard().as_deref(), Some(final_text.as_str()), "the final text is the document's");
    assert!(prefs(&ui).private_mode);
}

#[test]
fn every_language_names_it() {
    for lang in l10n::Lang::ALL {
        l10n::set_for_thread(Some(lang));
        let items = view_items(&Prefs { private_mode: true, ..Prefs::default() });
        let name = l10n::tr!("Private Mode");
        l10n::set_for_thread(None);
        assert!(items.iter().any(|(n, on)| n == name && *on), "{lang:?}: {items:?}");
        assert!(lang == l10n::Lang::En || name != "Private Mode", "{lang:?} translates it");
    }
    l10n::set_for_thread(Some(l10n::Lang::Ru));
    let ru = (l10n::tr!("Private mode"), l10n::tr!("Hidden text looks"), l10n::tr!("Stays visible"), HiddenLook::Blur.name());
    l10n::set_for_thread(None);
    assert_eq!(ru, ("Приватный режим", "Как выглядит скрытый текст", "Остаётся видимым", "Размытые чернила"), "as in the mockup");
}

#[test]
fn a_selection_reads_while_it_is_selected() {
    let mut ui = Ui::open(LONG);
    ui.set_prefs(|p| p.private_mode = true);
    ui.select_text("the other candidates quietly disappear");
    ui.draw();
    let sel = ui.find("the other candidates quietly disappear");
    let (ranges, shapes) = shown(&mut ui).expect("masked");
    // The caret is at the selection's end, before the full stop: the live word is “disappear.”.
    let read = sel.start..ui.find("disappear.").end;
    assert_eq!(ranges, vec![read.clone()], "the selection reads, and the live word's full stop");
    let hidden_chars: Vec<(usize, char)> = ui.text().chars().enumerate().filter(|(i, c)| !c.is_whitespace() && !read.contains(i)).collect();
    assert_eq!(shapes.len(), hidden_chars.len() - 1, "every other char is a dot (`# ` hides anyway)");

    // Exactly the selection: a drag that stops mid-word shows half a word.
    let early = ui.find("too early").start;
    ui.select(early - 4..early + 6);
    ui.draw();
    let (ranges, _) = shown(&mut ui).unwrap();
    assert_eq!(ranges, vec![early - 4..ui.find("early.").end], "“ing too ea” and the caret's word, “early.”, which it runs into");

    ui.keys("right");
    ui.draw();
    let (ranges, _) = shown(&mut ui).unwrap();
    assert_eq!(ranges, vec![ui.find("early.")], "the selection gone: the live word only");
}

#[test]
fn a_selection_ending_mid_word_reads_with_the_caret_word() {
    let mut ui = Ui::open(LONG);
    ui.set_prefs(|p| p.private_mode = true);
    let (a, b) = (ui.find("pick").start, ui.find("sentence").start + 3);
    ui.select(a..b);
    let (ranges, _) = shown(&mut ui).unwrap();
    assert_eq!(ranges, vec![a..ui.find("sentence").end], "one range: the selection runs into the live word");

    ui.set_prefs(|p| p.stays_visible = StaysVisible::Nothing);
    ui.draw();
    assert_eq!(shown(&mut ui).unwrap().0, vec![a..b], "with nothing else visible, exactly the selection");
}

#[test]
fn a_window_in_the_background_hides_its_selection() {
    let mut ui = Ui::open(LONG);
    ui.set_prefs(|p| p.private_mode = true);
    ui.select_text("You pick a word");
    ui.draw();
    assert_eq!(shown(&mut ui).unwrap().0, vec![ui.find("You pick a word,")], "the selection and the live word's comma");
    ui.win.update(|_, window, cx| window.blur(cx));
    ui.draw();
    let (ranges, _) = shown(&mut ui).unwrap();
    assert_eq!(ranges, vec![ui.find("word,")], "the caret's word only");
    assert_eq!(ui.selection(), ui.find("You pick a word"), "the selection is kept");
}

#[test]
fn editing_and_off_draw_no_mask_with_a_selection() {
    let mut ui = Ui::open(LONG);
    ui.select_text("You pick a word");
    ui.draw();
    assert!(shown(&mut ui).is_none(), "off");
    ui.set_prefs(|p| p.private_mode = true);
    ui.keys("cmd-d");
    ui.draw();
    assert!(shown(&mut ui).is_none(), "editing");
}

#[test]
fn a_selection_reads_with_its_hyphens() {
    use crate::prefs::Alignment;
    const BOOK: &str = "# Chapter One\n\nHe was hopelessly in debt to his landlady, and was afraid of meeting her. \
This was not because he was cowardly and abject, quite the contrary; but for some time past he had been in \
an overstrained irritable condition, verging on hypochondria.\n";
    let mut ui = Ui::open(BOOK);
    ui.set_prefs(|p| {
        p.alignment = Alignment::Justified;
        p.hyphenation = true;
        p.private_mode = true;
    });
    ui.select(ui.find("He was").start..ui.find("hypochondria.").end);
    ui.draw();
    let (shown_ranges, _) = shown(&mut ui).expect("masked");
    let (ends, live) = ui.read(|ws, cx| {
        let layout = ws.page.read(cx).layout.as_ref().expect("laid out");
        (layout.row_ends(2), layout.live_rects_on(2, &shown_ranges))
    });
    let broken: Vec<(usize, Pixels)> = ends.iter().enumerate().filter(|(_, (_, h))| *h).map(|(i, (x, _))| (i, *x)).collect();
    assert!(!broken.is_empty(), "some word is broken: {ends:?}");
    let top = live.iter().map(|r| r.top()).fold(px(1e9), |a, b| if b < a { b } else { a });
    for (row, x) in broken {
        let y = top + ui.line_height() * (row as f32 + 0.5);
        let inside = live.iter().any(|r| r.left() < x - px(1.) && x - px(1.) <= r.right() && r.top() <= y && y < r.bottom());
        assert!(inside, "row {row}'s hyphen, ending at {x:?}, is painted as text: {live:?}");
    }
}
