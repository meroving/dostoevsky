use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::Note;

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading { level: u8, inlines: Vec<Inline> },
    Paragraph(Vec<Inline>),
    Quote(Vec<Block>),
    /// `start` is the first number of an ordered list.
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Code(String),
    Rule,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Run(Run),
    Break,
    /// An index into the chapter's notes.
    NoteStart(usize),
    NoteEnd(usize),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: Option<String>,
}

impl Run {
    pub fn plain(text: &str) -> Run {
        Run { text: text.into(), ..Default::default() }
    }
}

pub fn plain(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Run(r) => s.push_str(&r.text),
            Inline::Break => s.push('\n'),
            _ => {}
        }
    }
    s
}

fn options() -> Options {
    Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS
}

/// Note markers, in source bytes.
struct Marks {
    /// (byte, is_start, note) sorted by byte, ends before starts at the same byte.
    at: Vec<(usize, bool, usize)>,
    next: usize,
}

impl Marks {
    fn new(md: &str, notes: &[Note]) -> Marks {
        let byte = byte_of(md);
        let mut at = Vec::new();
        for (i, n) in notes.iter().enumerate() {
            if n.range.start >= n.range.end {
                continue;
            }
            at.push((byte(n.range.start), true, i));
            at.push((byte(n.range.end), false, i));
        }
        at.sort_by_key(|&(b, start, _)| (b, start));
        Marks { at, next: 0 }
    }

    fn upto(&mut self, byte: usize, out: &mut Vec<Inline>) {
        while let Some(&(b, start, i)) = self.at.get(self.next) {
            if b > byte {
                break;
            }
            out.push(if start { Inline::NoteStart(i) } else { Inline::NoteEnd(i) });
            self.next += 1;
        }
    }

    /// Markers strictly inside `r`, as byte offsets into it.
    fn inside(&mut self, r: &Range<usize>) -> Vec<(usize, Inline)> {
        let mut v = Vec::new();
        while let Some(&(b, start, i)) = self.at.get(self.next) {
            if b >= r.end {
                break;
            }
            v.push((b.saturating_sub(r.start), if start { Inline::NoteStart(i) } else { Inline::NoteEnd(i) }));
            self.next += 1;
        }
        v
    }

    fn rest(&mut self, out: &mut Vec<Inline>) {
        self.upto(usize::MAX, out);
    }
}

fn byte_of(md: &str) -> impl Fn(usize) -> usize + '_ {
    move |chars: usize| md.char_indices().nth(chars).map_or(md.len(), |(b, _)| b)
}

#[derive(Default, Clone)]
struct Fmt {
    bold: usize,
    italic: usize,
    strike: usize,
    link: Vec<String>,
}

enum Open {
    Heading(u8, Vec<Inline>),
    Paragraph(Vec<Inline>),
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Vec<Block>>),
    Item(Vec<Block>, Vec<Inline>),
    Code(String),
    /// Tables, footnotes and other containers read as paragraphs of their text.
    Other(Vec<Inline>),
}

