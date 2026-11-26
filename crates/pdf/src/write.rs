//! The pages as a PDF. Each face goes into the file once, as a subset holding just the glyphs the
//! pages show, and the text is those glyphs' codes with a map back to the characters: the words
//! can be selected, searched for and copied out of the file. Nothing in it depends on the run:
//! the same pages give the same bytes.

use std::collections::{BTreeMap, BTreeSet};

use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr, types::SystemInfo};
use rustybuzz::ttf_parser::GlyphId;
use subsetter::GlyphRemapper;

use crate::font::Font;
use crate::layout::{Document, Item, Page};

/// A face as the file holds it.
struct Embedded {
    /// The object of its Type0 font, which a page's resources name.
    reference: Ref,
    /// Each glyph's code in the font: its glyph number in the subset.
    codes: BTreeMap<u16, u16>,
    /// How far each glyph reaches in the original face, in its units.
    advances: BTreeMap<u16, u16>,
    units: u16,
}

pub fn encode(doc: &Document, title: &str, author: &str) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let mut next = 1;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let (catalog, page_tree, info) = (alloc(), alloc(), alloc());

    let mut used: BTreeMap<Font, BTreeSet<u16>> = BTreeMap::new();
    for run in doc.pages.iter().flat_map(|p| &p.items).filter_map(|i| match i {
        Item::Text(run) => Some(run),
        Item::Rule { .. } => None,
    }) {
        used.entry(run.style.font).or_default().extend(run.glyphs.iter().map(|g| g.glyph));
    }
    let fonts: BTreeMap<Font, Embedded> = used.iter().map(|(font, glyphs)| (*font, embed(&mut pdf, &mut alloc, font, glyphs, &doc.text[font]))).collect();

    let mut pages = Vec::new();
    for page in &doc.pages {
        let (id, content_id) = (alloc(), alloc());
        let mut stream = Content::new();
        draw(&mut stream, page, &fonts);
        pdf.stream(content_id, &deflate(&stream.finish())).filter(Filter::FlateDecode);
        let mut writer = pdf.page(id);
        writer.parent(page_tree).media_box(Rect::new(0., 0., doc.width, doc.height)).contents(content_id);
        let mut resources = writer.resources();
        // Just the faces a page writes in, under names of their place in the list: one dictionary.
        let mut names = resources.fonts();
        for (i, (font, embedded)) in fonts.iter().enumerate() {
            if page.items.iter().any(|item| matches!(item, Item::Text(run) if run.style.font == *font)) {
                names.pair(Name(format!("F{i}").as_bytes()), embedded.reference);
            }
        }
        names.finish();
        resources.finish();
        writer.finish();
        pages.push(id);
    }
    let mut tree = pdf.pages(page_tree);
    tree.kids(pages.iter().copied());
    tree.count(pages.len() as i32);
    tree.finish();
    pdf.catalog(catalog).pages(page_tree);
    let mut document_info = pdf.document_info(info);
    document_info.title(TextStr(title.trim())).author(TextStr(author.trim())).creator(TextStr("Dostoevsky"));
    document_info.finish();
    pdf.finish()
}


