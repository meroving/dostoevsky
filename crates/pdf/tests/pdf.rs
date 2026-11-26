//! The files the crate writes, read back by a reader of their own: their pages in order, their
//! text through their fonts' maps, and real outlines for every face the pages name.

mod common;

use std::collections::BTreeSet;

use common::{PARA, opts, pdf_of, read::Object, read::Pdf, sample};
use export::{Book, Chapter, Style};
use pdf::layout::Item;

/// A font's program as the file holds it, inflated.
fn program<'a>(file: &'a Pdf, font: &'a Object) -> (Vec<u8>, &'a Object) {
    let Object::Array(descendants) = file.member(font, b"DescendantFonts").expect("a descendant") else { panic!("a list of one descendant") };
    let descendant = file.object(&descendants[0]);
    let descriptor = file.member(descendant, b"FontDescriptor").expect("a descriptor");
    let stream = file.member(descriptor, b"FontFile2").or_else(|| file.member(descriptor, b"FontFile3")).expect("the face is embedded");
    (file.data(stream), descriptor)
}

#[test]
fn every_style_becomes_a_file_whose_pages_read_back_as_they_were_laid_out() {
    for style in Style::ALL {
        let mut o = opts(style);
        o.title_page = true;
        o.chapter_titles = export::ChapterTitles::FileNames;
        let book = sample();
        let (doc, bytes) = pdf_of(&book, &o);
        let file = Pdf::parse(&bytes);
        let pages = file.pages();
        assert_eq!(pages.len(), doc.pages.len(), "{style:?}: a page in the file for each of the document's");
        assert!(pages.len() >= 4, "{style:?}: {} pages", pages.len());
        for (i, page) in pages.iter().enumerate() {
            let Object::Array(rect) = file.member(page, b"MediaBox").expect("a page's box") else { panic!("a box is an array") };
            let numbers: Vec<f64> = rect.iter().map(|v| v.number().unwrap()).collect();
            let want = [0., 0., doc.width as f64, doc.height as f64];
            assert!(numbers.iter().zip(want).all(|(n, w)| (n - w).abs() < 0.01), "{style:?} page {i}: {numbers:?}");
            // What a reader selects from the file is what the layout put on the page, line for line.
            let selected: Vec<String> = file.text(page).into_iter().filter(|l| !l.is_empty()).collect();
            let laid: Vec<String> = doc.rows(i).into_iter().map(|r| r.text.clone()).collect();
            assert_eq!(selected, laid, "{style:?} page {i}");
        }
        let info = file.info();
        assert_eq!(info.get("Title").map(String::as_str), Some("Crime and Punishment"));
        assert_eq!(info.get("Author").map(String::as_str), Some("Fyodor Dostoevsky"));
        assert_eq!(info.get("Creator").map(String::as_str), Some("Dostoevsky"));
    }
}

#[test]
fn every_face_a_page_names_is_in_the_file_with_outlines_and_a_map_to_characters() {
    let (_, bytes) = pdf_of(&sample(), &opts(Style::Book));
    let file = Pdf::parse(&bytes);
    let mut faces = 0;
    for page in file.pages() {
        for (name, font) in file.fonts(page) {
            faces += 1;
            assert_eq!(file.member(font, b"Subtype").and_then(Object::name), Some(&b"Type0"[..]), "{name:?}");
            assert_eq!(file.member(font, b"Encoding").and_then(Object::name), Some(&b"Identity-H"[..]), "{name:?}");
            let (program, _) = program(&file, font);
            let face = ttf_parser::Face::parse(&program, 0).unwrap_or_else(|e| panic!("{name:?}: outlines that parse: {e}"));
            assert!(face.tables().glyf.is_some() || face.tables().cff.is_some(), "{name:?}: outlines of one kind or the other");
            assert!(!file.unicode(font).is_empty(), "{name:?}: a map back to characters");
        }
    }
    assert!(faces >= 3, "the body, the heading, code: {faces} faces on the pages");
}

