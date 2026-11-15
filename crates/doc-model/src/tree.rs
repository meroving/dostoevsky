//! Offsets are display chars relative to the container: a group counts as its active
//! variant's length, a shown cut as its text's, a cut that isn't shown as nothing.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use english::is_word_char;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VariantId(pub u64);

/// Per session; not saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CutId(pub u64);

/// Per session; not saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommentId(pub u64);

/// The start marker carries it; the end marker names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub(crate) id: CommentId,
    pub(crate) text: String,
    /// RFC 3339, as the app gave it.
    pub(crate) created: Option<String>,
    /// For a comment from another app; `None` is the writer.
    pub(crate) author: Option<String>,
}

/// A label and drawing hint only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Word,
    Sentence,
    Paragraph,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Source {
    #[default]
    Me,
    Ai,
    Import,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Look {
    #[default]
    Text,
    Mark,
}

/// Cuts made in one step, shown as one entry in the Cuts panel.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Batch {
    pub id: String,
    pub name: String,
}

/// An immutable piece of text. Never empty; may contain `\n` only as its last character (so
/// one keystroke copies at most one line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub(crate) text: String,
    pub(crate) len: usize,
}

impl Run {
    pub(crate) fn new(text: String) -> Self {
        let len = text.chars().count();
        Run { text, len }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Text(Arc<Run>),
    Group(Arc<Group>),
    Cut(Arc<Cut>),
    /// Zero-width, carries the comment.
    CommentStart(Arc<Comment>),
    /// Zero-width.
    CommentEnd(CommentId),
}

impl Node {
    pub(crate) fn len(&self) -> usize {
        match self {
            Node::Text(r) => r.len,
            Node::Group(g) => g.display_len(),
            Node::Cut(c) => c.display_len(),
            Node::CommentStart(_) | Node::CommentEnd(_) => 0,
        }
    }

    /// Cuts and markers take none.
    pub(crate) fn kept_len(&self) -> usize {
        match self {
            Node::Text(r) => r.len,
            Node::Group(g) => g.variants[g.active].content.nodes.iter().map(Node::kept_len).sum(),
            _ => 0,
        }
    }

    pub(crate) fn has_marks(&self) -> bool {
        match self {
            Node::CommentStart(_) | Node::CommentEnd(_) => true,
            Node::Group(g) => g.variants.iter().any(|v| v.content.marks),
            Node::Cut(c) => c.content.marks,
            Node::Text(_) => false,
        }
    }

    /// `(id, is_start)`.
    pub(crate) fn marker(&self) -> Option<(CommentId, bool)> {
        match self {
            Node::CommentStart(c) => Some((c.id, true)),
            Node::CommentEnd(id) => Some((*id, false)),
            _ => None,
        }
    }

    fn text(t: &str) -> Node {
        Node::Text(Arc::new(Run::new(t.to_string())))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub(crate) id: GroupId,
    pub(crate) scope: Scope,
    pub(crate) active: usize,
    pub(crate) variants: Vec<Variant>,
}

impl Group {
    pub(crate) fn display_len(&self) -> usize {
        self.variants[self.active].content.len
    }

    /// The original is never a version without the words: one that became the original (the
    /// versions before it went) is an ordinary version holding a cut.
    pub(crate) fn keep_original(&mut self) {
        if let Some(v) = self.variants.first_mut() {
            v.without = false;
        }
    }

    pub(crate) fn active_content_mut(&mut self) -> &mut Container {
        let i = self.active;
        &mut self.variants[i].content
    }

    fn contains_group(&self, id: GroupId) -> bool {
        self.id == id || self.variants.iter().any(|v| v.content.contains_group(id))
    }

    fn contains_cut(&self, id: CutId) -> bool {
        self.variants.iter().any(|v| v.content.contains_cut(id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub(crate) id: VariantId,
    pub(crate) by: Source,
    pub(crate) content: Container,
    /// A version without the words: its content is them, cut ([`crate::Document::add_cut_version`]).
    /// Only while it is exactly one cut ([`Variant::words_cut`]); otherwise an ordinary version.
    pub(crate) without: bool,
}

impl Variant {
    /// The cut, for a version without the words.
    pub(crate) fn words_cut(&self) -> Option<&Cut> {
        if self.without {
            self.content.only_cut()
        } else {
            None
        }
    }
}

/// Its text is in `content` whatever the trace looks like; `shown` only decides whether the
/// page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    pub(crate) id: CutId,
    /// Never empty, never holds cuts. For a cut of whole paragraphs, includes the blank line
    /// it took with it.
    pub(crate) content: Container,
    /// Drawn as faded text (its length counts on the page); otherwise a zero-width point.
    pub(crate) shown: bool,
    /// The cut's own look; `None` follows the app's setting.
    pub(crate) look: Option<Look>,
    /// The document clock, for freshness; 0 when loaded.
    pub(crate) made: u64,
    pub(crate) paragraph: bool,
    pub(crate) batch: Option<Arc<Batch>>,
}

impl Cut {
    pub(crate) fn display_len(&self) -> usize {
        if self.shown {
            self.content.len
        } else {
            0
        }
    }
}

pub(crate) enum Piece<'a> {
    /// Whether it is a cut's.
    Text(&'a str, bool),
    /// A cut that takes no room (a mark, or hidden).
    Point,
    Open(Vec<String>),
    Close,
    /// Not inside cuts.
    CommentStart(&'a Comment),
    CommentEnd(CommentId),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Container {
    pub(crate) nodes: Vec<Node>,
    pub(crate) len: usize,
    /// A comment's marker is here or below (cached with `len`), so a page with no comments
    /// is never walked for them.
    pub(crate) marks: bool,
}

enum Loc {
    /// Between `nodes[i-1]` and `nodes[i]` (before any zero-width node at `i`).
    Boundary(usize),
    /// Strictly inside run `i` at char offset `off` (0 < off < len).
    InRun(usize, usize),
    /// Strictly inside group or shown cut `i` at display offset `off` (0 < off < len).
    InNode(usize, usize),
}

pub(crate) enum Payload<'a> {
    Text(&'a str),
    Nodes(Vec<Node>),
}

impl Payload<'_> {
    fn into_nodes(self) -> Vec<Node> {
        match self {
            Payload::Text("") => Vec::new(),
            Payload::Text(t) => vec![Node::text(t)],
            Payload::Nodes(ns) => ns,
        }
    }

    /// Only word characters typed at a word group's edge join it, and not when it shows a
    /// version without the words (there is nothing to join).
    fn joins(&self, g: &Group) -> bool {
        if g.variants[g.active].without {
            return false;
        }
        match self {
            Payload::Text(t) => {
                g.scope == Scope::Word && !t.is_empty() && t.chars().all(is_word_char)
            }
            Payload::Nodes(_) => false,
        }
    }
}

fn byte_idx(s: &str, ch: usize) -> usize {
    s.char_indices().nth(ch).map_or(s.len(), |(i, _)| i)
}

fn push_text(out: &mut Vec<Node>, run: Arc<Run>) {
    if let Some(Node::Text(last)) = out.last_mut() {
        if !last.text.ends_with('\n') {
            let mut t = String::with_capacity(last.text.len() + run.text.len());
            t.push_str(&last.text);
            t.push_str(&run.text);
            *last = Arc::new(Run::new(t));
            return;
        }
    }
    out.push(Node::Text(run));
}

/// Every cut, at any depth, replaced by its text: for text going into a cut.
pub(crate) fn flatten_all(nodes: Vec<Node>) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            Node::Cut(c) => out.extend(c.content.nodes.iter().cloned()),
            Node::Group(mut g) if g.contains_any_cut() => {
                let gm = Arc::make_mut(&mut g);
                for v in &mut gm.variants {
                    let ns = std::mem::take(&mut v.content.nodes);
                    v.content = Container::from_nodes(flatten_all(ns));
                }
                out.push(Node::Group(g));
            }
            n => out.push(n),
        }
    }
    out
}

/// For `nodes` becoming a cut's content: shown cuts on the visible path join the text; points
/// (and groups that show nothing) go to `out_cuts`, next to the new cut; shown cuts in a hidden
/// version go to `lost` (their place is gone).
pub(crate) fn absorb_cuts(nodes: Vec<Node>, visible: bool, out_cuts: &mut Vec<Node>, lost: &mut Vec<Arc<Cut>>) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            Node::Cut(c) if visible && c.shown => out.extend(c.content.nodes.iter().cloned()),
            Node::Cut(c) if c.shown => lost.push(c),
            Node::Cut(c) => out_cuts.push(Node::Cut(c)),
            // A group that shows nothing (its version is only marks) goes next to the cut too.
            Node::Group(g) if visible && g.display_len() == 0 => out_cuts.push(Node::Group(g)),
            Node::Group(mut g) if g.contains_any_cut() => {
                let gm = Arc::make_mut(&mut g);
                let active = gm.variants[gm.active].id;
                let shows = gm.active;
                for (k, v) in gm.variants.iter_mut().enumerate() {
                    let ns = std::mem::take(&mut v.content.nodes);
                    // A version that is only cuts empties and goes; its cuts are kept.
                    v.content = Container::from_nodes(absorb_cuts(ns, visible && k == shows, out_cuts, lost));
                }
                gm.variants.retain(|v| !v.content.nodes.is_empty());
                gm.keep_original();
                if gm.variants.is_empty() {
                    continue;
                }
                gm.active = gm.variants.iter().position(|v| v.id == active).unwrap_or(0);
                out.push(Node::Group(g));
            }
            n => out.push(n),
        }
    }
    out
}

