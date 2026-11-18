//! GPUI may break before `?` (to let URLs wrap), stranding a question's `?` on the next row.
//! `rewrap` redoes its wrap keeping closing marks with their word, and may hyphenate.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

pub struct Hyphens<'a> {
    /// Char indices, ascending.
    pub breaks: &'a dyn Fn(&str) -> Rc<[usize]>,
    pub width: Pixels,
    /// Bytes of the word being typed, which is never broken (it would jump at every letter).
    pub keep: Option<Range<usize>>,
}

/// Fewest letters in a breakable word, and on either side of the break.
const MIN_WORD: usize = 6;
const MIN_SIDE: usize = 3;
/// Most consecutive rows that end with a hyphen.
const MAX_RUN: usize = 2;
/// A mono face's space: in Plex every space may double, as it always could.
const MAX_STRETCH: f32 = 0.6;
/// Glyph x is summed in f32 thousands of pixels into a paragraph, and comes out a little
/// differently when the line is shaped in pieces (a ghost, Zen writing's ink): a row that fits
/// the column exactly (a mono face, `column_chars` wide) must fit however it was shaped.
const SLACK: Pixels = px(0.05);

/// The first row is `indent` shorter. Returns, per boundary, whether its row ends hyphenated.
pub fn rewrap(line: &mut WrappedLine, wrap_width: Pixels, hyphens: Option<&Hyphens>, indent: Pixels) -> Vec<bool> {
    let (boundaries, hyphenated) = boundaries(&line.unwrapped_layout, &line.text, wrap_width, hyphens, indent);
    if boundaries[..] != line.wrap_boundaries[..] {
        let layout = WrappedLineLayout {
            unwrapped_layout: line.unwrapped_layout.clone(),
            wrap_boundaries: boundaries.into(),
            wrap_width: Some(wrap_width),
        };
        **line = Arc::new(layout);
    }
    hyphenated
}

/// `hyphenated` is `rewrap`'s answer; spaces before byte `from` (a quote's `> `) don't grow.
/// A space grows by at most `MAX_STRETCH` em; a row that would need more is left as it is.
/// The cap is in em, not in spaces: a book face's space is a third of a mono one's.
pub fn justify(line: &mut WrappedLine, width: Pixels, hyphenated: &[bool], hyphen: Pixels, from: usize, indent: Pixels) {
    let ul = &line.unwrapped_layout;
    let text = &line.text;
    let glyphs: Vec<(usize, char, Pixels)> = ul
        .runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .map(|g| (g.index, text.get(g.index..).and_then(|s| s.chars().next()).unwrap_or('\0'), g.position.x))
        .collect();
    let flat = |b: &WrapBoundary| ul.runs[..b.run_ix].iter().map(|r| r.glyphs.len()).sum::<usize>() + b.glyph_ix;
    let starts: Vec<usize> = std::iter::once(0).chain(line.wrap_boundaries.iter().map(flat)).collect();
    let x_at = |k: usize| glyphs.get(k).map_or(ul.width, |g| g.2);
    let mut shift = vec![px(0.); glyphs.len()];
    let mut acc = px(0.);
    for (r, w) in starts.windows(2).enumerate() {
        let (s, e) = (w[0], w[1]);
        let mut moved = px(0.);
        let first = (s..e).find(|&k| glyphs[k].1 != ' ');
        let last = (s..e).rev().find(|&k| glyphs[k].1 != ' ');
        let spaces: Vec<usize> = match (first, last) {
            (Some(a), Some(z)) => (a..z).filter(|&k| glyphs[k].1 == ' ' && glyphs[k].0 >= from).collect(),
            _ => Vec::new(),
        };
        if let (Some(z), Some(&sp)) = (last, spaces.first()) {
            let used = x_at(z + 1) - x_at(s) + if hyphenated.get(r) == Some(&true) { hyphen } else { px(0.) };
            let row_width = if r == 0 { width - indent } else { width };
            let per = (row_width - used) / spaces.len() as f32;
            let space_w = x_at(sp + 1) - x_at(sp);
            if per > px(0.) && per <= space_w.max(ul.font_size * MAX_STRETCH) {
                for (k, sh) in shift.iter_mut().enumerate().take(e).skip(s) {
                    *sh = acc + moved;
                    if spaces.contains(&k) {
                        moved += per;
                    }
                }
            }
        }
        if moved == px(0.) {
            shift[s..e].fill(acc);
        }
        acc += moved;
    }
    let last = *starts.last().unwrap_or(&0);
    shift[last..].fill(acc);
    if acc == px(0.) {
        return;
    }
    let mut layout = LineLayout { font_size: ul.font_size, width: ul.width, ascent: ul.ascent, descent: ul.descent, runs: ul.runs.clone(), len: ul.len };
    let mut k = 0;
    for run in &mut layout.runs {
        for g in &mut run.glyphs {
            g.position.x += shift[k];
            k += 1;
        }
    }
    layout.width += acc;
    let wrapped = WrappedLineLayout {
        unwrapped_layout: Arc::new(layout),
        wrap_boundaries: line.wrap_boundaries.clone(),
        wrap_width: line.wrap_width,
    };
    **line = Arc::new(wrapped);
}

