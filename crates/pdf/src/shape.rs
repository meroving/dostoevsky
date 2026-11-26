//! A paragraph's text shaped: each run in one face and writing system, each glyph with the
//! text it stands for.

use std::collections::HashMap;
use std::ops::Range;
use std::str::FromStr;

use rustybuzz::{Direction, Language, Script, ShapePlan, UnicodeBuffer, ttf_parser::Tag};
use unicode_script::{Script as Writing, UnicodeScript};

use crate::font::{Font, Fonts};

/// One glyph as its face shaped it. `cluster` is where its text starts in the paragraph, in bytes,
/// and `len` how much of it: a ligature stands for several characters.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub id: u16,
    pub cluster: u32,
    pub len: u16,
    /// In points. `advance` has the run's tracking after it.
    pub advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

/// A stretch of a paragraph in one face and size.
pub struct Shaped {
    pub font: Font,
    pub size: f32,
    /// Where the stretch's text starts in the paragraph, in bytes.
    pub start: usize,
    pub glyphs: Vec<Glyph>,
}

/// Shapes for one document: a plan per face and writing system, and a face per character that a
/// text's own face lacks.
pub struct Shaper<'a> {
    fonts: &'a Fonts,
    language: Option<Language>,
    plans: HashMap<(Font, Writing, bool), ShapePlan>,
    fallbacks: HashMap<char, Option<Font>>,
}

const RTL: &[Writing] = &[Writing::Adlam, Writing::Arabic, Writing::Hebrew, Writing::Mandaic, Writing::Nko, Writing::Samaritan, Writing::Syriac, Writing::Thaana];

fn script(writing: Writing) -> Option<Script> {
    Script::from_iso15924_tag(Tag::from_bytes(writing.short_name().as_bytes().try_into().ok()?))
}

