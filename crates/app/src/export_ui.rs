use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use export::{Book, Chapter, ChapterTitles, Options, Paper, Paragraphs, Style, Text};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trc, trn};
use serde::{Deserialize, Serialize};

use crate::actions::*;
use crate::format;
use crate::prefs::{Alignment, Prefs};
use crate::project::Kind;
use crate::sheets::{button, Sheet};
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Word,
    Epub,
    Html,
    Markdown,
    Text,
}

impl Format {
    pub const ALL: [Format; 6] = [Format::Pdf, Format::Word, Format::Epub, Format::Html, Format::Markdown, Format::Text];

    fn key(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Word => "docx",
            Format::Epub => "epub",
            Format::Html => "html",
            Format::Markdown => "md",
            Format::Text => "txt",
        }
    }

    fn from_key(k: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.key() == k)
    }

    pub fn extension(self) -> &'static str {
        self.key()
    }

    fn name(self) -> &'static str {
        match self {
            Format::Pdf => "PDF",
            Format::Word => "Word",
            Format::Epub => "ePub",
            Format::Html => tr!("Web page"),
            Format::Markdown => "Markdown",
            Format::Text => tr!("Plain text"),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Format::Pdf => tr!("print, send"),
            Format::Word => tr!("for an editor"),
            Format::Epub => tr!("an ebook"),
            Format::Html => tr!("one .html"),
            Format::Markdown => tr!("other apps"),
            Format::Text => tr!("no markup"),
        }
    }

    /// Markdown and plain text have no type to set.
    pub fn styles(self) -> &'static [Style] {
        match self {
            Format::Pdf | Format::Word => &[Style::Page, Style::Manuscript, Style::Book],
            Format::Epub | Format::Html => &[Style::Page, Style::Book],
            Format::Markdown | Format::Text => &[],
        }
    }

    pub fn default_style(self) -> Style {
        match self {
            Format::Word => Style::Manuscript,
            Format::Epub => Style::Book,
            _ => Style::Page,
        }
    }

    fn paged(self) -> bool {
        matches!(self, Format::Pdf | Format::Word)
    }
}

fn style_name(s: Style) -> &'static str {
    match s {
        Style::Page => tr!("Page"),
        Style::Manuscript => tr!("Manuscript"),
        Style::Book => tr!("Book"),
    }
}

