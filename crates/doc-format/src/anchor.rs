use std::ops::Range;

use crate::text::{after, before, block_of, fuzzy_find, similarity, Block};

/// Whole-document search: minimum score, and lead over the runner-up.
const GLOBAL_MIN: f64 = 0.75;
const GLOBAL_MARGIN: f64 = 0.1;
const FUZZY_MIN: f64 = 0.8;
const FUZZY_BLOCKS: usize = 2;
const PARAGRAPH_MIN: f64 = 0.6;
const PARAGRAPH_BLOCKS: usize = 3;
/// Context similarity assumed when a record has no context strings.
const NO_CONTEXT: f64 = 0.5;
/// A point found by one side of its context alone needs at least this many chars of it.
const MIN_SIDE: usize = 6;
/// Fuzzy search is skipped beyond this many DP cells.
const FUZZY_MAX_CELLS: usize = 50_000_000;

/// A container's text and its blocks (one block for variants and notes).
pub struct Haystack<'a> {
    pub text: &'a [char],
    pub blocks: &'a [Block],
    /// Whole-document search; only the body has more than one block.
    pub global: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Hint {
    pub block: usize,
    pub offset: Option<usize>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    Exact(Range<usize>),
    /// Something similar: the body wins, so the caller replaces the active variant with the
    /// text in this range.
    Fuzzy(Range<usize>),
}

impl Found {
    pub fn range(&self) -> Range<usize> {
        match self {
            Found::Exact(r) | Found::Fuzzy(r) => r.clone(),
        }
    }
}

