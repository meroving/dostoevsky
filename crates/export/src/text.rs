use crate::ir::{self, Block, Inline};
use crate::{Book, ChapterTitles};

pub fn markdown(book: &Book, titles: ChapterTitles) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in &book.chapters {
        if let Some(p) = &c.part {
            parts.push(format!("# {}", p.trim()));
        }
        let body = c.markdown.trim();
        let starts_with_heading = body.starts_with('#');
        if titles == ChapterTitles::FileNames && !starts_with_heading && !c.file_title.trim().is_empty() {
            parts.push(format!("# {}", c.file_title.trim()));
        }
        if !body.is_empty() {
            parts.push(body.to_string());
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    parts.join("\n\n") + "\n"
}

pub fn plain(book: &Book, titles: ChapterTitles) -> String {
    let mut out: Vec<String> = Vec::new();
    for c in book.blocks(titles) {
        if let Some(p) = &c.part {
            out.push(p.trim().to_string());
        }
        for b in &c.blocks {
            block(b, "", &mut out);
        }
    }
    if out.is_empty() {
        return String::new();
    }
    out.join("\n\n") + "\n"
}

fn block(b: &Block, indent: &str, out: &mut Vec<String>) {
    match b {
        Block::Heading { inlines, .. } | Block::Paragraph(inlines) => out.push(indented(&ir::plain(inlines), indent)),
        Block::Quote(bs) => {
            for b in bs {
                block(b, indent, out);
            }
        }
        Block::List { start, items } => {
            let mut lines = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let marker = match start {
                    Some(n) => format!("{}. ", n + i as u64),
                    None => "- ".into(),
                };
                let mut inner = Vec::new();
                let pad = " ".repeat(marker.chars().count());
                for b in item {
                    block(b, &pad, &mut inner);
                }
                let text = inner.join("\n");
                lines.push(format!("{indent}{marker}{}", text.trim_start()));
            }
            out.push(lines.join("\n"));
        }
        Block::Code(s) => out.push(indented(s, indent)),
        Block::Rule => out.push(format!("{indent}* * *")),
    }
}

fn indented(s: &str, indent: &str) -> String {
    s.lines().map(|l| format!("{indent}{l}")).collect::<Vec<_>>().join("\n")
}

pub fn inline_text(inlines: &[Inline]) -> String {
    ir::plain(inlines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Chapter;

    fn book() -> Book {
        Book {
            title: "Crime".into(),
            chapters: vec![
                Chapter { part: Some("Part One".into()), file_title: "The garret".into(), markdown: "On an *exceptionally* hot evening.\n\n- a\n- b\n".into(), ..Default::default() },
                Chapter { file_title: "The visit".into(), markdown: "# A visit\n\nA **knock**.\n\n---\n\nAfter.\n".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn markdown_joins_chapters_and_adds_titles_from_file_names() {
        assert_eq!(markdown(&book(), ChapterTitles::Headings), "# Part One\n\nOn an *exceptionally* hot evening.\n\n- a\n- b\n\n# A visit\n\nA **knock**.\n\n---\n\nAfter.\n");
        let named = markdown(&book(), ChapterTitles::FileNames);
        assert!(named.starts_with("# Part One\n\n# The garret\n\nOn an"));
        assert!(!named.contains("# The visit"));
    }

    #[test]
    fn plain_text_drops_markup() {
        let t = plain(&book(), ChapterTitles::Headings);
        assert_eq!(t, "Part One\n\nOn an exceptionally hot evening.\n\n- a\n- b\n\nA visit\n\nA knock.\n\n* * *\n\nAfter.\n");
    }

    #[test]
    fn an_empty_page_is_empty() {
        assert_eq!(plain(&Book::page("", "", String::new()), ChapterTitles::Headings), "");
        assert_eq!(markdown(&Book::page("", "", "\n".into()), ChapterTitles::Headings), "");
    }
}
