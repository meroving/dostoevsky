use std::ops::Range;

use crate::segment::Sentence;

/// Char range in `hay`: an exact match first, then ignoring case, curly quotes and runs of spaces.
pub fn find(hay: &str, quote: &str) -> Option<Range<usize>> {
    let quote = quote.trim();
    if quote.is_empty() {
        return None;
    }
    if let Some(b) = hay.find(quote) {
        let start = hay[..b].chars().count();
        return Some(start..start + quote.chars().count());
    }
    // `map[i]` is the char index in `hay` of normalised char `i`.
    let (h, map) = normalise(hay);
    let (q, _) = normalise(quote);
    if q.is_empty() {
        return None;
    }
    let hc: Vec<char> = h.chars().collect();
    let qc: Vec<char> = q.chars().collect();
    let at = (0..=hc.len().checked_sub(qc.len())?).find(|&i| hc[i..i + qc.len()] == qc[..])?;
    let start = map[at];
    let end = map[at + qc.len() - 1] + 1;
    Some(start..end)
}

fn normalise(s: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut map = Vec::new();
    let mut last_space = false;
    for (i, c) in s.chars().enumerate() {
        let c = match c {
            '\u{2018}' | '\u{2019}' | '\u{02BC}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2013}' | '\u{2014}' => '-',
            '\u{00A0}' | '\t' | '\n' => ' ',
            c => c,
        };
        if c == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        for l in c.to_lowercase() {
            out.push(l);
            map.push(i);
        }
    }
    (out, map)
}

/// `n` is 1-based, as numbered in the prompt.
pub fn in_sentence(sentences: &[Sentence], n: i64, quote: &str) -> Option<Range<usize>> {
    let s = sentences.get(usize::try_from(n).ok()?.checked_sub(1)?)?;
    let r = find(&s.text, quote)?;
    Some(s.range.start + r.start..s.range.start + r.end)
}

/// Also looks in the neighbouring sentences: models are often off by one.
pub fn near_sentence(sentences: &[Sentence], n: i64, quote: &str) -> Option<Range<usize>> {
    in_sentence(sentences, n, quote).or_else(|| in_sentence(sentences, n - 1, quote)).or_else(|| in_sentence(sentences, n + 1, quote))
}

/// Takes one adjacent space along, so removing the range doesn't leave two spaces.
pub fn with_space(text: &[char], r: Range<usize>) -> Range<usize> {
    let after = text.get(r.end).copied();
    let before = r.start.checked_sub(1).and_then(|i| text.get(i).copied());
    if after == Some(' ') {
        r.start..r.end + 1
    } else if before == Some(' ') {
        r.start - 1..r.end
    } else {
        r
    }
}

pub fn to_words(text: &[char], mut r: Range<usize>) -> Range<usize> {
    let word = |c: char| english::is_word_char(c);
    while r.start > 0 && r.start < text.len() && word(text[r.start]) && word(text[r.start - 1]) {
        r.start -= 1;
    }
    while r.end > 0 && r.end < text.len() && word(text[r.end - 1]) && word(text[r.end]) {
        r.end += 1;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::sentences;

    #[test]
    fn exact_then_loose() {
        assert_eq!(find("Most of the friction", "the friction"), Some(8..20));
        assert_eq!(find("You can\u{2019}t remember", "can't"), Some(4..9));
        assert_eq!(find("It is,  in a way", "IS, IN A"), Some(3..12));
        assert_eq!(find("hello", "bye"), None);
        assert_eq!(find("hello", "  "), None);
    }

    #[test]
    fn quotes_anchor_in_the_document() {
        let t = "One sentence here. Then quietly another.";
        let s = sentences(t, 0..t.len());
        let r = in_sentence(&s, 2, "quietly").unwrap();
        assert_eq!(&t[r.clone()], "quietly");
        assert_eq!(in_sentence(&s, 1, "quietly"), None);
        assert_eq!(near_sentence(&s, 1, "quietly"), Some(r));
        assert_eq!(in_sentence(&s, 0, "One"), None);
        assert_eq!(in_sentence(&s, 9, "One"), None);
    }

    #[test]
    fn spaces_and_words() {
        let t: Vec<char> = "candidates quietly disappear.".chars().collect();
        assert_eq!(with_space(&t, 11..18), 11..19);
        let t2: Vec<char> = "a street, really.".chars().collect();
        assert_eq!(with_space(&t2, 10..16), 9..16);
        assert_eq!(to_words(&t, 12..16), 11..18);
    }
}