/// A page's drawing commands.
fn draw(out: &mut Content, page: &Page, fonts: &BTreeMap<Font, Embedded>) {
    for item in &page.items {
        match item {
            Item::Rule { x1, y, x2, width, ink } => {
                let [r, g, b] = ink.rgb();
                out.save_state();
                out.set_line_width(*width).set_stroke_rgb(r, g, b).move_to(*x1, *y).line_to(*x2, *y).stroke();
                out.restore_state();
            }
            Item::Text(run) => {
                let Some(first) = run.glyphs.first() else { continue };
                let Some(index) = fonts.keys().position(|f| *f == run.style.font) else { continue };
                let embedded = &fonts[&run.style.font];
                let name = format!("F{index}");
                let [r, g, b] = run.style.ink.rgb();
                let size = run.style.size;
                out.save_state();
                out.begin_text();
                out.set_font(Name(name.as_bytes()), size);
                out.set_fill_rgb(r, g, b);
                // A face without a bold or an italic of its own is stroked a little, or leaned.
                if run.style.font.faux_bold {
                    out.set_text_rendering_mode(pdf_writer::types::TextRenderingMode::FillStroke).set_stroke_rgb(r, g, b).set_line_width(size * 0.03);
                }
                let lean = if run.style.font.faux_italic { 0.21 } else { 0. };
                out.set_text_matrix([1., 0., lean, 1., first.x, first.y]);
                let mut positioned = out.show_positioned();
                let mut items = positioned.items();
                // Glyphs the pen reaches by itself share a string; a move between two splits it.
                let mut codes: Vec<u8> = Vec::new();
                for (i, g) in run.glyphs.iter().enumerate() {
                    if i > 0 {
                        // The pen has moved on by the last glyph's width in the font: bring it to this one's place.
                        let last = run.glyphs[i - 1];
                        let after = last.x + embedded.advances.get(&last.glyph).copied().unwrap_or(0) as f32 * size / embedded.units as f32;
                        let adjust = ((after - g.x) * 100_000. / size).round() / 100.;
                        if adjust != 0. {
                            items.show(Str(&codes));
                            codes.clear();
                            items.adjust(adjust);
                        }
                    }
                    codes.extend(embedded.codes.get(&g.glyph).copied().unwrap_or(1).to_be_bytes());
                }
                items.show(Str(&codes));
                items.finish();
                drop(positioned);
                out.end_text();
                out.restore_state();
            }
        }
    }
}

/// The face as the file embeds it: a subset of its glyphs, unless its licence has it whole.
fn embed(pdf: &mut Pdf, alloc: &mut impl FnMut() -> Ref, font: &Font, glyphs: &BTreeSet<u16>, text: &BTreeMap<u16, String>) -> Embedded {
    let (rec, face) = (font.rec, &font.rec.face);
    let mut remap = GlyphRemapper::new();
    remap.remap(0);
    for &g in glyphs {
        remap.remap(g);
    }
    // A variable face goes in at its default instance, as the shapes were laid out with it.
    let (program, subset) = if rec.no_subset {
        (rec.data.to_vec(), false)
    } else {
        match if face.is_variable() { subsetter::subset_with_variations(rec.data, rec.index, &[], &remap) } else { subsetter::subset(rec.data, rec.index, &remap) } {
            Ok(sub) => (sub, true),
            Err(_) => (rec.data.to_vec(), false),
        }
    };
    let codes: BTreeMap<u16, u16> = glyphs.iter().map(|&g| (g, if subset { remap.get(g).unwrap_or(1) } else { g })).collect();
    let advances: BTreeMap<u16, u16> = glyphs.iter().filter_map(|&g| Some((g, face.glyph_hor_advance(GlyphId(g))?))).collect();
    let units = face.units_per_em();
    let unit = 1000. / units as f32;

    let (reference, descendant, descriptor, file, map) = (alloc(), alloc(), alloc(), alloc(), alloc());
    // TrueType outlines go in as such; PostScript ones, in an OpenType file, as a Type0 CID font.
    let truetype = face.tables().glyf.is_some();
    let full = format!("{}+{}", tag(&rec.family, rec.bold || font.faux_bold, rec.italic || font.faux_italic, glyphs), postscript_name(font));
    let full = Name(full.as_bytes());
    let identity = SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };

    let compressed = deflate(&program);
    {
        let mut stream = pdf.stream(file, &compressed);
        stream.filter(Filter::FlateDecode);
        if !truetype {
            stream.pair(Name(b"Subtype"), Name(b"OpenType"));
        }
        stream.finish();
    }
    let mut flags = pdf_writer::types::FontFlags::NON_SYMBOLIC;
    if rec.family.contains("mono") || face.is_monospaced() {
        flags |= pdf_writer::types::FontFlags::FIXED_PITCH;
    } else if SERIF.iter().any(|s| rec.family.contains(s)) {
        flags |= pdf_writer::types::FontFlags::SERIF;
    }
    if rec.italic || font.faux_italic {
        flags |= pdf_writer::types::FontFlags::ITALIC;
    }
    if font.faux_bold {
        flags |= pdf_writer::types::FontFlags::FORCE_BOLD;
    }
    let bounds = face.global_bounding_box();
    let mut describe = pdf.font_descriptor(descriptor);
    describe
        .name(full)
        .flags(flags)
        .bbox(Rect::new(bounds.x_min as f32 * unit, bounds.y_min as f32 * unit, bounds.x_max as f32 * unit, bounds.y_max as f32 * unit))
        .italic_angle(face.italic_angle() - if font.faux_italic { 12. } else { 0. })
        .ascent(face.ascender() as f32 * unit)
        .descent(face.descender() as f32 * unit)
        .cap_height(face.capital_height().unwrap_or(face.ascender()) as f32 * unit)
        .stem_v(if font.faux_bold { 120. } else { 80. });
    if truetype {
        describe.font_file2(file);
    } else {
        describe.font_file3(file);
    }
    describe.finish();

    let mut cid_font = pdf.cid_font(descendant);
    cid_font.subtype(if truetype { pdf_writer::types::CidFontType::Type2 } else { pdf_writer::types::CidFontType::Type0 }).base_font(full).system_info(identity).font_descriptor(descriptor).default_width(0.);
    let mut widths = cid_font.widths();
    for (glyph, code) in &codes {
        widths.same(*code, *code, (advances.get(glyph).copied().unwrap_or(0) as f32 * unit * 100.).round() / 100.);
    }
    widths.finish();
    if truetype {
        cid_font.cid_to_gid_map_predefined(Name(b"Identity"));
    }
    cid_font.finish();

    let mut unicode = pdf_writer::types::UnicodeCmap::new(Name(b"Adobe-Identity-UCS"), SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"UCS"), supplement: 0 });
    for (glyph, code) in &codes {
        if let Some(text) = text.get(glyph) {
            unicode.pair_with_multiple(*code, text.chars());
        }
    }
    let map_bytes = unicode.finish();
    pdf.cmap(map, &map_bytes).name(Name(b"Adobe-Identity-UCS")).system_info(identity).finish();
    pdf.type0_font(reference).base_font(full).encoding_predefined(Name(b"Identity-H")).descendant_font(descendant).to_unicode(map).finish();

    Embedded { reference, codes, advances, units: units as u16 }
}