/// The cuts that take no room on the visible path of a deleted group: their spot went with
/// visible text, so they are kept as notes.
fn visible_points(n: &Node, out: &mut Vec<Arc<Cut>>) {
    match n {
        Node::Cut(c) if !c.shown => out.push(c.clone()),
        Node::Group(g) => {
            for m in &g.variants[g.active].content.nodes {
                visible_points(m, out);
            }
        }
        _ => {}
    }
}

/// What a copy carries.
pub(crate) fn drop_cuts(nodes: Vec<Node>) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            Node::Cut(_) => {}
            Node::Group(mut g) if g.contains_any_cut() => {
                let gm = Arc::make_mut(&mut g);
                let active = gm.variants[gm.active].id;
                for v in &mut gm.variants {
                    let ns = std::mem::take(&mut v.content.nodes);
                    v.content = Container::from_nodes(drop_cuts(ns));
                }
                // A version that was all cut goes; if it was the one showing, nothing of the
                // group was kept text.
                if gm.variants[gm.active].content.nodes.is_empty() {
                    continue;
                }
                gm.variants.retain(|v| !v.content.nodes.is_empty());
                gm.keep_original();
                gm.active = gm.variants.iter().position(|v| v.id == active).unwrap_or(0);
                out.push(Node::Group(g));
            }
            n => out.push(n),
        }
    }
    out
}

/// What a copy carries.
pub(crate) fn drop_comments(nodes: Vec<Node>) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            Node::CommentStart(_) | Node::CommentEnd(_) => {}
            Node::Group(mut g) if g.variants.iter().any(|v| v.content.has_comments()) => {
                let gm = Arc::make_mut(&mut g);
                let active = gm.variants[gm.active].id;
                for v in &mut gm.variants {
                    let ns = std::mem::take(&mut v.content.nodes);
                    v.content = Container::from_nodes(drop_comments(ns));
                }
                if gm.variants[gm.active].content.nodes.is_empty() {
                    continue;
                }
                gm.variants.retain(|v| !v.content.nodes.is_empty());
                gm.keep_original();
                gm.active = gm.variants.iter().position(|v| v.id == active).unwrap_or(0);
                out.push(Node::Group(g));
            }
            Node::Cut(mut c) if c.content.has_comments() => {
                let cm = Arc::make_mut(&mut c);
                let ns = std::mem::take(&mut cm.content.nodes);
                cm.content = Container::from_nodes(drop_comments(ns));
                out.push(Node::Cut(c));
            }
            n => out.push(n),
        }
    }
    out
}

/// A range `inside` taken from between `before` and `after` takes along the comments it holds
/// whole: a comment starting right at its start, or ending right at its end, whose other marker
/// is inside it or right at its other edge.
pub(crate) fn grab_edges(before: &mut Vec<Node>, inside: &mut Vec<Node>, after: &mut Vec<Node>) {
    let ids = |nodes: &[Node], start: bool| -> HashSet<CommentId> {
        nodes.iter().filter_map(Node::marker).filter(|m| m.1 == start).map(|m| m.0).collect()
    };
    let head_len = after.iter().take_while(|n| n.len() == 0 && !matches!(n, Node::Group(_))).count();
    let mut ends = ids(inside, false);
    ends.extend(ids(&after[..head_len], false));
    let tail_from = before.len() - before.iter().rev().take_while(|n| n.len() == 0 && !matches!(n, Node::Group(_))).count();
    let tail: Vec<Node> = before.drain(tail_from..).collect();
    let mut starts = ids(inside, true);
    let mut moved = Vec::new();
    for n in tail {
        match &n {
            Node::CommentStart(c) if ends.contains(&c.id) => {
                starts.insert(c.id);
                moved.push(n);
            }
            _ => before.push(n),
        }
    }
    inside.splice(0..0, moved);
    let head: Vec<Node> = after.drain(..head_len).collect();
    let mut stay = Vec::new();
    for n in head {
        match &n {
            Node::CommentEnd(id) if starts.contains(id) => inside.push(n),
            _ => stay.push(n),
        }
    }
    after.splice(0..0, stay);
}

/// The order of markers next to each other, by kept-text offset: ends first, the comment that
/// starts last first; then starts, the one that ends last first; equal spans by what they say.
/// So comments at one place nest, and equal documents have equal trees.
type RunKey = (
    u8,
    std::cmp::Reverse<usize>,
    Option<std::cmp::Reverse<(String, Option<String>, Option<String>)>>,
    Option<(String, Option<String>, Option<String>)>,
    std::cmp::Reverse<usize>,
);

fn run_keys(nodes: &[Node]) -> Vec<Option<RunKey>> {
    let mut start_at: HashMap<CommentId, usize> = HashMap::new();
    let mut end_at: HashMap<CommentId, usize> = HashMap::new();
    let mut what: HashMap<CommentId, (String, Option<String>, Option<String>)> = HashMap::new();
    let mut start_index: HashMap<CommentId, usize> = HashMap::new();
    let mut o = 0;
    for (i, n) in nodes.iter().enumerate() {
        match n {
            Node::CommentStart(c) => {
                start_at.insert(c.id, o);
                start_index.insert(c.id, i);
                what.insert(c.id, (c.text.clone(), c.created.clone(), c.author.clone()));
            }
            Node::CommentEnd(id) => {
                end_at.insert(*id, o);
            }
            _ => {}
        }
        // Kept text, not display: how cuts show (not an edit) mustn't reorder anything.
        o += n.kept_len();
    }
    nodes
        .iter()
        .map(|n| match n.marker() {
            Some((id, false)) => Some((
                0,
                std::cmp::Reverse(start_at.get(&id).copied().unwrap_or(0)),
                what.get(&id).cloned().map(std::cmp::Reverse),
                None,
                // Equal comments close in the reverse order they open.
                std::cmp::Reverse(start_index.get(&id).copied().unwrap_or(0)),
            )),
            Some((id, true)) => Some((1, std::cmp::Reverse(end_at.get(&id).copied().unwrap_or(usize::MAX)), None, what.get(&id).cloned(), std::cmp::Reverse(0))),
            None => None,
        })
        .collect()
}

fn runs_sorted(nodes: &[Node]) -> bool {
    let keys = run_keys(nodes);
    keys.windows(2).all(|w| match (&w[0], &w[1]) {
        (Some(a), Some(b)) => a <= b,
        _ => true,
    })
}

fn sort_runs(nodes: &mut [Node]) {
    let keys = run_keys(nodes);
    let mut i = 0;
    while i < nodes.len() {
        if keys[i].is_none() {
            i += 1;
            continue;
        }
        let j = (i..nodes.len()).find(|&k| keys[k].is_none()).unwrap_or(nodes.len());
        let mut run: Vec<(Option<RunKey>, Node)> = keys[i..j].iter().cloned().zip(nodes[i..j].iter().cloned()).collect();
        run.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, (_, n)) in run.into_iter().enumerate() {
            nodes[i + k] = n;
        }
        i = j;
    }
}