/// GPUI's algorithm, but for `is_word_char` and hyphenation.
fn boundaries(layout: &LineLayout, text: &str, full_width: Pixels, hyphens: Option<&Hyphens>, indent: Pixels) -> (Vec<WrapBoundary>, Vec<bool>) {
    let mut boundaries = Vec::new();
    let mut hyphenated = Vec::new();
    let mut run_of_hyphens = 0;
    let mut first_non_whitespace = false;
    let mut last_candidate: Option<(WrapBoundary, Pixels)> = None;
    let mut last_boundary = WrapBoundary { run_ix: 0, glyph_ix: 0 };
    let mut last_boundary_ix = 0;
    let mut last_boundary_x = px(0.);
    let mut prev = '\0';
    let glyphs: Vec<Glyph> = layout
        .runs
        .iter()
        .enumerate()
        .flat_map(|(run_ix, run)| {
            run.glyphs.iter().enumerate().map(move |(glyph_ix, g)| {
                let ch = text.get(g.index..).and_then(|s| s.chars().next()).unwrap_or('\0');
                Glyph { boundary: WrapBoundary { run_ix, glyph_ix }, ch, x: g.position.x, byte: g.index }
            })
        })
        .collect();
    let index_of = |b: WrapBoundary| glyphs.iter().position(|g| g.boundary == b).unwrap_or(0);
    for (i, g) in glyphs.iter().enumerate() {
        let (boundary, ch, x) = (g.boundary, g.ch, g.x);
        if ch == '\n' {
            continue;
        }
        let candidate = if is_word_char(ch) { prev == ' ' && ch != ' ' } else { ch != ' ' };
        if candidate && first_non_whitespace {
            last_candidate = Some((boundary, x));
        }
        if ch != ' ' {
            first_non_whitespace = true;
        }
        let next_x = glyphs.get(i + 1).map_or(layout.width, |g| g.x);
        let wrap_width = if boundaries.is_empty() { full_width - indent } else { full_width };
        if next_x - last_boundary_x > wrap_width + SLACK && boundary > last_boundary {
            let broken = hyphens
                .filter(|_| run_of_hyphens < MAX_RUN)
                .and_then(|h| hyphen_break(&glyphs, text, i, last_boundary_ix, last_boundary_x, wrap_width, h));
            if let Some(k) = broken {
                (last_boundary, last_boundary_x, last_boundary_ix) = (glyphs[k].boundary, glyphs[k].x, k);
                last_candidate = None;
                run_of_hyphens += 1;
            } else {
                (last_boundary, last_boundary_x) = last_candidate.take().unwrap_or((boundary, x));
                last_boundary_ix = index_of(last_boundary);
                run_of_hyphens = 0;
            }
            boundaries.push(last_boundary);
            hyphenated.push(broken.is_some());
        }
        prev = ch;
    }
    (boundaries, hyphenated)
}

struct Glyph {
    boundary: WrapBoundary,
    ch: char,
    x: Pixels,
    byte: usize,
}