const FONTS: [&str; 11] = ["IBM Plex Mono", "JetBrains Mono", "iA Writer Quattro S", "Literata", "Iowan Old Style", "Charter", "Palatino", "Georgia", "Times New Roman", "Courier New", "Arial"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct TextMemo {
    font: String,
    size: f32,
    line_spacing: f32,
    justified: bool,
    hyphenate: bool,
    indented: bool,
}

impl TextMemo {
    fn of(t: &Text) -> TextMemo {
        TextMemo { font: t.font.clone(), size: t.size, line_spacing: t.line_spacing, justified: t.align == export::Align::Justified, hyphenate: t.hyphenate, indented: t.paragraphs == Paragraphs::Indented }
    }

    fn apply(&self, mut t: Text) -> Text {
        if !self.font.is_empty() {
            t.font = self.font.clone();
        }
        if self.size > 0. {
            t.size = self.size;
        }
        if self.line_spacing > 0. {
            t.line_spacing = self.line_spacing;
        }
        t.align = if self.justified { export::Align::Justified } else { export::Align::Left };
        t.hyphenate = self.hyphenate;
        t.paragraphs = if self.indented { Paragraphs::Indented } else { Paragraphs::Spaced };
        t
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Memory {
    format: Option<String>,
    style: Option<String>,
    paper: Option<String>,
    whole_book: bool,
    title_page: Option<bool>,
    page_numbers: Option<bool>,
    comments: bool,
    my_comments: bool,
    contents: Option<bool>,
    file_titles: bool,
    /// By style key.
    texts: BTreeMap<String, TextMemo>,
    /// A single file's (a book's are in its `.dostoevsky`).
    title: Option<String>,
    author: Option<String>,
}

fn memory_path(key: &Path) -> Option<PathBuf> {
    crate::files::support_dir().map(|d| d.join("Export").join(format!("{}.json", crate::files::path_key(key))))
}

fn last_author_path() -> Option<PathBuf> {
    crate::files::support_dir().map(|d| d.join("Export").join("author.txt"))
}

fn load_memory(key: Option<&Path>) -> Memory {
    key.and_then(memory_path).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// The sheet remembers on Save.
fn remembered(key: Option<&Path>) -> bool {
    key.and_then(memory_path).is_some_and(|p| p.exists())
}

/// Read when the tooltip shows, not on every frame.
pub fn export_tip(in_book: bool, key: Option<&Path>) -> &'static str {
    if !in_book {
        tr!("Export… · this page")
    } else if !remembered(key) || load_memory(key).whole_book {
        tr!("Export… · whole book")
    } else {
        tr!("Export… · this chapter")
    }
}

fn save_memory(key: Option<&Path>, m: &Memory) {
    let Some(p) = key.and_then(memory_path) else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(m) {
        let _ = std::fs::write(p, s);
    }
}

fn user_name() -> String {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        objc2_foundation::NSFullUserName().to_string()
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        String::new()
    }
}

fn region_paper() -> Paper {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        #[allow(deprecated)]
        let code = objc2_foundation::NSLocale::currentLocale().countryCode().map(|c| c.to_string()).unwrap_or_default();
        if ["US", "CA", "MX", "CL", "CO", "VE", "PH", "PR"].contains(&code.as_str()) {
            return Paper::Letter;
        }
    }
    Paper::A4
}

#[derive(Default)]
pub struct Preview {
    pub pdf: Option<Arc<crate::pdf::Pdf>>,
    pub page: usize,
    pub image: Option<Arc<RenderImage>>,
    /// In pixels.
    pub image_size: (u32, u32),
    pub text: Option<String>,
    pub busy: bool,
    pub version: u64,
}

pub struct ExportForm {
    pub whole_book: bool,
    pub format: Format,
    pub style: Style,
    pub text: Text,
    pub text_open: bool,
    pub font_open: bool,
    pub paper: Paper,
    pub title_page: bool,
    pub page_numbers: bool,
    pub comments: bool,
    pub my_comments: bool,
    pub contents: bool,
    pub file_titles: bool,
    pub cover: Option<(PathBuf, Vec<u8>, String)>,
    pub title: Entity<TextView>,
    pub author: Entity<TextView>,
    pub page_book: Book,
    pub whole: Option<Book>,
    pub preview: Preview,
    memory: Memory,
    _subs: Vec<Subscription>,
}

impl ExportForm {
    pub fn book(&self) -> &Book {
        match (&self.whole, self.whole_book) {
            (Some(b), true) => b,
            _ => &self.page_book,
        }
    }

    pub fn chapter_titles(&self) -> ChapterTitles {
        if self.file_titles && self.whole_book {
            ChapterTitles::FileNames
        } else {
            ChapterTitles::Headings
        }
    }
}

fn default_text(style: Style, paper: Paper, p: &Prefs) -> Text {
    match style {
        Style::Manuscript => Text::manuscript(),
        Style::Book => Text::book(paper),
        Style::Page => {
            let hand = p.hand_headings.then(|| p.hand_font.family());
            let mut t = Text::page(&p.font_family, p.font_size, p.line_spacing, p.alignment == Alignment::Justified, p.hyphenation, hand);
            if p.paragraphs == crate::prefs::ParagraphLook::Indented {
                t.paragraphs = Paragraphs::Indented;
            }
            t
        }
    }
}

fn title_from_name(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let t = export::title_from_stem(&stem);
    if t.is_empty() {
        stem
    } else {
        t
    }
}

fn first_heading(md: &str) -> Option<String> {
    let line = md.lines().find(|l| !l.trim().is_empty())?;
    let t = line.trim_start().strip_prefix('#')?.trim_start_matches('#').trim();
    (!t.is_empty()).then(|| t.to_string())
}

type Final = (String, Vec<doc_model::AltNote>, Vec<(std::ops::Range<usize>, doc_model::CommentInfo)>);

fn final_of(doc: &doc_model::Document) -> Final {
    let (md, notes) = doc_format::clean_with_notes(doc);
    (md, notes, doc.clean_comments())
}

fn chapter_of((md, notes, comments): Final, file_title: String, part: Option<String>) -> Chapter {
    // Notes are chars into the final text, as export wants them.
    let mut notes: Vec<export::Note> = notes.into_iter().map(|n| export::Note { range: n.range, others: n.others, comment: None }).collect();
    notes.extend(comments.into_iter().map(|(range, c)| export::Note { range, others: Vec::new(), comment: Some((c.text, c.author)) }));
    Chapter { part, file_title, markdown: md, notes }
}

fn language_of(book: &Book) -> Option<String> {
    let mut sample = String::new();
    for c in &book.chapters {
        sample.push_str(&c.markdown);
        if sample.len() > 40_000 {
            break;
        }
    }
    prose::detect(&sample).map(|l| l.code().to_string())
}

fn group_thousands(n: usize) -> String {
    crate::sheets::group_thousands(n)
}

impl Workspace {
    fn export_key(&self, cx: &App) -> Option<PathBuf> {
        self.project_root().map(Path::to_path_buf).or_else(|| self.model.read(cx).path.clone())
    }

    fn in_book(&self) -> bool {
        self.project_kind() == Some(Kind::Book)
    }

    #[cfg(test)]
    pub(crate) fn export_page_book_for_test(&self, cx: &App) -> Book {
        self.export_page_book(cx)
    }

    fn export_page_book(&self, cx: &App) -> Book {
        let m = self.model.read(cx);
        let file_title = m.path.as_deref().map(title_from_name).unwrap_or_default();
        let mut b = Book { chapters: vec![chapter_of(final_of(&m.doc), file_title, None)], ..Default::default() };
        b.language = language_of(&b);
        b
    }

    /// Unsaved words included; no notes, no drawer.
    fn export_whole_book(&self, cx: &App) -> Option<Book> {
        let p = self.project.as_ref().filter(|p| p.tree.kind == Kind::Book)?;
        let page = self.model.read(cx);
        let mut chapters = Vec::new();
        for g in &p.tree.main {
            let mut part = g.title.clone();
            for e in &g.entries {
                let text = if page.path.as_deref() == Some(e.path.as_path()) {
                    final_of(&page.doc)
                } else if let Some(q) = p.parked.get(&e.path) {
                    final_of(&q.model.doc)
                } else {
                    std::fs::read_to_string(&e.path).map(|t| final_of(&format::load(&t).doc)).unwrap_or_default()
                };
                let label = e.label.trim();
                let file_title = {
                    let mut c = label.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                };
                chapters.push(chapter_of(text, file_title, part.take()));
            }
        }
        let mut b = Book { chapters, ..Default::default() };
        b.language = language_of(&b);
        Some(b)
    }

    pub fn copy_final_markdown(&mut self, _: &CopyFinalMarkdown, _: &mut Window, cx: &mut Context<Self>) {
        let book = self.export_page_book(cx);
        let md = export::text::markdown(&book, ChapterTitles::Headings);
        let html = export::html::fragment(&book, ChapterTitles::Headings);
        let words = english::word_count(&md);
        crate::clipboard::write_rich(&md, &html, cx);
        self.flash_status(trn!("Copied the final text · {n} word", "Copied the final text · {n} words", words, n = group_thousands(words)), cx);
    }

    pub fn export(&mut self, _: &Export, window: &mut Window, cx: &mut Context<Self>) {
        self.open_export(false, window, cx);
    }

    /// `(in_book, key)` for [`export_tip`].
    pub(crate) fn export_tip_source(&self, cx: &App) -> (bool, Option<PathBuf>) {
        (self.in_book(), self.export_key(cx))
    }

    /// From the panel, a never-exported book starts on the whole book; else on the last choice.
    pub(crate) fn open_export(&mut self, from_panel: bool, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.sheet, Some(Sheet::Export(_))) {
            self.close_sheet(window, cx);
            return;
        }
        let key = self.export_key(cx);
        let first = !remembered(key.as_deref());
        let memory = load_memory(key.as_deref());
        let p = Prefs::get(cx).clone();
        let page_book = self.export_page_book(cx);
        let whole = self.export_whole_book(cx);
        let format = memory.format.as_deref().and_then(Format::from_key).unwrap_or(Format::Pdf);
        let style = memory.style.as_deref().and_then(Style::from_key).filter(|s| format.styles().contains(s)).unwrap_or(format.default_style());
        let paper = memory.paper.as_deref().and_then(Paper::from_key).unwrap_or_else(region_paper);
        let text = memory.texts.get(style.key()).map(|m| m.apply(default_text(style, paper, &p))).unwrap_or_else(|| default_text(style, paper, &p));
        let settings = self.project_root().and_then(crate::project::read_settings);
        let title = settings
            .as_ref()
            .and_then(|s| s.title.clone())
            .or_else(|| memory.title.clone())
            .or_else(|| self.in_book().then(|| self.project_root().map(title_from_name)).flatten())
            .or_else(|| first_heading(&page_book.chapters[0].markdown))
            .or_else(|| self.model.read(cx).path.as_deref().map(title_from_name))
            .unwrap_or_default();
        let author = settings
            .as_ref()
            .and_then(|s| s.author.clone())
            .or_else(|| memory.author.clone())
            .or_else(|| last_author_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
            .unwrap_or_else(user_name);
        let model = self.model.clone();
        let field = |text: String, placeholder: &'static str, cx: &mut Context<Self>| {
            let model = model.clone();
            cx.new(move |cx| {
                let mut f = TextView::new(Mode::Field, model, cx);
                f.placeholder = placeholder.into();
                f.set_field_text(&text, cx);
                f
            })
        };
        let title_f = field(title, tr!("title"), cx);
        let author_f = field(author, tr!("author"), cx);
        let mut subs = Vec::new();
        for f in [&title_f, &author_f] {
            subs.push(cx.subscribe_in(f, window, |this, _, e: &ViewEvent, window, cx| match e {
                ViewEvent::Confirm => this.export_save(window, cx),
                ViewEvent::Cancel => this.close_sheet(window, cx),
                ViewEvent::Edited => this.export_refresh(cx),
                _ => {}
            }));
        }
        let whole_book = whole.is_some() && (memory.whole_book || (from_panel && first));
        let form = ExportForm {
            whole_book,
            format,
            style,
            text,
            text_open: false,
            font_open: false,
            paper,
            title_page: memory.title_page.unwrap_or(style == Style::Manuscript),
            page_numbers: memory.page_numbers.unwrap_or(true),
            comments: memory.comments,
            my_comments: memory.my_comments,
            contents: memory.contents.unwrap_or(true),
            file_titles: memory.file_titles,
            cover: None,
            title: title_f,
            author: author_f,
            page_book,
            whole,
            preview: Preview::default(),
            memory,
            _subs: subs,
        };
        self.open_sheet(Sheet::Export(Box::new(form)), window, cx);
        self.export_refresh(cx);
    }

    pub fn print(&mut self, _: &Print, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Sheet::Export(f)) = &self.sheet {
            if let Some(pdf) = f.preview.pdf.clone().filter(|_| f.format == Format::Pdf) {
                let title = f.title.read(cx).field_text().to_string();
                self.print_pdf(pdf.bytes.clone(), title, window, cx);
                return;
            }
        }
        let key = self.export_key(cx);
        let memory = load_memory(key.as_deref());
        let p = Prefs::get(cx).clone();
        let style = memory.style.as_deref().and_then(Style::from_key).unwrap_or(Style::Page);
        let paper = memory.paper.as_deref().and_then(Paper::from_key).unwrap_or_else(region_paper);
        let text = memory.texts.get(style.key()).map(|m| m.apply(default_text(style, paper, &p))).unwrap_or_else(|| default_text(style, paper, &p));
        let mut book = self.export_page_book(cx);
        book.title = memory.title.clone().or_else(|| first_heading(&book.chapters[0].markdown)).unwrap_or_default();
        book.author = memory.author.clone().unwrap_or_default();
        let mut o = Options::new(style, text, paper);
        o.title_page = false;
        o.page_numbers = memory.page_numbers.unwrap_or(true);
        let title = book.title.clone();
        cx.spawn_in(window, async move |this, cx| {
            let pdf = cx.background_spawn(async move { crate::pdf::render(&book, &o) }).await;
            let _ = this.update_in(cx, |this, window, cx| this.print_pdf(pdf.bytes, title, window, cx));
        })
        .detach();
    }

    /// The Mac's print panel on the window.
    #[cfg(target_os = "macos")]
    fn print_pdf(&mut self, pdf: Vec<u8>, title: String, window: &mut Window, _: &mut Context<Self>) {
        crate::pdf::print(&pdf, &title, window);
    }

    /// Off the Mac the system prints the PDF, or its viewer opens it (dsky-fbso.16); the status
    /// line says which, or why nothing could.
    #[cfg(not(target_os = "macos"))]
    fn print_pdf(&mut self, pdf: Vec<u8>, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let rx = crate::pdf::print(pdf, title);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(r) = rx.recv().await else { return };
            let _ = this.update(cx, |this, cx| match r {
                Ok(crate::pdf::Printed::Sent) => this.flash_status(tr!("Sent to the printer"), cx),
                Ok(crate::pdf::Printed::Opened) => this.flash_status(tr!("Opened the PDF: print it from there"), cx),
                Ok(crate::pdf::Printed::Cancelled) => {}
                Err(e) => this.flash_status(tr!("Couldn't print: {error}", error = e), cx),
            });
        })
        .detach();
    }

    fn export_form(&mut self) -> Option<&mut ExportForm> {
        match &mut self.sheet {
            Some(Sheet::Export(f)) => Some(f),
            _ => None,
        }
    }

    pub(crate) fn change_export(&mut self, f: impl FnOnce(&mut ExportForm, &Prefs), cx: &mut Context<Self>) {
        let p = Prefs::get(cx).clone();
        if let Some(form) = self.export_form() {
            f(form, &p);
            self.export_refresh(cx);
        }
    }

    pub(crate) fn set_export_format(&mut self, format: Format, cx: &mut Context<Self>) {
        self.change_export(
            |f, p| {
                if f.format == format {
                    return;
                }
                f.format = format;
                let style = if format.styles().contains(&format.default_style()) { format.default_style() } else { f.style };
                set_style(f, style, p);
                f.text_open = false;
                f.preview.page = 0;
            },
            cx,
        );
    }

    pub(crate) fn set_export_style(&mut self, style: Style, cx: &mut Context<Self>) {
        self.change_export(|f, p| set_style(f, style, p), cx);
    }

    fn export_options(f: &ExportForm, preview: bool) -> Options {
        let mut o = Options::new(f.style, f.text.clone(), f.paper);
        o.title_page = f.title_page;
        o.page_numbers = f.page_numbers;
        o.comments = f.comments && f.format == Format::Word;
        o.my_comments = f.my_comments && f.format == Format::Word;
        o.comment_author = user_name();
        o.contents = f.contents;
        o.chapter_titles = f.chapter_titles();
        o.cover = f.cover.as_ref().map(|(_, b, m)| (b.clone(), m.clone()));
        if preview && f.format == Format::Epub {
            // An ebook previews as a small page in its own font; the reader picks theirs.
            o.paper = Paper::A5;
            o.page_numbers = false;
            if f.style == Style::Book {
                o.text.font = "Iowan Old Style".into();
                o.text.size = 11.;
            }
        }
        if f.format == Format::Html {
            o.page_numbers = false;
        }
        o
    }

    fn export_book_now(&self, cx: &App) -> Option<Book> {
        let Some(Sheet::Export(f)) = &self.sheet else { return None };
        let mut b = f.book().clone();
        b.title = f.title.read(cx).field_text().trim().to_string();
        b.author = f.author.read(cx).field_text().trim().to_string();
        Some(b)
    }

    pub(crate) fn export_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(book) = self.export_book_now(cx) else { return };
        let Some(Sheet::Export(f)) = &mut self.sheet else { return };
        f.preview.version += 1;
        let version = f.preview.version;
        match f.format {
            Format::Markdown | Format::Text => {
                let t = if f.format == Format::Markdown { export::text::markdown(&book, f.chapter_titles()) } else { export::text::plain(&book, f.chapter_titles()) };
                f.preview.text = Some(t.lines().take(400).collect::<Vec<_>>().join("\n"));
                f.preview.busy = false;
                cx.notify();
                return;
            }
            _ => f.preview.text = None,
        }
        let o = Self::export_options(f, true);
        let page = f.preview.page;
        f.preview.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(120)).await;
            let current = this.update(cx, |this, _| this.export_form().is_some_and(|f| f.preview.version == version)).unwrap_or(false);
            if !current {
                return;
            }
            let (pdf, image) = cx
                .background_spawn(async move {
                    let pdf = crate::pdf::render(&book, &o);
                    let page = page.min(pdf.pages.saturating_sub(1));
                    let image = crate::pdf::rasterize(&pdf, page, PREVIEW_PX);
                    (pdf, image)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(f) = this.export_form().filter(|f| f.preview.version == version) {
                    f.preview.page = f.preview.page.min(pdf.pages.saturating_sub(1));
                    f.preview.pdf = Some(Arc::new(pdf));
                    f.preview.image_size = image.as_ref().map_or((0, 0), |b| (b.width, b.height));
                    f.preview.image = image.map(render_image);
                    f.preview.busy = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// `page` from 0.
    pub(crate) fn export_turn(&mut self, page: usize, cx: &mut Context<Self>) {
        let Some(f) = self.export_form() else { return };
        let Some(pdf) = f.preview.pdf.clone() else { return };
        let page = page.min(pdf.pages.saturating_sub(1));
        if page == f.preview.page {
            return;
        }
        f.preview.page = page;
        let version = f.preview.version;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let image = cx.background_spawn(async move { crate::pdf::rasterize(&pdf, page, PREVIEW_PX) }).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(f) = this.export_form().filter(|f| f.preview.version == version && f.preview.page == page) {
                    f.preview.image_size = image.as_ref().map_or((0, 0), |b| (b.width, b.height));
                    f.preview.image = image.map(render_image);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn export_remember(&mut self, cx: &mut Context<Self>) {
        let key = self.export_key(cx);
        let root = self.project_root().map(Path::to_path_buf);
        let settings = self.project.as_ref().map(|p| p.tree.settings());
        let Some(Sheet::Export(f)) = &mut self.sheet else { return };
        let title = f.title.read(cx).field_text().trim().to_string();
        let author = f.author.read(cx).field_text().trim().to_string();
        let p = Prefs::get(cx).clone();
        let m = &mut f.memory;
        m.format = Some(f.format.key().into());
        if !f.format.styles().is_empty() {
            m.style = Some(f.style.key().into());
        }
        m.paper = Some(f.paper.key().into());
        m.whole_book = f.whole_book;
        m.title_page = Some(f.title_page);
        m.page_numbers = Some(f.page_numbers);
        m.comments = f.comments;
        m.my_comments = f.my_comments;
        m.contents = Some(f.contents);
        m.file_titles = f.file_titles;
        if !f.style.locked() && !f.format.styles().is_empty() {
            if f.text == default_text(f.style, f.paper, &p) {
                m.texts.remove(f.style.key());
            } else {
                m.texts.insert(f.style.key().into(), TextMemo::of(&f.text));
            }
        }
        // A book's title and author belong to the book; a file's are remembered here.
        match (&root, settings) {
            (Some(root), Some(s)) if s.kind == Kind::Book && crate::project::keeps_settings_file(root, &s) => {
                let old = crate::project::read_settings(root);
                if old.as_ref().and_then(|o| o.title.clone()).unwrap_or_default() != title || old.as_ref().and_then(|o| o.author.clone()).unwrap_or_default() != author {
                    let _ = crate::project::write_settings(root, &crate::project::Settings { kind: s.kind, notes: s.notes, title: Some(title.clone()), author: Some(author.clone()), date: None });
                }
                m.title = None;
                m.author = None;
            }
            _ => {
                m.title = Some(title).filter(|t| !t.is_empty());
                m.author = Some(author.clone()).filter(|a| !a.is_empty());
            }
        }
        let memory = m.clone();
        save_memory(key.as_deref(), &memory);
        if !author.is_empty() {
            if let Some(p) = last_author_path() {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(p, &author);
            }
        }
    }

    fn export_target(&self, cx: &App) -> (PathBuf, String) {
        let Some(Sheet::Export(f)) = &self.sheet else { return (PathBuf::from("."), String::new()) };
        let ext = f.format.extension();
        if let (true, Some(root)) = (f.whole_book, self.project_root()) {
            let dir = root.parent().unwrap_or(root).to_path_buf();
            return (dir, format!("{}.{ext}", crate::project::folder_name(root)));
        }
        let m = self.model.read(cx);
        let dir = m.path.as_ref().and_then(|p| p.parent().map(PathBuf::from)).or_else(|| crate::platform::home().map(|h| h.join("Documents"))).unwrap_or_else(|| PathBuf::from("."));
        let stem = m.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| tr!("Untitled").to_string());
        // Markdown next to its own .md mustn't propose the same name.
        let name = if f.format == Format::Markdown { format!("{stem} ({}).md", tr!("final")) } else { format!("{stem}.{ext}") };
        (dir, name)
    }

    fn export_bytes(&self, cx: &App) -> Option<impl std::future::Future<Output = Vec<u8>> + 'static> {
        let book = self.export_book_now(cx)?;
        let Some(Sheet::Export(f)) = &self.sheet else { return None };
        let o = Self::export_options(f, false);
        let format = f.format;
        let titles = f.chapter_titles();
        let ready = f.preview.pdf.clone().filter(|_| format == Format::Pdf && !f.preview.busy);
        let task = cx.background_spawn(async move {
            match format {
                Format::Pdf => ready.map(|p| p.bytes.clone()).unwrap_or_else(|| crate::pdf::render(&book, &o).bytes),
                Format::Word => export::docx::write(&book, &o),
                Format::Epub => export::epub::write(&book, &o),
                Format::Html => export::html::page(&book, &o).into_bytes(),
                Format::Markdown => export::text::markdown(&book, titles).into_bytes(),
                Format::Text => export::text::plain(&book, titles).into_bytes(),
            }
        });
        Some(task)
    }

    pub(crate) fn export_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.sheet, Some(Sheet::Export(_))) {
            return;
        }
        self.export_remember(cx);
        let (dir, name) = self.export_target(cx);
        let Some(bytes) = self.export_bytes(cx) else { return };
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let bytes = bytes.await;
            let r = crate::files::write_atomic(&path, &bytes);
            let _ = this.update_in(cx, |this, window, cx| match r {
                Ok(()) => {
                    if matches!(this.sheet, Some(Sheet::Export(_))) {
                        this.close_sheet(window, cx);
                    }
                    let shown = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    this.flash_status_reveal(tr!("Exported {name}", name = shown), path, cx);
                }
                Err(e) => this.flash_status(tr!("Couldn't export: {error}", error = e), cx),
            });
        })
        .detach();
    }

    /// A web page's HTML source goes as text, for a blog's HTML field.
    pub(crate) fn export_copy(&mut self, cx: &mut Context<Self>) {
        let Some(book) = self.export_book_now(cx) else { return };
        let Some(Sheet::Export(f)) = &self.sheet else { return };
        let o = Self::export_options(f, false);
        let text = match f.format {
            Format::Markdown => export::text::markdown(&book, f.chapter_titles()),
            Format::Text => export::text::plain(&book, f.chapter_titles()),
            Format::Html => export::html::page(&book, &o),
            _ => return,
        };
        self.export_remember(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.flash_status(tr!("Copied"), cx);
    }

    pub(crate) fn export_choose_cover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(tr!("Choose").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let mime = match ext.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                _ => {
                    let _ = this.update(cx, |this, cx| this.flash_status(tr!("A cover is a .jpg or .png"), cx));
                    return;
                }
            };
            let Ok(bytes) = std::fs::read(&path) else { return };
            let _ = this.update(cx, |this, cx| this.change_export(|f, _| f.cover = Some((path, bytes, mime.into())), cx));
        })
        .detach();
    }

    pub(crate) fn render_export(&mut self, window: &mut Window, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let Some(Sheet::Export(f)) = &self.sheet else { return (div().into_any_element(), div().into_any_element()) };
        let ws = cx.entity().downgrade();
        let in_book = self.in_book();
        let hint = if self.project_root().is_some() { tr!("remembered for this book") } else { tr!("remembered for this file") };
        let title = div()
            .flex()
            .items_center()
            .child(div().font_family(theme::hand()).text_size(theme::hand_px(25.)).text_color(c(theme::accent())).child(tr!("Export")))
            .child(div().flex_1())
            .child(div().text_size(px(11.)).text_color(c(theme::faint())).child(hint))
            .into_any_element();

        let page_words = english::word_count(&f.page_book.chapters[0].markdown);
        let mut what = vec![(false, if in_book { tr!("This chapter").to_string() } else { tr!("This page").to_string() }, trn!("{n} word", "{n} words", page_words, n = group_thousands(page_words)).to_string(), true)];
        match &f.whole {
            Some(b) => {
                let w = b.words();
                what.push((true, tr!("Whole book").to_string(), format!("{} · {}", group_thousands(w), trn!("{n} ch.", "{n} ch.", b.chapters.len(), n = b.chapters.len())), true));
            }
            None => what.push((true, tr!("Whole book").to_string(), tr!("not a book").to_string(), false)),
        }
        let what_seg = seg2("export-what", what, f.whole_book, {
            let ws = ws.clone();
            move |v, cx| {
                let _ = ws.update(cx, |this, cx| this.change_export(|f, _| {
                    f.whole_book = v;
                    f.preview.page = 0;
                }, cx));
            }
        });

        let formats = div().grid().grid_cols(2).gap_1p5().children(Format::ALL.into_iter().map(|fmt| {
            let on = f.format == fmt;
            let ws = ws.clone();
            div()
                .id(("export-format", fmt as usize))
                .role(Role::RadioButton)
                .aria_label(fmt.name())
                .aria_selected(on)
                .px_2p5()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(if on { c(theme::accent()) } else { c(theme::line()) })
                .bg(if on { ca(theme::accent(), 0.12) } else { c(theme::bg()) })
                .cursor_pointer()
                .hover(|d| d.border_color(ca(theme::accent(), 0.6)))
                .on_click(move |_, _, cx| {
                    let _ = ws.update(cx, |this, cx| this.set_export_format(fmt, cx));
                })
                .child(div().text_size(px(12.5)).text_color(if on { c(theme::text()) } else { c(theme::text_2()) }).child(fmt.name()))
                .child(div().text_size(px(10.5)).text_color(c(theme::muted())).child(fmt.hint()))
                .test_support()
        }));

        let styles = f.format.styles();
        let style_seg = (!styles.is_empty()).then(|| {
            let ws = ws.clone();
            seg_wide(
                "export-style",
                styles.iter().map(|s| (*s, style_name(*s).to_string())).collect(),
                f.style,
                move |s, cx| {
                    let _ = ws.update(cx, |this, cx| this.set_export_style(s, cx));
                },
            )
        });

        let rows = if f.text_open { self.export_text_rows(cx) } else { self.export_option_rows(cx) };
        let Some(Sheet::Export(f)) = &self.sheet else { unreachable!() };
        let options_label = if f.text_open {
            let ws = ws.clone();
            div()
                .flex()
                .items_center()
                .child(label(tr!("TEXT")))
                .child(div().flex_1())
                .child(
                    div()
                        .id("export-text-back")
                        .text_size(px(11.5))
                        .text_color(c(theme::accent()))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let _ = ws.update(cx, |this, cx| {
                                if let Some(f) = this.export_form() {
                                    f.text_open = false;
                                    f.font_open = false;
                                }
                                cx.notify();
                            });
                        })
                        .child(tr!("‹ options"))
                        .test_support(),
                )
                .into_any_element()
        } else if rows.is_empty() {
            div().into_any_element()
        } else {
            label(tr!("OPTIONS")).into_any_element()
        };

        let can_copy = matches!(f.format, Format::Markdown | Format::Text | Format::Html);
        let can_print = f.format == Format::Pdf;
        let acts = div()
            .flex()
            .items_center()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(c(theme::line()))
            .child(
                div()
                    .id("export-cancel")
                    .text_color(c(theme::muted()))
                    .cursor_pointer()
                    .hover(|d| d.text_color(c(theme::text_2())))
                    .on_click(cx.listener(|this, _, w, cx| this.close_sheet(w, cx)))
                    .child(tr!("Cancel"))
                    .test_support(),
            )
            .child(div().flex_1())
            .when(can_copy, |d| d.child(button("export-copy", tr!("Copy"), false).on_click(cx.listener(|this, _, _, cx| this.export_copy(cx))).test_support()))
            .when(can_print, |d| d.child(button("export-print", tr!("Print…"), false).on_click(cx.listener(|this, _, w, cx| this.print(&Print, w, cx))).test_support()))
            .child(button("export-save", tr!("Save…"), true).on_click(cx.listener(|this, _, w, cx| this.export_save(w, cx))).test_support());

        let controls = div()
            .id("export-controls")
            .w(px(340.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .pr_5()
            .border_r_1()
            .border_color(c(theme::line()))
            .child(
                div().id("export-scroll").flex_1().min_h_0().overflow_y_scroll().pb_3().child(
                // Not squeezed when the options are taller than the window: the column scrolls.
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(label(tr!("WHAT")))
                    .child(what_seg)
                    .child(label(tr!("FORMAT")))
                    .child(formats)
                    .children(style_seg.map(|s| div().flex().flex_col().gap_1().child(label(tr!("STYLE"))).child(s)))
                    .child(options_label)
                    .children(rows),
                ),
            )
            .child(acts);

        let preview = self.render_export_preview(window, cx);
        let body = div()
            .id("export")
            .key_context("Export")
            .on_action(cx.listener(|this, _: &Confirm, w, cx| this.export_save(w, cx)))
            .size_full()
            .flex()
            .gap_5()
            .child(controls)
            .child(preview)
            .into_any_element();
        (title, body)
    }

    fn export_option_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(Sheet::Export(f)) = &self.sheet else { return Vec::new() };
        let ws = cx.entity().downgrade();
        let mut rows = Vec::new();
        let fmt = f.format;
        if !fmt.styles().is_empty() {
            rows.push(self.export_text_summary(cx));
        }
        let Some(Sheet::Export(f)) = &self.sheet else { return Vec::new() };
        let toggle_row = |id: &'static str, name: &'static str, on: bool, set: fn(&mut ExportForm, bool)| {
            let ws = ws.clone();
            opt_row(
                name,
                crate::sheets::toggle_with(id, on, move |cx, v| {
                    let _ = ws.update(cx, |this, cx| this.change_export(|f, _| set(f, v), cx));
                })
                .into_any_element(),
            )
        };
        if matches!(fmt, Format::Pdf | Format::Word | Format::Html | Format::Epub) {
            rows.push(toggle_row("export-title-page", tr!("Title page"), f.title_page, |f, v| f.title_page = v));
        }
        if (f.title_page && fmt != Format::Markdown && fmt != Format::Text) || fmt == Format::Epub {
            rows.push(opt_row(trc!("book", "Title"), field_box("export-title", f.title.clone())));
            rows.push(opt_row(tr!("Author"), field_box("export-author", f.author.clone())));
        }
        if fmt == Format::Epub {
            let cover = match &f.cover {
                Some((p, _, _)) => {
                    let ws = ws.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().max_w(px(130.)).truncate().text_color(c(theme::text_2())).child(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
                        .child(
                            div()
                                .id("export-cover-clear")
                                .text_color(c(theme::muted()))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    let _ = ws.update(cx, |this, cx| this.change_export(|f, _| f.cover = None, cx));
                                })
                                .child("×")
                                .test_support(),
                        )
                        .into_any_element()
                }
                None => button("export-cover", tr!("Choose…"), false).on_click(cx.listener(|this, _, w, cx| this.export_choose_cover(w, cx))).test_support().into_any_element(),
            };
            rows.push(opt_row(tr!("Cover"), cover));
            rows.push(toggle_row("export-contents", tr!("Contents"), f.contents, |f, v| f.contents = v));
        }
        if fmt.paged() {
            let ws2 = ws.clone();
            rows.push(opt_row(
                tr!("Paper"),
                seg("export-paper", Paper::ALL.iter().map(|p| (*p, p.label().to_string())).collect(), f.paper, move |p, cx| {
                    let _ = ws2.update(cx, |this, cx| this.change_export(
                        |f, prefs| {
                            // A Text not changed by hand follows the paper (a book is 11 pt on A5).
                            let untouched = f.text == default_text(f.style, f.paper, prefs);
                            f.paper = p;
                            if untouched {
                                f.text = default_text(f.style, p, prefs);
                            }
                        },
                        cx,
                    ));
                })
                .into_any_element(),
            ));
            rows.push(toggle_row("export-page-numbers", tr!("Page numbers"), f.page_numbers, |f, v| f.page_numbers = v));
        }
        if fmt == Format::Word {
            let ws2 = ws.clone();
            rows.push(opt_row(
                tr!("Alternatives"),
                seg("export-alternatives", vec![(false, tr!("Leave out").to_string()), (true, tr!("As comments").to_string())], f.comments, move |v, cx| {
                    let _ = ws2.update(cx, |this, cx| this.change_export(|f, _| f.comments = v, cx));
                })
                .into_any_element(),
            ));
            let ws3 = ws.clone();
            rows.push(opt_row(
                tr!("My comments"),
                seg("export-my-comments", vec![(false, tr!("Leave out").to_string()), (true, tr!("As comments").to_string())], f.my_comments, move |v, cx| {
                    let _ = ws3.update(cx, |this, cx| this.change_export(|f, _| f.my_comments = v, cx));
                })
                .into_any_element(),
            ));
        }
        if f.whole_book {
            let ws2 = ws.clone();
            rows.push(opt_row(
                tr!("Chapter titles"),
                seg("export-chapter-titles", vec![(false, tr!("Headings").to_string()), (true, tr!("File names").to_string())], f.file_titles, move |v, cx| {
                    let _ = ws2.update(cx, |this, cx| this.change_export(|f, _| f.file_titles = v, cx));
                })
                .into_any_element(),
            ));
        }
        rows
    }

    fn export_text_summary(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Sheet::Export(f)) = &self.sheet else { return div().into_any_element() };
        let t = &f.text;
        let locked = f.style.locked();
        let mut parts = Vec::new();
        if f.format != Format::Epub {
            let (font, size) = t.summary_parts();
            parts.push(format!("{font} {size}"));
        }
        if t.line_spacing >= 1.95 {
            parts.push(tr!("double").to_string());
        }
        parts.push(if t.align == export::Align::Justified { tr!("justified").to_string() } else { tr!("left").to_string() });
        if t.hyphenate {
            parts.push(tr!("hyphens").to_string());
        }
        if f.format == Format::Epub {
            parts.push(tr!("reader’s font").to_string());
        }
        let summary = parts.join(" · ");
        let control = div()
            .id("export-text")
            .flex()
            .items_center()
            .gap_2()
            .max_w(px(235.))
            .when(locked, |d| d.child(div().flex_none().text_size(px(9.5)).px_1().rounded_sm().border_1().border_color(c(theme::line())).text_color(c(theme::muted())).child(tr!("STANDARD"))))
            .child(div().min_w_0().truncate().text_size(px(11.5)).text_color(if locked { c(theme::muted()) } else { c(theme::text_2()) }).child(summary))
            .when(!locked, |d| {
                d.cursor_pointer().child(div().text_color(c(theme::muted())).child("›")).on_click(cx.listener(|this, _, _, cx| {
                    if let Some(f) = this.export_form() {
                        f.text_open = true;
                    }
                    cx.notify();
                }))
            })
            .test_support()
            .into_any_element();
        opt_row(tr!("Text"), control)
    }

    fn export_text_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(Sheet::Export(f)) = &self.sheet else { return Vec::new() };
        let ws = cx.entity().downgrade();
        let t = f.text.clone();
        let mut rows = Vec::new();
        if f.format != Format::Epub {
            let open = f.font_open;
            let ws2 = ws.clone();
            let word = f.format == Format::Word;
            let font_box = div()
                .id("export-font")
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_0p5()
                .rounded_md()
                .border_1()
                .border_color(if open { c(theme::accent()) } else { c(theme::line()) })
                .bg(c(theme::bg()))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let _ = ws2.update(cx, |this, cx| {
                        if let Some(f) = this.export_form() {
                            f.font_open = !f.font_open;
                        }
                        cx.notify();
                    });
                })
                .child(div().text_size(px(11.5)).text_color(c(theme::text_2())).child(t.font.clone()))
                .child(div().text_color(c(theme::muted())).child("▾"))
                .test_support();
            rows.push(opt_row(tr!("Font"), font_box.into_any_element()));
            if open {
                let list = div().flex().flex_col().py_1().children(FONTS.into_iter().enumerate().map(|(i, name)| {
                    let on = t.font == name;
                    let ws = ws.clone();
                    div()
                        .id(("export-font-choice", i))
                        .role(Role::RadioButton)
                        .aria_label(name)
                        .aria_selected(on)
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .cursor_pointer()
                        .when(on, |d| d.bg(ca(theme::accent(), 0.16)))
                        .hover(|d| d.bg(ca(theme::accent(), 0.1)))
                        .on_click(move |_, _, cx| {
                            let _ = ws.update(cx, |this, cx| {
                                this.change_export(|f, _| {
                                    f.text.font = name.to_string();
                                    f.font_open = false;
                                }, cx)
                            });
                        })
                        .child(div().font_family(name).text_size(px(12.5)).text_color(c(theme::text())).child(name))
                        .when(word && !export::style::word_safe(name), |d| d.child(div().text_size(px(10.)).text_color(c(theme::faint())).child(tr!("only if they have it"))))
                        .test_support()
                }));
                rows.push(list.into_any_element());
            }
            let ws2 = ws.clone();
            rows.push(opt_row(
                tr!("Size"),
                seg("export-size", export::style::SIZES.iter().map(|s| ((s * 10.) as i32, format!("{}", *s as i32))).collect(), (t.size * 10.).round() as i32, move |v, cx| {
                    let _ = ws2.update(cx, |this, cx| this.change_export(|f, _| f.text.size = v as f32 / 10., cx));
                })
                .into_any_element(),
            ));
        }
        let ws2 = ws.clone();
        rows.push(opt_row(
            tr!("Line spacing"),
            seg("export-spacing", export::style::SPACINGS.iter().map(|s| ((s * 100.).round() as i32, spacing_label(*s))).collect(), (t.line_spacing * 100.).round() as i32, move |v, cx| {
                let _ = ws2.update(cx, |this, cx| this.change_export(|f, _| f.text.line_spacing = v as f32 / 100., cx));
            })
            .into_any_element(),
        ));
        let ws2 = ws.clone();
        rows.push(opt_row(
            tr!("Alignment"),
            seg("export-align", vec![(false, trc!("alignment", "Left").to_string()), (true, tr!("Justified").to_string())], t.align == export::Align::Justified, move |v, cx| {
                let _ = ws2.update(cx, |this, cx| this.change_export(|f, _| f.text.align = if v { export::Align::Justified } else { export::Align::Left }, cx));
            })
            .into_any_element(),
        ));
        let ws2 = ws.clone();
        rows.push(opt_row(
            tr!("Hyphenation"),
            crate::sheets::toggle_with("export-hyphenation", t.hyphenate, move |cx, v| {
                let _ = ws2.update(cx, |this, cx| this.change_export(|f, _| f.text.hyphenate = v, cx));
            })
            .into_any_element(),
        ));
        let ws2 = ws.clone();
        rows.push(opt_row(
            tr!("Paragraphs"),
            seg("export-paragraphs", vec![(false, tr!("Spaced").to_string()), (true, tr!("Indented").to_string())], t.paragraphs == Paragraphs::Indented, move |v, cx| {
                let _ = ws2.update(cx, |this, cx| this.change_export(|f, _| f.text.paragraphs = if v { Paragraphs::Indented } else { Paragraphs::Spaced }, cx));
            })
            .into_any_element(),
        ));
        let style = f.style;
        let ws_page = ws.clone();
        let ws_reset = ws.clone();
        rows.push(
            div()
                .flex()
                .items_center()
                .pt_2()
                .text_size(px(11.5))
                .when(style != Style::Page, |d| {
                    d.child(
                        div()
                            .id("export-use-page")
                            .text_color(c(theme::accent()))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                let _ = ws_page.update(cx, |this, cx| {
                                    this.change_export(|f, p| {
                                        let page = default_text(Style::Page, f.paper, p);
                                        f.text.font = page.font;
                                        f.text.align = page.align;
                                        f.text.hyphenate = page.hyphenate;
                                        f.text.paragraphs = page.paragraphs;
                                    }, cx)
                                });
                            })
                            .child(tr!("Use the page’s settings"))
                            .test_support(),
                    )
                })
                .child(div().flex_1())
                .child(
                    div()
                        .id("export-reset")
                        .text_color(c(theme::muted()))
                        .cursor_pointer()
                        .hover(|d| d.text_color(c(theme::text_2())))
                        .on_click(move |_, _, cx| {
                            let _ = ws_reset.update(cx, |this, cx| this.change_export(|f, p| f.text = default_text(f.style, f.paper, p), cx));
                        })
                        .child(tr!("Reset to {style}", style = style_name(style)))
                        .test_support(),
                )
                .into_any_element(),
        );
        rows
    }

    fn render_export_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(Sheet::Export(f)) = &self.sheet else { return div().into_any_element() };
        let pv = &f.preview;
        let shadow = vec![BoxShadow { color: hsla(0., 0., 0., 0.18), offset: point(px(0.), px(6.)), blur_radius: px(20.), spread_radius: px(0.), inset: false }];
        let inner: AnyElement = if let Some(text) = &pv.text {
            div()
                .id("export-preview-text")
                .w_full()
                .max_w(px(620.))
                .h_full()
                .overflow_y_scroll()
                .p_5()
                .rounded_lg()
                .bg(c(theme::bg()))
                .border_1()
                .border_color(c(theme::line()))
                .font_family(theme::UI)
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(c(theme::text()))
                .children(text.split('\n').map(|l| div().min_h(px(19.)).child(l.to_string())).collect::<Vec<_>>())
                .into_any_element()
        } else if let Some(image) = &pv.image {
            let vp = window.viewport_size();
            let room_w = f32::from(vp.width - crate::sheets::sheet_inset(vp.width) * 2.) - 52. - 340. - 20. - 40.;
            let room_h = f32::from(vp.height) - 54. - 46. - 52. - 50. - 40. - 44.;
            let aspect = pv.image_size.1 as f32 / pv.image_size.0.max(1) as f32;
            let epub = f.format == Format::Epub;
            let (bezel, radius) = if epub { (12., 12.) } else { (0., 2.) };
            let mut w = (room_w - 2. * bezel).max(80.);
            let mut h = w * aspect;
            if h > room_h - 2. * bezel {
                h = (room_h - 2. * bezel).max(80.);
                w = h / aspect.max(0.01);
            }
            let page = div()
                .w(px(w))
                .h(px(h))
                .rounded(px(radius))
                .overflow_hidden()
                .bg(gpui_kit::white())
                .when(!epub, |d| d.shadow(shadow.clone()))
                .child(img(ImageSource::Render(image.clone())).size_full());
            if epub {
                div().p(px(bezel)).rounded(px(26.)).bg(hsla(0., 0., 0.12, 1.)).shadow(shadow).child(page).into_any_element()
            } else {
                page.into_any_element()
            }
        } else {
            div().text_color(c(theme::muted())).child(tr!("Setting the pages…")).into_any_element()
        };
        let bar = match (&pv.text, &pv.pdf) {
            (Some(t), _) => {
                let book = f.book();
                let words = book.words();
                let mut s = vec![trn!("{n} word", "{n} words", words, n = group_thousands(words)).to_string()];
                if f.whole_book {
                    s.push(trn!("{n} chapter", "{n} chapters", book.chapters.len(), n = book.chapters.len()).to_string());
                }
                s.push(trn!("{n} line", "{n} lines", t.lines().count(), n = group_thousands(t.lines().count())).to_string());
                div().child(s.join(" · ")).into_any_element()
            }
            (None, Some(pdf)) => {
                let (page, pages) = (pv.page, pdf.pages);
                let comments = if f.format == Format::Word && f.comments {
                    let n: usize = f.book().chapters.iter().map(|c| c.notes.iter().filter(|n| n.comment.is_none() && !n.others.is_empty()).count()).sum();
                    Some(trn!("{n} place with other versions", "{n} places with other versions", n, n = n).to_string())
                } else {
                    None
                };
                let info = if f.format == Format::Epub {
                    let ch = f.book().chapters.len();
                    trn!("{n} chapter", "{n} chapters", ch, n = ch).to_string()
                } else {
                    let mut s = trn!("{page} of {n} page", "{page} of {n} pages", pages, page = page + 1, n = group_thousands(pages)).to_string();
                    s.push_str(&format!(" · {}", f.paper.label()));
                    if let Some(c) = comments {
                        s.push_str(&format!(" · {c}"));
                    }
                    s
                };
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .id("export-page-prev")
                            .px_2()
                            .cursor_pointer()
                            .text_color(if page > 0 { c(theme::text_2()) } else { c(theme::faint()) })
                            .on_click(cx.listener(move |this, _, _, cx| this.export_turn(page.saturating_sub(1), cx)))
                            .child("‹")
                            .test_support(),
                    )
                    .child(info)
                    .child(
                        div()
                            .id("export-page-next")
                            .px_2()
                            .cursor_pointer()
                            .text_color(if page + 1 < pages { c(theme::text_2()) } else { c(theme::faint()) })
                            .on_click(cx.listener(move |this, _, _, cx| this.export_turn(page + 1, cx)))
                            .child("›")
                            .test_support(),
                    )
                    .into_any_element()
            }
            _ => div().into_any_element(),
        };
        div()
            .id("export-preview")
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded_lg()
            .bg(ca(theme::line(), 0.55))
            .flex()
            .flex_col()
            .items_center()
            .p_5()
            .gap_3()
            .child(div().flex_1().min_h_0().w_full().flex().items_center().justify_center().child(inner))
            .child(div().flex_none().text_size(px(11.5)).text_color(c(theme::muted())).child(bar))
            .when(pv.busy && pv.image.is_some() && pv.text.is_none(), |d| d.opacity(0.85))
            .into_any_element()
    }
}

