use std::ops::{Add, AddAssign};

use english::is_word_char;

use crate::Lang;

/// Below this many words the number says more about chance than about the text.
pub const MIN_WORDS: usize = 100;

/// Summable: the stats of texts together (a chapter, a book) are the sums of theirs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub words: usize,
    pub sentences: usize,
    pub syllables: usize,
}

impl Add for Stats {
    type Output = Stats;
    fn add(self, o: Stats) -> Stats {
        Stats { words: self.words + o.words, sentences: self.sentences + o.sentences, syllables: self.syllables + o.syllables }
    }
}

impl AddAssign for Stats {
    fn add_assign(&mut self, o: Stats) {
        *self = *self + o;
    }
}

pub fn stats(text: &str, lang: Lang) -> Stats {
    let mut s = Stats { sentences: sentences(text), ..Stats::default() };
    for word in text.split(|c: char| !is_word_char(c)) {
        if !word.chars().any(char::is_alphanumeric) {
            continue;
        }
        s.words += 1;
        // A number is read as a word or two; one syllable keeps it from skewing anything.
        s.syllables += if word.chars().any(char::is_alphabetic) { syllables(word, lang) } else { 1 };
    }
    s
}

/// Each run of `.`, `!`, `?` or `…` ends a sentence, and so does a line that stops without one.
fn sentences(text: &str) -> usize {
    let ends = |c: char| matches!(c, '.' | '!' | '?' | '…');
    let closers = |c: char| matches!(c, '"' | '\'' | '”' | '’' | '»' | ')' | ']' | '*' | '_');
    let mut n = 0;
    for line in text.lines() {
        let line = line.trim();
        if !line.chars().any(char::is_alphanumeric) {
            continue;
        }
        let mut in_end = false;
        for c in line.chars() {
            if ends(c) {
                if !in_end {
                    n += 1;
                }
                in_end = true;
            } else if !closers(c) {
                in_end = false;
            }
        }
        if !line.trim_end_matches(closers).ends_with(ends) {
            n += 1;
        }
    }
    n
}

/// Russian is exact (one per vowel letter); the others count vowel groups, with exceptions.
pub fn syllables(word: &str, lang: Lang) -> usize {
    let w: Vec<char> = word.to_lowercase().chars().filter(|c| c.is_alphabetic()).collect();
    if w.is_empty() {
        return 0;
    }
    match lang {
        Lang::Ru => w.iter().filter(|c| "аеёиоуыэюя".contains(**c)).count(),
        Lang::En => english(&w),
        Lang::De => groups(&w, "aeiouyäöü").max(1),
        Lang::Fr => french(&w),
        Lang::Es => romance(&w, "aeiouáéíóúü", "aeoáéíóú", ""),
        Lang::It => romance(&w, "aeiouàèéìíòóù", "aeoàèéìíòóù", ""),
        Lang::Pt => romance(&w, "aeiouáâãàéêíóôõúü", "aeoáâãàéêíóôõú", "ãõ"),
    }
}

fn groups(w: &[char], vowels: &str) -> usize {
    let mut n = 0;
    let mut prev = false;
    for c in w {
        let v = vowels.contains(*c);
        if v && !prev {
            n += 1;
        }
        prev = v;
    }
    n
}

/// English: vowel groups after dropping a silent final -e, -es or -ed; a long group counts two.
fn english(w: &[char]) -> usize {
    if w.len() <= 3 {
        return 1;
    }
    let vowel = |c: char| "aeiouy".contains(c);
    let mut w = w.to_vec();
    let n = w.len();
    let before = |i: usize| w.get(i).is_some_and(|c| *c != 'l' && !vowel(*c));
    if w.ends_with(&['e', 'd']) || (w.ends_with(&['e', 's']) && before(n - 3)) {
        w.truncate(n - 2);
    } else if w.ends_with(&['e']) && before(n - 2) {
        w.truncate(n - 1);
    }
    if w.first() == Some(&'y') {
        w.remove(0);
    }
    let mut count = 0;
    let mut run: usize = 0;
    for c in w.iter().chain(std::iter::once(&' ')) {
        if vowel(*c) {
            run += 1;
        } else if run > 0 {
            count += run.div_ceil(2);
            run = 0;
        }
    }
    count.max(1)
}

/// French: vowel groups, a final mute -e or -es not counted.
fn french(w: &[char]) -> usize {
    let n = groups(w, "aeiouyàâäéèêëîïôöûùüÿœæ");
    let mute = w.ends_with(&['e']) || w.ends_with(&['e', 's']);
    if mute && n > 1 { n - 1 } else { n.max(1) }
}

/// Two strong vowels side by side are two syllables, a weak one joins its neighbour, and a
/// `nasal` vowel joins the vowel after it (Portuguese -ão, -õe).
fn romance(w: &[char], vowels: &str, strong: &str, nasal: &str) -> usize {
    let mut n = groups(w, vowels);
    for pair in w.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if strong.contains(a) && strong.contains(b) && !nasal.contains(a) {
            n += 1;
        }
    }
    n.max(1)
}

