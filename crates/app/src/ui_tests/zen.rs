//! Zen writing's dials (WIN-12): what stays bright (sentence, line, paragraph) and how the rest
//! fades (flat, near, the whole page). Whatever stays bright, the fade counts wrapped rows outward
//! from it, four to a step and eased between; the rest of a bright row is step one.

use std::ops::Range;

use crate::prefs::{Effects, Prefs, ZenFade, ZenFocus};
use crate::ui_tests::Ui;
use crate::view::ZEN_DIM;

const DOC: &str = "# Keep the Words\n\nMost of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear. Editors have always known this. A good editor doesn't erase your sentence. They write a suggestion in the margin and let you decide.\n\nSecond paragraph here.\n\nThird paragraph here.\n\nFourth paragraph here.\n\nFifth paragraph here.\n\nSixth one.\n";

const NEAR: [f32; 4] = [1., 0.55, 0.42, ZEN_DIM];
const WHOLE: [f32; 6] = [1., 0.42, 0.24, 0.12, 0.06, 0.03];

fn step(table: &[f32], s: usize) -> f32 {
    table[s.min(table.len() - 1)]
}

/// `d` rows from what stays bright: four rows to a step, eased between.
fn row_step(table: &[f32], d: usize) -> f32 {
    if d == 0 {
        return 1.;
    }
    let i = (d - 1) as f32 / 4.;
    let (lo, t) = (i.floor() as usize, i.fract());
    if t == 0. { step(table, lo + 1) } else { step(table, lo + 1) + (step(table, lo + 2) - step(table, lo + 1)) * t }
}

fn zen(focus: ZenFocus, fade: ZenFade) -> Ui {
    let mut ui = Ui::open(DOC);
    ui.set_prefs(|p| {
        p.effects = Effects::Off;
        p.zen_writing = true;
        p.zen_focus = focus;
        p.zen_fade = fade;
    });
    ui
}

fn alpha_at(ui: &Ui, c: usize) -> f32 {
    ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").alpha_at(c))
}

fn alpha(ui: &Ui, needle: &str) -> f32 {
    alpha_at(ui, ui.find(needle).start)
}

fn alphas(ui: &Ui, needles: &[&str]) -> Vec<f32> {
    needles.iter().map(|n| alpha(ui, n)).collect()
}

/// The long paragraph's wrapped rows, as char ranges.
fn rows(ui: &Ui) -> Vec<Range<usize>> {
    rows_of(ui, ui.find("Most of the friction").start..ui.find("let you decide.").end)
}

/// The wrapped rows of `chars`, as char ranges (a row's trailing space included).
fn rows_of(ui: &Ui, chars: Range<usize>) -> Vec<Range<usize>> {
    let (p, end) = (chars.clone(), chars.end);
    let y = |c: usize| ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").point_for(c).expect("laid out").y);
    let mut rows: Vec<Range<usize>> = Vec::new();
    for c in p.start..end {
        match rows.last_mut() {
            Some(r) if y(r.start) == y(c) => r.end = c + 1,
            _ => rows.push(c..c + 1),
        }
    }
    rows
}

#[test]
fn sentence_keeps_the_caret_sentence_bright() {
    let mut ui = zen(ZenFocus::Sentence, ZenFade::Off);
    ui.caret_at(ui.find("pick a word").start);
    assert_eq!(alpha(&ui, "You pick"), 1.);
    assert_eq!(alpha(&ui, "disappear."), 1.);
    assert_eq!(alphas(&ui, &["Keep", "Most of", "Editors", "Second"]), [ZEN_DIM; 4]);

    // Right after its full stop the sentence just written stays lit; the next one lights as it starts.
    ui.caret_at(ui.find("disappear.").end);
    assert_eq!(alphas(&ui, &["You pick", "Editors"]), [1., ZEN_DIM]);
    ui.caret_at(ui.find("Editors").start);
    assert_eq!(alphas(&ui, &["You pick", "Editors"]), [ZEN_DIM, 1.]);

    // A selection lights every block it touches, whatever the unit.
    ui.select(ui.find("pick a word"));
    assert_eq!(alphas(&ui, &["Most of", "Editors", "Second"]), [1., 1., ZEN_DIM]);

    // A heading or a list item is one sentence of its own.
    ui.caret_at(ui.find("Second").start);
    assert_eq!(alphas(&ui, &["Second", "Most of", "Third"]), [1., ZEN_DIM, ZEN_DIM]);
}

