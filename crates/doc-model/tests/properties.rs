//! Property tests: random sequences of every operation, with the document's guarantees
//! checked after each step.

mod common;

use common::{look, op_strategy, pick, pick_comment, pick_cut, place, presentation, range, Op};
use doc_model::{Carried, CutId, CutPlace, Document, GroupId, Place, Scope, Source};
use proptest::prelude::*;

fn splice(s: &str, r: std::ops::Range<usize>, t: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[..r.start]
        .iter()
        .chain(t.chars().collect::<Vec<_>>().iter())
        .chain(chars[r.end..].iter())
        .collect()
}

fn ghosted_chars(d: &Document) -> Vec<bool> {
    let mut v = vec![false; d.len()];
    for r in d.ghost_ranges() {
        v[r].iter_mut().for_each(|x| *x = true);
    }
    v
}

/// Without the chars of shown cuts: what a copy carries.
fn kept_in(d: &Document, text: &str, r: std::ops::Range<usize>) -> String {
    let g = ghosted_chars(d);
    text.chars().enumerate().filter(|(i, _)| r.contains(i) && !g[*i]).map(|(_, c)| c).collect()
}

/// A version that was all one cut goes when the cut does.
fn versions(d: &Document) -> usize {
    d.all_group_ids().into_iter().map(|g| d.group(g).unwrap().variants.len()).sum()
}

fn cut_ids(d: &Document) -> Vec<CutId> {
    let mut v: Vec<CutId> = d.cuts().into_iter().map(|c| c.id).collect();
    v.sort();
    v
}

