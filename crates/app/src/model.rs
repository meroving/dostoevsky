use std::ops::Range;
use std::path::PathBuf;

use doc_model::{Cycled, Document, EditError, GroupId, Removed, Scope, Source, StepTag};
use gpui_kit::{App, Context, Entity, EventEmitter};
use l10n::tr;

pub struct DocModel {
    pub doc: Document,
    /// Bumped on every change; views refresh from it.
    pub version: u64,
    pub saved_version: u64,
    pub path: Option<PathBuf>,
    pub layer: bool,
    /// The file as last read or written, for change detection.
    pub disk_text: Option<String>,
    /// What the file held that the model doesn't (detached groups, unknown records).
    pub kept: doc_format::Preserved,
    /// Where Save As proposes to save.
    pub suggested: Option<PathBuf>,
    /// Shown instead of Untitled while unsaved.
    pub name: Option<String>,
    pub warnings: Vec<String>,
    /// (index, group, version right after): ⌘Z restores the last one while nothing else changed.
    discarded: Vec<(usize, doc_format::DetachedGroup, u64)>,
    /// Stable whatever entity holds it; a cut on the clipboard names it.
    pub uid: u64,
}

#[derive(Clone, Debug)]
pub enum ModelEvent {
    Changed,
}

impl EventEmitter<ModelEvent> for DocModel {}

impl DocModel {
    pub fn new(doc: Document) -> Self {
        DocModel {
            doc,
            version: 1,
            saved_version: 1,
            path: None,
            layer: doc_format::DEFAULT_LAYER,
            disk_text: None,
            kept: Default::default(),
            suggested: None,
            name: None,
            warnings: Vec::new(),
            discarded: Vec::new(),
            uid: doc_model::new_link(),
        }
        .presented()
    }

    fn presented(mut self) -> Self {
        self.present();
        self
    }

    /// Not an edit, and a fresh cut stays faded through it. Call after the setting changes.
    pub fn present(&mut self) {
        let p = doc_model::Presentation { editing: true, look: crate::prefs::cut_look() };
        if self.doc.presentation() != p {
            self.doc.set_presentation(p);
            // The page changes, the file doesn't: a clean document stays clean.
            let clean = !self.dirty();
            self.version += 1;
            if clean {
                self.saved_version = self.version;
            }
        }
    }

