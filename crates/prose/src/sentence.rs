use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sentence {
    /// Display chars in the document.
    pub range: Range<usize>,
    pub text: String,
    /// 0-based, counting non-blank lines.
    pub paragraph: usize,
}

/// `range` is in display chars; each non-blank line is a paragraph, its markers left out.
pub fn sentences(text: &str, range: Range<usize>) -> Vec<Sentence> {
    let chars: Vec<char> = text.chars().collect();
    let range = range.start.min(chars.len())..range.end.min(chars.len());
    let mut out = Vec::new();
    let mut para = 0usize;
    let mut line_start = range.start;
    let mut i = range.start;
    while i <= range.end {
        if i == range.end || chars[i] == '\n' {
            let line: String = chars[line_start..i].iter().collect();
            let trimmed = line.trim();
            let is_rule = !trimmed.is_empty()
                && trimmed.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '))
                && trimmed.chars().filter(|c| !c.is_whitespace()).count() >= 3;
            if !trimmed.is_empty() && !is_rule {
                let skip = marker_len(&line);
                let body: String = line.chars().skip(skip).collect();
                let base = line_start + skip;
                for r in split(&body) {
                    let s = &body[r.clone()];
                    let lead = s.chars().take_while(|c| c.is_whitespace()).count();
                    let len = s.chars().count();
                    let trail = s.chars().rev().take_while(|c| c.is_whitespace()).count();
                    if lead + trail >= len {
                        continue;
                    }
                    let at = base + body[..r.start].chars().count();
                    let (start, end) = (at + lead, at + len - trail);
                    out.push(Sentence { text: chars[start..end].iter().collect(), range: start..end, paragraph: para });
                }
                para += 1;
            }
            line_start = i + 1;
        }
        i += 1;
    }
    out
}

/// Byte ranges, spaces included.
fn split(body: &str) -> Vec<Range<usize>> {
    // A ? or ! before a lower-case word (“Is it you?” she asked.) ends nothing.
    let mut joined: Vec<Range<usize>> = Vec::new();
    for (b, s) in body.split_sentence_bound_indices() {
        let lower = s.trim_start().chars().next().is_some_and(char::is_lowercase);
        match joined.last_mut() {
            Some(last) if lower || false_end(&body[last.clone()]) => last.end = b + s.len(),
            _ => joined.push(b..b + s.len()),
        }
    }
    let mut out = Vec::new();
    for r in joined {
        let s = &body[r.clone()];
        let mut from = 0;
        for (k, c) in s.char_indices() {
            if c != '…' {
                continue;
            }
            let after = k + c.len_utf8();
            let rest = &s[after..];
            let spaces = rest.len() - rest.trim_start().len();
            if spaces > 0 && rest.trim_start().chars().next().is_some_and(char::is_uppercase) {
                out.push(r.start + from..r.start + after + spaces);
                from = after + spaces;
            }
        }
        out.push(r.start + from..r.end);
    }
    out
}

const TITLES: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "St", "Prof", "Sr", "Jr", "Mt", "Capt", "Col", "Gen", "Lt", "Rev", "Hon", // en
    "Hr", "Fr", "Nr", // de
    "M", "Mme", "Mlle", "Me", // fr
    "Sra", "Srta", "Dña", "Dn", // es, pt
    "Sig", "Dott", "Avv", // it
    "ул", "им", "тов", "гр", // ru
];

fn false_end(piece: &str) -> bool {
    let t = piece.trim_end();
    let Some(head) = t.strip_suffix('.') else { return false };
    let word: String = head.chars().rev().take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
    // The word must stand alone ("S.", not "U.S." or "end.").
    let before = head[..head.len() - word.len()].chars().next_back();
    if before.is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '"' | '“' | '«' | '\'' | '‘')) {
        return false;
    }
    let mut cs = word.chars();
    let initial = matches!((cs.next(), cs.next()), (Some(c), None) if c.is_uppercase());
    initial || TITLES.contains(&word.as_str())
}

