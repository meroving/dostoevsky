use std::collections::{HashMap, HashSet};
use std::ops::Range;

use doc_model::snapshot::{kept_text, DocTree, TreeComment, TreeCut, TreeGroup, TreeNode, TreeNote, TreeVariant};
use doc_model::{Batch, Document, GroupId, Look, Scope, Source};
use l10n::{tr, trn};
use serde_json::Value;

use crate::anchor::{Found, Haystack, Hint, Spot};
use crate::layout::{self, RawBlock};
use crate::text::{blocks, Block};
use crate::write::ghost_key;
use crate::{
    ids, normalized_body, parse_scope, parse_source, DetachedGroup, DetachedVariant, ExternalCut,
    Fields, LoadedDoc, Preserved, DEFAULT_LAYER,
};

/// Never fails: problems become [`LoadedDoc::warnings`], unplaceable groups get detached.
pub fn parse(file: &str) -> LoadedDoc {
    parse_with(file, &[], None, false).0
}

/// The cuts of no file make the document; the project's files' come back as [`ExternalCut`]s.
pub fn parse_cuts(file: &str) -> (LoadedDoc, Vec<ExternalCut>) {
    parse_with(file, &[], None, true)
}

/// Cuts with no trace or place become notes naming `name`, for [`crate::serialize_page`].
pub fn parse_page(file: &str, name: &str, cuts: &[ExternalCut]) -> LoadedDoc {
    parse_with(file, cuts, Some(name), false).0
}

fn parse_with(file: &str, extra: &[ExternalCut], plant: Option<&str>, split: bool) -> (LoadedDoc, Vec<ExternalCut>) {
    let (doc, external) = parse_inner(file, extra, plant, split);
    (doc, external)
}

fn parse_inner(file: &str, extra: &[ExternalCut], plant: Option<&str>, split: bool) -> (LoadedDoc, Vec<ExternalCut>) {
    let (text, encoding) = layout::decode(file);
    let mut kept = Preserved {
        encoding,
        ..Preserved::default()
    };
    let parts = layout::split(&text);
    if parts.is_none() && extra.is_empty() {
        return (
            LoadedDoc {
                doc: Document::from_text(&text),
                layer: DEFAULT_LAYER,
                kept,
                warnings: Vec::new(),
                newer_version: None,
            },
            Vec::new(),
        );
    }
    let (before_trailer, trailer, raw_blocks) = match &parts {
        Some(sp) => (sp.before, sp.trailer, sp.blocks.clone()),
        None => (text.as_str(), "", Vec::new()),
    };
    let body = normalized_body(before_trailer);
    let mut warnings = Vec::new();

    let newest = raw_blocks
        .iter()
        .filter_map(|b| match b {
            RawBlock::Meta { version, .. } => Some(*version),
            RawBlock::Overflow(_) => None,
        })
        .max();
    if let Some(v) = newest.filter(|&v| v > 1) {
        warnings.push(tr!(
            "this file was saved by a newer version (format v{v}); opened as plain text",
            v = v
        ));
        kept.foreign_trailer = Some(trailer.to_string());
        return (
            LoadedDoc {
                doc: Document::from_text(&body),
                layer: DEFAULT_LAYER,
                kept,
                warnings,
                newer_version: Some(v),
            },
            Vec::new(),
        );
    }

    let mut notes = Vec::new();
    let mut lines = Vec::new();
    for b in raw_blocks {
        match b {
            RawBlock::Overflow(t) => notes.extend(layout::read_notes(&t)),
            RawBlock::Meta { lines: l, .. } => lines.extend(l),
        }
    }
    for e in extra {
        let i = notes.len();
        notes.push(e.note.clone());
        for r in &e.records {
            let mut r = r.clone();
            crate::write::renumber_pub(&mut r, i);
            lines.push(r.to_string());
        }
    }
    let mut external = Vec::new();
    if split {
        (notes, lines, external) = split_external(notes, lines);
    }
    let mut r = Reader {
        body: body.chars().collect(),
        blocks: blocks(&body),
        notes: notes.iter().map(|n| n.chars().collect()).collect(),
        alts: Vec::new(),
        ghosts: Vec::new(),
        child_alts: HashMap::new(),
        child_ghosts: HashMap::new(),
        handled: HashSet::new(),
        kept,
        warnings,
        layer: DEFAULT_LAYER,
        stashes: Vec::new(),
        comments: Vec::new(),
        plant: plant.map(String::from),
    };
    for line in &lines {
        r.record(line);
    }
    let tree = r.build(&notes);
    let doc = match Document::from_snapshot(tree) {
        Ok(d) => d,
        Err(e) => {
            r.warnings.push(tr!(
                "could not rebuild the document ({e}); opened as text",
                e = e
            ));
            Document::from_text(&body)
        }
    };
    (
        LoadedDoc {
            doc,
            layer: r.layer,
            kept: r.kept,
            warnings: r.warnings,
            newer_version: None,
        },
        external,
    )
}

