use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use doc_model::{Carried, CutId, CutInfo, CutPlace, Document, EditError, Look};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::{Cancel, Redo, Undo};
use crate::drawer::ToPage;
use crate::list_panel;
use crate::overflow::NoteDrag;
use crate::theme::{self, c, ca};
use crate::view::TextView;
use crate::workspace::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Page,
    Book,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CutRef {
    Page(CutId),
    Drawer(CutId),
    Parked(PathBuf, CutId),
    /// Index among the chapter's cuts in the project's cuts file.
    Disk(PathBuf, usize),
    Batch(String),
}

impl CutRef {
    pub fn id(&self) -> Option<CutId> {
        match self {
            CutRef::Page(id) | CutRef::Drawer(id) | CutRef::Parked(_, id) => Some(*id),
            CutRef::Disk(..) | CutRef::Batch(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trace {
    Faded,
    Mark,
    Untraced,
    Gone,
    /// In a version that isn't showing.
    Hidden,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub at: CutRef,
    pub text: String,
    pub trace: Trace,
    pub chip: Option<String>,
    /// Made since the page last changed how cuts show (*just now*).
    pub fresh: bool,
    /// Whole paragraphs, or more than one line: it lands between paragraphs.
    pub block: bool,
    /// (id, name) of the Trim batch it was made in.
    pub batch: Option<(String, String)>,
    pub nested: bool,
    pub comments: Vec<String>,
    /// A version without the words, showing: it leaves the panel when another version shows.
    pub version: bool,
}

pub struct Section {
    pub file: Option<PathBuf>,
    /// None for the page's own list.
    pub title: Option<String>,
    pub items: Vec<Item>,
    pub here: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutOp {
    Edit,
    PutBack,
    GoTo,
    Copy,
    Delete,
}

/// How many items of another file's group show before *N more*.
const FOLDED: usize = 3;

fn paragraphs_before(text: &[char], at: usize) -> usize {
    let mut n = 0;
    let mut line_start = true;
    let mut prev_blank = true;
    let mut i = 0;
    while i < at.min(text.len()) {
        if line_start {
            let end = text[i..].iter().position(|c| *c == '\n').map_or(text.len(), |k| i + k);
            let blank = text[i..end].iter().all(|c| c.is_whitespace());
            if !blank && prev_blank {
                n += 1;
            }
            prev_blank = blank;
        }
        line_start = text[i] == '\n';
        i += 1;
    }
    n
}

/// The paragraph a cut sits in (`¶n`), or for whole paragraphs the one it sits after.
pub(crate) fn chip_at(text: &[char], at: usize, paragraph: bool) -> String {
    if paragraph {
        let n = paragraphs_before(text, at);
        if n == 0 { tr!("before ¶1").to_string() } else { tr!("after ¶{n}", n = n).to_string() }
    } else {
        tr!("¶{n}", n = paragraphs_before(text, at + 1).max(1)).to_string()
    }
}

/// `from ¶3`: the paragraph whose start is `quote`, and where it starts.
pub fn origin_in(text: &str, quote: &str) -> Option<(usize, usize)> {
    let mut n = 0;
    let mut prev_blank = true;
    let mut at = 0;
    for line in text.split('\n') {
        let blank = line.trim().is_empty();
        if !blank && prev_blank {
            n += 1;
            if line.starts_with(quote) || quote.starts_with(line) && !line.is_empty() {
                return Some((n, at));
            }
        }
        prev_blank = blank;
        at += line.chars().count() + 1;
    }
    None
}

fn items_of(doc: &Document, label: Option<&str>, wrap: &dyn Fn(CutId) -> CutRef) -> Vec<Item> {
    items_where(doc, label, wrap, &|_| true)
}

fn items_where(doc: &Document, label: Option<&str>, wrap: &dyn Fn(CutId) -> CutRef, keep: &dyn Fn(&CutInfo) -> bool) -> Vec<Item> {
    let cuts: Vec<CutInfo> = doc.listed_cuts().into_iter().filter(|c| keep(c)).collect();
    if cuts.is_empty() {
        return Vec::new();
    }
    let plain = doc.text();
    let text: Vec<char> = plain.chars().collect();
    let look = doc.presentation().look;
    let named = |chip: String| match label {
        Some(l) => format!("{l} {chip}"),
        None => chip,
    };
    cuts.iter().map(|c| item_of(c, &text, &plain, look, label, &named, wrap)).collect()
}

fn item_of(c: &CutInfo, text: &[char], plain: &str, look: Look, label: Option<&str>, named: &dyn Fn(String) -> String, wrap: &dyn Fn(CutId) -> CutRef) -> Item {
    let (trace, chip) = match &c.place {
        CutPlace::Page { range, .. } => {
            let trace = if c.look.unwrap_or(look) == Look::Mark { Trace::Mark } else { Trace::Faded };
            (trace, Some(named(chip_at(text, range.start, c.paragraph))))
        }
        CutPlace::Hidden { at } => (Trace::Hidden, Some(named(chip_at(text, *at, c.paragraph)))),
        CutPlace::Note(_) => {
            let chip = c.origin.as_deref().and_then(|q| origin_in(plain, q)).map(|(n, _)| match label {
                Some(l) => tr!("from {name} ¶{n}", name = l, n = n).to_string(),
                None => tr!("from ¶{n}", n = n).to_string(),
            });
            (if c.lost { Trace::Gone } else { Trace::Untraced }, chip.or_else(|| label.map(|l| tr!("from {name}", name = l).to_string())))
        }
    };
    Item {
        at: wrap(c.id),
        text: c.text.clone(),
        trace,
        chip,
        fresh: c.fresh,
        block: c.paragraph || c.text.trim().contains('\n'),
        batch: c.batch.as_ref().map(|b| (b.id.clone(), b.name.clone())),
        nested: false,
        comments: c.comments.clone(),
        version: c.version.is_some(),
    }
}

fn batched(items: Vec<Item>, unfolded: &std::collections::HashSet<String>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for it in &items {
        let Some((id, name)) = it.batch.clone() else {
            out.push(it.clone());
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        let members: Vec<&Item> = items.iter().filter(|o| o.batch.as_ref().is_some_and(|(b, _)| *b == id)).collect();
        let n = members.len();
        out.push(Item {
            at: CutRef::Batch(id.clone()),
            text: trn!("{name} · {n} cut", "{name} · {n} cuts", n, name = name),
            trace: it.trace,
            chip: it.chip.clone(),
            fresh: members.iter().any(|m| m.fresh),
            block: false,
            batch: None,
            nested: false,
            comments: members.iter().flat_map(|m| m.comments.clone()).collect(),
            version: false,
        });
        if unfolded.contains(&id) {
            out.extend(members.into_iter().map(|m| Item { nested: true, ..m.clone() }));
        }
    }
    out
}

/// A closed chapter's cut, unless it is a version without the words that isn't showing.
pub(crate) fn listed_on_disk(cut: &doc_format::ExternalCut) -> bool {
    cut.records.first().and_then(|r| r.get("version")).and_then(|v| v.as_str()) != Some("hidden")
}

fn item_on_disk(path: &Path, i: usize, cut: &doc_format::ExternalCut, label: &str) -> Item {
    let stash = cut.records.first();
    let at = stash.and_then(|r| r.get("from")).and_then(|f| f.get("at"));
    let paragraph = stash.and_then(|r| r.get("paragraph")).and_then(|v| v.as_bool()).unwrap_or(false);
    let trace_val = stash.and_then(|r| r.get("trace"));
    let (trace, chip) = match at {
        Some(at) => {
            let block = at.get("block").and_then(|b| b.as_u64()).unwrap_or(0) as usize;
            let mark = trace_val.and_then(|t| t.as_str()) == Some("mark");
            let gap = at.get("gap").and_then(|g| g.as_bool()).unwrap_or(false);
            let chip = if paragraph || gap {
                if block == 0 { tr!("before ¶1").to_string() } else { tr!("after ¶{n}", n = block).to_string() }
            } else {
                tr!("¶{n}", n = block + 1).to_string()
            };
            (if mark { Trace::Mark } else { Trace::Faded }, Some(format!("{label} {chip}")))
        }
        None => (if trace_val.is_some() { Trace::Gone } else { Trace::Untraced }, Some(tr!("from {name}", name = label).to_string())),
    };
    let text = cut.text().trim_end().to_string();
    let block = paragraph || text.contains('\n');
    let comments = cut.records.iter().filter(|r| r.get("kind").and_then(|k| k.as_str()) == Some("comment")).filter_map(|r| r.get("text").and_then(|t| t.as_str()).map(String::from)).collect();
    let version = stash.and_then(|r| r.get("version")).and_then(|v| v.as_str()) == Some("showing");
    Item { at: CutRef::Disk(path.to_path_buf(), i), text, trace, chip, fresh: false, block, batch: None, nested: false, comments, version }
}

impl Workspace {
    /// Its document's cuts, then (in a project) the cuts of no file that came from it.
    fn page_items(&self, cx: &App) -> Vec<Item> {
        let unfolded = self.drawer.as_ref().map(|d| d.unfolded_batches.clone()).unwrap_or_default();
        let mut items = batched(items_of(&self.model.read(cx).doc, None, &CutRef::Page), &unfolded);
        if let (Some(drawer), Some(here)) = (self.shared_drawer(), self.page_source(cx)) {
            items.extend(items_where(&drawer.read(cx).doc, None, &CutRef::Drawer, &|c| c.source.as_deref() == Some(here.as_str())));
        }
        items
    }

    pub(crate) fn panel_sections(&self, cx: &App) -> Vec<Section> {
        let here = self.model.read(cx).path.clone();
        let page = Section { file: here.clone(), title: None, items: self.page_items(cx), here: true };
        let Some(p) = self.project.as_ref().filter(|_| self.cuts_scope == Scope::Book) else { return vec![page] };
        let title = |e: &crate::project::Entry| match e.number {
            Some(n) => format!("{n} · {}", e.label.to_uppercase()),
            None => e.label.to_uppercase(),
        };
        let mut out = Vec::new();
        let mut page = page;
        page.title = here.as_ref().and_then(|h| p.tree.entry(h)).map(title).or_else(|| Some(tr!("THIS PAGE").to_string()));
        out.push(page);
        let drawer = p.drawer.model.read(cx);
        let root = p.tree.root.clone();
        let mut named = std::collections::HashSet::new();
        for e in p.tree.all() {
            if Some(&e.path) == here.as_ref() {
                named.insert(crate::drawer::relative(&root, &e.path));
                continue;
            }
            let rel = crate::drawer::relative(&root, &e.path);
            named.insert(rel.clone());
            let label = e.label.as_str();
            let mut items = match p.parked.get(&e.path) {
                Some(q) => items_of(&q.model.doc, Some(label), &|id| CutRef::Parked(e.path.clone(), id)),
                None => p
                    .drawer
                    .external
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| Some(&c.file) == rel.as_ref() && listed_on_disk(c))
                    .map(|(i, c)| item_on_disk(&e.path, i, c, label))
                    .collect(),
            };
            items.extend(items_where(&drawer.doc, Some(label), &CutRef::Drawer, &|c| c.source == rel));
            if !items.is_empty() {
                out.push(Section { file: Some(e.path.clone()), title: Some(title(e)), items, here: false });
            }
        }
        let loose = items_where(&drawer.doc, None, &CutRef::Drawer, &|c| !named.contains(&c.source));
        if !loose.is_empty() {
            out.push(Section { file: None, title: Some(tr!("FROM NO FILE").to_string()), items: loose, here: false });
        }
        out
    }

    pub(crate) fn panel_items(&self, cx: &App) -> Vec<Item> {
        let unfolded = self.drawer.as_ref().map(|d| d.unfolded.clone()).unwrap_or_default();
        self.panel_sections(cx)
            .into_iter()
            .flat_map(|s| {
                let open = s.here || s.file.as_ref().is_some_and(|f| unfolded.contains(f)) || s.items.len() <= FOLDED + 1;
                let n = if open { s.items.len() } else { FOLDED };
                s.items.into_iter().take(n)
            })
            .collect()
    }

    /// Counted without building the list: the header draws on every keystroke.
    pub(crate) fn cuts_count_for_tab(&self, cx: &App) -> usize {
        let page = self.model.read(cx).doc.listed_cuts().len();
        let Some(p) = &self.project else { return page };
        let drawer = p.drawer.model.read(cx).doc.listed_cuts();
        if self.cuts_scope == Scope::Page {
            let here = self.page_source(cx);
            return page + drawer.iter().filter(|c| here.is_some() && c.source == here).count();
        }
        let here = self.model.read(cx).path.clone();
        let root = &p.tree.root;
        let others: usize = p
            .tree
            .all()
            .filter(|e| Some(&e.path) != here.as_ref())
            .map(|e| match p.parked.get(&e.path) {
                Some(q) => q.model.doc.listed_cuts().len(),
                None => {
                    let rel = crate::drawer::relative(root, &e.path);
                    p.drawer.external.iter().filter(|c| Some(&c.file) == rel.as_ref() && listed_on_disk(c)).count()
                }
            })
            .sum();
        page + others + drawer.len()
    }

    /// `note` counts the page's cuts, then the cuts of no file; with `at_drawer`, those alone.
    pub(crate) fn found_cut(&self, note: usize, at_drawer: bool, cx: &App) -> Option<CutRef> {
        let page = self.model.read(cx).doc.notes();
        let drawer = self.shared_drawer().map(|d| d.read(cx).doc.notes()).unwrap_or_default();
        if at_drawer {
            return drawer.get(note).map(|n| CutRef::Drawer(n.id));
        }
        match page.get(note) {
            Some(n) => Some(CutRef::Page(n.id)),
            None => drawer.get(note - page.len()).map(|n| CutRef::Drawer(n.id)),
        }
    }

    /// Reads and parks a chapter on disk so its cut can be used; returns the cut there.
    fn materialize(&mut self, at: CutRef) -> Option<CutRef> {
        let CutRef::Disk(path, i) = at else { return Some(at) };
        let p = self.project.as_ref()?;
        let want = p.drawer.external.get(i)?.text().trim().to_string();
        if !p.parked.contains_key(&path) {
            let m = self.load_chapter(&path).ok()?;
            self.project.as_mut()?.parked.insert(path.clone(), crate::project_ui::Parked::fresh(m));
        }
        let q = self.project.as_ref()?.parked.get(&path)?;
        let id = q.model.doc.cuts().into_iter().find(|c| c.text.trim() == want)?.id;
        Some(CutRef::Parked(path, id))
    }

    fn landing(&self, at: usize, block: bool, cx: &App) -> doc_model::Place {
        let text: Vec<char> = self.model.read(cx).doc.text().chars().collect();
        crate::cuts::place_at(&text, at, block)
    }

    /// Moving removes its trace; from another document both halves are one undo step.
    pub(crate) fn use_cut(&mut self, at: CutRef, to: ToPage, copy: bool, cx: &mut Context<Self>) -> Result<Range<usize>, EditError> {
        let at = self.materialize(at).ok_or(EditError::NoSuchCut)?;
        let block = |doc: &Document, id: CutId| doc.cut_info(id).is_some_and(|c| c.paragraph);
        match at {
            CutRef::Page(id) => {
                let place = match to {
                    ToPage::Place(p) => p,
                    ToPage::At(a) => self.landing(a, block(&self.model.read(cx).doc, id), cx),
                };
                self.model.update(cx, |m, cx| m.edit(cx, |d| d.drop_text(Carried::Cut(id), place, copy)))
            }
            CutRef::Drawer(id) => {
                let drawer = self.shared_drawer().ok_or(EditError::NoSuchCut)?;
                let frag = drawer.read(cx).doc.cut_fragment(id)?;
                let place = match to {
                    ToPage::Place(p) => p,
                    ToPage::At(a) => self.landing(a, block(&drawer.read(cx).doc, id), cx),
                };
                let r = self.model.update(cx, |m, cx| m.edit(cx, |d| d.drop_text(Carried::Fragment(frag.trimmed()), place, true)))?;
                if !copy && drawer.update(cx, |m, cx| m.edit(cx, |d| d.delete_cut(id)).is_ok()) {
                    self.link_steps(cx);
                }
                Ok(r)
            }
            CutRef::Parked(path, id) => {
                let q = self.project.as_ref().and_then(|p| p.parked.get(&path)).ok_or(EditError::NoSuchCut)?;
                let b = block(&q.model.doc, id);
                let place = match to {
                    ToPage::Place(p) => p,
                    ToPage::At(a) => self.landing(a, b, cx),
                };
                if copy {
                    let frag = q.model.doc.cut_fragment(id)?;
                    return self.model.update(cx, |m, cx| m.edit(cx, |d| d.drop_text(Carried::Fragment(frag.trimmed()), place, true)));
                }
                let uid = q.model.uid;
                self.move_cut_from_parked(uid, id, place, cx)
            }
            CutRef::Disk(..) | CutRef::Batch(_) => Err(EditError::NoSuchCut),
        }
    }

    pub(crate) fn cut_op(&mut self, at: CutRef, op: CutOp, window: &mut Window, cx: &mut Context<Self>) {
        match op {
            CutOp::Edit => return self.edit_note(at, window, cx),
            CutOp::Copy => {
                if let Some(t) = self.cut_text(&at, cx) {
                    cx.write_to_clipboard(ClipboardItem::new_string(t));
                }
            }
            CutOp::PutBack | CutOp::Delete if matches!(at, CutRef::Batch(_)) => {
                let CutRef::Batch(b) = &at else { unreachable!() };
                let b = b.clone();
                let _ = self.model.update(cx, |m, cx| m.edit(cx, |d| if op == CutOp::PutBack { d.put_back_batch(&b) } else { d.delete_batch(&b) }));
                self.page.update(cx, |p, cx| p.refresh(cx));
            }
            CutOp::GoTo => self.go_to_cut(at, window, cx),
            CutOp::PutBack => {
                if let Some(CutRef::Page(id)) = self.bring_to_page(at, window, cx) {
                    self.put_back_cut(id, cx);
                }
            }
            CutOp::Delete => self.delete_cut_at(at, cx),
        }
        if let Some(d) = &mut self.drawer {
            d.editing = None;
            d.editor = None;
        }
        cx.notify();
    }

    fn cut_text(&self, at: &CutRef, cx: &App) -> Option<String> {
        match at {
            CutRef::Page(id) => self.model.read(cx).doc.cut_info(*id).map(|c| c.text),
            CutRef::Drawer(id) => self.shared_drawer()?.read(cx).doc.cut_info(*id).map(|c| c.text),
            CutRef::Parked(path, id) => self.project.as_ref()?.parked.get(path)?.model.doc.cut_info(*id).map(|c| c.text),
            CutRef::Disk(_, i) => self.project.as_ref()?.drawer.external.get(*i).map(|c| c.text().trim_end().to_string()),
            CutRef::Batch(b) => {
                let words: Vec<String> = self.model.read(cx).doc.cuts().into_iter().filter(|c| c.batch.as_ref().is_some_and(|x| x.id == *b)).map(|c| c.text.trim().to_string()).collect();
                (!words.is_empty()).then(|| words.join(" "))
            }
        }
    }

    fn bring_to_page(&mut self, at: CutRef, window: &mut Window, cx: &mut Context<Self>) -> Option<CutRef> {
        let at = self.materialize(at)?;
        match at {
            CutRef::Parked(path, id) => self.switch_to(&path, window, cx).then_some(CutRef::Page(id)),
            other => Some(other),
        }
    }

    pub(crate) fn go_to_cut(&mut self, at: CutRef, window: &mut Window, cx: &mut Context<Self>) {
        if let CutRef::Drawer(id) = &at {
            let Some(c) = self.shared_drawer().and_then(|d| d.read(cx).doc.cut_info(*id)) else { return };
            match c.source {
                Some(src) if self.page_source(cx).as_deref() != Some(src.as_str()) => self.follow_origin_in(&src, c.origin.as_deref(), window, cx),
                _ => {
                    if let Some((_, start)) = c.origin.as_deref().and_then(|q| self.origin_paragraph(q, cx)) {
                        self.follow_origin(start, cx);
                    }
                }
            }
            return;
        }
        let at = match at {
            CutRef::Batch(b) => {
                let first = self.model.read(cx).doc.cuts().into_iter().find(|c| c.batch.as_ref().is_some_and(|x| x.id == b));
                let Some(first) = first else { return };
                CutRef::Page(first.id)
            }
            other => other,
        };
        let Some(CutRef::Page(id)) = self.bring_to_page(at, window, cx) else { return };
        let Some(c) = self.model.read(cx).doc.cut_info(id) else { return };
        match c.place {
            CutPlace::Page { range, .. } => self.page.update(cx, |p, cx| {
                let flash = if range.is_empty() { p.index().paragraph_at(range.start) } else { range.clone() };
                p.reveal(range.start, cx);
                p.flash = Some((flash, Instant::now()));
                cx.notify();
            }),
            CutPlace::Hidden { at } => self.page.update(cx, |p, cx| {
                p.reveal(at, cx);
                cx.notify();
            }),
            CutPlace::Note(_) => {
                if let Some((_, start)) = c.origin.as_deref().and_then(|q| self.origin_paragraph(q, cx)) {
                    self.follow_origin(start, cx);
                }
            }
        }
    }

    /// One click: the cut becomes the one ⌘↩ uses and its place, if on this page, comes to the
    /// middle of the window. Neither the caret nor the file changes.
    fn select_cut(&mut self, at: CutRef, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &mut self.drawer else { return };
        if d.editing.as_ref().is_some_and(|e| *e != at) {
            d.editing = None;
            d.editor = None;
        }
        d.selected = Some(at.clone());
        if d.editing.is_none() {
            let f = d.focus.clone();
            window.focus(&f, cx);
        }
        self.centre_cut(&at, cx);
        cx.notify();
    }

    /// Where the cut was on this page: its faded text or mark, else the paragraph its quote is in.
    fn place_on_page(&self, at: &CutRef, cx: &App) -> Option<Range<usize>> {
        let (c, drawer_side) = match at {
            CutRef::Page(id) => (self.model.read(cx).doc.cut_info(*id)?, false),
            CutRef::Batch(b) => (self.model.read(cx).doc.cuts().into_iter().find(|c| c.batch.as_ref().is_some_and(|x| x.id == *b))?, false),
            CutRef::Drawer(id) => (self.shared_drawer()?.read(cx).doc.cut_info(*id)?, true),
            CutRef::Parked(..) | CutRef::Disk(..) => return None,
        };
        if drawer_side && c.source.is_some_and(|src| self.page_source(cx).as_deref() != Some(src.as_str())) {
            return None;
        }
        let idx = self.page.read(cx).index();
        match c.place {
            CutPlace::Page { range, .. } if range.is_empty() => Some(idx.paragraph_at(range.start)),
            CutPlace::Page { range, .. } => Some(range),
            CutPlace::Hidden { at } => Some(at..at),
            CutPlace::Note(_) => {
                let (_, start) = self.origin_paragraph(c.origin.as_deref()?, cx)?;
                Some(idx.paragraph_at(start))
            }
        }
    }

    fn centre_cut(&mut self, at: &CutRef, cx: &mut Context<Self>) {
        let Some(r) = self.place_on_page(at, cx) else { return };
        self.page.update(cx, |p, cx| {
            if !r.is_empty() {
                p.flash = Some((r.clone(), Instant::now()));
            }
            p.centre_on(r, cx);
        });
    }

    fn delete_cut_at(&mut self, at: CutRef, cx: &mut Context<Self>) {
        let Some(at) = self.materialize(at) else { return };
        match at {
            CutRef::Page(id) => {
                let _ = self.model.update(cx, |m, cx| m.edit(cx, |d| d.delete_cut(id)));
                self.page.update(cx, |p, cx| p.refresh(cx));
            }
            CutRef::Drawer(id) => {
                if let Some(d) = self.shared_drawer() {
                    let _ = d.update(cx, |m, cx| m.edit(cx, |d| d.delete_cut(id)));
                }
            }
            CutRef::Parked(path, id) => {
                if let Some(q) = self.project.as_mut().and_then(|p| p.parked.get_mut(&path)) {
                    if q.model.doc.delete_cut(id).is_ok() {
                        q.model.version += 1;
                    }
                }
            }
            CutRef::Disk(..) | CutRef::Batch(_) => {}
        }
    }

    fn open_cut_menu(&mut self, at: CutRef, trace: Trace, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let traced = matches!(trace, Trace::Faded | Trace::Mark | Trace::Hidden);
        self.context_menu = Some(crate::context_menu::ContextMenu::cut(at, traced, pos));
        cx.notify();
    }

    pub fn render_drawer(&mut self, side: crate::panels::Side, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !cx.has_active_drag() {
            if let Some(d) = &mut self.drawer {
                d.hot = false;
            }
        }
        let d = self.drawer.as_ref()?;
        let hot = d.hot;
        let slot = hot
            .then(|| self.page.read(cx).carrying.clone().map(|r| crate::drag::preview(self.page.read(cx).index().slice(r))))
            .flatten();
        let editing = d.editing.clone();
        let found = d.found.clone().or_else(|| d.selected.clone());
        let editor = d.editor.clone();
        let unfolded = d.unfolded.clone();
        let card_bounds = d.cards.clone();
        let focus = d.focus.clone();
        let sections = self.panel_sections(cx);
        let total: usize = sections.iter().map(|s| s.items.len()).sum();
        let kind = self.project_kind();
        let whose = kind.is_none().then(|| {
            let m = self.model.read(cx);
            let name = m.path.as_deref().map_or_else(|| m.name.clone().unwrap_or_else(|| tr!("untitled").to_string()), |p| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            (name, format!(" · {}", trn!("{n} cut", "{n} cuts", total)))
        });
        let switch = kind.map(|k| {
            let whole = match k {
                crate::project::Kind::Book => tr!("whole book"),
                crate::project::Kind::Journal => tr!("whole journal"),
                crate::project::Kind::Folder => tr!("whole folder"),
            };
            list_panel::scope_switch(
                "cuts-scope",
                [tr!("this page").into(), whole.into()],
                usize::from(self.cuts_scope == Scope::Book),
                cx.listener(|this, i: &usize, _, cx| {
                    this.cuts_scope = if *i == 1 { Scope::Book } else { Scope::Page };
                    if let Some(d) = &mut this.drawer {
                        d.editing = None;
                        d.editor = None;
                    }
                    cx.notify();
                }),
            )
        });
        let own = if kind.is_some() { self.model.read(cx).doc.cuts().iter().filter(|c| matches!(c.place, CutPlace::Note(_)) && c.source.is_none()).count() } else { 0 };
        let own_row = (own > 0).then(|| {
            let file = self.model.read(cx).path.as_deref().and_then(|p| self.project.as_ref().and_then(|q| q.tree.entry(p)).map(|e| e.label.clone())).unwrap_or_else(|| tr!("this file").to_string());
            div()
                .id("file-scraps")
                .flex()
                .flex_col()
                .gap(px(2.))
                .mx(px(8.))
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(c(theme::muted()))
                .child(trn!("{file} keeps {n} cut in its own file", "{file} keeps {n} cuts in its own file", own, file = file))
                .child(
                    div()
                        .id("move-file-scraps")
                        .cursor_pointer()
                        .text_color(c(theme::accent()))
                        .hover(|s| s.text_color(c(theme::text())))
                        .on_click(cx.listener(|this, _, _, cx| this.move_file_scraps(cx)))
                        .child(if own == 1 { tr!("keep it with the project’s cuts") } else { tr!("keep them with the project’s cuts") })
                        .test_support(),
                )
                .test_support()
        });
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut is_card: Vec<bool> = if slot.is_some() { vec![false] } else { Vec::new() };
        let mut flat = 0usize;
        let book = sections.len() > 1 || sections.first().is_some_and(|s| s.title.is_some());
        for (si, s) in sections.into_iter().enumerate() {
            let n = s.items.len();
            if n == 0 {
                continue;
            }
            let open = s.here || s.file.as_ref().is_some_and(|f| unfolded.contains(f)) || n <= FOLDED + 1;
            if book {
                rows.push(list_panel::section_header(("cuts-section", si), s.title.clone().unwrap_or_default(), n));
                is_card.push(false);
            }
            let shown = if open { n } else { FOLDED };
            for it in s.items.into_iter().take(shown) {
                rows.push(self.render_cut(flat, it, editing.as_ref(), found.as_ref(), editor.as_ref(), cx));
                is_card.push(true);
                flat += 1;
            }
            if !open {
                let file = s.file.clone();
                is_card.push(false);
                rows.push(list_panel::fold_row(
                    ("cuts-more", si),
                    trn!("{n} more", "{n} more", n - FOLDED),
                    cx.listener(move |this, _, _, cx| {
                        if let (Some(d), Some(f)) = (&mut this.drawer, &file) {
                            d.unfolded.insert(f.clone());
                        }
                        cx.notify();
                    }),
                ));
            }
        }
        let empty = total == 0;
        let tabs = crate::panels::strip_at_top(self.tab_strip(side, cx));
        let foot = self.foot_here(side, cx).then(|| self.render_panel_foot(cx));
        Some(
            crate::panels::frame(div().id("overflow").test_support(), side, cx)
                .flex()
                .flex_col()
                // The panel starts under the header; its content stays level with the page.
                .pt(px(160. - crate::workspace::HEADER_H))
                .px(px(13.))
                .pb(px(64.))
                .gap_3()
                .text_size(px(14.))
                .line_height(px(21.))
                .when(hot, |d| d.bg(ca(theme::accent(), 0.06)).border_color(ca(theme::accent(), 0.5)))
                .on_drop(cx.listener(|this, d: &crate::drag::TextDrag, w, cx| this.drop_text_on_overflow(d, w, cx)))
                .on_drag_move(cx.listener(|this, e: &DragMoveEvent<crate::drag::TextDrag>, _, cx| {
                    let hot = e.bounds.contains(&e.event.position);
                    if let Some(d) = &mut this.drawer {
                        if d.hot != hot {
                            d.hot = hot;
                            cx.notify();
                        }
                    }
                }))
                .child(crate::workspace::resize_handle(side.edge()))
                // Zero-size, so a click in the panel doesn't take the keyboard from a selected cut.
                .child(
                    div()
                        .id("cuts-keys")
                        .absolute()
                        .size_0()
                        .track_focus(&focus)
                        .key_context("Note")
                        // A selected cut has the keyboard, but undo is still the page's.
                        .on_action(cx.listener(|this, _: &Undo, _, cx| this.page.update(cx, |p, cx| p.undo(cx))))
                        .on_action(cx.listener(|this, _: &Redo, _, cx| this.page.update(cx, |p, cx| p.redo(cx))))
                        .on_action(cx.listener(|this, _: &Cancel, w, cx| this.stop_editing_note(w, cx))),
                )
                .child(tabs)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .pl(px(8.))
                        .child(div().font_family(theme::hand()).text_size(theme::hand_px(19.)).text_color(c(theme::accent())).child(tr!("Cuts")))
                        .children(switch)
                        .children(whose.map(|(name, count)| {
                            div()
                                .id("overflow-whose")
                                .aria_label(format!("{name}{count}"))
                                .flex()
                                .min_w_0()
                                .text_size(px(10.5))
                                .line_height(px(14.))
                                .text_color(c(theme::faint()))
                                .child(div().min_w_0().truncate().child(name))
                                .child(div().flex_none().whitespace_nowrap().child(count))
                                .test_support()
                        })),
                )
                .children(own_row)
                .child(
                    div()
                        .on_children_prepainted(move |b, _, _| {
                            *card_bounds.borrow_mut() = b.into_iter().zip(is_card.iter().chain(std::iter::repeat(&false))).filter(|(_, k)| **k).map(|(b, _)| b).collect();
                        })
                        .id("notes")
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .flex_1()
                        .overflow_y_scroll()
                        .children(slot.map(|text| {
                            div()
                                .id("overflow-slot")
                                .flex()
                                .flex_col()
                                .gap_1()
                                .p_2()
                                .rounded_md()
                                .border_1()
                                .border_dashed()
                                .border_color(ca(theme::accent(), 0.75))
                                .bg(ca(theme::accent(), 0.06))
                                .child(div().text_size(px(10.5)).text_color(c(theme::accent())).child(tr!("put it here")))
                                .child(div().text_color(c(theme::muted())).child(text))
                                .test_support()
                        }))
                        .children(rows)
                        .when(empty && !hot, |d| {
                            d.child(
                                div()
                                    .id("cuts-empty")
                                    .text_color(c(theme::faint()))
                                    .text_size(px(12.))
                                    .child(tr!("What you cut waits here, with its place. {cut} fades the text where it was; {stash} leaves no trace.", stash = crate::os_text::key("⇧⌘X"), cut = crate::os_text::key("⌘X")))
                                    .test_support(),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(if hot { c(theme::accent()) } else { c(theme::faint()) })
                        .child(if hot { tr!("release to cut it here · {alt} keeps it on the page", alt = crate::os_text::key("⌥")) } else { tr!("{paste} or drag to use · {put_back} puts back", paste = crate::os_text::key("⌘V"), put_back = crate::os_text::key("⌘/")) }),
                )
                .children(foot)
                .into_any_element(),
        )
    }

    fn render_cut(&self, i: usize, it: Item, editing: Option<&CutRef>, found: Option<&CutRef>, editor: Option<&Entity<TextView>>, cx: &mut Context<Self>) -> AnyElement {
        let is_editing = editing == Some(&it.at);
        let drag = NoteDrag { at: it.at.clone(), preview: crate::drag::preview(&it.text), words: english::word_count(&it.text), block: it.block };
        let body: AnyElement = match (is_editing, editor) {
            (true, Some(e)) => div().child(e.clone()).into_any_element(),
            _ => div()
                .text_size(px(theme::NOTE_PX))
                .line_height(px(theme::NOTE_LINE))
                .text_color(c(theme::note_text()))
                .line_clamp(2)
                .text_ellipsis()
                .child(if it.text.is_empty() { " ".to_string() } else { it.text.clone() })
                .into_any_element(),
        };
        let (glyph, state) = match it.trace {
            // A version without the words says so: it leaves the panel when another version shows.
            Trace::Faded | Trace::Mark if it.version => (if it.trace == Trace::Mark { "|" } else { "ab" }, tr!("a version")),
            Trace::Faded => ("ab", tr!("faded")),
            Trace::Mark => ("|", tr!("a mark")),
            Trace::Untraced => ("·", tr!("no trace")),
            Trace::Gone => ("·", tr!("place gone")),
            Trace::Hidden => ("ab", tr!("in another version")),
        };
        let what = state.to_string();
        // The glyph and the chip already say faded, a mark or none and where; words only for what they can't.
        let unusual = matches!(it.trace, Trace::Gone | Trace::Hidden) || it.version;
        let shown = match (unusual, it.fresh) {
            (true, true) => Some(format!("{state} · {}", tr!("just now"))),
            (true, false) => Some(state.to_string()),
            (false, true) => Some(tr!("just now").to_string()),
            (false, false) => None,
        };
        let state = if it.fresh { format!("{state} · {}", tr!("just now")) } else { state.to_string() };
        let notrace = matches!(it.trace, Trace::Untraced | Trace::Gone);
        let at_menu = it.at.clone();
        let at_click = it.at.clone();
        let at_chip = it.at.clone();
        let trace = it.trace;
        let chip = it.chip.clone().map(|label| {
            div()
                .id(("origin", i))
                .aria_label(label.clone())
                .flex_none()
                .px(px(5.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(theme::origin_line()))
                .text_size(px(9.5))
                .line_height(px(15.))
                .text_color(c(theme::origin_text()))
                .hover(|s| s.text_color(c(theme::origin_hover_text())).border_color(c(theme::origin_hover_line())))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.go_to_cut(at_chip.clone(), window, cx);
                }))
                .child(label)
                .test_support()
        });
        let meta = div()
            .flex()
            .items_center()
            .gap(px(5.))
            .text_size(px(10.))
            .line_height(px(15.))
            .text_color(c(theme::faint()))
            .child(
                div()
                    .id(("cut-glyph", i))
                    .w(px(16.))
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(if trace == Trace::Mark { ca(theme::accent(), 0.7) } else { c(theme::faint()) })
                    .tooltip(crate::about::tip(what))
                    .child(glyph),
            )
            .when(notrace, |d| d.children(shown.clone()))
            .children(chip)
            .when(!notrace, |d| d.children(shown))
            .when(!it.comments.is_empty(), |d| {
                let n = it.comments.len();
                d.child(
                    div()
                        .id(("cut-comments", i))
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(c(theme::comment()))
                        .aria_label(it.comments.join(" · "))
                        .tooltip(crate::about::tip(it.comments.join("\n")))
                        .child(trn!("{n} comment", "{n} comments", n))
                        .test_support(),
                )
            });
        div()
            .id(("note", i))
            .aria_label(format!("{} · {state}", it.text.chars().take(60).collect::<String>()))
            .flex()
            .flex_col()
            .gap(px(3.))
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(if is_editing { ca(theme::accent(), 0.5) } else { ca(theme::line(), 0.) })
            .when(found == Some(&it.at), |d| d.bg(ca(theme::line(), 0.9)))
            .when(it.fresh, |d| d.bg(ca(theme::accent(), 0.05)))
            .hover(|s| s.bg(ca(theme::line(), 0.5)))
            .cursor_pointer()
            .when(it.nested, |d| d.ml(px(12.)))
            .when(!matches!(it.at, CutRef::Batch(_)), |d| d.on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone())))
            .on_click(cx.listener(move |this, e: &ClickEvent, w, cx| {
                if let CutRef::Batch(b) = &at_click {
                    if let Some(d) = &mut this.drawer {
                        if !d.unfolded_batches.remove(b) {
                            d.unfolded_batches.insert(b.clone());
                        }
                    }
                    this.select_cut(at_click.clone(), w, cx);
                } else if this.drawer.as_ref().and_then(|d| d.editing.as_ref()) == Some(&at_click) {
                } else if e.click_count() >= 2 {
                    this.edit_note(at_click.clone(), w, cx);
                } else {
                    this.select_cut(at_click.clone(), w, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_cut_menu(at_menu.clone(), trace, e.position, cx);
                }),
            )
            .child(body)
            .child(meta)
            .test_support()
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{chip_at, origin_in, paragraphs_before};

    #[test]
    fn chips_count_paragraphs() {
        let t: Vec<char> = "# Title\n\nOne two.\n\nThree four.".chars().collect();
        assert_eq!(paragraphs_before(&t, 0), 0);
        assert_eq!(chip_at(&t, 2, false), "¶1");
        assert_eq!(chip_at(&t, 12, false), "¶2");
        assert_eq!(chip_at(&t, 19, true), "after ¶2", "a gap before the third paragraph");
        assert_eq!(chip_at(&t, 0, true), "before ¶1");
        assert_eq!(origin_in("# Title\n\nOne two.\n\nThree four.", "Three"), Some((3, 19)));
    }
}