impl Haystack<'_> {
    fn expected(&self, h: &Hint) -> Option<usize> {
        // Offsets come from the file, which anyone may have edited: never trust them to add up.
        let b = self.blocks.get(h.block)?;
        b.start.checked_add(h.offset.unwrap_or(0))
    }

    fn hint_block(&self, h: &Hint) -> usize {
        h.block.min(self.blocks.len() - 1)
    }

    fn block_span(&self, i: usize) -> Range<usize> {
        let end = self.blocks.get(i + 1).map_or(self.text.len(), |b| b.start);
        self.blocks[i].start..end
    }

    fn matches_at(&self, s: usize, quote: &[char]) -> bool {
        s.checked_add(quote.len())
            .is_some_and(|e| e <= self.text.len() && &self.text[s..e] == quote)
    }

    /// Starts of `quote` beginning in `lo..hi`.
    fn occurrences(&self, quote: &[char], lo: usize, hi: usize) -> Vec<usize> {
        let Some(&first) = quote.first() else {
            return Vec::new();
        };
        let hi = hi.min((self.text.len() + 1).saturating_sub(quote.len()));
        (lo..hi)
            .filter(|&s| self.text[s] == first && self.matches_at(s, quote))
            .collect()
    }

    fn context(&self, h: &Hint, s: usize, e: usize) -> f64 {
        let side = |stored: &Option<String>, actual: String| match stored {
            None => NO_CONTEXT,
            Some(st) => {
                let a: Vec<char> = st.chars().collect();
                let b: Vec<char> = actual.chars().collect();
                similarity(&a, &b)
            }
        };
        (side(&h.before, before(self.text, s)) + side(&h.after, after(self.text, e))) / 2.0
    }

    fn find_exact(&self, h: &Hint, quote: &[char]) -> Option<usize> {
        if quote.is_empty() {
            return None;
        }
        let expected = self.expected(h);
        if let Some(p) = expected.filter(|&p| self.matches_at(p, quote)) {
            return Some(p);
        }
        let near = expected.unwrap_or_else(|| self.blocks[self.hint_block(h)].start);
        let span = self.block_span(self.hint_block(h));
        let best = self
            .occurrences(quote, span.start, span.end)
            .into_iter()
            .map(|s| (self.context(h, s, s + quote.len()), s))
            .max_by(|a, b| {
                a.0.total_cmp(&b.0)
                    .then(b.1.abs_diff(near).cmp(&a.1.abs_diff(near)))
            });
        if let Some((_, s)) = best {
            return Some(s);
        }
        if !self.global {
            return None;
        }
        let hb = self.hint_block(h);
        let mut scored: Vec<(f64, usize)> = self
            .occurrences(quote, 0, self.text.len())
            .into_iter()
            .map(|s| {
                let dist = block_of(self.blocks, s).abs_diff(hb) as f64;
                let score = 0.75 * self.context(h, s, s + quote.len()) + 0.25 / (1.0 + dist);
                (score, s)
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        match scored.as_slice() {
            [] => None,
            [(best, s)] => (*best >= GLOBAL_MIN).then_some(*s),
            [(best, s), (second, _), ..] => {
                (*best >= GLOBAL_MIN && best - second >= GLOBAL_MARGIN).then_some(*s)
            }
        }
    }

    pub fn find_alt(&self, h: &Hint, quote: &[char], paragraph: bool) -> Option<Found> {
        if let Some(s) = self.find_exact(h, quote) {
            return Some(Found::Exact(s..s + quote.len()));
        }
        let hb = self.hint_block(h);
        if paragraph {
            return self.find_paragraph(hb, quote);
        }
        let lo = self.blocks[hb.saturating_sub(FUZZY_BLOCKS)].start;
        let hi_block = (hb + FUZZY_BLOCKS + 1).min(self.blocks.len());
        let hi = self
            .blocks
            .get(hi_block)
            .map_or(self.text.len(), |b| b.start);
        let hi = (hi + quote.len()).min(self.text.len());
        if quote.len() * (hi - lo) > FUZZY_MAX_CELLS {
            return None;
        }
        let near = self.expected(h).unwrap_or(lo).saturating_sub(lo);
        let (r, dist) = fuzzy_find(quote, &self.text[lo..hi], near)?;
        // An exact occurrence was already judged (and rejected) by `find_exact`; fuzzy matching
        // is only for spans edited outside the app.
        if dist == 0 {
            return None;
        }
        let sim = 1.0 - dist as f64 / quote.len().max(r.len()) as f64;
        (sim >= FUZZY_MIN).then(|| Found::Fuzzy(lo + r.start..lo + r.end))
    }

    fn find_paragraph(&self, hb: usize, quote: &[char]) -> Option<Found> {
        let content = |i: usize| &self.text[self.blocks[i].start..self.blocks[i].end];
        let mut order: Vec<usize> = (0..self.blocks.len()).collect();
        order.sort_by_key(|&i| i.abs_diff(hb));
        if let Some(&i) = order.iter().find(|&&i| content(i) == quote) {
            let b = self.blocks[i];
            return Some(Found::Exact(b.start..b.end));
        }
        let mut best: Option<(f64, usize)> = None;
        for &i in order
            .iter()
            .take_while(|&&i| i.abs_diff(hb) <= PARAGRAPH_BLOCKS)
        {
            let c = content(i);
            let (short, long) = (c.len().min(quote.len()), c.len().max(quote.len()));
            if c.is_empty() || (short as f64) < PARAGRAPH_MIN * long as f64 {
                continue; // can't reach the threshold
            }
            let s = similarity(c, quote);
            if s >= PARAGRAPH_MIN && best.is_none_or(|(b, _)| s > b) {
                best = Some((s, i));
            }
        }
        let (_, i) = best?;
        let b = self.blocks[i];
        Some(Found::Fuzzy(b.start..b.end))
    }

    /// `length` chars starting with `quote`, clipped to the text; ghosts are never fuzzy-matched.
    pub fn find_ghost(
        &self,
        h: &Hint,
        quote: Option<&[char]>,
        length: usize,
    ) -> Option<Range<usize>> {
        let s = match quote {
            Some(q) if !q.is_empty() => self.find_exact(h, q)?,
            _ => self.expected(h).filter(|&p| p < self.text.len())?,
        };
        let e = s.saturating_add(length).min(self.text.len());
        (s < e).then_some(s..e)
    }
}

/// A point between two chars, or the gap before a block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spot {
    /// In the body; for a spot inside a variant, its top-level ancestor's.
    pub block: usize,
    /// Chars from the start of the block's source (or the variant's text); `None` for a gap.
    pub offset: Option<usize>,
    /// Up to 16 chars each side, not crossing a line break; for a gap, the ends of the blocks
    /// around it.
    pub before: String,
    pub after: String,
    /// Inside a group's variant: the group's id and the variant's index.
    pub parent: Option<String>,
    pub pvariant: Option<usize>,
}

impl Spot {
    pub fn is_gap(&self) -> bool {
        self.offset.is_none()
    }

