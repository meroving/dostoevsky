use std::collections::{HashMap, HashSet};
use std::ops::Range;

use english::is_word_char;

use crate::token::{Class, Token};
use crate::Lang;

/// In words.
pub const ECHO_WINDOW: usize = 50;

/// A word's echo: `again` repeats `first`, `between` words later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Echo {
    pub first: Range<usize>,
    pub again: Range<usize>,
    pub first_word: String,
    pub word: String,
    pub between: usize,
}

const AUXILIARIES: &[&str] = &[
    "be", "have", "do", "will", "would", "shall", "should", "can", "could", "may", "might", "must", "быть", "стать", "мочь", "sein", "haben", "werden", "können", "müssen", "sollen", "wollen", "être", "avoir",
    "pouvoir", "devoir", "ser", "estar", "haber", "poder", "essere", "avere", "potere", "dovere", "ter", "ir",
];

/// `skip`: dictionary forms that never echo (the title's). In languages the tagger can't read,
/// the commonest words and anything under four letters never echo.
pub fn echoes(tokens: &[Token], lang: Lang, skip: &HashSet<String>) -> Vec<Echo> {
    let mut last: HashMap<String, (usize, &Token)> = HashMap::new();
    let mut out = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let base = t.base();
        let letters = t.word.chars().filter(|c| c.is_alphabetic()).count();
        let name = t.word.chars().next().is_some_and(char::is_uppercase);
        let quiet = t.class.is_function_word()
            || matches!(t.class, Class::Interjection | Class::Number)
            || letters < 4
            || name
            || AUXILIARIES.contains(&base.as_str())
            || skip.contains(&base)
            || lang.is_common(&base);
        if quiet {
            continue;
        }
        if let Some((j, first)) = last.get(&base) {
            let between = i - j - 1;
            if between < ECHO_WINDOW {
                out.push(Echo { first: first.range.clone(), again: t.range.clone(), first_word: first.word.clone(), word: t.word.clone(), between });
            }
        }
        last.insert(base, (i, t));
    }
    out
}

/// For languages the tagger can't read: no class or lemma, so only the same spelling echoes.
pub fn plain_tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut start = None;
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().copied().chain(std::iter::once(' ')).enumerate() {
        match (is_word_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                let word: String = chars[s..i].iter().collect();
                if word.chars().any(char::is_alphanumeric) {
                    out.push(Token { range: s..i, word, class: Class::Other, lemma: None });
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::t;

    fn tagged(words: &[(&str, Class, &str)]) -> Vec<Token> {
        let mut at = 0;
        words
            .iter()
            .map(|(w, c, l)| {
                let tok = t(at, w, *c, Some(l));
                at += w.chars().count() + 1;
                tok
            })
            .collect()
    }

    #[test]
    fn word_and_words() {
        let text = tagged(&[
            ("You", Class::Pronoun, "you"),
            ("pick", Class::Verb, "pick"),
            ("a", Class::Determiner, "a"),
            ("word", Class::Noun, "word"),
            ("and", Class::Conjunction, "and"),
            ("the", Class::Determiner, "the"),
            ("other", Class::Adjective, "other"),
            ("words", Class::Noun, "word"),
            ("disappear", Class::Verb, "disappear"),
            ("and", Class::Conjunction, "and"),
            ("the", Class::Determiner, "the"),
        ]);
        let e = echoes(&text, Lang::En, &HashSet::new());
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!((e[0].first_word.as_str(), e[0].word.as_str(), e[0].between), ("word", "words", 3));
    }

    #[test]
    fn what_never_echoes() {
        let text = tagged(&[
            ("Anna", Class::Noun, "anna"),
            ("was", Class::Verb, "be"),
            ("here", Class::Adverb, "here"),
            ("Anna", Class::Noun, "anna"),
            ("was", Class::Verb, "be"),
            ("glad", Class::Adjective, "glad"),
            ("cat", Class::Noun, "cat"),
            ("cat", Class::Noun, "cat"),
            ("Keep", Class::Verb, "keep"),
            ("keep", Class::Verb, "keep"),
        ]);
        let skip: HashSet<String> = ["keep".to_string()].into();
        assert!(echoes(&text, Lang::En, &skip).is_empty(), "names, auxiliaries, short words, the title's words");
    }

    #[test]
    fn far_apart_is_fine() {
        let mut text = tagged(&[("choosing", Class::Verb, "choose")]);
        for i in 0..ECHO_WINDOW {
            text.push(t(100 + i * 5, "and", Class::Conjunction, Some("and")));
        }
        text.push(t(1000, "chose", Class::Verb, Some("choose")));
        assert!(echoes(&text, Lang::En, &HashSet::new()).is_empty(), "{} words apart", ECHO_WINDOW);
    }

    #[test]
    fn untagged_languages_echo_by_spelling() {
        let text = "La scrittura viene dalla scelta, e la scelta viene presto.";
        let e = echoes(&plain_tokens(text), Lang::It, &HashSet::new());
        let words: Vec<&str> = e.iter().map(|e| e.word.as_str()).collect();
        assert_eq!(words, ["scelta", "viene"]);
    }
}
