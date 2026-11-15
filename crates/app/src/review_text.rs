//! What a Review row says, without a UI (mockup review-snippets, A): the page's words around
//! a result, cut inside its sentence; a fix as a word diff; the names of a fix's two buttons.

use std::ops::Range;

use crate::text::TextIndex;

/// Words of context a row shows before and after its result.
pub const BEFORE: usize = 4;
pub const AFTER: usize = 7;
/// A fix's buttons quote its words up to this many characters; longer, they say "the fix" / "mine".
const NAMED: usize = 18;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Same(String),
    Gone(String),
    Added(String),
}

/// `was` → `now`, word by word: the words both share are `Same`; the rest is `Gone` (only in
/// `was`) or `Added` (only in `now`). Changed parts never start or end with whitespace, and
/// where both go and come at one place, `Gone` comes first.
pub fn word_diff(was: &str, now: &str) -> Vec<Part> {
    let a = tokens(was);
    let b = tokens(now);
    // Long stretches aren't worth a table: everything changed.
    let ops: Vec<(u8, &str)> = if a.len() * b.len() > 160_000 {
        a.iter().map(|t| (1, *t)).chain(b.iter().map(|t| (2, *t))).collect()
    } else {
        lcs_ops(&a, &b)
    };
    let mut out: Vec<Part> = Vec::new();
    let (mut gone, mut added) = (String::new(), String::new());
    let flush = |out: &mut Vec<Part>, gone: &mut String, added: &mut String| {
        if gone.is_empty() && added.is_empty() {
            return;
        }
        let lead = lead_ws(added).or_else(|| lead_ws(gone)).unwrap_or_default();
        let trail = trail_ws(added).or_else(|| trail_ws(gone)).unwrap_or_default();
        push_same(out, &lead);
        if !gone.trim().is_empty() {
            out.push(Part::Gone(gone.trim().to_string()));
        }
        if !added.trim().is_empty() {
            out.push(Part::Added(added.trim().to_string()));
        }
        push_same(out, &trail);
        gone.clear();
        added.clear();
    };
    for (op, t) in ops {
        match op {
            0 => {
                flush(&mut out, &mut gone, &mut added);
                push_same(&mut out, t);
            }
            1 => gone.push_str(t),
            _ => added.push_str(t),
        }
    }
    flush(&mut out, &mut gone, &mut added);
    out
}

/// How many separate places a diff changes.
pub fn changes(parts: &[Part]) -> usize {
    let mut n = 0;
    let mut in_change = false;
    for p in parts {
        let changed = !matches!(p, Part::Same(_));
        if changed && !in_change {
            n += 1;
        }
        in_change = changed;
    }
    n
}

/// What a fix's buttons say after "Settle on": the words they put in or take out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Names {
    /// [“the”] · [“teh”].
    Replace { gone: String, added: String },
    /// [no “an”] · [“an”].
    Remove(String),
    /// [“of”] · [no “of”].
    Add(String),
    /// [the fix] · [mine]: more than one place, or too long to quote.
    Whole,
}

pub fn names(parts: &[Part]) -> Names {
    if changes(parts) != 1 {
        return Names::Whole;
    }
    let gone = parts.iter().find_map(|p| if let Part::Gone(t) = p { Some(t.clone()) } else { None });
    let added = parts.iter().find_map(|p| if let Part::Added(t) = p { Some(t.clone()) } else { None });
    let short = |t: &Option<String>| t.as_ref().is_none_or(|t| t.chars().count() <= NAMED);
    if !short(&gone) || !short(&added) {
        return Names::Whole;
    }
    match (gone, added) {
        (Some(gone), Some(added)) => Names::Replace { gone, added },
        (Some(gone), None) => Names::Remove(gone),
        (None, Some(added)) => Names::Add(added),
        (None, None) => Names::Whole,
    }
}

/// A typo's fix (mockup review-settle): nothing to keep the original for, so its row offers no
/// Keep both. The word it replaces is misspelled, or the fix changes only punctuation or case, or
/// it takes out a word doubled next to it.
pub fn typo(parts: &[Part], misspelled: impl Fn(&str) -> bool) -> bool {
    let bare = |t: &str| t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let was: String = parts.iter().filter_map(|p| match p {
        Part::Same(t) | Part::Gone(t) => Some(t.as_str()),
        Part::Added(_) => None,
    }).collect();
    let now: String = parts.iter().filter_map(|p| match p {
        Part::Same(t) | Part::Added(t) => Some(t.as_str()),
        Part::Gone(_) => None,
    }).collect();
    if bare(&was) == bare(&now) {
        return true;
    }
    if changes(parts) != 1 {
        return false;
    }
    let at = parts.iter().position(|p| !matches!(p, Part::Same(_))).unwrap_or(0);
    match (&parts[at], parts.get(at + 1)) {
        (Part::Gone(g), Some(Part::Added(_))) => {
            let w = g.trim_matches(|c: char| !c.is_alphanumeric());
            !w.is_empty() && !w.contains(char::is_whitespace) && misspelled(w)
        }
        (Part::Gone(g), _) => {
            let w = bare(g);
            let near = |p: Option<&Part>, last: bool| match p {
                Some(Part::Same(t)) => {
                    let mut words = t.split_whitespace();
                    let n = if last { words.next_back() } else { words.next() };
                    n.is_some_and(|n| bare(n) == w)
                }
                _ => false,
            };
            !w.is_empty() && !g.contains(char::is_whitespace) && (near(at.checked_sub(1).and_then(|i| parts.get(i)), true) || near(parts.get(at + 1), false))
        }
        _ => false,
    }
}