#[cfg(feature = "shots")]
impl Workspace {
    pub(crate) fn script_export(&mut self, e: &serde_json::Value, cx: &mut Context<Self>) {
        use serde_json::Value;
        if let Some(fmt) = e.get("format").and_then(Value::as_str).and_then(Format::from_key) {
            self.set_export_format(fmt, cx);
        }
        if let Some(style) = e.get("style").and_then(Value::as_str).and_then(Style::from_key) {
            self.set_export_style(style, cx);
        }
        let page = e.get("page").and_then(Value::as_u64);
        self.change_export(
            |f, prefs| {
                if let Some(v) = e.get("whole_book").and_then(Value::as_bool) {
                    f.whole_book = v && f.whole.is_some();
                }
                if let Some(v) = e.get("text_open").and_then(Value::as_bool) {
                    f.text_open = v;
                }
                if let Some(v) = e.get("comments").and_then(Value::as_bool) {
                    f.comments = v;
                }
                if let Some(v) = e.get("my_comments").and_then(Value::as_bool) {
                    f.my_comments = v;
                }
                if let Some(v) = e.get("title_page").and_then(Value::as_bool) {
                    f.title_page = v;
                }
                if let Some(p) = e.get("paper").and_then(Value::as_str).and_then(Paper::from_key) {
                    let untouched = f.text == default_text(f.style, f.paper, prefs);
                    f.paper = p;
                    if untouched {
                        f.text = default_text(f.style, p, prefs);
                    }
                }
                if let Some(p) = page {
                    f.preview.page = p as usize;
                }
            },
            cx,
        );
    }
}