/// A cut at a comment's edge is outside it.
fn edges_settled(nodes: &[Node]) -> bool {
    let next = |i: usize| nodes[i + 1..].iter().find(|n| n.marker().is_none());
    let prev = |i: usize| nodes[..i].iter().rev().find(|n| n.marker().is_none());
    nodes.iter().enumerate().all(|(i, n)| match n {
        Node::CommentStart(_) => !matches!(next(i), Some(Node::Cut(_))),
        Node::CommentEnd(_) => !matches!(prev(i), Some(Node::Cut(_))),
        _ => true,
    })
}

fn shed_edge_cuts(nodes: &mut Vec<Node>) {
    while !edges_settled(nodes) {
        let n = nodes.len();
        for i in 0..n {
            let j = (i + 1..n).find(|&k| nodes[k].marker().is_none());
            if let (Node::CommentStart(_), Some(j)) = (&nodes[i], j) {
                if matches!(nodes[j], Node::Cut(_)) {
                    let m = nodes.remove(i);
                    nodes.insert(j, m);
                    break;
                }
            }
            let k = (0..i).rev().find(|&k| nodes[k].marker().is_none());
            if let (Node::CommentEnd(_), Some(k)) = (&nodes[i], k) {
                if matches!(nodes[k], Node::Cut(_)) {
                    let m = nodes.remove(i);
                    nodes.insert(k, m);
                    break;
                }
            }
        }
    }
}

/// One level deep, in order.
pub(crate) fn split_unmatched(nodes: &mut Vec<Node>) -> Vec<Node> {
    let starts: HashSet<CommentId> = nodes.iter().filter_map(Node::marker).filter(|m| m.1).map(|m| m.0).collect();
    let ends: HashSet<CommentId> = nodes.iter().filter_map(Node::marker).filter(|m| !m.1).map(|m| m.0).collect();
    let mut out = Vec::new();
    let mut keep = Vec::with_capacity(nodes.len());
    for n in nodes.drain(..) {
        match n.marker() {
            Some((id, true)) if !ends.contains(&id) => out.push(n),
            Some((id, false)) if !starts.contains(&id) => out.push(n),
            _ => keep.push(n),
        }
    }
    *nodes = keep;
    out
}

impl Group {
    fn contains_any_cut(&self) -> bool {
        self.variants.iter().any(|v| v.content.has_cuts())
    }
}

impl Container {
    pub(crate) fn from_text(text: &str) -> Self {
        Self::from_nodes(vec![Node::text(text)])
    }

    pub(crate) fn from_nodes(nodes: Vec<Node>) -> Self {
        let mut c = Container { nodes, len: 0, marks: false };
        c.normalize();
        c
    }

    pub(crate) fn recompute(&mut self) {
        self.len = self.nodes.iter().map(Node::len).sum();
        self.marks = self.nodes.iter().any(Node::has_marks);
    }

    /// Drops empty runs and cuts, splits runs after each `\n`, merges neighbours; unchanged runs
    /// keep their `Arc`.
    pub(crate) fn normalize(&mut self) {
        let mut out: Vec<Node> = Vec::with_capacity(self.nodes.len());
        for node in self.nodes.drain(..) {
            match node {
                Node::Text(r) => {
                    if r.text.is_empty() {
                        continue;
                    }
                    let inner_newline = r.text.find('\n').is_some_and(|i| i + 1 < r.text.len());
                    if inner_newline {
                        for piece in r.text.split_inclusive('\n') {
                            push_text(&mut out, Arc::new(Run::new(piece.to_string())));
                        }
                    } else {
                        push_text(&mut out, r);
                    }
                }
                Node::Cut(c) if c.content.len == 0 => {}
                g => out.push(g),
            }
        }
        self.nodes = out;
        self.recompute();
    }

    fn locate(&self, pos: usize) -> Loc {
        let mut o = 0;
        for (i, n) in self.nodes.iter().enumerate() {
            if pos == o {
                return Loc::Boundary(i);
            }
            let l = n.len();
            if pos < o + l {
                return match n {
                    Node::Text(_) => Loc::InRun(i, pos - o),
                    _ => Loc::InNode(i, pos - o),
                };
            }
            o += l;
        }
        Loc::Boundary(self.nodes.len())
    }

    /// Edges inclusive: `(node index, node offset, exact match)`.
    pub(crate) fn enclosing(&self, s: usize, e: usize) -> Option<(usize, usize, bool)> {
        let mut o = 0;
        for (i, n) in self.nodes.iter().enumerate() {
            let l = n.len();
            if o >= e {
                break;
            }
            let holds = matches!(n, Node::Group(_)) || matches!(n, Node::Cut(c) if c.shown);
            if holds && o <= s && e <= o + l {
                return Some((i, o, s == o && e == o + l));
            }
            o += l;
        }
        None
    }

    pub(crate) fn with_group_at<R>(&mut self, i: usize, f: impl FnOnce(&mut Group) -> R) -> R {
        let Node::Group(g) = &mut self.nodes[i] else {
            unreachable!("node {i} is not a group")
        };
        let r = f(Arc::make_mut(g));
        self.recompute();
        r
    }

    pub(crate) fn with_cut_at<R>(&mut self, i: usize, f: impl FnOnce(&mut Cut) -> R) -> R {
        let Node::Cut(c) = &mut self.nodes[i] else {
            unreachable!("node {i} is not a cut")
        };
        let r = f(Arc::make_mut(c));
        self.recompute();
        r
    }

    /// The container inside node `i` (a group's active variant or a cut's text) that holds
    /// display offsets strictly inside it.
    fn with_inside<R>(&mut self, i: usize, f: impl FnOnce(&mut Container, bool) -> R) -> R {
        match &self.nodes[i] {
            Node::Group(_) => self.with_group_at(i, |g| f(g.active_content_mut(), false)),
            Node::Cut(_) => self.with_cut_at(i, |c| f(&mut c.content, true)),
            _ => unreachable!("node {i} has no inside"),
        }
    }

    /// Inside a shown cut, cuts in the payload can't stay cuts: faded ones join its text, marks
    /// go just before it, and those that were in a hidden version are returned, their place gone.
    pub(crate) fn insert(&mut self, pos: usize, p: Payload<'_>, allow_join: bool) -> Vec<Arc<Cut>> {
        match self.locate(pos) {
            Loc::InRun(i, off) => {
                let Node::Text(run) = &self.nodes[i] else {
                    unreachable!()
                };
                let run = run.clone();
                let b = byte_idx(&run.text, off);
                let mut mid = vec![Node::text(&run.text[..b])];
                mid.extend(p.into_nodes());
                mid.push(Node::text(&run.text[b..]));
                self.nodes.splice(i..=i, mid);
                self.normalize();
                Vec::new()
            }
            Loc::InNode(i, off) => match p {
                Payload::Nodes(ns) if matches!(self.nodes[i], Node::Cut(_)) => {
                    let (mut beside, mut lost) = (Vec::new(), Vec::new());
                    let flat = absorb_cuts(ns, true, &mut beside, &mut lost);
                    self.with_cut_at(i, |c| c.content.insert(off, Payload::Nodes(flat), allow_join));
                    if !beside.is_empty() {
                        self.nodes.splice(i..i, beside);
                        self.normalize();
                    }
                    lost
                }
                p => self.with_inside(i, |c, _| c.insert(off, p, allow_join)),
            },
            Loc::Boundary(mut i) => {
                // Text at a comment's end goes after it, as at a group's edge.
                while matches!(self.nodes.get(i), Some(Node::CommentEnd(_))) {
                    i += 1;
                }
                // A word group's edge joins across the comment markers next to it.
                let left = (0..i).rev().find(|&k| self.nodes[k].marker().is_none());
                let join_left = allow_join
                    && matches!(left.map(|k| &self.nodes[k]), Some(Node::Group(g)) if p.joins(g));
                if let (true, Some(k)) = (join_left, left) {
                    return self.with_group_at(k, |g| {
                        let end = g.display_len();
                        g.active_content_mut().insert(end, p, true)
                    });
                }
                let right = (i..self.nodes.len()).find(|&k| self.nodes[k].marker().is_none());
                let join_right = allow_join
                    && matches!(right.map(|k| &self.nodes[k]), Some(Node::Group(g)) if p.joins(g));
                if let (true, Some(k)) = (join_right, right) {
                    return self.with_group_at(k, |g| g.active_content_mut().insert(0, p, true));
                }
                let mid = p.into_nodes();
                self.nodes.splice(i..i, mid);
                self.normalize();
                Vec::new()
            }
        }
    }

