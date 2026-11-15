//! The markup stays XHTML-clean (`<br/>`, escaped text): the ePub uses it as it is.

use crate::ir::{Block, Inline, Run};
use crate::style::{trim_num, Align, Options, Paragraphs, Style, Text};
use crate::{esc, Book, ChapterTitles};

/// No styles: Mail, Docs and the like set their own type.
pub fn fragment(book: &Book, titles: ChapterTitles) -> String {
    let mut out = String::new();
    for c in book.blocks(titles) {
        if let Some(p) = &c.part {
            out.push_str(&format!("<h1>{}</h1>\n", esc(p.trim())));
        }
        blocks(&c.blocks, false, &mut out);
    }
    out
}

pub fn page(book: &Book, o: &Options) -> String {
    let lang = book.language.as_deref().unwrap_or("en");
    let mut out = String::new();
    out.push_str(&format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\"/>\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\n<title>{}</title>\n",
        esc(if book.title.trim().is_empty() { "Untitled" } else { book.title.trim() })
    ));
    if !book.author.trim().is_empty() {
        out.push_str(&format!("<meta name=\"author\" content=\"{}\"/>\n", esc(book.author.trim())));
    }
    out.push_str("<style>\n");
    out.push_str(&css(&o.text, o.style, true));
    out.push_str("</style>\n</head>\n<body>\n<main>\n");
    if o.title_page && !book.title.trim().is_empty() {
        out.push_str(&format!("<header class=\"title\"><h1>{}</h1>", esc(book.title.trim())));
        if !book.author.trim().is_empty() {
            out.push_str(&format!("<p class=\"author\">{}</p>", esc(book.author.trim())));
        }
        out.push_str("</header>\n");
    }
    for c in book.blocks(o.chapter_titles) {
        out.push_str("<section>\n");
        if let Some(p) = &c.part {
            out.push_str(&format!("<h1 class=\"part\">{}</h1>\n", esc(p.trim())));
        }
        blocks(&c.blocks, o.style == Style::Book, &mut out);
        out.push_str("</section>\n");
    }
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

pub fn font_stack(font: &str) -> String {
    let fallback = match font {
        "IBM Plex Mono" | "JetBrains Mono" | "Courier New" => "ui-monospace, Menlo, monospace",
        "iA Writer Quattro S" | "Arial" | "Calibri" => "-apple-system, Helvetica, sans-serif",
        "Literata" => "\"Iowan Old Style\", Georgia, serif",
        _ => "Georgia, serif",
    };
    format!("\"{font}\", {fallback}")
}

/// `screen`: the font and a centred column for a browser; an ePub leaves the font, its size and
/// the page to the reader.
pub fn css(t: &Text, style: Style, screen: bool) -> String {
    let mut s = String::new();
    if screen {
        s.push_str(&format!(
            "body {{ margin: 0; background: #fdfbf7; color: #1d1b20; font-family: {}; font-size: {}pt; line-height: {}; }}\nmain {{ max-width: 36em; margin: 0 auto; padding: 3em 1.5em 5em; }}\n",
            font_stack(&t.font),
            trim_num(t.size),
            trim_num(t.line_spacing)
        ));
        s.push_str("@media (prefers-color-scheme: dark) { body { background: #12141c; color: #e9dfd6; } a { color: #a59cf0; } }\n");
    } else {
        s.push_str(&format!("body {{ line-height: {}; }}\n", trim_num(t.line_spacing)));
    }
    let align = match t.align {
        Align::Justified => "justify",
        Align::Left => "left",
    };
    let hy = if t.hyphenate { "auto" } else { "manual" };
    s.push_str(&format!("p, li {{ text-align: {align}; hyphens: {hy}; -webkit-hyphens: {hy}; }}\n"));
    match t.paragraphs {
        Paragraphs::Spaced => s.push_str("p { margin: 0 0 1em; }\n"),
        Paragraphs::Indented => s.push_str("p { margin: 0; text-indent: 1.5em; }\nh1 + p, h2 + p, h3 + p, hr + p, blockquote + p, ul + p, ol + p, pre + p, .first { text-indent: 0; }\n"),
    }
    let hf = t.heading_font.as_deref().map(font_stack).unwrap_or_else(|| "inherit".into());
    s.push_str(&format!("h1, h2, h3, h4, h5, h6 {{ font-family: {hf}; line-height: 1.25; hyphens: manual; -webkit-hyphens: manual; }}\n"));
    s.push_str("h1 { font-size: 1.6em; margin: 1.6em 0 1em; }\nh2 { font-size: 1.3em; margin: 1.4em 0 .8em; }\nh3 { font-size: 1.1em; margin: 1.2em 0 .6em; }\n");
    s.push_str("blockquote { margin: 1em 0; padding-left: 1.2em; border-left: 2px solid #c9c0b0; color: #4a4550; }\n");
    s.push_str("hr { border: 0; text-align: center; margin: 1.5em 0; }\nhr::after { content: \"*   *   *\"; }\n");
    s.push_str("pre { white-space: pre-wrap; font-family: ui-monospace, Menlo, monospace; font-size: .9em; }\ncode { font-family: ui-monospace, Menlo, monospace; font-size: .92em; }\n");
    s.push_str(".title { text-align: center; margin: 4em 0 5em; }\n.title h1 { font-size: 2.2em; margin: 0 0 .5em; }\n.title .author { text-align: center; font-style: italic; }\n");
    s.push_str("h1.part { text-align: center; font-size: 1em; letter-spacing: .3em; text-transform: uppercase; font-weight: normal; margin: 3em 0 .5em; }\n");
    if style == Style::Book {
        s.push_str("section > h1, section > h2 { text-align: center; font-weight: normal; font-style: italic; }\n.first::first-line { font-variant: small-caps; letter-spacing: .03em; }\n");
    }
    s
}