fn set_style(f: &mut ExportForm, style: Style, p: &Prefs) {
    if f.style != style || !f.format.styles().contains(&f.style) {
        f.style = style;
        f.text = f.memory.texts.get(style.key()).map(|m| m.apply(default_text(style, f.paper, p))).unwrap_or_else(|| default_text(style, f.paper, p));
        if style == Style::Manuscript {
            f.title_page = true;
        }
    }
}

/// Wide enough for a sharp page on a Retina screen.
const PREVIEW_PX: u32 = 1100;

fn render_image(b: crate::pdf::Bitmap) -> Arc<RenderImage> {
    let buf = image::RgbaImage::from_raw(b.width, b.height, b.bgra).unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new(vec![image::Frame::new(buf)]))
}

fn spacing_label(s: f32) -> String {
    let t = format!("{s:.2}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn label(t: &'static str) -> impl IntoElement {
    div().pt_3().pb_1().text_size(px(10.)).text_color(c(theme::faint())).child(t)
}

fn opt_row(name: &'static str, control: AnyElement) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .min_h(px(34.))
        .border_b_1()
        .border_color(c(theme::line()))
        .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(c(theme::text())).child(name))
        .child(control)
        .into_any_element()
}

fn field_box(id: &'static str, field: Entity<TextView>) -> AnyElement {
    div().id(id).w(px(180.)).px_2().py_0p5().rounded_md().bg(c(theme::bg())).border_1().border_color(c(theme::line())).text_size(px(12.)).child(field).test_support().into_any_element()
}

