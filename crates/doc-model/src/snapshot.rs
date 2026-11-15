use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::tree::{self, Container, Node, Run, Variant};
use crate::{Batch, CommentId, CutId, GroupId, Look, Scope, Source, VariantId};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DocTree {
    pub page: Vec<TreeNode>,
    pub notes: Vec<TreeNote>,
}

/// A cut with no spot. `origin`: up to 24 chars of the paragraph it was stashed from. Holds no cuts.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TreeNote {
    pub content: Vec<TreeNode>,
    pub origin: Option<String>,
    /// Stashed as whole paragraphs: dragged back, it lands between paragraphs.
    pub paragraph: bool,
    /// The file it came from, relative to the project.
    pub source: Option<String>,
    /// Its spot is gone; it keeps the look its trace had.
    pub lost: bool,
    pub look: Option<Look>,
    pub batch: Option<Batch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeNode {
    Text {
        text: String,
    },
    Group(TreeGroup),
    Cut(TreeCut),
    /// `id` pairs it with its end within this tree only.
    CommentStart(TreeComment),
    CommentEnd { id: u64 },
}

/// `author` `None`: the writer.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TreeComment {
    pub id: u64,
    pub text: String,
    pub created: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeGroup {
    pub id: GroupId,
    pub scope: Scope,
    pub active: usize,
    /// Index 0 is the original.
    pub variants: Vec<TreeVariant>,
}

/// Its text is never empty and holds no cuts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeCut {
    pub content: Vec<TreeNode>,
    /// Its text shows, faded; otherwise it takes no room (a mark, or hidden).
    pub shown: bool,
    /// Its own look; `None` follows the app's setting (`"trace":true`).
    pub look: Option<Look>,
    /// Whole paragraphs: `content` ends (at the page's end, starts) with the blank line it took.
    pub paragraph: bool,
    pub batch: Option<Batch>,
}