impl<'a> Shaper<'a> {
    /// `language`: the document's (`en`, `ru`), for the forms a face keeps for a language.
    pub fn new(fonts: &'a Fonts, language: Option<&str>) -> Shaper<'a> {
        Shaper { fonts, language: language.and_then(|l| Language::from_str(l).ok()), plans: HashMap::new(), fallbacks: HashMap::new() }
    }

    /// `text` sits `start` bytes into its paragraph; `tracking` goes after each of its glyphs.
    pub fn shape(&mut self, text: &str, start: usize, font: Font, size: f32, tracking: f32) -> Vec<Shaped> {
        let faces = self.faces(text, font);
        let mut out = Vec::new();
        for (range, face) in faces {
            for (piece, writing) in writings(&text[range.clone()]) {
                let piece = range.start + piece.start..range.start + piece.end;
                out.push(self.run(&text[piece.clone()], start + piece.start, face, size, tracking, writing, RTL.contains(&writing)));
            }
        }
        out
    }

    /// Where the text changes face: a character `font` hasn't takes the first face that has it.
    fn faces(&mut self, text: &str, font: Font) -> Vec<(Range<usize>, Font)> {
        let has = |f: Font, c: char| c.is_whitespace() || f.rec.face.glyph_index(c).is_some();
        let mut out = Vec::new();
        let mut current = font;
        let mut from = 0;
        for (at, c) in text.char_indices() {
            let wanted = if has(font, c) { font } else { self.fallbacks.entry(c).or_insert_with(|| self.fonts.fallback(font, c)).unwrap_or(current) };
            if wanted != current {
                if at > from {
                    out.push((from..at, current));
                }
                current = wanted;
                from = at;
            }
        }
        out.push((from..text.len(), current));
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn run(&mut self, text: &str, start: usize, font: Font, size: f32, tracking: f32, writing: Writing, rtl: bool) -> Shaped {
        let direction = if rtl { Direction::RightToLeft } else { Direction::LeftToRight };
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(direction);
        buffer.set_script(script(writing).unwrap_or(rustybuzz::script::LATIN));
        let plan = self.plan(font, writing, rtl);
        let shaped = rustybuzz::shape_with_plan(&font.rec.face, plan, buffer);
        let scale = size / font.rec.face.units_per_em() as f32;
        let infos = shaped.glyph_infos();
        let mut glyphs: Vec<Glyph> = infos
            .iter()
            .zip(shaped.glyph_positions())
            .enumerate()
            .map(|(i, (info, place))| {
                let next = infos.get(i + 1).filter(|n| n.cluster > info.cluster).map_or(text.len() as u32, |n| n.cluster);
                Glyph { id: info.glyph_id as u16, cluster: (start + info.cluster as usize) as u32, len: (next - info.cluster) as u16, advance: place.x_advance as f32 * scale + tracking, x_offset: place.x_offset as f32 * scale, y_offset: place.y_offset as f32 * scale }
            })
            .collect();
        // Right-to-left glyphs come rightmost first: the line lays them out left to right.
        if rtl {
            glyphs.reverse();
        }
        Shaped { font, size, start, glyphs }
    }

    fn plan(&mut self, font: Font, writing: Writing, rtl: bool) -> &ShapePlan {
        let direction = if rtl { Direction::RightToLeft } else { Direction::LeftToRight };
        let script = script(writing).unwrap_or(rustybuzz::script::LATIN);
        let language = self.language.clone();
        self.plans.entry((font, writing, rtl)).or_insert_with(|| ShapePlan::new(&font.rec.face, direction, Some(script), language.as_ref(), &[]))
    }
}

/// The text's stretches by writing system. A character with a system of its own (not common or
/// inherited) starts a stretch; spaces and punctuation go with the stretch they follow, and
/// leading ones with the one after them.
fn writings(text: &str) -> Vec<(Range<usize>, Writing)> {
    // Start, end and the stretch's system: `None` until a character with one arrives.
    let mut out: Vec<(usize, usize, Option<Writing>)> = Vec::new();
    for (at, c) in text.char_indices() {
        let writing = c.script();
        let strong = !matches!(writing, Writing::Common | Writing::Inherited);
        let end = at + c.len_utf8();
        match out.last_mut() {
            Some(last) if !strong => last.1 = end,
            Some(last) if last.2.is_none() => {
                last.1 = end;
                last.2 = Some(writing);
            }
            Some(last) if last.2 == Some(writing) => last.1 = end,
            _ => out.push((at, end, strong.then_some(writing))),
        }
    }
    out.into_iter().map(|(s, e, w)| (s..e, w.unwrap_or(Writing::Common))).collect()
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{Shaper, writings};
    use crate::font::{Font, tests::bundled};

    fn literata() -> (crate::font::Fonts, Font) {
        let fonts = bundled();
        let font = fonts.resolve("Literata", false, false).unwrap();
        (fonts, font)
    }

    #[test]
    fn a_ligature_stands_for_its_letters() {
        let (fonts, font) = literata();
        let mut shaper = Shaper::new(&fonts, Some("en"));
        let runs = shaper.shape("office", 10, font, 11., 0.);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start, 10);
        assert!(runs[0].glyphs.iter().all(|g| g.cluster >= 10));
        let ligature = runs[0].glyphs.iter().find(|g| g.len > 1).expect("the ffi ligature");
        let from = ligature.cluster as usize - 10;
        assert_eq!(&"office"[from..from + ligature.len as usize], "ffi");
        assert_eq!(runs[0].glyphs.iter().map(|g| g.len).sum::<u16>(), 6, "every character is accounted for");
        assert!(runs[0].glyphs.iter().all(|g| g.id != 0), "no missing glyph");
    }

    #[test]
    fn kerning_narrows_a_pair_and_tracking_widens() {
        let (fonts, font) = literata();
        let mut shaper = Shaper::new(&fonts, None);
        let width = |shaper: &mut Shaper, s: &str, tracking: f32| shaper.shape(s, 0, font, 12., tracking).iter().flat_map(|r| r.glyphs.iter().map(|g| g.advance)).sum::<f32>();
        let (pair, t, o) = (width(&mut shaper, "To", 0.), width(&mut shaper, "T", 0.), width(&mut shaper, "o", 0.));
        assert!(pair < t + o - 0.01, "kerned {pair} is narrower than {t} + {o}");
        assert!((width(&mut shaper, "ab", 1.) - width(&mut shaper, "ab", 0.) - 2.).abs() < 1e-4, "tracking after every glyph");
    }

    #[test]
    fn stretches_split_at_a_change_of_writing_system() {
        let ranges: Vec<Range<usize>> = writings("hello мир").into_iter().map(|(r, _)| r).collect();
        assert_eq!(ranges.iter().map(|r| r.len()).collect::<Vec<_>>(), [6, 6], "the space goes with the Latin stretch");
        assert_eq!(writings("hi there").len(), 1, "a space doesn't split");
        assert_eq!(writings("«hello»").len(), 1, "punctuation goes with its text");
        assert_eq!(writings("سلام").len(), 1);
        assert!(writings("سلام").iter().all(|(_, w)| super::RTL.contains(w)));
        assert!(writings("…").iter().all(|(_, w)| !super::RTL.contains(w)));
    }

    #[test]
    fn a_character_the_face_lacks_takes_another_face() {
        let (fonts, font) = literata();
        let mut shaper = Shaper::new(&fonts, None);
        let missing = ['☃', '中', 'Ώ', 'я', 'א'].into_iter().find(|&c| font.rec.face.glyph_index(c).is_none());
        let Some(c) = missing else { return };
        let text = format!("a{c}b");
        let runs = shaper.shape(&text, 0, font, 11., 0.);
        assert_eq!(runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.len as usize)).sum::<usize>(), text.len(), "every character is accounted for");
        assert!(runs.windows(2).all(|w| w[0].start <= w[1].start), "runs follow the text");
        // The letters keep their face; the character takes another when one has it, and never the missing-glyph box.
        let (a, b) = (runs[0].glyphs[0], runs.last().unwrap().glyphs[0]);
        assert_eq!((a.cluster, a.len, runs[0].font), (0, 1, font));
        assert_eq!((b.cluster as usize, b.len), (text.len() - 1, 1));
        assert!(runs.iter().all(|r| r.glyphs.iter().filter(|g| text[g.cluster as usize..].starts_with(c)).all(|g| g.id != 0 && r.font.rec.face.glyph_index(c).is_some())));
    }
}