/// Every visible char's wrapped row, numbered down the page (`None` for spaces).
fn page_rows(ui: &Ui) -> Vec<Option<usize>> {
    let text: Vec<char> = ui.text().chars().collect();
    let y = |c: usize| {
        ui.read(|ws, cx| f32::from(ws.page.read(cx).layout.as_ref().expect("laid out").point_for(c).expect("laid out").y))
    };
    let ys: Vec<Option<f32>> = (0..text.len()).map(|c| (!text[c].is_whitespace()).then(|| y(c))).collect();
    let mut rows: Vec<f32> = ys.iter().flatten().copied().collect();
    rows.sort_by(f32::total_cmp);
    rows.dedup();
    ys.iter().map(|y| y.map(|y| rows.iter().position(|r| *r == y).expect("a row"))).collect()
}

/// Each visible char outside `bright` is as faint as its distance in rows from the bright ones.
fn check_fade(ui: &Ui, table: &[f32], bright: &dyn Fn(usize) -> bool, what: &str) {
    let rows = page_rows(ui);
    let lit: Vec<usize> = rows.iter().enumerate().filter(|(c, r)| r.is_some() && bright(*c)).filter_map(|(_, r)| *r).collect();
    let (lo, hi) = (*lit.iter().min().expect("something lit"), *lit.iter().max().expect("something lit"));
    for (c, r) in rows.iter().enumerate() {
        let Some(r) = *r else { continue };
        let want = if bright(c) { 1. } else { row_step(table, (if r < lo { lo - r } else { r.saturating_sub(hi) }).max(1)) };
        assert_eq!(alpha_at(ui, c), want, "{what}: char {c} on row {r} (lit {lo}..={hi})");
    }
}

#[test]
fn distance_fade_counts_rows_whatever_stays_bright() {
    let mut ui = zen(ZenFocus::Sentence, ZenFade::Near);
    let caret = ui.find("pick a word").start;
    ui.caret_at(caret);
    let sentence = ui.find("You pick a word, the sentence moves on, and the other candidates quietly disappear.");
    let paragraph = ui.find("Most of the friction").start..ui.find("let you decide.").end;
    for (fade, table) in [(ZenFade::Near, &NEAR[..]), (ZenFade::WholePage, &WHOLE[..])] {
        ui.set_prefs(|p| p.zen_fade = fade);
        for focus in [ZenFocus::Sentence, ZenFocus::Paragraph, ZenFocus::Line] {
            ui.set_prefs(|p| p.zen_focus = focus);
            let rows = page_rows(&ui);
            let caret_row = rows[caret];
            let bright = |c: usize| match focus {
                ZenFocus::Sentence => sentence.contains(&c),
                ZenFocus::Paragraph => paragraph.contains(&c),
                ZenFocus::Line => rows[c] == caret_row,
            };
            check_fade(&ui, table, &bright, &format!("{focus:?} + {fade:?}"));
        }
    }

    // Both ways: below the caret's paragraph as above it.
    ui.set_prefs(|p| p.zen_focus = ZenFocus::Paragraph);
    ui.caret_at(ui.find("Fourth").start);
    let fourth = ui.find("Fourth paragraph here.");
    check_fade(&ui, &WHOLE, &|c| fourth.contains(&c), "Paragraph + WholePage, lower down");
    // Four rows to a step: no paragraph falls faster than another.
    assert_eq!(alphas(&ui, &["Third", "Fifth", "Second", "Sixth"]), [1, 1, 2, 2].map(|d| row_step(&WHOLE, d)));
}

#[test]
fn an_empty_paragraph_is_one_row_from_each_neighbour() {
    // Of three blank lines the middle one is an empty paragraph the caret can sit in.
    let mut ui = Ui::open("One.\n\nTwo.\n\n\n\nThree.\n\nFour.\n");
    ui.set_prefs(|p| {
        p.effects = Effects::Off;
        p.zen_writing = true;
        p.zen_fade = ZenFade::WholePage;
    });
    let c = ui.find("Two.\n\n").end;
    ui.caret_at(c);
    assert_eq!(ui.selection(), c..c);
    let want = [1, 1, 2, 2].map(|d| row_step(&WHOLE, d));
    assert_eq!(alphas(&ui, &["Two", "Three", "One", "Four"]), want);
    ui.set_prefs(|p| p.zen_focus = ZenFocus::Line);
    assert_eq!(alphas(&ui, &["Two", "Three", "One", "Four"]), want);
}

