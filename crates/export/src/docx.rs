use std::collections::BTreeMap;

use crate::ir::{Block, Inline, Run};
use crate::style::{Align, Paragraphs, Style};
use crate::zip::Zip;
use crate::{esc, Book, Options};

const W_NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

/// The language tag Word wants (`ru` → `ru-RU`), so it hyphenates and spell-checks in it.
fn word_lang(code: Option<&str>) -> &'static str {
    match code.unwrap_or("en") {
        "ru" => "ru-RU",
        "de" => "de-DE",
        "fr" => "fr-FR",
        "es" => "es-ES",
        "it" => "it-IT",
        "pt" => "pt-BR",
        _ => "en-US",
    }
}

fn twips(pt: f32) -> i64 {
    (pt * 20.).round() as i64
}

struct Doc<'a> {
    o: &'a Options,
    body: String,
    links: Vec<String>,
    comments: BTreeMap<usize, Body>,
    next_comment: usize,
    open: Vec<usize>,
    break_before: bool,
}

pub fn write(book: &Book, o: &Options) -> Vec<u8> {
    let mut d = Doc { o, body: String::new(), links: Vec::new(), comments: BTreeMap::new(), next_comment: 0, open: Vec::new(), break_before: false };
    let manuscript = o.style == Style::Manuscript;
    if o.title_page {
        d.title_page(book);
    }
    for (ci, c) in book.blocks(o.chapter_titles).into_iter().enumerate() {
        if ci > 0 || o.title_page {
            d.break_before = true;
        }
        if let Some(p) = &c.part {
            d.para("Part", &[Inline::Run(Run::plain(p.trim()))], &[], None);
            d.break_before = true;
        }
        // Note index in this chapter → comment id, for notes with something to say.
        let ids: Vec<Option<usize>> = c
            .notes
            .iter()
            .map(|n| {
                let body = match &n.comment {
                    Some((text, author)) if o.my_comments => Some(Body::Mine(text.clone(), author.clone().filter(|a| !a.is_empty()).unwrap_or_else(|| o.comment_author.clone()))),
                    Some(_) => None,
                    None => (o.comments && !n.others.is_empty()).then(|| Body::Others(n.others.clone())),
                };
                body.map(|b| {
                    let id = d.next_comment;
                    d.next_comment += 1;
                    d.comments.insert(id, b);
                    id
                })
            })
            .collect();
        d.blocks(&c.blocks, &ids, 0, true);
    }
    // A place left open (it shouldn't be) still closes, or Word won't open the file.
    for id in std::mem::take(&mut d.open) {
        d.body.push_str(&format!("<w:p><w:commentRangeEnd w:id=\"{id}\"/><w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:commentReference w:id=\"{id}\"/></w:r></w:p>"));
    }
    if d.body.is_empty() {
        d.body.push_str("<w:p/>");
    }

    let (w, h) = o.paper.size();
    let m = twips(o.paper.margin());
    let header = manuscript && o.page_numbers;
    let footer = !manuscript && o.page_numbers;
    let mut sect = String::from("<w:sectPr>");
    if header {
        sect.push_str("<w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>");
    }
    if footer {
        sect.push_str("<w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/>");
    }
    sect.push_str(&format!(
        "<w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:top=\"{m}\" w:right=\"{m}\" w:bottom=\"{m}\" w:left=\"{m}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/>",
        twips(w),
        twips(h),
        twips(o.paper.margin() / 2.),
        twips(o.paper.margin() / 2.)
    ));
    if o.title_page && (header || footer) {
        sect.push_str("<w:titlePg/>");
    }
    sect.push_str("</w:sectPr>");

    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {W_NS}><w:body>{}{sect}</w:body></w:document>",
        d.body
    );

    let mut rels = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    rels.push_str("<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>");
    rels.push_str("<Relationship Id=\"rIdSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>");
    if header {
        rels.push_str("<Relationship Id=\"rIdHeader\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/>");
    }
    if footer {
        rels.push_str("<Relationship Id=\"rIdFooter\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>");
    }
    if !d.comments.is_empty() {
        rels.push_str("<Relationship Id=\"rIdComments\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments\" Target=\"comments.xml\"/>");
    }
    for (i, l) in d.links.iter().enumerate() {
        rels.push_str(&format!(
            "<Relationship Id=\"rIdLink{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            esc(l)
        ));
    }
    rels.push_str("</Relationships>");

    let mut types = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    types.push_str("<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>");
    types.push_str("<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>");
    types.push_str("<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>");
    if header {
        types.push_str("<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>");
    }
    if footer {
        types.push_str("<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>");
    }
    if !d.comments.is_empty() {
        types.push_str("<Override PartName=\"/word/comments.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/>");
    }
    types.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    types.push_str("</Types>");

    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";

    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:language>{}</dc:language></cp:coreProperties>",
        esc(book.title.trim()),
        esc(book.author.trim()),
        word_lang(book.language.as_deref())
    );

    let mut z = Zip::new();
    z.deflated("[Content_Types].xml", types.as_bytes());
    z.deflated("_rels/.rels", root_rels.as_bytes());
    z.deflated("docProps/core.xml", core.as_bytes());
    z.deflated("word/document.xml", document.as_bytes());
    z.deflated("word/_rels/document.xml.rels", rels.as_bytes());
    z.deflated("word/styles.xml", styles(book, o).as_bytes());
    z.deflated("word/settings.xml", settings(o).as_bytes());
    if header {
        z.deflated("word/header1.xml", running_head(book).as_bytes());
    }
    if footer {
        z.deflated("word/footer1.xml", page_number_footer().as_bytes());
    }
    if !d.comments.is_empty() {
        z.deflated("word/comments.xml", comments(&d.comments).as_bytes());
    }
    z.finish()
}