/// `notes` are char ranges into `md`.
pub fn parse(md: &str, notes: &[Note]) -> Vec<Block> {
    let mut marks = Marks::new(md, notes);
    let mut stack: Vec<Open> = Vec::new();
    let mut out: Vec<Block> = Vec::new();
    let mut fmt = Fmt::default();

    fn push_block(stack: &mut [Open], out: &mut Vec<Block>, b: Block) {
        match stack.last_mut() {
            Some(Open::Quote(v)) => v.push(b),
            Some(Open::Item(v, inl)) => {
                if !inl.is_empty() {
                    v.push(Block::Paragraph(std::mem::take(inl)));
                }
                v.push(b)
            }
            _ => out.push(b),
        }
    }
    fn inlines(stack: &mut [Open]) -> Option<&mut Vec<Inline>> {
        match stack.last_mut()? {
            Open::Heading(_, v) | Open::Paragraph(v) | Open::Item(_, v) | Open::Other(v) => Some(v),
            _ => None,
        }
    }
    fn run(fmt: &Fmt, text: &str, code: bool) -> Inline {
        Inline::Run(Run { text: text.into(), bold: fmt.bold > 0, italic: fmt.italic > 0, code, strike: fmt.strike > 0, link: fmt.link.last().cloned() })
    }

    for (ev, r) in Parser::new_ext(md, options()).into_offset_iter() {
        let is_code = matches!(ev, Event::Code(_));
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => stack.push(Open::Heading(heading_level(level), Vec::new())),
                Tag::Paragraph => {
                    // A tight list item's text comes without a paragraph; a loose one's with.
                    stack.push(Open::Paragraph(Vec::new()))
                }
                Tag::BlockQuote(_) => stack.push(Open::Quote(Vec::new())),
                Tag::List(start) => stack.push(Open::List(start, Vec::new())),
                Tag::Item => stack.push(Open::Item(Vec::new(), Vec::new())),
                Tag::CodeBlock(_) => stack.push(Open::Code(String::new())),
                Tag::Emphasis => fmt.italic += 1,
                Tag::Strong => fmt.bold += 1,
                Tag::Strikethrough => fmt.strike += 1,
                Tag::Link { dest_url, .. } => fmt.link.push(dest_url.to_string()),
                Tag::Table(_) | Tag::FootnoteDefinition(_) | Tag::HtmlBlock => stack.push(Open::Other(Vec::new())),
                Tag::TableRow | Tag::TableHead => {}
                Tag::TableCell => {
                    if let Some(v) = inlines(&mut stack) {
                        if !v.is_empty() {
                            v.push(Inline::Run(Run::plain(" · ")));
                        }
                    }
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Heading(_) => {
                    if let Some(Open::Heading(l, mut v)) = stack.pop() {
                        marks.upto(r.end, &mut v);
                        push_block(&mut stack, &mut out, Block::Heading { level: l, inlines: v });
                    }
                }
                TagEnd::Paragraph => {
                    if let Some(Open::Paragraph(mut v)) = stack.pop() {
                        marks.upto(r.end, &mut v);
                        push_block(&mut stack, &mut out, Block::Paragraph(v));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    if let Some(Open::Quote(v)) = stack.pop() {
                        push_block(&mut stack, &mut out, Block::Quote(v));
                    }
                }
                TagEnd::Item => {
                    if let Some(Open::Item(mut v, inl)) = stack.pop() {
                        if !inl.is_empty() {
                            v.push(Block::Paragraph(inl));
                        }
                        if let Some(Open::List(_, items)) = stack.last_mut() {
                            items.push(v);
                        }
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Open::List(start, items)) = stack.pop() {
                        push_block(&mut stack, &mut out, Block::List { start, items });
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Open::Code(s)) = stack.pop() {
                        push_block(&mut stack, &mut out, Block::Code(s.trim_end_matches('\n').to_string()));
                    }
                }
                TagEnd::Emphasis => fmt.italic = fmt.italic.saturating_sub(1),
                TagEnd::Strong => fmt.bold = fmt.bold.saturating_sub(1),
                TagEnd::Strikethrough => fmt.strike = fmt.strike.saturating_sub(1),
                TagEnd::Link => {
                    fmt.link.pop();
                }
                TagEnd::Table | TagEnd::FootnoteDefinition | TagEnd::HtmlBlock => {
                    if let Some(Open::Other(v)) = stack.pop() {
                        if !v.is_empty() {
                            push_block(&mut stack, &mut out, Block::Paragraph(v));
                        }
                    }
                }
                TagEnd::TableRow | TagEnd::TableHead => {
                    if let Some(Open::Other(v)) = stack.last_mut() {
                        if !v.is_empty() {
                            v.push(Inline::Break);
                        }
                    }
                }
                _ => {}
            },
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                let code = is_code;
                if let Some(Open::Code(s)) = stack.last_mut() {
                    s.push_str(&t);
                    continue;
                }
                let Some(v) = inlines(&mut stack) else { continue };
                marks.upto(r.start, v);
                // Markers inside the run split it, when the run is the source as written.
                let inside = marks.inside(&r);
                if inside.is_empty() || t.len() != r.len() {
                    v.push(run(&fmt, &t, code));
                    v.extend(inside.into_iter().map(|(_, m)| m));
                } else {
                    let mut from = 0;
                    for (at, m) in inside {
                        let at = at.min(t.len());
                        if at > from && t.is_char_boundary(at) {
                            v.push(run(&fmt, &t[from..at], code));
                            from = at;
                        }
                        v.push(m);
                    }
                    if from < t.len() {
                        v.push(run(&fmt, &t[from..], code));
                    }
                }
            }
            Event::SoftBreak => {
                if let Some(v) = inlines(&mut stack) {
                    v.push(Inline::Run(Run { text: " ".into(), bold: fmt.bold > 0, italic: fmt.italic > 0, ..Default::default() }));
                }
            }
            Event::HardBreak => {
                if let Some(v) = inlines(&mut stack) {
                    v.push(Inline::Break);
                }
            }
            Event::Rule => push_block(&mut stack, &mut out, Block::Rule),
            Event::TaskListMarker(done) => {
                if let Some(v) = inlines(&mut stack) {
                    v.push(Inline::Run(Run::plain(if done { "☑ " } else { "☐ " })));
                }
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                // Not prose; text between tags still comes as Text.
                let _ = h;
            }
            Event::FootnoteReference(_) => {}
        }
    }
    let mut tail = Vec::new();
    marks.rest(&mut tail);
    if !tail.is_empty() {
        if let Some(v) = last_inlines(&mut out) {
            v.extend(tail);
        }
    }
    out
}

