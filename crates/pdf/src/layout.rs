//! Lines and pages: where every glyph of a book goes. TextKit's rules, measured on the Mac, so one
//! style gives the same page: a line is as tall as the style says, its baseline sits `line height
//! minus round(descender)` below its top and the font's line gap (if it has one) is added under
//! it; a paragraph keeps its space before at the top of a page, except at the start of a chapter.
//! Where the Mac has no rule, the pages get a typesetter's: no heading ends a page, and no
//! paragraph leaves one line alone at the foot of a page or the head of the next.

use std::collections::BTreeMap;
use std::ops::Range;

use export::{Book, Options};
use unicode_linebreak::BreakOpportunity;

use crate::compose::{self, Align, Para, ParaLook, RunLook};
use crate::font::{Font, Fonts};
use crate::shape::{Shaped, Shaper};

/// The face code is set in: Menlo on the Mac; a system's monospace, else JetBrains Mono.
pub const CODE_FAMILY: &str = "Menlo";

const EPS: f32 = 1e-3;
/// Lines in a row that may end in a hyphen.
const HYPHENS_IN_A_ROW: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ink {
    Text,
    /// A quotation in a book: fainter.
    Faded,
}

impl Ink {
    pub fn rgb(self) -> [f32; 3] {
        match self {
            Ink::Text => [0.08, 0.07, 0.09],
            Ink::Faded => [0.28, 0.26, 0.30],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub font: Font,
    pub size: f32,
    pub ink: Ink,
    pub underline: bool,
    pub strike: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub glyph: u16,
    /// On its page, in points.
    pub x: f32,
    /// On its page, in points, up from the bottom edge.
    pub y: f32,
    pub advance: f32,
}

/// A glyph on its line: from the line's left edge, and above its baseline.
#[derive(Clone, Copy, Debug)]
struct Set {
    glyph: u16,
    x: f32,
    rise: f32,
    advance: f32,
}

/// Glyphs of one style along one baseline, in points, `y` up from the page's bottom edge.
#[derive(Clone, Debug)]
pub struct Run {
    pub style: Style,
    pub glyphs: Vec<Placed>,
}

pub enum Item {
    Text(Run),
    /// An underline or a strikethrough.
    Rule { x1: f32, y: f32, x2: f32, width: f32, ink: Ink },
}

#[derive(Default)]
pub struct Page {
    pub items: Vec<Item>,
}

pub struct Document {
    /// In points.
    pub width: f32,
    pub height: f32,
    pub pages: Vec<Page>,
    /// For each face, the text each of its glyphs shows.
    pub text: BTreeMap<Font, BTreeMap<u16, String>>,
}

/// A line of text on a page: its baseline, where it starts and ends, and what it says.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub baseline: f32,
    pub x1: f32,
    pub x2: f32,
    pub text: String,
}

/// A line being gathered: its baseline, where it starts and ends, and its glyphs' text by place.
type Gathered = (f32, f32, f32, Vec<(f32, String)>);

impl Document {
    /// The glyphs on a page as lines of text, top to bottom and left to right, as a reader
    /// selecting them would get them.
    pub fn rows(&self, page: usize) -> Vec<Row> {
        let mut lines: Vec<Gathered> = Vec::new();
        for item in self.pages.get(page).into_iter().flat_map(|p| &p.items) {
            let Item::Text(run) = item else { continue };
            for g in &run.glyphs {
                let text = self.text.get(&run.style.font).and_then(|m| m.get(&g.glyph)).cloned().unwrap_or_default();
                let text = if g.advance == 0. && text == "-" { String::new() } else { text };
                match lines.iter_mut().find(|l| (l.0 - g.y).abs() < 0.5) {
                    Some(l) => {
                        l.1 = l.1.min(g.x);
                        l.2 = l.2.max(g.x + g.advance);
                        l.3.push((g.x, text));
                    }
                    None => lines.push((g.y, g.x, g.x + g.advance, vec![(g.x, text)])),
                }
            }
        }
        lines.sort_by(|a, b| b.0.total_cmp(&a.0));
        lines
            .into_iter()
            .map(|(y, x1, x2, mut words)| {
                words.sort_by(|a, b| a.0.total_cmp(&b.0));
                Row { baseline: y, x1, x2, text: words.into_iter().map(|w| w.1).collect() }
            })
            .collect()
    }
}

/// A paragraph broken into lines.
struct Laid {
    look: ParaLook,
    keep: bool,
    lines: Vec<Line>,
}

struct Line {
    runs: Vec<(Style, Vec<Set>, Range<usize>)>,
    /// How far the text reaches, trailing spaces aside.
    used: f32,
    /// Where the line starts, in from the margin.
    indent: f32,
    descent: f32,
    leading: f32,
}

/// Where a line ends.
struct Break {
    at: usize,
    last: bool,
    /// At a line break of the text's own.
    forced: bool,
    hyphen: bool,
}

struct Setting<'a> {
    fonts: &'a Fonts,
    shaper: Shaper<'a>,
    family: String,
    heading: Option<String>,
    /// The page, in points: its width and height, the margin, the text area's width and height.
    page: (f32, f32, f32, f32, f32),
    text: BTreeMap<Font, BTreeMap<u16, String>>,
}