fn apply(d: &mut Document, op: &Op) -> Result<(), TestCaseError> {
    let before = d.text();
    let len = d.len();
    let ids = d.all_group_ids();
    let cuts_before = d.cuts();
    let shown_before = ghosted_chars(d);
    let clean_before = d.clean_text();
    let comments_before = d.comments();
    let versions_before = versions(d);
    match op {
        Op::Replace { a, b, text } => {
            let r = range(len, *a, *b);
            d.replace(r.clone(), text).unwrap();
            prop_assert_eq!(
                d.text(),
                splice(&before, r.clone(), text),
                "replace must equal a plain splice"
            );
            // A cut leaves only when its shown text was deleted; one whose spot was deleted stays.
            let now = cut_ids(d);
            for c in &cuts_before {
                if !now.contains(&c.id) {
                    // ... or it was in a version that went with its group.
                    let ok = matches!(&c.place, CutPlace::Page { range: cr, shown: true } if r.start <= cr.start && cr.end <= r.end)
                        || matches!(&c.place, CutPlace::Hidden { at } if r.start <= *at && *at <= r.end);
                    prop_assert!(ok, "cut {:?} was lost by a replace", c);
                }
            }
        }
        Op::CreateGroup { a, b, scope } => {
            let scope = [Scope::Word, Scope::Sentence, Scope::Paragraph][*scope as usize];
            let _ = d.create_group(range(len, *a, *b), scope);
            prop_assert_eq!(d.text(), before, "creating a group never changes the text");
        }
        Op::CutVersion { g } => {
            if let Some(id) = pick(&ids, *g) {
                let n = d.group(id).unwrap().variants.len();
                if d.add_cut_version(id, Source::Ai).is_ok() {
                    let info = d.group(id).unwrap();
                    prop_assert_eq!(info.variants.len(), n + 1);
                    prop_assert!(info.variants[n].cut);
                    let c = d.cuts().into_iter().find(|c| c.version == Some(id));
                    let hidden = matches!(c.map(|c| c.place), Some(CutPlace::Hidden { .. }));
                    prop_assert!(hidden, "a new version's cut isn't on the page");
                }
            }
            prop_assert_eq!(d.text(), before, "a new version doesn't show");
        }
        Op::AddVariant { g, text } => {
            if let Some(id) = pick(&ids, *g) {
                let _ = d.add_variant(id, text, Source::Me);
            }
            prop_assert_eq!(d.text(), before);
        }
        Op::Cycle { g, delta } => {
            if let Some(id) = pick(&ids, *g) {
                let vis = d.visible_groups().into_iter().find(|v| v.id == id);
                let variants = d
                    .group(id)
                    .unwrap()
                    .variants
                    .iter()
                    .map(|v| v.text.clone())
                    .collect::<Vec<_>>();
                let cuts: Vec<bool> = d.group(id).unwrap().variants.iter().map(|v| v.cut).collect();
                let c = d.cycle(id, *delta as isize).unwrap();
                let after = d.text();
                // A version without the words shows its text faded, or nothing as a mark.
                let shown = d.group_range(id).is_some_and(|r| !r.is_empty());
                match vis {
                    Some(v) if cuts[c.active] => {
                        let new_text = if shown { variants[c.active].as_str() } else { "" };
                        prop_assert_eq!(after, splice(&before, v.range.clone(), new_text));
                    }
                    None => prop_assert_eq!(
                        after,
                        before,
                        "cycling a hidden group changes nothing visible"
                    ),
                    Some(v) => {
                        let new_text = &variants[c.active];
                        let expected = splice(&before, v.range.clone(), new_text);
                        prop_assert_eq!(after, expected);
                    }
                }
                let still: Vec<_> = d
                    .group(id)
                    .unwrap()
                    .variants
                    .iter()
                    .map(|v| v.text.clone())
                    .collect();
                prop_assert_eq!(still, variants, "cycling never edits variants");
            }
        }
        Op::RemoveVariant { g, v } => {
            if let Some(id) = pick(&ids, *g) {
                let n = d.group(id).unwrap().variants.len();
                let _ = d.remove_variant(id, *v as usize % n);
            }
        }
        Op::Settle { g } => {
            if let Some(id) = pick(&ids, *g) {
                d.settle(id).unwrap();
                prop_assert_eq!(d.text(), before, "settling keeps the visible text");
                prop_assert!(d.group(id).is_none());
            }
        }
        Op::Ghost { a, b } => {
            let r = range(len, *a, *b);
            d.ghost(r.clone()).unwrap();
            prop_assert_eq!(d.text(), before);
            let g = ghosted_chars(d);
            prop_assert!(
                r.clone().all(|i| g[i]),
                "every char in a ghosted range is ghosted"
            );
        }
        Op::Revive { a, b } => {
            let r = range(len, *a, *b);
            // A version without the words goes back by showing the original, which may differ.
            let shown: Vec<_> = d.traces().into_iter().filter(|t| t.shown).map(|t| t.id).collect();
            let versions: Vec<_> = d
                .cuts_in(r.clone())
                .into_iter()
                .filter(|id| shown.contains(id))
                .filter_map(|id| d.cut_info(id).and_then(|c| c.version))
                .collect();
            d.revive(r.clone()).unwrap();
            if !versions.is_empty() {
                for g in versions {
                    prop_assert_eq!(d.group(g).map(|g| g.active), Some(0));
                }
                return Ok(());
            }
            prop_assert_eq!(d.text(), before);
            let g = ghosted_chars(d);
            prop_assert!(
                r.clone().all(|i| !g[i]),
                "nothing in a revived range stays ghosted"
            );
        }
        Op::CopyPaste { a, b, at } => {
            let r = range(len, *a, *b);
            let frag = d.copy(r.clone()).unwrap();
            let copied = kept_in(d, &before, r.clone());
            prop_assert_eq!(frag.plain_text(), copied.clone());
            let at = *at as usize % (len + 1);
            d.paste(at, &frag).unwrap();
            prop_assert_eq!(d.text(), splice(&before, at..at, &copied));
        }
        Op::Stash { a, b } => {
            let r = range(len, *a, *b);
            if !r.is_empty() {
                let moved: String = before.chars().skip(r.start).take(r.len()).collect();
                d.stash(r.clone()).unwrap();
                prop_assert_eq!(d.text(), splice(&before, r, ""));
                prop_assert_eq!(&d.notes()[0].text, &moved);
            }
        }
        Op::Unstash { n, at } => {
            let notes = d.notes();
            if !notes.is_empty() {
                let i = *n as usize % notes.len();
                let at = *at as usize % (len + 1);
                d.unstash(i, at).unwrap();
                prop_assert_eq!(d.text(), splice(&before, at..at, &notes[i].text));
            }
        }
        Op::Drop {
            a,
            b,
            at,
            block,
            copy,
        } => {
            // Text moves or is copied whole; only whitespace at the seams changes.
            let r = range(len, *a, *b);
            let at = *at as usize % (len + 1);
            let to = if *block {
                Place::Block(at)
            } else {
                Place::Inline(at)
            };
            let ink = |s: &str| s.chars().filter(|c| !c.is_whitespace()).count();
            let t: Vec<char> = before.chars().collect();
            let mut tr = r.clone();
            while tr.start < tr.end && t[tr.start].is_whitespace() {
                tr.start += 1;
            }
            while tr.end > tr.start && t[tr.end - 1].is_whitespace() {
                tr.end -= 1;
            }
            let carried: String = if *copy { kept_in(d, &before, tr.clone()) } else { t[tr.clone()].iter().collect() };
            match d.drop_text(Carried::Page(r), to, *copy) {
                Ok(now) => {
                    let text = d.text();
                    let landed: String = text.chars().skip(now.start).take(now.len()).collect();
                    prop_assert_eq!(&landed, &carried);
                    let moved = text != before;
                    let extra = if *copy && moved { ink(&carried) } else { 0 };
                    prop_assert_eq!(ink(&text), ink(&before) + extra);
                }
                Err(_) => prop_assert_eq!(d.text(), before),
            }
        }
        Op::Cut { a, b, look: l } => {
            let r = range(len, *a, *b);
            match d.cut(r.clone(), look(*l)) {
                Ok((id, got)) => {
                    prop_assert_eq!(d.text(), before, "a cut never reflows the page");
                    prop_assert!(got.start <= r.start && r.end <= got.end, "the cut covers the range");
                    prop_assert!(d.traces().iter().any(|t| t.id == id && t.range == got && t.shown));
                    let g = ghosted_chars(d);
                    prop_assert!(got.clone().all(|i| g[i]), "a new cut shows");
                    let now = cut_ids(d);
                    for c in &cuts_before {
                        if !now.contains(&c.id) {
                            let inside = match &c.place {
                                CutPlace::Page { range: cr, shown: true } => got.start <= cr.start && cr.end <= got.end,
                                _ => false,
                            };
                            prop_assert!(inside, "cut {:?} was lost by a cut", c);
                        }
                    }
                }
                Err(_) => prop_assert_eq!(d.text(), before),
            }
        }
        Op::PutBack { c } => {
            if let Some(id) = pick_cut(d, *c) {
                let info = d.cut_info(id).unwrap();
                match d.put_back(id) {
                    // A version without the words goes back by showing the original; it stays.
                    Ok(_) if info.version.is_some() => {
                        let g = d.group(info.version.unwrap()).unwrap();
                        prop_assert_eq!(g.active, 0);
                        prop_assert!(d.cut_info(id).is_some());
                    }
                    Ok(r) => {
                        prop_assert!(d.cut_info(id).is_none());
                        match &info.place {
                            CutPlace::Page { shown: true, .. } => prop_assert_eq!(d.text(), before, "putting back faded text doesn't move the page"),
                            CutPlace::Page { range: cr, shown: false } => {
                                let r = r.expect("on the page");
                                prop_assert_eq!(r.start, cr.start);
                                let back: String = d.text().chars().skip(r.start).take(r.len()).collect();
                                prop_assert_eq!(back.trim(), info.text.trim());
                                prop_assert_eq!(d.text(), splice(&before, cr.clone(), &back));
                            }
                            _ => {}
                        }
                    }
                    Err(_) => prop_assert!(matches!(info.place, CutPlace::Note(_))),
                }
            }
        }
        Op::Forget { c } => {
            if let Some(id) = pick_cut(d, *c) {
                let info = d.cut_info(id).unwrap();
                if d.forget_spot(id).is_ok() {
                    let now = d.cut_info(id).unwrap();
                    prop_assert_eq!(now.place, CutPlace::Note(0));
                    prop_assert_eq!(now.text, info.text);
                    if versions(d) == versions_before {
                        if let CutPlace::Page { range: cr, shown: true } = info.place {
                            prop_assert_eq!(d.text(), splice(&before, cr, ""));
                        } else {
                            prop_assert_eq!(d.text(), before);
                        }
                    }
                }
            }
        }
        Op::SetLook { c, look: l } => {
            if let Some(id) = pick_cut(d, *c) {
                let _ = d.set_look(id, look(*l));
                prop_assert_eq!(cut_ids(d), cuts_before.iter().map(|c| c.id).collect::<Vec<_>>().tap_sort());
                prop_assert_eq!(d.clean_text(), clean_before);
            }
        }
        Op::DeleteCut { c } => {
            if let Some(id) = pick_cut(d, *c) {
                let info = d.cut_info(id).unwrap();
                d.delete_cut(id).unwrap();
                prop_assert!(d.cut_info(id).is_none());
                match (info.version, info.place) {
                    // Showing, a version without the words settles its group (as settling does,
                    // the other versions go with their cuts).
                    (Some(g), CutPlace::Page { .. }) => prop_assert!(d.group(g).is_none()),
                    _ => prop_assert_eq!(cut_ids(d).len(), cuts_before.len() - 1),
                }
            }
        }
        Op::MoveCut { c, at, block, copy } => {
            if let Some(id) = pick_cut(d, *c) {
                let info = d.cut_info(id).unwrap();
                let at = *at as usize % (len + 1);
                match d.drop_text(Carried::Cut(id), place(at, *block), *copy) {
                    Ok(now) => {
                        let landed: String = d.text().chars().skip(now.start).take(now.len()).collect();
                        if !shown_before.get(at).copied().unwrap_or(false) || at == 0 {
                            prop_assert_eq!(landed.trim(), info.text.trim());
                        }
                        prop_assert_eq!(d.cut_info(id).is_some(), *copy || info.version.is_some(), "a move takes the cut (a version's is copied)");
                    }
                    Err(_) => prop_assert_eq!(d.text(), before),
                }
            }
        }
        Op::Present { editing, mark } => {
            d.set_presentation(presentation(*editing, *mark));
            prop_assert_eq!(cut_ids(d), cuts_before.iter().map(|c| c.id).collect::<Vec<_>>().tap_sort());
            prop_assert_eq!(d.clean_text(), clean_before, "how cuts show never changes the final text");
        }
        Op::Comment { a, b, text } => {
            let r = range(len, *a, *b);
            let n = d.comments().len();
            if let Ok(id) = d.add_comment(r.clone(), text, None) {
                prop_assert_eq!(d.text(), before, "a comment never changes the text");
                prop_assert_eq!(d.clean_text(), clean_before);
                prop_assert_eq!(d.comment_text(id), Some(text.clone()));
                // It covers the range or more, unless an end fell in a faded cut.
                let shown = ghosted_chars(d);
                if let Some(c) = d.comments().into_iter().find(|c| c.id == id) {
                    prop_assert_eq!(d.comments().len(), n + 1);
                    if !r.clone().any(|i| shown[i]) {
                        prop_assert!(c.range.start <= r.start && r.end <= c.range.end, "{:?} doesn't cover {:?}", c.range, r);
                    }
                }
            }
        }
        Op::EditComment { c, text } => {
            if let Some(id) = pick_comment(d, *c) {
                d.set_comment_text(id, text).unwrap();
                prop_assert_eq!(d.comment_text(id), Some(text.clone()));
                prop_assert_eq!(d.text(), before);
            }
        }
        Op::DeleteComment { c } => {
            if let Some(id) = pick_comment(d, *c) {
                let n = d.comments().len();
                d.delete_comment(id).unwrap();
                prop_assert_eq!(d.comments().len(), n - 1);
                prop_assert_eq!(d.comment_text(id), None);
                prop_assert_eq!(d.text(), before);
            }
        }
        Op::Undo => {
            d.undo();
        }
        Op::Redo => {
            d.redo();
        }
        Op::Break => d.break_undo_group(),
    }
    let clean = d.clean_text().chars().count();
    for (r, _) in d.clean_comments() {
        prop_assert!(r.start < r.end && r.end <= clean, "{:?} past {}", r, clean);
    }
    for c in d.comments() {
        if let Some(was) = comments_before.iter().find(|b| b.id == c.id) {
            if !matches!(op, Op::EditComment { .. } | Op::Undo | Op::Redo) {
                prop_assert_eq!(&c.text, &was.text);
            }
        }
    }
    if let Err(e) = d.check_invariants() {
        return Err(TestCaseError::fail(format!(
            "invariant broken after {op:?}: {e}"
        )));
    }
    Ok(())
}