/// 0 (very hard) to 100 (very easy), or `None` under [`MIN_WORDS`] words. Constants: Flesch (1948),
/// Oborneva (2006), Amstad (1978), Kandel & Moles (1958), Fernández-Huerta (1959, words per
/// sentence), Flesch–Vacca (1972), Martins et al. (1996).
pub fn ease(s: Stats, lang: Lang) -> Option<u8> {
    if s.words < MIN_WORDS || s.sentences == 0 {
        return None;
    }
    let asl = s.words as f64 / s.sentences as f64;
    let asw = s.syllables as f64 / s.words as f64;
    let score = match lang {
        Lang::En => 206.835 - 1.015 * asl - 84.6 * asw,
        Lang::Ru => 206.835 - 1.3 * asl - 60.1 * asw,
        Lang::De => 180.0 - asl - 58.5 * asw,
        Lang::Fr => 207.0 - 1.015 * asl - 73.6 * asw,
        Lang::Es => 206.84 - 0.60 * (100.0 * asw) - 1.02 * asl,
        Lang::It => 217.0 - 1.3 * asl - 0.6 * (100.0 * asw),
        Lang::Pt => 248.835 - 1.015 * asl - 84.6 * asw,
    };
    Some(score.round().clamp(0.0, 100.0) as u8)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    VeryEasy,
    Easy,
    FairlyEasy,
    Plain,
    FairlyHard,
    Hard,
    VeryHard,
}

pub fn band(score: u8) -> Band {
    match score {
        90.. => Band::VeryEasy,
        80..=89 => Band::Easy,
        70..=79 => Band::FairlyEasy,
        60..=69 => Band::Plain,
        50..=59 => Band::FairlyHard,
        30..=49 => Band::Hard,
        _ => Band::VeryHard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_syllables() {
        for (w, n) in [("the", 1), ("cat", 1), ("friction", 2), ("writing", 2), ("candidates", 3), ("quietly", 3), ("disappear", 3), ("make", 1), ("table", 2), ("remembered", 3), ("you're", 1), ("editor", 3), ("yellow", 2)] {
            assert_eq!(syllables(w, Lang::En), n, "{w}");
        }
    }

    #[test]
    fn other_syllables() {
        assert_eq!(syllables("человек", Lang::Ru), 3);
        assert_eq!(syllables("в", Lang::Ru), 0);
        assert_eq!(syllables("Непривлекательный", Lang::Ru), 6);
        assert_eq!(syllables("Schreiben", Lang::De), 2);
        assert_eq!(syllables("Häuser", Lang::De), 2);
        assert_eq!(syllables("écriture", Lang::Fr), 3);
        assert_eq!(syllables("difficultés", Lang::Fr), 4);
        assert_eq!(syllables("poeta", Lang::Es), 3);
        assert_eq!(syllables("ciudad", Lang::Es), 2);
        assert_eq!(syllables("país", Lang::Es), 2);
        assert_eq!(syllables("scrittura", Lang::It), 3);
        assert_eq!(syllables("paura", Lang::It), 2);
        assert_eq!(syllables("coração", Lang::Pt), 3);
        assert_eq!(syllables("leão", Lang::Pt), 2);
    }

    #[test]
    fn counting_sentences() {
        assert_eq!(sentences("One. Two! Three?"), 3);
        assert_eq!(sentences("Wait… what?! No."), 3);
        assert_eq!(sentences("# A heading\n\nA line of verse\nand another.\n\n- a list item"), 4);
        assert_eq!(sentences("He said “go.” Then left."), 2);
        assert_eq!(sentences("---\n\n12"), 1);
    }

    #[test]
    fn formulas() {
        // 100 words in 5 sentences with 150 syllables: ASL 20, ASW 1.5.
        let s = Stats { words: 100, sentences: 5, syllables: 150 };
        assert_eq!(ease(s, Lang::En), Some(60));
        assert_eq!(ease(s, Lang::Ru), Some(91));
        assert_eq!(ease(s, Lang::De), Some(72));
        assert_eq!(ease(s, Lang::Fr), Some(76));
        assert_eq!(ease(s, Lang::Es), Some(96));
        assert_eq!(ease(s, Lang::It), Some(100));
        assert_eq!(ease(s, Lang::Pt), Some(100));
        assert_eq!(ease(Stats { words: 100, sentences: 1, syllables: 250 }, Lang::En), Some(0));
        assert_eq!(ease(Stats { words: 99, sentences: 5, syllables: 150 }, Lang::En), None);
        assert_eq!(ease(Stats { words: 120, sentences: 0, syllables: 150 }, Lang::En), None);
    }

    #[test]
    fn bands() {
        assert_eq!(band(100), Band::VeryEasy);
        assert_eq!(band(85), Band::Easy);
        assert_eq!(band(70), Band::FairlyEasy);
        assert_eq!(band(64), Band::Plain);
        assert_eq!(band(58), Band::FairlyHard);
        assert_eq!(band(30), Band::Hard);
        assert_eq!(band(29), Band::VeryHard);
    }

    #[test]
    fn a_page() {
        let text = "Most of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear. A week later you can’t remember what they were, only that one of them was better.\n\nEditors have always known this. A good editor doesn’t erase your sentence. They write a suggestion in the margin and let you decide. The page holds both versions until you’re ready, and you judge them the only way that works: in context, with the real sentence around them.";
        let s = stats(text, Lang::En);
        assert_eq!((s.words, s.sentences), (89, 7));
        let s = s + s;
        let e = ease(s, Lang::En).unwrap();
        assert!((60..=85).contains(&e), "{e}");
    }
}
