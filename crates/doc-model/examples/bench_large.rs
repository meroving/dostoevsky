//! Latency check on a 100k-word document.
use doc_model::{Document, Scope, Source};
use std::time::Instant;

fn main() {
    let para =
        "Most of the friction in writing comes from choosing too early and losing the rest. "
            .repeat(4);
    let text = vec![para.trim_end(); 1600].join("\n\n");
    let mut d = Document::from_text(&text);
    println!(
        "chars: {}, words: ~{}",
        d.len(),
        text.split_whitespace().count()
    );

    let step = d.len() / 300;
    let mut groups = Vec::new();
    for i in 0..300 {
        let pos = d
            .text()
            .char_indices()
            .skip(i * step)
            .find(|(_, c)| *c == 'f')
            .map(|(b, _)| d.text()[..b].chars().count());
        if let Some(p) = pos {
            if let Ok(g) = d.create_group(p..p + 8, Scope::Word) {
                for v in ["tension", "hesitation", "struggle"] {
                    d.add_variant(g, v, Source::Me).unwrap();
                }
                groups.push(g);
            }
        }
    }
    println!("groups: {}", d.visible_groups().len());

    let mid = d.len() / 2;
    let t = Instant::now();
    for i in 0..1000 {
        d.replace(mid + i..mid + i, "x").unwrap();
    }
    println!(
        "typing:  {:>8.1} µs/keystroke",
        t.elapsed().as_secs_f64() * 1e6 / 1000.0
    );

    let t = Instant::now();
    for i in 0..1000 {
        d.cycle(groups[i % groups.len()], 1).unwrap();
    }
    println!(
        "cycling: {:>8.1} µs/cycle",
        t.elapsed().as_secs_f64() * 1e6 / 1000.0
    );

    let t = Instant::now();
    for _ in 0..100 {
        std::hint::black_box(d.text());
    }
    println!(
        "text():  {:>8.1} µs/call",
        t.elapsed().as_secs_f64() * 1e6 / 100.0
    );
    let t = Instant::now();
    for _ in 0..100 {
        std::hint::black_box(d.visible_groups());
    }
    println!(
        "visible_groups(): {:>8.1} µs/call",
        t.elapsed().as_secs_f64() * 1e6 / 100.0
    );
}
