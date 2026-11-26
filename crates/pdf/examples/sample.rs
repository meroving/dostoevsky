//! `cargo run -p pdf --example sample` → each style's PDF and its first pages as bitmaps (.bmp)
//! in `target/pdf-sample/`, set in the app's own faces: to look at what the engine makes.

#[path = "../tests/common/mod.rs"]
#[allow(dead_code)]
mod common;

use export::Style;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pdf-sample");
    std::fs::create_dir_all(&dir).unwrap();
    let fonts = common::bundled();
    for style in Style::ALL {
        let mut o = common::opts(style);
        o.title_page = true;
        let pdf = pdf::render(&common::sample(), &o, &fonts, &|_| Vec::new());
        let name = format!("{style:?}").to_lowercase();
        std::fs::write(dir.join(format!("{name}.pdf")), &pdf.bytes).unwrap();
        for i in 0..pdf.pages().min(4) {
            let b = pdf::rasterize(&pdf, i, 800).unwrap();
            std::fs::write(dir.join(format!("{name}-{i}.bmp")), bmp(&b)).unwrap();
        }
        println!("{name}: {} pages, {} bytes", pdf.pages(), pdf.bytes.len());
    }
    println!("{}", dir.display());
}

/// A 32-bit BMP: BGRA rows, bottom to top.
fn bmp(b: &pdf::Bitmap) -> Vec<u8> {
    let size = 54 + b.bgra.len() as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend(b"BM");
    out.extend(size.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((b.width as i32).to_le_bytes());
    out.extend((b.height as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0u8; 24]);
    for row in b.bgra.chunks_exact(b.width as usize * 4).rev() {
        out.extend(row);
    }
    out
}
