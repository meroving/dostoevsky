use std::ops::Range;
use std::time::Duration;

use gpui_kit::{App, Global};

/// How long launch leaves the first frame before spelling starts.
const AFTER_LAUNCH: Duration = Duration::from_millis(100);

pub trait Speller {
    /// Char ranges.
    fn misspelled(&self, text: &str) -> Vec<Range<usize>>;
    /// Best first.
    fn guesses(&self, word: &str) -> Vec<String>;
    fn learn(&self, word: &str);
    fn ignore(&self, word: &str);
}

pub struct Spelling {
    speller: Box<dyn Speller>,
    /// Bumped when Learn or Ignore change what is misspelled, so cached lines are rechecked.
    generation: u64,
}

impl Global for Spelling {}

impl Spelling {
    pub fn set(cx: &mut App, speller: Box<dyn Speller>) {
        let generation = Spelling::generation(cx) + 1;
        cx.set_global(Spelling { speller, generation });
        cx.refresh_windows();
    }

    pub fn init(cx: &mut App) {
        #[cfg(target_os = "macos")]
        Spelling::set(cx, Box::new(mac::MacSpeller::new()));
        // Off the Mac, Hunspell's dictionaries are read off the main thread (a big one takes a
        // moment); with none installed nothing is underlined. On Windows, which ships no Hunspell
        // dictionaries, its own checker spells, made on the main thread (COM's objects stay on
        // their thread); a writer's own .aff and .dic in Dictionaries/ join it.
        #[cfg(not(target_os = "macos"))]
        {
            let (dirs, languages) = (crate::hunspell::dictionary_dirs(), crate::hunspell::wanted_languages());
            let learned = crate::files::support_dir().map(|d| d.join("Spelling").join("learned.txt"));
            #[cfg(windows)]
            let wanted = languages.clone();
            cx.spawn(async move |cx| {
                let speller = cx.background_executor().spawn(async move { crate::hunspell::Hunspell::load(&dirs, &languages, learned) }).await;
                #[cfg(windows)]
                cx.update(|cx| {
                    let windows = crate::win_spell::WinSpeller::new(&wanted);
                    let both: Option<Box<dyn Speller>> = match (windows, speller) {
                        (Some(w), Some(h)) => Some(Box::new(Together(Box::new(w), Box::new(h)))),
                        (Some(w), None) => Some(Box::new(w)),
                        (None, Some(h)) => Some(Box::new(h)),
                        (None, None) => None,
                    };
                    if let Some(s) = both {
                        Spelling::set(cx, s);
                    }
                });
                #[cfg(not(windows))]
                if let Some(speller) = speller {
                    cx.update(|cx| Spelling::set(cx, Box::new(speller)));
                }
            })
            .detach();
        }
    }

    /// Deferred past the first frame: loading frameworks and checking every line would delay it.
    pub fn init_after_launch(cx: &mut App) {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(AFTER_LAUNCH).await;
            cx.update(Spelling::init);
        })
        .detach();
    }

    pub fn misspelled(cx: &App, text: &str) -> Vec<Range<usize>> {
        cx.try_global::<Spelling>().map(|s| s.speller.misspelled(text)).unwrap_or_default()
    }

    pub fn guesses(cx: &App, word: &str) -> Vec<String> {
        cx.try_global::<Spelling>().map(|s| s.speller.guesses(word)).unwrap_or_default()
    }

    pub fn generation(cx: &App) -> u64 {
        cx.try_global::<Spelling>().map_or(0, |s| s.generation)
    }

    pub fn learn(cx: &mut App, word: &str) {
        if let Some(s) = cx.try_global::<Spelling>() {
            s.speller.learn(word);
            cx.global_mut::<Spelling>().generation += 1;
            cx.refresh_windows();
        }
    }

    pub fn ignore(cx: &mut App, word: &str) {
        if let Some(s) = cx.try_global::<Spelling>() {
            s.speller.ignore(word);
            cx.global_mut::<Spelling>().generation += 1;
            cx.refresh_windows();
        }
    }
}

