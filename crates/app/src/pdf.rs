//! The final text as pages: a PDF, the Export sheet's preview bitmaps, printing. On the Mac
//! TextKit sets the `pdf` crate's page composition and PDFKit prints (runs off the main thread:
//! TextKit objects owned by one thread are fine); elsewhere the crate's own engine sets and draws
//! the pages (dsky-fbso.3), hyphenated by `hyphen.rs`'s patterns, and printing waits for its
//! own bead.

#[cfg(not(target_os = "macos"))]
use std::sync::Arc;

#[cfg(target_os = "macos")]
use ::pdf::compose::{Align, Para, ParaLook, RunLook, compose, opening_drop};

pub use ::pdf::Bitmap;

#[derive(Clone, Default)]
pub struct Pdf {
    pub bytes: Vec<u8>,
    pub pages: usize,
    /// The pages as the engine laid them out, for the preview's bitmaps.
    #[cfg(not(target_os = "macos"))]
    laid: Option<Arc<::pdf::Document>>,
}

#[cfg(target_os = "macos")]
pub use mac::{print, rasterize, render};

#[cfg(not(target_os = "macos"))]
pub use engine::{print, rasterize, render};

#[cfg(not(target_os = "macos"))]
mod engine {
    use std::sync::OnceLock;

    use export::{Book, Options};

    use super::{Bitmap, Pdf};

