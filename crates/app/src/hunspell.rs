//! Spelling off the Mac (dsky-fbso.4): Hunspell dictionaries, the ones Linux's desktops and
//! LibreOffice share, checked by `spellbook` (pure Rust). One dictionary per language the writer
//! reads: the UI's, the system's, and English; a word is right when any of them knows it, as
//! the Mac's checker accepts a word from any of the writer's languages. Learned words are kept in
//! `Spelling/learned.txt` in the app's folder; ignored ones for the session.

// On the Mac NSSpellChecker spells; this is built there only for its tests.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use unicode_segmentation::UnicodeSegmentation;

use crate::spell::Speller;

struct Dictionary {
    /// The file's name without `.dic`: `en_US`, `ru_RU` (for the tests: which were picked).
    #[cfg_attr(not(test), allow(dead_code))]
    name: String,
    dict: spellbook::Dictionary,
    /// The letters of its `TRY` line: which words its guesses suit.
    letters: HashSet<char>,
}

pub struct Hunspell {
    dicts: Vec<Dictionary>,
    learned: RefCell<HashSet<String>>,
    ignored: RefCell<HashSet<String>>,
    /// Learned words, one a line.
    file: Option<PathBuf>,
}

/// Where dictionaries are looked for, the app's own folder first: `Dictionaries/` in it takes a
/// writer's own `.aff` and `.dic` (Windows has no shared ones).
pub fn dictionary_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = crate::files::support_dir() {
        dirs.push(dir.join("Dictionaries"));
    }
    if let Some(path) = std::env::var_os("DICPATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if cfg!(not(windows)) {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("HOME").unwrap_or_default();
        dirs.push(var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")).join("hunspell"));
        let shared: Vec<PathBuf> = var("XDG_DATA_DIRS").map(|v| std::env::split_paths(&v).collect()).unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
        for dir in shared.iter().chain([PathBuf::from("/usr/share")].iter()) {
            dirs.push(dir.join("hunspell"));
            dirs.push(dir.join("myspell"));
            dirs.push(dir.join("myspell/dicts"));
        }
    }
    let mut seen = HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// The languages to spell in, as `ru_RU` or `en`: the UI's, the system's in order, English.
pub fn wanted_languages() -> Vec<String> {
    let mut tags = vec![l10n::current().code().to_string()];
    tags.extend(crate::platform::preferred_languages());
    tags.push("en".into());
    let mut seen = HashSet::new();
    tags.into_iter().map(|t| t.replace('-', "_")).filter(|t| !t.is_empty() && seen.insert(t.to_lowercase())).collect()
}

impl Hunspell {
    /// `None` when no dictionary for any of `languages` is found: then nothing is underlined.
    pub fn load(dirs: &[PathBuf], languages: &[String], learned: Option<PathBuf>) -> Option<Hunspell> {
        let found = available(dirs);
        let mut dicts = Vec::new();
        for (name, dic) in pick(&found, languages) {
            match open(&dic) {
                Ok(dict) => dicts.push(Dictionary { name, dict: dict.0, letters: dict.1 }),
                Err(e) => eprintln!("dostoevsky: couldn't read the {name} dictionary: {e}"),
            }
        }
        if dicts.is_empty() {
            return None;
        }
        let words = learned.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
        let learned_words = words.lines().map(str::trim).filter(|w| !w.is_empty()).map(String::from).collect();
        Some(Hunspell { dicts, learned: RefCell::new(learned_words), ignored: RefCell::new(HashSet::new()), file: learned })
    }

    /// The dictionaries in use, by name.
    #[cfg(test)]
    pub fn languages(&self) -> Vec<&str> {
        self.dicts.iter().map(|d| d.name.as_str()).collect()
    }

    fn known(&self, word: &str) -> bool {
        let straight = word.replace('’', "'");
        let (learned, ignored) = (self.learned.borrow(), self.ignored.borrow());
        let known = [word, straight.as_str()].into_iter().any(|w| {
            learned.contains(w) || learned.contains(&w.to_lowercase()) || ignored.contains(w) || self.dicts.iter().any(|d| d.dict.check(w))
        });
        known
    }
}

impl Speller for Hunspell {
    fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        words(text).into_iter().filter(|(_, w)| !self.known(w)).map(|(r, _)| r).collect()
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        // The dictionary whose alphabet the word is written in first.
        let lower: Vec<char> = word.to_lowercase().chars().collect();
        let mut order: Vec<&Dictionary> = self.dicts.iter().collect();
        order.sort_by_key(|d| std::cmp::Reverse(lower.iter().filter(|c| d.letters.contains(c)).count()));
        let mut out = Vec::new();
        for d in order {
            d.dict.suggest(&word.replace('’', "'"), &mut out);
            if !out.is_empty() {
                if word.contains('’') {
                    out.iter_mut().for_each(|g| *g = g.replace('\'', "’"));
                }
                return out;
            }
        }
        out
    }

    fn learn(&self, word: &str) {
        if !self.learned.borrow_mut().insert(word.to_string()) {
            return;
        }
        let Some(file) = &self.file else { return };
        let mut words: Vec<String> = self.learned.borrow().iter().cloned().collect();
        words.sort();
        let written = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| crate::files::write_atomic(file, (words.join("\n") + "\n").as_bytes()));
        if let Err(e) = written {
            eprintln!("dostoevsky: couldn't keep the learned word: {e}");
        }
    }

    fn ignore(&self, word: &str) {
        self.ignored.borrow_mut().insert(word.to_string());
    }
}