    /// Returns the cuts whose spot was deleted (points strictly inside), to keep as notes.
    pub(crate) fn replace(&mut self, s: usize, e: usize, text: &str) -> Vec<Arc<Cut>> {
        if s == e {
            self.insert(s, Payload::Text(text), true);
            return Vec::new();
        }
        if let Some((i, o, exact)) = self.enclosing(s, e) {
            match &self.nodes[i] {
                Node::Group(_) if exact && text.is_empty() => {
                    // Deleting a whole span deletes the group; a mark in what showed
                    // keeps its cut.
                    let n = self.nodes.remove(i);
                    self.normalize();
                    let mut evicted = Vec::new();
                    visible_points(&n, &mut evicted);
                    return evicted;
                }
                Node::Cut(_) if exact => {
                    // A whole cut selected and deleted or typed over: it goes (⌫ deletes).
                    self.nodes.remove(i);
                    self.normalize();
                    if !text.is_empty() {
                        self.insert(s, Payload::Text(text), false);
                    }
                    return Vec::new();
                }
                // Strictly inside, or a whole group with new text: edit inside.
                _ => return self.with_inside(i, |c, _| c.replace(s - o, e - o, text)),
            }
        }
        let evicted = self.delete_across(s, e);
        if !text.is_empty() {
            self.insert(s, Payload::Text(text), false);
        }
        evicted
    }

    /// Removes fully covered groups and shown cuts, clips partly covered ones (a group's active
    /// variant only), evicts points strictly inside.
    fn delete_across(&mut self, s: usize, e: usize) -> Vec<Arc<Cut>> {
        let nodes = std::mem::take(&mut self.nodes);
        let mut out = Vec::with_capacity(nodes.len());
        let mut evicted = Vec::new();
        let mut o = 0;
        for n in nodes {
            let l = n.len();
            let (ns, ne) = (o, o + l);
            o = ne;
            if ne <= s || ns >= e {
                out.push(n);
                continue;
            }
            let (cs, ce) = (s.max(ns) - ns, e.min(ne) - ns);
            match n {
                Node::Text(r) => {
                    let (bs, be) = (byte_idx(&r.text, cs), byte_idx(&r.text, ce));
                    let kept = format!("{}{}", &r.text[..bs], &r.text[be..]);
                    out.push(Node::text(&kept));
                }
                Node::Cut(c) if l == 0 => evicted.push(c),
                // A comment's marker stays where the text was: the comment shrinks.
                Node::CommentStart(_) | Node::CommentEnd(_) => out.push(n),
                Node::Group(_) if s <= ns && ne <= e => visible_points(&n, &mut evicted),
                Node::Cut(_) if s <= ns && ne <= e => {}
                Node::Group(mut g) => {
                    evicted.extend(Arc::make_mut(&mut g).active_content_mut().replace(cs, ce, ""));
                    out.push(Node::Group(g));
                }
                Node::Cut(mut c) => {
                    Arc::make_mut(&mut c).content.replace(cs, ce, "");
                    out.push(Node::Cut(c));
                }
            }
        }
        self.nodes = out;
        self.normalize();
        evicted
    }

    /// What [`Container::replace`] would delete, kept.
    pub(crate) fn take(&mut self, s: usize, e: usize) -> Vec<Node> {
        self.take_inner(s, e, true)
    }

    /// `cuts`: whether the text of cuts comes along (a move) or not (a copy).
    fn take_inner(&mut self, s: usize, e: usize, cuts: bool) -> Vec<Node> {
        if s >= e {
            return Vec::new();
        }
        if let Some((i, o, exact)) = self.enclosing(s, e) {
            if exact {
                let n = self.nodes.remove(i);
                self.normalize();
                return vec![n];
            }
            return self.with_inside(i, |c, in_cut| {
                let got = c.take_inner(s - o, e - o, cuts);
                if in_cut && !cuts {
                    Vec::new()
                } else {
                    got
                }
            });
        }
        let nodes = std::mem::take(&mut self.nodes);
        let (mut before, mut after) = (Vec::with_capacity(nodes.len()), Vec::new());
        let mut taken = Vec::new();
        let mut o = 0;
        for n in nodes {
            let l = n.len();
            let (ns, ne) = (o, o + l);
            o = ne;
            if ne <= s {
                before.push(n);
                continue;
            }
            if ns >= e {
                after.push(n);
                continue;
            }
            let (cs, ce) = (s.max(ns) - ns, e.min(ne) - ns);
            // A node only partly covered stays on the side it sticks out of.
            let left = ns < s;
            match n {
                Node::Text(r) => {
                    let (bs, be) = (byte_idx(&r.text, cs), byte_idx(&r.text, ce));
                    before.push(Node::text(&r.text[..bs]));
                    after.push(Node::text(&r.text[be..]));
                    taken.push(Node::text(&r.text[bs..be]));
                }
                Node::CommentStart(_) | Node::CommentEnd(_) => taken.push(n),
                Node::Group(_) | Node::Cut(_) if s <= ns && ne <= e => taken.push(n),
                Node::Group(mut g) => {
                    taken.extend(Arc::make_mut(&mut g).active_content_mut().take_inner(cs, ce, cuts));
                    if left { before.push(Node::Group(g)) } else { after.push(Node::Group(g)) }
                }
                Node::Cut(mut c) => {
                    let got = Arc::make_mut(&mut c).content.take_inner(cs, ce, cuts);
                    if cuts {
                        taken.extend(got);
                    }
                    if left { before.push(Node::Cut(c)) } else { after.push(Node::Cut(c)) }
                }
            }
        }
        // Whole comments go with the text; one only partly in it keeps its markers here.
        grab_edges(&mut before, &mut taken, &mut after);
        let stay = split_unmatched(&mut taken);
        before.extend(stay);
        before.extend(after);
        self.nodes = before;
        self.normalize();
        Container::from_nodes(taken).nodes
    }

    pub(crate) fn extract_kept(&self, s: usize, e: usize) -> Vec<Node> {
        drop_cuts(drop_comments(self.clone().take_inner(s, e, false)))
    }

    /// `s` and `e` must not fall strictly inside a group or a shown cut. Zero-width nodes at `s`
    /// go before, at `e` after.
    fn partition(&self, s: usize, e: usize) -> (Vec<Node>, Vec<Node>, Vec<Node>) {
        let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
        let mut o = 0;
        for n in &self.nodes {
            let l = n.len();
            let (ns, ne) = (o, o + l);
            o = ne;
            match n {
                Node::Text(r) if ns < e && s < ne => {
                    let (cs, ce) = (s.max(ns) - ns, e.min(ne) - ns);
                    let (bs, be) = (byte_idx(&r.text, cs), byte_idx(&r.text, ce));
                    a.push(Node::text(&r.text[..bs]));
                    b.push(Node::text(&r.text[bs..be]));
                    c.push(Node::text(&r.text[be..]));
                }
                _ if l == 0 => {
                    if ns <= s {
                        a.push(n.clone())
                    } else if ns < e {
                        b.push(n.clone())
                    } else {
                        c.push(n.clone())
                    }
                }
                _ if ne <= s => a.push(n.clone()),
                _ if ns >= e => c.push(n.clone()),
                _ => b.push(n.clone()),
            }
        }
        (a, b, c)
    }