fn last_inlines(blocks: &mut [Block]) -> Option<&mut Vec<Inline>> {
    match blocks.last_mut()? {
        Block::Heading { inlines, .. } | Block::Paragraph(inlines) => Some(inlines),
        Block::Quote(b) => last_inlines(b),
        Block::List { items, .. } => last_inlines(items.last_mut()?),
        _ => None,
    }
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(md: &str, s: &str, others: &[&str]) -> Note {
        let b = md.find(s).unwrap();
        let start = md[..b].chars().count();
        Note { range: start..start + s.chars().count(), others: others.iter().map(|s| s.to_string()).collect(), comment: None }
    }

    #[test]
    fn reads_headings_paragraphs_lists_quotes_and_rules() {
        let md = "# The garret\n\nOn an *exceptionally* hot **evening**.\n\n> A quote\n\n- one\n- two\n\n1. first\n\n---\n\nEnd  \nline\n";
        let b = parse(md, &[]);
        assert!(matches!(&b[0], Block::Heading { level: 1, inlines } if plain(inlines) == "The garret"));
        let Block::Paragraph(p) = &b[1] else { panic!() };
        assert_eq!(plain(p), "On an exceptionally hot evening.");
        assert!(p.iter().any(|i| matches!(i, Inline::Run(r) if r.italic && r.text == "exceptionally")));
        assert!(p.iter().any(|i| matches!(i, Inline::Run(r) if r.bold && r.text == "evening")));
        assert!(matches!(&b[2], Block::Quote(q) if q.len() == 1));
        assert!(matches!(&b[3], Block::List { start: None, items } if items.len() == 2));
        assert!(matches!(&b[4], Block::List { start: Some(1), .. }));
        assert_eq!(b[5], Block::Rule);
        let Block::Paragraph(p) = &b[6] else { panic!() };
        assert!(p.contains(&Inline::Break));
    }

    #[test]
    fn notes_mark_their_place_inside_runs() {
        let md = "On an exceptionally hot evening.\n";
        let n = note(md, "exceptionally hot", &["stifling"]);
        let b = parse(md, &[n]);
        let Block::Paragraph(p) = &b[0] else { panic!() };
        let start = p.iter().position(|i| *i == Inline::NoteStart(0)).unwrap();
        let end = p.iter().position(|i| *i == Inline::NoteEnd(0)).unwrap();
        assert!(start < end);
        assert_eq!(plain(&p[start..end]), "exceptionally hot");
        assert_eq!(plain(p), "On an exceptionally hot evening.");
    }

    #[test]
    fn notes_across_markup_and_paragraphs_still_open_and_close() {
        let md = "One *two* three.\n\nFour five.\n";
        let n = note(md, "*two* three.\n\nFour", &["x"]);
        let b = parse(md, &[n]);
        let count = |b: &Block, m: &Inline| match b {
            Block::Paragraph(p) => p.iter().filter(|i| *i == m).count(),
            _ => 0,
        };
        assert_eq!(b.iter().map(|x| count(x, &Inline::NoteStart(0))).sum::<usize>(), 1);
        assert_eq!(b.iter().map(|x| count(x, &Inline::NoteEnd(0))).sum::<usize>(), 1);
    }

    #[test]
    fn a_note_on_the_last_word_closes() {
        let md = "The end";
        let n = note(md, "end", &["finish"]);
        let b = parse(md, &[n]);
        let Block::Paragraph(p) = &b[0] else { panic!() };
        assert_eq!(p.last(), Some(&Inline::NoteEnd(0)));
    }
}
