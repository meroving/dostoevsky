#[path = "../../doc-model/tests/common/mod.rs"]
mod common;

use common::{session_strategy, session_strategy_with, Session};
use doc_format::{canonical, parse, serialize, Preserved};
use doc_model::Document;
use proptest::prelude::*;

fn tricky_text() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "a", "é", " ", "\n", "-->", "--", "-", ">", "\\", "\\\\", "<!--", "{\"", "}", "#",
        ]),
        0..6,
    )
    .prop_map(|v| v.concat())
    .boxed()
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000),
        ..ProptestConfig::default()
    }
}

/// A saved body ends in exactly one newline; give the model the same shape.
fn normalize_end(d: &mut Document) {
    let mut i = d.len();
    while i > 0 {
        let shown = d.ghost_ranges().iter().any(|r| r.start < i && i <= r.end);
        let c = d.text_range(i - 1..i);
        if shown {
            i -= 1;
            continue;
        }
        if c != "\n" {
            break;
        }
        d.replace(i - 1..i, "").unwrap();
        i -= 1;
    }
    let l = d.len();
    d.replace(l..l, "\n").unwrap();
}

/// The shape a save gives: all-cut versions keep their text as their own, the end normalized.
fn build(session: &Session) -> Document {
    let mut d = session.build();
    // Normalizing the end can leave a version that is all cuts: repeat until stable.
    for _ in 0..8 {
        let before = d.snapshot();
        d = Document::from_snapshot(canonical(&before)).unwrap();
        normalize_end(&mut d);
        if d.snapshot() == before {
            break;
        }
    }
    d
}

fn check_round_trip(session: &Session, layer: bool) -> Result<(), TestCaseError> {
    let d = build(session);
    let f = serialize(&d, layer, &Preserved::default());
    let l = parse(&f);
    prop_assert!(l.warnings.is_empty(), "{:?}\n{}", l.warnings, f);
    prop_assert!(l.detached().is_empty());
    l.doc.check_invariants().map_err(TestCaseError::fail)?;
    let expected = Document::from_snapshot(canonical(&d.snapshot()))
        .unwrap()
        .snapshot();
    prop_assert_eq!(
        l.doc.snapshot(),
        expected,
        "parse(serialize(d)) == d\n{}",
        f
    );
    if f.contains("<!-- dostoevsky:") {
        prop_assert_eq!(l.layer, layer);
    } else {
        prop_assert_eq!(&f, &d.text(), "no trailer: the text, byte for byte");
    }
    prop_assert_eq!(
        l.serialize(),
        f,
        "serialize(parse(f)) == f for files we wrote"
    );
    Ok(())
}

/// Guards against a vacuous round-trip test.
#[test]
fn generator_covers_the_interesting_shapes() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let mut seen = [0usize; 7];
    let probes = [
        "\"kind\":\"comment\"",
        "\"kind\":\"alt\"",
        "\"parent\"",       // nested groups
        "\"at\":{\"note\"", // groups in notes (and in cuts)
        "\"trace\":",      // cuts with a spot
        "\"trace\":\"mark\"", // a cut's own look
        "<!-- dostoevsky:overflow",
    ];
    for _ in 0..500 {
        let s = session_strategy().new_tree(&mut runner).unwrap().current();
        let f = serialize(&s.build(), true, &Preserved::default());
        for (n, p) in seen.iter_mut().zip(probes) {
            *n += usize::from(f.contains(p));
        }
    }
    for (n, p) in seen.iter().zip(probes) {
        assert!(*n >= 10, "only {n}/500 documents have {p}");
    }
}

fn alt_records(s: &str) -> usize {
    s.lines().filter(|l| l.contains("\"kind\":\"alt\"")).count()
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn documents_round_trip(session in session_strategy(), layer in any::<bool>()) {
        check_round_trip(&session, layer)?;
    }

    #[test]
    fn escaping_round_trips(
        session in session_strategy_with("[a-z .\n]{0,30}", tricky_text()),
        layer in any::<bool>(),
    ) {
        check_round_trip(&session, layer)?;
    }

    /// FILE-2
    #[test]
    fn plain_markdown_is_byte_identical(
        pieces in prop::collection::vec(
            prop::sample::select(vec![
                "a", "b", "é", " ", "\t", "\n", "\n\n", "\r\n", "\r", "#", "- ", "* ", "> ",
                "```", "1. ", "|", "<!--", "-->", "<!-- x -->", "\u{feff}", "---\n", "    ",
            ]),
            0..40,
        ),
        bom in any::<bool>(),
    ) {
        let mut f = pieces.concat();
        if bom {
            f.insert(0, '\u{feff}');
        }
        let l = parse(&f);
        prop_assert!(l.warnings.is_empty());
        prop_assert_eq!(l.serialize(), f);
    }

    #[test]
    fn outside_edits_never_drop_groups(
        session in session_strategy(),
        a in any::<u16>(),
        b in any::<u16>(),
        insert in "[a-z .\n]{0,12}",
    ) {
        let d = build(&session);
        let f = serialize(&d, true, &Preserved::default());
        let Some(split) = f.find("\n<!-- dostoevsky:") else {
            return Ok(());
        };
        let body: Vec<char> = f[..split].chars().collect();
        let r = common::range(body.len(), a, b);
        let edited: String = body[..r.start]
            .iter()
            .chain(insert.chars().collect::<Vec<_>>().iter())
            .chain(body[r.end..].iter())
            .collect();
        let f2 = format!("{edited}{}", &f[split..]);
        let l = parse(&f2);
        l.doc.check_invariants().map_err(TestCaseError::fail)?;
        let live = l.doc.all_group_ids().len();
        let detached: usize = l
            .detached()
            .iter()
            .map(|g| g.records.iter().filter(|r| r["kind"] == "alt").count())
            .sum();
        prop_assert_eq!(live + detached, alt_records(&f2), "no group is dropped\n{}", f2);
        prop_assert_eq!(l.doc.cuts().len(), parse(&f).doc.cuts().len(), "no cut is dropped\n{}", f2);
        let saved = l.serialize();
        prop_assert_eq!(alt_records(&saved), alt_records(&f2));
        prop_assert_eq!(parse(&saved).serialize(), saved.clone(), "stable after re-anchoring");
    }
}
