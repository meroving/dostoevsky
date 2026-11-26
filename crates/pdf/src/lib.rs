//! Pages of the final text as a PDF and as bitmaps, with no UI and no system text engine: the
//! page composition TextKit draws on the Mac, and the engine that draws it everywhere else.

pub mod compose;
pub mod font;
pub mod layout;
pub mod raster;
pub mod shape;
pub mod write;

use std::sync::Arc;

use export::{Book, Options};

pub use font::Fonts;
pub use layout::Document;
pub use raster::Bitmap;

/// A book's pages: the file, and how they were laid out (for the preview's bitmaps).
#[derive(Clone)]
pub struct Pdf {
    pub bytes: Vec<u8>,
    pub laid: Arc<Document>,
}

impl Pdf {
    pub fn pages(&self) -> usize {
        self.laid.pages.len()
    }
}

/// The book set in `o`'s style. `hyphenate` gives a word's hyphenation points, in characters
/// from its start; none is fine.
pub fn render(book: &Book, o: &Options, fonts: &Fonts, hyphenate: &dyn Fn(&str) -> Vec<usize>) -> Pdf {
    let laid = layout::typeset(book, o, fonts, hyphenate);
    let bytes = write::encode(&laid, &book.title, &book.author);
    Pdf { bytes, laid: Arc::new(laid) }
}

/// Page `index` (from 0) on white, `width` pixels wide.
pub fn rasterize(pdf: &Pdf, index: usize, width: u32) -> Option<Bitmap> {
    raster::rasterize(&pdf.laid, index, width)
}