/// Returns (kept notes, kept records, cuts whose stash names a file, each renumbered as note 0).
fn split_external(notes: Vec<String>, lines: Vec<String>) -> (Vec<String>, Vec<String>, Vec<ExternalCut>) {
    let parsed: Vec<Option<Value>> = lines.iter().map(|l| serde_json::from_str::<Value>(l).ok().filter(|v| v.is_object())).collect();
    let kind = |v: &Value| v.get("kind").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut owner: HashMap<usize, String> = HashMap::new();
    let mut stash_of: HashMap<usize, usize> = HashMap::new();
    for (li, v) in parsed.iter().enumerate() {
        let Some(v) = v else { continue };
        if kind(v) != "stash" {
            continue;
        }
        let Some(file) = v.get("from").and_then(|f| f.get("file")).and_then(Value::as_str).filter(|f| !f.is_empty()) else { continue };
        let quote = v.get("quote").and_then(Value::as_str);
        let idx = v.get("note").and_then(Value::as_u64).map(|n| n as usize);
        let starts = |i: usize| quote.is_none_or(|q| notes[i].starts_with(q));
        let note = idx
            .filter(|&i| i < notes.len() && starts(i))
            .or_else(|| (0..notes.len()).find(|&i| starts(i) && !owner.contains_key(&i)))
            .or(idx.filter(|&i| i < notes.len()));
        if let Some(i) = note {
            if let std::collections::hash_map::Entry::Vacant(e) = owner.entry(i) {
                e.insert(file.to_string());
                stash_of.insert(i, li);
            }
        }
    }
    if owner.is_empty() {
        return (notes, lines, Vec::new());
    }
    let mut belongs: HashMap<usize, usize> = HashMap::new();
    let mut group_note: HashMap<String, usize> = HashMap::new();
    for (li, v) in parsed.iter().enumerate() {
        let Some(v) = v else { continue };
        if kind(v) == "alt" || kind(v) == "comment" {
            if let Some(n) = v.get("at").and_then(|a| a.get("note")).and_then(Value::as_u64).map(|n| n as usize) {
                if v.get("parent").is_none() && owner.contains_key(&n) {
                    belongs.insert(li, n);
                    if let Some(id) = v.get("id").and_then(Value::as_str) {
                        group_note.insert(id.to_string(), n);
                    }
                }
            }
        }
    }
    loop {
        let mut grew = false;
        for (li, v) in parsed.iter().enumerate() {
            let Some(v) = v else { continue };
            if belongs.contains_key(&li) {
                continue;
            }
            if let Some(n) = v.get("parent").and_then(Value::as_str).and_then(|p| group_note.get(p)).copied() {
                belongs.insert(li, n);
                if let Some(id) = v.get("id").and_then(Value::as_str) {
                    group_note.insert(id.to_string(), n);
                }
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut external: Vec<ExternalCut> = Vec::new();
    let mut ext_of: HashMap<usize, usize> = HashMap::new();
    for (i, file) in owner.iter().collect::<std::collections::BTreeMap<_, _>>() {
        ext_of.insert(*i, external.len());
        let mut stash = parsed[stash_of[i]].clone().expect("stash");
        crate::write::renumber_pub(&mut stash, 0);
        external.push(ExternalCut { file: file.clone(), note: notes[*i].clone(), records: vec![stash] });
    }
    let mut ordered: Vec<(usize, usize)> = belongs.iter().map(|(&l, &n)| (l, n)).collect();
    ordered.sort();
    for (li, n) in ordered {
        let mut r = parsed[li].clone().expect("record");
        crate::write::renumber_pub(&mut r, 0);
        external[ext_of[&n]].records.push(r);
    }
    let kept_notes: Vec<usize> = (0..notes.len()).filter(|i| !owner.contains_key(i)).collect();
    let new_index: HashMap<usize, usize> = kept_notes.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    let mut out_lines = Vec::new();
    for (li, l) in lines.into_iter().enumerate() {
        if belongs.contains_key(&li) || stash_of.values().any(|&s| s == li) {
            continue;
        }
        match &parsed[li] {
            Some(v) => {
                let mut v = v.clone();
                let n = if kind(&v) == "stash" {
                    v.get("note").and_then(Value::as_u64)
                } else {
                    v.get("at").and_then(|a| a.get("note")).and_then(Value::as_u64)
                };
                if let Some(k) = n.and_then(|n| new_index.get(&(n as usize))) {
                    crate::write::renumber_pub(&mut v, *k);
                }
                out_lines.push(v.to_string());
            }
            None => out_lines.push(l),
        }
    }
    let out_notes = kept_notes.into_iter().map(|i| notes[i].clone()).collect();
    (out_notes, out_lines, external)
}

#[derive(Clone, Debug, Default)]
struct At {
    block: Option<usize>,
    note: Option<usize>,
    offset: Option<usize>,
    length: Option<usize>,
    quote: Option<String>,
    before: Option<String>,
    after: Option<String>,
}

impl At {
    fn read(v: Option<&Value>) -> At {
        let num = |k: &str| {
            v.and_then(|a| a.get(k))
                .and_then(Value::as_u64)
                .map(|n| n as usize)
        };
        let s = |k: &str| {
            v.and_then(|a| a.get(k))
                .and_then(Value::as_str)
                .map(String::from)
        };
        At {
            block: num("block"),
            note: num("note"),
            offset: num("offset"),
            length: num("length"),
            quote: s("quote"),
            before: s("before"),
            after: s("after"),
        }
    }

    fn hint(&self, block: usize) -> Hint {
        Hint {
            block,
            offset: self.offset,
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }
}

struct Alt {
    raw: Value,
    id: String,
    gid: GroupId,
    scope: Scope,
    at: At,
    active: usize,
    /// Text, author, and whether it is a version without the words (`"cut":true`).
    variants: Vec<(String, Source, bool)>,
    parent: Option<String>,
    pvariant: Option<usize>,
    ghost: bool,
    detached: bool,
    extra: Fields,
    variant_extra: Vec<(String, Fields)>,
}

struct Ghost {
    raw: Value,
    at: At,
    parent: Option<String>,
    pvariant: Option<usize>,
}

struct Cand {
    alt: usize,
    range: Range<usize>,
    /// The active one is the body's text after a fuzzy match.
    variants: Vec<String>,
    /// Groups and ghosts inside this one's range: of its active variant, in relative offsets.
    inner: Vec<Cand>,
    inner_ghosts: Vec<Range<usize>>,
}

struct Space<'a> {
    text: &'a [char],
    blocks: &'a [Block],
    global: bool,
    /// Ghosts here become cuts (in the body and its variants); in notes they become text.
    cuts: bool,
}

struct Reader {
    body: Vec<char>,
    blocks: Vec<Block>,
    notes: Vec<Vec<char>>,
    alts: Vec<Alt>,
    ghosts: Vec<Ghost>,
    child_alts: HashMap<String, Vec<usize>>,
    child_ghosts: HashMap<String, Vec<usize>>,
    handled: HashSet<usize>,
    kept: Preserved,
    warnings: Vec<String>,
    layer: bool,
    stashes: Vec<StashRecord>,
    comments: Vec<CommentRecord>,
    /// Set when a chapter is read with its project cuts: stash records naming it are its own.
    plant: Option<String>,
}

struct CommentRecord {
    at: At,
    comment: TreeComment,
    parent: Option<String>,
    pvariant: Option<usize>,
    raw: Value,
}

struct StashRecord {
    note: Option<usize>,
    quote: Option<String>,
    origin: Option<String>,
    file: Option<String>,
    paragraph: bool,
    at: Option<Spot>,
    /// Its trace: `Some(None)` follows the setting.
    trace: Option<Option<Look>>,
    batch: Option<Batch>,
}

#[derive(Clone, Default)]
struct NoteMeta {
    origin: Option<String>,
    paragraph: bool,
    source: Option<String>,
    at: Option<Spot>,
    trace: Option<Option<Look>>,
    batch: Option<Batch>,
}

const ALT_FIELDS: &[&str] = &[
    "kind", "id", "scope", "at", "active", "variants", "parent", "pvariant", "ghost", "detached",
];
#[derive(Clone, Copy)]
enum Detached {
    Orphan,
    Gone,
    Overlaps,
}

const GHOST_FIELDS: &[&str] = &["kind", "at", "parent", "pvariant"];
const COMMENT_FIELDS: &[&str] = &["kind", "at", "text", "created", "author", "parent", "pvariant"];
const STASH_FIELDS: &[&str] = &["kind", "note", "quote", "from", "paragraph", "trace", "batch", "version"];

fn unknown_fields(o: &Fields, known: &[&str]) -> Fields {
    o.iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

impl Reader {
    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    fn record(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v @ Value::Object(_)) if v.get("kind").is_some_and(Value::is_string) => v,
            _ => {
                self.warn(tr!("kept a meta line that isn't a record: {line}", line = line));
                self.kept.invalid_lines.push(line.to_string());
                return;
            }
        };
        let o = v.as_object().expect("object").clone();
        match o["kind"].as_str().unwrap_or_default() {
            "doc" => {
                if let Some(l) = o.get("layer").and_then(Value::as_bool) {
                    self.layer = l;
                }
                let extra = unknown_fields(&o, &["kind", "layer"]);
                self.kept.doc_fields.extend(extra);
            }
            "alt" => match self.read_alt(&o, v.clone()) {
                Some(a) => self.alts.push(a),
                None => {
                    self.warn(tr!("kept a malformed alt record as is"));
                    self.kept.unknown_records.push(v);
                }
            },
            "ghost" => {
                let g = Ghost {
                    at: At::read(o.get("at")),
                    parent: o.get("parent").and_then(Value::as_str).map(String::from),
                    pvariant: o
                        .get("pvariant")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize),
                    raw: v.clone(),
                };
                if g.at.length.is_none() {
                    self.warn(tr!("dropped an old faded passage without a length"));
                    return;
                }
                let extra = unknown_fields(&o, GHOST_FIELDS);
                if !extra.is_empty() {
                    self.kept.ghost_fields.insert(ghost_key(&o), extra);
                }
                self.ghosts.push(g);
            }
            "comment" => {
                let s = |k: &str| o.get(k).and_then(Value::as_str).map(String::from);
                let comment = TreeComment {
                    id: self.comments.len() as u64 + 1,
                    text: s("text").unwrap_or_default(),
                    created: s("created"),
                    author: s("author"),
                };
                let extra = unknown_fields(&o, COMMENT_FIELDS);
                if !extra.is_empty() {
                    self.kept.comment_fields.insert(comment.text.clone(), extra);
                }
                self.comments.push(CommentRecord {
                    at: At::read(o.get("at")),
                    comment,
                    parent: s("parent"),
                    pvariant: o.get("pvariant").and_then(Value::as_u64).map(|n| n as usize),
                    raw: v.clone(),
                });
            }
            "stash" => {
                let quote = o.get("quote").and_then(Value::as_str).map(String::from);
                let extra = unknown_fields(&o, STASH_FIELDS);
                if let (false, Some(q)) = (extra.is_empty(), &quote) {
                    self.kept.stash_fields.insert(q.clone(), extra);
                }
                let from = o.get("from");
                self.stashes.push(StashRecord {
                    at: from.and_then(|f| f.get("at")).and_then(Spot::read),
                    trace: match o.get("trace") {
                        Some(Value::Bool(true)) => Some(None),
                        Some(Value::String(t)) if t == "mark" => Some(Some(Look::Mark)),
                        Some(Value::String(t)) if t == "text" => Some(Some(Look::Text)),
                        Some(Value::String(_)) => Some(None),
                        _ => None,
                    },
                    batch: o.get("batch").and_then(|b| {
                        Some(Batch {
                            id: b.get("id")?.as_str()?.to_string(),
                            name: b.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                        })
                    }),
                    note: o.get("note").and_then(Value::as_u64).map(|n| n as usize),
                    quote,
                    origin: o
                        .get("from")
                        .and_then(|f| f.get("quote"))
                        .and_then(Value::as_str)
                        .map(String::from),
                    file: o
                        .get("from")
                        .and_then(|f| f.get("file"))
                        .and_then(Value::as_str)
                        .filter(|f| !f.is_empty())
                        .map(String::from),
                    paragraph: o.get("paragraph").and_then(Value::as_bool).unwrap_or(false),
                });
            }
            _ => self.kept.unknown_records.push(v),
        }
    }

    fn read_alt(&mut self, o: &Fields, raw: Value) -> Option<Alt> {
        let id = o.get("id")?.as_str()?.to_string();
        let scope = parse_scope(o.get("scope")?.as_str()?)?;
        let mut active = o.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut variants = Vec::new();
        let mut variant_fields = Vec::new();
        for (i, v) in o.get("variants")?.as_array()?.iter().enumerate() {
            let (text, by, without, extra) = match v {
                Value::String(s) => (s.clone(), Source::Me, false, Fields::new()),
                Value::Object(vo) => (
                    vo.get("t")?.as_str()?.to_string(),
                    vo.get("by")
                        .and_then(Value::as_str)
                        .and_then(parse_source)
                        .unwrap_or(Source::Me),
                    vo.get("cut").and_then(Value::as_bool).unwrap_or(false),
                    unknown_fields(vo, &["t", "by", "cut"]),
                ),
                _ => return None,
            };
            // A version without the words has no text: its cut is planted in it.
            if text.is_empty() && !(without && i > 0) {
                // The model has no empty variants; an empty active one can't be anchored.
                if i == active {
                    return None;
                }
                if i < active {
                    active -= 1;
                }
                continue;
            }
            if !extra.is_empty() {
                variant_fields.push((text.clone(), extra));
            }
            variants.push((text, by, without && i > 0));
        }
        if variants.is_empty() {
            return None;
        }
        if active >= variants.len() {
            self.warn(tr!(
                "group {id}: active variant out of range, showing the first",
                id = id
            ));
            active = 0;
        }
        let a = Alt {
            id,
            gid: GroupId(0), // assigned in `build`
            scope,
            at: At::read(o.get("at")),
            active,
            variants,
            parent: o.get("parent").and_then(Value::as_str).map(String::from),
            pvariant: o
                .get("pvariant")
                .and_then(Value::as_u64)
                .map(|n| n as usize),
            ghost: o.get("ghost").and_then(Value::as_bool).unwrap_or(false),
            detached: o.get("detached").and_then(Value::as_bool).unwrap_or(false),
            extra: unknown_fields(o, ALT_FIELDS),
            variant_extra: variant_fields,
            raw,
        };
        Some(a)
    }

    /// The id its string encodes when that is unique, otherwise a fresh one above all of them.
    fn assign_ids(&mut self) {
        let decoded: Vec<Option<GroupId>> = self.alts.iter().map(|a| ids::decode(&a.id)).collect();
        let mut next = decoded.iter().flatten().map(|g| g.0).max().unwrap_or(0);
        let mut seen = HashSet::new();
        let mut names = HashSet::new();
        for (a, d) in self.alts.iter_mut().zip(decoded) {
            match d {
                Some(g) if seen.insert(g) => a.gid = g,
                _ => {
                    next += 1;
                    a.gid = GroupId(next);
                    seen.insert(a.gid);
                    if d.is_none() && ids::is_valid(&a.id) && names.insert(a.id.clone()) {
                        self.kept.id_names.insert(a.gid, a.id.clone());
                    }
                }
            }
        }
    }

    fn build(&mut self, notes: &[String]) -> DocTree {
        self.assign_ids();
        for (i, a) in self.alts.iter().enumerate() {
            if let (false, Some(p)) = (a.detached, &a.parent) {
                self.child_alts.entry(p.clone()).or_default().push(i);
            }
        }
        for (i, g) in self.ghosts.iter().enumerate() {
            if let Some(p) = &g.parent {
                self.child_ghosts.entry(p.clone()).or_default().push(i);
            }
        }
        for i in 0..self.alts.len() {
            if self.alts[i].detached {
                self.detach(i, None);
            }
        }

        let top_alts = |r: &Reader, note: Option<usize>| -> Vec<usize> {
            (0..r.alts.len())
                .filter(|&i| {
                    let a = &r.alts[i];
                    !a.detached && a.parent.is_none() && a.at.note == note
                })
                .collect()
        };
        let top_ghosts = |r: &Reader, note: Option<usize>| -> Vec<usize> {
            (0..r.ghosts.len())
                .filter(|&i| r.ghosts[i].parent.is_none() && r.ghosts[i].at.note == note)
                .collect()
        };

        let body = std::mem::take(&mut self.body);
        let blocks = std::mem::take(&mut self.blocks);
        let sp = Space {
            text: &body,
            blocks: &blocks,
            global: true,
            cuts: true,
        };
        let (alts, ghosts) = (top_alts(self, None), top_ghosts(self, None));
        let cands = self.place_alts(&sp, &alts, false);
        let ghost_ranges = self.place_ghosts(&sp, &ghosts, false);
        let mut page = self.container(&sp, cands, ghost_ranges);

        let metas = self.resolve_stashes(notes);
        let note_chars = std::mem::take(&mut self.notes);
        let mut tree_notes = Vec::new();
        for (i, chars) in note_chars.iter().enumerate() {
            let one = [Block {
                start: 0,
                end: chars.len(),
            }];
            let sp = Space {
                text: chars,
                blocks: &one,
                global: false,
                cuts: false,
            };
            let alts = top_alts(self, Some(i));
            let cands = self.place_alts(&sp, &alts, true);
            let m = &metas[i];
            tree_notes.push(TreeNote {
                content: self.container(&sp, cands, Vec::new()),
                origin: m.origin.clone(),
                paragraph: m.paragraph,
                source: m.source.clone(),
                lost: m.trace.is_some(),
                look: m.trace.flatten(),
                batch: m.batch.clone(),
            });
        }

        // Before cuts take their notes' comments back to the page.
        self.plant_comments(&mut page, &body, &blocks, &mut tree_notes);

        let mut out_notes = Vec::new();
        let mut lost = 0;
        for (i, mut n) in tree_notes.into_iter().enumerate() {
            let m = &metas[i];
            let mine = m.source.is_none() || m.source == self.plant;
            let (Some(spot), true, Some(trace)) = (&m.at, mine, m.trace) else {
                out_notes.push(n);
                continue;
            };
            let text = doc_model::snapshot::content_text(&n.content);
            let planted = !text.is_empty() && self.plant_cut(&mut page, &body, &blocks, spot, &n, trace);
            if planted {
                continue;
            }
            if !text.is_empty() {
                lost += 1;
            }
            n.lost = true;
            n.look = trace;
            out_notes.push(n);
        }
        if lost > 0 {
            self.warn(trn!(
                "a cut lost its place: it is kept with no trace",
                "{n} cuts lost their place: they are kept with no trace",
                lost
            ));
        }

        // Records that never found a container: missing parent, bad note index, cycles.
        for i in 0..self.alts.len() {
            self.detach(i, Some(Detached::Orphan));
        }
        let orphans = self.child_ghosts.values().map(Vec::len).sum::<usize>();
        if orphans > 0 {
            self.warn(trn!(
                "dropped {n} old faded passage inside unknown groups",
                "dropped {n} old faded passages inside unknown groups",
                orphans
            ));
        }
        DocTree {
            page,
            notes: out_notes,
        }
    }

    /// A comment whose text is gone is dropped with a warning; in a detached group it stays there.
    fn plant_comments(&mut self, page: &mut Vec<TreeNode>, body: &[char], blocks: &[Block], notes: &mut [TreeNote]) {
        let mut lost = 0;
        for r in std::mem::take(&mut self.comments) {
            let quote: Vec<char> = r.at.quote.as_deref().unwrap_or_default().chars().collect();
            let find = |text: &[char], blocks: &[Block], global: bool, block: usize| -> Option<std::ops::Range<usize>> {
                let h = Haystack { text, blocks, global };
                if quote.is_empty() {
                    return None;
                }
                h.find_alt(&r.at.hint(block), &quote, false).map(|f| f.range())
            };
            let in_local = |content: &mut Vec<TreeNode>| -> bool {
                let chars: Vec<char> = kept_text(content).chars().collect();
                let one = [Block { start: 0, end: chars.len() }];
                match find(&chars, &one, false, 0) {
                    Some(range) => {
                        plant_comment(content, range.start, range.end, r.comment.clone());
                        true
                    }
                    None => false,
                }
            };
            let placed = if let (Some(parent), Some(k)) = (&r.parent, r.pvariant) {
                let detached = self.kept.detached.iter_mut().find(|d| d.id == *parent);
                match (detached, self.alts.iter().find(|a| a.id == *parent)) {
                    (Some(d), _) => {
                        d.records.push(r.raw.clone());
                        continue;
                    }
                    (None, Some(a)) => {
                        let gid = a.gid;
                        let mut in_group = |content: &mut Vec<TreeNode>| in_local(content);
                        plant_in_group(page, gid, k, &mut in_group) || notes.iter_mut().any(|n| plant_in_group(&mut n.content, gid, k, &mut in_group))
                    }
                    (None, None) => false,
                }
            } else if let Some(i) = r.at.note {
                notes.get_mut(i).is_some_and(|n| in_local(&mut n.content))
            } else {
                match find(body, blocks, true, r.at.block.unwrap_or(0)) {
                    Some(range) => {
                        plant_comment(page, range.start, range.end, r.comment.clone());
                        true
                    }
                    None => false,
                }
            };
            if !placed {
                lost += 1;
            }
        }
        if lost > 0 {
            self.warn(trn!("dropped {n} comment whose text is gone", "dropped {n} comments whose text is gone", lost));
        }
    }

    /// Returns whether the spot was found.
    #[allow(clippy::too_many_arguments)]
    fn plant_cut(&mut self, page: &mut Vec<TreeNode>, body: &[char], blocks: &[Block], spot: &Spot, n: &TreeNote, look: Option<Look>) -> bool {
        let make = || {
            TreeNode::Cut(TreeCut {
                content: n.content.clone(),
                shown: look != Some(Look::Mark),
                look,
                paragraph: n.paragraph,
                batch: n.batch.clone(),
            })
        };
        if let (Some(parent), Some(k)) = (&spot.parent, spot.pvariant) {
            let Some(gid) = self.alts.iter().find(|a| a.id == *parent && !a.detached).map(|a| a.gid) else {
                return false;
            };
            return plant_in_group(page, gid, k, &mut |content: &mut Vec<TreeNode>| {
                let chars: Vec<char> = kept_text(content).chars().collect();
                let one = [Block { start: 0, end: chars.len() }];
                let h = Haystack { text: &chars, blocks: &one, global: false };
                let local = Spot { block: 0, ..spot.clone() };
                let Some(p) = h.find_point(&local) else { return false };
                plant(content, p, make());
                true
            });
        }
        let h = Haystack { text: body, blocks, global: true };
        let found = if spot.is_gap() { h.find_gap(spot) } else { h.find_point(spot) };
        let Some(p) = found else { return false };
        plant(page, p, make());
        true
    }

    fn resolve_stashes(&mut self, notes: &[String]) -> Vec<NoteMeta> {
        let mut metas = vec![NoteMeta::default(); notes.len()];
        let mut taken = HashSet::new();
        for StashRecord {
            note: idx,
            quote,
            origin: from,
            file,
            paragraph,
            at,
            trace,
            batch,
        } in std::mem::take(&mut self.stashes)
        {
            let starts = |i: usize| quote.as_deref().is_none_or(|q| notes[i].starts_with(q));
            let note = idx
                .filter(|&i| i < notes.len() && starts(i))
                .or_else(|| (0..notes.len()).find(|&i| starts(i) && !taken.contains(&i)))
                .or(idx.filter(|&i| i < notes.len()));
            let Some(i) = note else {
                self.warn(tr!("dropped a stash record for a note that doesn't exist"));
                continue;
            };
            taken.insert(i);
            metas[i] = NoteMeta {
                paragraph: paragraph && (from.is_some() || at.is_some()),
                origin: from,
                source: file,
                at,
                trace,
                batch,
            };
        }
        metas
    }

    /// Records that can't be placed are detached.
    fn place_alts(&mut self, sp: &Space, alts: &[usize], single_block: bool) -> Vec<Cand> {
        let h = Haystack {
            text: sp.text,
            blocks: sp.blocks,
            global: sp.global,
        };
        let mut out = Vec::new();
        for &i in alts {
            let a = &self.alts[i];
            let quote: Vec<char> = a.variants[a.active].0.chars().collect();
            let block = if single_block {
                0
            } else {
                a.at.block.unwrap_or(0)
            };
            let paragraph = a.scope == Scope::Paragraph && a.at.offset.is_none();
            // A version without the words showing: the group is a point between its neighbours.
            let found = if quote.is_empty() {
                let hint = a.at.hint(block);
                let spot = Spot {
                    block: hint.block,
                    offset: Some(hint.offset.unwrap_or(0)),
                    before: hint.before.unwrap_or_default(),
                    after: hint.after.unwrap_or_default(),
                    parent: None,
                    pvariant: None,
                };
                h.find_point(&spot).map(|p| Found::Exact(p..p))
            } else {
                h.find_alt(&a.at.hint(block), &quote, paragraph)
            };
            let mut variants: Vec<String> = a.variants.iter().map(|v| v.0.clone()).collect();
            match found {
                Some(f) => {
                    let range = f.range();
                    if let Found::Fuzzy(_) = f {
                        variants[a.active] = sp.text[range.clone()].iter().collect();
                        let id = a.id.clone();
                        self.warn(tr!(
                            "group {id}: the text changed outside the app; kept the new text",
                            id = id
                        ));
                    }
                    out.push(Cand {
                        alt: i,
                        range,
                        variants,
                        inner: Vec::new(),
                        inner_ghosts: Vec::new(),
                    });
                }
                None => self.detach(i, Some(Detached::Gone)),
            }
        }
        out
    }

    fn place_ghosts(
        &mut self,
        sp: &Space,
        ghosts: &[usize],
        single_block: bool,
    ) -> Vec<Range<usize>> {
        let h = Haystack {
            text: sp.text,
            blocks: sp.blocks,
            global: sp.global,
        };
        let mut out = Vec::new();
        let mut lost = 0;
        for &i in ghosts {
            let g = &self.ghosts[i];
            let block = if single_block {
                0
            } else {
                g.at.block.unwrap_or(0)
            };
            let quote: Option<Vec<char>> = g.at.quote.as_ref().map(|q| q.chars().collect());
            match h.find_ghost(
                &g.at.hint(block),
                quote.as_deref(),
                g.at.length.unwrap_or(0),
            ) {
                Some(r) => out.push(r),
                None => lost += 1,
            }
        }
        if lost > 0 {
            self.warn(trn!("dropped {n} old faded passage whose text is gone", "dropped {n} old faded passages whose text is gone", lost));
        }
        out
    }

    /// A ghost over a whole group holds it; over part, it is cut inside the active variant.
    fn container(
        &mut self,
        sp: &Space,
        mut cands: Vec<Cand>,
        ghosts: Vec<Range<usize>>,
    ) -> Vec<TreeNode> {
        // Partly overlapping or identical spans: the later record is detached.
        cands.sort_by_key(|c| c.alt);
        let mut accepted: Vec<Cand> = Vec::new();
        for c in cands {
            let clash = accepted.iter().any(|a| {
                let (x, y) = (&a.range, &c.range);
                let disjoint = x.end <= y.start || y.end <= x.start;
                let nested = (x.start <= y.start && y.end <= x.end)
                    || (y.start <= x.start && x.end <= y.end);
                !disjoint && (!nested || x == y)
            });
            if clash {
                self.detach(c.alt, Some(Detached::Overlaps));
            } else {
                accepted.push(c);
            }
        }
        // A span inside another one belongs to that group's active variant. A group showing
        // nothing (a version without the words) at another's edge is beside it, not in it.
        accepted.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then(b.range.is_empty().cmp(&a.range.is_empty()))
                .then(b.range.end.cmp(&a.range.end))
        });
        let mut parent: Vec<Option<usize>> = vec![None; accepted.len()];
        let mut stack: Vec<usize> = Vec::new();
        for i in 0..accepted.len() {
            while stack
                .last()
                .is_some_and(|&t| accepted[t].range.end <= accepted[i].range.start)
            {
                stack.pop();
            }
            parent[i] = stack.last().copied();
            stack.push(i);
        }
        let mut slots: Vec<Option<Cand>> = accepted.into_iter().map(Some).collect();
        for i in (0..slots.len()).rev() {
            if let Some(p) = parent[i] {
                let mut c = slots[i].take().expect("child");
                let base = slots[p].as_ref().expect("parent").range.start;
                c.range = c.range.start - base..c.range.end - base;
                slots[p].as_mut().expect("parent").inner.push(c);
            }
        }
        let mut roots: Vec<Cand> = slots.into_iter().flatten().collect();

        let mut own: Vec<Range<usize>> = Vec::new();
        if sp.cuts {
            for g in ghosts {
                let mut rest = vec![g];
                for r in &mut roots {
                    let mut next = Vec::new();
                    for piece in rest {
                        let (s, e) = (piece.start.max(r.range.start), piece.end.min(r.range.end));
                        let whole = piece.start <= r.range.start && r.range.end <= piece.end;
                        if s < e && !whole {
                            r.inner_ghosts.push(s - r.range.start..e - r.range.start);
                            if piece.start < s {
                                next.push(piece.start..s);
                            }
                            if e < piece.end {
                                next.push(e..piece.end);
                            }
                        } else {
                            next.push(piece);
                        }
                    }
                    rest = next;
                }
                own.extend(rest);
            }
            own.extend(roots.iter().filter(|r| self.alts[r.alt].ghost).map(|r| r.range.clone()));
        }
        own.retain(|r| !r.is_empty());
        own.sort_by_key(|r| r.start);
        let mut cuts: Vec<(Range<usize>, bool)> = Vec::new();
        for r in own {
            match cuts.last_mut() {
                Some((last, _)) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => cuts.push((r, false)),
            }
        }
        if sp.global {
            // Whole paragraphs take their blank line along, unless that runs into a group.
            for i in 0..cuts.len() {
                let Some(span) = doc_model::paragraph_span(sp.text, cuts[i].0.clone()) else { continue };
                let span = cuts[i].0.start.min(span.start)..cuts[i].0.end.max(span.end);
                let clashes = roots.iter().any(|r| r.range.start < span.end && span.start < r.range.end && !(span.start <= r.range.start && r.range.end <= span.end))
                    || cuts.iter().enumerate().any(|(j, (o, _))| j != i && o.start < span.end && span.start < o.end);
                if !clashes {
                    cuts[i] = (span, true);
                }
            }
        }

        let text = |range: Range<usize>| TreeNode::Text { text: sp.text[range].iter().collect() };
        let mut out = Vec::new();
        let mut pos = 0;
        let mut roots = roots.into_iter().peekable();
        for (range, paragraph) in cuts {
            while let Some(r) = roots.next_if(|r| r.range.start < range.start) {
                if pos < r.range.start {
                    out.push(text(pos..r.range.start));
                }
                pos = r.range.end;
                out.push(self.group_node(r, sp.cuts));
            }
            if pos < range.start {
                out.push(text(pos..range.start));
            }
            let mut content = Vec::new();
            let mut cpos = range.start;
            while let Some(r) = roots.next_if(|r| r.range.end <= range.end) {
                if cpos < r.range.start {
                    content.push(text(cpos..r.range.start));
                }
                cpos = r.range.end;
                content.push(self.group_node(r, sp.cuts));
            }
            if cpos < range.end {
                content.push(text(cpos..range.end));
            }
            pos = range.end;
            out.push(TreeNode::Cut(TreeCut { content, shown: true, look: None, paragraph, batch: None }));
        }
        for r in roots {
            if pos < r.range.start {
                out.push(text(pos..r.range.start));
            }
            pos = r.range.end;
            out.push(self.group_node(r, sp.cuts));
        }
        if pos < sp.text.len() {
            out.push(text(pos..sp.text.len()));
        }
        out
    }

    fn group_node(&mut self, mut c: Cand, cuts: bool) -> TreeNode {
        self.handled.insert(c.alt);
        let a = &mut self.alts[c.alt];
        let (id, gid, active) = (a.id.clone(), a.gid, a.active);
        let scope = a.scope;
        let by: Vec<Source> = a.variants.iter().map(|v| v.1).collect();
        let without: Vec<bool> = a.variants.iter().map(|v| v.2).collect();
        if !a.extra.is_empty() {
            self.kept
                .group_fields
                .insert(gid, std::mem::take(&mut a.extra));
        }
        if !a.variant_extra.is_empty() {
            self.kept
                .variant_fields
                .insert(gid, std::mem::take(&mut a.variant_extra));
        }
        let kids = self.child_alts.remove(&id).unwrap_or_default();
        let kid_ghosts = self.child_ghosts.remove(&id).unwrap_or_default();
        let mut variants = Vec::with_capacity(c.variants.len());
        for (k, text) in c.variants.iter().enumerate() {
            let chars: Vec<char> = text.chars().collect();
            let one = [Block {
                start: 0,
                end: chars.len(),
            }];
            let sp = Space {
                text: &chars,
                blocks: &one,
                global: false,
                cuts,
            };
            let mine: Vec<usize> = kids
                .iter()
                .copied()
                .filter(|&i| self.alts[i].pvariant == Some(k))
                .collect();
            let mut cands = self.place_alts(&sp, &mine, true);
            let mine: Vec<usize> = kid_ghosts
                .iter()
                .copied()
                .filter(|&i| self.ghosts[i].pvariant == Some(k))
                .collect();
            let mut ghosts = self.place_ghosts(&sp, &mine, true);
            if k == active {
                cands.append(&mut c.inner);
                ghosts.append(&mut c.inner_ghosts);
            }
            variants.push(TreeVariant {
                by: by[k],
                content: self.container(&sp, cands, ghosts),
                without: without[k],
            });
        }
        // Children naming a missing variant stay unhandled: `build` detaches them.
        TreeNode::Group(TreeGroup {
            id: gid,
            scope,
            active,
            variants,
        })
    }

    fn detach(&mut self, i: usize, reason: Option<Detached>) {
        if !self.handled.insert(i) {
            return;
        }
        let a = &self.alts[i];
        let mut first = a.raw.clone();
        if let Some(o) = first.as_object_mut() {
            o.insert("detached".into(), Value::Bool(true));
        }
        let mut records = vec![first];
        let mut stack = vec![a.id.clone()];
        let d = DetachedGroup {
            id: a.id.clone(),
            scope: a.scope,
            active: a.active,
            variants: a
                .variants
                .iter()
                .map(|(text, by, _)| DetachedVariant {
                    text: text.clone(),
                    by: *by,
                })
                .collect(),
            ghost: a.ghost,
            records: Vec::new(),
        };
        if let Some(r) = reason {
            let id = &a.id;
            self.warnings.push(match r {
                Detached::Orphan => tr!(
                    "group {id} was detached: it refers to a group or note that doesn't exist",
                    id = id
                ),
                Detached::Gone => tr!(
                    "group {id} was detached: its text is no longer in the document",
                    id = id
                ),
                Detached::Overlaps => tr!("group {id} was detached: it overlaps another group", id = id),
            });
        }
        while let Some(id) = stack.pop() {
            for k in self.child_alts.remove(&id).unwrap_or_default() {
                if self.handled.insert(k) {
                    records.push(self.alts[k].raw.clone());
                    stack.push(self.alts[k].id.clone());
                }
            }
            for g in self.child_ghosts.remove(&id).unwrap_or_default() {
                records.push(self.ghosts[g].raw.clone());
            }
        }
        self.kept.detached.push(DetachedGroup { records, ..d });
    }
}

