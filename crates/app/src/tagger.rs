use prose::{Lang, Token};

/// Char ranges into `text`; empty when the tagger can't read `lang`.
pub fn tokens(text: &str, lang: Lang) -> Vec<Token> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    #[cfg(target_os = "macos")]
    {
        mac::tokens(text, lang)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = lang;
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::{autoreleasepool, Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    use objc2_foundation::{NSArray, NSRange, NSString, NSValue};
    use prose::{Class, Lang, Token};

    #[link(name = "NaturalLanguage", kind = "framework")]
    unsafe extern "C" {}

    /// `NLTokenUnitWord`.
    const WORD: isize = 0;
    /// `NLTaggerOmitPunctuation | NLTaggerOmitWhitespace | NLTaggerOmitOther`.
    const OMIT: usize = 2 | 4 | 8;

    pub fn tokens(text: &str, lang: Lang) -> Vec<Token> {
        autoreleasepool(|_| unsafe { read(text, lang) }).unwrap_or_default()
    }

    unsafe fn read(text: &str, lang: Lang) -> Option<Vec<Token>> {
        let class = AnyClass::get(c"NLTagger")?;
        let lexical = NSString::from_str("LexicalClass");
        let lemma = NSString::from_str("Lemma");
        let schemes = NSArray::from_retained_slice(&[lexical.clone(), lemma.clone()]);
        let tagger: Allocated<AnyObject> = msg_send![class, alloc];
        let tagger: Option<Retained<AnyObject>> = msg_send![tagger, initWithTagSchemes: &*schemes];
        let tagger = tagger?;
        let string = NSString::from_str(text);
        let all = NSRange::new(0, string.length());
        let _: () = msg_send![&*tagger, setString: &*string];
        let _: () = msg_send![&*tagger, setLanguage: &*NSString::from_str(lang.code()), range: all];
        let mut ranges: Option<Retained<NSArray<NSValue>>> = None;
        let tags: Option<Retained<NSArray<NSString>>> = msg_send![&*tagger, tagsInRange: all, unit: WORD, scheme: &*lexical, options: OMIT, tokenRanges: &mut ranges];
        let (tags, ranges) = (tags?, ranges?);
        let map = super::utf16_to_char(text);
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::with_capacity(tags.len());
        for (tag, value) in tags.iter().zip(ranges.iter()) {
            let r: NSRange = msg_send![&*value, rangeValue];
            let (Some(&start), Some(&end)) = (map.get(r.location), map.get(r.location + r.length)) else { continue };
            if start >= end {
                continue;
            }
            let base: Option<Retained<NSString>> = msg_send![&*tagger, tagAtIndex: r.location, unit: WORD, scheme: &*lemma, tokenRange: std::ptr::null_mut::<NSRange>()];
            out.push(Token {
                range: start..end,
                word: chars[start..end].iter().collect(),
                class: Class::from_tag(&tag.to_string()),
                lemma: base.map(|b| b.to_string().to_lowercase()),
            });
        }
        Some(out)
    }
}

/// The char of each UTF-16 unit, plus one past the end.
#[cfg(target_os = "macos")]
fn utf16_to_char(text: &str) -> Vec<usize> {
    let mut map = Vec::with_capacity(text.len() + 1);
    for (i, ch) in text.chars().enumerate() {
        for _ in 0..ch.len_utf16() {
            map.push(i);
        }
    }
    map.push(text.chars().count());
    map
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::tokens;
    use prose::{Class, Lang};

    #[test]
    fn reads_words() {
        let text = "Your sentence isn’t erased by a good editor.";
        let t = tokens(text, Lang::En);
        let erased = t.iter().find(|t| t.word == "erased").expect("erased");
        assert_eq!(erased.class, Class::Verb);
        assert_eq!(erased.lemma.as_deref(), Some("erase"));
        assert_eq!(text.chars().skip(erased.range.start).take(erased.range.len()).collect::<String>(), "erased");

        let ru = "Письмо было написано ещё вчера.";
        let t = tokens(ru, Lang::Ru);
        assert!(t.iter().any(|t| t.word == "написано" && t.class == Class::Verb), "{t:?}");
        assert!(prose::is_passive(&t, Lang::Ru));
    }
}
