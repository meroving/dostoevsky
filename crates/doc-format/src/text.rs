use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser};

/// Chars on each side of an anchor.
pub const CONTEXT: usize = 16;

/// In chars; block 0 starts at 0 (leading blank lines are its), `end` drops trailing whitespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub start: usize,
    pub end: usize,
}

/// Fixed: changing it would move block indexes.
pub fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

/// Never empty: a body with no blocks has one empty block 0.
pub fn blocks(body: &str) -> Vec<Block> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut depth = 0usize;
    for (ev, r) in Parser::new_ext(body, markdown_options()).into_offset_iter() {
        match ev {
            Event::Start(_) => {
                if depth == 0 {
                    ranges.push(r);
                }
                depth += 1;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ if depth == 0 => ranges.push(r),
            _ => {}
        }
    }
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Block> = Vec::with_capacity(ranges.len().max(1));
    let (mut chars, mut bytes, mut last_end) = (0, 0, 0);
    for r in ranges {
        if !out.is_empty() && r.start < last_end {
            continue;
        }
        let trimmed_end = r.start + body[r.clone()].trim_end().len();
        chars += body[bytes..r.start].chars().count();
        let start = chars;
        chars += body[r.start..trimmed_end].chars().count();
        bytes = trimmed_end;
        last_end = r.end;
        out.push(Block { start, end: chars });
    }
    if out.is_empty() {
        out.push(Block { start: 0, end: 0 });
    }
    out[0].start = 0;
    out
}

pub fn block_of(blocks: &[Block], pos: usize) -> usize {
    blocks.partition_point(|b| b.start <= pos).saturating_sub(1)
}

/// Up to [`CONTEXT`] chars before `s`, not crossing a line break.
pub fn before(text: &[char], s: usize) -> String {
    let mut i = s;
    while i > 0 && s - i < CONTEXT && text[i - 1] != '\n' {
        i -= 1;
    }
    text[i..s].iter().collect()
}

/// Up to [`CONTEXT`] chars after `e`, not crossing a line break.
pub fn after(text: &[char], e: usize) -> String {
    let mut i = e;
    while i < text.len() && i - e < CONTEXT && text[i] != '\n' {
        i += 1;
    }
    text[e..i].iter().collect()
}

pub fn levenshtein(a: &[char], b: &[char]) -> usize {
    if a.is_empty() || b.is_empty() {
        return a.len().max(b.len());
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 1 − edit distance / longer length; two empty strings are identical.
pub fn similarity(a: &[char], b: &[char]) -> f64 {
    let n = a.len().max(b.len());
    if n == 0 {
        return 1.0;
    }
    1.0 - levenshtein(a, b) as f64 / n as f64
}

/// Best approximate occurrence of `pat` in `text` (Sellers' algorithm: edit distance with a
/// free start and end). Returns `(range, distance)`; ties go to the start closest to `near`.
pub fn fuzzy_find(pat: &[char], text: &[char], near: usize) -> Option<(Range<usize>, usize)> {
    let m = pat.len();
    if m == 0 || text.is_empty() {
        return None;
    }
    // (cost, start) per pattern row, for the current text column.
    let mut col: Vec<(usize, usize)> = (0..=m).map(|i| (i, 0)).collect();
    let mut best: Option<(Range<usize>, usize)> = None;
    for j in 1..=text.len() {
        let mut diag = col[0];
        col[0] = (0, j);
        for i in 1..=m {
            let up = col[i]; // D[i][j-1]
            let sub = (diag.0 + usize::from(pat[i - 1] != text[j - 1]), diag.1);
            let del = (col[i - 1].0 + 1, col[i - 1].1); // D[i-1][j]
            let ins = (up.0 + 1, up.1);
            let mut c = sub;
            for o in [del, ins] {
                if o.0 < c.0 {
                    c = o;
                }
            }
            diag = up;
            col[i] = c;
        }
        let (cost, start) = col[m];
        let better = match &best {
            None => true,
            Some((r, d)) => {
                cost < *d || (cost == *d && start.abs_diff(near) < r.start.abs_diff(near))
            }
        };
        if better {
            best = Some((start..j, cost));
        }
    }
    best.filter(|(r, _)| !r.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn blocks_are_top_level_and_in_chars() {
        let body = "# Tïtle\n\nPara one\nstill one.\n\n- a\n- b\n\n> quote\n\n---\n\nlast\n";
        let b = blocks(body);
        let text = c(body);
        let src: Vec<String> = b
            .iter()
            .map(|b| text[b.start..b.end].iter().collect())
            .collect();
        assert_eq!(
            src,
            [
                "# Tïtle",
                "Para one\nstill one.",
                "- a\n- b",
                "> quote",
                "---",
                "last"
            ]
        );
        assert_eq!(blocks(""), [Block { start: 0, end: 0 }]);
        assert_eq!(
            blocks("\n\nx\n")[0].start,
            0,
            "leading blank lines belong to block 0"
        );
        assert_eq!(block_of(&b, 0), 0);
        assert_eq!(block_of(&b, 9), 1);
        assert_eq!(block_of(&b, 8), 0, "the gap belongs to the block before");
    }

    #[test]
    fn context_stops_at_line_breaks() {
        let t = c("# Head\nMost of the friction in writing comes from");
        assert_eq!(before(&t, 2), "# ");
        assert_eq!(after(&t, 6), "");
        assert_eq!(before(&t, 19), "Most of the ");
        assert_eq!(after(&t, 27), " in writing come");
    }

    #[test]
    fn fuzzy_finds_edited_spans() {
        let text = c("the quick brown fox jumps over the lazy dog");
        let (r, d) = fuzzy_find(&c("quikc brown"), &text, 0).unwrap();
        assert_eq!(text[r].iter().collect::<String>(), "quick brown");
        assert_eq!(d, 2);
        let (r, d) = fuzzy_find(&c("the"), &text, 30).unwrap();
        assert_eq!((r, d), (31..34, 0), "ties go to the nearest");
        assert!((similarity(&c("kitten"), &c("sitting")) - (1.0 - 3.0 / 7.0)).abs() < 1e-9);
        assert_eq!(similarity(&[], &[]), 1.0);
    }
}
