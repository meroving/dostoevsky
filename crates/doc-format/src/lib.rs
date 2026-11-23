//! Reads and writes Dostoevsky files: plain Markdown plus an optional trailer of HTML comments
//! holding alternatives and cuts (docs/FILE-FORMAT.md).

mod anchor;
mod atomic;
pub mod ids;
mod layout;
mod read;
mod spaces;
mod text;
mod write;

use std::collections::HashMap;

use doc_model::snapshot::{DocTree, TreeNode};
use doc_model::{Document, GroupId, Scope, Source};
use serde_json::Value;

pub use atomic::write_atomic;
pub use layout::Encoding;
pub use anchor::Spot;
pub use read::{parse, parse_cuts, parse_page};
pub use write::{serialize, serialize_cuts, serialize_page};

/// Opaque to the app except for `file`, which it renames with the chapter's file.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalCut {
    /// The chapter, relative to the project, with `/` between folders.
    pub file: String,
    pub note: String,
    pub records: Vec<Value>,
}

impl ExternalCut {
    pub fn text(&self) -> &str {
        &self.note
    }

    pub fn has_spot(&self) -> bool {
        self.records.first().and_then(|r| r.get("from")).and_then(|f| f.get("at")).is_some()
    }
}

/// A JSON object, in its original field order.
pub type Fields = serde_json::Map<String, Value>;

/// A `doc` record is written only when the layer is on.
pub const DEFAULT_LAYER: bool = false;

pub struct LoadedDoc {
    pub doc: Document,
    pub layer: bool,
    /// Everything from the file the model doesn't hold; pass it back to [`serialize`].
    pub kept: Preserved,
    /// Never fatal.
    pub warnings: Vec<String>,
    /// Unknown major version: opened as plain text, trailer kept verbatim; treat as read-only.
    pub newer_version: Option<u64>,
}

impl LoadedDoc {
    pub fn serialize(&self) -> String {
        serialize(&self.doc, self.layer, &self.kept)
    }

    pub fn detached(&self) -> &[DetachedGroup] {
        &self.kept.detached
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preserved {
    /// The app removes an entry when the user re-attaches or discards it.
    pub detached: Vec<DetachedGroup>,
    pub unknown_records: Vec<Value>,
    pub invalid_lines: Vec<String>,
    pub doc_fields: Fields,
    pub group_fields: HashMap<GroupId, Fields>,
    /// Extra fields of variant objects (`at` timestamps…), matched to variants by text.
    pub variant_fields: HashMap<GroupId, Vec<(String, Fields)>>,
    /// Unknown fields of `ghost` records (older files), by location.
    pub ghost_fields: HashMap<String, Fields>,
    /// Unknown fields of `stash` records, by note quote.
    pub stash_fields: HashMap<String, Fields>,
    /// Unknown fields of `comment` records, by the comment's text.
    pub comment_fields: HashMap<String, Fields>,
    /// Ids from the file that are valid but not in the derived form ([`ids`]), by group.
    pub id_names: HashMap<GroupId, String>,
    pub foreign_trailer: Option<String>,
    pub encoding: Encoding,
}

impl Preserved {
    fn is_empty(&self) -> bool {
        self.detached.is_empty()
            && self.unknown_records.is_empty()
            && self.invalid_lines.is_empty()
            && self.doc_fields.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetachedGroup {
    pub id: String,
    pub scope: Scope,
    pub active: usize,
    pub variants: Vec<DetachedVariant>,
    pub ghost: bool,
    /// The group's record, then the records nested in it, verbatim.
    pub records: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetachedVariant {
    pub text: String,
    pub by: Source,
}

/// Active variants, no cuts, no comments.
pub fn clean_markdown(doc: &Document) -> String {
    doc.clean_text()
}

/// Also where (chars) the places with other versions are.
pub fn clean_with_notes(doc: &Document) -> (String, Vec<doc_model::AltNote>) {
    doc.clean_text_with_notes()
}

/// Single-variant groups dissolve and an all-cuts version keeps their text, as a save → load does.
pub fn canonical(tree: &DocTree) -> DocTree {
    fn nodes(ns: &[TreeNode], out: &mut Vec<TreeNode>) {
        for n in ns {
            match n {
                TreeNode::Text { text } => out.push(TreeNode::Text { text: text.clone() }),
                TreeNode::Group(g) if g.variants.len() == 1 => nodes(&g.variants[0].content, out),
                TreeNode::Group(g) => {
                    let mut g = g.clone();
                    for v in &mut g.variants {
                        v.content = conv(&v.content);
                        // An all-cuts version has no text in the file: its cuts are saved as its text.
                        // Not a version without the words: that one is its cut.
                        if !v.without && doc_model::snapshot::kept_text(&v.content).is_empty() {
                            v.content = flatten(&v.content);
                        }
                    }
                    out.push(TreeNode::Group(g));
                }
                TreeNode::Cut(c) => {
                    let mut c = c.clone();
                    let mut inner = Vec::new();
                    nodes(&c.content, &mut inner);
                    c.content = inner;
                    out.push(TreeNode::Cut(c));
                }
                n @ (TreeNode::CommentStart(_) | TreeNode::CommentEnd { .. }) => out.push(n.clone()),
            }
        }
    }
    fn flatten(ns: &[TreeNode]) -> Vec<TreeNode> {
        let mut out = Vec::new();
        for n in ns {
            match n {
                TreeNode::Cut(c) => out.extend(c.content.iter().cloned()),
                n => out.push(n.clone()),
            }
        }
        out
    }
    fn conv(ns: &[TreeNode]) -> Vec<TreeNode> {
        let mut out = Vec::new();
        nodes(ns, &mut out);
        // A group with no kept text (a version without the words showing) comes before the cuts
        // and comment markers at its spot: a load plants those after it.
        for i in 1..out.len() {
            if matches!(&out[i], TreeNode::Group(_)) && out[i].kept_len() == 0 {
                let mut j = i;
                while j > 0 && matches!(&out[j - 1], TreeNode::Cut(_) | TreeNode::CommentStart(_) | TreeNode::CommentEnd { .. }) {
                    out.swap(j - 1, j);
                    j -= 1;
                }
            }
        }
        out
    }
    DocTree {
        page: conv(&tree.page),
        notes: tree
            .notes
            .iter()
            .map(|n| doc_model::snapshot::TreeNote {
                content: conv(&n.content),
                ..n.clone()
            })
            .collect(),
    }
}

pub(crate) fn scope_str(s: Scope) -> &'static str {
    match s {
        Scope::Word => "word",
        Scope::Sentence => "sentence",
        Scope::Paragraph => "paragraph",
    }
}

pub(crate) fn parse_scope(s: &str) -> Option<Scope> {
    match s {
        "word" => Some(Scope::Word),
        "sentence" => Some(Scope::Sentence),
        "paragraph" => Some(Scope::Paragraph),
        _ => None,
    }
}

pub(crate) fn source_str(s: Source) -> &'static str {
    match s {
        Source::Me => "me",
        Source::Ai => "ai",
        Source::Import => "import",
    }
}

pub(crate) fn parse_source(s: &str) -> Option<Source> {
    match s {
        "me" => Some(Source::Me),
        "ai" => Some(Source::Ai),
        "import" => Some(Source::Import),
        _ => None,
    }
}

/// The body as written in front of a trailer: trailing newlines collapsed to one.
pub(crate) fn normalized_body(text: &str) -> String {
    let mut b = text.trim_end_matches('\n').to_string();
    b.push('\n');
    b
}