/// Where to break the word glyph `i` ran past the row's end in, so it and a hyphen fit the row.
fn hyphen_break(glyphs: &[Glyph], text: &str, i: usize, row: usize, row_x: Pixels, width: Pixels, h: &Hyphens) -> Option<usize> {
    let letter = |k: usize| glyphs[k].ch.is_alphabetic();
    let mut k = i;
    if !letter(k) {
        k = k.checked_sub(1).filter(|&k| letter(k))?;
    }
    let mut ws = k;
    while ws > 0 && letter(ws - 1) {
        ws -= 1;
    }
    let mut we = k + 1;
    while we < glyphs.len() && letter(we) {
        we += 1;
    }
    if ws <= row || !matches!(glyphs[ws - 1].ch, ' ' | '«' | '“' | '„' | '"' | '‘' | '(' | '*' | '_') {
        return None;
    }
    let tail = &glyphs[we..];
    let gap = tail.iter().position(|g| g.ch == ' ')?;
    if tail[..gap].iter().any(|g| g.ch.is_alphanumeric() || matches!(g.ch, '-' | '/' | '@' | '\'' | '’' | '`'))
        || !tail[gap..].iter().any(|g| g.ch.is_alphanumeric())
    {
        return None;
    }
    let (b0, b1) = (glyphs[ws].byte, glyphs.get(we).map_or(text.len(), |g| g.byte));
    if h.keep.as_ref().is_some_and(|r| r.start <= b1 && b0 <= r.end) {
        return None;
    }
    let word = &text[b0..b1];
    let n = word.chars().count();
    if n < MIN_WORD || n != we - ws || word.chars().skip(1).any(char::is_uppercase) {
        return None;
    }
    (h.breaks)(word)
        .iter()
        .rev()
        .filter(|&&p| p >= MIN_SIDE && n - p >= MIN_SIDE)
        .map(|&p| ws + p)
        .find(|&k| glyphs[k].x - row_x + h.width <= width + SLACK && k > row)
}