/// Two spellers as one: a word is wrong only when both find it wrong; guesses are the first's,
/// else the second's; Learn and Ignore go to both. Windows' checker and a writer's own Hunspell
/// dictionaries.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Together(pub Box<dyn Speller>, pub Box<dyn Speller>);

impl Speller for Together {
    fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        let first = self.0.misspelled(text);
        if first.is_empty() {
            return first;
        }
        let second = self.1.misspelled(text);
        first.into_iter().filter(|r| second.iter().any(|s| s.start < r.end && r.start < s.end)).collect()
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        let first = self.0.guesses(word);
        if first.is_empty() { self.1.guesses(word) } else { first }
    }

    fn learn(&self, word: &str) {
        self.0.learn(word);
        self.1.learn(word);
    }

    fn ignore(&self, word: &str) {
        self.0.ignore(word);
        self.1.ignore(word);
    }
}

/// Char offset of each UTF-16 offset in `text` (macOS and Windows report ranges in UTF-16).
#[cfg(any(target_os = "macos", windows, test))]
pub(crate) fn utf16_to_char(text: &str) -> Vec<usize> {
    let mut map = Vec::with_capacity(text.len() + 1);
    for (i, ch) in text.chars().enumerate() {
        for _ in 0..ch.len_utf16() {
            map.push(i);
        }
    }
    map.push(text.chars().count());
    map
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ops::Range;

    use objc2::rc::Retained;
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSInteger, NSRange, NSString, NSTextCheckingType};

    pub struct MacSpeller {
        checker: Retained<NSSpellChecker>,
        /// One spell document for the app: Ignore lasts for the session.
        tag: NSInteger,
    }

    impl MacSpeller {
        pub fn new() -> MacSpeller {
            MacSpeller { checker: NSSpellChecker::sharedSpellChecker(), tag: NSSpellChecker::uniqueSpellDocumentTag() }
        }
    }

    impl super::Speller for MacSpeller {
        fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
            if !text.chars().any(char::is_alphabetic) {
                return Vec::new();
            }
            let s = NSString::from_str(text);
            let map = super::utf16_to_char(text);
            let len = map.len() - 1;
            // Not `checkSpellingOfString`: with no language it checks only in the checker's one.
            // SAFETY: no options, no orthography and a null word count are all allowed.
            let results = unsafe {
                self.checker.checkString_range_types_options_inSpellDocumentWithTag_orthography_wordCount(
                    &s,
                    NSRange::new(0, len),
                    NSTextCheckingType::Spelling.0,
                    None,
                    self.tag,
                    None,
                    std::ptr::null_mut(),
                )
            };
            let mut out: Vec<Range<usize>> = results
                .iter()
                .map(|r| r.range())
                .filter(|r| r.length > 0 && r.location < len)
                .map(|r| map[r.location]..map[(r.location + r.length).min(len)])
                .collect();
            out.sort_by_key(|r| r.start);
            out
        }

        fn guesses(&self, word: &str) -> Vec<String> {
            let s = NSString::from_str(word);
            let range = NSRange::new(0, word.encode_utf16().count());
            let guesses = |language: Option<&NSString>| -> Vec<String> {
                self.checker
                    .guessesForWordRange_inString_language_inSpellDocumentWithTag(range, &s, language, self.tag)
                    .map(|a| a.iter().map(|g| g.to_string()).collect())
                    .unwrap_or_default()
            };
            // Guesses come from one language at a time: try the user's spelling languages in order.
            let first = guesses(None);
            if !first.is_empty() {
                return first;
            }
            for language in self.checker.userPreferredLanguages().iter().take(8) {
                let g = guesses(Some(&language));
                if !g.is_empty() {
                    return g;
                }
            }
            Vec::new()
        }

        fn learn(&self, word: &str) {
            self.checker.learnWord(&NSString::from_str(word));
        }

        fn ignore(&self, word: &str) {
            self.checker.ignoreWord_inSpellDocumentWithTag(&NSString::from_str(word), self.tag);
        }
    }
}