/// The words to check, with their char ranges: letters, with an apostrophe inside (UAX #29). Not
/// a word with a digit or an underscore, one in capitals (an acronym), nor anything in a web or
/// mail address.
fn words(text: &str) -> Vec<(Range<usize>, &str)> {
    // Whitespace-separated stretches that are addresses, in bytes.
    let mut addresses: Vec<Range<usize>> = Vec::new();
    let mut from = None;
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (c.is_whitespace(), from) {
            (false, None) => from = Some(i),
            (true, Some(s)) => {
                let chunk = &text[s..i];
                if chunk.contains("://") || chunk.starts_with("www.") || chunk.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.')) {
                    addresses.push(s..i);
                }
                from = None;
            }
            _ => {}
        }
    }
    let (mut byte, mut char_at) = (0, 0);
    let mut out = Vec::new();
    for (at, token) in text.split_word_bound_indices() {
        char_at += text[byte..at].chars().count();
        byte = at;
        if !token.chars().any(char::is_alphabetic) || token.chars().any(|c| c.is_numeric() || c == '_') {
            continue;
        }
        if addresses.iter().any(|a| a.contains(&at)) {
            continue;
        }
        let letters = token.chars().filter(|c| c.is_alphabetic()).count();
        if letters > 1 && token.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase) {
            continue;
        }
        let len = token.chars().count();
        out.push((char_at..char_at + len, token));
    }
    out
}

/// Every dictionary in `dirs`: a `.dic` beside its `.aff`, by name; the first folder's wins.
fn available(dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut here: Vec<(String, PathBuf)> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "dic") && p.with_extension("aff").is_file())
            .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p)))
            .collect();
        here.sort();
        for (name, path) in here {
            if !out.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                out.push((name, path));
            }
        }
    }
    out
}

/// One dictionary for each language: the same name (`en_GB`), else the language's own country
/// (`de_DE`, `en_US` for English), else the first of that language.
fn pick(found: &[(String, PathBuf)], languages: &[String]) -> Vec<(String, PathBuf)> {
    let primary = |name: &str| name.split(['_', '-']).next().unwrap_or("").to_lowercase();
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for tag in languages {
        let lang = primary(tag);
        if out.iter().any(|(n, _)| primary(n) == lang) {
            continue;
        }
        let home = if lang == "en" { "en_US".to_string() } else { format!("{lang}_{}", lang.to_uppercase()) };
        let choice = found
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(tag))
            .or_else(|| found.iter().find(|(n, _)| n.eq_ignore_ascii_case(&home)))
            .or_else(|| found.iter().find(|(n, _)| primary(n) == lang));
        if let Some(c) = choice {
            out.push(c.clone());
        }
    }
    out
}

