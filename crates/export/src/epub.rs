use crate::ir::Block;
use crate::style::Style;
use crate::zip::Zip;
use crate::{esc, html, Book, Options};

struct Entry {
    file: String,
    title: String,
    part: Option<String>,
}

pub fn write(book: &Book, o: &Options) -> Vec<u8> {
    write_at(book, o, &now())
}

pub fn write_at(book: &Book, o: &Options, modified: &str) -> Vec<u8> {
    let lang = book.language.clone().unwrap_or_else(|| "en".into());
    let title = if book.title.trim().is_empty() { "Untitled".to_string() } else { book.title.trim().to_string() };
    let author = book.author.trim();
    let mut z = Zip::new();
    z.stored("mimetype", b"application/epub+zip");
    z.deflated(
        "META-INF/container.xml",
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>",
    );

    let mut css = html::css(&o.text, o.style, false);
    css.push_str("section.title { text-align: center; margin-top: 30%; }\nsection.title h1 { font-size: 1.8em; font-weight: normal; }\nsection.title .author { text-align: center; text-indent: 0; font-style: italic; }\n");
    css.push_str("section.part { text-align: center; margin-top: 30%; }\nh1.part { text-transform: uppercase; letter-spacing: .3em; font-size: 1em; font-weight: normal; }\n");
    css.push_str(".cover { text-align: center; margin: 0; padding: 0; }\n.cover img { max-width: 100%; max-height: 100%; }\nnav ol { list-style: none; padding-left: 1em; }\n");
    if o.style == Style::Book {
        css.push_str("section.chapter > h1:first-child { margin-top: 20%; }\n");
    }
    z.deflated("OEBPS/style.css", css.as_bytes());

    let mut manifest = String::from("<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    let mut spine = String::new();

    if let Some((bytes, mime)) = &o.cover {
        let ext = if mime.contains("png") { "png" } else { "jpg" };
        z.stored(&format!("OEBPS/cover.{ext}"), bytes);
        manifest.push_str(&format!("<item id=\"cover-image\" href=\"cover.{ext}\" media-type=\"{}\" properties=\"cover-image\"/>\n", esc(mime)));
        manifest.push_str("<item id=\"cover\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
        spine.push_str("<itemref idref=\"cover\" linear=\"no\"/>\n");
        let body = format!("<section class=\"cover\" epub:type=\"cover\"><img src=\"cover.{ext}\" alt=\"{}\"/></section>", esc(&title));
        z.deflated("OEBPS/cover.xhtml", xhtml(&lang, &title, &body).as_bytes());
    }
    if o.title_page {
        let mut body = format!("<section class=\"title\" epub:type=\"titlepage\"><h1>{}</h1>", esc(&title));
        if !author.is_empty() {
            body.push_str(&format!("<p class=\"author\">{}</p>", esc(author)));
        }
        body.push_str("</section>");
        z.deflated("OEBPS/title.xhtml", xhtml(&lang, &title, &body).as_bytes());
        manifest.push_str("<item id=\"title\" href=\"title.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
        spine.push_str("<itemref idref=\"title\"/>\n");
    }
    if o.contents {
        spine.push_str("<itemref idref=\"nav\"/>\n");
    }

    let mut entries: Vec<Entry> = Vec::new();
    let chapters = book.blocks(o.chapter_titles);
    for (i, c) in chapters.iter().enumerate() {
        let n = i + 1;
        let file = format!("chapter-{n:03}.xhtml");
        let ctitle = first_heading(&c.blocks).or_else(|| Some(book.chapters[i].file_title.trim().to_string()).filter(|t| !t.is_empty())).unwrap_or_else(|| {
            if chapters.len() == 1 {
                title.clone()
            } else {
                format!("{n}")
            }
        });
        let mut body = String::new();
        if let Some(p) = &c.part {
            body.push_str(&format!("<section class=\"part\" epub:type=\"part\"><h1 class=\"part\">{}</h1></section>\n", esc(p.trim())));
        }
        body.push_str("<section class=\"chapter\" epub:type=\"chapter\">\n");
        html::blocks(&c.blocks, o.style == Style::Book, &mut body);
        body.push_str("</section>");
        z.deflated(&format!("OEBPS/{file}"), xhtml(&lang, &ctitle, &body).as_bytes());
        manifest.push_str(&format!("<item id=\"c{n}\" href=\"{file}\" media-type=\"application/xhtml+xml\"/>\n"));
        spine.push_str(&format!("<itemref idref=\"c{n}\"/>\n"));
        entries.push(Entry { file, title: ctitle, part: c.part.clone() });
    }
    if entries.is_empty() {
        let file = "chapter-001.xhtml".to_string();
        z.deflated(&format!("OEBPS/{file}"), xhtml(&lang, &title, "<section class=\"chapter\"></section>").as_bytes());
        manifest.push_str("<item id=\"c1\" href=\"chapter-001.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
        spine.push_str("<itemref idref=\"c1\"/>\n");
        entries.push(Entry { file, title: title.clone(), part: None });
    }

    z.deflated("OEBPS/nav.xhtml", nav(&lang, &title, &entries).as_bytes());
    let id = format!("urn:uuid:{}", uuid_from(&format!("{title}\u{0}{author}")));
    z.deflated("OEBPS/toc.ncx", ncx(&id, &title, &entries).as_bytes());

    let mut meta = format!(
        "<dc:identifier id=\"bookid\">{id}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{}</dc:language>\n<meta property=\"dcterms:modified\">{}</meta>\n",
        esc(&title),
        esc(&lang),
        esc(modified)
    );
    if !author.is_empty() {
        meta.push_str(&format!("<dc:creator id=\"author\">{}</dc:creator>\n", esc(author)));
    }
    if o.cover.is_some() {
        meta.push_str("<meta name=\"cover\" content=\"cover-image\"/>\n");
    }
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n{meta}</metadata>\n<manifest>\n{manifest}</manifest>\n<spine toc=\"ncx\">\n{spine}</spine>\n</package>\n",
        esc(&lang)
    );
    z.deflated("OEBPS/content.opf", opf.as_bytes());
    z.finish()
}

fn xhtml(lang: &str, title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{l}\" xml:lang=\"{l}\">\n<head>\n<meta charset=\"UTF-8\"/>\n<title>{t}</title>\n<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        l = esc(lang),
        t = esc(title)
    )
}

fn first_heading(bs: &[Block]) -> Option<String> {
    match bs.first()? {
        Block::Heading { inlines, .. } => Some(crate::ir::plain(inlines).trim().to_string()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

fn nav(lang: &str, title: &str, entries: &[Entry]) -> String {
    let mut ol = String::from("<ol>\n");
    let mut in_part = false;
    for e in entries {
        if let Some(p) = &e.part {
            if in_part {
                ol.push_str("</ol></li>\n");
            }
            ol.push_str(&format!("<li><a href=\"{}\">{}</a><ol>\n", e.file, esc(p.trim())));
            in_part = true;
        }
        ol.push_str(&format!("<li><a href=\"{}\">{}</a></li>\n", e.file, esc(&e.title)));
    }
    if in_part {
        ol.push_str("</ol></li>\n");
    }
    ol.push_str("</ol>");
    let body = format!("<nav epub:type=\"toc\" id=\"toc\"><h1>{}</h1>\n{ol}\n</nav>", esc(title));
    xhtml(lang, title, &body)
}

fn ncx(id: &str, title: &str, entries: &[Entry]) -> String {
    let mut points = String::new();
    for (i, e) in entries.iter().enumerate() {
        points.push_str(&format!(
            "<navPoint id=\"p{n}\" playOrder=\"{n}\"><navLabel><text>{}</text></navLabel><content src=\"{}\"/></navPoint>\n",
            esc(&e.title),
            e.file,
            n = i + 1
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n<head><meta name=\"dtb:uid\" content=\"{}\"/></head>\n<docTitle><text>{}</text></docTitle>\n<navMap>\n{points}</navMap>\n</ncx>\n",
        esc(id),
        esc(title)
    )
}

/// Stable across exports, so a reader's library sees the same book.
fn uuid_from(s: &str) -> String {
    let a = crc32fast::hash(s.as_bytes());
    let b = crc32fast::hash(format!("{s}\u{1}").as_bytes());
    let c = crc32fast::hash(format!("{s}\u{2}").as_bytes());
    let d = crc32fast::hash(format!("{s}\u{3}").as_bytes());
    let hex = format!("{a:08x}{b:08x}{c:08x}{d:08x}");
    format!("{}-{}-4{}-a{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}

pub fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Days since 1970-01-01 → (year, month, day), Howard Hinnant's algorithm.
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Paper, Text};
    use crate::{zip, Chapter};

    fn book() -> Book {
        Book {
            title: "Crime & Punishment".into(),
            author: "Fyodor Dostoevsky".into(),
            language: Some("ru".into()),
            chapters: vec![
                Chapter { part: Some("Part One".into()), file_title: "The garret".into(), markdown: "On an *exceptionally* hot evening.\n".into(), ..Default::default() },
                Chapter { file_title: "The visit".into(), markdown: "# A visit\n\nA knock.\n".into(), ..Default::default() },
            ],
        }
    }

    fn get<'a>(e: &'a [(String, Vec<u8>)], name: &str) -> &'a str {
        std::str::from_utf8(&e.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("{name}")).1).unwrap()
    }

    #[test]
    fn a_book_is_a_valid_package() {
        let mut o = Options::new(Style::Book, Text::book(Paper::A5), Paper::A5);
        o.title_page = true;
        o.cover = Some((vec![0xFF, 0xD8, 0xFF], "image/jpeg".into()));
        let bytes = write_at(&book(), &o, "2026-11-12T10:00:00Z");
        let e = zip::read(&bytes).unwrap();
        assert_eq!(e[0].0, "mimetype");
        let opf = get(&e, "OEBPS/content.opf");
        assert!(opf.contains("<dc:title>Crime &amp; Punishment</dc:title>"));
        assert!(opf.contains("<dc:creator id=\"author\">Fyodor Dostoevsky</dc:creator>"));
        assert!(opf.contains("<dc:language>ru</dc:language>"));
        assert!(opf.contains("<meta property=\"dcterms:modified\">2026-11-12T10:00:00Z</meta>"));
        assert!(opf.contains("properties=\"cover-image\""));
        assert!(opf.contains("properties=\"nav\""));
        for f in ["OEBPS/chapter-001.xhtml", "OEBPS/chapter-002.xhtml", "OEBPS/title.xhtml", "OEBPS/cover.xhtml", "OEBPS/cover.jpg", "OEBPS/toc.ncx", "OEBPS/style.css"] {
            assert!(e.iter().any(|(n, _)| n == f), "{f}");
        }
        let nav = get(&e, "OEBPS/nav.xhtml");
        assert!(nav.contains(">Part One</a><ol>"));
        assert!(nav.contains(">The garret</a>"));
        assert!(nav.contains(">A visit</a>"));
        let c1 = get(&e, "OEBPS/chapter-001.xhtml");
        assert!(c1.contains("lang=\"ru\""));
        assert!(c1.contains("<em>exceptionally</em>"));
        let css = get(&e, "OEBPS/style.css");
        assert!(css.contains("hyphens: auto"));
        assert!(!css.contains("font-family: \"Iowan"), "the reader picks the font");
    }

    #[test]
    fn the_same_book_keeps_its_identifier() {
        let o = Options::new(Style::Book, Text::book(Paper::A5), Paper::A5);
        let a = write_at(&book(), &o, "x");
        let b = write_at(&book(), &o, "x");
        assert_eq!(a, b);
    }

    #[test]
    fn dates_are_iso() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_404), (2025, 11, 12));
        assert!(now().ends_with('Z'));
    }
}
