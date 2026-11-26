//! How a style sets a chapter: each paragraph's indents, spacing, alignment and the look of
//! its runs. Plain decisions with no fonts in them, so TextKit on the Mac and [`crate`]'s own
//! engine elsewhere lay out the same paragraphs.

use export::{Block, Inline, Options, Run};

#[derive(Clone, Copy, PartialEq)]
pub struct ParaLook {
    pub align: Align,
    pub first_indent: f64,
    pub head_indent: f64,
    pub before: f64,
    pub after: f64,
    pub line: f64,
    pub hyphenate: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Center,
    Justified,
}

#[derive(Clone, Copy, PartialEq)]
pub struct RunLook {
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub heading: bool,
    pub strike: bool,
    pub underline: bool,
    pub grey: bool,
    pub kern: f64,
}

#[derive(Clone, PartialEq)]
pub struct Para {
    pub look: ParaLook,
    pub pieces: Vec<(String, RunLook)>,
    /// A heading or a scene break: never the last thing on a page.
    pub keep: bool,
}

/// A share of the text area's height.
pub fn opening_drop(o: &Options, starts_with_heading: bool) -> f64 {
    match o.style {
        export::Style::Manuscript => 1. / 3.,
        export::Style::Book if starts_with_heading => 0.2,
        _ => 0.,
    }
}

pub fn compose(blocks: &[Block], o: &Options) -> Vec<Para> {
    let t = &o.text;
    let size = t.size as f64;
    let line = size * t.line_spacing as f64;
    let indented = t.paragraphs == export::Paragraphs::Indented;
    let manuscript = o.style == export::Style::Manuscript;
    let book = o.style == export::Style::Book;
    let base = ParaLook {
        align: if t.align == export::Align::Justified { Align::Justified } else { Align::Left },
        first_indent: if indented { if manuscript { 36. } else { size * 1.5 } } else { 0. },
        head_indent: 0.,
        before: 0.,
        after: if indented { 0. } else { size },
        line,
        hyphenate: t.hyphenate,
    };
    let body = RunLook { size, bold: false, italic: false, mono: false, heading: false, strike: false, underline: false, grey: false, kern: 0. };
    let mut out = Vec::new();
    let mut after_heading = true;
    walk(blocks, o, base, body, 0, &mut after_heading, &mut out);
    if book {
        if let Some(p) = out.iter_mut().find(|p| p.pieces.iter().any(|(_, r)| !r.heading)) {
            small_caps_opening(p, size);
        }
    }
    out
}

fn small_caps_opening(p: &mut Para, size: f64) {
    let Some(first) = p.pieces.iter().position(|(t, _)| !t.trim().is_empty()) else { return };
    let (text, look) = p.pieces[first].clone();
    let mut words = 0;
    let mut cut = text.len();
    for (i, c) in text.char_indices() {
        if c.is_whitespace() && i > 0 && !text[..i].ends_with(char::is_whitespace) {
            words += 1;
            if words == 4 {
                cut = i;
                break;
            }
        }
    }
    let (head, rest) = text.split_at(cut);
    let caps = RunLook { size: size * 0.82, kern: size * 0.04, ..look };
    let mut pieces = vec![(head.to_uppercase(), caps)];
    if !rest.is_empty() {
        pieces.push((rest.to_string(), look));
    }
    p.pieces.splice(first..=first, pieces);
}

