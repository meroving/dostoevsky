//! Spelling on Windows (dsky-fbso.17): the system's own checker, ISpellCheckerFactory, which
//! Word, Edge and the text boxes of Windows share. One checker per language the writer's Windows
//! spells in, the ones they read first (the UI's, the system's, English); a word is right when
//! any of them knows it, as on the Mac. Learn Spelling adds the word to Windows' user dictionary
//! (it lasts, and the other apps know it too); Ignore lasts for the session.
//!
//! COM objects belong to the thread that made them: the speller is made and used on the main
//! thread, which GPUI has already set up for COM.

use std::collections::HashSet;
use std::ops::Range;

use windows::Win32::Globalization::{CORRECTIVE_ACTION_GET_SUGGESTIONS, CORRECTIVE_ACTION_REPLACE, ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IEnumString};
use windows::core::{HSTRING, PWSTR};

use crate::spell::Speller;

pub struct WinSpeller {
    /// `en-US`, its checker; the writer's languages first.
    checkers: Vec<(String, ISpellChecker)>,
}

impl WinSpeller {
    /// `None` when Windows spells in none of the languages, or has no spell checker.
    pub fn new(languages: &[String]) -> Option<WinSpeller> {
        // SAFETY: COM calls on this thread, after initialising it (a second call only counts up;
        // a thread already in the multithreaded apartment keeps it, which suits the checker too).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let factory: ISpellCheckerFactory = CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let supported = strings(&factory.SupportedLanguages().ok()?);
            let mut checkers = Vec::new();
            for tag in choose(&supported, languages) {
                match factory.CreateSpellChecker(&HSTRING::from(tag.as_str())) {
                    Ok(c) => checkers.push((tag, c)),
                    Err(e) => eprintln!("dostoevsky: no {tag} spell checker: {e}"),
                }
            }
            (!checkers.is_empty()).then_some(WinSpeller { checkers })
        }
    }

    /// The checkers' languages, in order.
    #[cfg(test)]
    pub fn languages(&self) -> Vec<&str> {
        self.checkers.iter().map(|(t, _)| t.as_str()).collect()
    }

    /// Each word `checker` finds wrong, in UTF-16 units: not repeated words ("the the"), which
    /// the Mac's spelling doesn't mark either.
    fn errors(checker: &ISpellChecker, text: &HSTRING) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        // SAFETY: the checker is this thread's; each error comes from its enumerator.
        unsafe {
            let Ok(errors) = checker.Check(text) else { return out };
            loop {
                let mut error = None;
                if errors.Next(&mut error).is_err() {
                    break;
                }
                let Some(error) = error else { break };
                let action = error.CorrectiveAction().unwrap_or_default();
                if action != CORRECTIVE_ACTION_GET_SUGGESTIONS && action != CORRECTIVE_ACTION_REPLACE {
                    continue;
                }
                if let (Ok(start), Ok(len)) = (error.StartIndex(), error.Length()) {
                    out.push(start as usize..(start + len) as usize);
                }
            }
        }
        out
    }
}

impl Speller for WinSpeller {
    fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        if !text.chars().any(char::is_alphabetic) {
            return Vec::new();
        }
        let wide = HSTRING::from(text);
        let mut found: Option<Vec<Range<usize>>> = None;
        for (_, checker) in &self.checkers {
            let errors = WinSpeller::errors(checker, &wide);
            // Wrong only where every language finds it wrong.
            found = Some(match found {
                None => errors,
                Some(f) => f.into_iter().filter(|r| errors.iter().any(|e| e.start < r.end && r.start < e.end)).collect(),
            });
            if found.as_ref().is_some_and(Vec::is_empty) {
                break;
            }
        }
        let map = crate::spell::utf16_to_char(text);
        let last = map.len() - 1;
        let ranges = found.unwrap_or_default().into_iter().filter(|r| r.start < r.end && r.start < last).map(|r| map[r.start]..map[r.end.min(last)]);
        // As Hunspell does: not an acronym, a word with digits, nor an address.
        let words: Vec<Range<usize>> = crate::hunspell::words(text).into_iter().map(|(r, _)| r).collect();
        ranges.filter(|r| words.iter().any(|w| w.start < r.end && r.start < w.end)).collect()
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        // Each language's guesses; the ones written in the word's letters win (a Russian word's
        // guesses come from the Russian checker), else the first language's.
        let letters: HashSet<char> = word.to_lowercase().chars().filter(|c| c.is_alphabetic()).collect();
        let wide = HSTRING::from(word);
        let mut best: (usize, Vec<String>) = (0, Vec::new());
        for (_, checker) in &self.checkers {
            // SAFETY: the checker is this thread's.
            let guesses = unsafe { checker.Suggest(&wide) }.map(|e| strings(&e)).unwrap_or_default();
            let Some(first) = guesses.first() else { continue };
            let shared = first.to_lowercase().chars().filter(|c| letters.contains(c)).count();
            if best.1.is_empty() || shared > best.0 {
                best = (shared, guesses);
            }
        }
        best.1
    }

    fn learn(&self, word: &str) {
        // Windows keeps added words in one dictionary for every language (Spelling\neutral);
        // each checker is asked, in case another provider keeps one per language.
        let wide = HSTRING::from(word);
        for (tag, checker) in &self.checkers {
            // SAFETY: the checker is this thread's.
            if let Err(e) = unsafe { checker.Add(&wide) } {
                eprintln!("dostoevsky: couldn't add the word to the {tag} dictionary: {e}");
            }
        }
    }

    fn ignore(&self, word: &str) {
        let wide = HSTRING::from(word);
        for (_, checker) in &self.checkers {
            // SAFETY: the checker is this thread's.
            let _ = unsafe { checker.Ignore(&wide) };
        }
    }
}