    /// The app's own faces first, then the system's: read once, kept for the app's life.
    fn fonts() -> &'static ::pdf::Fonts {
        static FONTS: OnceLock<::pdf::Fonts> = OnceLock::new();
        FONTS.get_or_init(|| ::pdf::Fonts::new(&crate::theme::FONT_FILES))
    }

    pub fn render(book: &Book, o: &Options) -> Pdf {
        // The book's language hyphenates, as the page does (hyphen.rs).
        let lang = book.language.as_deref().and_then(|code| {
            let primary = code.split(['-', '_']).next().unwrap_or("");
            prose::Lang::ALL.into_iter().find(|l| l.code().eq_ignore_ascii_case(primary))
        });
        let hyphenate = |word: &str| lang.map(|l| crate::hyphen::breaks(word, l).to_vec()).unwrap_or_default();
        let pdf = ::pdf::render(book, o, fonts(), &hyphenate);
        Pdf { pages: pdf.pages(), bytes: pdf.bytes, laid: Some(pdf.laid) }
    }

    /// `index` from 0; `width` in pixels.
    pub fn rasterize(pdf: &Pdf, index: usize, width: u32) -> Option<Bitmap> {
        ::pdf::raster::rasterize(pdf.laid.as_ref()?, index, width)
    }

    /// No print dialog off the Mac yet: false, as when the Mac's panel couldn't open.
    pub fn print(_: &[u8], _: &str, _: &gpui_kit::Window) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Once;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_app_kit::{
        NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontDescriptor, NSFontDescriptorSymbolicTraits, NSFontFamilyAttribute, NSGraphicsContext, NSLayoutManager, NSMutableParagraphStyle, NSTextAlignment, NSTextContainer, NSTextStorage,
    };
    use objc2_core_foundation::{CFData, CFDictionary, CFMutableData, CFRetained, CGFloat, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGDataConsumer, CGDataProvider, CGFont, CGImageAlphaInfo, CGImageByteOrderInfo, CGPDFBox, CGPDFContextBeginPage, CGPDFContextClose, CGPDFContextCreate, CGPDFContextEndPage, CGPDFDocument, CGPDFPage};
    use objc2_foundation::{NSAttributedString, NSAttributedStringKey, NSMutableAttributedString, NSMutableDictionary, NSNumber, NSPoint, NSRange, NSString};

    use super::{Align, Bitmap, Para, ParaLook, Pdf, RunLook, compose, opening_drop};
    use export::{Book, Options};

    /// So TextKit finds the app's fonts the way the page does.
    fn register_fonts() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            for bytes in crate::theme::FONT_FILES {
                let data = CFData::from_bytes(bytes);
                let Some(provider) = CGDataProvider::with_cf_data(Some(&data)) else { continue };
                let Some(font) = CGFont::with_data_provider(&provider) else { continue };
                // SAFETY: a valid font; errors aren't wanted (a font already there is fine). The
                // call is deprecated for apps that ship font files; ours are inside the binary.
                #[allow(deprecated)]
                unsafe { objc2_core_text::CTFontManagerRegisterGraphicsFont(&font, std::ptr::null_mut()) };
            }
        });
    }

    fn font(family: &str, size: f64, bold: bool, italic: bool) -> Retained<NSFont> {
        let attrs: Retained<NSMutableDictionary<objc2_app_kit::NSFontDescriptorAttributeName, AnyObject>> = NSMutableDictionary::new();
        // SAFETY: a static key.
        attrs.insert(unsafe { NSFontFamilyAttribute }, &*NSString::from_str(family));
        // SAFETY: the attributes are a family name.
        let d = unsafe { NSFontDescriptor::fontDescriptorWithFontAttributes(Some(&attrs)) };
        let mut traits = NSFontDescriptorSymbolicTraits::empty();
        if bold {
            traits |= NSFontDescriptorSymbolicTraits::TraitBold;
        }
        if italic {
            traits |= NSFontDescriptorSymbolicTraits::TraitItalic;
        }
        let d = if traits.is_empty() { d } else { d.fontDescriptorWithSymbolicTraits(traits) };
        NSFont::fontWithDescriptor_size(&d, size as CGFloat)
            .filter(|f| f.familyName().is_some_and(|n| n.to_string() == family) || traits.is_empty())
            .or_else(|| NSFont::fontWithDescriptor_size(&d, size as CGFloat))
            .or_else(|| NSFont::fontWithName_size(&NSString::from_str("Times New Roman"), size as CGFloat))
            .unwrap_or_else(|| NSFont::systemFontOfSize(size as CGFloat))
    }

    fn para_style(l: &ParaLook) -> Retained<NSMutableParagraphStyle> {
        let p = NSMutableParagraphStyle::new();
        p.setAlignment(match l.align {
            Align::Left => NSTextAlignment::Left,
            Align::Center => NSTextAlignment::Center,
            Align::Justified => NSTextAlignment::Justified,
        });
        // AppKit measures the first line from the margin too: a quote's lines all start at its indent.
        p.setFirstLineHeadIndent(if l.head_indent > 0. && l.first_indent == 0. { l.head_indent } else { l.first_indent });
        p.setHeadIndent(l.head_indent);
        p.setParagraphSpacingBefore(l.before);
        p.setParagraphSpacing(l.after);
        p.setMinimumLineHeight(l.line);
        p.setMaximumLineHeight(l.line);
        if l.hyphenate {
            p.setHyphenationFactor(1.0);
        }
        p
    }

    struct Fonts<'a> {
        family: &'a str,
        heading: Option<&'a str>,
    }

    fn attributed(paras: &[Para], fonts: &Fonts, lang: Option<&str>) -> Retained<NSMutableAttributedString> {
        let s = NSMutableAttributedString::new();
        let lang_key = NSString::from_str("NSLanguage");
        let lang_val = lang.map(NSString::from_str);
        let black = NSColor::colorWithSRGBRed_green_blue_alpha(0.08, 0.07, 0.09, 1.);
        let grey = NSColor::colorWithSRGBRed_green_blue_alpha(0.28, 0.26, 0.3, 1.);
        for (pi, p) in paras.iter().enumerate() {
            let style = para_style(&p.look);
            let mut pieces = p.pieces.clone();
            if pi + 1 < paras.len() {
                let look = pieces.last().map(|(_, l)| *l).unwrap_or(RunLook { size: p.look.line / 1.4, bold: false, italic: false, mono: false, heading: false, strike: false, underline: false, grey: false, kern: 0. });
                pieces.push(("\n".into(), look));
            }
            for (text, look) in pieces {
                if text.is_empty() {
                    continue;
                }
                let family = if look.mono { "Menlo" } else if look.heading { fonts.heading.unwrap_or(fonts.family) } else { fonts.family };
                let f = font(family, look.size, look.bold, look.italic);
                let attrs: Retained<NSMutableDictionary<NSAttributedStringKey, AnyObject>> = NSMutableDictionary::new();
                // SAFETY: static AppKit keys, values of the types they take.
                unsafe {
                    attrs.insert(objc2_app_kit::NSFontAttributeName, &*f);
                    attrs.insert(objc2_app_kit::NSParagraphStyleAttributeName, &*style);
                    attrs.insert(objc2_app_kit::NSForegroundColorAttributeName, if look.grey { &*grey } else { &*black });
                    if look.kern != 0. {
                        attrs.insert(objc2_app_kit::NSKernAttributeName, &*NSNumber::new_f64(look.kern));
                    }
                    if look.strike {
                        attrs.insert(objc2_app_kit::NSStrikethroughStyleAttributeName, &*NSNumber::new_isize(1));
                    }
                    if look.underline {
                        attrs.insert(objc2_app_kit::NSUnderlineStyleAttributeName, &*NSNumber::new_isize(1));
                    }
                }
                if let Some(l) = &lang_val {
                    attrs.insert(&*lang_key, &**l);
                }
                // SAFETY: the attributes above are valid for an attributed string.
                let a = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(&text), &attrs) };
                s.appendAttributedString(&a);
            }
        }
        s
    }

    fn label(text: &str, family: &str, size: f64, bold: bool, italic: bool) -> Retained<NSAttributedString> {
        let attrs: Retained<NSMutableDictionary<NSAttributedStringKey, AnyObject>> = NSMutableDictionary::new();
        let f = font(family, size, bold, italic);
        let c = NSColor::colorWithSRGBRed_green_blue_alpha(0.08, 0.07, 0.09, 1.);
        // SAFETY: static AppKit keys.
        unsafe {
            attrs.insert(objc2_app_kit::NSFontAttributeName, &*f);
            attrs.insert(objc2_app_kit::NSForegroundColorAttributeName, &*c);
            NSAttributedString::new_with_attributes(&NSString::from_str(text), &attrs)
        }
    }

    /// The PDF context flipped so y grows down, as TextKit draws.
    struct Page<'a> {
        ctx: &'a CGContext,
        gc: Retained<NSGraphicsContext>,
    }

    impl<'a> Page<'a> {
        fn begin(ctx: &'a CGContext, height: f64) -> Page<'a> {
            // SAFETY: a PDF context; no page info.
            unsafe { CGPDFContextBeginPage(Some(ctx), None) };
            CGContext::save_g_state(Some(ctx));
            CGContext::translate_ctm(Some(ctx), 0., height);
            CGContext::scale_ctm(Some(ctx), 1., -1.);
            let gc = NSGraphicsContext::graphicsContextWithCGContext_flipped(ctx, true);
            NSGraphicsContext::setCurrentContext(Some(&gc));
            Page { ctx, gc }
        }

        fn end(self) {
            let _ = &self.gc;
            NSGraphicsContext::setCurrentContext(None);
            CGContext::restore_g_state(Some(self.ctx));
            CGPDFContextEndPage(Some(self.ctx));
        }
    }

    fn draw_at(a: &NSAttributedString, x: f64, y: f64) {
        a.drawAtPoint(CGPoint::new(x, y));
    }

    pub fn render(book: &Book, o: &Options) -> Pdf {
        objc2::rc::autoreleasepool(|_| render_inner(book, o))
    }

    fn render_inner(book: &Book, o: &Options) -> Pdf {
        register_fonts();
        let (w, h) = o.paper.size();
        let (w, h) = (w as f64, h as f64);
        let m = o.paper.margin() as f64;
        let (cw, ch) = (w - 2. * m, h - 2. * m);
        let Some(data) = CFMutableData::new(None, 0) else { return Pdf::default() };
        let Some(consumer) = CGDataConsumer::with_cf_data(Some(&data)) else { return Pdf::default() };
        let media = CGRect::new(CGPoint::new(0., 0.), CGSize::new(w, h));
        let info: Retained<NSMutableDictionary<NSString, AnyObject>> = NSMutableDictionary::new();
        info.insert(&*NSString::from_str("Title"), &*NSString::from_str(book.title.trim()));
        info.insert(&*NSString::from_str("Author"), &*NSString::from_str(book.author.trim()));
        info.insert(&*NSString::from_str("Creator"), &*NSString::from_str("Dostoevsky"));
        // SAFETY: NSDictionary and CFDictionary are toll-free bridged.
        let info_cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&info) as *const CFDictionary) };
        // SAFETY: a valid consumer and box.
        let Some(ctx) = (unsafe { CGPDFContextCreate(Some(&consumer), &media, Some(info_cf)) }) else { return Pdf::default() };

        let fonts = Fonts { family: &o.text.font, heading: o.text.heading_font.as_deref() };
        let lang = book.language.as_deref();
        let size = o.text.size as f64;
        let manuscript = o.style == export::Style::Manuscript;
        let mut pages = 0usize;
        let mut number = 0usize;

        let furniture = |number: usize| {
            if !o.page_numbers {
                return;
            }
            if manuscript {
                let title = book.title.trim().to_uppercase();
                let key: String = title.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
                let head = [book.surname().to_string(), key, number.to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" / ");
                let a = label(&head, &o.text.font, size, false, false);
                let sz = a.size();
                draw_at(&a, w - m - sz.width, m / 2. - sz.height / 2.);
            } else {
                let a = label(&number.to_string(), &o.text.font, size * 0.85, false, false);
                let sz = a.size();
                draw_at(&a, (w - sz.width) / 2., h - m / 2. - sz.height / 2.);
            }
        };

        if o.title_page {
            let page = Page::begin(&ctx, h);
            let title = book.title.trim();
            let author = book.author.trim();
            if manuscript {
                let name = label(author, &o.text.font, size, false, false);
                draw_at(&name, m, m);
                let words = label(&format!("about {} words", group(book.rounded_words())), &o.text.font, size, false, false);
                draw_at(&words, w - m - words.size().width, m);
                let t = label(&title.to_uppercase(), &o.text.font, size, false, false);
                draw_at(&t, (w - t.size().width) / 2., h / 2. - size * 2.);
                if !author.is_empty() {
                    let by = label(&format!("by {author}"), &o.text.font, size, false, false);
                    draw_at(&by, (w - by.size().width) / 2., h / 2.);
                }
            } else {
                let t = label(title, fonts.heading.unwrap_or(fonts.family), size * 2.2, false, o.style == export::Style::Book);
                let tsz = t.size();
                draw_at(&t, (w - tsz.width.min(cw)) / 2., m + ch / 3.);
                if !author.is_empty() {
                    let a = label(author, &o.text.font, size * 1.1, false, true);
                    draw_at(&a, (w - a.size().width) / 2., m + ch / 3. + tsz.height + size * 1.5);
                }
            }
            page.end();
            pages += 1;
            number += 1;
        }

        let chapters = book.blocks(o.chapter_titles);
        let empty = chapters.iter().all(|c| c.blocks.is_empty() && c.part.is_none());
        for c in &chapters {
            if let Some(part) = &c.part {
                let page = Page::begin(&ctx, h);
                let a = label(&part.trim().to_uppercase(), &o.text.font, size, false, false);
                draw_at(&a, (w - a.size().width) / 2., m + ch / 3.);
                page.end();
                pages += 1;
                number += 1;
            }
            if c.blocks.is_empty() {
                continue;
            }
            let paras = compose(&c.blocks, o);
            let text = attributed(&paras, &fonts, lang);
            let storage = NSTextStorage::new();
            storage.appendAttributedString(&text);
            let lm = NSLayoutManager::new();
            storage.addLayoutManager(&lm);
            let drop = (ch * opening_drop(o, matches!(c.blocks.first(), Some(export::Block::Heading { .. })))).round();
            let total = lm.numberOfGlyphs();
            let mut containers: Vec<(Retained<NSTextContainer>, NSRange, f64)> = Vec::new();
            loop {
                let top = if containers.is_empty() { drop } else { 0. };
                let tc = NSTextContainer::initWithSize(NSTextContainer::alloc(), CGSize::new(cw, ch - top));
                tc.setLineFragmentPadding(0.);
                lm.addTextContainer(&tc);
                let r = lm.glyphRangeForTextContainer(&tc);
                let done = r.location + r.length >= total;
                let stuck = r.length == 0 && containers.len() > 2;
                containers.push((tc, r, top));
                if done || stuck || containers.len() > 20_000 {
                    break;
                }
            }
            for (_, r, top) in &containers {
                if r.length == 0 && pages > 0 && containers.len() > 1 {
                    continue;
                }
                let page = Page::begin(&ctx, h);
                lm.drawBackgroundForGlyphRange_atPoint(*r, NSPoint::new(m, m + top));
                lm.drawGlyphsForGlyphRange_atPoint(*r, NSPoint::new(m, m + top));
                number += 1;
                furniture(number);
                page.end();
                pages += 1;
            }
        }
        if pages == 0 || (empty && !o.title_page && pages == 0) {
            let page = Page::begin(&ctx, h);
            page.end();
            pages += 1;
        }
        CGPDFContextClose(Some(&ctx));
        drop(ctx);
        let bytes = cf_bytes(&data);
        Pdf { bytes, pages }
    }

    fn cf_bytes(data: &CFMutableData) -> Vec<u8> {
        let d: &CFData = data;
        let len = d.length() as usize;
        let ptr = d.byte_ptr();
        if ptr.is_null() || len == 0 {
            return Vec::new();
        }
        // SAFETY: CFData's bytes, `len` long, alive while `data` is.
        unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
    }

    fn document(pdf: &[u8]) -> Option<CFRetained<CGPDFDocument>> {
        let data = CFData::from_bytes(pdf);
        let provider = CGDataProvider::with_cf_data(Some(&data))?;
        CGPDFDocument::with_provider(Some(&provider))
    }

    /// `index` from 0; `width` in pixels.
    pub fn rasterize(pdf: &Pdf, index: usize, width: u32) -> Option<Bitmap> {
        objc2::rc::autoreleasepool(|_| {
            let doc = document(&pdf.bytes)?;
            let page: CFRetained<CGPDFPage> = CGPDFDocument::page(Some(&doc), index + 1)?;
            let mb = CGPDFPage::box_rect(Some(&page), CGPDFBox::MediaBox);
            if mb.size.width <= 0. {
                return None;
            }
            let scale = width as f64 / mb.size.width;
            let height = (mb.size.height * scale).round().max(1.) as u32;
            let stride = width as usize * 4;
            let mut buf = vec![0u8; stride * height as usize];
            let space = CGColorSpace::new_device_rgb()?;
            let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
            // SAFETY: `buf` outlives the context and is `stride × height` bytes.
            let ctx = unsafe { CGBitmapContextCreate(buf.as_mut_ptr().cast(), width as usize, height as usize, 8, stride, Some(&space), info) }?;
            CGContext::set_rgb_fill_color(Some(&ctx), 1., 1., 1., 1.);
            CGContext::fill_rect(Some(&ctx), CGRect::new(CGPoint::new(0., 0.), CGSize::new(width as f64, height as f64)));
            CGContext::scale_ctm(Some(&ctx), scale, scale);
            CGContext::translate_ctm(Some(&ctx), -mb.origin.x, -mb.origin.y);
            CGContext::draw_pdf_page(Some(&ctx), Some(&page));
            drop(ctx);
            Some(Bitmap { width, height, bgra: buf })
        })
    }

    /// False when the print panel couldn't open.
    pub fn print(pdf: &[u8], title: &str, window: &gpui_kit::Window) -> bool {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSView;
        use objc2_foundation::NSData;
        use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Some(mtm) = MainThreadMarker::new() else { return false };
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return false };
        // SAFETY: AppKit handles carry the window's content view, alive while the window is.
        let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
        let Some(ns_window) = view.window() else { return false };
        let data = NSData::with_bytes(pdf);
        // SAFETY: PDF bytes; PDFKit copies what it needs.
        let Some(doc) = (unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }) else { return false };
        let info = objc2_app_kit::NSPrintInfo::sharedPrintInfo();
        // SAFETY: on the main thread, with the shared print info.
        let Some(op) = (unsafe { doc.printOperationForPrintInfo_scalingMode_autoRotate(Some(&info), PDFPrintScalingMode::PageScaleNone, true, mtm) }) else { return false };
        op.setJobTitle(Some(&NSString::from_str(title)));
        op.setShowsPrintPanel(true);
        // SAFETY: no delegate, so no selector is called back.
        unsafe { op.runOperationModalForWindow_delegate_didRunSelector_contextInfo(&ns_window, None, None, std::ptr::null_mut()) };
        true
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
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use export::{Book, Options, Paper, Style, Text};

    #[test]
    fn a_book_becomes_pages() {
        let md = "# The garret\n\n".to_string() + &"On an exceptionally hot evening early in July a young man came out of the garret in which he lodged. ".repeat(60);
        let book = Book { title: "Crime and Punishment".into(), author: "Fyodor Dostoevsky".into(), language: Some("en".into()), chapters: vec![export::Chapter { markdown: md, ..Default::default() }, export::Chapter { markdown: "# Two\n\nShort.\n".into(), ..Default::default() }] };
        let mut o = Options::new(Style::Manuscript, Text::manuscript(), Paper::A4);
        o.title_page = true;
        let pdf = render(&book, &o);
        assert!(pdf.bytes.starts_with(b"%PDF"));
        assert!(pdf.pages >= 4, "title, chapter one over pages, chapter two: {}", pdf.pages);
        let page = rasterize(&pdf, 1, 200).unwrap();
        assert_eq!(page.width, 200);
        assert_eq!(page.bgra.len(), (page.width * page.height * 4) as usize);
        assert!(page.bgra.chunks(4).any(|p| p[0] < 128), "some ink on the page");
        assert!(rasterize(&pdf, pdf.pages + 3, 200).is_none());
    }
}

