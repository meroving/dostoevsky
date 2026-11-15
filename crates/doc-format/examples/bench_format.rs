//! Save/load latency on a ~100k-word document:
//! `cargo run --release -p doc-format --example bench_format`
use doc_format::{parse, serialize, Preserved};
use doc_model::{Document, Scope, Source};
use std::time::Instant;

fn main() {
    let para =
        "Most of the friction in writing comes from choosing too early and losing the rest. "
            .repeat(4);
    let text = vec![para.trim_end(); 1600].join("\n\n") + "\n";
    let mut d = Document::from_text(&text);
    let chars: Vec<char> = text.chars().collect();
    let step = chars.len() / 300;
    for i in 0..300 {
        if let Some(p) = (i * step..chars.len()).find(|&j| chars[j..].starts_with(&['f', 'r'])) {
            if let Ok(g) = d.create_group(p..p + 8, Scope::Word) {
                for v in ["tension", "hesitation", "struggle"] {
                    d.add_variant(g, v, Source::Me).unwrap();
                }
            }
        }
    }
    for i in (0..1600).step_by(10) {
        let s = i * (para.trim_end().chars().count() + 2);
        d.ghost(s..s + 20).unwrap();
    }
    let t = Instant::now();
    let f = serialize(&d, true, &Preserved::default());
    println!(
        "serialize: {:>6.1} ms ({} KB)",
        t.elapsed().as_secs_f64() * 1e3,
        f.len() / 1024
    );
    let t = Instant::now();
    let l = parse(&f);
    println!("parse:     {:>6.1} ms", t.elapsed().as_secs_f64() * 1e3);
    assert!(l.warnings.is_empty());
    // Re-anchoring after an outside edit near the top shifts every block.
    let edited = format!("A new first paragraph.\n\n{f}");
    let t = Instant::now();
    let l = parse(&edited);
    println!(
        "parse after an outside edit: {:>6.1} ms ({} detached)",
        t.elapsed().as_secs_f64() * 1e3,
        l.detached().len()
    );
}
