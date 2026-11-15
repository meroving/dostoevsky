use std::time::{Duration, Instant};

use doc_model::{Document, Scope, Source};

use crate::model::DocModel;
use crate::ui_tests::Ui;

const PARA: &str = "Most of the friction in writing comes from choosing too early, and the other \
candidates quietly disappear before you can judge them in context with the real sentence.";

fn document(words: usize, groups: usize) -> String {
    let per = PARA.split_whitespace().count();
    let text = vec![PARA; words / per].join("\n\n") + "\n";
    let mut d = Document::from_text(&text);
    let stride = PARA.chars().count() + 2;
    for i in 0..groups {
        let at = i * stride + "Most of the ".len();
        let g = d.create_group(at..at + "friction".len(), Scope::Word).unwrap();
        for v in ["tension", "hesitation", "struggle"] {
            d.add_variant(g, v, Source::Me).unwrap();
        }
    }
    crate::format::serialize(&DocModel::new(d))
}

fn budget(ms: f64) -> Duration {
    let slack: f64 = std::env::var("PERF_SLACK").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    Duration::from_secs_f64(ms * slack / 1000.)
}

fn p99(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let at = |q: usize| samples[(samples.len() * q / 100).min(samples.len() - 1)].as_secs_f64() * 1e3;
    eprintln!("perf:   p50 {:.2} p90 {:.2} p99 {:.2} max {:.2} ms (n={})", at(50), at(90), at(99), at(100), samples.len());
    samples[(samples.len() * 99 / 100).min(samples.len() - 1)]
}

fn check(what: &str, got: Duration, budget: Duration) {
    eprintln!("perf: {what}: {:.2} ms (budget {:.1} ms)", got.as_secs_f64() * 1e3, budget.as_secs_f64() * 1e3);
    assert!(got <= budget, "{what}: {got:?} is over the NFR-1 budget of {budget:?}");
}

/// ED-8
#[test]
#[ignore = "perf: run in release with scripts/perf_check.sh"]
fn nfr_1_keystroke_to_paint_at_20k_words() {
    let mut ui = Ui::open(&document(20_000, 200));
    ui.with_app(crate::spell::Spelling::init);
    let mid = ui.text().chars().count() / 2;
    ui.caret_at(mid);
    let mut samples = Vec::new();
    for i in 0..320 {
        let t = Instant::now();
        ui.type_text(if i % 6 == 5 { " " } else { "x" });
        if i >= 20 {
            samples.push(t.elapsed());
        }
    }
    check("keystroke to paint, p99 at 20k words", p99(samples), budget(8.));
}

#[test]
#[ignore = "perf: run in release with scripts/perf_check.sh"]
fn nfr_1_cycling_at_20k_words() {
    let mut ui = Ui::open(&document(20_000, 200));
    let g = ui.groups()[1].id;
    ui.hover_group(g);
    let mut samples = Vec::new();
    for i in 0..220 {
        let t = Instant::now();
        ui.keys("down");
        if i >= 20 {
            samples.push(t.elapsed());
        }
    }
    check("cycle to paint, p99 at 20k words", p99(samples), budget(8.));
}

#[test]
#[ignore = "perf: run in release with scripts/perf_check.sh"]
fn nfr_1_open_100k_words() {
    let md = document(100_000, 300);
    // The first open in a process warms up font loading; time the second.
    let _ = Ui::open_timed(&md);
    let (ui, took) = Ui::open_timed(&md);
    assert!(ui.groups().len() >= 300);
    check("open 100k words to first frame", took, budget(300.));
}

/// PROJ-6
#[test]
#[ignore = "perf: run in release with scripts/perf_check.sh"]
fn nfr_1_keystroke_to_paint_beside_a_long_journal() {
    let dir = crate::ui_tests::TempDir::new("perf-journal");
    let mut n = 0;
    for year in 2021..=2024 {
        for month in 1..=12 {
            for day in 1..=28 {
                if n < 1200 {
                    dir.file(&format!("{year}-{month:02}-{day:02}.md"), "Slept badly, the radiator ticking all night.\n");
                    n += 1;
                }
            }
        }
    }
    let mut ui = Ui::open_folder(&dir.0);
    assert!(ui.read(|ws, _| ws.panel_open()));
    ui.type_text("Today ");
    let mut samples = Vec::new();
    for i in 0..200 {
        let t = Instant::now();
        ui.type_text(if i % 2 == 0 { "a" } else { " " });
        samples.push(t.elapsed());
    }
    check("keystroke beside 1,200 entries p99", p99(samples), budget(8.));
}

/// PROJ-5
#[test]
#[ignore = "perf: run in release with scripts/perf_check.sh"]
fn nfr_1_flipping_between_chapters() {
    let dir = crate::ui_tests::TempDir::new("perf-book");
    let chapter = document(8_000, 40);
    for i in 1..=10 {
        dir.file(&format!("{i:02}-chapter.md"), &chapter);
    }
    let mut ui = Ui::open_folder(&dir.0);
    ui.keys("cmd-p");
    let mut samples = Vec::new();
    for i in 0..40 {
        let t = Instant::now();
        ui.keys(if i % 20 < 9 { "down" } else if i % 20 == 9 || i % 20 == 19 { "cmd-p" } else { "up" });
        samples.push(t.elapsed());
    }
    check("flip between 8k-word chapters p99", p99(samples), budget(100.));
}

#[test]
#[ignore = "perf: run in release with scripts/perf_check.sh"]
fn nfr_1_keystroke_to_paint_beside_a_book() {
    let dir = crate::ui_tests::TempDir::new("perf-beside");
    dir.file("01-chapter.md", &document(20_000, 200));
    for i in 2..=30 {
        dir.file(&format!("{i:02}-chapter.md"), "Short.\n");
    }
    let mut ui = Ui::open_folder(&dir.0);
    assert!(ui.read(|ws, _| ws.panel_open()));
    let mid = ui.text().chars().count() / 2;
    ui.caret_at(mid);
    let mut samples = Vec::new();
    for i in 0..220 {
        let t = Instant::now();
        ui.type_text(if i % 6 == 5 { " " } else { "x" });
        if i >= 20 {
            samples.push(t.elapsed());
        }
    }
    check("keystroke beside a 20k-word book p99", p99(samples), budget(8.));
}
