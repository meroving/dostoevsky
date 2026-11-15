//! Random edit sessions, shared by doc-model's and doc-format's property tests via `#[path]`.
#![allow(dead_code)]

use doc_model::{CommentId, CutId, Document, GroupId, Look, Presentation, Scope, Source};
use proptest::prelude::*;

#[derive(Clone, Debug)]
pub enum Op {
    Replace { a: u16, b: u16, text: String },
    CreateGroup { a: u16, b: u16, scope: u8 },
    AddVariant { g: u16, text: String },
    /// A version without the words.
    CutVersion { g: u16 },
    Cycle { g: u16, delta: i8 },
    RemoveVariant { g: u16, v: u8 },
    Settle { g: u16 },
    Ghost { a: u16, b: u16 },
    Revive { a: u16, b: u16 },
    CopyPaste { a: u16, b: u16, at: u16 },
    Stash { a: u16, b: u16 },
    Unstash { n: u8, at: u16 },
    Drop { a: u16, b: u16, at: u16, block: bool, copy: bool },
    Cut { a: u16, b: u16, look: u8 },
    PutBack { c: u16 },
    Forget { c: u16 },
    SetLook { c: u16, look: u8 },
    DeleteCut { c: u16 },
    MoveCut { c: u16, at: u16, block: bool, copy: bool },
    Present { editing: bool, mark: bool },
    Comment { a: u16, b: u16, text: String },
    EditComment { c: u16, text: String },
    DeleteComment { c: u16 },
    Undo,
    Redo,
    Break,
}

pub fn text_strategy() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop::sample::select(vec!['a', 'b', 'é', ' ', '.', '\n', 'x', '-', '’']),
        0..6,
    )
    .prop_map(|v| v.into_iter().collect())
    .boxed()
}

pub fn op_strategy() -> BoxedStrategy<Op> {
    op_strategy_with(text_strategy())
}

pub fn op_strategy_with(text: BoxedStrategy<String>) -> BoxedStrategy<Op> {
    let p = any::<u16>;
    prop_oneof![
        4 => (p(), p(), text.clone()).prop_map(|(a, b, text)| Op::Replace { a, b, text }),
        3 => (p(), p(), 0u8..3).prop_map(|(a, b, scope)| Op::CreateGroup { a, b, scope }),
        3 => (p(), text.clone()).prop_map(|(g, text)| Op::AddVariant { g, text }),
        3 => (p(), -2i8..3).prop_map(|(g, delta)| Op::Cycle { g, delta }),
        1 => p().prop_map(|g| Op::CutVersion { g }),
        1 => (p(), any::<u8>()).prop_map(|(g, v)| Op::RemoveVariant { g, v }),
        1 => p().prop_map(|g| Op::Settle { g }),
        2 => (p(), p()).prop_map(|(a, b)| Op::Ghost { a, b }),
        1 => (p(), p()).prop_map(|(a, b)| Op::Revive { a, b }),
        1 => (p(), p(), p()).prop_map(|(a, b, at)| Op::CopyPaste { a, b, at }),
        1 => (p(), p()).prop_map(|(a, b)| Op::Stash { a, b }),
        1 => (any::<u8>(), p()).prop_map(|(n, at)| Op::Unstash { n, at }),
        1 => (p(), p(), p(), any::<bool>(), any::<bool>()).prop_map(|(a, b, at, block, copy)| Op::Drop { a, b, at, block, copy }),
        2 => (p(), p(), 0u8..3).prop_map(|(a, b, look)| Op::Cut { a, b, look }),
        1 => p().prop_map(|c| Op::PutBack { c }),
        1 => p().prop_map(|c| Op::Forget { c }),
        1 => (p(), 0u8..3).prop_map(|(c, look)| Op::SetLook { c, look }),
        1 => p().prop_map(|c| Op::DeleteCut { c }),
        1 => (p(), p(), any::<bool>(), any::<bool>()).prop_map(|(c, at, block, copy)| Op::MoveCut { c, at, block, copy }),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(editing, mark)| Op::Present { editing, mark }),
        2 => (p(), p(), text.clone()).prop_map(|(a, b, text)| Op::Comment { a, b, text }),
        1 => (p(), text).prop_map(|(c, text)| Op::EditComment { c, text }),
        1 => p().prop_map(|c| Op::DeleteComment { c }),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
        1 => Just(Op::Break),
    ]
    .boxed()
}

pub fn range(len: usize, a: u16, b: u16) -> std::ops::Range<usize> {
    let x = a as usize % (len + 1);
    let y = b as usize % (len + 1);
    x.min(y)..x.max(y)
}

pub fn pick(ids: &[GroupId], g: u16) -> Option<GroupId> {
    (!ids.is_empty()).then(|| ids[g as usize % ids.len()])
}

pub fn pick_cut(d: &Document, c: u16) -> Option<CutId> {
    let ids: Vec<CutId> = d.cuts().into_iter().map(|c| c.id).collect();
    (!ids.is_empty()).then(|| ids[c as usize % ids.len()])
}

pub fn pick_comment(d: &Document, c: u16) -> Option<CommentId> {
    let ids: Vec<CommentId> = d.comments().into_iter().map(|c| c.id).collect();
    (!ids.is_empty()).then(|| ids[c as usize % ids.len()])
}