impl Doc<'_> {
    fn title_page(&mut self, book: &Book) {
        let title = book.title.trim();
        let author = book.author.trim();
        if self.o.style == Style::Manuscript {
            // Shunn: the name top left, the length top right, the title halfway down.
            let words = book.rounded_words();
            self.body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"TitleInfo\"/><w:tabs><w:tab w:val=\"right\" w:pos=\"{}\"/></w:tabs></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r><w:r><w:tab/><w:t xml:space=\"preserve\">about {} words</w:t></w:r></w:p>",
                twips(self.o.paper.size().0 - 2. * self.o.paper.margin()),
                esc(author),
                group(words)
            ));
            let (_, h) = self.o.paper.size();
            let before = twips((h - 2. * self.o.paper.margin()) / 2. - 60.);
            self.body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"Title\"/><w:spacing w:before=\"{before}\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                esc(&title.to_uppercase())
            ));
            if !author.is_empty() {
                self.body.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Subtitle\"/></w:pPr><w:r><w:t xml:space=\"preserve\">by {}</w:t></w:r></w:p>", esc(author)));
            }
        } else {
            let (_, h) = self.o.paper.size();
            let before = twips((h - 2. * self.o.paper.margin()) / 3.);
            self.body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"Title\"/><w:spacing w:before=\"{before}\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                esc(title)
            ));
            if !author.is_empty() {
                self.body.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Subtitle\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(author)));
            }
        }
    }

    /// `first`: the chapter's opening (a paragraph right after its heading starts flush).
    fn blocks(&mut self, bs: &[Block], ids: &[Option<usize>], depth: usize, first: bool) {
        let mut after_heading = first;
        for b in bs {
            match b {
                Block::Heading { level, inlines } => {
                    let style = format!("Heading{}", (*level).clamp(1, 3));
                    self.para(&style, inlines, ids, None);
                    after_heading = true;
                    continue;
                }
                Block::Paragraph(inlines) => {
                    let style = if depth > 0 {
                        "Quote"
                    } else if after_heading && self.o.text.paragraphs == Paragraphs::Indented {
                        "FirstParagraph"
                    } else {
                        "Normal"
                    };
                    self.para(style, inlines, ids, None);
                }
                Block::Quote(inner) => self.blocks(inner, ids, depth + 1, false),
                Block::List { start, items } => {
                    for (i, item) in items.iter().enumerate() {
                        let marker = match start {
                            Some(n) => format!("{}.", n + i as u64),
                            None => "•".to_string(),
                        };
                        let mut marked = false;
                        for b in item {
                            match b {
                                Block::Paragraph(inl) | Block::Heading { inlines: inl, .. } => {
                                    let m = (!marked).then(|| marker.clone());
                                    marked = true;
                                    self.para("ListParagraph", inl, ids, m);
                                }
                                other => self.blocks(std::slice::from_ref(other), ids, depth + 1, false),
                            }
                        }
                    }
                }
                Block::Code(s) => {
                    for line in s.split('\n') {
                        self.para("Code", &[Inline::Run(Run { text: line.into(), code: true, ..Default::default() })], ids, None);
                    }
                }
                Block::Rule => {
                    let mark = if self.o.style == Style::Manuscript { "#" } else { "*\u{a0}\u{a0}\u{a0}*\u{a0}\u{a0}\u{a0}*" };
                    self.para("SceneBreak", &[Inline::Run(Run::plain(mark))], ids, None);
                    after_heading = self.o.text.paragraphs == Paragraphs::Indented;
                    continue;
                }
            }
            after_heading = false;
        }
    }

    fn para(&mut self, style: &str, inlines: &[Inline], ids: &[Option<usize>], marker: Option<String>) {
        self.body.push_str("<w:p><w:pPr>");
        self.body.push_str(&format!("<w:pStyle w:val=\"{style}\"/>"));
        if std::mem::take(&mut self.break_before) {
            self.body.push_str("<w:pageBreakBefore/>");
        }
        self.body.push_str("</w:pPr>");
        if let Some(m) = marker {
            self.body.push_str(&format!("<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r><w:r><w:tab/></w:r>", esc(&m)));
        }
        for i in inlines {
            match i {
                Inline::Run(r) => self.run(r),
                Inline::Break => self.body.push_str("<w:r><w:br/></w:r>"),
                Inline::NoteStart(n) => {
                    if let Some(Some(id)) = ids.get(*n) {
                        self.body.push_str(&format!("<w:commentRangeStart w:id=\"{id}\"/>"));
                        self.open.push(*id);
                    }
                }
                Inline::NoteEnd(n) => {
                    if let Some(Some(id)) = ids.get(*n) {
                        if let Some(at) = self.open.iter().position(|x| x == id) {
                            self.open.remove(at);
                            self.body.push_str(&format!(
                                "<w:commentRangeEnd w:id=\"{id}\"/><w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:commentReference w:id=\"{id}\"/></w:r>"
                            ));
                        }
                    }
                }
            }
        }
        self.body.push_str("</w:p>");
    }

    fn run(&mut self, r: &Run) {
        let mut props = String::new();
        if r.link.is_some() {
            props.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
        }
        if r.code {
            props.push_str("<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\" w:cs=\"Courier New\"/>");
        }
        if r.bold {
            props.push_str("<w:b/>");
        }
        if r.italic {
            // A manuscript underlines what print would set in italics? Shunn now prefers italics.
            props.push_str("<w:i/>");
        }
        if r.strike {
            props.push_str("<w:strike/>");
        }
        let text = format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", if props.is_empty() { String::new() } else { format!("<w:rPr>{props}</w:rPr>") }, esc(&r.text));
        match &r.link {
            Some(l) => {
                let i = self.links.iter().position(|x| x == l).unwrap_or_else(|| {
                    self.links.push(l.clone());
                    self.links.len() - 1
                });
                self.body.push_str(&format!("<w:hyperlink r:id=\"rIdLink{i}\" w:history=\"1\">{text}</w:hyperlink>"));
            }
            None => self.body.push_str(&text),
        }
    }
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

