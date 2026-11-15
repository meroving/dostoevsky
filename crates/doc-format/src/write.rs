use std::collections::{HashMap, HashSet};

use doc_model::snapshot::{content_text, kept_text, TreeComment, TreeCut, TreeGroup, TreeNode, TreeNote};
use doc_model::{Document, GroupId, Look, Scope, Source};
use serde_json::{json, Value};

use crate::anchor::Spot;
use crate::layout::{self, OVERFLOW_HEADER};
use crate::text::{after, before, block_of, blocks, Block};
use crate::{canonical, ids, normalized_body, scope_str, source_str, ExternalCut, Fields, Preserved, DEFAULT_LAYER};

/// With no groups, cuts or notes (and nothing kept) this is the text only, byte for byte; with a
/// trailer, the body's trailing newlines collapse to one.
pub fn serialize(doc: &Document, layer: bool, kept: &Preserved) -> String {
    let mut out = write(doc, layer, kept);
    let notes = std::mem::take(&mut out.notes);
    out.assemble(notes, kept)
}

/// The file keeps its own scraps; its cuts come back as [`ExternalCut`]s for the project's cuts
/// file, with `from.file` = `file`.
pub fn serialize_page(doc: &Document, layer: bool, kept: &Preserved, file: &str) -> (String, Vec<ExternalCut>) {
    let mut out = write(doc, layer, kept);
    let (theirs, mine): (Vec<NoteOut>, Vec<NoteOut>) = std::mem::take(&mut out.notes).into_iter().partition(|n| n.project);
    let external = theirs.into_iter().map(|n| n.external(file)).collect();
    (out.assemble(mine, kept), external)
}

pub fn serialize_cuts(doc: &Document, layer: bool, kept: &Preserved, external: &[ExternalCut]) -> String {
    let mut out = write(doc, layer, kept);
    let mut notes = std::mem::take(&mut out.notes);
    notes.extend(external.iter().map(NoteOut::from_external));
    out.assemble(notes, kept)
}

/// Its records numbered as note 0, renumbered when the file is put together.
#[derive(Clone, Debug)]
struct NoteOut {
    text: String,
    /// The `stash` record first, if any.
    records: Vec<(Key, Value)>,
    /// Kept in the project's cuts file when the document is a project's chapter.
    project: bool,
}

impl NoteOut {
    fn external(self, file: &str) -> ExternalCut {
        let mut records: Vec<Value> = self.records.into_iter().map(|(_, v)| v).collect();
        if let Some(Value::Object(o)) = records.first_mut() {
            if o.get("kind") == Some(&json!("stash")) {
                set_file(o, file);
            }
        }
        ExternalCut { file: file.to_string(), note: self.text, records }
    }

    fn from_external(e: &ExternalCut) -> NoteOut {
        let mut records: Vec<(Key, Value)> = Vec::new();
        for (i, r) in e.records.iter().enumerate() {
            let mut r = r.clone();
            if let Value::Object(o) = &mut r {
                if o.get("kind") == Some(&json!("stash")) {
                    set_file(o, &e.file);
                    records.push((Key { section: 4, index: 0, offset: 0, seq: i }, r));
                    continue;
                }
            }
            records.push((Key { seq: i, ..kept_key(&r, 0) }, r));
        }
        NoteOut { text: e.note.clone(), records, project: true }
    }
}

/// Puts `from.file` in front of `from`'s other fields.
fn set_file(o: &mut Fields, file: &str) {
    let mut from = Fields::new();
    from.insert("file".into(), json!(file));
    if let Some(Value::Object(old)) = o.get("from") {
        for (k, v) in old {
            if k != "file" && k != "block" {
                from.insert(k.clone(), v.clone());
            }
        }
    }
    o.insert("from".into(), Value::Object(from));
}

struct Out {
    kept: String,
    body: String,
    records: Vec<(Key, Value)>,
    /// From the page itself, not the doc record or kept ones.
    has_records: bool,
    notes: Vec<NoteOut>,
}

