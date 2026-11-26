//! What the tests share: the app's font files, the three styles in those faces, and a reader for
//! the files the crate writes.

pub mod read;

pub const BUNDLED: [&[u8]; 19] = [
        include_bytes!("../../../app/assets/fonts/JetBrainsMono-Regular.ttf"),
        include_bytes!("../../../app/assets/fonts/JetBrainsMono-Bold.ttf"),
        include_bytes!("../../../app/assets/fonts/JetBrainsMono-Italic.ttf"),
        include_bytes!("../../../app/assets/fonts/JetBrainsMono-BoldItalic.ttf"),
        include_bytes!("../../../app/assets/fonts/Caveat.ttf"),
        include_bytes!("../../../app/assets/fonts/ShantellSans-Regular.ttf"),
        include_bytes!("../../../app/assets/fonts/BadScript-Regular.ttf"),
        include_bytes!("../../../app/assets/fonts/IBMPlexMono-Regular.ttf"),
        include_bytes!("../../../app/assets/fonts/IBMPlexMono-Bold.ttf"),
        include_bytes!("../../../app/assets/fonts/IBMPlexMono-Italic.ttf"),
        include_bytes!("../../../app/assets/fonts/IBMPlexMono-BoldItalic.ttf"),
        include_bytes!("../../../app/assets/fonts/iAWriterQuattroS-Regular.ttf"),
        include_bytes!("../../../app/assets/fonts/iAWriterQuattroS-Bold.ttf"),
        include_bytes!("../../../app/assets/fonts/iAWriterQuattroS-Italic.ttf"),
        include_bytes!("../../../app/assets/fonts/iAWriterQuattroS-BoldItalic.ttf"),
        include_bytes!("../../../app/assets/fonts/Literata-Regular.ttf"),
        include_bytes!("../../../app/assets/fonts/Literata-Bold.ttf"),
        include_bytes!("../../../app/assets/fonts/Literata-Italic.ttf"),
        include_bytes!("../../../app/assets/fonts/Literata-BoldItalic.ttf"),
];

/// The app's fonts and nothing of the system's: the same pages on every system.
pub fn bundled() -> pdf::font::Fonts {
    pdf::font::Fonts::new(&BUNDLED)
}

use export::{Book, Chapter, Options, Paper, Style, Text};

/// A style as the app sets it, in the app's own faces.
pub fn opts(style: Style) -> Options {
    let (text, paper) = match style {
        Style::Manuscript => (Text { font: "Literata".into(), ..Text::manuscript() }, Paper::Letter),
        Style::Book => (Text { font: "Literata".into(), ..Text::book(Paper::A5) }, Paper::A5),
        Style::Page => (Text::page("IBM Plex Mono", 19., 1.8, true, true, None), Paper::A4),
    };
    Options::new(style, text, paper)
}

pub const PARA: &str = "On an exceptionally hot evening early in July a young man came out of the garret in which he lodged in S. Place and walked slowly, as though in hesitation, towards K. bridge. He had successfully avoided meeting his landlady on the staircase. His garret was under the roof of a high, five-storied house and was more like a cupboard than a room.";

/// A short book with a little of everything a page shows.
pub fn sample() -> Book {
    let md = format!(
        "# The garret\n\n{PARA}\n\nThe landlady who provided him with *garret, dinners, and attendance*, lived on the floor below, and every time he went out he was obliged to pass her kitchen, the door of which invariably stood open. And **this**. And ~~that~~. And [a link](https://dostoevsky.app).\n\n> A quote that is a little long so that it wraps onto a second line in the page.\n\n- one item\n- two items\n\n1. first\n2. second\n\n---\n\n{PARA}\n\n{PARA}\n\n```\nlet x = 1;\n```\n"
    );
    Book {
        title: "Crime and Punishment".into(),
        author: "Fyodor Dostoevsky".into(),
        language: Some("en".into()),
        chapters: vec![
            Chapter { part: Some("Part One".into()), markdown: md, ..Default::default() },
            Chapter { file_title: "A visit".into(), markdown: "A knock at the door.\n".repeat(30), ..Default::default() },
            Chapter { markdown: "# Three\n\nShort.\n".into(), ..Default::default() },
        ],
    }
}

/// The book's pages laid out and written out, through the crate's own front door.
pub fn pdf_of(book: &Book, o: &Options) -> (pdf::layout::Document, Vec<u8>) {
    let pdf = pdf::render(book, o, &bundled(), &|_| Vec::new());
    let laid = std::sync::Arc::try_unwrap(pdf.laid).unwrap_or_else(|_| panic!("the only hold on the pages"));
    (laid, pdf.bytes)
}
