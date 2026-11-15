//! The final text leaves the app as Markdown, plain text, HTML, Word or ePub. No UI and no
//! platform code: PDF is laid out by the app (TextKit) from the same [`ir`] blocks.

pub mod docx;
pub mod epub;
pub mod html;
pub mod ir;
pub mod style;
pub mod text;
pub mod zip;

use std::ops::Range;

pub use ir::{Block, Inline, Run};
pub use style::{Align, ChapterTitles, Options, Paper, Paragraphs, Style, Text};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub title: String,
    pub author: String,
    /// A language code (`en`, `ru`).
    pub language: Option<String>,
    pub chapters: Vec<Chapter>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chapter {
    /// Set on the first chapter of a part (a book's subfolder).
    pub part: Option<String>,
    /// From the file name, for chapters that don't start with a heading of their own.
    pub file_title: String,
    /// The final text: chosen versions only, no ghosts, no comment block.
    pub markdown: String,
    pub notes: Vec<Note>,
}

/// A range of the final text, in chars, that had other versions or a comment.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Note {
    pub range: Range<usize>,
    pub others: Vec<String>,
    /// The comment and its author (`None`: the book's author).
    pub comment: Option<(String, Option<String>)>,
}

impl Book {
    pub fn page(title: &str, author: &str, markdown: String) -> Book {
        Book { title: title.into(), author: author.into(), language: None, chapters: vec![Chapter { markdown, ..Default::default() }] }
    }

    pub fn words(&self) -> usize {
        self.chapters.iter().map(|c| english::word_count(&c.markdown)).sum()
    }

    /// A chapter that starts with a heading keeps its own title.
    pub fn blocks(&self, titles: ChapterTitles) -> Vec<ChapterBlocks> {
        self.chapters
            .iter()
            .map(|c| {
                let mut blocks = ir::parse(&c.markdown, &c.notes);
                let starts_with_heading = matches!(blocks.first(), Some(Block::Heading { .. }));
                if titles == ChapterTitles::FileNames && !starts_with_heading && !c.file_title.trim().is_empty() {
                    blocks.insert(0, Block::Heading { level: 1, inlines: vec![Inline::Run(Run::plain(c.file_title.trim()))] });
                }
                ChapterBlocks { part: c.part.clone(), blocks, notes: c.notes.clone() }
            })
            .collect()
    }

    /// To the nearest 100 under 10,000 words, to the nearest 1,000 above.
    pub fn rounded_words(&self) -> usize {
        let w = self.words();
        let step = if w < 10_000 { 100 } else { 1_000 };
        if w == 0 {
            return 0;
        }
        ((w + step / 2) / step * step).max(step)
    }

    pub fn surname(&self) -> &str {
        self.author.split_whitespace().last().unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChapterBlocks {
    pub part: Option<String>,
    pub blocks: Vec<Block>,
    pub notes: Vec<Note>,
}

/// `01-the-garret` → "The garret"; `2026-11-01` stays as it is.
pub fn title_from_stem(stem: &str) -> String {
    let digits = stem.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &stem[digits..];
    let rest = if digits > 0 && digits <= 6 && rest.starts_with(['-', '_', ' ', '.']) { &rest[1..] } else if digits > 0 && rest.is_empty() { "" } else { stem };
    let words = rest.replace(['-', '_'], " ");
    let words = words.trim();
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters aren't allowed in XML 1.0.
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_file_names() {
        assert_eq!(title_from_stem("01-the-garret"), "The garret");
        assert_eq!(title_from_stem("12_a_visit"), "A visit");
        assert_eq!(title_from_stem("preface"), "Preface");
        assert_eq!(title_from_stem("3d-printing"), "3d printing");
        assert_eq!(title_from_stem("07"), "");
    }

    #[test]
    fn manuscript_word_counts_are_rounded() {
        let b = |n: usize| Book::page("", "", "word ".repeat(n));
        assert_eq!(b(1_234).rounded_words(), 1_200);
        assert_eq!(b(84_310).rounded_words(), 84_000);
        assert_eq!(b(30).rounded_words(), 100);
    }

    #[test]
    fn chapters_get_titles_from_file_names_only_when_they_have_none() {
        let book = Book {
            chapters: vec![
                Chapter { file_title: "The garret".into(), markdown: "On an evening.\n".into(), ..Default::default() },
                Chapter { file_title: "The visit".into(), markdown: "# A visit\n\nA knock.\n".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let blocks = book.blocks(ChapterTitles::FileNames);
        assert!(matches!(&blocks[0].blocks[0], Block::Heading { inlines, .. } if ir::plain(inlines) == "The garret"));
        assert!(matches!(&blocks[1].blocks[0], Block::Heading { inlines, .. } if ir::plain(inlines) == "A visit"));
        let own = book.blocks(ChapterTitles::Headings);
        assert!(matches!(&own[0].blocks[0], Block::Paragraph(_)));
    }
}