/// Where a label goes, from the left edge of its page.
enum At {
    Left(f32),
    Center,
    /// Centered, unless wider than this, when it starts at the margin.
    CenterWithin(f32),
    Right(f32),
}

impl<'a> Setting<'a> {
    fn new(fonts: &'a Fonts, language: Option<&str>, text: &export::Text, page: (f32, f32, f32, f32, f32)) -> Setting<'a> {
        Setting { fonts, shaper: Shaper::new(fonts, language), family: text.font.clone(), heading: text.heading_font.clone(), page, text: BTreeMap::new() }
    }

    fn style(&self, look: &RunLook) -> Option<Style> {
        let family = if look.mono { CODE_FAMILY } else if look.heading { self.heading.as_deref().unwrap_or(&self.family) } else { &self.family };
        let font = self.fonts.resolve(family, look.bold, look.italic)?;
        Some(Style { font, size: look.size as f32, ink: if look.grey { Ink::Faded } else { Ink::Text }, underline: look.underline, strike: look.strike })
    }

    /// A style's hyphen: its glyph and how far it reaches.
    fn hyphen(&mut self, style: Style) -> (u16, f32) {
        let shaped = self.shaper.shape("-", 0, style.font, style.size, 0.);
        let glyph = shaped.iter().flat_map(|r| &r.glyphs).find(|g| g.len == 1).map_or((0, style.size * 0.3), |g| (g.id, g.advance));
        self.text.entry(style.font).or_default().insert(glyph.0, "-".into());
        glyph
    }

    /// The natural height of a line in a face at a size: ascent, descent and line gap.
    fn line_height(&self, family: &str, size: f32, bold: bool, italic: bool) -> f32 {
        let Some(font) = self.fonts.resolve(family, bold, italic) else { return size };
        let scale = size / font.rec.face.units_per_em() as f32;
        (font.rec.face.ascender() - font.rec.face.descender() + font.rec.face.line_gap()) as f32 * scale
    }

    /// One line with nothing after it on its line, `top` from the page's top edge to the top of
    /// its line. Returns how wide and tall it is.
    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, page: &mut Page, text: &str, family: &str, size: f32, bold: bool, italic: bool, at: At, top: f32) -> (f32, f32) {
        let Some(font) = self.fonts.resolve(family, bold, italic) else { return (0., 0.) };
        let runs = self.shaper.shape(text, 0, font, size, 0.);
        let width = runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.advance)).sum::<f32>();
        let x = match at {
            At::Left(x) => x,
            At::Center => (self.page.0 - width) / 2.,
            At::CenterWithin(cw) => (self.page.0 - width.min(cw)) / 2.,
            At::Right(right) => right - width,
        };
        // The baseline is the line's top, from the page's top edge, plus its ascent.
        let baseline = self.page.1 - top - font.rec.face.ascender() as f32 * size / font.rec.face.units_per_em() as f32;
        let mut items: Vec<(Style, Vec<Placed>)> = Vec::new();
        let mut pen = x;
        for run in &runs {
            for g in &run.glyphs {
                self.text.entry(run.font).or_default().insert(g.id, text[g.cluster as usize..g.cluster as usize + g.len as usize].to_string());
                let placed = Placed { glyph: g.id, x: pen + g.x_offset, y: baseline + g.y_offset, advance: g.advance };
                match items.last_mut() {
                    Some((style, glyphs)) if style.font == run.font && (glyphs[0].y - placed.y).abs() < EPS => glyphs.push(placed),
                    _ => items.push((Style { font: run.font, size, ink: Ink::Text, underline: false, strike: false }, vec![placed])),
                }
                pen += g.advance;
            }
        }
        page.items.extend(items.into_iter().map(|(style, glyphs)| Item::Text(Run { style, glyphs })));
        (width, self.line_height(family, size, bold, italic))
    }
}

/// How far the text from `s` to `e` reaches, trailing spaces aside.
fn width(runs: &[Shaped], s: usize, e: usize) -> f32 {
    runs.iter()
        .map(|run| {
            let a = run.glyphs.partition_point(|g| (g.cluster as usize) < s);
            let b = run.glyphs.partition_point(|g| (g.cluster as usize) < e);
            run.glyphs[a..b].iter().map(|g| g.advance).sum::<f32>()
        })
        .sum()
}

/// `e` without the spaces and line breaks at its end.
fn trimmed(text: &str, mut e: usize) -> usize {
    while let Some(c) = text[..e].chars().next_back() {
        if !matches!(c, ' ' | '\u{2028}') {
            break;
        }
        e -= c.len_utf8();
    }
    e
}

/// The style of the glyph showing the character at `at`.
fn style_at(runs: &[Shaped], styles: &[Style], at: usize) -> Option<Style> {
    runs.iter().zip(styles).find(|(r, _)| r.glyphs.iter().any(|g| (g.cluster as usize) <= at && at < g.cluster as usize + g.len as usize)).map(|(_, s)| *s)
}