    /// Nesting at any depth, no partial overlap.
    pub(crate) fn wrap(&mut self, s: usize, e: usize, group: impl FnOnce(Container) -> Group) -> Result<(), crate::EditError> {
        if let Some((i, o, exact)) = self.enclosing(s, e) {
            return match &self.nodes[i] {
                Node::Group(g) if exact => Err(crate::EditError::AlreadyGroup(g.id)),
                Node::Group(_) => self.with_group_at(i, |g| g.active_content_mut().wrap(s - o, e - o, group)),
                _ => Err(crate::EditError::InCut),
            };
        }
        if self.cuts_across(s, e) {
            return Err(crate::EditError::PartialOverlap);
        }
        let (mut a, b, c) = self.partition(s, e);
        a.push(Node::Group(Arc::new(group(Container::from_nodes(b)))));
        a.extend(c);
        *self = Container::from_nodes(a);
        Ok(())
    }

    /// Whether `s` or `e` falls strictly inside a group or a shown cut of this container.
    fn cuts_across(&self, s: usize, e: usize) -> bool {
        let mut o = 0;
        for n in &self.nodes {
            let (ns, ne) = (o, o + n.len());
            o = ne;
            if !matches!(n, Node::Text(_)) && ((ns < s && s < ne) || (ns < e && e < ne)) {
                return true;
            }
        }
        false
    }

    /// Partly covered groups and cuts grow the range to their whole; a range inside one variant
    /// is cut inside it; inside a shown cut it is refused.
    pub(crate) fn cut(&mut self, s: usize, e: usize, make: &mut dyn FnMut(Container) -> Cut, lost: &mut Vec<Arc<Cut>>) -> Result<Range<usize>, crate::EditError> {
        if s >= e {
            return Err(crate::EditError::EmptyRange);
        }
        if let Some((i, o, exact)) = self.enclosing(s, e) {
            match &self.nodes[i] {
                Node::Cut(_) => return Err(crate::EditError::InCut),
                Node::Group(_) if !exact => {
                    let r = self.with_group_at(i, |g| g.active_content_mut().cut(s - o, e - o, make, lost))?;
                    return Ok(r.start + o..r.end + o);
                }
                _ => {}
            }
        }
        let (mut s, mut e) = (s, e);
        let mut o = 0;
        for n in &self.nodes {
            let (ns, ne) = (o, o + n.len());
            o = ne;
            if !matches!(n, Node::Text(_)) && ns < e && s < ne {
                s = s.min(ns);
                e = e.max(ne);
            }
        }
        let (mut a, mut b, mut c) = self.partition(s, e);
        grab_edges(&mut a, &mut b, &mut c);
        let mut beside = Vec::new();
        let inner = absorb_cuts(b, true, &mut beside, lost);
        a.extend(beside);
        a.push(Node::Cut(Arc::new(make(Container::from_nodes(inner)))));
        a.append(&mut c);
        *self = Container::from_nodes(a);
        Ok(s..e)
    }

    /// Inside a run, group or shown cut where `pos` falls strictly inside one; a start goes after
    /// the comment ends already there.
    pub(crate) fn insert_marker(&mut self, pos: usize, m: Node) {
        match self.locate(pos) {
            Loc::InRun(i, off) => {
                let Node::Text(run) = &self.nodes[i] else { unreachable!() };
                let run = run.clone();
                let b = byte_idx(&run.text, off);
                self.nodes.splice(i..=i, [Node::text(&run.text[..b]), m, Node::text(&run.text[b..])]);
                self.normalize();
            }
            Loc::InNode(i, off) => self.with_inside(i, |c, _| c.insert_marker(off, m)),
            Loc::Boundary(mut i) => {
                if matches!(m, Node::CommentStart(_)) {
                    while matches!(self.nodes.get(i), Some(Node::CommentEnd(_))) {
                        i += 1;
                    }
                }
                self.nodes.insert(i, m);
                self.recompute();
            }
        }
    }

    /// Inside the version that holds the range, around a group it covers exactly; refused in a
    /// shown cut.
    pub(crate) fn add_comment(&mut self, s: usize, e: usize, start: Node, end: Node) -> Result<(), crate::EditError> {
        if let Some((i, o, exact)) = self.enclosing(s, e) {
            match &self.nodes[i] {
                Node::Cut(_) => return Err(crate::EditError::InCut),
                Node::Group(_) if !exact => {
                    return self.with_group_at(i, |g| g.active_content_mut().add_comment(s - o, e - o, start, end));
                }
                _ => {}
            }
        }
        self.insert_marker(e, end);
        self.insert_marker(s, start);
        Ok(())
    }

    pub(crate) fn has_comments(&self) -> bool {
        self.marks
    }

    /// Whether every comment here and below has its two markers in one container, in order,
    /// with something between them.
    pub(crate) fn comments_settled(&self) -> bool {
        if !self.marks {
            return true;
        }
        let mut open: Vec<(CommentId, bool)> = Vec::new();
        let mut seen: HashSet<CommentId> = HashSet::new();
        if !edges_settled(&self.nodes) || !runs_sorted(&self.nodes) {
            return false;
        }
        for n in &self.nodes {
            match n {
                Node::CommentStart(c) => {
                    if !seen.insert(c.id) {
                        return false;
                    }
                    open.push((c.id, false));
                }
                Node::CommentEnd(id) => match open.iter().position(|(o, _)| o == id) {
                    Some(k) if open[k].1 => {
                        open.remove(k);
                    }
                    _ => return false,
                },
                other => {
                    let inner_ok = match other {
                        Node::Group(g) => g.variants.iter().all(|v| v.content.comments_settled()),
                        Node::Cut(c) => c.content.comments_settled(),
                        _ => true,
                    };
                    if !inner_ok {
                        return false;
                    }
                    for o in &mut open {
                        o.1 = true;
                    }
                }
            }
        }
        open.is_empty()
    }