fn styles(book: &Book, o: &Options) -> String {
    let t = &o.text;
    let font = esc(&t.font);
    let size = (t.size * 2.).round() as i64;
    let line = (t.line_spacing * 240.).round() as i64;
    let jc = match t.align {
        Align::Justified => "both",
        Align::Left => "left",
    };
    let manuscript = o.style == Style::Manuscript;
    let book_style = o.style == Style::Book;
    let indent = if manuscript { 720 } else { twips(t.size * 1.5) };
    let (after, first_line) = match t.paragraphs {
        Paragraphs::Spaced => (twips(t.size), 0),
        Paragraphs::Indented => (0, indent),
    };
    let heading_font = t.heading_font.as_deref().filter(|f| crate::style::word_safe(f)).map(esc);
    let hfont = heading_font.map(|f| format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>")).unwrap_or_default();
    let lang = word_lang(book.language.as_deref());
    let (_, h) = o.paper.size();
    // A manuscript's chapter starts a third of the way down its page.
    let chapter_before = if manuscript { twips((h - 2. * o.paper.margin()) / 3.) } else if book_style { twips(t.size * 6.) } else { twips(t.size * 2.) };
    let h1 = if manuscript {
        format!("<w:pPr><w:keepNext/><w:spacing w:before=\"{chapter_before}\" w:after=\"{}\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr>{hfont}</w:rPr>", twips(t.size * t.line_spacing))
    } else if book_style {
        format!("<w:pPr><w:keepNext/><w:spacing w:before=\"{chapter_before}\" w:after=\"{}\" w:line=\"240\" w:lineRule=\"auto\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr>{hfont}<w:i/><w:sz w:val=\"{}\"/></w:rPr>", twips(t.size * 3.), size * 7 / 4)
    } else {
        format!("<w:pPr><w:keepNext/><w:spacing w:before=\"{chapter_before}\" w:after=\"{}\" w:line=\"240\" w:lineRule=\"auto\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr>{hfont}<w:b/><w:sz w:val=\"{}\"/></w:rPr>", twips(t.size), size * 3 / 2)
    };
    let sub = |lvl: usize, scale: i64| {
        if manuscript {
            format!("<w:pPr><w:keepNext/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:outlineLvl w:val=\"{lvl}\"/></w:pPr><w:rPr>{hfont}</w:rPr>")
        } else {
            format!(
                "<w:pPr><w:keepNext/><w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"240\" w:lineRule=\"auto\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/><w:outlineLvl w:val=\"{lvl}\"/></w:pPr><w:rPr>{hfont}<w:b/><w:sz w:val=\"{}\"/></w:rPr>",
                twips(t.size * 1.2),
                twips(t.size * 0.6),
                size * scale / 4
            )
        }
    };
    let mut s = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles {W_NS}>");
    s.push_str(&format!(
        "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:eastAsia=\"{font}\" w:cs=\"{font}\"/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/><w:lang w:val=\"{lang}\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"{after}\" w:line=\"{line}\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>"
    ));
    let style = |id: &str, name: &str, based: Option<&str>, body: &str| {
        format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"{}><w:name w:val=\"{name}\"/>{}<w:qFormat/>{body}</w:style>",
            if id == "Normal" { " w:default=\"1\"" } else { "" },
            based.map(|b| format!("<w:basedOn w:val=\"{b}\"/><w:next w:val=\"Normal\"/>")).unwrap_or_default()
        )
    };
    s.push_str(&style("Normal", "Normal", None, &format!("<w:pPr><w:ind w:firstLine=\"{first_line}\"/><w:jc w:val=\"{jc}\"/></w:pPr>")));
    s.push_str(&style("FirstParagraph", "First Paragraph", Some("Normal"), "<w:pPr><w:ind w:firstLine=\"0\"/></w:pPr>"));
    s.push_str(&style("Heading1", "heading 1", Some("Normal"), &h1));
    s.push_str(&style("Heading2", "heading 2", Some("Normal"), &sub(1, 5)));
    s.push_str(&style("Heading3", "heading 3", Some("Normal"), &sub(2, 4)));
    s.push_str(&style("Title", "Title", Some("Normal"), &format!("<w:pPr><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:spacing w:after=\"{}\"/></w:pPr><w:rPr>{}</w:rPr>", twips(t.size * t.line_spacing), if manuscript { String::new() } else { format!("<w:sz w:val=\"{}\"/>", size * 2) })));
    s.push_str(&style("Subtitle", "Subtitle", Some("Normal"), &format!("<w:pPr><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr>{}</w:rPr>", if manuscript { "" } else { "<w:i/>" })));
    s.push_str(&style("TitleInfo", "Title Info", Some("Normal"), "<w:pPr><w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>"));
    s.push_str(&style("Part", "Part", Some("Normal"), &format!("<w:pPr><w:keepNext/><w:spacing w:before=\"{}\" w:after=\"{}\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:caps/><w:spacing w:val=\"40\"/></w:rPr>", twips((h - 2. * o.paper.margin()) / 3.), twips(t.size))));
    s.push_str(&style("Quote", "Quote", Some("Normal"), &format!("<w:pPr><w:ind w:left=\"{}\" w:right=\"{}\" w:firstLine=\"0\"/></w:pPr>", 720, if manuscript { 0 } else { 360 })));
    s.push_str(&style("ListParagraph", "List Paragraph", Some("Normal"), "<w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"720\"/></w:tabs><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr>"));
    s.push_str(&style("Code", "Code", Some("Normal"), "<w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:ind w:left=\"360\" w:firstLine=\"0\"/><w:jc w:val=\"left\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\" w:cs=\"Courier New\"/><w:sz w:val=\"20\"/></w:rPr>"));
    s.push_str(&style("SceneBreak", "Scene Break", Some("Normal"), &format!("<w:pPr><w:spacing w:before=\"{}\" w:after=\"{}\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/></w:pPr>", if manuscript { 0 } else { twips(t.size) }, if manuscript { 0 } else { twips(t.size) })));
    s.push_str(&style("CommentText", "annotation text", Some("Normal"), "<w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/></w:rPr>"));
    s.push_str("<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"2F5496\"/><w:u w:val=\"single\"/></w:rPr></w:style>");
    s.push_str("<w:style w:type=\"character\" w:styleId=\"CommentReference\"><w:name w:val=\"annotation reference\"/><w:rPr><w:sz w:val=\"16\"/></w:rPr></w:style>");
    s.push_str("</w:styles>");
    s
}