    pub fn read(v: &serde_json::Value) -> Option<Spot> {
        let o = v.as_object()?;
        let num = |k: &str| o.get(k).and_then(serde_json::Value::as_u64).map(|n| n as usize);
        let s = |k: &str| o.get(k).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        let gap = o.get("gap").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let offset = num("offset");
        if !gap && offset.is_none() {
            return None;
        }
        Some(Spot {
            block: num("block").unwrap_or(0),
            offset: if gap { None } else { offset },
            before: s("before"),
            after: s("after"),
            parent: o.get("parent").and_then(serde_json::Value::as_str).map(String::from),
            pvariant: num("pvariant"),
        })
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        m.insert("block".into(), self.block.into());
        if let (Some(p), Some(k)) = (&self.parent, self.pvariant) {
            m.insert("parent".into(), p.clone().into());
            m.insert("pvariant".into(), k.into());
        }
        match self.offset {
            Some(o) => {
                m.insert("offset".into(), o.into());
            }
            None => {
                m.insert("gap".into(), true.into());
            }
        }
        if self.offset.is_some() || !self.before.is_empty() {
            m.insert("before".into(), self.before.clone().into());
        }
        if self.offset.is_some() || !self.after.is_empty() {
            m.insert("after".into(), self.after.clone().into());
        }
        serde_json::Value::Object(m)
    }

    pub fn point(text: &[char], blocks: &[Block], p: usize) -> Spot {
        let b = block_of(blocks, p);
        Spot {
            block: b,
            offset: Some(p - blocks[b].start),
            before: before(text, p),
            after: after(text, p),
            parent: None,
            pvariant: None,
        }
    }

    /// `b` = the block count: after the last one.
    pub fn gap(text: &[char], blocks: &[Block], b: usize) -> Spot {
        Spot {
            block: b,
            offset: None,
            before: b.checked_sub(1).map(|i| before(text, blocks[i].end)).unwrap_or_default(),
            after: blocks.get(b).map(|bl| after(text, bl.start)).unwrap_or_default(),
            parent: None,
            pvariant: None,
        }
    }
}

