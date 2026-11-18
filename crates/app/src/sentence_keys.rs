use std::ops::Range;

use doc_model::Place;

use crate::markdown::{self, BlockKind};
use crate::text::TextIndex;

pub fn sentences_in(idx: &TextIndex, p: &Range<usize>) -> Vec<Range<usize>> {
    if p.is_empty() {
        return Vec::new();
    }
    let s = idx.slice(p.clone());
    prose::sentences(s, 0..s.chars().count())
        .into_iter()
        .map(|x| p.start + x.range.start..p.start + x.range.end)
        .collect()
}

fn text_line(idx: &TextIndex, l: usize, down: bool) -> Option<usize> {
    let blank = |l: usize| idx.line_str(l).trim().is_empty();
    if down {
        (l + 1..idx.lines.len()).find(|&k| !blank(k))
    } else {
        (0..l).rev().find(|&k| !blank(k))
    }
}

fn next_paragraph(idx: &TextIndex, p: &Range<usize>, down: bool) -> Option<Range<usize>> {
    let l = if down { idx.line_of(p.end) } else { idx.line_of(p.start) };
    let k = text_line(idx, l, down)?;
    Some(idx.paragraph_at(idx.lines[k].char_start))
}

fn inside(skip: &[Range<usize>], c: usize) -> bool {
    skip.iter().any(|r| r.start <= c && c < r.end)
}

/// The end of the text after the last; starts inside `skip` (faded cuts) don't count.
pub fn next_sentence(idx: &TextIndex, c: usize, skip: &[Range<usize>]) -> usize {
    let mut p = idx.paragraph_at(c);
    loop {
        if let Some(s) = sentences_in(idx, &p).into_iter().find(|s| s.start > c && !inside(skip, s.start)) {
            return s.start;
        }
        match next_paragraph(idx, &p, true) {
            Some(n) => p = n,
            None => return idx.len(),
        }
    }
}

/// Or of the one before when `c` is already at a start.
pub fn prev_sentence(idx: &TextIndex, c: usize, skip: &[Range<usize>]) -> usize {
    let mut p = idx.paragraph_at(c);
    loop {
        if let Some(s) = sentences_in(idx, &p).into_iter().rev().find(|s| s.start < c && !inside(skip, s.start)) {
            return s.start;
        }
        match next_paragraph(idx, &p, false) {
            Some(n) => p = n,
            None => return 0,
        }
    }
}

pub fn sentence_around(idx: &TextIndex, r: &Range<usize>) -> Option<Range<usize>> {
    let p = idx.paragraph_at(r.start);
    sentences_in(idx, &p).into_iter().find(|s| s.start <= r.start && r.end <= s.end)
}

pub fn is_sentence(idx: &TextIndex, r: &Range<usize>) -> bool {
    !r.is_empty() && sentence_around(idx, r).is_some_and(|s| s == *r)
}