/// Families a reader would call serif, for the flag that says so.
const SERIF: [&str; 9] = ["literata", "times", "georgia", "palatino", "charter", "iowan", "liberation serif", "dejavu serif", "noto serif"];

/// The face's PostScript name (`Literata-BoldItalic`), else one made from its family; a style the
/// face is drawn in without having it is added to the end (`Caveat-Regular,Bold`).
fn postscript_name(font: &Font) -> String {
    let face = &font.rec.face;
    let own = face.names().into_iter().filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME).find_map(|n| n.to_string());
    let mut name: String = own
        .unwrap_or_else(|| {
            let family: String = font.rec.family.split_whitespace().map(|w| w[..1].to_uppercase() + &w[1..]).collect();
            let style = match (font.rec.bold, font.rec.italic) {
                (true, true) => "BoldItalic",
                (true, false) => "Bold",
                (false, true) => "Italic",
                (false, false) => "Regular",
            };
            format!("{family}-{style}")
        })
        .chars()
        .filter(|c| c.is_ascii_graphic() && !"[](){}<>/%#".contains(*c))
        .collect();
    match (font.faux_bold, font.faux_italic) {
        (true, true) => name.push_str(",BoldItalic"),
        (true, false) => name.push_str(",Bold"),
        (false, true) => name.push_str(",Italic"),
        (false, false) => {}
    }
    name
}

/// Six capitals in front of a subset's name, as the standard asks: from the face and the glyphs
/// it keeps, so two subsets of one face have two names.
fn tag(family: &str, bold: bool, italic: bool, glyphs: &BTreeSet<u16>) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in family.bytes().chain([b'B', u8::from(bold), u8::from(italic)]).chain(glyphs.iter().flat_map(|g| g.to_le_bytes())) {
        hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
    }
    (0..6).map(|i| char::from(b'A' + ((hash >> (i * 8)) % 26) as u8)).collect()
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}