fn write(doc: &Document, layer: bool, kept: &Preserved) -> Out {
    let tree = canonical(&doc.snapshot());
    let text = kept_text(&tree.page);
    let body = normalized_body(&text);
    let body_chars: Vec<char> = body.chars().collect();
    let blocks = blocks(&body);
    let trail = text.trim_end_matches('\n').chars().count();
    let mut w = Writer {
        kept,
        blocks: &blocks,
        records: Vec::new(),
        used_variant_fields: HashSet::new(),
        notes: Vec::new(),
        trail,
        trail_taken: trail + 1,
    };
    w.container(&tree.page, &body_chars, Area::Body, None);
    let has_records = !w.records.is_empty();
    for n in &tree.notes {
        w.note(n, &body_chars);
    }
    if layer != DEFAULT_LAYER || !kept.doc_fields.is_empty() {
        let mut r = Fields::new();
        r.insert("kind".into(), json!("doc"));
        if layer != DEFAULT_LAYER {
            r.insert("layer".into(), json!(layer));
        }
        extend_new(&mut r, &kept.doc_fields);
        w.push(Key::DOC, Value::Object(r));
    }
    w.kept_records(&tree);
    let Writer { records, notes, .. } = w;
    Out { kept: text, body, records, has_records, notes }
}

impl Out {
    /// `notes` are numbered in this order.
    fn assemble(self, notes: Vec<NoteOut>, kept: &Preserved) -> String {
        if let Some(trailer) = &kept.foreign_trailer {
            let out = format!("{}\n{trailer}", normalized_body(&self.kept));
            return layout::encode(&out, kept.encoding);
        }
        // Only the doc record: a plain document stays plain.
        let has_content = self.has_records || !notes.is_empty() || !kept.is_empty();
        if !has_content {
            return layout::encode(&self.kept, kept.encoding);
        }
        let mut records = self.records;
        let mut texts = Vec::with_capacity(notes.len());
        for (g, n) in notes.into_iter().enumerate() {
            texts.push(n.text);
            for (mut key, mut r) in n.records {
                key.index = g;
                key.seq += 10_000_000 + g * 1000;
                renumber(&mut r, g);
                records.push((key, r));
            }
        }
        let mut out = self.body;
        if !texts.is_empty() {
            out.push('\n');
            out.push_str(OVERFLOW_HEADER);
            out.push('\n');
            out.push_str(&layout::write_notes(&texts));
            out.push_str("\n-->\n");
        }
        if !records.is_empty() || !kept.invalid_lines.is_empty() {
            records.sort_by_key(|a| a.0);
            out.push('\n');
            out.push_str(layout::META_HEADER);
            out.push_str("1\n");
            for (_, r) in records {
                out.push_str(&layout::escape_json_line(&r.to_string()));
                out.push('\n');
            }
            for l in &kept.invalid_lines {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str("-->\n");
        }
        layout::encode(&out, kept.encoding)
    }
}

pub(crate) fn renumber_pub(r: &mut Value, note: usize) {
    renumber(r, note)
}

/// Sets the note a record belongs to: `note` on a stash record, `at.note` on the others.
fn renumber(r: &mut Value, note: usize) {
    let Some(o) = r.as_object_mut() else { return };
    if o.get("kind") == Some(&json!("stash")) {
        o.insert("note".into(), json!(note));
    } else if let Some(Value::Object(at)) = o.get_mut("at") {
        if at.contains_key("note") {
            at.insert("note".into(), json!(note));
        }
    }
}

/// Doc first, then body records by block and offset, then note records, then stash records.
/// Nested records follow their top-level ancestor in document order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    section: u8,
    index: usize,
    offset: usize,
    seq: usize,
}

impl Key {
    const DOC: Key = Key::new(0, 0, 0);

