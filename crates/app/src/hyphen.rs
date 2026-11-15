use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use prose::Lang;

type Cache = HashMap<(Lang, String), Rc<[usize]>>;

thread_local! {
    static WORDS: RefCell<Cache> = RefCell::new(HashMap::new());
}

/// Char indices `word` may break before, ascending; empty without a dictionary.
pub fn breaks(word: &str, lang: Lang) -> Rc<[usize]> {
    WORDS.with(|w| {
        let mut w = w.borrow_mut();
        if let Some(b) = w.get(&(lang, word.to_string())) {
            return b.clone();
        }
        if w.len() > 20_000 {
            w.clear();
        }
        let b: Rc<[usize]> = sys::breaks(word, lang).into();
        w.insert((lang, word.to_string()), b.clone());
        b
    })
}

#[cfg(target_os = "macos")]
mod sys {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::c_void;

    use prose::Lang;

    type CFIndex = isize;
    type Ref = *const c_void;

    #[repr(C)]
    struct CFRange {
        location: CFIndex,
        length: CFIndex,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCharacters(alloc: Ref, chars: *const u16, len: CFIndex) -> Ref;
        fn CFLocaleCreate(alloc: Ref, id: Ref) -> Ref;
        fn CFStringIsHyphenationAvailableForLocale(locale: Ref) -> u8;
        fn CFStringGetHyphenationLocationBeforeIndex(s: Ref, location: CFIndex, limit: CFRange, options: usize, locale: Ref, ch: *mut u32) -> CFIndex;
        fn CFRelease(cf: Ref);
    }

    fn cf_string(s: &str) -> Ref {
        let u: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: `u` outlives the call, which copies it.
        unsafe { CFStringCreateWithCharacters(std::ptr::null(), u.as_ptr(), u.len() as CFIndex) }
    }

    thread_local! {
        /// `None`: no dictionary.
        static LOCALES: RefCell<HashMap<Lang, Option<usize>>> = RefCell::new(HashMap::new());
    }

    fn locale(lang: Lang) -> Option<Ref> {
        LOCALES.with(|l| {
            *l.borrow_mut().entry(lang).or_insert_with(|| {
                let id = match lang {
                    Lang::En => "en_US",
                    Lang::Ru => "ru_RU",
                    Lang::De => "de_DE",
                    Lang::Fr => "fr_FR",
                    Lang::Es => "es_ES",
                    Lang::It => "it_IT",
                    Lang::Pt => "pt_BR",
                };
                let s = cf_string(id);
                // SAFETY: plain CoreFoundation calls on objects made here; `s` is released once.
                unsafe {
                    let loc = CFLocaleCreate(std::ptr::null(), s);
                    CFRelease(s);
                    if loc.is_null() {
                        return None;
                    }
                    if CFStringIsHyphenationAvailableForLocale(loc) == 0 {
                        CFRelease(loc);
                        return None;
                    }
                    Some(loc as usize)
                }
            })
        })
        .map(|p| p as Ref)
    }

    pub fn breaks(word: &str, lang: Lang) -> Vec<usize> {
        let Some(loc) = locale(lang) else { return Vec::new() };
        let mut at = Vec::new();
        let mut n = 0;
        for c in word.chars() {
            at.push(n);
            n += c.len_utf16();
        }
        let s = cf_string(word);
        let mut out = Vec::new();
        let mut before = n as CFIndex;
        // SAFETY: `s` and `loc` are live CoreFoundation objects; `s` is released once.
        unsafe {
            loop {
                let p = CFStringGetHyphenationLocationBeforeIndex(s, before, CFRange { location: 0, length: n as CFIndex }, 0, loc, std::ptr::null_mut());
                if p <= 0 || p >= before {
                    break;
                }
                if let Ok(i) = at.binary_search(&(p as usize)) {
                    out.push(i);
                }
                before = p;
            }
            CFRelease(s);
        }
        out.reverse();
        out
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    pub fn breaks(_: &str, _: prose::Lang) -> Vec<usize> {
        Vec::new()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use prose::Lang;

    use super::breaks;

    fn shown(word: &str, lang: Lang) -> String {
        let b = breaks(word, lang);
        word.chars().enumerate().flat_map(|(i, c)| if b.contains(&i) { vec!['-', c] } else { vec![c] }).collect()
    }

    #[test]
    fn the_system_dictionaries_break_words() {
        let en = shown("hesitation", Lang::En);
        assert!(en.contains('-') && en.replace('-', "") == "hesitation", "{en}");
        let ru = shown("непременно", Lang::Ru);
        assert!(ru.contains('-') && ru.replace('-', "") == "непременно", "{ru}");
        assert!(breaks("a", Lang::En).is_empty());
    }

    #[test]
    fn breaks_are_ascending_char_indices() {
        let b = breaks("отворенной", Lang::Ru);
        assert!(b.windows(2).all(|w| w[0] < w[1]), "{b:?}");
        assert!(b.iter().all(|&i| i > 0 && i < "отворенной".chars().count()));
    }
}