pub fn look(i: u8) -> Option<Look> {
    [None, Some(Look::Text), Some(Look::Mark)][i as usize % 3]
}

pub fn presentation(editing: bool, mark: bool) -> Presentation {
    Presentation { editing, look: if mark { Look::Mark } else { Look::Text } }
}

pub fn place(at: usize, block: bool) -> doc_model::Place {
    if block {
        doc_model::Place::Block(at)
    } else {
        doc_model::Place::Inline(at)
    }
}

pub fn scope(i: u8) -> Scope {
    [Scope::Word, Scope::Sentence, Scope::Paragraph][i as usize % 3]
}

/// Applies `op`, ignoring rejected operations.
pub fn apply_loose(d: &mut Document, op: &Op) {
    let len = d.len();
    let ids = d.all_group_ids();
    match op {
        Op::Replace { a, b, text } => {
            let _ = d.replace(range(len, *a, *b), text);
        }
        Op::CreateGroup { a, b, scope: s } => {
            let _ = d.create_group(range(len, *a, *b), scope(*s));
        }
        Op::AddVariant { g, text } => {
            if let Some(id) = pick(&ids, *g) {
                let by = [Source::Me, Source::Ai, Source::Import][text.len() % 3];
                let _ = d.add_variant(id, text, by);
            }
        }
        Op::Cycle { g, delta } => {
            if let Some(id) = pick(&ids, *g) {
                let _ = d.cycle(id, *delta as isize);
            }
        }
        Op::CutVersion { g } => {
            if let Some(id) = pick(&ids, *g) {
                let _ = d.add_cut_version(id, Source::Ai);
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
                let _ = d.settle(id);
            }
        }
        Op::Ghost { a, b } => {
            let _ = d.ghost(range(len, *a, *b));
        }
        Op::Revive { a, b } => {
            let _ = d.revive(range(len, *a, *b));
        }
        Op::CopyPaste { a, b, at } => {
            if let Ok(f) = d.copy(range(len, *a, *b)) {
                let _ = d.paste(*at as usize % (len + 1), &f);
            }
        }
        Op::Stash { a, b } => {
            let _ = d.stash(range(len, *a, *b));
        }
        Op::Unstash { n, at } => {
            let k = d.notes().len();
            if k > 0 {
                let _ = d.unstash(*n as usize % k, *at as usize % (len + 1));
            }
        }
        Op::Drop {
            a,
            b,
            at,
            block,
            copy,
        } => {
            let at = *at as usize % (len + 1);
            let to = if *block {
                doc_model::Place::Block(at)
            } else {
                doc_model::Place::Inline(at)
            };
            let _ = d.drop_text(doc_model::Carried::Page(range(len, *a, *b)), to, *copy);
        }
        Op::Cut { a, b, look: l } => {
            let _ = d.cut(range(len, *a, *b), look(*l));
        }
        Op::PutBack { c } => {
            if let Some(id) = pick_cut(d, *c) {
                let _ = d.put_back(id);
            }
        }
        Op::Forget { c } => {
            if let Some(id) = pick_cut(d, *c) {
                let _ = d.forget_spot(id);
            }
        }
        Op::SetLook { c, look: l } => {
            if let Some(id) = pick_cut(d, *c) {
                let _ = d.set_look(id, look(*l));
            }
        }
        Op::DeleteCut { c } => {
            if let Some(id) = pick_cut(d, *c) {
                let _ = d.delete_cut(id);
            }
        }
        Op::MoveCut { c, at, block, copy } => {
            if let Some(id) = pick_cut(d, *c) {
                let _ = d.drop_text(doc_model::Carried::Cut(id), place(*at as usize % (len + 1), *block), *copy);
            }
        }
        Op::Present { editing, mark } => d.set_presentation(presentation(*editing, *mark)),
        Op::Comment { a, b, text } => {
            let _ = d.add_comment(range(len, *a, *b), text, Some("2026-11-12T10:00:00Z".into()));
        }
        Op::EditComment { c, text } => {
            if let Some(id) = pick_comment(d, *c) {
                let _ = d.set_comment_text(id, text);
            }
        }
        Op::DeleteComment { c } => {
            if let Some(id) = pick_comment(d, *c) {
                let _ = d.delete_comment(id);
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
}

#[derive(Clone, Debug)]
pub struct Session {
    pub seed: String,
    pub ops: Vec<Op>,
}

impl Session {
    pub fn build(&self) -> Document {
        let mut d = Document::from_text(&self.seed);
        for op in &self.ops {
            apply_loose(&mut d, op);
        }
        d
    }
}

pub fn session_strategy() -> BoxedStrategy<Session> {
    session_strategy_with("[a-z .\n]{0,40}", text_strategy())
}

pub fn session_strategy_with(
    seed: &'static str,
    text: BoxedStrategy<String>,
) -> BoxedStrategy<Session> {
    (
        seed,
        prop::collection::vec(op_strategy_with(text), 0..60),
    )
        .prop_map(|(seed, ops)| Session { seed, ops })
        .boxed()
}