/// `book` marks a chapter's first paragraph after a heading for the opening.
pub fn blocks(bs: &[Block], book: bool, out: &mut String) {
    let mut after_heading = false;
    for b in bs {
        match b {
            Block::Heading { level, inlines: i } => {
                let l = (*level).clamp(1, 6);
                out.push_str(&format!("<h{l}>"));
                inlines(i, out);
                out.push_str(&format!("</h{l}>\n"));
                after_heading = true;
                continue;
            }
            Block::Paragraph(i) => {
                out.push_str(if book && after_heading { "<p class=\"first\">" } else { "<p>" });
                inlines(i, out);
                out.push_str("</p>\n");
            }
            Block::Quote(inner) => {
                out.push_str("<blockquote>\n");
                blocks(inner, false, out);
                out.push_str("</blockquote>\n");
            }
            Block::List { start, items } => {
                match start {
                    Some(1) => out.push_str("<ol>\n"),
                    Some(n) => out.push_str(&format!("<ol start=\"{n}\">\n")),
                    None => out.push_str("<ul>\n"),
                }
                for item in items {
                    out.push_str("<li>");
                    if let [Block::Paragraph(i)] = item.as_slice() {
                        inlines(i, out);
                    } else {
                        out.push('\n');
                        blocks(item, false, out);
                    }
                    out.push_str("</li>\n");
                }
                out.push_str(if start.is_some() { "</ol>\n" } else { "</ul>\n" });
            }
            Block::Code(s) => out.push_str(&format!("<pre><code>{}</code></pre>\n", esc(s))),
            Block::Rule => out.push_str("<hr/>\n"),
        }
        after_heading = false;
    }
}

pub fn inlines(is: &[Inline], out: &mut String) {
    for i in is {
        match i {
            Inline::Run(r) => run(r, out),
            Inline::Break => out.push_str("<br/>"),
            Inline::NoteStart(_) | Inline::NoteEnd(_) => {}
        }
    }
}

fn run(r: &Run, out: &mut String) {
    let mut open = Vec::new();
    if let Some(l) = &r.link {
        out.push_str(&format!("<a href=\"{}\">", esc(l)));
        open.push("</a>");
    }
    for (on, o, c) in [(r.bold, "<strong>", "</strong>"), (r.italic, "<em>", "</em>"), (r.strike, "<del>", "</del>"), (r.code, "<code>", "</code>")] {
        if on {
            out.push_str(o);
            open.push(c);
        }
    }
    out.push_str(&esc(&r.text));
    for c in open.into_iter().rev() {
        out.push_str(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chapter, Paper};

    #[test]
    fn the_fragment_is_bare_html() {
        let b = Book::page("", "", "# Title\n\nOn an *exceptionally* hot evening & a [link](https://x.y).\n\n- a\n- b\n\n---\n".into());
        let h = fragment(&b, ChapterTitles::Headings);
        assert_eq!(h, "<h1>Title</h1>\n<p>On an <em>exceptionally</em> hot evening &amp; a <a href=\"https://x.y\">link</a>.</p>\n<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n<hr/>\n");
    }

    #[test]
    fn a_page_carries_its_type_and_language() {
        let mut b = Book::page("Crime", "F. D.", "# One\n\nText.\n".into());
        b.language = Some("ru".into());
        let o = Options::new(Style::Book, Text::book(Paper::A5), Paper::A5);
        let h = page(&b, &o);
        assert!(h.contains("<html lang=\"ru\">"));
        assert!(h.contains("\"Iowan Old Style\", Georgia, serif"));
        assert!(h.contains("text-align: justify; hyphens: auto"));
        assert!(h.contains("<p class=\"first\">Text.</p>"));
        assert!(h.contains("<title>Crime</title>"));
    }

    #[test]
    fn parts_open_their_chapters() {
        let b = Book { chapters: vec![Chapter { part: Some("Part One".into()), markdown: "x".into(), ..Default::default() }], ..Default::default() };
        assert!(fragment(&b, ChapterTitles::Headings).starts_with("<h1>Part One</h1>"));
    }
}