    pub fn dirty(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| tr!("Untitled").into())
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.version += 1;
        cx.emit(ModelEvent::Changed);
        cx.notify();
    }

    pub fn edit<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Document) -> Result<R, EditError>,
    ) -> Result<R, EditError> {
        let r = f(&mut self.doc);
        if r.is_ok() {
            self.changed(cx);
        }
        r
    }

    pub fn replace(&mut self, cx: &mut Context<Self>, r: Range<usize>, t: &str) -> bool {
        self.edit(cx, |d| d.replace(r, t)).is_ok()
    }

    pub fn cycle(&mut self, cx: &mut Context<Self>, id: GroupId, delta: isize) -> Option<Cycled> {
        self.edit(cx, |d| d.cycle(id, delta)).ok()
    }

    pub fn set_active(&mut self, cx: &mut Context<Self>, id: GroupId, i: usize) -> Option<Cycled> {
        self.edit(cx, |d| d.set_active(id, i)).ok()
    }

    pub fn create_group(
        &mut self,
        cx: &mut Context<Self>,
        r: Range<usize>,
        scope: Scope,
    ) -> Result<GroupId, EditError> {
        self.edit(cx, |d| d.create_group(r, scope))
    }

    pub fn add_variant(&mut self, cx: &mut Context<Self>, id: GroupId, text: &str) -> bool {
        self.edit(cx, |d| d.add_variant(id, text, Source::Me)).is_ok()
    }

    pub fn add_variant_and_show(&mut self, cx: &mut Context<Self>, id: GroupId, text: &str) -> Option<Cycled> {
        self.edit(cx, |d| d.add_variant_and_show(id, text, Source::Me)).ok()
    }

    pub fn remove_variant(
        &mut self,
        cx: &mut Context<Self>,
        id: GroupId,
        i: usize,
    ) -> Option<Removed> {
        self.edit(cx, |d| d.remove_variant(id, i)).ok()
    }

    /// Undoable until the next change.
    pub fn discard_detached(&mut self, cx: &mut Context<Self>, i: usize) -> bool {
        if i >= self.kept.detached.len() {
            return false;
        }
        let g = self.kept.detached.remove(i);
        self.changed(cx);
        self.discarded.push((i, g, self.version));
        true
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) -> bool {
        if self.discarded.last().is_some_and(|(_, _, v)| *v == self.version) {
            let (i, g, _) = self.discarded.pop().expect("checked");
            let i = i.min(self.kept.detached.len());
            self.kept.detached.insert(i, g);
            self.changed(cx);
            return true;
        }
        let ok = self.doc.undo();
        if ok {
            self.changed(cx);
        }
        ok
    }

    /// A change to save, not to undo.
    pub fn rename_sources(&mut self, cx: &mut Context<Self>, map: impl Fn(&str) -> Option<String>) {
        if self.doc.rename_sources(map) {
            self.changed(cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) -> bool {
        let ok = self.doc.redo();
        if ok {
            self.changed(cx);
        }
        ok
    }

    pub fn set_layer(&mut self, cx: &mut Context<Self>, on: bool) {
        if self.layer != on {
            self.layer = on;
            self.present();
            // Saved in the `doc` record, so an edit; a page never saved still closes without asking.
            let clean = self.path.is_none() && !self.dirty();
            self.changed(cx);
            if clean {
                self.saved_version = self.version;
            }
        }
    }

    /// Cuts are never counted.
    pub fn counts(&self, range: Option<Range<usize>>) -> Counts {
        self.counted(range).1
    }

    pub fn reading(&self, range: Option<Range<usize>>) -> Option<(prose::Lang, prose::Stats)> {
        prose::reading(&self.counted(range).0)
    }

    /// The counted text (cuts that show are blanked out) and its counts.
    fn counted(&self, range: Option<Range<usize>>) -> (String, Counts) {
        let text = self.doc.text();
        let ghosts = self.doc.ghost_ranges();
        let range = range.unwrap_or(0..self.doc.len());
        let mut counted = String::new();
        let (mut gi, mut chars) = (0, 0);
        for (i, ch) in text.chars().enumerate().skip(range.start).take(range.len()) {
            while gi < ghosts.len() && ghosts[gi].end <= i {
                gi += 1;
            }
            // A cut's edge splits words, as on the page.
            let counts = !(gi < ghosts.len() && ghosts[gi].start <= i);
            counted.push(if counts { ch } else { ' ' });
            if counts && ch != '\n' {
                chars += 1;
            }
        }
        let counts = Counts {
            words: english::word_count(&counted),
            chars,
        };
        (counted, counts)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub words: usize,
    pub chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joint {
    Page,
    Drawer,
    Both,
    Nothing,
    /// Only the drawer's half is left; the other half is in this link's file, not on the page.
    Elsewhere(u64),
}

/// One history for page and drawer; a drawer step tied to a file not on the page waits for it.
pub fn joint_history(page: &Entity<DocModel>, drawer: &Entity<DocModel>, redo: bool, cx: &mut App) -> Joint {
    let tag = |m: &Entity<DocModel>, cx: &App| {
        let d = &m.read(cx).doc;
        if redo { d.redo_tag() } else { d.undo_tag() }
    };
    let (p, d): (Option<StepTag>, Option<StepTag>) = (tag(page, cx), tag(drawer, cx));
    // A linked step whose partner isn't next in the other document can't go alone.
    let alone = |t: Option<StepTag>, other: Option<StepTag>| t.filter(|t| t.link.is_none() || other.and_then(|o| o.link) == t.link);
    let (p_ok, d_ok) = (alone(p, d), alone(d, p));
    let pick = match (p_ok, d_ok) {
        (Some(a), Some(b)) if a.link.is_some() && a.link == b.link => Joint::Both,
        (Some(a), Some(b)) => {
            let page_first = if redo { a.seq <= b.seq } else { a.seq >= b.seq };
            if page_first { Joint::Page } else { Joint::Drawer }
        }
        (Some(_), None) => Joint::Page,
        (None, Some(_)) => Joint::Drawer,
        (None, None) => match d.and_then(|t| t.link) {
            Some(link) => return Joint::Elsewhere(link),
            None => return Joint::Nothing,
        },
    };
    let run = |m: &Entity<DocModel>, cx: &mut App| {
        m.update(cx, |m, cx| if redo { m.redo(cx) } else { m.undo(cx) });
    };
    match pick {
        Joint::Page => run(page, cx),
        Joint::Drawer => run(drawer, cx),
        Joint::Both => {
            run(page, cx);
            run(drawer, cx);
        }
        _ => {}
    }
    pick
}