/// A shape per stretch of text between the line breaks of its own.
fn shape_span(st: &mut Setting, text: &str, from: usize, till: usize, style: Style, runs: &mut Vec<Shaped>, styles: &mut Vec<Style>) {
    let mut at = from;
    for end in text[from..till].match_indices('\u{2028}').map(|(i, _)| from + i).chain(std::iter::once(till)) {
        if end > at {
            let had = runs.len();
            runs.extend(st.shaper.shape(&text[at..end], at, style.font, style.size, 0.));
            styles.extend(std::iter::repeat_n(style, runs.len() - had));
        }
        at = end + '\u{2028}'.len_utf8();
    }
}

/// Breaks a paragraph into lines, each with its glyphs placed from the line's left edge.
fn lay(st: &mut Setting, para: &Para, cw: f32, hyphenate: &dyn Fn(&str) -> Vec<usize>) -> Laid {
    let look = para.look;
    let mut text = String::new();
    let mut spans: Vec<(usize, Style)> = Vec::new();
    for (t, piece) in &para.pieces {
        if let Some(style) = st.style(piece) {
            spans.push((text.len(), style));
            text.push_str(&t.replace('\t', "    "));
        }
    }
    let mut runs = Vec::new();
    let mut styles = Vec::new();
    for (i, &(from, style)) in spans.iter().enumerate() {
        shape_span(st, &text, from, spans.get(i + 1).map_or(text.len(), |s| s.0), style, &mut runs, &mut styles);
    }
    // Where a line may end: by the line-breaking rules, at the start of a glyph's text or of a break.
    let mut starts: Vec<u32> = runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.cluster)).collect();
    starts.sort_unstable();
    let mut opportunities: Vec<(usize, bool)> = unicode_linebreak::linebreaks(&text)
        .filter(|(at, _)| *at > 0 && (*at == text.len() || starts.binary_search(&(*at as u32)).is_ok() || text[*at..].starts_with('\u{2028}')))
        .map(|(at, o)| (at, o == BreakOpportunity::Mandatory))
        .collect();
    if opportunities.last().is_none_or(|(at, _)| *at != text.len()) {
        opportunities.push((text.len(), true));
    }

    let mut lines = Vec::new();
    let mut s = 0;
    let mut first = true;
    let mut hyphens = 0;
    while s < text.len() || lines.is_empty() {
        let indent = if first { if look.first_indent != 0. { look.first_indent } else { look.head_indent } } else { look.head_indent } as f32;
        let avail = (cw - indent).max(1.);
        // The furthest end that fits, and the first that is too far.
        let mut fits: Option<(usize, bool)> = None;
        let mut stops = None;
        for &(at, mandatory) in &opportunities[opportunities.partition_point(|(at, _)| *at <= s)..] {
            if width(&runs, s, at) <= avail + EPS {
                fits = Some((at, mandatory));
                if mandatory {
                    break;
                }
            } else {
                stops = Some(at);
                break;
            }
        }
        let end = match (fits, stops) {
            (Some((at, mandatory)), None) => Break { at, last: at == text.len(), forced: mandatory, hyphen: false },
            _ => {
                // What is left is one piece, from where a line may end to where the next one would.
                let from = fits.map_or(s, |(at, _)| at);
                let till = stops.unwrap_or(text.len());
                let word = from + text[from..till].find(|c: char| !c.is_alphabetic()).unwrap_or(till - from);
                let letters: Vec<usize> = text[from..word].char_indices().map(|(i, _)| from + i).collect();
                let hyphen_width = style_at(&runs, &styles, from).map_or(0., |style| st.hyphen(style).1);
                let mut point = None;
                // Not a word too short to spare letters, a name, nor the last word of a paragraph.
                if look.hyphenate && hyphens < HYPHENS_IN_A_ROW && !fits.is_some_and(|(_, m)| m) && till < text.len() && letters.len() >= 6 && !text[from..word].chars().skip(1).any(char::is_uppercase) {
                    for p in hyphenate(&text[from..word]) {
                        // Two letters stay before the break, three after it.
                        if p >= 2 && letters.len() - p >= 3 && width(&runs, s, letters[p]) + hyphen_width <= avail + EPS {
                            point = Some(letters[p]);
                        }
                    }
                }
                match point {
                    Some(at) => Break { at, last: false, forced: false, hyphen: true },
                    None if from > s => Break { at: from, last: false, forced: false, hyphen: false },
                    None => Break { at: emergency(&runs, &text, s, avail), last: false, forced: false, hyphen: false },
                }
            }
        };
        let till = trimmed(&text, end.at);
        let natural = width(&runs, s, till);
        let stretch = look.align == Align::Justified && !end.last && !end.forced && natural < avail;
        // Justified lines take up the slack in their spaces; a line with none, between its glyphs.
        let mut spaces = 0usize;
        let mut count = 0usize;
        for g in runs.iter().flat_map(|r| &r.glyphs) {
            if (g.cluster as usize) >= s && (g.cluster as usize) < till {
                count += 1;
                spaces += usize::from(text[g.cluster as usize..].starts_with(' '));
            }
        }
        let slack = avail - natural;
        let (extra, gap) = if !stretch { (0., 0.) } else if spaces > 0 { (slack / spaces as f32, 0.) } else if count > 1 { (0., slack / (count - 1) as f32) } else { (0., 0.) };
        let mut x = 0.;
        let mut seen = 0;
        let (mut descent, mut leading) = (0f32, 0f32);
        let mut placed: Vec<(Style, Vec<Set>, Range<usize>)> = Vec::new();
        for (run, style) in runs.iter().zip(&styles) {
            let a = run.glyphs.partition_point(|g| (g.cluster as usize) < s);
            let b = run.glyphs.partition_point(|g| (g.cluster as usize) < till);
            if a == b {
                continue;
            }
            let scale = style.size / run.font.rec.face.units_per_em() as f32;
            descent = descent.max(-run.font.rec.face.descender() as f32 * scale);
            leading = leading.max(run.font.rec.face.line_gap() as f32 * scale);
            let mut line_run = Vec::with_capacity(b - a);
            for g in &run.glyphs[a..b] {
                let at = g.cluster as usize;
                st.text.entry(run.font).or_default().insert(g.id, text[at..at + g.len as usize].to_string());
                let advance = g.advance + if text[at..].starts_with(' ') { extra } else { 0. };
                line_run.push(Set { glyph: g.id, x: x + g.x_offset, rise: g.y_offset, advance });
                x += advance;
                seen += 1;
                if gap > 0. && seen < count {
                    x += gap;
                }
            }
            let last = run.glyphs[b - 1];
            placed.push((*style, line_run, run.glyphs[a].cluster as usize..last.cluster as usize + last.len as usize));
        }
        if end.hyphen {
            let style = style_at(&runs, &styles, end.at - 1).unwrap_or(spans[0].1);
            let (glyph, advance) = st.hyphen(style);
            let hyphen = Set { glyph, x, rise: 0., advance };
            match placed.last_mut() {
                Some((last, glyphs, _)) if *last == style => glyphs.push(hyphen),
                _ => placed.push((style, vec![hyphen], end.at - 1..end.at)),
            }
            x += advance;
        }
        if placed.is_empty() {
            // A line with nothing on it still has the paragraph's height.
            if let Some((_, style)) = spans.first() {
                let scale = style.size / style.font.rec.face.units_per_em() as f32;
                descent = -style.font.rec.face.descender() as f32 * scale;
                leading = style.font.rec.face.line_gap() as f32 * scale;
            }
        }
        lines.push(Line { runs: placed, used: x, indent, descent, leading });
        if end.last {
            break;
        }
        hyphens = if end.hyphen { hyphens + 1 } else { 0 };
        first = end.forced;
        let mut next = end.at;
        while text[next..].starts_with(' ') {
            next += 1;
        }
        s = if next > s { next } else { text[s..].chars().next().map_or(text.len(), |c| s + c.len_utf8()) };
    }
    Laid { look, keep: para.keep, lines }
}