#[allow(clippy::too_many_arguments)]
fn walk(blocks: &[Block], o: &Options, base: ParaLook, body: RunLook, depth: usize, after_heading: &mut bool, out: &mut Vec<Para>) {
    let t = &o.text;
    let size = t.size as f64;
    let manuscript = o.style == export::Style::Manuscript;
    let book = o.style == export::Style::Book;
    for b in blocks {
        match b {
            Block::Heading { level, inlines } => {
                let (scale, align) = match (o.style, level) {
                    (export::Style::Manuscript, _) => (1., Align::Center),
                    (export::Style::Book, 1) => (1.75, Align::Center),
                    (export::Style::Book, _) => (1.2, Align::Center),
                    (_, 1) => (if t.heading_font.is_some() { 1.9 } else { 1.5 }, Align::Left),
                    (_, 2) => (if t.heading_font.is_some() { 1.5 } else { 1.25 }, Align::Left),
                    _ => (if t.heading_font.is_some() { 1.25 } else { 1.1 }, Align::Left),
                };
                let hsize = size * scale;
                let look = ParaLook {
                    align,
                    first_indent: 0.,
                    head_indent: 0.,
                    before: if out.is_empty() { 0. } else { size * if book { 2. } else { 1.2 } },
                    after: if manuscript { size * t.line_spacing as f64 } else if book { size * 2.5 } else { size * 0.8 },
                    line: if manuscript { base.line } else { hsize * 1.25 },
                    hyphenate: false,
                };
                let run = RunLook { size: hsize, bold: !manuscript && !book && t.heading_font.is_none(), italic: book && *level == 1, heading: true, ..body };
                out.push(Para { look, pieces: pieces(inlines, run), keep: true });
                *after_heading = true;
                continue;
            }
            Block::Paragraph(inlines) => {
                let mut look = base;
                if *after_heading {
                    look.first_indent = 0.;
                }
                if depth > 0 {
                    look.first_indent = 0.;
                    look.head_indent = size * 2.;
                    look.after = if t.paragraphs == export::Paragraphs::Spaced { size } else { size * 0.5 };
                }
                let run = if depth > 0 && !manuscript { RunLook { grey: true, ..body } } else { body };
                out.push(Para { look, pieces: pieces(inlines, run), keep: false });
            }
            Block::Quote(inner) => {
                let mut ah = false;
                walk(inner, o, base, body, depth + 1, &mut ah, out);
                if let Some(p) = out.last_mut() {
                    p.look.after = p.look.after.max(size);
                }
            }
            Block::List { start, items } => {
                for (i, item) in items.iter().enumerate() {
                    let marker = match start {
                        Some(n) => format!("{}.\u{a0}", n + i as u64),
                        None => "•\u{a0}\u{a0}".into(),
                    };
                    let mut first = true;
                    for b in item {
                        match b {
                            Block::Paragraph(inl) | Block::Heading { inlines: inl, .. } => {
                                let look = ParaLook { first_indent: size * 0.5, head_indent: size * 1.6, after: if t.paragraphs == export::Paragraphs::Spaced { size * 0.4 } else { 0. }, ..base };
                                let mut ps = Vec::new();
                                if first {
                                    ps.push((marker.clone(), body));
                                }
                                ps.extend(pieces(inl, body));
                                out.push(Para { look, pieces: ps, keep: false });
                                first = false;
                            }
                            other => {
                                let mut ah = false;
                                walk(std::slice::from_ref(other), o, base, body, depth + 1, &mut ah, out);
                            }
                        }
                    }
                }
                if let Some(p) = out.last_mut() {
                    p.look.after = p.look.after.max(if t.paragraphs == export::Paragraphs::Spaced { size } else { size * 0.5 });
                }
            }
            Block::Code(s) => {
                let look = ParaLook { align: Align::Left, first_indent: 0., head_indent: size, after: size * 0.8, line: size * 1.3, hyphenate: false, before: 0. };
                out.push(Para { look, pieces: vec![(s.replace('\n', "\u{2028}"), RunLook { mono: true, size: size * 0.86, ..body })], keep: false });
            }
            Block::Rule => {
                let mark = if manuscript { "#" } else { "*\u{a0}\u{a0}\u{a0}*\u{a0}\u{a0}\u{a0}*" };
                let look = ParaLook { align: Align::Center, first_indent: 0., head_indent: 0., before: if manuscript { 0. } else { size * 0.6 }, after: if manuscript { 0. } else { size * 1.2 }, ..base };
                out.push(Para { look, pieces: vec![(mark.into(), body)], keep: true });
                *after_heading = true;
                continue;
            }
        }
        *after_heading = false;
    }
}

fn pieces(inlines: &[Inline], base: RunLook) -> Vec<(String, RunLook)> {
    let mut v = Vec::new();
    for i in inlines {
        match i {
            Inline::Run(r) => v.push((r.text.clone(), run_look(r, base))),
            // U+2028 keeps the paragraph's style.
            Inline::Break => v.push(("\u{2028}".into(), base)),
            Inline::NoteStart(_) | Inline::NoteEnd(_) => {}
        }
    }
    v
}

fn run_look(r: &Run, base: RunLook) -> RunLook {
    RunLook {
        bold: base.bold || r.bold,
        italic: base.italic || r.italic,
        mono: r.code,
        size: if r.code { base.size * 0.9 } else { base.size },
        strike: r.strike,
        underline: r.link.is_some(),
        ..base
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use export::{Paper, Style, Text};

    fn opts(style: Style) -> Options {
        let text = match style {
            Style::Manuscript => Text::manuscript(),
            Style::Book => Text::book(Paper::A5),
            Style::Page => Text::page("IBM Plex Mono", 19., 1.8, true, true, None),
        };
        Options::new(style, text, Paper::A4)
    }

    #[test]
    fn paragraphs_follow_the_style() {
        let blocks = export::ir::parse("# One\n\nFirst para.\n\nSecond para.\n\n---\n\nAfter.\n", &[]);
        let book = compose(&blocks, &opts(Style::Book));
        assert!(book[0].pieces[0].1.heading && book[0].pieces[0].1.italic);
        assert_eq!(book[1].look.first_indent, 0., "the opening paragraph is flush");
        assert_eq!(book[1].pieces[0].0, "FIRST PARA.", "small capitals open the chapter");
        assert!(book[2].look.first_indent > 0.);
        assert!(book[2].look.hyphenate && book[2].look.align == Align::Justified);
        assert_eq!(book[3].pieces[0].0, "*\u{a0}\u{a0}\u{a0}*\u{a0}\u{a0}\u{a0}*");
        assert_eq!(book[4].look.first_indent, 0., "flush after a scene break");

        let ms = compose(&blocks, &opts(Style::Manuscript));
        assert_eq!(ms[0].look.align, Align::Center);
        assert_eq!(ms[2].look.first_indent, 36., "half an inch");
        assert_eq!(ms[2].look.line, 24., "double-spaced 12 pt");
        assert!(!ms[2].look.hyphenate);
        assert_eq!(ms[3].pieces[0].0, "#");

        let page = compose(&blocks, &opts(Style::Page));
        assert_eq!(page[2].look.first_indent, 0.);
        assert!(page[2].look.after > 0., "spaced paragraphs");
    }

    #[test]
    fn chapters_open_lower_in_a_manuscript_and_a_book() {
        assert_eq!(opening_drop(&opts(Style::Manuscript), false), 1. / 3.);
        assert_eq!(opening_drop(&opts(Style::Book), true), 0.2);
        assert_eq!(opening_drop(&opts(Style::Page), true), 0.);
    }

}