fn seg<T: std::marker::Copy + PartialEq + 'static>(id: &'static str, options: Vec<(T, String)>, current: T, set: impl Fn(T, &mut App) + 'static) -> impl IntoElement {
    seg_in(id, options, current, false, set)
}

/// A segmented control as wide as its column, the segments sharing the width (like WHAT's).
fn seg_wide<T: std::marker::Copy + PartialEq + 'static>(id: &'static str, options: Vec<(T, String)>, current: T, set: impl Fn(T, &mut App) + 'static) -> impl IntoElement {
    seg_in(id, options, current, true, set)
}

fn seg_in<T: std::marker::Copy + PartialEq + 'static>(id: &'static str, options: Vec<(T, String)>, current: T, wide: bool, set: impl Fn(T, &mut App) + 'static) -> impl IntoElement {
    let set = std::rc::Rc::new(set);
    div().flex().rounded_md().border_1().border_color(c(theme::line())).overflow_hidden().children(options.into_iter().enumerate().map(move |(i, (v, name))| {
        let on = v == current;
        let set = set.clone();
        div()
            .id((id, i))
            .px_2()
            .map(|d| if wide { d.flex_1().flex().justify_center().py_1() } else { d.py_0p5() })
            .text_size(px(11.5))
            .cursor_pointer()
            .when(i > 0, |d| d.border_l_1().border_color(c(theme::line())))
            .when(on, |d| d.bg(ca(theme::accent(), 0.22)).text_color(c(theme::text())))
            .when(!on, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text_2()))))
            .role(Role::RadioButton)
            .aria_label(name.clone())
            .aria_selected(on)
            .on_click(move |_, _, cx| set(v, cx))
            .child(name)
            .test_support()
    }))
}