/// A line that nothing can break gets one anyway: as many glyphs as fit, and never none.
fn emergency(runs: &[Shaped], text: &str, s: usize, avail: f32) -> usize {
    let mut at = s;
    for g in runs.iter().flat_map(|r| &r.glyphs) {
        let end = g.cluster as usize + g.len as usize;
        if g.len == 0 || (g.cluster as usize) < s || end <= s {
            continue;
        }
        // The first glyph stays however wide it is; a later one only while the line still fits.
        if at > s && width(runs, s, end) > avail + EPS {
            break;
        }
        at = end;
    }
    if at == s { text[s..].chars().next().map_or(s, |c| s + c.len_utf8()) } else { at }
}

/// How many of a paragraph's lines, from its `from`th, fit with the first one's top `top` down a
/// text area `height` tall.
fn fits(blocks: &[Laid], i: usize, from: usize, top: f32, height: f32, page_empty: bool) -> usize {
    let block = &blocks[i];
    let left = block.lines.len() - from;
    let mut n = (0..=left).take_while(|n| top + tall_to(blocks, i, from, from + *n) <= height + EPS).last().unwrap_or(0);
    // Not one line of a paragraph alone at the foot of a page, nor at the head of the next.
    if left >= 2 && n == left - 1 {
        n -= 1;
    }
    if left >= 2 && from == 0 && n == 1 {
        n = 0;
    }
    // A heading stays with the first two lines of what follows it.
    if from == 0 && n > 0 && block.keep {
        let mut used = top + tall_to(blocks, i, 0, n) + block.look.after as f32;
        let mut next = i + 1;
        while next < blocks.len() && blocks[next].keep {
            used += blocks[next].look.before as f32 + tall_to(blocks, next, 0, blocks[next].lines.len()) + blocks[next].look.after as f32;
            next += 1;
        }
        if let Some(follow) = blocks.get(next) {
            used += follow.look.before as f32 + tall_to(blocks, next, 0, follow.lines.len().min(2));
        }
        if used > height + EPS {
            n = 0;
        }
    }
    // An empty page takes a line whatever else: a page must never stay blank for a rule.
    if page_empty {
        n = n.max(usize::from(left > 0));
    }
    n
}

/// How far a paragraph's lines `from` (inclusive) to `to` reach from the top of the first.
fn tall_to(blocks: &[Laid], i: usize, from: usize, to: usize) -> f32 {
    blocks[i].lines[from..to].iter().map(|l| blocks[i].look.line as f32 + l.leading).sum()
}