fn marker_len(line: &str) -> usize {
    let mut n = 0;
    let mut it = line.chars().peekable();
    while let Some(&c) = it.peek() {
        if c == '#' || c == '>' {
            n += 1;
            it.next();
        } else {
            break;
        }
    }
    if n == 0 {
        let rest: String = line.chars().take(6).collect();
        let t = rest.trim_start();
        let indent = rest.chars().count() - t.chars().count();
        if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
            return indent + 2;
        }
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && t[digits..].starts_with(". ") {
            return indent + digits + 2;
        }
        return 0;
    }
    n + line.chars().skip(n).take_while(|c| *c == ' ').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(t: &str) -> Vec<String> {
        sentences(t, 0..t.chars().count()).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn splits_paragraphs_into_sentences() {
        let t = "# Keep the Words\n\nMost of the friction comes early. You pick a word.\n\nEditors know this.";
        let s = sentences(t, 0..t.chars().count());
        let texts: Vec<_> = s.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Keep the Words", "Most of the friction comes early.", "You pick a word.", "Editors know this."]);
        assert_eq!(s.iter().map(|s| s.paragraph).collect::<Vec<_>>(), [0, 1, 1, 2]);
        let chars: Vec<char> = t.chars().collect();
        for x in &s {
            assert_eq!(chars[x.range.clone()].iter().collect::<String>(), x.text);
        }
    }

    #[test]
    fn a_range_restricts_the_sentences() {
        let t = "Skip me. Keep me.";
        let s = sentences(t, 9..17);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].text, "Keep me.");
        assert_eq!(s[0].range, 9..17);
    }

    #[test]
    fn non_ascii_offsets_are_chars() {
        let t = "Он пришёл. Café ouvert.";
        let s = sentences(t, 0..t.chars().count());
        assert_eq!(s[1].text, "Café ouvert.");
        assert_eq!(s[1].range.start, 11);
    }

    #[test]
    fn rules_and_list_markers_are_skipped() {
        assert_eq!(texts("- item one\n---\n1. step two"), ["item one", "step two"]);
    }

    #[test]
    fn initials_and_titles_end_nothing() {
        let t = "A young man came out of the garret in which he lodged in S. Place and walked towards K. bridge. He stopped.";
        assert_eq!(texts(t), [t.strip_suffix(" He stopped.").unwrap(), "He stopped."]);
        assert_eq!(texts("Mr. Marmeladov had been a counsellor. Then he drank."), ["Mr. Marmeladov had been a counsellor.", "Then he drank."]);
        assert_eq!(texts("Он жил на ул. Садовой. Было жарко."), ["Он жил на ул. Садовой.", "Было жарко."]);
        assert_eq!(texts("M. Dupont est là. Il attend."), ["M. Dupont est là.", "Il attend."]);
    }

    #[test]
    fn a_word_that_only_ends_like_a_title_still_ends() {
        assert_eq!(texts("He said OK. Then he left."), ["He said OK.", "Then he left."]);
        assert_eq!(texts("It was the U.S. Then it wasn't."), ["It was the U.S.", "Then it wasn't."]);
    }

    #[test]
    fn dialogue_stays_whole() {
        assert_eq!(texts("“Is it you?” she asked. “Yes, it’s me,” he said."), ["“Is it you?” she asked.", "“Yes, it’s me,” he said."]);
        assert_eq!(texts("— Ты дома? — спросила она. Он не ответил."), ["— Ты дома? — спросила она.", "Он не ответил."]);
    }

    #[test]
    fn an_ellipsis_before_a_capital_ends_a_sentence() {
        assert_eq!(texts("He stopped on the stairs… Then he went on."), ["He stopped on the stairs…", "Then he went on."]);
        assert_eq!(texts("He stopped… and went on."), ["He stopped… and went on."]);
    }
}
