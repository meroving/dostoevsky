mod common;

use common::{range, session_strategy};
use doc_model::snapshot::{DocTree, SnapshotError, TreeGroup, TreeNode, TreeNote, TreeVariant};
use doc_model::{Document, GroupId, Scope, Source};
use proptest::prelude::*;

fn observe(d: &Document) -> impl PartialEq + std::fmt::Debug {
    let groups: Vec<_> = d
        .all_group_ids()
        .into_iter()
        .map(|id| {
            let g = d.group(id).unwrap();
            let v: Vec<_> = g.variants.into_iter().map(|v| (v.by, v.text)).collect();
            (id, g.scope, g.active, g.ghosted, v)
        })
        .collect();
    let notes: Vec<_> = d.notes().into_iter().map(|n| (n.text, n.origin, n.source, n.paragraph, n.lost, n.look, n.batch)).collect();
    let cuts: Vec<_> = d.cuts().into_iter().map(|c| (c.text, c.place, c.look, c.paragraph, c.batch, c.lost)).collect();
    (
        d.text(),
        d.clean_text(),
        d.ghost_ranges(),
        d.visible_groups(),
        notes,
        cuts,
        groups,
    )
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2000),
        ..ProptestConfig::default()
    })]

    #[test]
    fn snapshot_round_trip(session in session_strategy(), a in any::<u16>(), b in any::<u16>()) {
        let mut d = session.build();
        // Freshness isn't part of a snapshot; a presentation change ends it.
        d.set_presentation(d.presentation());
        let tree = d.snapshot();
        let mut e = Document::from_snapshot(tree.clone()).unwrap();
        e.set_presentation(d.presentation());
        e.check_invariants().map_err(TestCaseError::fail)?;
        prop_assert_eq!(e.snapshot(), tree.clone());
        prop_assert_eq!(observe(&e), observe(&d));
        prop_assert!(!e.can_undo() && !e.can_redo());

        let r = range(e.len(), a, b);
        if let Ok(g) = e.create_group(r, Scope::Word) {
            e.add_variant(g, "new", Source::Ai).unwrap();
        }
        let all = e.copy(0..e.len()).unwrap();
        e.paste(0, &all).unwrap();
        e.check_invariants().map_err(TestCaseError::fail)?;

        let mut stripped = tree.clone();
        stripped.strip_ids();
        prop_assert!(stripped.eq_ignoring_ids(&tree));
    }
}

fn group(id: u64, active: usize, variants: &[&str]) -> TreeNode {
    TreeNode::Group(TreeGroup {
        id: GroupId(id),
        scope: Scope::Word,
        active,
        variants: variants
            .iter()
            .map(|t| TreeVariant {
                by: Source::Me,
                content: if t.is_empty() {
                    vec![]
                } else {
                    vec![TreeNode::text(*t)]
                },
                without: false,
            })
            .collect(),
    })
}

fn page(nodes: Vec<TreeNode>) -> DocTree {
    DocTree {
        page: nodes,
        notes: vec![],
    }
}

#[test]
fn invalid_groups_are_rejected() {
    let err = |t| Document::from_snapshot(t).err();
    assert_eq!(
        err(page(vec![group(1, 0, &[])])),
        Some(SnapshotError::NoVariants(GroupId(1)))
    );
    assert_eq!(
        err(page(vec![group(1, 2, &["a", "b"])])),
        Some(SnapshotError::ActiveOutOfRange(GroupId(1)))
    );
    assert_eq!(
        err(page(vec![group(1, 0, &["a", ""])])),
        Some(SnapshotError::EmptyVariant(GroupId(1)))
    );
}

#[test]
fn runs_are_normalized_and_duplicate_ids_replaced() {
    let mut t = page(vec![
        TreeNode::text("Most "),
        TreeNode::text("of\nthe "),
        group(7, 1, &["friction", "tension"]),
        TreeNode::text(""),
        group(7, 0, &["x", "y"]),
    ]);
    t.notes.push(TreeNote {
        content: vec![TreeNode::text("stashed")],
        origin: Some("Most of".into()),
        ..TreeNote::default()
    });
    let mut d = Document::from_snapshot(t).unwrap();
    d.check_invariants().unwrap();
    assert_eq!(d.text(), "Most of\nthe tensionx");
    assert_eq!(d.notes()[0].origin.as_deref(), Some("Most of"));
    let ids = d.all_group_ids();
    assert_eq!(ids[0], GroupId(7));
    assert!(ids[1].0 > 7, "the duplicate gets a fresh id");
    assert_eq!(
        d.snapshot().page[..2],
        [TreeNode::text("Most of\n"), TreeNode::text("the ")]
    );
    let g = d.create_group(0..4, Scope::Word).unwrap();
    assert!(!ids.contains(&g));
    d.check_invariants().unwrap();
}