impl TreeCut {
    /// A cut that shows, follows the setting and isn't part of a batch.
    pub fn new(content: Vec<TreeNode>) -> Self {
        TreeCut { content, shown: true, look: None, paragraph: false, batch: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeVariant {
    pub by: Source,
    pub content: Vec<TreeNode>,
    /// A version without the words: `content` is them, cut.
    pub without: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    NoVariants(GroupId),
    ActiveOutOfRange(GroupId),
    EmptyVariant(GroupId),
    EmptyCut,
    NestedCut,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::NoVariants(g) => write!(f, "group {} has no variants", g.0),
            SnapshotError::ActiveOutOfRange(g) => write!(f, "group {} has a bad active index", g.0),
            SnapshotError::EmptyVariant(g) => write!(f, "group {} has an empty variant", g.0),
            SnapshotError::EmptyCut => write!(f, "a cut has no text"),
            SnapshotError::NestedCut => write!(f, "a cut inside a cut or a note"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl TreeNode {
    pub fn text(text: impl Into<String>) -> Self {
        TreeNode::Text { text: text.into() }
    }

    /// Display chars: groups count their active variant, shown cuts their text.
    pub fn len(&self) -> usize {
        match self {
            TreeNode::Text { text, .. } => text.chars().count(),
            TreeNode::Group(g) => g
                .variants
                .get(g.active)
                .map_or(0, |v| content_len(&v.content)),
            TreeNode::Cut(c) if c.shown => content_len(&c.content),
            TreeNode::Cut(_) | TreeNode::CommentStart(_) | TreeNode::CommentEnd { .. } => 0,
        }
    }

    /// Cuts count nothing.
    pub fn kept_len(&self) -> usize {
        match self {
            TreeNode::Text { text, .. } => text.chars().count(),
            TreeNode::Group(g) => g.variants.get(g.active).map_or(0, |v| kept_len(&v.content)),
            TreeNode::Cut(_) | TreeNode::CommentStart(_) | TreeNode::CommentEnd { .. } => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn content_len(nodes: &[TreeNode]) -> usize {
    nodes.iter().map(TreeNode::len).sum()
}

pub fn kept_len(nodes: &[TreeNode]) -> usize {
    nodes.iter().map(TreeNode::kept_len).sum()
}

pub fn content_text(nodes: &[TreeNode]) -> String {
    let mut s = String::new();
    push_text(nodes, &mut s, true);
    s
}

pub fn kept_text(nodes: &[TreeNode]) -> String {
    let mut s = String::new();
    push_text(nodes, &mut s, false);
    s
}

fn push_text(nodes: &[TreeNode], s: &mut String, cuts: bool) {
    for n in nodes {
        match n {
            TreeNode::Text { text, .. } => s.push_str(text),
            TreeNode::Group(g) => {
                if let Some(v) = g.variants.get(g.active) {
                    push_text(&v.content, s, cuts)
                }
            }
            TreeNode::Cut(c) if cuts && c.shown => push_text(&c.content, s, cuts),
            TreeNode::Cut(_) | TreeNode::CommentStart(_) | TreeNode::CommentEnd { .. } => {}
        }
    }
}

impl DocTree {
    pub fn text(&self) -> String {
        content_text(&self.page)
    }

    pub fn strip_ids(&mut self) {
        fn strip(nodes: &mut [TreeNode]) {
            for n in nodes {
                match n {
                    TreeNode::Group(g) => {
                        g.id = GroupId(0);
                        for v in &mut g.variants {
                            strip(&mut v.content);
                        }
                    }
                    TreeNode::Cut(c) => strip(&mut c.content),
                    TreeNode::CommentStart(c) => c.id = 0,
                    TreeNode::CommentEnd { id } => *id = 0,
                    TreeNode::Text { .. } => {}
                }
            }
        }
        strip(&mut self.page);
        for n in &mut self.notes {
            strip(&mut n.content);
        }
    }

    /// Both trees should be normalized, as [`crate::Document::snapshot`] returns them.
    pub fn eq_ignoring_ids(&self, other: &DocTree) -> bool {
        let (mut a, mut b) = (self.clone(), other.clone());
        a.strip_ids();
        b.strip_ids();
        a == b
    }
}

/// Comment ids renumbered 1, 2, 3… in document order, so equal documents give equal snapshots.
#[derive(Default)]
pub(crate) struct CommentNumbers(HashMap<CommentId, u64>);

impl CommentNumbers {
    fn of(&mut self, id: CommentId) -> u64 {
        let n = self.0.len() as u64 + 1;
        *self.0.entry(id).or_insert(n)
    }
}

pub(crate) fn from_container(c: &Container, nums: &mut CommentNumbers) -> Vec<TreeNode> {
    c.nodes
        .iter()
        .map(|n| match n {
            Node::Text(r) => TreeNode::Text { text: r.text.clone() },
            Node::Cut(c) => TreeNode::Cut(TreeCut {
                content: from_container(&c.content, nums),
                shown: c.shown,
                look: c.look,
                paragraph: c.paragraph,
                batch: c.batch.as_deref().cloned(),
            }),
            Node::CommentStart(c) => TreeNode::CommentStart(TreeComment {
                id: nums.of(c.id),
                text: c.text.clone(),
                created: c.created.clone(),
                author: c.author.clone(),
            }),
            Node::CommentEnd(id) => TreeNode::CommentEnd { id: nums.of(*id) },
            Node::Group(g) => TreeNode::Group(TreeGroup {
                id: g.id,
                scope: g.scope,
                active: g.active,
                variants: g
                    .variants
                    .iter()
                    .map(|v| TreeVariant {
                        by: v.by,
                        content: from_container(&v.content, nums),
                        without: v.without,
                    })
                    .collect(),
            }),
        })
        .collect()
}

/// Group ids are kept when unique; duplicates and all variants get fresh ids above the largest.
pub(crate) struct Builder {
    seen: HashSet<u64>,
    pub(crate) next_id: u64,
    /// Tree comment id → fresh document id.
    comments: HashMap<u64, u64>,
}

impl Builder {
    pub(crate) fn new(tree: &DocTree) -> Self {
        fn max_id(nodes: &[TreeNode], m: &mut u64) {
            for n in nodes {
                match n {
                    TreeNode::Group(g) => {
                        *m = (*m).max(g.id.0);
                        for v in &g.variants {
                            max_id(&v.content, m);
                        }
                    }
                    TreeNode::Cut(c) => max_id(&c.content, m),
                    _ => {}
                }
            }
        }
        let mut m = 0;
        max_id(&tree.page, &mut m);
        for n in &tree.notes {
            max_id(&n.content, &mut m);
        }
        Builder {
            seen: HashSet::new(),
            next_id: m,
            comments: HashMap::new(),
        }
    }

    pub(crate) fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Fresh, and the same for both its markers.
    fn comment_id(&mut self, id: u64) -> CommentId {
        if let Some(&c) = self.comments.get(&id) {
            return CommentId(c);
        }
        let c = self.alloc();
        self.comments.insert(id, c);
        CommentId(c)
    }

    /// `cuts`: whether the nodes may hold cuts (not inside a cut or a note).
    pub(crate) fn container(&mut self, nodes: &[TreeNode], cuts: bool) -> Result<Container, SnapshotError> {
        let mut out = Vec::with_capacity(nodes.len());
        for n in nodes {
            match n {
                TreeNode::Text { text } => out.push(Node::Text(Arc::new(Run::new(text.clone())))),
                TreeNode::CommentStart(c) => out.push(Node::CommentStart(Arc::new(tree::Comment {
                    id: self.comment_id(c.id),
                    text: c.text.clone(),
                    created: c.created.clone(),
                    author: c.author.clone(),
                }))),
                TreeNode::CommentEnd { id } => out.push(Node::CommentEnd(self.comment_id(*id))),
                TreeNode::Cut(c) => {
                    if !cuts {
                        return Err(SnapshotError::NestedCut);
                    }
                    let content = self.container(&c.content, false)?;
                    if content.nodes.is_empty() {
                        return Err(SnapshotError::EmptyCut);
                    }
                    out.push(Node::Cut(Arc::new(tree::Cut {
                        id: CutId(self.alloc()),
                        content,
                        shown: c.shown,
                        look: c.look,
                        made: 0,
                        paragraph: c.paragraph,
                        batch: c.batch.clone().map(Arc::new),
                    })));
                }
                TreeNode::Group(g) => {
                    if g.variants.is_empty() {
                        return Err(SnapshotError::NoVariants(g.id));
                    }
                    if g.active >= g.variants.len() {
                        return Err(SnapshotError::ActiveOutOfRange(g.id));
                    }
                    let id = if self.seen.insert(g.id.0) {
                        g.id
                    } else {
                        let id = self.alloc();
                        self.seen.insert(id);
                        GroupId(id)
                    };
                    let mut variants = Vec::with_capacity(g.variants.len());
                    for v in &g.variants {
                        let content = self.container(&v.content, cuts)?;
                        if content.nodes.is_empty() {
                            return Err(SnapshotError::EmptyVariant(g.id));
                        }
                        variants.push(Variant {
                            id: VariantId(self.alloc()),
                            by: v.by,
                            content,
                            without: v.without,
                        });
                    }
                    out.push(Node::Group(Arc::new(tree::Group {
                        id,
                        scope: g.scope,
                        active: g.active,
                        variants,
                    })));
                }
            }
        }
        Ok(Container::from_nodes(out))
    }
}