    const fn new(section: u8, index: usize, offset: usize) -> Key {
        Key {
            section,
            index,
            offset,
            seq: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Area {
    Body,
    Note(usize),
}

#[derive(Clone)]
struct Nest {
    parent: String,
    pvariant: usize,
    loc: (&'static str, usize),
    key: Key,
    /// In a version without the words: whether it shows.
    without: Option<bool>,
}

struct Writer<'a> {
    kept: &'a Preserved,
    blocks: &'a [Block],
    records: Vec<(Key, Value)>,
    used_variant_fields: HashSet<(GroupId, usize)>,
    notes: Vec<NoteOut>,
    /// Where the body's trailing newlines start in the kept text (they become one), and how far
    /// cuts among them have taken them so far.
    trail: usize,
    trail_taken: usize,
}

fn id_of(kept: &Preserved, id: GroupId) -> String {
    kept.id_names
        .get(&id)
        .cloned()
        .unwrap_or_else(|| ids::encode(id))
}

fn extend_new(r: &mut Fields, extra: &Fields) {
    for (k, v) in extra {
        if !r.contains_key(k) {
            r.insert(k.clone(), v.clone());
        }
    }
}

impl Writer<'_> {
    fn push(&mut self, mut key: Key, v: Value) {
        key.seq = self.records.len();
        self.records.push((key, v));
    }

    /// `at` for a span starting at `s` in the container, and the record's sort key.
    fn anchor(
        &self,
        area: Area,
        nest: Option<&Nest>,
        text: &[char],
        s: usize,
        e: usize,
        context: bool,
    ) -> (Fields, Key) {
        let (loc, offset, key) = match (nest, area) {
            (Some(n), _) => (n.loc, s, n.key),
            (None, Area::Body) => {
                let b = block_of(self.blocks, s);
                let off = s - self.blocks[b].start;
                (("block", b), off, Key::new(1, b, off))
            }
            (None, Area::Note(i)) => (("note", i), s, Key::new(2, i, s)),
        };
        let mut at = Fields::new();
        at.insert(loc.0.into(), json!(loc.1));
        at.insert("offset".into(), json!(offset));
        if context {
            at.insert("before".into(), json!(before(text, s)));
            at.insert("after".into(), json!(after(text, e.min(text.len()))));
        }
        (at, key)
    }

    fn container(&mut self, nodes: &[TreeNode], text: &[char], area: Area, nest: Option<&Nest>) {
        let mut o = 0;
        let mut open: Vec<(&TreeComment, usize)> = Vec::new();
        for n in nodes {
            match n {
                TreeNode::Text { text: t } => o += t.chars().count(),
                TreeNode::CommentStart(c) => open.push((c, o)),
                TreeNode::CommentEnd { id } => {
                    if let Some(k) = open.iter().position(|(c, _)| c.id == *id) {
                        let (c, s) = open.remove(k);
                        self.comment(c, text, s, o, area, nest);
                    }
                }
                TreeNode::Group(g) => {
                    let l = n.kept_len();
                    self.group(g, text, o, o + l, area, nest);
                    o += l;
                }
                TreeNode::Cut(c) => {
                    // A cut among the trailing newlines (which the body keeps one of) takes
                    // the ones before it along, so nothing between cuts there is lost.
                    if nest.is_none() && matches!(area, Area::Body) && o > self.trail {
                        // The newlines beyond the one the body keeps go into the cut.
                        let lead = "\n".repeat(o.saturating_sub(self.trail_taken));
                        self.trail_taken = self.trail_taken.max(o);
                        let mut c = c.clone();
                        if !lead.is_empty() {
                            c.content.insert(0, TreeNode::text(lead));
                        }
                        self.cut(&c, text, (self.trail + 1).min(text.len()), nest);
                    } else {
                        self.cut(c, text, o.min(text.len()), nest);
                    }
                }
            }
        }
    }

    fn comment(&mut self, c: &TreeComment, text: &[char], s: usize, e: usize, area: Area, nest: Option<&Nest>) {
        let (s, e) = (s.min(text.len()), e.min(text.len()));
        let (mut at, key) = self.anchor(area, nest, text, s, e, true);
        at.insert("length".into(), json!(e - s));
        at.insert("quote".into(), json!(text[s..e].iter().collect::<String>()));
        let mut r = Fields::new();
        r.insert("kind".into(), json!("comment"));
        r.insert("at".into(), Value::Object(at));
        r.insert("text".into(), json!(c.text));
        if let Some(t) = &c.created {
            r.insert("created".into(), json!(t));
        }
        if let Some(a) = &c.author {
            r.insert("author".into(), json!(a));
        }
        if let Some(n) = nest {
            r.insert("parent".into(), json!(n.parent));
            r.insert("pvariant".into(), json!(n.pvariant));
        }
        if let Some(extra) = self.kept.comment_fields.get(&c.text) {
            extend_new(&mut r, extra);
        }
        self.push(key, Value::Object(r));
    }

    /// Numbered as note 0.
    fn note_records(&mut self, content: &[TreeNode]) -> Vec<(Key, Value)> {
        let saved = std::mem::take(&mut self.records);
        let chars: Vec<char> = content_text(content).chars().collect();
        self.container(content, &chars, Area::Note(0), None);
        std::mem::replace(&mut self.records, saved)
    }

    fn cut(&mut self, c: &TreeCut, text: &[char], p: usize, nest: Option<&Nest>) {
        // Its text as it is: a paragraph cut's blank line comes along, so it goes back exactly.
        let content = c.content.clone();
        let note = content_text(&content);
        let (spot, link) = match nest {
            Some(n) => (
                Spot {
                    block: n.loc.1,
                    offset: Some(p),
                    before: before(text, p),
                    after: after(text, p),
                    parent: Some(n.parent.clone()),
                    pvariant: Some(n.pvariant),
                },
                n.loc.1,
            ),
            None => {
                // A paragraph cut right after the last block, or right before a block, is a gap;
                // anywhere else (the text around it was edited) a point.
                let last = self.blocks[self.blocks.len() - 1];
                let at_end = p == last.end && text[p..].iter().all(|c| c.is_whitespace());
                let start = self.blocks.iter().position(|b| b.start == p && (p == 0 || text[p - 1] == '\n'));
                match (c.paragraph, at_end, start) {
                    (true, true, _) => (Spot::gap(text, self.blocks, self.blocks.len()), self.blocks.len() - 1),
                    (true, false, Some(b)) => (Spot::gap(text, self.blocks, b), b),
                    _ => (Spot::point(text, self.blocks, p), block_of(self.blocks, p)),
                }
            }
        };
        // The paragraph link older readers follow: the start of its block.
        let origin = if nest.is_none() { block_quote(text, self.blocks[link]) } else { None };
        let mut from = Fields::new();
        if let Some(q) = &origin {
            from.insert("block".into(), json!(link));
            from.insert("quote".into(), json!(q));
        }
        from.insert("at".into(), spot.to_json());
        let quote: String = note.chars().take(24).collect();
        let mut r = Fields::new();
        r.insert("kind".into(), json!("stash"));
        r.insert("note".into(), json!(0));
        r.insert("quote".into(), json!(quote));
        r.insert("from".into(), Value::Object(from));
        if c.paragraph {
            r.insert("paragraph".into(), json!(true));
        }
        r.insert("trace".into(), trace_json(c.look));
        // A version without the words: a project's panel lists it from the file only while it shows.
        if let Some(shows) = nest.and_then(|n| n.without) {
            r.insert("version".into(), json!(if shows { "showing" } else { "hidden" }));
        }
        if let Some(b) = &c.batch {
            r.insert("batch".into(), json!({"id": b.id, "name": b.name}));
        }
        if let Some(extra) = self.kept.stash_fields.get(&quote) {
            extend_new(&mut r, extra);
        }
        let mut records = vec![(Key::new(4, 0, 0), Value::Object(r))];
        records.extend(self.note_records(&content));
        self.notes.push(NoteOut { text: note, records, project: true });
    }

    fn note(&mut self, n: &TreeNote, body: &[char]) {
        let note = content_text(&n.content);
        let mut records = Vec::new();
        if n.origin.is_some() || n.source.is_some() || n.lost || n.batch.is_some() {
            records.push((Key::new(4, 0, 0), self.stash(&note, n, body)));
        }
        records.extend(self.note_records(&n.content));
        self.notes.push(NoteOut { text: note, records, project: n.source.is_some() });
    }

    fn group(
        &mut self,
        g: &TreeGroup,
        text: &[char],
        s: usize,
        e: usize,
        area: Area,
        nest: Option<&Nest>,
    ) {
        // The body's trailing newlines may have been trimmed: a group showing only line breaks
        // at the end is anchored at the end of the body.
        let (s, e) = (s.min(text.len()), e.min(text.len()));
        let id = id_of(self.kept, g.id);
        let paragraph_at_block_start = g.scope == Scope::Paragraph
            && nest.is_none()
            && matches!(area, Area::Body)
            && self.blocks[block_of(self.blocks, s)].start == s;
        let (mut at, key) = self.anchor(area, nest, text, s, e, !paragraph_at_block_start);
        if paragraph_at_block_start {
            at.remove("offset");
        }
        let texts: Vec<String> = g
            .variants
            .iter()
            .map(|v| kept_text(&v.content))
            .collect();
        let variants: Vec<Value> = g
            .variants
            .iter()
            .zip(&texts)
            .map(|(v, t)| self.variant(g.id, t, v.by, v.without))
            .collect();
        let mut r = Fields::new();
        r.insert("kind".into(), json!("alt"));
        r.insert("id".into(), json!(id));
        r.insert("scope".into(), json!(scope_str(g.scope)));
        r.insert("at".into(), Value::Object(at));
        r.insert("active".into(), json!(g.active));
        r.insert("variants".into(), Value::Array(variants));
        if let Some(n) = nest {
            r.insert("parent".into(), json!(n.parent));
            r.insert("pvariant".into(), json!(n.pvariant));
        }
        if let Some(extra) = self.kept.group_fields.get(&g.id) {
            extend_new(&mut r, extra);
        }
        self.push(key, Value::Object(r));
        let loc = match (nest, area) {
            (Some(n), _) => n.loc,
            (None, Area::Body) => ("block", block_of(self.blocks, s)),
            (None, Area::Note(i)) => ("note", i),
        };
        for (k, v) in g.variants.iter().enumerate() {
            let chars: Vec<char> = texts[k].chars().collect();
            let inner = Nest {
                parent: id.clone(),
                pvariant: k,
                loc,
                key,
                without: v.without.then_some(k == g.active),
            };
            self.container(&v.content, &chars, area, Some(&inner));
        }
    }

    /// A bare string for the writer's own variants, else `{t, by, …}`.
    /// `without`: a version without the words, `{"t":"","cut":true}` (its cut is a record of its own).
    fn variant(&mut self, id: GroupId, text: &str, by: Source, without: bool) -> Value {
        let extra = self.kept.variant_fields.get(&id).and_then(|list| {
            let i = list.iter().enumerate().position(|(i, (t, _))| {
                t == text && !self.used_variant_fields.contains(&(id, i))
            })?;
            self.used_variant_fields.insert((id, i));
            Some(&list[i].1)
        });
        if by == Source::Me && extra.is_none() && !without {
            return json!(text);
        }
        let mut o = Fields::new();
        o.insert("t".into(), json!(text));
        o.insert("by".into(), json!(source_str(by)));
        if without {
            o.insert("cut".into(), json!(true));
        }
        if let Some(extra) = extra {
            extend_new(&mut o, extra);
        }
        Value::Object(o)
    }

    /// In a project's drawer `from` names the file instead of a block.
    fn stash(&self, note: &str, n: &TreeNote, body: &[char]) -> Value {
        let quote: String = note.chars().take(24).collect();
        let mut from = Fields::new();
        if let Some(file) = &n.source {
            from.insert("file".into(), json!(file));
        } else if let Some(origin) = &n.origin {
            let o: Vec<char> = origin.chars().collect();
            if let Some(b) = self
                .blocks
                .iter()
                .position(|b| body[b.start..].starts_with(&o))
            {
                from.insert("block".into(), json!(b));
            }
        }
        if let Some(origin) = &n.origin {
            from.insert("quote".into(), json!(origin));
        }
        let paragraph = n.paragraph && n.origin.is_some();
        let mut r = Fields::new();
        r.insert("kind".into(), json!("stash"));
        r.insert("note".into(), json!(0));
        r.insert("quote".into(), json!(quote));
        r.insert("from".into(), Value::Object(from));
        if paragraph {
            r.insert("paragraph".into(), json!(true));
        }
        if n.lost {
            r.insert("trace".into(), trace_json(n.look));
        }
        if let Some(b) = &n.batch {
            r.insert("batch".into(), json!({"id": b.id, "name": b.name}));
        }
        if let Some(extra) = self.kept.stash_fields.get(&quote) {
            extend_new(&mut r, extra);
        }
        Value::Object(r)
    }

    /// Verbatim. A detached id that now collides with a live group is renamed, with the `parent`
    /// references inside its records.
    fn kept_records(&mut self, tree: &doc_model::snapshot::DocTree) {
        let mut taken: HashSet<String> = HashSet::new();
        fn live(nodes: &[TreeNode], kept: &Preserved, out: &mut HashSet<String>) {
            for n in nodes {
                match n {
                    TreeNode::Group(g) => {
                        out.insert(id_of(kept, g.id));
                        for v in &g.variants {
                            live(&v.content, kept, out);
                        }
                    }
                    TreeNode::Cut(c) => live(&c.content, kept, out),
                    _ => {}
                }
            }
        }
        live(&tree.page, self.kept, &mut taken);
        for n in &tree.notes {
            live(&n.content, self.kept, &mut taken);
        }
        for d in &self.kept.detached {
            let mut records = d.records.clone();
            let mut renames: HashMap<String, String> = HashMap::new();
            for r in &records {
                if r.get("kind") != Some(&json!("alt")) {
                    continue;
                }
                let Some(id) = r.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if taken.contains(id) {
                    let mut k = 0;
                    let fresh = loop {
                        let f = ids::fallback(id, k);
                        if !taken.contains(&f) {
                            break f;
                        }
                        k += 1;
                    };
                    renames.insert(id.to_string(), fresh.clone());
                    taken.insert(fresh);
                } else {
                    taken.insert(id.to_string());
                }
            }
            for (i, r) in records.iter_mut().enumerate() {
                let Some(o) = r.as_object_mut() else { continue };
                for field in ["id", "parent"] {
                    if let Some(new) = o
                        .get(field)
                        .and_then(Value::as_str)
                        .and_then(|v| renames.get(v))
                    {
                        o.insert(field.into(), json!(new));
                    }
                }
                if i == 0 {
                    o.insert("detached".into(), json!(true));
                }
            }
            // A group's records stay together, in order, placed by its own record.
            let key = kept_key(&records[0], 0);
            for r in records {
                let seq = 1_000_000 + self.records.len();
                self.records.push((Key { seq, ..key }, r));
            }
        }
        for (i, r) in self.kept.unknown_records.iter().enumerate() {
            let key = kept_key(r, 2_000_000 + i);
            self.records.push((key, r.clone()));
        }
    }
}

/// From its own `at`, after generated records at the same place.
fn kept_key(r: &Value, base: usize) -> Key {
    let at = r.get("at");
    let num = |k: &str| at.and_then(|a| a.get(k)).and_then(Value::as_u64);
    let offset = num("offset").unwrap_or(0) as usize;
    let (section, index) = match (num("block"), num("note")) {
        (Some(b), _) => (1, b as usize),
        (None, Some(n)) => (2, n as usize),
        _ => (3, 0),
    };
    Key {
        section,
        index,
        offset,
        seq: base,
    }
}

pub(crate) fn ghost_key(r: &Fields) -> String {
    let s = |v: Option<&Value>| v.map(Value::to_string).unwrap_or_default();
    let at = |k: &str| s(r.get("at").and_then(|a| a.get(k)));
    format!(
        "{}|{}|{}|{}|{}|{}",
        s(r.get("parent")),
        s(r.get("pvariant")),
        at("block"),
        at("note"),
        at("offset"),
        at("length"),
    )
}

/// `true` follows the setting, otherwise the cut's own look.
fn trace_json(look: Option<Look>) -> Value {
    match look {
        None => json!(true),
        Some(Look::Text) => json!("text"),
        Some(Look::Mark) => json!("mark"),
    }
}

fn block_quote(text: &[char], b: Block) -> Option<String> {
    let q: String = text[b.start.min(text.len())..].iter().take_while(|c| **c != '\n').take(24).collect();
    (!q.trim().is_empty()).then_some(q)
}