#[test]
fn line_keeps_the_caret_row_bright_and_steps_by_row() {
    let mut ui = zen(ZenFocus::Line, ZenFade::Off);
    let rows = rows(&ui);
    assert!(rows.len() >= 4, "the paragraph wraps: {rows:?}");
    ui.caret_at(rows[2].start + 3);
    for (i, r) in rows.iter().enumerate() {
        let want = if i == 2 { 1. } else { ZEN_DIM };
        assert_eq!((alpha_at(&ui, r.start), alpha_at(&ui, r.end - 1)), (want, want), "row {i}");
    }
    assert_eq!(alphas(&ui, &["Keep", "Second"]), [ZEN_DIM; 2]);

    // Steps count rows, through the paragraph and on into its neighbours.
    ui.set_prefs(|p| p.zen_fade = ZenFade::WholePage);
    let last = rows.len() - 1;
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(alpha_at(&ui, r.start), row_step(&WHOLE, i.abs_diff(2)), "row {i}");
    }
    assert_eq!(alpha(&ui, "Keep"), row_step(&WHOLE, 3));
    assert_eq!(alpha(&ui, "Second"), row_step(&WHOLE, last - 2 + 1));
    // A step is four rows: no faster than a paragraph's.
    assert_eq!(row_step(&WHOLE, 5), step(&WHOLE, 2));
    assert!(row_step(&WHOLE, 2) < step(&WHOLE, 1) && row_step(&WHOLE, 2) > step(&WHOLE, 2));

    // The light moves with the caret's row.
    ui.caret_at(rows[1].start + 1);
    assert_eq!((alpha_at(&ui, rows[1].start), alpha_at(&ui, rows[2].start)), (1., row_step(&WHOLE, 1)));

    // Cutting runs at rows and sentences moves no text (shaping in pieces rounds differently,
    // by thousandths of a pixel).
    let at = |ui: &Ui| -> Vec<(f32, f32)> {
        (0..ui.text().chars().count())
            .map(|c| ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").point_for(c).expect("laid out")))
            .map(|p| (f32::from(p.x), f32::from(p.y)))
            .collect()
    };
    let same = |a: &[(f32, f32)], b: &[(f32, f32)]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 0.01 && p.1 == q.1);
    let lit = at(&ui);
    ui.set_prefs(|p| p.zen_focus = ZenFocus::Sentence);
    assert!(same(&at(&ui), &lit));
    ui.set_prefs(|p| p.zen_writing = false);
    assert!(same(&at(&ui), &lit));
    ui.set_prefs(|p| {
        p.zen_writing = true;
        p.zen_focus = ZenFocus::Line;
    });

    // A selection lights every row of the blocks it touches.
    ui.select(rows[1].start + 1..rows[2].start + 1);
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(alpha_at(&ui, r.start), 1., "row {i}");
    }
    assert_eq!(alphas(&ui, &["Keep", "Second", "Third"]), [row_step(&WHOLE, 1), row_step(&WHOLE, 1), row_step(&WHOLE, 2)]);
}

#[test]
fn typing_a_sentence_doesnt_fade_it_in() {
    // Placed with effects off, so nothing is fading when they go on.
    let mut ui = zen(ZenFocus::Sentence, ZenFade::Near);
    ui.caret_at(ui.find("pick a word").end);
    ui.set_prefs(|p| p.effects = Effects::On);
    assert_ne!(ui.read(|_, cx| Prefs::get(cx).motion()), Effects::Off);
    assert!(!ui.read(|ws, cx| ws.page.read(cx).zen_fading()));
    ui.type_text("s");
    assert!(!ui.read(|ws, cx| ws.page.read(cx).zen_fading()), "the sentence grew, it didn't move");
    assert_eq!(alpha(&ui, "words,"), 1.);
    ui.caret_at(ui.find("Editors").start);
    assert!(ui.read(|ws, cx| ws.page.read(cx).zen_fading()), "the light moved");
}