/// The page's words before and after `range`, inside the sentence it starts in: up to `before`
/// words before it and `after` words after, an ellipsis where the sentence goes on. Line breaks
/// read as spaces.
pub fn around(idx: &TextIndex, range: Range<usize>, before: usize, after: usize) -> (String, String) {
    let len = idx.len();
    let range = range.start.min(len)..range.end.min(len).max(range.start.min(len));
    let para = idx.paragraph_at(range.start);
    let text = idx.slice(para.clone());
    let n = text.chars().count();
    let sentence = prose::sentences(text, 0..n)
        .into_iter()
        .map(|s| s.range.start + para.start..s.range.end + para.start)
        .find(|s| s.start <= range.start && range.start < s.end.max(s.start + 1))
        .unwrap_or(para.clone());
    let from = sentence.start.min(range.start);
    let to = sentence.end.max(range.end).min(para.end.max(range.end));
    let flat = |s: &str| s.replace('\n', " ");
    let head = flat(idx.slice(from..range.start));
    let tail = flat(idx.slice(range.end..to));
    (last_words(head.trim_start(), before), first_words(tail.trim_end(), after))
}

/// `range` without the whitespace at its ends: a cut's range may take the space before it.
pub fn tight(idx: &TextIndex, range: Range<usize>) -> Range<usize> {
    let t = idx.slice(range.clone());
    let lead = t.chars().take_while(|c| c.is_whitespace()).count();
    let trail = if t.chars().count() > lead { t.chars().rev().take_while(|c| c.is_whitespace()).count() } else { 0 };
    let start = (range.start + lead).min(range.end);
    start..(range.end - trail).max(start)
}

fn last_words(s: &str, k: usize) -> String {
    let starts: Vec<usize> = word_starts(s);
    if starts.len() > k {
        format!("…{}", &s[starts[starts.len() - k]..])
    } else {
        s.to_string()
    }
}

fn first_words(s: &str, k: usize) -> String {
    let ends = word_ends(s);
    // The words right after the result are the ones that close it: "rhythm." keeps its stop.
    if ends.len() > k {
        format!("{}…", &s[..ends[k - 1]])
    } else {
        s.to_string()
    }
}

fn word_starts(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut prev_ws = true;
    for (i, ch) in s.char_indices() {
        let ws = ch.is_whitespace();
        if !ws && prev_ws {
            out.push(i);
        }
        prev_ws = ws;
    }
    out
}

fn word_ends(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut in_word = false;
    for (i, ch) in s.char_indices() {
        let ws = ch.is_whitespace();
        if ws && in_word {
            out.push(i);
        }
        in_word = !ws;
    }
    if in_word {
        out.push(s.len());
    }
    out
}