/// At kept position `p` (cuts take no room), after any cut already there.
fn plant(nodes: &mut Vec<TreeNode>, p: usize, cut: TreeNode) {
    let mut o = 0;
    let mut i = 0;
    while i < nodes.len() {
        let l = nodes[i].kept_len();
        if o == p && l > 0 {
            nodes.insert(i, cut);
            return;
        }
        if o < p && p < o + l {
            match &mut nodes[i] {
                TreeNode::Text { text } => {
                    let b = text.char_indices().nth(p - o).map_or(text.len(), |(k, _)| k);
                    let rest = TreeNode::Text { text: text[b..].to_string() };
                    text.truncate(b);
                    nodes.insert(i + 1, cut);
                    nodes.insert(i + 2, rest);
                }
                TreeNode::Group(g) => {
                    let a = g.active;
                    plant(&mut g.variants[a].content, p - o, cut);
                }
                _ => unreachable!("a cut or a comment's marker takes no room in the kept text"),
            }
            return;
        }
        o += l;
        i += 1;
    }
    nodes.push(cut);
}

fn plant_in_group(nodes: &mut [TreeNode], id: GroupId, k: usize, f: &mut dyn FnMut(&mut Vec<TreeNode>) -> bool) -> bool {
    for n in nodes {
        match n {
            TreeNode::Group(g) if g.id == id => return g.variants.get_mut(k).is_some_and(|v| f(&mut v.content)),
            TreeNode::Group(g) => {
                for v in &mut g.variants {
                    if plant_in_group(&mut v.content, id, k, f) {
                        return true;
                    }
                }
            }
            TreeNode::Cut(c) => {
                if plant_in_group(&mut c.content, id, k, f) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// The end first, then the start, each after the cuts and markers already at its position.
fn plant_comment(nodes: &mut Vec<TreeNode>, s: usize, e: usize, c: TreeComment) {
    let id = c.id;
    plant(nodes, e, TreeNode::CommentEnd { id });
    plant(nodes, s, TreeNode::CommentStart(c));
}