    /// A marker whose partner is outside its container moves out of a group to its outer edge
    /// (the comment grows to hold the group) and out of a cut to its inner edge (the cut's text
    /// leaves the comment); comments with nothing between their markers, or with them the wrong
    /// way round, go. Returns the markers still without a partner here, for the parent to place.
    /// Only paths that change are copied.
    pub(crate) fn settle_comments(&mut self) -> Vec<Node> {
        if self.comments_settled() {
            return Vec::new();
        }
        let nodes = std::mem::take(&mut self.nodes);
        let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
        let split = |lifted: Vec<Node>| -> (Vec<Node>, Vec<Node>) { lifted.into_iter().partition(|n| matches!(n, Node::CommentStart(_))) };
        for n in nodes {
            match n {
                Node::Group(mut g) if !g.variants.iter().all(|v| v.content.comments_settled()) => {
                    let mut lifted = Vec::new();
                    for v in &mut Arc::make_mut(&mut g).variants {
                        lifted.extend(v.content.settle_comments());
                    }
                    let (starts, ends) = split(lifted);
                    out.extend(starts);
                    out.push(Node::Group(g));
                    out.extend(ends);
                }
                Node::Cut(mut c) if !c.content.comments_settled() => {
                    let lifted = Arc::make_mut(&mut c).content.settle_comments();
                    let (starts, ends) = split(lifted);
                    out.extend(ends);
                    out.push(Node::Cut(c));
                    out.extend(starts);
                }
                n => out.push(n),
            }
        }
        shed_edge_cuts(&mut out);
        let mut drop = vec![false; out.len()];
        let mut start_at: HashMap<CommentId, usize> = HashMap::new();
        let mut end_at: HashMap<CommentId, usize> = HashMap::new();
        for (i, n) in out.iter().enumerate() {
            if let Some((id, start)) = n.marker() {
                let at = if start { &mut start_at } else { &mut end_at };
                match at.entry(id) {
                    std::collections::hash_map::Entry::Occupied(_) => drop[i] = true,
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(i);
                    }
                }
            }
        }
        let mut unmatched = Vec::new();
        for (id, &si) in &start_at {
            match end_at.get(id) {
                Some(&ei) if si < ei && out[si + 1..ei].iter().any(|n| n.marker().is_none()) => {}
                Some(&ei) => {
                    drop[si] = true;
                    drop[ei] = true;
                }
                None => {
                    drop[si] = true;
                    unmatched.push(si);
                }
            }
        }
        for (id, &ei) in &end_at {
            if !start_at.contains_key(id) {
                drop[ei] = true;
                unmatched.push(ei);
            }
        }
        unmatched.sort_unstable();
        let lifted: Vec<Node> = unmatched.iter().map(|&i| out[i].clone()).collect();
        self.nodes = out.into_iter().zip(drop).filter(|(_, d)| !d).map(|(n, _)| n).collect();
        sort_runs(&mut self.nodes);
        self.normalize();
        lifted
    }

    pub(crate) fn modify_comment(&mut self, id: CommentId, f: &mut dyn FnMut(&mut Comment)) -> bool {
        for n in &mut self.nodes {
            let found = match n {
                Node::CommentStart(c) if c.id == id => {
                    f(Arc::make_mut(c));
                    return true;
                }
                Node::Group(g) if g.variants.iter().any(|v| v.content.find_comment(id).is_some()) => {
                    Arc::make_mut(g).variants.iter_mut().any(|v| v.content.modify_comment(id, f))
                }
                Node::Cut(c) if c.content.find_comment(id).is_some() => Arc::make_mut(c).content.modify_comment(id, f),
                _ => false,
            };
            if found {
                return true;
            }
        }
        false
    }

    pub(crate) fn remove_comment(&mut self, id: CommentId) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| n.marker().is_none_or(|(m, _)| m != id));
        if self.nodes.len() != before {
            self.normalize();
            return true;
        }
        for i in 0..self.nodes.len() {
            let hit = match &self.nodes[i] {
                Node::Group(g) => g.variants.iter().any(|v| v.content.find_comment(id).is_some()),
                Node::Cut(c) => c.content.find_comment(id).is_some(),
                _ => false,
            };
            if hit {
                match &self.nodes[i] {
                    Node::Group(_) => self.with_group_at(i, |g| g.variants.iter_mut().any(|v| v.content.remove_comment(id))),
                    _ => self.with_cut_at(i, |c| c.content.remove_comment(id)),
                };
                return true;
            }
        }
        false
    }

    /// Not in cuts. Returns how many.
    pub(crate) fn remove_all_comments(&mut self) -> usize {
        let mut n = self.nodes.iter().filter(|n| matches!(n, Node::CommentStart(_))).count();
        self.nodes.retain(|n| n.marker().is_none());
        for node in &mut self.nodes {
            if let Node::Group(g) = node {
                if g.variants.iter().any(|v| v.content.has_comments()) {
                    for v in &mut Arc::make_mut(g).variants {
                        n += v.content.remove_all_comments();
                    }
                }
            }
        }
        self.normalize();
        n
    }

    pub(crate) fn find_comment(&self, id: CommentId) -> Option<&Comment> {
        for n in &self.nodes {
            let found = match n {
                Node::CommentStart(c) if c.id == id => return Some(c),
                Node::Group(g) => g.variants.iter().find_map(|v| v.content.find_comment(id)),
                Node::Cut(c) => c.content.find_comment(id),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// The visible path only, by start; comments inside cuts aren't on the page.
    pub(crate) fn collect_comments<'a>(&'a self, base: usize, out: &mut Vec<(&'a Comment, Range<usize>)>) {
        let mut o = base;
        let mut open: Vec<(usize, usize)> = Vec::new();
        for n in &self.nodes {
            match n {
                Node::CommentStart(c) => {
                    open.push((out.len(), o));
                    out.push((c, o..o));
                }
                Node::CommentEnd(id) => {
                    if let Some(k) = open.iter().position(|&(i, _)| out[i].0.id == *id) {
                        let (i, s) = open.remove(k);
                        out[i].1 = s..o;
                    }
                }
                Node::Group(g) => g.variants[g.active].content.collect_comments(o, out),
                _ => {}
            }
            o += n.len();
        }
    }

    /// Inside versions and cuts too, in order.
    pub(crate) fn all_comments<'a>(&'a self, out: &mut Vec<&'a Comment>) {
        for n in &self.nodes {
            match n {
                Node::CommentStart(c) => out.push(c),
                Node::Group(g) => {
                    for v in &g.variants {
                        v.content.all_comments(out);
                    }
                }
                Node::Cut(c) => c.content.all_comments(out),
                _ => {}
            }
        }
    }

    /// `f(text, in_a_cut)` for each run on the page, in display order.
    pub(crate) fn pieces(&self, in_cut: bool, f: &mut dyn FnMut(&str, bool)) {
        for n in &self.nodes {
            match n {
                Node::Text(r) => f(&r.text, in_cut),
                Node::Group(g) => g.variants[g.active].content.pieces(in_cut, f),
                Node::Cut(c) if c.shown => c.content.pieces(true, f),
                Node::Cut(_) | Node::CommentStart(_) | Node::CommentEnd(_) => {}
            }
        }
    }

    pub(crate) fn text(&self) -> String {
        let mut s = String::new();
        self.pieces(false, &mut |t, _| s.push_str(t));
        s
    }

    /// Every cut left out: what a file's body holds.
    pub(crate) fn kept_text(&self) -> String {
        let mut s = String::new();
        self.pieces(false, &mut |t, cut| {
            if !cut {
                s.push_str(t)
            }
        });
        s
    }

    /// Like [`Self::pieces`], with `Open(others)` / `Close` around the active variant of each
    /// visible group that has other versions (`others`: their text, in order, the active one
    /// left out), and `Point` where a cut takes no room. Groups in cuts get no marks: their text
    /// isn't in the final text.
    pub(crate) fn pieces_marked(&self, in_cut: bool, f: &mut dyn FnMut(Piece<'_>)) {
        for n in &self.nodes {
            match n {
                Node::Text(r) => f(Piece::Text(&r.text, in_cut)),
                Node::Cut(c) if c.shown => c.content.pieces_marked(true, f),
                Node::Cut(_) => f(Piece::Point),
                Node::CommentStart(c) if !in_cut => f(Piece::CommentStart(c)),
                Node::CommentEnd(id) if !in_cut => f(Piece::CommentEnd(*id)),
                Node::CommentStart(_) | Node::CommentEnd(_) => {}
                Node::Group(g) => {
                    let active = g.variants[g.active].content.kept_text();
                    let others: Vec<String> = if in_cut || g.variants.len() < 2 {
                        Vec::new()
                    } else {
                        g.variants
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i != g.active)
                            .map(|(_, v)| v.content.kept_text().trim().to_string())
                            .filter(|t| !t.is_empty() && *t != active.trim())
                            .collect()
                    };
                    // Nothing kept (a version without the words): no place in the final text,
                    // and no marks to split the spaces around it.
                    let marked = !others.is_empty() && !active.is_empty();
                    if marked {
                        f(Piece::Open(others));
                    }
                    g.variants[g.active].content.pieces_marked(in_cut, f);
                    if marked {
                        f(Piece::Close);
                    }
                }
            }
        }
    }

    pub(crate) fn text_range(&self, s: usize, e: usize, out: &mut String) {
        let mut o = 0;
        for n in &self.nodes {
            if o >= e {
                break;
            }
            let l = n.len();
            let (ns, ne) = (o, o + l);
            o = ne;
            if ne <= s || l == 0 {
                continue;
            }
            let (cs, ce) = (s.max(ns) - ns, e.min(ne) - ns);
            match n {
                Node::Text(r) => {
                    out.push_str(&r.text[byte_idx(&r.text, cs)..byte_idx(&r.text, ce)])
                }
                Node::Group(g) => g.variants[g.active].content.text_range(cs, ce, out),
                Node::Cut(c) => c.content.text_range(cs, ce, out),
                Node::CommentStart(_) | Node::CommentEnd(_) => {}
            }
        }
    }

    /// Only active variants and shown cuts are searched.
    pub(crate) fn visible_range(&self, id: GroupId, base: usize) -> Option<Range<usize>> {
        let mut o = base;
        for n in &self.nodes {
            let l = n.len();
            match n {
                Node::Group(g) => {
                    if g.id == id {
                        return Some(o..o + l);
                    }
                    if let Some(r) = g.variants[g.active].content.visible_range(id, o) {
                        return Some(r);
                    }
                }
                Node::Cut(c) if c.shown => {
                    if let Some(r) = c.content.visible_range(id, o) {
                        return Some(r);
                    }
                }
                _ => {}
            }
            o += l;
        }
        None
    }

    pub(crate) fn contains_group(&self, id: GroupId) -> bool {
        self.nodes.iter().any(|n| match n {
            Node::Group(g) => g.contains_group(id),
            Node::Cut(c) => c.content.contains_group(id),
            _ => false,
        })
    }

    pub(crate) fn contains_cut(&self, id: CutId) -> bool {
        self.nodes.iter().any(|n| match n {
            Node::Group(g) => g.contains_cut(id),
            Node::Cut(c) => c.id == id,
            _ => false,
        })
    }

    pub(crate) fn has_cuts(&self) -> bool {
        self.nodes.iter().any(|n| match n {
            Node::Group(g) => g.contains_any_cut(),
            Node::Cut(_) => true,
            _ => false,
        })
    }

    pub(crate) fn find_group(&self, id: GroupId) -> Option<&Group> {
        for n in &self.nodes {
            match n {
                Node::Group(g) => {
                    if g.id == id {
                        return Some(g);
                    }
                    if let Some(found) = g.variants.iter().find_map(|v| v.content.find_group(id)) {
                        return Some(found);
                    }
                }
                Node::Cut(c) => {
                    if let Some(found) = c.content.find_group(id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn find_cut(&self, id: CutId) -> Option<&Cut> {
        for n in &self.nodes {
            match n {
                Node::Cut(c) if c.id == id => return Some(c),
                Node::Group(g) => {
                    if let Some(found) = g.variants.iter().find_map(|v| v.content.find_cut(id)) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Anywhere, inactive variants and cuts included; refreshes cached lengths along the path.
    pub(crate) fn modify_group<R>(
        &mut self,
        id: GroupId,
        f: &mut dyn FnMut(&mut Group) -> R,
    ) -> Option<R> {
        let i = self.nodes.iter().position(|n| match n {
            Node::Group(g) => g.contains_group(id),
            Node::Cut(c) => c.content.contains_group(id),
            _ => false,
        })?;
        if matches!(self.nodes[i], Node::Cut(_)) {
            return self.with_cut_at(i, |c| c.content.modify_group(id, f));
        }
        self.with_group_at(i, |g| {
            if g.id == id {
                return Some(f(g));
            }
            let vi = g
                .variants
                .iter()
                .position(|v| v.content.contains_group(id))?;
            g.variants[vi].content.modify_group(id, f)
        })
    }

    pub(crate) fn modify_parent(
        &mut self,
        id: GroupId,
        f: &mut dyn FnMut(&mut Container, usize),
    ) -> bool {
        if let Some(i) = self
            .nodes
            .iter()
            .position(|n| matches!(n, Node::Group(g) if g.id == id))
        {
            f(self, i);
            self.normalize();
            return true;
        }
        let Some(i) = self.nodes.iter().position(|n| match n {
            Node::Group(g) => g.contains_group(id),
            Node::Cut(c) => c.content.contains_group(id),
            _ => false,
        }) else {
            return false;
        };
        if matches!(self.nodes[i], Node::Cut(_)) {
            return self.with_cut_at(i, |c| c.content.modify_parent(id, f));
        }
        self.with_group_at(i, |g| {
            g.variants
                .iter_mut()
                .any(|v| v.content.contains_group(id) && v.content.modify_parent(id, f))
        })
    }

    pub(crate) fn modify_cut_parent<R>(
        &mut self,
        id: CutId,
        f: &mut dyn FnMut(&mut Container, usize) -> R,
    ) -> Option<R> {
        if let Some(i) = self
            .nodes
            .iter()
            .position(|n| matches!(n, Node::Cut(c) if c.id == id))
        {
            let r = f(self, i);
            self.normalize();
            return Some(r);
        }
        let i = self
            .nodes
            .iter()
            .position(|n| matches!(n, Node::Group(g) if g.contains_cut(id)))?;
        self.with_group_at(i, |g| {
            let vi = g.variants.iter().position(|v| v.content.contains_cut(id))?;
            g.variants[vi].content.modify_cut_parent(id, f)
        })
    }

    /// Refreshes cached lengths along the path.
    pub(crate) fn modify_cut<R>(&mut self, id: CutId, f: &mut dyn FnMut(&mut Cut) -> R) -> Option<R> {
        self.modify_cut_parent(id, &mut |c, i| c.with_cut_at(i, &mut *f))
    }

    pub(crate) fn collect_group_ids(&self, out: &mut Vec<GroupId>) {
        for n in &self.nodes {
            match n {
                Node::Group(g) => {
                    out.push(g.id);
                    for v in &g.variants {
                        v.content.collect_group_ids(out);
                    }
                }
                Node::Cut(c) => c.content.collect_group_ids(out),
                _ => {}
            }
        }
    }

    /// `(cut, display range, visible)`; a cut in a hidden version gets the outermost hidden
    /// group's place.
    /// Cuts that are the whole of a version: the cut, its group and the version's index.
    pub(crate) fn collect_cut_versions(&self, out: &mut Vec<(CutId, GroupId, usize)>) {
        for n in &self.nodes {
            if let Node::Group(g) = n {
                for (k, v) in g.variants.iter().enumerate() {
                    if let Some(c) = v.words_cut() {
                        out.push((c.id, g.id, k));
                    }
                    v.content.collect_cut_versions(out);
                }
            }
        }
    }

    /// The cut, when it is all there is (comment markers aside): a version without the words.
    pub(crate) fn only_cut(&self) -> Option<&Cut> {
        let mut it = self.nodes.iter().filter(|n| !matches!(n, Node::CommentStart(_) | Node::CommentEnd(_)));
        match (it.next(), it.next()) {
            (Some(Node::Cut(c)), None) => Some(c),
            _ => None,
        }
    }

    pub(crate) fn collect_cuts<'a>(&'a self, base: usize, visible: bool, out: &mut Vec<(&'a Cut, Range<usize>, bool)>) {
        let mut o = base;
        for n in &self.nodes {
            let l = n.len();
            match n {
                Node::Cut(c) => out.push((c, if visible { o..o + l } else { base..base }, visible)),
                Node::Group(g) => {
                    for (k, v) in g.variants.iter().enumerate() {
                        let shows = visible && k == g.active;
                        v.content.collect_cuts(if visible { o } else { base }, shows, out);
                    }
                }
                _ => {}
            }
            o += l;
        }
    }

    pub(crate) fn collect_visible(
        &self,
        base: usize,
        depth: usize,
        in_cut: bool,
        out: &mut Vec<crate::VisibleGroup>,
    ) {
        let mut o = base;
        for n in &self.nodes {
            let l = n.len();
            match n {
                Node::Group(g) => {
                    out.push(crate::VisibleGroup {
                        id: g.id,
                        scope: g.scope,
                        range: o..o + l,
                        depth,
                        active: g.active,
                        variant_count: g.variants.len(),
                        ghosted: in_cut,
                        cut: g.variants[g.active].words_cut().is_some(),
                    });
                    g.variants[g.active]
                        .content
                        .collect_visible(o, depth + 1, in_cut, out);
                }
                Node::Cut(c) if c.shown => c.content.collect_visible(o, depth, true, out),
                _ => {}
            }
            o += l;
        }
    }

    /// Sets `shown` on every cut to `want(cut)`; only paths that change are copied.
    pub(crate) fn present(&mut self, want: &dyn Fn(&Cut) -> bool) -> bool {
        let mut changed = false;
        for n in &mut self.nodes {
            match n {
                Node::Cut(c) => {
                    let w = want(c);
                    if c.shown != w {
                        Arc::make_mut(c).shown = w;
                        changed = true;
                    }
                }
                Node::Group(g) if g.variants.iter().any(|v| v.content.needs_present(want)) => {
                    for v in &mut Arc::make_mut(g).variants {
                        changed |= v.content.present(want);
                    }
                }
                _ => {}
            }
        }
        if changed {
            self.recompute();
        }
        changed
    }

    fn needs_present(&self, want: &dyn Fn(&Cut) -> bool) -> bool {
        self.nodes.iter().any(|n| match n {
            Node::Cut(c) => c.shown != want(c),
            Node::Group(g) => g.variants.iter().any(|v| v.content.needs_present(want)),
            _ => false,
        })
    }

    /// Drops versions left with nothing in them (a cut that was all of a version went), and
    /// groups left with no version. Only paths that change are copied.
    pub(crate) fn prune_empty(&mut self) -> bool {
        fn needs(c: &Container) -> bool {
            c.nodes.iter().any(|n| match n {
                Node::Group(g) => g.variants.iter().any(|v| v.content.nodes.is_empty() || needs(&v.content)),
                _ => false,
            })
        }
        if !needs(self) {
            return false;
        }
        let nodes = std::mem::take(&mut self.nodes);
        for n in nodes {
            match n {
                Node::Group(mut g) if g.variants.iter().any(|v| v.content.nodes.is_empty() || needs(&v.content)) => {
                    let gm = Arc::make_mut(&mut g);
                    let active = gm.variants[gm.active].id;
                    for v in &mut gm.variants {
                        v.content.prune_empty();
                    }
                    gm.variants.retain(|v| !v.content.nodes.is_empty());
                    gm.keep_original();
                    if gm.variants.is_empty() {
                        continue;
                    }
                    gm.active = gm.variants.iter().position(|v| v.id == active).unwrap_or(0).min(gm.variants.len() - 1);
                    self.nodes.push(Node::Group(g));
                }
                n => self.nodes.push(n),
            }
        }
        self.normalize();
        true
    }

    pub(crate) fn check(
        &self,
        path: &str,
        ids: &mut HashSet<u64>,
        allow_empty: bool,
        cuts_allowed: bool,
    ) -> Result<(), String> {
        let mut sum = 0;
        for (i, n) in self.nodes.iter().enumerate() {
            match n {
                Node::Text(r) => {
                    if r.text.is_empty() {
                        return Err(format!("{path}: empty run at {i}"));
                    }
                    if r.len != r.text.chars().count() {
                        return Err(format!("{path}: stale run length at {i}"));
                    }
                    if r.text.find('\n').is_some_and(|j| j + 1 < r.text.len()) {
                        return Err(format!("{path}: run {i} has an inner newline"));
                    }
                    if let Some(Node::Text(prev)) = i.checked_sub(1).map(|j| &self.nodes[j]) {
                        if !prev.text.ends_with('\n') {
                            return Err(format!("{path}: runs {} and {i} should be merged", i - 1));
                        }
                    }
                }
                Node::Group(g) => {
                    let p = format!("{path}/g{}", g.id.0);
                    if g.variants.is_empty() || g.active >= g.variants.len() {
                        return Err(format!(
                            "{p}: bad active {} of {}",
                            g.active,
                            g.variants.len()
                        ));
                    }
                    if !ids.insert(g.id.0) {
                        return Err(format!("{p}: duplicate group id"));
                    }
                    if g.variants[0].without {
                        return Err(format!("{p}: the original is a version without the words"));
                    }
                    for v in &g.variants {
                        if !ids.insert(v.id.0) {
                            return Err(format!("{p}: duplicate variant id {}", v.id.0));
                        }
                        v.content.check(&format!("{p}/v{}", v.id.0), ids, false, cuts_allowed)?;
                    }
                }
                Node::Cut(c) => {
                    let p = format!("{path}/c{}", c.id.0);
                    if !cuts_allowed {
                        return Err(format!("{p}: a cut inside a cut or a note"));
                    }
                    if !ids.insert(c.id.0) {
                        return Err(format!("{p}: duplicate cut id"));
                    }
                    c.content.check(&p, ids, false, false)?;
                }
                Node::CommentStart(c) => {
                    if !ids.insert(c.id.0) {
                        return Err(format!("{path}: duplicate comment id {}", c.id.0));
                    }
                }
                Node::CommentEnd(_) => {}
            }
            sum += n.len();
        }
        if sum != self.len {
            return Err(format!("{path}: cached len {} != {sum}", self.len));
        }
        if !allow_empty && self.nodes.is_empty() {
            return Err(format!("{path}: empty variant"));
        }
        if self.has_comments() && !self.comments_settled() {
            return Err(format!("{path}: comments not settled"));
        }
        if self.marks != self.nodes.iter().any(Node::has_marks) {
            return Err(format!("{path}: stale marks flag"));
        }
        Ok(())
    }
}

/// Fresh group, variant and cut ids: pasting must never duplicate ids.
pub(crate) fn reid(nodes: &[Node], next: &mut u64) -> Vec<Node> {
    let mut comments: HashMap<CommentId, CommentId> = HashMap::new();
    let mut fresh = |id: CommentId, next: &mut u64| {
        *comments.entry(id).or_insert_with(|| {
            *next += 1;
            CommentId(*next)
        })
    };
    nodes
        .iter()
        .map(|n| match n {
            Node::Text(_) => n.clone(),
            Node::CommentStart(c) => Node::CommentStart(Arc::new(Comment { id: fresh(c.id, next), ..(**c).clone() })),
            Node::CommentEnd(id) => Node::CommentEnd(fresh(*id, next)),
            Node::Cut(c) => {
                *next += 1;
                Node::Cut(Arc::new(Cut {
                    id: CutId(*next),
                    content: Container { nodes: reid(&c.content.nodes, next), len: c.content.len, marks: c.content.marks },
                    ..(**c).clone()
                }))
            }
            Node::Group(g) => {
                *next += 1;
                let id = GroupId(*next);
                let variants = g
                    .variants
                    .iter()
                    .map(|v| {
                        *next += 1;
                        Variant {
                            id: VariantId(*next),
                            by: v.by,
                            without: v.without,
                            content: Container {
                                nodes: reid(&v.content.nodes, next),
                                len: v.content.len,
                                marks: v.content.marks,
                            },
                        }
                    })
                    .collect();
                Node::Group(Arc::new(Group {
                    id,
                    scope: g.scope,
                    active: g.active,
                    variants,
                }))
            }
        })
        .collect()
}

/// `(old range, new length)`, or `None` if equal. Equal nodes are skipped from both ends (shared
/// `Arc`s compare by pointer). If exactly one group differs and only inside its active
/// variant, or one shown cut only in its text, the search descends into it.
pub(crate) fn diff(old: &Container, new: &Container) -> Option<(Range<usize>, usize)> {
    let (a, b) = (&old.nodes, &new.nodes);
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    if p == a.len() && p == b.len() {
        return None;
    }
    let mut q = 0;
    while q < a.len() - p && q < b.len() - p && a[a.len() - 1 - q] == b[b.len() - 1 - q] {
        q += 1;
    }
    let pre: usize = a[..p].iter().map(Node::len).sum();
    let (am, bm) = (&a[p..a.len() - q], &b[p..b.len() - q]);
    match (am, bm) {
        ([Node::Group(x)], [Node::Group(y)]) => {
            let same_shell = x.id == y.id
                && x.scope == y.scope
                && x.active == y.active
                && x.variants.len() == y.variants.len()
                && x.variants
                    .iter()
                    .zip(&y.variants)
                    .enumerate()
                    .all(|(i, (u, v))| {
                        u.id == v.id && u.by == v.by && (i == x.active || u.content == v.content)
                    });
            if same_shell {
                let (u, v) = (&x.variants[x.active].content, &y.variants[y.active].content);
                if let Some((r, n)) = diff(u, v) {
                    return Some((pre + r.start..pre + r.end, n));
                }
            }
        }
        ([Node::Cut(x)], [Node::Cut(y)]) if x.shown && y.shown => {
            let same_shell = x.id == y.id && x.look == y.look && x.made == y.made && x.paragraph == y.paragraph && x.batch == y.batch;
            if same_shell {
                if let Some((r, n)) = diff(&x.content, &y.content) {
                    return Some((pre + r.start..pre + r.end, n));
                }
            }
        }
        _ => {}
    }
    let old_len: usize = am.iter().map(Node::len).sum();
    let new_len: usize = bm.iter().map(Node::len).sum();
    Some((pre..pre + old_len, new_len))
}