trait TapSort {
    fn tap_sort(self) -> Self;
}

impl<T: Ord> TapSort for Vec<T> {
    fn tap_sort(mut self) -> Self {
        self.sort();
        self
    }
}

/// Text, ghost ranges, notes, and every group's (id, active, variant texts).
type Snapshot = (
    String,
    Vec<std::ops::Range<usize>>,
    Vec<String>,
    Vec<(GroupId, usize, Vec<String>)>,
);

fn snapshot(d: &Document) -> Snapshot {
    let groups = d
        .all_group_ids()
        .into_iter()
        .map(|id| {
            let g = d.group(id).unwrap();
            (
                id,
                g.active,
                g.variants.into_iter().map(|v| v.text).collect(),
            )
        })
        .collect();
    (
        d.text(),
        d.ghost_ranges(),
        d.notes().into_iter().map(|n| n.text).collect(),
        groups,
    )
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2000),
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_sessions_keep_every_guarantee(
        seed in "[a-z .\n]{0,40}",
        ops in prop::collection::vec(op_strategy(), 0..60),
    ) {
        let mut d = Document::from_text(&seed);
        let initial = snapshot(&d);
        for op in &ops {
            apply(&mut d, op)?;
        }
        // The tip of the history: if the session ended with undos, redo reaches past `ops`.
        while d.redo() {}
        let last = snapshot(&d);
        while d.undo() {
            d.check_invariants().map_err(TestCaseError::fail)?;
        }
        prop_assert_eq!(snapshot(&d), initial, "undo all → initial document");
        while d.redo() {}
        prop_assert_eq!(snapshot(&d), last, "redo all → final document");
    }

    #[test]
    fn full_cycle_returns_to_the_same_text(
        seed in "[a-z ]{1,30}",
        cuts in prop::collection::vec((any::<u16>(), any::<u16>()), 1..6),
        variants in prop::collection::vec("[a-z]{1,8}", 1..4),
    ) {
        let mut d = Document::from_text(&seed);
        for (a, b) in cuts {
            let r = range(d.len(), a, b);
            if let Ok(id) = d.create_group(r, Scope::Word) {
                for v in &variants {
                    d.add_variant(id, v, Source::Me).unwrap();
                }
            }
        }
        let start = d.text();
        for id in d.all_group_ids() {
            let n = d.group(id).unwrap().variants.len();
            for _ in 0..n {
                d.cycle(id, 1).unwrap();
            }
            prop_assert_eq!(d.text(), start.clone());
        }
    }
}