/// A dictionary and its `TRY` letters, read in the encoding its `.aff` names (`SET KOI8-R`).
fn open(dic: &Path) -> Result<(spellbook::Dictionary, HashSet<char>), String> {
    let aff_bytes = std::fs::read(dic.with_extension("aff")).map_err(|e| e.to_string())?;
    let dic_bytes = std::fs::read(dic).map_err(|e| e.to_string())?;
    let encoding = encoding_of(&aff_bytes)?;
    let decode = |bytes: &[u8]| encoding.decode(bytes).0.into_owned();
    let (aff, dic) = (decode(&aff_bytes), decode(&dic_bytes));
    let letters = aff.lines().find_map(|l| l.strip_prefix("TRY")).map(|t| t.trim().to_lowercase().chars().collect()).unwrap_or_default();
    let dict = spellbook::Dictionary::new(&aff, &dic).map_err(|e| e.to_string())?;
    Ok((dict, letters))
}

/// The encoding an `.aff` file's `SET` line names; UTF-8 when it names none.
fn encoding_of(aff: &[u8]) -> Result<&'static encoding_rs::Encoding, String> {
    let set = aff.split(|b| *b == b'\n').find_map(|line| line.strip_prefix(b"SET")).map(|rest| String::from_utf8_lossy(rest).trim().to_lowercase());
    let Some(label) = set else { return Ok(encoding_rs::UTF_8) };
    // Hunspell's names for Windows' code pages.
    let label = label.strip_prefix("microsoft-").unwrap_or(&label).to_string();
    encoding_rs::Encoding::for_label(label.as_bytes()).ok_or_else(|| format!("its encoding {label} isn't known"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Hunspell, available, encoding_of, pick, words};
    use crate::spell::Speller;

    const EN_AFF: &str = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzESIANRTOLCDUGMPHBYFVKWZ'\nSFX S Y 1\nSFX S 0 s .\n";
    const EN_DIC: &str = "8\nwriting/S\nfriction\nmost\nof\nthe\ncomes\nfrom\ndon't\n";

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("dostoevsky-hunspell-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn put(dir: &Path, name: &str, aff: &[u8], dic: &[u8]) {
        std::fs::write(dir.join(format!("{name}.aff")), aff).unwrap();
        std::fs::write(dir.join(format!("{name}.dic")), dic).unwrap();
    }

    fn wrong(speller: &Hunspell, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        speller.misspelled(text).into_iter().map(|r| chars[r].iter().collect()).collect()
    }

    #[test]
    fn words_are_letters_and_not_addresses_numbers_or_acronyms() {
        let list = |t: &str| words(t).into_iter().map(|(_, w)| w.to_string()).collect::<Vec<_>>();
        assert_eq!(list("Don’t stop—it's “fine”, ok?"), ["Don’t", "stop", "it's", "fine", "ok"]);
        assert_eq!(list("see https://exmaple.com/pth or www.foo.org and me@mial.com"), ["see", "or", "and"]);
        assert_eq!(list("NASA sent R2D2 to x_y in 1999 at 10am"), ["sent", "to", "in", "at"]);
        assert_eq!(list("A cat"), ["A", "cat"], "a single capital is a word");
        // Char ranges, not bytes.
        let ranges: Vec<_> = words("Ёж и ёлка").into_iter().map(|(r, _)| r).collect();
        assert_eq!(ranges, [0..2, 3..4, 5..9]);
    }

    #[test]
    fn a_word_any_dictionary_knows_is_right() {
        let d = dir("check");
        put(&d.0, "en_US", EN_AFF.as_bytes(), EN_DIC.as_bytes());
        // A Russian dictionary in KOI8-R, as some systems still ship it.
        let (aff, _, _) = encoding_rs::KOI8_R.encode("SET KOI8-R\nTRY оеаинтсрвлкмдпуяыьгзбчйхжшюцщэфъё\n");
        let (dic, _, _) = encoding_rs::KOI8_R.encode("3\nписьмо\nтрение\nи\n");
        put(&d.0, "ru_RU", &aff, &dic);
        let speller = Hunspell::load(std::slice::from_ref(&d.0), &["ru_RU".into(), "en".into()], None).unwrap();
        assert_eq!(speller.languages(), ["ru_RU", "en_US"]);
        assert_eq!(wrong(&speller, "Most of the frction comes from writings."), ["frction"]);
        assert_eq!(wrong(&speller, "Трение и писмо."), ["писмо"]);
        assert_eq!(wrong(&speller, "Don’t, don't."), Vec::<String>::new(), "a curly apostrophe is a straight one");
        assert_eq!(speller.guesses("frction"), ["friction"]);
        assert_eq!(speller.guesses("писмо"), ["письмо"], "from the dictionary of the word's letters");
    }

    #[test]
    fn learned_words_are_kept_and_ignored_ones_are_not() {
        let d = dir("learn");
        put(&d.0, "en_US", EN_AFF.as_bytes(), EN_DIC.as_bytes());
        let file = d.0.join("Spelling/learned.txt");
        let speller = Hunspell::load(std::slice::from_ref(&d.0), &["en".into()], Some(file.clone())).unwrap();
        speller.learn("Raskolnikov");
        speller.ignore("Sonya");
        assert!(wrong(&speller, "Raskolnikov and Sonya").iter().all(|w| w == "and"));
        let again = Hunspell::load(std::slice::from_ref(&d.0), &["en".into()], Some(file.clone())).unwrap();
        assert_eq!(wrong(&again, "Raskolnikov Sonya"), ["Sonya"], "learned lasts, ignored was the session's");
        assert_eq!(std::fs::read_to_string(file).unwrap(), "Raskolnikov\n");
    }

    #[test]
    fn one_dictionary_per_language_the_nearest() {
        let found: Vec<(String, PathBuf)> = ["de_AT", "de_DE", "en_AU", "en_GB", "en_US", "ru_RU", "pt_BR"].iter().map(|n| (n.to_string(), PathBuf::from(n))).collect();
        let names = |langs: &[&str]| pick(&found, &langs.iter().map(|s| s.to_string()).collect::<Vec<_>>()).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!(names(&["en_GB", "en"]), ["en_GB"], "the country asked for, and English once");
        assert_eq!(names(&["de", "en"]), ["de_DE", "en_US"], "the language's own country");
        assert_eq!(names(&["pt_BR"]), ["pt_BR"]);
        assert_eq!(names(&["pt"]), ["pt_BR"], "else the first of the language");
        assert_eq!(names(&["fr", "ru"]), ["ru_RU"], "none for French");
    }

    #[test]
    fn a_folder_without_dictionaries_gives_no_speller() {
        let d = dir("none");
        std::fs::write(d.0.join("en_US.dic"), "1\nword\n").unwrap();
        assert!(available(std::slice::from_ref(&d.0)).is_empty(), "a .dic without its .aff is no dictionary");
        assert!(Hunspell::load(std::slice::from_ref(&d.0), &["en".into()], None).is_none());
    }

    /// The system's own dictionaries, as a writer's Linux has them: `hunspell-en-us` and
    /// `hunspell-ru` on Debian and Ubuntu (the Linux CI installs both and runs it).
    #[test]
    #[ignore = "needs Hunspell's English and Russian dictionaries installed"]
    fn system_dictionaries_find_russian_and_english() {
        let speller = Hunspell::load(&super::dictionary_dirs(), &["ru_RU".into(), "en_US".into()], None).expect("dictionaries installed");
        assert_eq!(speller.languages(), ["ru_RU", "en_US"]);
        assert_eq!(wrong(&speller, "Большая часть трения в писмене происходит."), ["писмене"]);
        assert_eq!(wrong(&speller, "Most of the frction comes too erly."), ["frction", "erly"]);
        assert_eq!(wrong(&speller, "Он сказал: frction и писмене, don’t."), ["frction", "писмене"]);
        assert!(speller.guesses("frction").contains(&"friction".to_string()), "{:?}", speller.guesses("frction"));
        let russian = speller.guesses("писмене");
        assert!(!russian.is_empty() && russian.iter().all(|g| g.chars().all(|c| c == ' ' || ('а'..='я').contains(&c.to_lowercase().next().unwrap()))), "from the Russian dictionary: {russian:?}");
    }

    #[test]
    fn an_aff_file_names_its_encoding() {
        assert_eq!(encoding_of(b"# x\nSET ISO8859-1\nTRY a\n").unwrap(), encoding_rs::WINDOWS_1252);
        assert_eq!(encoding_of(b"SET microsoft-cp1251\n").unwrap(), encoding_rs::WINDOWS_1251);
        assert_eq!(encoding_of(b"TRY a\n").unwrap(), encoding_rs::UTF_8);
        assert!(encoding_of(b"SET ISCII-DEVANAGARI\n").is_err());
    }
}