/// Runs of whitespace and runs of everything else.
fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<bool> = None;
    for (i, ch) in s.char_indices() {
        let ws = ch.is_whitespace();
        if prev.is_some_and(|p| p != ws) {
            out.push(&s[start..i]);
            start = i;
        }
        prev = Some(ws);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// 0 both, 1 only in `a`, 2 only in `b`; in order.
fn lcs_ops<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(u8, &'a str)> {
    let (n, m) = (a.len(), b.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[at(i, j)] = if a[i] == b[j] { t[at(i + 1, j + 1)] + 1 } else { t[at(i + 1, j)].max(t[at(i, j + 1)]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(n + m);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push((0, a[i]));
            i += 1;
            j += 1;
        } else if j < m && (i == n || t[at(i, j + 1)] >= t[at(i + 1, j)]) {
            out.push((2, b[j]));
            j += 1;
        } else {
            out.push((1, a[i]));
            i += 1;
        }
    }
    out
}

fn lead_ws(s: &str) -> Option<String> {
    let w: String = s.chars().take_while(|c| c.is_whitespace()).collect();
    (!w.is_empty()).then_some(w)
}

fn trail_ws(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        return None;
    }
    let n = s.chars().rev().take_while(|c| c.is_whitespace()).count();
    (n > 0).then(|| s.chars().skip(s.chars().count() - n).collect())
}

fn push_same(out: &mut Vec<Part>, t: &str) {
    if t.is_empty() {
        return;
    }
    if let Some(Part::Same(s)) = out.last_mut() {
        // Whitespace on both sides of a change collapses to one space.
        if t.trim().is_empty() && s.ends_with(char::is_whitespace) {
            return;
        }
        s.push_str(t);
    } else {
        out.push(Part::Same(t.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::{around, changes, names, tight, typo, word_diff, Names, Part};
    use crate::text::TextIndex;

    fn same(s: &str) -> Part {
        Part::Same(s.into())
    }
    fn gone(s: &str) -> Part {
        Part::Gone(s.into())
    }
    fn added(s: &str) -> Part {
        Part::Added(s.into())
    }
    /// The diff read back the way a row draws it: struck words in ~~, added in [].
    fn drawn(p: &[Part]) -> String {
        let mut out = String::new();
        for (i, x) in p.iter().enumerate() {
            match x {
                Part::Same(s) => out.push_str(s),
                Part::Gone(s) => out.push_str(&format!("~~{s}~~")),
                Part::Added(s) => {
                    if matches!(p.get(i.wrapping_sub(1)), Some(Part::Gone(_))) {
                        out.push(' ');
                    }
                    out.push_str(&format!("[{s}]"));
                }
            }
        }
        out
    }

    #[test]
    fn typos_are_misspellings_punctuation_and_doubled_words() {
        let dict = |w: &str| ["teh", "dissapear"].contains(&w);
        let is = |was: &str, now: &str| typo(&word_diff(was, now), dict);
        assert!(is("You pick teh word", "You pick the word"), "a misspelled word");
        assert!(is("quietly dissapear.", "quietly disappear."));
        assert!(is("and so on and", "and so on, and"), "a comma");
        assert!(is("the the door", "the door"), "a doubled word");
        assert!(is("Paris", "paris"), "case only");
        assert!(!is("and lets you decide", "and let you decide"), "a word for another word");
        assert!(!is("into an a calmer rhythm", "into a calmer rhythm"), "an article, not doubled");
        assert!(!is("only that one them", "only that one of them"), "an added word");
        assert!(!is("He walk home and sit", "He walked home and sat"), "two places");
    }

    #[test]
    fn a_removed_word_is_struck_between_its_neighbours() {
        let d = word_diff("into an a calmer rhythm", "into a calmer rhythm");
        assert_eq!(drawn(&d), "into ~~an~~ a calmer rhythm");
        assert_eq!(names(&d), Names::Remove("an".into()));
    }

    #[test]
    fn a_replaced_word_goes_then_comes() {
        let d = word_diff("teh", "the");
        assert_eq!(d, [gone("teh"), added("the")]);
        assert_eq!(names(&d), Names::Replace { gone: "teh".into(), added: "the".into() });
        assert_eq!(drawn(&word_diff("and lets you", "and let you")), "and ~~lets~~ [let] you");
    }

    #[test]
    fn an_added_word_keeps_its_spaces() {
        let d = word_diff("one them was", "one of them was");
        assert_eq!(d, [same("one "), added("of"), same(" them was")]);
        assert_eq!(names(&d), Names::Add("of".into()));
    }

    #[test]
    fn two_places_or_long_words_name_the_whole_fix() {
        let d = word_diff("he walk and talk", "he walks and talks");
        assert_eq!(changes(&d), 2);
        assert_eq!(names(&d), Names::Whole);
        assert_eq!(names(&word_diff("walked", "really did not walk at all to anyone")), Names::Whole);
    }

    #[test]
    fn context_stays_inside_the_sentence() {
        let t = "First one here. You pick a word, teh sentence moves on, and the others quietly go away for good. Last.";
        let idx = TextIndex::new(t.to_string());
        let at = t.find("teh").unwrap();
        let (b, a) = around(&idx, at..at + 3, 4, 7);
        assert_eq!(b, "You pick a word, ");
        assert_eq!(a, " sentence moves on, and the others quietly…");
        let at = t.find("good").unwrap();
        let (b, a) = around(&idx, at..at + 4, 3, 7);
        assert_eq!((b.as_str(), a.as_str()), ("…go away for ", "."));
    }

    #[test]
    fn a_cut_that_takes_the_space_before_it_belongs_to_its_own_sentence() {
        let t = "They quietly disappear. A week later you forget.";
        let idx = TextIndex::new(t.to_string());
        let at = t.find(" A week").unwrap();
        let r = tight(&idx, at..t.chars().count());
        assert_eq!(idx.slice(r.clone()), "A week later you forget.");
        assert_eq!(around(&idx, r, 4, 7), (String::new(), String::new()));
    }

    #[test]
    fn context_is_cut_mid_sentence_with_an_ellipsis() {
        let t = "Most of the friction in writing comes from choosing far too early on.";
        let idx = TextIndex::new(t.to_string());
        let at = t.find("choosing").unwrap();
        let (b, _) = around(&idx, at..at + 8, 2, 7);
        assert_eq!(b, "…comes from ");
    }
}
