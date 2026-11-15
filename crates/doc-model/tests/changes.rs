mod common;

use common::{apply_loose, op_strategy};
use doc_model::{Change, Document, Scope, Source};
use proptest::prelude::*;

/// Per char: `Some(ghosted)` if unchanged, `None` inside a reported change.
fn ghost_flags(d: &Document) -> Vec<Option<bool>> {
    let mut v = vec![Some(false); d.len()];
    for r in d.ghost_ranges() {
        v[r].iter_mut().for_each(|x| *x = Some(true));
    }
    v
}

fn replay(text: &str, flags: &mut Vec<Option<bool>>, changes: &[Change]) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for c in changes {
        match c {
            Change::Text { old, new_len } => {
                chars.splice(old.clone(), std::iter::repeat_n('\u{0}', *new_len));
                flags.splice(old.clone(), std::iter::repeat_n(None, *new_len));
            }
            Change::Style(r) => flags[r.clone()].iter_mut().for_each(|x| *x = None),
            Change::Notes => {}
            Change::Reset => panic!("unexpected reset"),
        }
    }
    chars.into_iter().collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1000),
        ..ProptestConfig::default()
    })]

    #[test]
    fn changes_describe_every_edit(
        seed in "[a-z .\n]{0,40}",
        ops in prop::collection::vec(op_strategy(), 0..60),
    ) {
        let mut d = Document::from_text(&seed);
        prop_assert!(d.take_changes().is_empty());
        for op in &ops {
            let (text, notes, groups) = (d.text(), d.notes(), d.visible_groups());
            let mut flags = ghost_flags(&d);
            apply_loose(&mut d, op);
            let changes = d.take_changes();
            let replayed = replay(&text, &mut flags, &changes);
            let now = d.text();
            // Placeholders stand for new text; everything else must match exactly.
            prop_assert_eq!(replayed.chars().count(), now.chars().count(), "{:?} {:?}", op, changes);
            for (a, b) in replayed.chars().zip(now.chars()) {
                prop_assert!(a == '\u{0}' || a == b, "{:?}: {:?}", op, changes);
            }
            for (i, (f, g)) in flags.iter().zip(ghost_flags(&d)).enumerate() {
                if let Some(f) = f {
                    prop_assert_eq!(Some(*f), g, "ghost at {} outside {:?} after {:?}", i, changes, op);
                }
            }
            let page_changed = text != now || groups != d.visible_groups();
            let page_reported = changes.iter().any(|c| !matches!(c, Change::Notes));
            prop_assert!(!page_changed || page_reported, "{:?} unreported", op);
            if notes != d.notes() {
                prop_assert!(changes.contains(&Change::Notes));
            }
        }
    }
}

#[test]
fn typing_reports_a_tight_text_change() {
    let mut d = Document::from_text("one\ntwo\nthree");
    d.replace(5..5, "X").unwrap();
    assert_eq!(
        d.take_changes(),
        [Change::Text {
            old: 4..8,
            new_len: 5
        }],
        "one line (one run) is re-laid out"
    );
    assert!(d.take_changes().is_empty());
}

#[test]
fn cycling_inside_a_group_reports_the_group() {
    let mut d = Document::from_text("It was a friction point.\nNext line.");
    let g = d.create_group(9..17, Scope::Word).unwrap();
    assert_eq!(
        d.take_changes(),
        [Change::Style(0..25)],
        "the line was split"
    );
    d.add_variant(g, "hurdle", Source::Me).unwrap();
    assert_eq!(d.take_changes(), [Change::Style(9..17)], "dots changed");
    d.cycle(g, 1).unwrap();
    assert_eq!(d.text(), "It was a hurdle point.\nNext line.");
    assert_eq!(
        d.take_changes(),
        [Change::Text {
            old: 9..17,
            new_len: 6
        }],
        "only the group"
    );
    d.undo();
    assert_eq!(
        d.take_changes(),
        [Change::Text {
            old: 9..15,
            new_len: 8
        }]
    );
    d.ghost(0..3).unwrap();
    assert_eq!(d.take_changes(), [Change::Style(0..9)]);
    d.stash(0..3).unwrap();
    assert_eq!(
        d.take_changes(),
        [
            Change::Text {
                old: 0..3,
                new_len: 0
            },
            Change::Notes
        ]
    );
}

#[test]
fn nested_edits_descend_into_the_active_variant() {
    let mut d = Document::from_text("aaa bbb ccc\n");
    let outer = d.create_group(0..11, Scope::Paragraph).unwrap();
    d.add_variant(outer, "other", Source::Me).unwrap();
    d.create_group(4..7, Scope::Word).unwrap();
    d.take_changes();
    d.replace(5..5, "x").unwrap();
    assert_eq!(
        d.take_changes(),
        [Change::Text {
            old: 4..7,
            new_len: 4
        }]
    );
}

#[test]
fn a_long_backlog_collapses_into_reset() {
    let mut d = Document::from_text("");
    for i in 0..5000 {
        d.replace(i..i, "x").unwrap();
    }
    assert_eq!(d.take_changes(), [Change::Reset]);
    d.replace(0..0, "y").unwrap();
    assert_eq!(d.take_changes().len(), 1);
}