#[test]
fn a_face_goes_in_only_as_far_as_the_book_shows_of_it() {
    let book = Book { title: "T".into(), author: String::new(), language: None, chapters: vec![Chapter { markdown: format!("{PARA}\n"), ..Default::default() }] };
    let (doc, bytes) = pdf_of(&book, &opts(Style::Page));
    let file = Pdf::parse(&bytes);
    let fonts = file.fonts(file.pages()[0]);
    assert_eq!(fonts.len(), 1, "one face on the page");
    let (program, _) = program(&file, fonts.values().next().unwrap());
    let face = ttf_parser::Face::parse(&program, 0).unwrap();
    let shown: BTreeSet<u16> = doc.pages[0].items.iter().filter_map(|i| match i {
        Item::Text(run) => Some(&run.glyphs),
        Item::Rule { .. } => None,
    }).flatten().map(|g| g.glyph).collect();
    assert_eq!(face.number_of_glyphs() as usize, shown.len() + 1, "just what the page shows, and the missing glyph");
    assert!(program.len() < 40_000, "{} bytes of the face's 135 000", program.len());
    // Each has its outline: a closed shape, not empty.
    struct Count(usize);
    impl ttf_parser::OutlineBuilder for Count {
        fn move_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {}
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
        fn close(&mut self) {}
    }
    let code = file.unicode(fonts.values().next().unwrap()).iter().find(|(_, t)| t.as_str() == "a").map(|(c, _)| *c).expect("a's code");
    assert!(face.outline_glyph(ttf_parser::GlyphId(code), &mut Count(0)).is_some(), "the letter has an outline");
}

#[test]
fn the_same_pages_give_the_same_bytes() {
    let (_, first) = pdf_of(&sample(), &opts(Style::Manuscript));
    let (_, again) = pdf_of(&sample(), &opts(Style::Manuscript));
    assert_eq!(first, again);
    let mut other = opts(Style::Manuscript);
    other.page_numbers = false;
    assert_ne!(first, pdf_of(&sample(), &other).1, "and other pages give other bytes");
}

#[test]
fn a_variable_face_goes_in_as_static_outlines() {
    // Caveat is a variable font that ships one face: bold is stroked for it, an italic leaned.
    let mut o = opts(Style::Page);
    o.text.font = "Caveat".into();
    o.text.heading_font = Some("Bad Script".into());
    let book = Book { title: "T".into(), author: String::new(), language: None, chapters: vec![Chapter { markdown: format!("# *A heading*\n\n{PARA}\n"), ..Default::default() }] };
    let (doc, bytes) = pdf_of(&book, &o);
    let file = Pdf::parse(&bytes);
    let page = &file.pages()[0];
    assert_eq!(file.text(page).into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>(), doc.rows(0).into_iter().map(|r| r.text.clone()).collect::<Vec<_>>());
    let styles: Vec<pdf::layout::Style> = doc.pages[0].items.iter().filter_map(|i| match i {
        Item::Text(run) => Some(run.style),
        _ => None,
    }).collect();
    for (_, font) in file.fonts(page) {
        let (program, descriptor) = program(&file, font);
        let face = ttf_parser::Face::parse(&program, 0).expect("a static font with outlines");
        assert!(face.tables().glyf.is_some() && face.tables().fvar.is_none(), "instanced to TrueType outlines");
        // "ABCDEF+BadScript-Regular,Italic": the face, and the style drawn without its own.
        let base = String::from_utf8_lossy(file.member(font, b"BaseFont").and_then(Object::name).unwrap_or_default()).to_lowercase();
        let of_this_face = |flag: fn(&pdf::layout::Style) -> bool| styles.iter().filter(|s| base.contains(&s.font.rec.family.replace(' ', ""))).any(&flag);
        let flags = file.member(descriptor, b"Flags").and_then(Object::number).unwrap_or(0.) as u32;
        assert_eq!(flags & (1 << 18) != 0, of_this_face(|s| s.font.faux_bold), "a stroked bold says so");
        assert_eq!(flags & (1 << 6) != 0, of_this_face(|s| s.font.faux_italic), "a leaned italic says so");
    }
}

#[test]
fn an_empty_book_is_still_a_file_with_one_page() {
    let (doc, bytes) = pdf_of(&Book::default(), &opts(Style::Page));
    assert_eq!(doc.pages.len(), 1);
    let file = Pdf::parse(&bytes);
    assert_eq!(file.pages().len(), 1);
    assert!(file.text(file.pages()[0]).iter().all(String::is_empty));
}