#[test]
fn defaults_are_paragraph_and_no_fade() {
    let p = Prefs::default();
    assert_eq!((p.zen_focus, p.zen_fade), (ZenFocus::Paragraph, ZenFade::Off));
    let old: Prefs = serde_json::from_str(r#"{"zen_writing": true}"#).unwrap();
    assert_eq!((old.zen_focus, old.zen_fade), (ZenFocus::Paragraph, ZenFade::Off));
    let set: Prefs = serde_json::from_str(r#"{"zen_focus": "line", "zen_fade": "whole-page"}"#).unwrap();
    assert_eq!((set.zen_focus, set.zen_fade), (ZenFocus::Line, ZenFade::WholePage));
}

#[test]
fn preferences_writing_has_the_two_dials() {
    let mut ui = Ui::open(DOC);
    let prefs = |ui: &Ui| ui.read(|_, cx| Prefs::get(cx).clone());
    ui.keys("cmd-,");
    ui.click_id("nav-writing");
    assert_eq!(ui.element(("zen-focus", 2usize)).selected(), Some(true), "Paragraph by default");
    assert_eq!(ui.element(("zen-fade", 0usize)).selected(), Some(true), "Off by default");
    // Zen writing is off in tests: the dials stay settable, saying what it does when it goes on.
    assert!(!prefs(&ui).zen_writing);
    ui.click_id(("zen-focus", 1usize));
    ui.click_id(("zen-fade", 2usize));
    let p = prefs(&ui);
    assert_eq!((p.zen_focus, p.zen_fade), (ZenFocus::Line, ZenFade::WholePage));
    ui.click_id(("zen-focus", 0usize));
    ui.click_id(("zen-fade", 1usize));
    let p = prefs(&ui);
    assert_eq!((p.zen_focus, p.zen_fade), (ZenFocus::Sentence, ZenFade::Near));
}

/// A justified, hyphenated mono column where a row fits exactly: shaped in pieces (one per
/// row's ink) it must wrap as it does whole, or the light lands mid-row.
#[test]
fn line_focus_wraps_as_the_page_does() {
    let text = "Но в любом случае, в мыслях Клод ожидал, что за пару-тройку часов дойдет, отдаст треклятый пакет, заберет деньги и как минимум треть из них прокутит в “Счастливой Эльзе”. Они с друзьями были завсегдатаями в этом богом освященном месте, Толстяк Лу конечно знал, что продавать им выпивку нельзя, но великодушно закрывал глаза, пока однажды их перебравшая банда не поссорилась со случайно зашедшими пропустить по стаканчику королевскими вестовыми и устроила грандиозный погром. Потом еще несколько лет в таверну приезжали королевские надсмотрщики и проверяли каждый счет в попытке найти утаенные от подати деньги.\n";
    let mut ui = Ui::open(text);
    ui.set_prefs(|p| {
        p.effects = Effects::Off;
        p.zen_writing = true;
        p.zen_fade = ZenFade::WholePage;
        p.alignment = crate::prefs::Alignment::Justified;
        p.hyphenation = true;
        p.font_family = "JetBrains Mono".into();
        p.font_size = 17.;
        p.column_chars = 74;
        p.line_spacing = 1.8;
    });
    ui.caret_at(ui.find("деньги.").end);
    ui.type_text(" ");
    let ends = |ui: &Ui| ui.read(|ws, cx| ws.page.read(cx).layout.as_ref().expect("laid out").row_ends(0));
    let whole = ends(&ui);
    let rows_whole = rows_of(&ui, 0..text.chars().count());
    ui.set_prefs(|p| p.zen_focus = ZenFocus::Line);
    assert_eq!(ends(&ui), whole);
    let rows = rows_of(&ui, 0..text.chars().count());
    assert_eq!(rows, rows_whole);
    // Every row is lit evenly: the caret's at full ink, the others a step per row.
    let last = rows.len() - 1;
    for (i, r) in rows.iter().enumerate() {
        let want = row_step(&WHOLE, last - i);
        assert_eq!((alpha_at(&ui, r.start), alpha_at(&ui, r.end - 1)), (want, want), "row {i}: {r:?}");
    }
}