/// The smallest unit strictly holding `r`: word, group, sentence, paragraph, then `None`.
pub fn grow(idx: &TextIndex, groups: &[Range<usize>], r: &Range<usize>) -> Option<Range<usize>> {
    let mut units: Vec<Range<usize>> = Vec::new();
    if r.is_empty() {
        let w = idx.word_at(r.start);
        if idx.slice(w.clone()).chars().any(english::is_word_char) {
            units.push(w);
        }
    }
    units.extend(groups.iter().cloned());
    units.extend(sentence_around(idx, r));
    let p = idx.paragraph_at(r.start);
    if !idx.slice(p.clone()).trim().is_empty() {
        units.push(p);
    }
    units
        .into_iter()
        .filter(|u| u.start <= r.start && r.end <= u.end && *u != *r)
        .min_by_key(|u| u.len())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unit {
    Sentences,
    Paragraphs,
}

/// Spaces around `r` don't count.
pub fn whole_unit(idx: &TextIndex, r: &Range<usize>) -> Option<Unit> {
    let r = trim(idx, r);
    if r.is_empty() {
        return None;
    }
    let (pa, pz) = (idx.paragraph_at(r.start), idx.paragraph_at(r.end));
    if pa.start == r.start && pz.end == r.end {
        return Some(Unit::Paragraphs);
    }
    let line = idx.line_of(r.start);
    if idx.line_of(r.end) != line {
        return None;
    }
    let ss = sentences_in(idx, &pa);
    let starts = ss.iter().any(|s| s.start == r.start);
    let ends = ss.iter().any(|s| s.end == r.end);
    (starts && ends).then_some(Unit::Sentences)
}

fn trim(idx: &TextIndex, r: &Range<usize>) -> Range<usize> {
    let s = idx.slice(r.clone());
    let lead = s.chars().take_while(|c| c.is_whitespace()).count();
    let trail = s.chars().rev().take_while(|c| c.is_whitespace()).count();
    let n = s.chars().count();
    if lead >= n {
        return r.start..r.start;
    }
    r.start + lead..r.end - trail
}

fn plain(idx: &TextIndex, line: usize) -> bool {
    markdown::style_line(idx.line_str(line)).kind == BlockKind::Paragraph
}

/// `None` at the document's edge, a heading, list item or quote line a sentence would join,
/// or a group `r` is inside or would cut.
pub fn move_target(idx: &TextIndex, groups: &[Range<usize>], r: &Range<usize>, up: bool) -> Option<Place> {
    let r = trim(idx, r);
    let unit = whole_unit(idx, &r)?;
    for g in groups {
        let overlaps = g.start < r.end && r.start < g.end;
        let holds = g.start <= r.start && r.end <= g.end && *g != r;
        let held = r.start <= g.start && g.end <= r.end;
        if holds || (overlaps && !held) {
            return None;
        }
    }
    let place = match unit {
        Unit::Paragraphs => {
            let (first, last) = (idx.paragraph_at(r.start), idx.paragraph_at(r.end));
            if up {
                // Headings are passed, but the document's first one (its title) stays on top.
                let prev = next_paragraph(idx, &first, false)?;
                if prev.start == 0 && matches!(markdown::style_line(idx.line_str(0)).kind, BlockKind::Heading(_)) {
                    return None;
                }
                Place::Block(prev.start)
            } else {
                let next = next_paragraph(idx, &last, true)?;
                Place::Block(next_paragraph(idx, &next, true).map_or(idx.len(), |p| p.start))
            }
        }
        Unit::Sentences => {
            let line = idx.line_of(r.start);
            let lr = idx.line_range(line);
            let p = idx.paragraph_at(r.start);
            let ss: Vec<_> = sentences_in(idx, &p).into_iter().filter(|s| lr.start <= s.start && s.end <= lr.end).collect();
            let near = if up {
                ss.iter().rev().find(|s| s.end <= r.start).map(|s| Place::Inline(s.start))
            } else {
                ss.iter().find(|s| s.start >= r.end).map(|s| Place::Inline(s.end))
            };
            match near {
                Some(p) => p,
                None => {
                    // Into the next paragraph: plain lines only, never leaving an empty line behind.
                    if !plain(idx, line) || ss.len() < 2 {
                        return None;
                    }
                    let k = text_line(idx, line, !up)?;
                    if !plain(idx, k) {
                        return None;
                    }
                    let kp = idx.paragraph_at(idx.lines[k].char_start);
                    let lk = idx.line_range(k);
                    let ks: Vec<_> = sentences_in(idx, &kp).into_iter().filter(|s| lk.start <= s.start && s.end <= lk.end).collect();
                    if up {
                        Place::Inline(ks.last()?.end)
                    } else {
                        Place::Inline(ks.first()?.start)
                    }
                }
            }
        }
    };
    if let Place::Inline(t) = place {
        if groups.iter().any(|g| g.start < t && t < g.end) {
            return None;
        }
    }
    Some(place)
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::{grow, is_sentence, move_target, next_sentence, prev_sentence, whole_unit, Unit};
    use crate::text::TextIndex;
    use doc_model::Place;

    const T: &str = "# The Garret\n\nHe lodged in S. Place and walked. His garret was a cupboard. The landlady lived below.\n\nAnd each time he passed, he felt sick. He was in debt.";

    fn idx() -> TextIndex {
        TextIndex::new(T.to_string())
    }
    fn at(s: &str) -> usize {
        T[..T.find(s).unwrap()].chars().count()
    }
    fn span(s: &str) -> std::ops::Range<usize> {
        at(s)..at(s) + s.chars().count()
    }

    #[test]
    fn walking_goes_sentence_by_sentence_over_paragraphs() {
        let i = idx();
        assert_eq!(next_sentence(&i, at("lodged"), &[]), at("His garret"));
        assert_eq!(next_sentence(&i, at("His garret"), &[]), at("The landlady"));
        assert_eq!(next_sentence(&i, at("landlady lived"), &[]), at("And each"));
        assert_eq!(next_sentence(&i, at("debt"), &[]), i.len());
        assert_eq!(next_sentence(&i, 3, &[]), at("He lodged"));
        assert_eq!(prev_sentence(&i, at("cupboard"), &[]), at("His garret"));
        assert_eq!(prev_sentence(&i, at("His garret"), &[]), at("He lodged"));
        assert_eq!(prev_sentence(&i, at("And each"), &[]), at("The landlady"));
        assert_eq!(prev_sentence(&i, at("He lodged"), &[]), at("The Garret"));
        assert_eq!(prev_sentence(&i, 1, &[]), 0);
    }

    #[test]
    fn walking_skips_faded_text() {
        let i = idx();
        let ghost = span("His garret was a cupboard.");
        assert_eq!(next_sentence(&i, at("lodged"), std::slice::from_ref(&ghost)), at("The landlady"));
        assert_eq!(prev_sentence(&i, at("The landlady"), &[ghost]), at("He lodged"));
    }

    #[test]
    fn growing_goes_word_group_sentence_paragraph() {
        let i = idx();
        let c = at("garret was") + 2;
        let w = grow(&i, &[], &(c..c)).unwrap();
        assert_eq!(w, span("garret"));
        let s = grow(&i, &[], &w).unwrap();
        assert_eq!(s, span("His garret was a cupboard."));
        let p = grow(&i, &[], &s).unwrap();
        assert_eq!(p, span("He lodged in S. Place and walked. His garret was a cupboard. The landlady lived below."));
        assert_eq!(grow(&i, &[], &p), None);
        let g = span("a cupboard");
        let c = at("cupboard");
        assert_eq!(grow(&i, std::slice::from_ref(&g), &(c..c)), Some(span("cupboard")));
        assert_eq!(grow(&i, std::slice::from_ref(&g), &span("cupboard")), Some(g.clone()));
        assert_eq!(grow(&i, std::slice::from_ref(&g), &g), Some(span("His garret was a cupboard.")));
        assert_eq!(grow(&i, &[], &span("garret was")), Some(span("His garret was a cupboard.")));
    }

    #[test]
    fn whole_units() {
        let i = idx();
        assert_eq!(whole_unit(&i, &span("His garret was a cupboard.")), Some(Unit::Sentences));
        assert_eq!(whole_unit(&i, &span("His garret was a cupboard. The landlady lived below.")), Some(Unit::Sentences));
        assert_eq!(whole_unit(&i, &span("And each time he passed, he felt sick. He was in debt.")), Some(Unit::Paragraphs));
        assert_eq!(whole_unit(&i, &span("garret was a")), None);
        assert!(is_sentence(&i, &span("He was in debt.")));
        assert!(!is_sentence(&i, &span("He was in debt")));
        assert!(!is_sentence(&i, &span("S. Place and walked.")));
    }

    #[test]
    fn where_a_sentence_moves() {
        let i = idx();
        let s = span("His garret was a cupboard.");
        assert_eq!(move_target(&i, &[], &s, false), Some(Place::Inline(span("The landlady lived below.").end)));
        assert_eq!(move_target(&i, &[], &s, true), Some(Place::Inline(at("He lodged"))));
        let last = span("The landlady lived below.");
        assert_eq!(move_target(&i, &[], &last, false), Some(Place::Inline(at("And each"))));
        let first = span("He lodged in S. Place and walked.");
        assert_eq!(move_target(&i, &[], &first, true), None);
        let back = span("And each time he passed, he felt sick.");
        assert_eq!(move_target(&i, &[], &back, true), Some(Place::Inline(last.end)));
        assert_eq!(move_target(&i, &[at("His")..last.end], &s, false), None);
        assert_eq!(move_target(&i, &[span("cupboard")], &s, false), Some(Place::Inline(last.end)));
        assert_eq!(move_target(&i, &[span("lived below")], &s, false), Some(Place::Inline(last.end)));
        assert_eq!(move_target(&i, &[span("garret was")], &span("His garret was a cupboard. The landlady lived below."), true), Some(Place::Inline(at("He lodged"))));
        assert_eq!(move_target(&i, &[span("cupboard. The")], &s, false), None);
    }

    #[test]
    fn where_a_paragraph_moves() {
        let i = idx();
        let p1 = span("He lodged in S. Place and walked. His garret was a cupboard. The landlady lived below.");
        let p2 = span("And each time he passed, he felt sick. He was in debt.");
        assert_eq!(move_target(&i, &[], &p1, false), Some(Place::Block(i.len())));
        assert_eq!(move_target(&i, &[], &p1, true), None);
        assert_eq!(move_target(&i, &[], &p2, true), Some(Place::Block(p1.start)));
        assert_eq!(move_target(&i, &[], &p2, false), None);
    }
}
