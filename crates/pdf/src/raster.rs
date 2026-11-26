//! A page as a bitmap, for the Export sheet's preview: the laid-out page drawn straight from its
//! glyphs' outlines, so the preview shows what the PDF holds without reading the PDF back.

use std::collections::HashMap;

use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::font::Font;
use crate::layout::{Document, Ink, Item};

/// BGRA, premultiplied, `width × height`, rows top to bottom.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Page `index` (from 0) on white, `width` pixels wide; `None` past the last page.
pub fn rasterize(doc: &Document, index: usize, width: u32) -> Option<Bitmap> {
    let page = doc.pages.get(index)?;
    if width == 0 || doc.width <= 0. {
        return None;
    }
    let scale = width as f32 / doc.width;
    let height = (doc.height * scale).round().max(1.) as u32;
    let mut pixmap = Pixmap::new(width, height)?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let mut outlines: HashMap<(Font, u16), Option<Path>> = HashMap::new();
    for item in &page.items {
        match item {
            Item::Rule { x1, y, x2, width, ink } => {
                // A hairline still shows: at least a pixel.
                let w = (width * scale).max(1.);
                let top = (doc.height - y) * scale - w / 2.;
                if let Some(rect) = Rect::from_xywh(x1 * scale, top, (x2 - x1) * scale, w) {
                    pixmap.fill_rect(rect, &paint(*ink), Transform::identity(), None);
                }
            }
            Item::Text(run) => {
                let font = run.style.font;
                let face = &font.rec.face;
                let k = run.style.size / face.units_per_em() as f32 * scale;
                let lean = if font.faux_italic { 0.21 } else { 0. };
                let ink = paint(run.style.ink);
                for g in &run.glyphs {
                    let Some(path) = outlines.entry((font, g.glyph)).or_insert_with(|| outline(font, g.glyph)) else { continue };
                    // Font units, y up, onto the bitmap, y down; an italic the face lacks is leaned.
                    let place = Transform::from_row(k, 0., -lean * k, -k, g.x * scale, (doc.height - g.y) * scale);
                    pixmap.fill_path(path, &ink, FillRule::Winding, place, None);
                    if font.faux_bold {
                        let stroke = Stroke { width: 0.03 * face.units_per_em() as f32, ..Stroke::default() };
                        pixmap.stroke_path(path, &ink, &stroke, place, None);
                    }
                }
            }
        }
    }
    let mut bgra = pixmap.take();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    Some(Bitmap { width, height, bgra })
}

fn paint(ink: Ink) -> Paint<'static> {
    let [r, g, b] = ink.rgb();
    let mut paint = Paint::default();
    paint.set_color_rgba8((r * 255.).round() as u8, (g * 255.).round() as u8, (b * 255.).round() as u8, 255);
    paint.anti_alias = true;
    paint
}

/// A glyph's outline in its face's units; `None` for one with nothing to draw (a space).
fn outline(font: Font, glyph: u16) -> Option<Path> {
    let mut builder = Outline(PathBuilder::new());
    font.rec.face.outline_glyph(GlyphId(glyph), &mut builder)?;
    builder.0.finish()
}

struct Outline(PathBuilder);

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::rasterize;
    use crate::font::tests::bundled;
    use crate::layout::typeset;
    use export::{Book, Chapter, Options, Paper, Style, Text};

    fn page(markdown: &str) -> crate::layout::Document {
        let mut o = Options::new(Style::Book, Text { font: "Literata".into(), ..Text::book(Paper::A5) }, Paper::A5);
        o.title_page = false;
        o.page_numbers = false;
        let book = Book { title: "T".into(), author: String::new(), language: None, chapters: vec![Chapter { markdown: markdown.into(), ..Default::default() }] };
        typeset(&book, &o, &bundled(), &|_| Vec::new())
    }

    /// Rows of the bitmap with any ink in them.
    fn inked_rows(b: &super::Bitmap) -> Vec<u32> {
        (0..b.height).filter(|y| b.bgra[(y * b.width * 4) as usize..((y + 1) * b.width * 4) as usize].chunks(4).any(|p| p[0] < 128)).collect()
    }

    #[test]
    fn a_page_is_ink_on_white_where_the_layout_put_its_lines() {
        let doc = page("Hello there, a line of text.\n");
        let b = rasterize(&doc, 0, 420).unwrap();
        assert_eq!((b.width, b.height), (420, (doc.height * 420. / doc.width).round() as u32));
        assert_eq!(b.bgra.len(), (b.width * b.height * 4) as usize);
        assert_eq!(&b.bgra[..4], &[255, 255, 255, 255], "white paper");
        let rows = inked_rows(&b);
        assert!(!rows.is_empty(), "some ink");
        // The one line sits by its baseline: ink above it, in the page's top margin area.
        let baseline = doc.rows(0)[0].baseline;
        let at = ((doc.height - baseline) * 420. / doc.width) as u32;
        assert!(rows.iter().all(|&y| y <= at + 4 && y + 20 >= at), "ink rows {rows:?} near the baseline's row {at}");
        // Dark ink: blue, green and red alike (BGRA), no tint.
        let darkest = b.bgra.chunks(4).min_by_key(|p| p[0]).unwrap();
        assert!(darkest[0] < 60 && (darkest[0] as i32 - darkest[2] as i32).abs() < 20, "{darkest:?}");
        assert!(rasterize(&doc, 1, 420).is_none(), "no second page");
    }

    #[test]
    fn a_rule_and_a_faux_style_draw_too() {
        let doc = page("[a link](https://x.y) and ~~struck~~\n");
        let b = rasterize(&doc, 0, 600).unwrap();
        // An underline is a row inked across most of the link's width.
        let longest = (0..b.height).map(|y| b.bgra[(y * b.width * 4) as usize..((y + 1) * b.width * 4) as usize].chunks(4).filter(|p| p[0] < 160).count()).max().unwrap();
        assert!(longest > 20, "a rule draws as a run of ink: {longest}");
    }
}