fn settings(o: &Options) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings {W_NS}>{}<w:defaultTabStop w:val=\"720\"/>{}<w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>",
        if o.text.hyphenate { "<w:autoHyphenation/>" } else { "" },
        if o.text.hyphenate { "<w:consecutiveHyphenLimit w:val=\"2\"/><w:doNotHyphenateCaps/>" } else { "" }
    )
}

fn field_page() -> &'static str {
    "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>"
}

fn running_head(book: &Book) -> String {
    let title = book.title.trim().to_uppercase();
    let key: String = title.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
    let head = [book.surname().to_string(), key].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" / ");
    let head = if head.is_empty() { String::new() } else { format!("{head} / ") };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:hdr {W_NS}><w:p><w:pPr><w:ind w:firstLine=\"0\"/><w:jc w:val=\"right\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>{}</w:p></w:hdr>",
        esc(&head),
        field_page()
    )
}

fn page_number_footer() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:ftr {W_NS}><w:p><w:pPr><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>{}</w:p></w:ftr>",
        field_page()
    )
}

enum Body {
    Others(Vec<String>),
    /// (comment, author).
    Mine(String, String),
}

fn comments(c: &BTreeMap<usize, Body>) -> String {
    let mut s = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:comments {W_NS}>");
    for (id, body) in c {
        let others = match body {
            Body::Others(o) => o,
            Body::Mine(text, author) => {
                let name = if author.trim().is_empty() { "Author" } else { author.trim() };
                let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect();
                s.push_str(&format!("<w:comment w:id=\"{id}\" w:author=\"{}\" w:initials=\"{}\">", esc(name), esc(&initials)));
                for (i, line) in text.trim().split('\n').enumerate() {
                    let r = if i == 0 { "<w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:annotationRef/></w:r>" } else { "" };
                    s.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"CommentText\"/></w:pPr>{r}<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(line)));
                }
                s.push_str("</w:comment>");
                continue;
            }
        };
        s.push_str(&format!("<w:comment w:id=\"{id}\" w:author=\"Dostoevsky\" w:initials=\"D\">"));
        let head = if others.len() == 1 { "Other version".to_string() } else { format!("Other versions ({})", others.len()) };
        s.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"CommentText\"/></w:pPr><w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:annotationRef/></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
            esc(&head)
        ));
        for o in others {
            for (i, line) in o.trim().split('\n').enumerate() {
                let prefix = if i == 0 { "· " } else { "  " };
                s.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"CommentText\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}{}</w:t></w:r></w:p>", prefix, esc(line)));
            }
        }
        s.push_str("</w:comment>");
    }
    s.push_str("</w:comments>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Paper, Text};
    use crate::{zip, Chapter, Note};

    fn part<'a>(entries: &'a [(String, Vec<u8>)], name: &str) -> &'a str {
        std::str::from_utf8(&entries.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("{name}")).1).unwrap()
    }

    fn crime() -> Book {
        let md = "# Chapter One\n\nOn an exceptionally hot evening early in July a young man came out.\n\nHe had *successfully* avoided his landlady.\n\n---\n\nAfter the break.\n".to_string();
        let b = md.find("exceptionally hot").unwrap();
        let notes = vec![Note { range: b..b + "exceptionally hot".len(), others: vec!["stifling".into(), "close and airless".into()], comment: None }];
        Book {
            title: "Crime and Punishment".into(),
            author: "Fyodor Dostoevsky".into(),
            language: Some("ru".into()),
            chapters: vec![
                Chapter { markdown: md, notes, ..Default::default() },
                Chapter { markdown: "# Chapter Two\n\nHe was not used to crowds.\n".into(), ..Default::default() },
            ],
        }
    }

    /// COM-11
    #[test]
    fn the_writers_comments_become_word_comments() {
        let mut book = crime();
        let md = book.chapters[0].markdown.clone();
        let b = md.find("young man").unwrap();
        book.chapters[0].notes.push(Note { range: b..b + 9, others: Vec::new(), comment: Some(("Name him?\nNot yet.".into(), None)) });
        let mut o = Options::new(Style::Page, Text::manuscript(), Paper::Letter);
        assert!(!zip::read(&write(&book, &o)).unwrap().iter().any(|(n, _)| n == "word/comments.xml"));
        o.my_comments = true;
        o.comment_author = "Anna Grigorievna".into();
        let entries = zip::read(&write(&book, &o)).unwrap();
        let c = part(&entries, "word/comments.xml");
        assert!(c.contains("w:author=\"Anna Grigorievna\" w:initials=\"AG\""), "{c}");
        assert!(c.contains(">Name him?<") && c.contains(">Not yet.<"));
        assert!(!c.contains("Other version"), "the other versions stay out unless asked");
        let doc = part(&entries, "word/document.xml");
        assert_eq!(doc.matches("<w:commentRangeStart").count(), 1);
    }

    #[test]
    fn a_manuscript_has_its_title_page_running_head_and_styles() {
        let o = Options::new(Style::Manuscript, Text::manuscript(), Paper::Letter);
        let entries = zip::read(&write(&crime(), &o)).unwrap();
        let doc = part(&entries, "word/document.xml");
        assert!(doc.contains("CRIME AND PUNISHMENT"));
        assert!(doc.contains("by Fyodor Dostoevsky"));
        assert!(doc.contains("about 100 words"));
        assert!(doc.contains("<w:titlePg/>"));
        assert!(doc.contains("<w:pageBreakBefore/>"));
        assert!(doc.contains("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>"));
        assert!(doc.contains(">#<"), "a scene break is a #");
        let styles = part(&entries, "word/styles.xml");
        assert!(styles.contains("w:ascii=\"Times New Roman\""));
        assert!(styles.contains("<w:sz w:val=\"24\"/>"));
        assert!(styles.contains("w:line=\"480\""));
        assert!(styles.contains("<w:lang w:val=\"ru-RU\"/>"));
        let head = part(&entries, "word/header1.xml");
        assert!(head.contains("Dostoevsky / CRIME AND PUNISHMENT / "));
        assert!(head.contains(" PAGE "));
        assert!(!part(&entries, "word/settings.xml").contains("autoHyphenation"));
        assert!(!entries.iter().any(|(n, _)| n == "word/comments.xml"));
    }

    #[test]
    fn justified_and_hyphenated_text_is_set_in_word() {
        let o = Options::new(Style::Book, Text::book(Paper::A5), Paper::A5);
        let entries = zip::read(&write(&crime(), &o)).unwrap();
        assert!(part(&entries, "word/styles.xml").contains("<w:jc w:val=\"both\"/>"));
        assert!(part(&entries, "word/settings.xml").contains("<w:autoHyphenation/>"));
        assert!(part(&entries, "word/footer1.xml").contains(" PAGE "));
        assert!(part(&entries, "word/document.xml").contains("<w:pStyle w:val=\"FirstParagraph\"/>"));
    }

    #[test]
    fn other_versions_become_comments() {
        let mut o = Options::new(Style::Manuscript, Text::manuscript(), Paper::A4);
        o.comments = true;
        let entries = zip::read(&write(&crime(), &o)).unwrap();
        let doc = part(&entries, "word/document.xml");
        let start = doc.find("<w:commentRangeStart w:id=\"0\"/>").unwrap();
        let end = doc.find("<w:commentRangeEnd w:id=\"0\"/>").unwrap();
        assert!(start < end);
        assert!(doc[start..end].contains("exceptionally hot"));
        assert!(doc.contains("<w:commentReference w:id=\"0\"/>"));
        let c = part(&entries, "word/comments.xml");
        assert!(c.contains("Other versions (2)"));
        assert!(c.contains("· stifling"));
        assert!(c.contains("· close and airless"));
        assert!(part(&entries, "[Content_Types].xml").contains("/word/comments.xml"));
        assert!(part(&entries, "word/_rels/document.xml.rels").contains("comments.xml"));
    }

    #[test]
    fn links_lists_and_quotes_survive() {
        let b = Book::page("T", "A", "See [the site](https://dostoevsky.app) & more.\n\n- one\n- two\n\n> quoted\n".into());
        let o = Options::new(Style::Page, Text::page("IBM Plex Mono", 19., 1.8, false, false, None), Paper::A4);
        let entries = zip::read(&write(&b, &o)).unwrap();
        let doc = part(&entries, "word/document.xml");
        assert!(doc.contains("<w:hyperlink r:id=\"rIdLink0\""));
        assert!(doc.contains("&amp; more"));
        assert!(doc.contains("<w:pStyle w:val=\"ListParagraph\"/>"));
        assert!(doc.contains("<w:pStyle w:val=\"Quote\"/>"));
        assert!(part(&entries, "word/_rels/document.xml.rels").contains("Target=\"https://dostoevsky.app\" TargetMode=\"External\""));
    }

    #[test]
    fn an_empty_page_is_still_a_document() {
        let o = Options::new(Style::Page, Text::manuscript(), Paper::A4);
        let entries = zip::read(&write(&Book::page("", "", String::new()), &o)).unwrap();
        assert!(part(&entries, "word/document.xml").contains("<w:p/>"));
    }
}