impl Haystack<'_> {
    /// A side with no context means a line break (or the text's edge) there.
    pub fn find_point(&self, spot: &Spot) -> Option<usize> {
        let b: Vec<char> = spot.before.chars().collect();
        let a: Vec<char> = spot.after.chars().collect();
        let n = self.text.len();
        let fits = |p: usize| {
            p >= b.len()
                && p <= n
                && self.text[p - b.len()..p] == b[..]
                && self.text[p..].starts_with(&a)
                && (!b.is_empty() || p == 0 || self.text[p - 1] == '\n')
                && (!a.is_empty() || p == n || self.text[p] == '\n')
        };
        // Offsets come from the file: never trust them to add up.
        let expected = self.blocks.get(spot.block).and_then(|bl| bl.start.checked_add(spot.offset.unwrap_or(0)));
        if let Some(p) = expected.filter(|&p| fits(p)) {
            return Some(p);
        }
        let quote: Vec<char> = b.iter().chain(a.iter()).copied().collect();
        if quote.is_empty() {
            return None;
        }
        let near = expected.unwrap_or(0);
        let hb = spot.block.min(self.blocks.len() - 1);
        let span = self.block_span(hb);
        let here: Vec<usize> = self
            .occurrences(&quote, span.start, span.end)
            .into_iter()
            .map(|s| s + b.len())
            .filter(|&p| fits(p))
            .collect();
        if let Some(&p) = here.iter().min_by_key(|&&p| p.abs_diff(near)) {
            return Some(p);
        }
        // One side of the context alone (the other was edited), if long enough and there once.
        for (side, after_side) in [(&a, true), (&b, false)] {
            if side.len() < MIN_SIDE {
                continue;
            }
            let at: Vec<usize> = self
                .occurrences(side, span.start, span.end)
                .into_iter()
                .map(|s| if after_side { s } else { s + side.len() })
                .collect();
            if at.len() == 1 {
                return Some(at[0]);
            }
        }
        if !self.global {
            return None;
        }
        let all: Vec<usize> = self
            .occurrences(&quote, 0, n)
            .into_iter()
            .map(|s| s + b.len())
            .filter(|&p| fits(p))
            .collect();
        let dist = |p: usize| block_of(self.blocks, p).abs_diff(hb);
        let best = all.iter().map(|&p| dist(p)).min()?;
        let nearest: Vec<usize> = all.into_iter().filter(|&p| dist(p) == best).collect();
        (nearest.len() == 1).then(|| nearest[0])
    }

    /// Returns the char position: a block's start, or the end of the last block.
    pub fn find_gap(&self, spot: &Spot) -> Option<usize> {
        let count = self.blocks.len();
        let pos = |i: usize| if i < count { self.blocks[i].start } else { self.blocks[count - 1].end };
        let starts = |i: usize| {
            let a: Vec<char> = spot.after.chars().collect();
            if i < count {
                !a.is_empty() && self.text[self.blocks[i].start..].starts_with(&a)
            } else {
                a.is_empty()
            }
        };
        let ends = |i: usize| {
            let b: Vec<char> = spot.before.chars().collect();
            if i == 0 {
                b.is_empty()
            } else {
                !b.is_empty() && self.text[..self.blocks[i - 1].end].ends_with(&b)
            }
        };
        // The context as the writer took it: empty where a block edge is a line break.
        let exact = |i: usize| {
            let a = if i < count { after(self.text, self.blocks[i].start) } else { String::new() };
            let b = if i > 0 { before(self.text, self.blocks[i - 1].end) } else { String::new() };
            a == spot.after && b == spot.before
        };
        let near = spot.block.min(count);
        let pick = |ok: &dyn Fn(usize) -> bool| -> Option<usize> {
            let c: Vec<usize> = (0..=count).filter(|&i| ok(i)).collect();
            let best = c.iter().map(|i| i.abs_diff(near)).min()?;
            let at: Vec<usize> = c.into_iter().filter(|i| i.abs_diff(near) == best).collect();
            (at.len() == 1).then(|| at[0])
        };
        if exact(near) {
            return Some(pos(near));
        }
        pick(&exact)
            .or_else(|| pick(&|i| starts(i) && ends(i)))
            .or_else(|| (!spot.after.is_empty()).then(|| pick(&|i| starts(i))).flatten())
            .or_else(|| (!spot.before.is_empty()).then(|| pick(&|i| ends(i))).flatten())
            .map(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::blocks;

    fn c(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn hint(block: usize, offset: usize, before: &str, after: &str) -> Hint {
        Hint {
            block,
            offset: Some(offset),
            before: Some(before.into()),
            after: Some(after.into()),
        }
    }

    #[test]
    fn steps_in_order() {
        let body = "Intro.\n\nThe cat sat. The cat ran.\n\nA dog.\n";
        let (t, b) = (c(body), blocks(body));
        let h = Haystack {
            text: &t,
            blocks: &b,
            global: true,
        };
        let cat = c("cat");
        assert_eq!(
            h.find_alt(&hint(1, 4, "The ", " sat. The cat ra"), &cat, false),
            Some(Found::Exact(12..15))
        );
        assert_eq!(
            h.find_alt(&hint(1, 0, "sat. The ", " ran."), &cat, false),
            Some(Found::Exact(25..28))
        );
        assert_eq!(
            h.find_alt(&hint(0, 3, "A ", "."), &c("dog"), false),
            Some(Found::Exact(37..40))
        );
        assert_eq!(
            h.find_alt(&hint(1, 0, "", ""), &c("The cat sad."), false),
            Some(Found::Fuzzy(8..20))
        );
        assert_eq!(h.find_alt(&hint(1, 0, "", ""), &c("zebra"), false), None);
    }

    #[test]
    fn ambiguous_matches_elsewhere_are_rejected() {
        let body = "x\n\nsame here\n\nsame here\n";
        let (t, b) = (c(body), blocks(body));
        let h = Haystack {
            text: &t,
            blocks: &b,
            global: true,
        };
        let found = h.find_exact(&hint(0, 0, "", ""), &c("same"));
        assert_eq!(found, None);
    }

    #[test]
    fn paragraphs_match_by_block() {
        let body = "One para.\n\nSoftware forgot this. Editors treat writing as a line.\n";
        let (t, b) = (c(body), blocks(body));
        let h = Haystack {
            text: &t,
            blocks: &b,
            global: true,
        };
        let para = Hint {
            block: 1,
            ..Hint::default()
        };
        let edited = c("Software forgot this. Most editors treat writing as a single line.");
        assert_eq!(h.find_alt(&para, &edited, true), Some(Found::Fuzzy(11..65)));
        assert_eq!(h.find_alt(&para, &c("Totally different"), true), None);
    }

    #[test]
    fn ghosts_are_clipped() {
        let t = c("abc ghost text");
        let b = [Block { start: 0, end: 14 }];
        let h = Haystack {
            text: &t,
            blocks: &b,
            global: false,
        };
        assert_eq!(
            h.find_ghost(&hint(0, 2, "", ""), Some(&c("ghost")), 99),
            Some(4..14)
        );
        assert_eq!(h.find_ghost(&hint(0, 4, "", ""), None, 5), Some(4..9));
        assert_eq!(h.find_ghost(&hint(0, 40, "", ""), None, 5), None);
    }
}