/// `cargo test -p dostoevsky pdf::samples -- --ignored` → raw BGRA in `target/export-sample/`.
#[cfg(all(test, target_os = "macos"))]
mod samples {
    use super::*;
    use export::{Book, Options, Paper, Style, Text};

    #[test]
    #[ignore]
    fn samples() {
        let para = "On an exceptionally hot evening early in July a young man came out of the garret in which he lodged in S. Place and walked slowly, as though in hesitation, towards K. bridge. He had successfully avoided meeting his landlady on the staircase. His garret was under the roof of a high, five-storied house and was more like a cupboard than a room.";
        let md = format!("# The garret\n\n{para}\n\nThe landlady who provided him with *garret, dinners, and attendance*, lived on the floor below, and every time he went out he was obliged to pass her kitchen, the door of which invariably stood open.\n\n> A quote that is a little long so that it wraps onto a second line in the page.\n\n- one item\n- two items\n\n---\n\n{para}\n\n{para}\n\n{para}\n");
        let book = Book { title: "Crime and Punishment".into(), author: "Fyodor Dostoevsky".into(), language: Some("en".into()), chapters: vec![export::Chapter { part: Some("Part One".into()), markdown: md, ..Default::default() }] };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/export-sample");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, style, text, paper) in [
            ("page", Style::Page, Text::page("IBM Plex Mono", 19., 1.8, true, true, Some("Bad Script")), Paper::A4),
            ("manuscript", Style::Manuscript, Text::manuscript(), Paper::Letter),
            ("book", Style::Book, Text::book(Paper::A5), Paper::A5),
        ] {
            let mut o = Options::new(style, text, paper);
            o.title_page = true;
            let pdf = render(&book, &o);
            std::fs::write(dir.join(format!("{name}.pdf")), &pdf.bytes).unwrap();
            for i in 0..pdf.pages.min(4) {
                let b = rasterize(&pdf, i, 700).unwrap();
                std::fs::write(dir.join(format!("{name}-{i}-{}x{}.bgra", b.width, b.height)), &b.bgra).unwrap();
            }
        }
    }
}