/// Each page's lines as (paragraph, line, top), the top so far down the text area, in points.
fn paginate(blocks: &[Laid], drop: f32, height: f32) -> Vec<Vec<(usize, usize, f32)>> {
    let mut pages: Vec<Vec<(usize, usize, f32)>> = vec![Vec::new()];
    let mut y = drop;
    // The paragraph being placed, and the first of its lines not on a page yet.
    let (mut i, mut from) = (0, 0);
    while let Some(block) = blocks.get(i) {
        if from == block.lines.len() {
            y += block.look.after as f32;
            i += 1;
            from = 0;
            continue;
        }
        // The space before a paragraph stays at the top of a page, except at a chapter's start.
        let before = if from == 0 && i > 0 { block.look.before as f32 } else { 0. };
        let empty = pages.last().is_some_and(Vec::is_empty);
        let n = fits(blocks, i, from, y + before, height, empty);
        if n == 0 && !empty {
            pages.push(Vec::new());
            y = 0.;
            continue;
        }
        let mut top = y + before;
        for li in from..from + n {
            pages.last_mut().expect("always a page").push((i, li, top));
            top += block.look.line as f32 + block.lines[li].leading;
        }
        y = top;
        from += n;
    }
    pages
}

/// The book's pages: every glyph placed, in points, `y` up from the page's bottom edge.
pub fn typeset(book: &Book, o: &Options, fonts: &Fonts, hyphenate: &dyn Fn(&str) -> Vec<usize>) -> Document {
    let (w, h) = o.paper.size();
    let m = o.paper.margin();
    let (cw, ch) = (w - 2. * m, h - 2. * m);
    let mut st = Setting::new(fonts, book.language.as_deref(), &o.text, (w, h, m, cw, ch));
    let size = o.text.size;
    let family = st.family.clone();
    let manuscript = o.style == export::Style::Manuscript;
    let mut doc = Document { width: w, height: h, pages: Vec::new(), text: BTreeMap::new() };
    let mut number = 0usize;

    if o.title_page {
        let mut page = Page::default();
        let (title, author) = (book.title.trim(), book.author.trim());
        if manuscript {
            st.label(&mut page, author, &family, size, false, false, At::Left(m), m);
            st.label(&mut page, &format!("about {} words", group(book.rounded_words())), &family, size, false, false, At::Right(w - m), m);
            st.label(&mut page, &title.to_uppercase(), &family, size, false, false, At::Center, h / 2. - size * 2.);
            if !author.is_empty() {
                st.label(&mut page, &format!("by {author}"), &family, size, false, false, At::Center, h / 2.);
            }
        } else {
            let heading = st.heading.clone().unwrap_or_else(|| family.clone());
            let (_, title_height) = st.label(&mut page, title, &heading, size * 2.2, false, o.style == export::Style::Book, At::CenterWithin(cw), m + ch / 3.);
            if !author.is_empty() {
                st.label(&mut page, author, &family, size * 1.1, false, true, At::Center, m + ch / 3. + title_height + size * 1.5);
            }
        }
        doc.pages.push(page);
        number += 1;
    }

    for c in book.blocks(o.chapter_titles) {
        if let Some(part) = &c.part {
            let mut page = Page::default();
            st.label(&mut page, &part.trim().to_uppercase(), &family, size, false, false, At::Center, m + ch / 3.);
            doc.pages.push(page);
            number += 1;
        }
        if c.blocks.is_empty() {
            continue;
        }
        let laid: Vec<Laid> = compose::compose(&c.blocks, o).into_iter().map(|p| lay(&mut st, &p, cw, hyphenate)).collect();
        let starts_with_heading = matches!(c.blocks.first(), Some(export::Block::Heading { .. }));
        let drop = (ch * compose::opening_drop(o, starts_with_heading) as f32).round();
        for lines in paginate(&laid, drop, ch) {
            let mut page = Page::default();
            for (i, li, top) in lines {
                let (block, line) = (&laid[i], &laid[i].lines[li]);
                let baseline = h - m - top - (block.look.line as f32 - line.descent.round());
                let shift = if block.look.align == Align::Center { (cw - line.indent - line.used) / 2. } else { 0. };
                let from = m + line.indent + shift;
                let mut items: Vec<(Style, f32, Vec<Placed>)> = Vec::new();
                for (style, glyphs, _) in &line.runs {
                    // Raised glyphs get a baseline of their own.
                    let mut split: Vec<(f32, Vec<Set>)> = Vec::new();
                    for g in glyphs {
                        match split.last_mut() {
                            Some((rise, set)) if (*rise - g.rise).abs() < EPS => set.push(*g),
                            _ => split.push((g.rise, vec![*g])),
                        }
                    }
                    for (rise, set) in split {
                        let y = baseline + rise;
                        let glyphs: Vec<Placed> = set.iter().map(|g| Placed { glyph: g.glyph, x: from + g.x, y, advance: g.advance }).collect();
                        match items.last_mut() {
                            Some((last, at, run)) if *last == *style && (*at - y).abs() < EPS => run.extend(glyphs),
                            _ => items.push((*style, y, glyphs)),
                        }
                    }
                    if style.underline || style.strike {
                        if let (Some(first), Some(last)) = (glyphs.first(), glyphs.last()) {
                            let face = &style.font.rec.face;
                            let scale = style.size / face.units_per_em() as f32;
                            // A face without the measures gets a usual one: a fifteenth of an em under the baseline, or a third above it.
                            let (position, thickness) = if style.underline { face.underline_metrics().map_or((-150, 60), |m| (m.position, m.thickness)) } else { face.strikeout_metrics().map_or((300, 60), |m| (m.position, m.thickness)) };
                            page.items.push(Item::Rule { x1: from + first.x, y: baseline + position as f32 * scale, x2: from + last.x + last.advance, width: (thickness as f32 * scale).max(0.4), ink: style.ink });
                        }
                    }
                }
                page.items.extend(items.into_iter().map(|(style, _, glyphs)| Item::Text(Run { style, glyphs })));
            }
            // The page number: a manuscript's running head, otherwise a number under the page.
            if o.page_numbers {
                if manuscript {
                    let title = book.title.trim().to_uppercase();
                    let key: Vec<&str> = title.split_whitespace().take(3).collect();
                    let head = [book.surname().to_string(), key.join(" "), (number + 1).to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" / ");
                    let height = st.line_height(&family, size, false, false);
                    st.label(&mut page, &head, &family, size, false, false, At::Right(w - m), m / 2. - height / 2.);
                } else {
                    let height = st.line_height(&family, size * 0.85, false, false);
                    st.label(&mut page, &(number + 1).to_string(), &family, size * 0.85, false, false, At::Center, h - m / 2. - height / 2.);
                }
            }
            doc.pages.push(page);
            number += 1;
        }
    }
    if doc.pages.is_empty() {
        doc.pages.push(Page::default());
    }
    doc.text = st.text;
    doc
}

fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Document, typeset};
    use crate::font::tests::bundled;
    use export::{Chapter, Options, Paper, Style, Text};

    const PARA: &str = "On an exceptionally hot evening early in July a young man came out of the garret in which he lodged in S. Place and walked slowly, as though in hesitation, towards K. bridge.";

    /// The three styles in the app's own faces, so the pages are the same on every system.
    fn opts(style: Style) -> Options {
        let (text, paper) = match style {
            Style::Manuscript => (Text { font: "Literata".into(), ..Text::manuscript() }, Paper::Letter),
            Style::Book => (Text { font: "Literata".into(), ..Text::book(Paper::A5) }, Paper::A5),
            Style::Page => (Text::page("IBM Plex Mono", 19., 1.8, true, true, None), Paper::A4),
        };
        let mut o = Options::new(style, text, paper);
        o.page_numbers = false;
        o.title_page = false;
        o
    }

    fn book(markdown: &str) -> export::Book {
        export::Book { title: "Crime and Punishment".into(), author: "Fyodor Dostoevsky".into(), language: Some("en".into()), chapters: vec![Chapter { markdown: markdown.into(), ..Default::default() }] }
    }

    fn pages(markdown: &str, o: &Options) -> Document {
        typeset(&book(markdown), o, &bundled(), &|_| Vec::new())
    }

    /// The margins of a book's page, in points from its left edge.
    const EDGES: (f32, f32) = (54., 419.53 - 54.);

    #[test]
    fn justified_lines_fill_the_measure_and_the_last_one_is_left_alone() {
        let doc = pages(&format!("{PARA}\n"), &opts(Style::Book));
        let rows = doc.rows(0);
        assert!(rows.len() >= 4, "{:?}", rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>());
        for row in &rows[..rows.len() - 1] {
            assert!((row.x2 - EDGES.1).abs() < 0.5, "a justified line ends at the margin: {row:?}");
        }
        assert!(rows[rows.len() - 1].x2 < EDGES.1 - 20., "the last line is ragged: {:?}", rows[rows.len() - 1]);
        assert!((rows[0].x1 - EDGES.0).abs() < 0.5, "a chapter opens flush: {:?}", rows[0]);
        assert!((rows[1].x1 - EDGES.0).abs() < 0.5, "and so do its other lines: {:?}", rows[1]);
        assert!(rows[0].text.starts_with("ON AN EXCEPTIONALLY HOT evening"), "four words open a book in capitals: {}", rows[0].text);
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<String>().replace(' ', "").to_lowercase(), PARA.replace(' ', "").to_lowercase());

        let doc = pages(&format!("{PARA}\n\n{PARA}\n"), &opts(Style::Book));
        let second = doc.rows(0).into_iter().find(|r| r.text.starts_with("On an")).unwrap();
        assert!((second.x1 - (EDGES.0 + 16.5)).abs() < 0.5, "the next paragraph's first line is indented a line's length and a half: {second:?}");
    }

    #[test]
    fn a_line_break_of_the_texts_own_leaves_a_justified_line_short() {
        let doc = pages(&format!("AAAA\u{2028}{PARA}"), &opts(Style::Book));
        let rows = doc.rows(0);
        assert_eq!(rows[0].text.trim(), "AAAA");
        assert!(rows[0].x2 < EDGES.0 + 60., "not stretched to the margin: {:?}", rows[0]);
    }

    #[test]
    fn a_word_longer_than_a_line_is_broken_and_stretched() {
        let doc = pages(&format!("{}\n", "A".repeat(60)), &opts(Style::Book));
        let rows = doc.rows(0);
        assert_eq!(rows.len(), 2, "{} lines for sixty wide letters over a 300 pt line", rows.len());
        assert!((rows[0].x2 - EDGES.1).abs() < 0.5, "the first line is letter-spaced to the margin: {:?}", rows[0]);
        assert_eq!(rows[0].text.len() + rows[1].text.trim().len(), 60);
    }

    #[test]
    fn lines_step_by_the_styles_line_height() {
        let doc = pages(&format!("{PARA}\n"), &opts(Style::Book));
        let rows = doc.rows(0);
        for pair in rows.windows(2) {
            assert!((pair[0].baseline - pair[1].baseline - 14.85).abs() < 1e-3, "{:?}", pair);
        }
    }

    #[test]
    fn a_chapter_opens_lower_in_a_book_and_a_manuscript() {
        let (w, h) = (419.53f32, 595.28f32);
        let ch = h - 108.;
        let doc = pages(&format!("# The garret\n\n{PARA}\n"), &opts(Style::Book));
        let heading = &doc.rows(0)[0];
        assert_eq!(heading.text, "The garret");
        let (x, y) = ((heading.x1 + heading.x2) / 2., heading.baseline);
        assert!((x - w / 2.).abs() < 1., "a heading is centred: {heading:?}");
        // The heading's line is 11 × 1.75 × 1.25 tall; its baseline sits at line height minus the rounded descender.
        assert!((y - (h - 54. - 0.2 * ch - (11. * 1.75 * 1.25 - 6.))).abs() < 0.5, "{heading:?}");

        let h = 792f32;
        let doc = pages(&format!("# The garret\n\n{PARA}\n\n{PARA}\n"), &opts(Style::Manuscript));
        let heading = &doc.rows(0)[0];
        assert!((heading.baseline - (h - 72. - (h - 144.) / 3. - (24. - 4.))).abs() < 0.5, "{heading:?}");
        assert!((doc.rows(0)[1].x1 - 72.).abs() < 0.5, "the paragraph after a heading opens flush: {:?}", doc.rows(0)[1]);
        let second = doc.rows(0).into_iter().filter(|r| r.text.starts_with("On an")).nth(1).unwrap();
        assert!((second.x1 - (72. + 36.)).abs() < 0.5, "the next one's first line comes half an inch in: {second:?}");
        assert!(second.x2 < 612. - 72. && rows_all_inside(&doc), "a manuscript is ragged: {second:?}");
    }

    fn rows_all_inside(doc: &Document) -> bool {
        let (m, right) = match (doc.width as u32, doc.height as u32) {
            (419, _) => (54., 419.53 - 54.),
            (612, _) => (72., 612. - 72.),
            _ => (72., 595.28 - 72.),
        };
        (0..doc.pages.len()).flat_map(|p| doc.rows(p)).all(|r| r.x1 >= m - 0.6 && r.x2 <= right + 0.6)
    }

    #[test]
    fn a_book_has_a_title_page_part_pages_and_page_numbers() {
        let mut o = opts(Style::Book);
        o.title_page = true;
        o.page_numbers = true;
        let mut b = book(&format!("{PARA}\n"));
        b.chapters[0].part = Some("Part One".into());
        b.chapters.push(Chapter { part: Some("Part Two".into()), markdown: "# Two\n\nShort.\n".into(), ..Default::default() });
        let doc = typeset(&b, &o, &bundled(), &|_| Vec::new());
        assert_eq!(doc.pages.len(), 5, "title, part one, its chapter, part two, its chapter");
        assert_eq!(doc.rows(0)[0].text, "Crime and Punishment");
        assert_eq!(doc.rows(1)[0].text, "PART ONE");
        assert_eq!(doc.rows(2).last().unwrap().text, "3", "the number under the page counts from the title page");
        assert_eq!(doc.rows(4).last().unwrap().text, "5");
        assert_eq!(doc.rows(4)[0].text, "Two");
        assert!(rows_all_inside(&doc));

        let mut o = opts(Style::Manuscript);
        o.title_page = true;
        o.page_numbers = true;
        let doc = typeset(&b, &o, &bundled(), &|_| Vec::new());
        // The name sits at the left of the page's head, the count of words at its right.
        assert!(doc.rows(0)[0].text.starts_with("Fyodor Dostoevsky") && doc.rows(0)[0].text.ends_with("about 100 words"), "{:?}", doc.rows(0));
        assert_eq!(doc.rows(2)[0].text, "Dostoevsky / CRIME AND PUNISHMENT / 3");
        assert!((doc.rows(2)[0].x2 - (612. - 72.)).abs() < 0.5, "the running head ends at the margin: {:?}", doc.rows(2)[0]);
    }

    #[test]
    fn no_heading_ends_a_page() {
        // Thirty lines, then a heading: the heading itself would still fit on the page, the line after it not.
        let md = format!("{}HHHH\n# Heading\n\nAfter.\n", "HHHH\u{2028}".repeat(29));
        let doc = pages(&md, &opts(Style::Book));
        assert_eq!(doc.pages.len(), 2);
        assert!(doc.rows(0).iter().all(|r| r.text.trim() == "HHHH"), "the page ends with the paragraph: {:?}", doc.rows(0).last());
        assert_eq!(doc.rows(1)[0].text, "Heading");
        assert_eq!(doc.rows(1)[1].text, "After.");
    }

    #[test]
    fn no_paragraph_leaves_one_line_alone_either_side_of_a_page() {
        let md = format!("{}HHHH\n\nAfter.\n", "HHHH\u{2028}".repeat(32));
        let doc = pages(&md, &opts(Style::Book));
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.rows(0).iter().filter(|r| r.text.trim() == "HHHH").count(), 31, "the last two lines go over together");
        assert_eq!(doc.rows(1).iter().filter(|r| r.text.trim() == "HHHH").count(), 2);
    }

    #[test]
    fn a_quote_is_indented_and_faint_and_a_list_hangs_its_marker() {
        let doc = pages(&format!("> {PARA}\n\n- one item that is long enough to wrap onto a second line of its own for sure\n- two\n"), &opts(Style::Book));
        assert!(doc.pages[0].items.iter().any(|i| matches!(i, super::Item::Text(r) if r.style.ink == super::Ink::Faded)), "a quotation is drawn fainter");
        let quote = &doc.rows(0);
        assert!((quote[0].x1 - (EDGES.0 + 22.)).abs() < 0.5, "a quote starts two ems in: {:?}", quote[0]);
        assert!((quote[1].x1 - (EDGES.0 + 22.)).abs() < 0.5, "on every line: {:?}", quote[1]);
        let list = doc.rows(0).into_iter().skip_while(|r| !r.text.starts_with('•')).collect::<Vec<_>>();
        assert!(list[0].text.starts_with("•"), "{:?}", list);
        assert!((list[0].x1 - (EDGES.0 + 5.5)).abs() < 0.5, "the marker starts half an em in: {:?}", list[0]);
        assert!((list[1].x1 - (EDGES.0 + 17.6)).abs() < 0.5, "its second line hangs under the text: {:?}", list[1]);
        assert!(rows_all_inside(&doc));
    }

    #[test]
    fn a_hyphenation_point_the_dictionary_offers_breaks_a_word() {
        // A monospace page of 68 characters: five ten-letter words leave room for eleven letters and a hyphen, not for the word.
        let md = format!("{}exceptionallyx and the rest of it goes on to the next line here.\n", "mmmmmmmmmm ".repeat(5));
        let hyphenate = |word: &str| if word.chars().count() >= 6 { vec![3, word.chars().count() - 3] } else { Vec::new() };
        let doc = typeset(&book(&md), &opts(Style::Page), &bundled(), &hyphenate);
        let nothing = typeset(&book(&md), &opts(Style::Page), &bundled(), &|_| Vec::new());
        let (rows, plain) = (doc.rows(0), nothing.rows(0));
        assert!(rows[0].text.ends_with("exceptional-"), "{:?}", rows[0]);
        assert!(rows[1].text.starts_with("lyx"), "{:?}", rows[1]);
        assert!(plain[0].text.ends_with("mmmmmmmmmm") && plain[1].text.starts_with("exceptionallyx"), "{plain:?}");
        // Nothing is lost nor changed but the hyphen.
        let all = |doc: &Document| doc.rows(0).into_iter().map(|r| r.text).collect::<String>();
        assert_eq!(all(&doc).replace(['-', ' '], ""), all(&nothing).replace(' ', ""));
        // The last word of a paragraph is never hyphenated, nor a word in capitals, nor one too short.
        let md = format!("{}exceptionallyx\n", "mmmmmmmmmm ".repeat(5));
        let doc = typeset(&book(&md), &opts(Style::Page), &bundled(), &hyphenate);
        assert_eq!(doc.rows(0)[1].text.trim(), "exceptionallyx", "{:?}", doc.rows(0));
    }

    #[test]
    fn every_glyph_of_a_long_chapter_stays_inside_its_page() {
        let (mut text, mut want) = (String::new(), String::new());
        for i in 0..60 {
            if i % 7 == 0 {
                text.push_str("# Chapter\n\n");
                want.push_str("Chapter");
            }
            text.push_str(&format!("{PARA}\n\n"));
            want.push_str(PARA);
            if i % 5 == 0 {
                text.push_str(&format!("- {PARA}\n- two items\n\n> {PARA}\n\n```\nlet x = 1;\n```\n\n"));
                want.push_str(&format!("{PARA}two items{PARA}let x = 1;"));
            }
        }
        let clean = |s: &str| s.replace([' ', '.', '-', '\u{a0}', '•'], "").to_lowercase();
        for style in Style::ALL {
            let doc = pages(&text, &opts(style));
            assert!(doc.pages.len() > 5, "{style:?}: {} pages", doc.pages.len());
            assert!(rows_all_inside(&doc), "{style:?}");
            let letters = (0..doc.pages.len()).flat_map(|p| doc.rows(p)).map(|r| r.text).collect::<String>();
            assert_eq!(clean(&letters), clean(&want), "{style:?}: every character comes out once, in order");
        }
    }
}