/// Every string an enumerator gives, each freed once read.
fn strings(e: &IEnumString) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        let mut one = [PWSTR::null()];
        let mut fetched = 0;
        // SAFETY: room for one string; COM allocated it and we free it.
        unsafe {
            if e.Next(&mut one, Some(&mut fetched)).is_err() || fetched == 0 || one[0].is_null() {
                break;
            }
            out.push(one[0].to_string().unwrap_or_default());
            CoTaskMemFree(Some(one[0].0 as *const _));
        }
    }
    out
}

/// One checker for each language the writer reads, as Hunspell's dictionaries are picked (the
/// country asked for, else the language's own, else its first), then one for each other
/// language Windows spells in: the Mac's checker reads every language the writer has.
fn choose(supported: &[String], languages: &[String]) -> Vec<String> {
    let names: Vec<(String, std::path::PathBuf)> = supported.iter().map(|t| (t.replace('-', "_"), std::path::PathBuf::new())).collect();
    let mut out: Vec<String> = crate::hunspell::pick(&names, languages).into_iter().map(|(n, _)| n.replace('_', "-")).collect();
    let primary = |t: &str| t.split(['-', '_']).next().unwrap_or("").to_lowercase();
    for tag in supported {
        if !out.iter().any(|o| primary(o) == primary(tag)) {
            out.push(tag.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{WinSpeller, choose};
    use crate::spell::Speller;

    fn wrong(speller: &WinSpeller, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        speller.misspelled(text).into_iter().map(|r| chars[r].iter().collect()).collect()
    }

    #[test]
    fn the_writers_languages_first_then_one_of_each_other() {
        let supported: Vec<String> = ["en-AU", "en-GB", "en-US", "fr-FR", "ru-RU", "pt-BR"].iter().map(|s| s.to_string()).collect();
        let langs = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(choose(&supported, &langs(&["ru", "en"])), ["ru-RU", "en-US", "fr-FR", "pt-BR"]);
        assert_eq!(choose(&supported, &langs(&["en_GB", "en"])), ["en-GB", "fr-FR", "ru-RU", "pt-BR"]);
        assert_eq!(choose(&supported, &langs(&["de", "en"])), ["en-US", "fr-FR", "ru-RU", "pt-BR"], "none for German");
    }

    /// English is in every Windows (windows-latest too); Russian where the writer added it.
    #[test]
    fn windows_finds_misspellings_and_guesses() {
        let speller = WinSpeller::new(&["en-US".into()]).expect("Windows spells English");
        assert_eq!(speller.languages()[0], "en-US");
        assert_eq!(wrong(&speller, "Most of the frction comes too erly."), ["frction", "erly"]);
        assert_eq!(wrong(&speller, "NASA sent R2D2 to see https://exmaple.com/pth, don’t."), Vec::<String>::new(), "acronyms, digits, addresses");
        let guesses = speller.guesses("frction");
        assert!(guesses.contains(&"friction".to_string()), "{guesses:?}");
    }

    #[test]
    fn russian_when_windows_has_it() {
        let Some(speller) = WinSpeller::new(&["ru-RU".into(), "en-US".into()]) else { return };
        if !speller.languages().iter().any(|l| l.starts_with("ru")) {
            eprintln!("no Russian spell checker here: skipped");
            return;
        }
        assert_eq!(wrong(&speller, "Большая часть трения в писмене происходит."), ["писмене"]);
        assert_eq!(wrong(&speller, "Он сказал: frction и писмене."), ["frction", "писмене"]);
        let russian = speller.guesses("писмене");
        assert!(!russian.is_empty() && russian[0].chars().all(|c| c == ' ' || ('а'..='я').contains(&c.to_lowercase().next().unwrap())), "from the Russian checker: {russian:?}");
    }
}
