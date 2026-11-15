use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use doc_model::Place;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::files;
use crate::format;
use crate::model::{DocModel, ModelEvent};
use crate::prefs::{Autosave, Prefs};
use crate::project;
use crate::workspace::{self, Workspace};

pub struct SharedDrawer {
    pub model: Entity<DocModel>,
    journal: Option<PathBuf>,
    journaled: u64,
    pub changed_at: Instant,
    /// Link id → the file holding the page half of that linked undo step.
    pub links: std::collections::HashMap<u64, PathBuf>,
    /// The chapters' cuts as the cuts file holds them: a chapter that loads takes its own; one
    /// that is written gives them back first.
    pub external: Vec<doc_format::ExternalCut>,
    pub external_unsaved: bool,
    /// The older `drawer.md` as read: removed once `cuts.md` holds it, unless something wrote it since.
    old_text: Option<String>,
}

/// `None` if there is no such file.
fn read_text(path: &Path) -> std::io::Result<Option<String>> {
    files::wait_for_writes(Some(path));
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| std::io::Error::other(tr!("the file is not UTF-8 text"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// `cuts.md`, or the older `drawer.md` until the app first writes (both as one if both exist).
/// Returns the cuts of no file, the chapters' cuts, and `drawer.md`'s text.
pub(crate) fn load_cuts_file(root: &Path) -> std::io::Result<(DocModel, Vec<doc_format::ExternalCut>, Option<String>)> {
    let path = project::drawer_path(root);
    let new = read_text(&path)?;
    let old = read_text(&project::old_drawer_path(root))?;
    let (mut loaded, mut external) = format::load_cuts(new.as_deref().or(old.as_deref()).unwrap_or(""));
    let both = new.is_some() && old.is_some();
    if let (true, Some(o)) = (both, &old) {
        let (l, more) = format::load_cuts(o);
        loaded.doc.absorb_notes(&l.doc);
        external.extend(more);
    }
    let mut m = DocModel::new(loaded.doc);
    m.layer = loaded.layer;
    m.present();
    m.kept = loaded.kept;
    m.warnings = loaded.warnings;
    m.path = Some(path);
    m.disk_text = new.or_else(|| old.clone());
    if both {
        // Dirty, so the two files are written as one.
        m.version += 1;
    }
    Ok((m, external, old))
}

pub enum ToPage {
    /// At the caret, as it is.
    At(usize),
    /// By the drop rule.
    Place(Place),
}

impl Workspace {
    pub(crate) fn shared_drawer(&self) -> Option<Entity<DocModel>> {
        self.project.as_ref().map(|p| p.drawer.model.clone())
    }

    /// Relative to the project, as a scrap's `source` names it.
    pub(crate) fn page_source(&self, cx: &App) -> Option<String> {
        let root = self.project_root()?;
        let path = self.model.read(cx).path.clone()?;
        relative(root, &path)
    }

    /// Returns a problem to show if the file couldn't be read.
    pub(crate) fn load_shared_drawer(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) -> (SharedDrawer, Option<String>) {
        let (m, external, old_text, problem) = match load_cuts_file(root) {
            Ok((m, external, old)) => (m, external, old, None),
            Err(e) => {
                // Unreadable: start empty with no path, so nothing overwrites it.
                let m = DocModel::new(doc_model::Document::new());
                let path = project::drawer_path(root);
                (m, Vec::new(), None, Some(tr!("Couldn't read the project's cuts ({path}): {error}", path = path.display(), error = e)))
            }
        };
        let model = cx.new(|_| m);
        self._subs.push(cx.subscribe_in(&model, window, |this, _, _: &ModelEvent, _, cx| {
            if let Some(p) = &mut this.project {
                p.drawer.changed_at = Instant::now();
            }
            cx.notify();
        }));
        let d = SharedDrawer {
            model,
            journal: workspace::new_journal(),
            journaled: 0,
            changed_at: Instant::now(),
            links: Default::default(),
            external,
            external_unsaved: false,
            old_text,
        };
        (d, problem)
    }

    /// Joins the page's and the drawer's last undo steps into one.
    pub(crate) fn link_steps(&mut self, cx: &mut Context<Self>) {
        let Some(drawer) = self.shared_drawer() else { return };
        let link = doc_model::new_link();
        self.model.update(cx, |m, _| m.doc.link_last_step(link));
        drawer.update(cx, |m, _| m.doc.link_last_step(link));
        let path = self.model.read(cx).path.clone();
        if let (Some(p), Some(path)) = (&mut self.project, path) {
            p.drawer.links.insert(link, path);
        }
    }

    /// Page text becomes the first of the page's cuts, with no trace; `copy` leaves it on the page.
    pub(crate) fn stash_range(&mut self, r: Range<usize>, copy: bool, cx: &mut Context<Self>) -> bool {
        self.model.update(cx, |m, cx| m.edit(cx, |d| if copy { d.stash_copy(r) } else { d.cut_without_trace(r).map(|_| 0) }).is_ok())
    }

    /// The page's own traceless cuts become the project's, in one undo step.
    pub(crate) fn move_file_scraps(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.model.read(cx).doc.cut_source().map(str::to_string) else { return };
        if let Ok(n) = self.model.update(cx, |m, cx| m.edit(cx, |d| d.claim_notes(&file))) {
            self.flash_status(trn!("Moved {n} cut into the project’s cuts", "Moved {n} cuts into the project’s cuts", n), cx);
        }
        cx.notify();
    }

    pub(crate) fn drawer_dirty(&self, cx: &App) -> bool {
        self.project.as_ref().is_some_and(|p| p.drawer.external_unsaved || p.drawer.model.read(cx).dirty())
    }

    pub(crate) fn chapter_cuts(&self, file: &str) -> Vec<doc_format::ExternalCut> {
        self.project.as_ref().map(|p| p.drawer.external.iter().filter(|c| c.file == file).cloned().collect()).unwrap_or_default()
    }

    /// Into the cuts file before the chapter is written without them; `false` if that write
    /// failed, and then the chapter mustn't be written.
    pub(crate) fn store_chapter_cuts(&mut self, file: &str, cuts: Vec<doc_format::ExternalCut>, cx: &mut Context<Self>) -> bool {
        let Some(p) = &mut self.project else { return true };
        if p.drawer.external.iter().filter(|c| c.file == file).eq(cuts.iter()) && !p.drawer.external_unsaved {
            return true;
        }
        p.drawer.external.retain(|c| c.file != file);
        p.drawer.external.extend(cuts);
        p.drawer.external_unsaved = true;
        self.write_drawer(cx)
    }

    /// One that never had anything leaves no file behind.
    pub(crate) fn save_drawer(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.drawer_dirty(cx) {
            return true;
        }
        self.write_drawer(cx)
    }

    fn write_drawer(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(p) = &mut self.project else { return true };
        let model = p.drawer.model.clone();
        let (path, text, version, empty) = {
            let m = model.read(cx);
            let Some(path) = m.path.clone() else { return false };
            (path, format::serialize_cuts(m, &p.drawer.external), m.version, m.doc.notes().is_empty() && p.drawer.external.is_empty())
        };
        let old = path.parent().map(project::old_drawer_path);
        if empty && !path.exists() && !old.as_ref().is_some_and(|o| o.exists()) {
            model.update(cx, |m, _| m.saved_version = version);
            p.drawer.external_unsaved = false;
            return true;
        }
        match files::write_atomic(&path, text.as_bytes()) {
            Ok(()) => {
                if Prefs::get(cx).backups {
                    let _ = files::backup(&path, text.as_bytes());
                }
                model.update(cx, |m, _| {
                    m.saved_version = version;
                    m.disk_text = Some(text);
                });
                p.drawer.external_unsaved = false;
                // `drawer.md` is now `cuts.md`, unless an older app wrote it since it was read.
                if let (Some(old), Some(was)) = (old, p.drawer.old_text.take()) {
                    if std::fs::read_to_string(&old).ok().as_deref() == Some(was.as_str()) {
                        let _ = std::fs::remove_file(&old);
                    }
                }
                if let Some(j) = &p.drawer.journal {
                    files::clear_journal(j);
                    p.drawer.journaled = 0;
                }
                true
            }
            Err(e) => {
                self.flash_status(tr!("Couldn't save the project's cuts: {error}", error = e), cx);
                false
            }
        }
    }

    pub(crate) fn autosave_drawer(&mut self, idle_only: bool, cx: &mut Context<Self>) {
        let mode = Prefs::get(cx).autosave;
        if mode == Autosave::Off || !self.drawer_dirty(cx) {
            return;
        }
        let quiet = self.project.as_ref().is_some_and(|p| p.drawer.changed_at.elapsed() > std::time::Duration::from_secs(2));
        if !idle_only || (mode == Autosave::Idle && quiet) {
            self.save_drawer(cx);
        }
    }

    pub(crate) fn journal_drawer(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &mut self.project else { return };
        let Some(j) = p.drawer.journal.clone() else { return };
        let m = p.drawer.model.read(cx);
        if !m.dirty() {
            if p.drawer.journaled != 0 {
                files::clear_journal(&j);
                p.drawer.journaled = 0;
            }
            return;
        }
        if m.version != p.drawer.journaled && files::write_journal(&j, m.path.as_deref(), &format::serialize(m)).is_ok() {
            p.drawer.journaled = m.version;
        }
    }

    pub(crate) fn clear_drawer_journal(&self) {
        if let Some(j) = self.project.as_ref().and_then(|p| p.drawer.journal.as_ref()) {
            files::clear_journal(j);
        }
    }

    pub(crate) fn drop_drawer_edits(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.project {
            p.drawer.model.update(cx, |m, _| m.saved_version = m.version);
        }
    }

    /// Unsaved changes here win and are written on the next save.
    pub(crate) fn reload_drawer_if_changed(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let model = p.drawer.model.clone();
        let m = model.read(cx);
        let Some(path) = m.path.clone() else { return };
        if m.dirty() || p.drawer.external_unsaved {
            return;
        }
        let Some(root) = path.parent().map(Path::to_path_buf) else { return };
        let on_disk = std::fs::read_to_string(&path).ok().or_else(|| std::fs::read_to_string(project::old_drawer_path(&root)).ok());
        if on_disk == m.disk_text || (on_disk.is_none() && m.disk_text.is_none()) {
            return;
        }
        let editing = self.drawer.as_ref().is_some_and(|d| d.editing.is_some());
        if editing {
            return;
        }
        let Ok((loaded, external, old)) = load_cuts_file(&root) else { return };
        if let Some(p) = &mut self.project {
            // Chapters on the page or parked keep the cuts they have; the others take these.
            p.drawer.external = external;
            p.drawer.old_text = old;
        }
        model.update(cx, |m, cx| {
            let uid = m.uid;
            *m = loaded;
            m.uid = uid;
            m.version += 1;
            m.saved_version = m.version;
            cx.emit(ModelEvent::Changed);
            cx.notify();
        });
    }

    /// All renames at once: a reorder renames a file to a name another file is leaving.
    pub(crate) fn drawer_follows_renames(&mut self, renames: &[project::Rename], cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else { return };
        let map: std::collections::HashMap<String, String> =
            renames.iter().filter_map(|r| Some((relative(&root, &r.from)?, relative(&root, &r.to)?))).collect();
        if map.is_empty() {
            return;
        }
        if let Some(p) = &mut self.project {
            for path in p.drawer.links.values_mut() {
                if let Some(r) = renames.iter().find(|r| r.from == *path) {
                    *path = r.to.clone();
                }
            }
            p.drawer.model.update(cx, |m, cx| m.rename_sources(cx, |s| map.get(s).cloned()));
            for c in &mut p.drawer.external {
                if let Some(to) = map.get(&c.file) {
                    c.file = to.clone();
                    p.drawer.external_unsaved = true;
                }
            }
            for q in p.parked.values_mut() {
                if let Some(to) = q.model.doc.cut_source().and_then(|s| map.get(s)).cloned() {
                    q.model.doc.set_cut_source(Some(to));
                }
            }
        }
        if let Some(to) = self.model.read(cx).doc.cut_source().and_then(|s| map.get(s)).cloned() {
            self.model.update(cx, |m, _| m.doc.set_cut_source(Some(to)));
        }
        self.save_drawer(cx);
    }

    /// Tells the user which file the linked undo step waits in.
    pub(crate) fn history_elsewhere(&mut self, link: u64, cx: &mut Context<Self>) {
        let name = self
            .project
            .as_ref()
            .and_then(|p| p.drawer.links.get(&link))
            .and_then(|path| self.project.as_ref().and_then(|p| p.tree.entry(path)).map(|e| e.label.clone()).or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned())));
        self.flash_status(
            match name {
                Some(n) => tr!("The last change to the project’s cuts came from {name}: undo it there", name = n),
                None => tr!("The last change to the project’s cuts came from another file: undo it there").to_string(),
            },
            cx,
        );
    }
}

/// With `/` between folders, as `drawer.md` records it.
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let r = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = r.iter().map(|c| c.to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}