/// GPUI's word characters (`LineWrapper::is_word_char`) and the closing marks it lets a row start with.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '\u{00C0}'..='\u{00FF}' | '\u{0100}'..='\u{017F}' | '\u{0180}'..='\u{024F}')
        || matches!(c, '\u{0400}'..='\u{04FF}')
        || matches!(c, '\u{1E00}'..='\u{1EFF}' | '\u{0300}'..='\u{036F}')
        || matches!(c, '\u{0980}'..='\u{09FF}')
        || matches!(c, '-' | '_' | '.' | '\'' | '’' | '‘' | '$' | '%' | '@' | '#' | '^' | '~' | ',' | '=' | ':' | ';')
        || matches!(c, '!' | ')' | ']' | '}' | '"' | '”' | '»' | '…')
        || c == '⋯'
        || matches!(c, '\u{202F}' | '\u{00A0}' | '\u{2011}')
        // Ours: UAX #14 has no break before these either.
        || matches!(c, '?' | '›')
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;

    use gpui_kit::{FontId, GlyphId, LineLayout, Pixels, ShapedGlyph, ShapedRun, WrappedLine, WrappedLineLayout, point, px};

    use super::{Hyphens, boundaries, is_word_char, justify};

    /// `text` laid out in one run, 10 px a character.
    fn mono(text: &str) -> LineLayout {
        let glyphs = text
            .char_indices()
            .enumerate()
            .map(|(i, (index, _))| ShapedGlyph { id: GlyphId(0), position: point(px(10. * i as f32), px(0.)), index, is_emoji: false })
            .collect::<Vec<_>>();
        let width = px(10. * glyphs.len() as f32);
        LineLayout { font_size: px(10.), width, runs: vec![ShapedRun { font_id: FontId(0), glyphs }], len: text.len(), ..Default::default() }
    }

    fn rows(text: &str, width: Pixels) -> Vec<String> {
        let layout = mono(text);
        let mut starts = vec![0];
        starts.extend(boundaries(&layout, text, width, None, px(0.)).0.iter().map(|b| layout.runs[b.run_ix].glyphs[b.glyph_ix].index));
        starts.push(text.len());
        starts.windows(2).map(|w| text[w[0]..w[1]].to_string()).collect()
    }

    #[test]
    fn an_indented_first_row_is_shorter() {
        let layout = mono("one two three four");
        let text = "one two three four";
        let starts = |indent: f32| {
            let b = boundaries(&layout, text, px(140.), None, px(indent)).0;
            b.iter().map(|b| layout.runs[b.run_ix].glyphs[b.glyph_ix].index).collect::<Vec<_>>()
        };
        assert_eq!(starts(0.), [14]);
        assert_eq!(starts(30.), [8]);
    }

    #[test]
    fn a_question_mark_never_starts_a_row() {
        assert_eq!(rows("это, что это?", px(120.)), ["это, что ", "это?"]);
        assert_eq!(rows("in the end?", px(100.)), ["in the ", "end?"]);
        assert_eq!(rows("a «quote›", px(80.)), ["a ", "«quote›"]);
    }

    #[test]
    fn other_breaks_are_gpuis() {
        assert_eq!(rows("one two three", px(80.)), ["one two ", "three"]);
        assert_eq!(rows("a/b/c/d/e/f", px(50.)), ["a/b/c", "/d/e", "/f"]);
        assert_eq!(rows("abcdefghij", px(40.)), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn closing_marks_stay_with_their_word() {
        for c in ['?', '!', '.', ',', '»', '›', ')', 'ж', 'é'] {
            assert!(is_word_char(c), "{c}");
        }
        for c in ['/', '(', '«', '—', ' '] {
            assert!(!is_word_char(c), "{c}");
        }
    }

    fn split(word: &str) -> Rc<[usize]> {
        let v = |c: char| "aeiouаеиоуыэюя".contains(c.to_ascii_lowercase());
        let cs: Vec<char> = word.chars().collect();
        (1..cs.len()).filter(|&i| v(cs[i - 1]) && !v(cs[i])).collect::<Vec<_>>().into()
    }

    /// The rows `text` wraps into at `width` with hyphenation; a broken row ends with `-`.
    fn hyphenated(text: &str, width: Pixels, keep: Option<std::ops::Range<usize>>) -> Vec<String> {
        let layout = mono(text);
        let h = Hyphens { breaks: &split, width: px(10.), keep };
        let (b, hy) = boundaries(&layout, text, width, Some(&h), px(0.));
        let mut starts = vec![0];
        starts.extend(b.iter().map(|b| layout.runs[b.run_ix].glyphs[b.glyph_ix].index));
        starts.push(text.len());
        starts.windows(2).enumerate().map(|(i, w)| format!("{}{}", &text[w[0]..w[1]], if hy.get(i) == Some(&true) { "-" } else { "" })).collect()
    }

    #[test]
    fn long_words_break_with_a_hyphen() {
        assert_eq!(hyphenated("walked in hesitation today", px(160.), None), ["walked in hesi-", "tation today"]);
        assert_eq!(rows("walked in hesitation today", px(160.)), ["walked in ", "hesitation today"]);
    }

    #[test]
    fn some_words_are_never_broken() {
        assert_eq!(hyphenated("walked in hesitation", px(160.), None), ["walked in ", "hesitation"]);
        assert_eq!(hyphenated("walked in a lemon cake", px(140.), None), ["walked in a ", "lemon cake"]);
        assert_eq!(hyphenated("walked in McDonaldson today", px(180.), None), ["walked in ", "McDonaldson today"]);
        assert_eq!(hyphenated("walked in well-tempered x", px(180.), None), ["walked in ", "well-tempered x"]);
        assert_eq!(hyphenated("walked in example.org/a x", px(180.), None), ["walked in ", "example.org/a x"]);
        assert_eq!(hyphenated("walked in hesitation2 x", px(180.), None), ["walked in ", "hesitation2 x"]);
        assert_eq!(hyphenated("walked in hesitation today", px(160.), Some(10..20)), ["walked in ", "hesitation today"]);
    }

    #[test]
    fn at_most_two_hyphens_in_a_row() {
        let rows = hyphenated("ab hesitation ab hesitation ab hesitation ab hesitation x", px(80.), None);
        assert!(!rows.windows(3).any(|w| w.iter().all(|r| r.ends_with('-'))), "{rows:?}");
    }

    fn justified(text: &str, width: Pixels, from: usize) -> (Vec<f32>, Vec<String>) {
        let layout = mono(text);
        let (b, hy) = boundaries(&layout, text, width, None, px(0.));
        let mut line = WrappedLine::default();
        *line = Arc::new(WrappedLineLayout { unwrapped_layout: Arc::new(layout), wrap_boundaries: b.into(), wrap_width: Some(width) });
        line.text = text.to_string().into();
        justify(&mut line, width, &hy, px(10.), from, px(0.));
        let xs = line.unwrapped_layout.runs[0].glyphs.iter().map(|g| f32::from(g.position.x)).collect();
        (xs, rows(text, width))
    }

    #[test]
    fn justified_rows_reach_the_edge() {
        let (xs, rows) = justified("aa bb cc ddddd", px(100.), 0);
        assert_eq!(rows, ["aa bb cc ", "ddddd"]);
        assert_eq!(&xs[..8], &[0., 10., 20., 40., 50., 60., 80., 90.]);
        assert_eq!(xs[10] - xs[9], 10.);
    }

    #[test]
    fn rows_that_would_gape_stay_ragged() {
        let (xs, _) = justified("aa b cccccccccc x", px(120.), 0);
        assert_eq!(&xs[..4], &[0., 10., 20., 30.]);
    }

    #[test]
    fn a_quotes_marker_doesnt_stretch() {
        let (xs, _) = justified("> aa bb cc ddddddddd", px(120.), 2);
        assert_eq!(xs[2], 20., "the space after `>` stays");
        assert_eq!(xs[5], 60., "the spaces between words grow");
    }
}
