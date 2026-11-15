#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    Page,
    Manuscript,
    Book,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Page, Style::Manuscript, Style::Book];

    pub fn key(self) -> &'static str {
        match self {
            Style::Page => "page",
            Style::Manuscript => "manuscript",
            Style::Book => "book",
        }
    }

    pub fn from_key(k: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.key() == k)
    }

    pub fn locked(self) -> bool {
        self == Style::Manuscript
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Justified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Paragraphs {
    Spaced,
    Indented,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub font: String,
    /// Points.
    pub size: f32,
    /// A multiple of the size.
    pub line_spacing: f32,
    pub align: Align,
    pub hyphenate: bool,
    pub paragraphs: Paragraphs,
    pub heading_font: Option<String>,
}

pub const APP_FONTS: [&str; 4] = ["IBM Plex Mono", "JetBrains Mono", "iA Writer Quattro S", "Literata"];
pub const BOOK_FONTS: [&str; 5] = ["Iowan Old Style", "Charter", "Palatino", "Georgia", "Times New Roman"];
/// Word doesn't embed fonts: these the reader's Mac or PC will have.
pub const WORD_FONTS: [&str; 5] = ["Times New Roman", "Georgia", "Courier New", "Arial", "Calibri"];

pub fn word_safe(font: &str) -> bool {
    WORD_FONTS.contains(&font)
}

pub const SIZES: [f32; 4] = [10., 11., 12., 13.];
pub const SPACINGS: [f32; 4] = [1.2, 1.35, 1.5, 2.];

impl Text {
    pub fn manuscript() -> Text {
        Text { font: "Times New Roman".into(), size: 12., line_spacing: 2., align: Align::Left, hyphenate: false, paragraphs: Paragraphs::Indented, heading_font: None }
    }

    pub fn book(paper: Paper) -> Text {
        let size = if paper == Paper::A5 { 11. } else { 12. };
        Text { font: "Iowan Old Style".into(), size, line_spacing: 1.35, align: Align::Justified, hyphenate: true, paragraphs: Paragraphs::Indented, heading_font: None }
    }

    /// 19 px on screen is 11 pt on paper; the screen's line spacing is eased (1.8 → 1.5).
    pub fn page(font: &str, screen_px: f32, screen_spacing: f32, justified: bool, hyphenate: bool, heading_font: Option<&str>) -> Text {
        let size = (screen_px * 0.58).round().clamp(8., 16.);
        let line_spacing = ((1. + (screen_spacing - 1.) * 0.625) * 20.).round() / 20.;
        Text {
            font: font.into(),
            size,
            line_spacing: line_spacing.clamp(1.1, 2.2),
            align: if justified { Align::Justified } else { Align::Left },
            hyphenate,
            paragraphs: Paragraphs::Spaced,
            heading_font: heading_font.map(Into::into),
        }
    }

    pub fn summary_parts(&self) -> (String, String) {
        (self.font.clone(), format!("{} pt", trim_num(self.size)))
    }
}

pub(crate) fn trim_num(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Paper {
    A4,
    Letter,
    A5,
}

impl Paper {
    pub const ALL: [Paper; 3] = [Paper::A4, Paper::Letter, Paper::A5];

    /// In points.
    pub fn size(self) -> (f32, f32) {
        match self {
            Paper::A4 => (595.28, 841.89),
            Paper::Letter => (612., 792.),
            Paper::A5 => (419.53, 595.28),
        }
    }

    /// In points.
    pub fn margin(self) -> f32 {
        match self {
            Paper::A4 | Paper::Letter => 72.,
            Paper::A5 => 54.,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Paper::A4 => "a4",
            Paper::Letter => "letter",
            Paper::A5 => "a5",
        }
    }

    pub fn from_key(k: &str) -> Option<Paper> {
        Paper::ALL.into_iter().find(|p| p.key() == k)
    }

    pub fn label(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::Letter => "Letter",
            Paper::A5 => "A5",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChapterTitles {
    Headings,
    /// A heading made from the file name for chapters that don't start with one.
    FileNames,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub style: Style,
    pub text: Text,
    pub paper: Paper,
    pub title_page: bool,
    pub page_numbers: bool,
    /// Word: the other versions of a place as comments on it.
    pub comments: bool,
    /// Word: the writer's own comments, by `comment_author`.
    pub my_comments: bool,
    pub comment_author: String,
    /// ePub: a contents page.
    pub contents: bool,
    pub chapter_titles: ChapterTitles,
    /// ePub: a cover image, as (bytes, MIME type).
    pub cover: Option<(Vec<u8>, String)>,
}

impl Options {
    pub fn new(style: Style, text: Text, paper: Paper) -> Options {
        Options {
            style,
            text,
            paper,
            title_page: style == Style::Manuscript,
            page_numbers: true,
            comments: false,
            my_comments: false,
            comment_author: String::new(),
            contents: true,
            chapter_titles: ChapterTitles::Headings,
            cover: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_style_converts_screen_type_for_paper() {
        let t = Text::page("IBM Plex Mono", 19., 1.8, true, false, None);
        assert_eq!(t.size, 11.);
        assert_eq!(t.line_spacing, 1.5);
        assert_eq!(t.align, Align::Justified);
        assert_eq!(t.paragraphs, Paragraphs::Spaced);
    }

    #[test]
    fn a_manuscript_is_double_spaced_ragged_and_unhyphenated() {
        let t = Text::manuscript();
        assert_eq!((t.size, t.line_spacing, t.align, t.hyphenate), (12., 2., Align::Left, false));
        assert!(Style::Manuscript.locked());
    }

    #[test]
    fn numbers_print_without_trailing_zeros() {
        assert_eq!(trim_num(11.), "11");
        assert_eq!(trim_num(1.35), "1.35");
        assert_eq!(trim_num(1.5), "1.5");
    }
}