/// Test dictionary: words not in it are misspelled; guesses are its words at edit distance 1.
#[cfg(test)]
pub struct WordList {
    words: std::cell::RefCell<std::collections::HashSet<String>>,
}

#[cfg(test)]
impl WordList {
    pub fn new(words: &str) -> WordList {
        WordList { words: std::cell::RefCell::new(words.split_whitespace().map(str::to_lowercase).collect()) }
    }
}

#[cfg(test)]
impl Speller for WordList {
    fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        let chars: Vec<char> = text.chars().collect();
        let is_word = |c: char| c.is_alphabetic() || c == '\'' || c == '’';
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if !chars[i].is_alphabetic() {
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && is_word(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect::<String>().to_lowercase();
            if !self.words.borrow().contains(&word) {
                out.push(start..i);
            }
        }
        out
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        let w: Vec<char> = word.to_lowercase().chars().collect();
        let near = |c: &Vec<char>| {
            let (a, b) = if c.len() >= w.len() { (c, &w) } else { (&w, c) };
            if a.len() - b.len() > 1 {
                return false;
            }
            let (mut i, mut j, mut edits) = (0, 0, 0);
            while i < a.len() && j < b.len() {
                if a[i] == b[j] {
                    i += 1;
                    j += 1;
                } else {
                    edits += 1;
                    i += 1;
                    if a.len() == b.len() {
                        j += 1;
                    }
                }
            }
            edits + (a.len() - i) <= 1
        };
        let mut out: Vec<String> = self.words.borrow().iter().filter(|c| near(&c.chars().collect())).cloned().collect();
        out.sort();
        out
    }

    fn learn(&self, word: &str) {
        self.words.borrow_mut().insert(word.to_lowercase());
    }

    fn ignore(&self, word: &str) {
        self.learn(word);
    }
}

#[cfg(test)]
mod tests {
    use super::utf16_to_char;

    #[test]
    fn utf16_offsets_map_to_chars() {
        // "é" is one UTF-16 unit; "😀" is two.
        assert_eq!(utf16_to_char("é😀a"), vec![0, 1, 1, 2, 3]);
    }

    #[test]
    fn together_a_word_is_wrong_only_when_both_say_so() {
        use super::{Speller, Together, WordList};
        let both = Together(Box::new(WordList::new("most of the friction")), Box::new(WordList::new("the trenie friction")));
        let text = "Most of the frction trenie";
        let chars: Vec<char> = text.chars().collect();
        let wrong = |s: &Together| s.misspelled(text).into_iter().map(|r| chars[r].iter().collect::<String>()).collect::<Vec<_>>();
        assert_eq!(wrong(&both), ["frction"], "the first knows Most, the second trenie");
        assert_eq!(both.guesses("frction"), ["friction"]);
        let second_only = Together(Box::new(WordList::new("")), Box::new(WordList::new("trenie")));
        assert_eq!(second_only.guesses("trenei"), Vec::<String>::new());
        assert_eq!(second_only.guesses("treni"), ["trenie"], "the second's when the first has none");
        both.learn("frction");
        assert!(wrong(&both).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_checker_finds_russian_and_english() {
        use super::Speller;
        let speller = super::mac::MacSpeller::new();
        let words = |text: &str| -> Vec<String> {
            let chars: Vec<char> = text.chars().collect();
            speller.misspelled(text).into_iter().map(|r| chars[r].iter().collect()).collect()
        };
        assert_eq!(words("Большая часть трения в писмене происходит."), ["писмене"]);
        assert_eq!(words("Most of the frction comes too erly."), ["frction", "erly"]);
        assert_eq!(words("Он сказал: frction и писмене."), ["frction", "писмене"]);
    }
}