fn seg2(id: &'static str, options: Vec<(bool, String, String, bool)>, current: bool, set: impl Fn(bool, &mut App) + 'static) -> impl IntoElement {
    let set = std::rc::Rc::new(set);
    div().flex().rounded_md().border_1().border_color(c(theme::line())).overflow_hidden().children(options.into_iter().enumerate().map(move |(i, (v, name, sub, enabled))| {
        let on = v == current;
        let set = set.clone();
        div()
            .id((id, i))
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .py_1()
            .when(i > 0, |d| d.border_l_1().border_color(c(theme::line())))
            .when(on, |d| d.bg(ca(theme::accent(), 0.22)))
            .role(Role::RadioButton)
            .aria_label(name.clone())
            .aria_selected(on)
            .when(enabled && !on, |d| d.cursor_pointer().hover(|d| d.bg(ca(theme::accent(), 0.08))).on_click(move |_, _, cx| set(v, cx)))
            .child(div().text_size(px(12.)).text_color(if !enabled { c(theme::faint()) } else if on { c(theme::text()) } else { c(theme::text_2()) }).child(name))
            .child(div().text_size(px(10.)).text_color(if enabled { c(theme::muted()) } else { c(theme::faint()) }).child(sub))
            .test_support()
    }))
}

#[cfg(test)]
mod tests {
    use super::{first_heading, Format, TextMemo};
    use export::{Style, Text};

    #[test]
    fn each_format_starts_in_its_style() {
        assert_eq!(Format::Word.default_style(), Style::Manuscript);
        assert_eq!(Format::Epub.default_style(), Style::Book);
        assert_eq!(Format::Pdf.default_style(), Style::Page);
        assert!(Format::Markdown.styles().is_empty());
        assert!(!Format::Epub.styles().contains(&Style::Manuscript));
    }

    #[test]
    fn a_remembered_text_comes_back() {
        let mut t = Text::book(export::Paper::A5);
        t.font = "Georgia".into();
        t.hyphenate = false;
        let back = TextMemo::of(&t).apply(Text::book(export::Paper::A5));
        assert_eq!(back, t);
    }

    #[test]
    fn titles_come_from_the_first_heading() {
        assert_eq!(first_heading("\n# The garret\n\nText").as_deref(), Some("The garret"));
        assert_eq!(first_heading("## Two\n").as_deref(), Some("Two"));
        assert_eq!(first_heading("Text first\n# Later\n"), None);
    }
}
