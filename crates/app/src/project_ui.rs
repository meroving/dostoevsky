use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::files;
use crate::format;
use crate::model::{DocModel, ModelEvent};
use crate::prefs::{Autosave, Prefs};
use crate::project::{self, Day, Kind, Rename, Tree};
use crate::sheets::group_thousands;
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::{self, PanelEdge, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Main,
    Notes,
}

pub struct Parked {
    pub model: DocModel,
    sel: Range<usize>,
    scroll: Pixels,
    journal: Option<PathBuf>,
}

impl Parked {
    pub(crate) fn fresh(model: DocModel) -> Parked {
        Parked { model, sel: 0..0, scroll: px(0.), journal: None }
    }
}

/// A document's saved state, to put back if a save fails.
struct Saved {
    version: u64,
    text: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Facts {
    stamp: SystemTime,
    pub words: usize,
    /// Line breaks left out.
    pub chars: usize,
    pub paragraphs: usize,
    pub first_line: String,
    pub reading: Option<(prose::Lang, prose::Stats)>,
}

/// `line`: asked for by a section's "+" line, and typed into where that line is.
pub enum EditTarget {
    New { dir: PathBuf, after: Option<PathBuf>, line: bool },
    Rename(PathBuf),
}

pub struct RowEdit {
    pub target: EditTarget,
    pub field: Entity<TextView>,
    _sub: Subscription,
}

pub enum FileOp {
    Renamed(Vec<Rename>),
    Trashed { from: PathBuf, to: PathBuf },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DropAt {
    Above(PathBuf),
    Below(PathBuf),
    Top(PathBuf),
}

#[derive(Clone)]
pub struct RowDrag {
    pub path: PathBuf,
    pub no: SharedString,
    pub label: SharedString,
    pub detail: SharedString,
}

impl Render for RowDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(216.))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .py(px(3.))
            .rounded(px(5.))
            .bg(c(theme::bg_raised()))
            .border_1()
            .border_color(c(theme::line()))
            .font_family(theme::UI)
            .text_size(px(13.))
            .line_height(px(19.))
            .text_color(c(theme::text()))
            .child(div().text_size(px(11.)).text_color(c(theme::accent())).child(self.no.clone()))
            .child(div().flex_1().min_w_0().truncate().child(self.label.clone()))
            .child(div().text_size(px(10.5)).text_color(c(theme::muted())).child(self.detail.clone()))
    }
}

/// Only rows near the view are real elements: every row would cost a keystroke 60 ms beside a
/// thousand journal entries.
const ROW_H: f32 = 24.;
const HEAD_H: f32 = 29.;
const OVERSCAN: f32 = 240.;

fn row_height(row: &Row) -> f32 {
    match row {
        Row::Head { .. } => HEAD_H,
        Row::File { .. } | Row::Add { .. } => ROW_H,
    }
}

#[derive(Clone, Debug)]
pub enum Row {
    /// `detail`: its words, or for the notes how many files.
    Head { title: String, dir: PathBuf, detail: Option<String>, collapsed: Option<bool> },
    /// `loose`: a file listed as not in the book (or journal).
    File { path: PathBuf, no: String, label: String, detail: String, placeholder: bool, loose: bool },
    /// Not a file, so ↑/↓ and ⌘[ ⌘] pass it by.
    Add { dir: PathBuf, after: Option<PathBuf>, what: Adds },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adds {
    Chapter,
    File,
    Note,
}

impl Adds {
    /// For the element id (`add-chapter`); the text is `line_label`.
    fn noun(self) -> &'static str {
        match self {
            Adds::Chapter => "chapter",
            Adds::File => "file",
            Adds::Note => "note",
        }
    }

    fn line_label(self) -> &'static str {
        match self {
            Adds::Chapter => tr!("+ new chapter"),
            Adds::File => tr!("+ new file"),
            Adds::Note => tr!("+ new note"),
        }
    }

    fn name_hint(self) -> &'static str {
        match self {
            Adds::Chapter => tr!("name the chapter"),
            Adds::File => tr!("name the file"),
            Adds::Note => tr!("name the note"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    List,
    Loose,
    Note,
}

pub struct ProjectWindow {
    pub tree: Tree,
    pub open: bool,
    pub focus: FocusHandle,
    pub parked: HashMap<PathBuf, Parked>,
    pub facts: HashMap<PathBuf, Facts>,
    pub collapsed: HashSet<PathBuf>,
    pub edit: Option<RowEdit>,
    pub undo: Vec<FileOp>,
    pub drop_at: Option<DropAt>,
    pub last: Option<PathBuf>,
    scroll: ScrollHandle,
    watcher: Option<crate::external::Watch>,
    pub drawer: crate::drawer::SharedDrawer,
}

fn facts_of(path: &Path, stamp: SystemTime) -> Option<Facts> {
    let text = std::fs::read_to_string(path).ok()?;
    let clean = doc_format::clean_markdown(&format::load(&text).doc);
    // A journal's day leaves out its date (PROJ-6): the row shows the day already.
    let day = path.file_stem().and_then(|s| project::parse_day(&s.to_string_lossy()));
    let mut lines = clean.lines().filter(|l| !l.trim().is_empty()).peekable();
    if lines.peek().is_some_and(|l| day.is_some_and(|d| d.is_heading(l))) {
        lines.next();
    }
    let first_line = lines
        .map(|l| l.trim_start_matches(['#', '>', '-', '*', ' ']).trim())
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string();
    let chars = clean.chars().filter(|c| *c != '\n').count();
    let reading = prose::reading(&clean);
    Some(Facts { stamp, words: english::word_count(&clean), chars, paragraphs: paragraph_count(&clean), first_line, reading })
}

pub fn paragraph_count(text: &str) -> usize {
    text.split("\n\n").filter(|b| !b.trim().is_empty()).count()
}

pub(crate) fn when(t: SystemTime) -> String {
    let d: chrono::DateTime<chrono::Local> = t.into();
    if d.date_naive() == chrono::Local::now().date_naive() {
        return tr!("today").into();
    }
    use chrono::Datelike as _;
    let day = d.day();
    match d.month() {
        1 => tr!("Jan {day}", day = day),
        2 => tr!("Feb {day}", day = day),
        3 => tr!("Mar {day}", day = day),
        4 => tr!("Apr {day}", day = day),
        5 => tr!("May {day}", day = day),
        6 => tr!("Jun {day}", day = day),
        7 => tr!("Jul {day}", day = day),
        8 => tr!("Aug {day}", day = day),
        9 => tr!("Sep {day}", day = day),
        10 => tr!("Oct {day}", day = day),
        11 => tr!("Nov {day}", day = day),
        _ => tr!("Dec {day}", day = day),
    }
}

impl ProjectWindow {
    fn words(&self, path: &Path, live: Option<(&Path, usize)>) -> Option<usize> {
        match live {
            Some((p, w)) if p == path => Some(w),
            _ => self.facts.get(path).map(|f| f.words),
        }
    }

    /// `live` is the page's own count, ahead of the disk.
    pub fn rows(&self, today: Day, live: Option<(&Path, usize)>) -> Vec<Row> {
        let groups = &self.tree.main;
        let journal = self.tree.kind == Kind::Journal;
        let plain = self.tree.kind == Kind::Folder;
        let mut rows = Vec::new();
        let today_missing = journal && self.tree.entry_for_day(today).is_none();
        if today_missing && !groups.first().and_then(|g| g.entries.first()).and_then(|e| e.day).is_some_and(|d| (d.year, d.month) == (today.year, today.month)) {
            rows.push(Row::Head { title: today.month_title(), dir: self.tree.root.clone(), detail: None, collapsed: None });
            rows.push(self.today_row(today));
        }
        for (i, g) in groups.iter().enumerate() {
            let collapsible = !journal && g.title.is_some();
            let collapsed = collapsible && self.collapsed.contains(&g.dir);
            if let Some(title) = &g.title {
                let words: Option<usize> = (!journal).then(|| g.entries.iter().filter_map(|e| self.words(&e.path, live)).sum());
                // Today's heading above already says the year.
                let title = if i == 0 && rows.len() == 2 { title.trim_end_matches(&format!(" {}", today.year)).to_string() } else { title.clone() };
                rows.push(Row::Head { title, dir: g.dir.clone(), detail: words.map(group_thousands), collapsed: collapsible.then_some(collapsed) });
            }
            if i == 0 && today_missing && rows.len() == 1 {
                rows.push(self.today_row(today));
            }
            if collapsed {
                continue;
            }
            for e in &g.entries {
                let row = if journal {
                    let label = self.facts.get(&e.path).map(|f| f.first_line.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| "…".into());
                    Row::File { path: e.path.clone(), no: e.day.map(Day::label).unwrap_or_default(), label, detail: String::new(), placeholder: false, loose: false }
                } else {
                    let detail = if plain { when(e.modified) } else { self.words(&e.path, live).map(group_thousands).unwrap_or_default() };
                    // A folder's row shows the whole name, `01-` and all: no number beside it.
                    let no = if plain { String::new() } else { e.number.map(|n| n.to_string()).unwrap_or_default() };
                    Row::File { path: e.path.clone(), no, label: e.label.clone(), detail, placeholder: false, loose: false }
                };
                rows.push(row);
            }
        }
        // A book or a folder ends with a way to add to it; a journal's today row is that already.
        if !journal {
            let (dir, after) = groups.last().map_or((self.tree.root.clone(), None), |g| (g.dir.clone(), g.entries.last().map(|e| e.path.clone())));
            rows.push(Row::Add { dir, after, what: if plain { Adds::File } else { Adds::Chapter } });
        }
        if !self.tree.loose.is_empty() {
            let dir = self.loose_key();
            let collapsed = self.collapsed.contains(&dir);
            let words: usize = self.tree.loose.iter().filter_map(|e| self.words(&e.path, live)).sum();
            let title = match self.tree.kind {
                Kind::Book => tr!("not in the book"),
                Kind::Journal => tr!("not in the journal"),
                Kind::Folder => tr!("not in the folder"),
            };
            rows.push(Row::Head { title: title.to_string(), dir, detail: Some(group_thousands(words)), collapsed: Some(collapsed) });
            if !collapsed {
                for e in &self.tree.loose {
                    let detail = self.words(&e.path, live).map(group_thousands).unwrap_or_default();
                    rows.push(Row::File { path: e.path.clone(), no: String::new(), label: e.label.clone(), detail, placeholder: false, loose: true });
                }
            }
        }
        if self.tree.notes_dir.is_some() {
            let key = self.notes_key();
            let shut = !self.collapsed.contains(&key);
            let n: usize = self.tree.notes.iter().map(|g| g.entries.len()).sum();
            let title = self.tree.notes_name().unwrap_or_default().trim_end_matches('/').to_string();
            let detail = match n {
                0 => tr!("empty").to_string(),
                n => trn!("{count} file", "{count} files", n, count = group_thousands(n)),
            };
            rows.push(Row::Head { title, dir: key, detail: Some(detail), collapsed: Some(shut) });
            if !shut {
                for g in &self.tree.notes {
                    let folded = g.title.is_some() && self.collapsed.contains(&g.dir);
                    if let Some(title) = &g.title {
                        rows.push(Row::Head { title: title.clone(), dir: g.dir.clone(), detail: Some(format!("{}", g.entries.len())), collapsed: Some(folded) });
                    }
                    if folded {
                        continue;
                    }
                    for e in &g.entries {
                        let detail = self.words(&e.path, live).map(group_thousands).unwrap_or_default();
                        rows.push(Row::File { path: e.path.clone(), no: String::new(), label: e.label.clone(), detail, placeholder: false, loose: false });
                    }
                }
                if let Some(dir) = &self.tree.notes_dir {
                    rows.push(Row::Add { dir: dir.clone(), after: None, what: Adds::Note });
                }
            }
        }
        rows
    }

    pub fn adds_in(&self, dir: &Path) -> Adds {
        if self.tree.notes_dir.as_ref().is_some_and(|n| dir.starts_with(n)) {
            Adds::Note
        } else if self.tree.kind == Kind::Book {
            Adds::Chapter
        } else {
            Adds::File
        }
    }

    /// The notes heading's key in `collapsed`, where it means *open*: notes start folded.
    fn notes_key(&self) -> PathBuf {
        self.tree.root.join(".notes-open")
    }

    /// The *not in the book* heading's key in `collapsed`: a dot name, so never a real folder.
    fn loose_key(&self) -> PathBuf {
        self.tree.root.join(".not-in-the-list")
    }

    pub fn place(&self, path: &Path) -> Place {
        if self.tree.is_note(path) {
            Place::Note
        } else if self.tree.is_loose(path) {
            Place::Loose
        } else {
            Place::List
        }
    }

    fn today_row(&self, today: Day) -> Row {
        Row::File { path: self.tree.today_path(today), no: today.label(), label: tr!("today…").into(), detail: String::new(), placeholder: true, loose: false }
    }

    fn visible_files(&self, today: Day) -> Vec<PathBuf> {
        self.rows(today, None)
            .into_iter()
            .filter_map(|r| match r {
                Row::File { path, .. } => Some(path),
                Row::Head { .. } | Row::Add { .. } => None,
            })
            .collect()
    }

    /// Folded parts too.
    fn files_in(&self, section: Section, today: Day) -> Vec<PathBuf> {
        match section {
            Section::Notes => self.tree.notes.iter().flat_map(|g| g.entries.iter()).map(|e| e.path.clone()).collect(),
            Section::Main => {
                let every: Vec<PathBuf> = self.tree.main.iter().flat_map(|g| g.entries.iter()).chain(&self.tree.loose).map(|e| e.path.clone()).collect();
                // A journal's today, not written yet, is in the list too.
                let shown: Vec<PathBuf> = self.visible_files(today).into_iter().filter(|q| !self.tree.is_note(q)).collect();
                if shown.len() > every.len() { shown } else { every }
            }
        }
    }

    /// `live` is the page's own numbers, ahead of the disk. Notes aren't counted.
    pub fn totals(&self, live: Option<(&Path, (usize, usize, usize))>) -> (usize, usize, usize) {
        let mut t = (0, 0, 0);
        for e in self.tree.main.iter().flat_map(|g| g.entries.iter()) {
            let n = match live {
                Some((p, n)) if p == e.path => Some(n),
                _ => self.facts.get(&e.path).map(|f| (f.words, f.chars, f.paragraphs)),
            };
            if let Some((w, c, p)) = n {
                t = (t.0 + w, t.1 + c, t.2 + p);
            }
        }
        t
    }

    /// Read with the formula of the language most of the words are in.
    pub fn reading_total(&self, live: Option<(&Path, Option<(prose::Lang, prose::Stats)>)>) -> Option<(prose::Lang, prose::Stats)> {
        let mut total = prose::Stats::default();
        let mut by_lang: HashMap<prose::Lang, usize> = HashMap::new();
        for e in self.tree.main.iter().flat_map(|g| g.entries.iter()) {
            let r = match live {
                Some((p, r)) if p == e.path => r,
                _ => self.facts.get(&e.path).and_then(|f| f.reading),
            };
            if let Some((lang, s)) = r {
                total += s;
                *by_lang.entry(lang).or_default() += s.words;
            }
        }
        let lang = by_lang.into_iter().max_by_key(|(_, w)| *w)?.0;
        Some((lang, total))
    }

    pub fn counts_toward_total(&self, path: &Path) -> bool {
        self.tree.main.iter().flat_map(|g| g.entries.iter()).any(|e| e.path == path)
    }

    fn summary(&self, live: Option<(&Path, usize)>) -> String {
        let groups = &self.tree.main;
        let n: usize = groups.iter().map(|g| g.entries.len()).sum();
        let words: usize = groups.iter().flat_map(|g| g.entries.iter()).filter_map(|e| self.words(&e.path, live)).sum();
        let words = trn!("{count} word", "{count} words", words, count = group_thousands(words));
        let count = group_thousands(n);
        match self.tree.kind {
            Kind::Book => format!("{words} · {}", trn!("{count} chapter", "{count} chapters", n, count = count)),
            Kind::Journal => format!("{} · {words}", trn!("{count} entry", "{count} entries", n, count = count)),
            Kind::Folder => format!("{} · {words}", trn!("{count} file", "{count} files", n, count = count)),
        }
    }

    fn remember(&self) {
        // A plain folder's choice lives here, since its folder is left untouched.
        let here = self.tree.chosen && !self.tree.root.join(project::SETTINGS_FILE).exists();
        let s = self.tree.settings();
        let memory = files::ProjectMemory {
            last: self.last.clone(),
            collapsed: self.collapsed.iter().cloned().collect(),
            panel_closed: !self.open,
            kind: here.then(|| s.kind.key().to_string()),
            notes: if here { s.notes } else { None },
        };
        files::save_project_memory(&self.tree.root, &memory);
    }
}

/// A file that doesn't exist yet (a journal's today) opens as an empty model.
pub fn load_model(path: &Path) -> std::io::Result<DocModel> {
    load_model_as(path, None)
}

pub fn load_model_as(path: &Path, chapter: Option<(&str, &[doc_format::ExternalCut])>) -> std::io::Result<DocModel> {
    // A new page left a moment ago may not be on disk yet.
    files::wait_for_writes(Some(path));
    if path.exists() {
        return crate::read_model_as(path, chapter);
    }
    let mut m = DocModel::new(doc_model::Document::new());
    m.path = Some(path.to_path_buf());
    if let Some((file, _)) = chapter {
        m.doc.set_cut_source(Some(file.to_string()));
    }
    Ok(m)
}

impl Workspace {
    pub fn project_root(&self) -> Option<&Path> {
        self.project.as_ref().map(|p| p.tree.root.as_path())
    }

    pub fn project_kind(&self) -> Option<Kind> {
        self.project.as_ref().map(|p| p.tree.kind)
    }

    pub fn holds(&self, path: &Path) -> bool {
        self.project.as_ref().is_some_and(|p| p.tree.holds(path))
    }

    /// `None` outside a project and for the cuts file itself.
    pub(crate) fn chapter_name(&self, path: &Path) -> Option<String> {
        let root = self.project_root()?;
        if project::is_cuts_file(root, path) {
            return None;
        }
        crate::drawer::relative(root, path)
    }

    pub(crate) fn load_chapter(&self, path: &Path) -> std::io::Result<DocModel> {
        match self.chapter_name(path) {
            Some(file) => {
                let cuts = self.chapter_cuts(&file);
                load_model_as(path, Some((&file, &cuts)))
            }
            None => load_model(path),
        }
    }

    pub(crate) fn load_page_text(&self, text: &str, cx: &App) -> format::Loaded {
        match self.model.read(cx).doc.cut_source().map(str::to_string) {
            Some(file) => format::load_chapter(text, &file, &self.chapter_cuts(&file)),
            None => format::load(text),
        }
    }

    /// A page with no edits yet takes its cuts from the cuts file; one with edits came from a
    /// journal, which has them.
    fn adopt_page_as_chapter(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.model.read(cx).path.clone().and_then(|p| self.chapter_name(&p)) else { return };
        let cuts = self.chapter_cuts(&file);
        self.model.update(cx, |m, cx| match m.disk_text.clone().filter(|_| !m.dirty() && !cuts.is_empty()) {
            Some(text) => {
                let l = format::load_chapter(&text, &file, &cuts);
                m.doc = l.doc;
                m.layer = l.layer;
                m.kept = l.kept;
                m.present();
                cx.emit(ModelEvent::Changed);
                cx.notify();
            }
            None => m.doc.set_cut_source(Some(file)),
        });
        self.page.update(cx, |p, cx| p.refresh(cx));
    }

    pub fn attach_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let tree = project::open(&root);
        let ask = !tree.chosen && !tree.is_empty();
        let memory = files::project_memory(&root);
        let last = self.model.read(cx).path.clone().or(memory.last);
        let (drawer, problem) = self.load_shared_drawer(&root, window, cx);
        let drawer_model = drawer.model.clone();
        self.file_panel = None;
        self.project = Some(ProjectWindow {
            tree,
            open: !memory.panel_closed,
            focus: cx.focus_handle(),
            parked: HashMap::new(),
            facts: HashMap::new(),
            collapsed: memory.collapsed.into_iter().collect(),
            edit: None,
            undo: Vec::new(),
            drop_at: None,
            last,
            scroll: ScrollHandle::new(),
            watcher: None,
            drawer,
        });
        // A note on the page opens the notes, so its row shows.
        if let (Some(p), Some(path)) = (&mut self.project, self.model.read(cx).path.clone()) {
            if p.tree.is_note(&path) {
                let key = p.notes_key();
                p.collapsed.insert(key);
            }
        }
        self.adopt_page_as_chapter(cx);
        // The page and the drawer share one history now.
        self.page.update(cx, |p, _| p.partner = Some(drawer_model));
        // An open drawer was showing the file's own scraps: show the project's.
        if self.drawer.take().is_some() {
            self.open_overflow_for_drop(window, cx);
        }
        if let Some(problem) = problem {
            self.flash_status(problem, cx);
        }
        self.watch_project(cx);
        self.count_words(cx);
        self.remember_project();
        self.quiet_journal_page(cx);
        crate::set_front_project(self.project_kind(), cx);
        if ask {
            self.ask_project_type(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn apply_settings(&mut self, s: project::Settings, cx: &mut Context<Self>) -> bool {
        let Some(root) = self.project_root().map(Path::to_path_buf) else { return false };
        if project::keeps_settings_file(&root, &s) {
            if let Err(e) = project::write_settings(&root, &s) {
                self.flash_status(tr!("Couldn't save the project’s type: {error}", error = e), cx);
                return false;
            }
        }
        let Some(p) = &mut self.project else { return false };
        p.tree = project::scan_as(&root, Some(s));
        self.remember_project();
        self.quiet_journal_page(cx);
        crate::set_front_project(self.project_kind(), cx);
        self.count_words(cx);
        cx.notify();
        true
    }

    /// A journal's page has no hint on it; a day not written yet starts with its date (PROJ-6).
    pub(crate) fn quiet_journal_page(&mut self, cx: &mut Context<Self>) {
        let quiet = self.project_kind() == Some(Kind::Journal);
        let path = self.model.read(cx).path.clone();
        let heading = path
            .filter(|p| quiet && !p.exists() && self.model.read(cx).doc.is_empty())
            .and_then(|p| {
                let day = project::parse_day(&p.file_stem()?.to_string_lossy())?;
                day.heading(project::date_heading(self.project_root()?)).map(|h| format!("# {h}\n\n"))
            });
        self.page.update(cx, |p, cx| {
            p.quiet_when_empty = quiet;
            cx.notify();
        });
        if let Some(text) = heading {
            let end = text.chars().count();
            let dated = self.model.update(cx, |m, cx| {
                let ok = m.replace(cx, 0..0, &text);
                m.doc.break_undo_group();
                ok
            });
            if dated {
                self.page.update(cx, |p, cx| {
                    p.refresh(cx);
                    p.select(end..end, cx);
                });
            }
        }
    }

    fn remember_project(&self) {
        if let Some(p) = &self.project {
            p.remember();
        }
    }

    pub fn open_enclosing_folder(&mut self, _: &OpenEnclosingFolder, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.model.read(cx).path.as_ref().and_then(|p| p.parent().map(PathBuf::from)) else { return };
        // Under the sandbox an opened file doesn't bring its folder: an open panel at the folder
        // asks for it, which grants it (and its bookmark keeps it, through the recent list).
        let denied = std::fs::read_dir(&dir).is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied);
        if denied && crate::bookmarks::sandboxed() {
            let rx = crate::platform::ask_for_folder(&dir, tr!("Allow Dostoevsky to open this folder as a project."), tr!("Open"), cx);
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(Some(chosen)) = rx.recv().await {
                    let _ = this.update_in(cx, |this, window, cx| this.open_folder_here(chosen, window, cx));
                }
            })
            .detach();
            return;
        }
        self.open_folder_here(dir, window, cx);
    }

    fn open_folder_here(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.attach_project(dir.clone(), window, cx);
        if let Some(p) = &mut self.project {
            p.open = true;
        }
        self.remember_project();
        files::add_recent(&dir);
        crate::refresh_menus(cx);
    }

    /// The open file has its own watcher (`external.rs`), which also reloads it.
    fn watch_project(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else { return };
        let (tx, rx) = smol::channel::unbounded::<()>();
        let start = crate::external::start_watcher(root.clone(), notify::RecursiveMode::Recursive, move |e| {
            let hidden = |p: &PathBuf| p.iter().any(|c| c.to_string_lossy().starts_with('.') && c != "." && c != "..");
            if e.paths.iter().any(|p| !hidden(p)) {
                let _ = tx.try_send(());
            }
        });
        cx.spawn(async move |this, cx| {
            let Some(watch) = start.await else { return };
            let current = this.update(cx, |this, cx| {
                // The window moved to another project while this watcher started.
                if this.project_root() != Some(root.as_path()) {
                    return false;
                }
                if let Some(p) = &mut this.project {
                    p.watcher = Some(watch);
                }
                // Files added or renamed while fseventsd was slow to answer.
                this.rescan(cx);
                true
            });
            if !matches!(current, Ok(true)) {
                return;
            }
            while rx.recv().await.is_ok() {
                cx.background_executor().timer(Duration::from_millis(300)).await;
                while rx.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.rescan(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.reload_drawer_if_changed(cx);
        let Some(p) = &mut self.project else { return };
        // A choice kept in Application Support (a plain folder) stays with the tree.
        let settings = project::read_settings(&p.tree.root).or_else(|| p.tree.chosen.then(|| p.tree.settings()));
        let tree = project::scan_as(&p.tree.root, settings);
        if tree == p.tree {
            return;
        }
        let kind_changed = tree.kind != p.tree.kind;
        p.tree = tree;
        if kind_changed {
            self.quiet_journal_page(cx);
            crate::set_front_project(self.project_kind(), cx);
        }
        self.count_words(cx);
        cx.notify();
    }

    /// Off the UI thread, so a thousand journal entries don't slow the window down.
    fn count_words(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let stale: Vec<(PathBuf, SystemTime)> = p
            .tree
            .all()
            .filter(|e| p.facts.get(&e.path).is_none_or(|f| f.stamp != e.modified))
            .map(|e| (e.path.clone(), e.modified))
            .collect();
        if stale.is_empty() {
            return;
        }
        let work = cx.background_spawn(async move {
            stale.into_iter().filter_map(|(path, stamp)| Some((path.clone(), facts_of(&path, stamp)?))).collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let done = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(p) = &mut this.project {
                    p.facts.extend(done);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// False if it stayed: the file couldn't be read, or the page couldn't be saved.
    pub fn switch_to(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.model.read(cx).path.as_deref() == Some(path) {
            return true;
        }
        if self.project_root().is_some_and(|r| project::is_cuts_file(r, path)) {
            // drawer.md is the drawer, not a page.
            self.open_overflow_for_drop(window, cx);
            return false;
        }
        if crate::bring_forward_other_window(path, cx.entity_id(), cx) {
            return false;
        }
        let parked = self.project.as_mut().and_then(|p| p.parked.remove(path));
        let incoming = match parked {
            Some(p) => p,
            None => match self.load_chapter(path) {
                Ok(m) => Parked::fresh(m),
                Err(e) => {
                    self.flash_status(tr!("Couldn't open {path}: {error}", path = path.display(), error = e), cx);
                    return false;
                }
            },
        };
        if !self.leave_page(window, cx) {
            if let Some(p) = &mut self.project {
                p.parked.insert(path.to_path_buf(), incoming);
            }
            return false;
        }
        self.install(incoming, window, cx);
        true
    }

    /// Autosave off keeps it unsaved, parked with its journal.
    fn leave_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.end_variant_edit(true, cx);
        self.sidebar = None;
        // Only when one is open: stopping gives the page the keyboard, and ↑/↓ needs it here.
        if self.drawer.as_ref().is_some_and(|d| d.editing.is_some()) {
            self.stop_editing_note(window, cx);
        }
        self.context_menu = None;
        self.pending_disk = None;
        self.autosave_drawer(false, cx);
        let m = self.model.read(cx);
        if !m.dirty() {
            return true;
        }
        if m.path.is_none() {
            // Words typed before there was a file get one, not a dialog in the way.
            let Some(root) = self.project_root().map(Path::to_path_buf) else { return false };
            let path = project::unused(&root.join("untitled.md"));
            self.model.update(cx, |m, _| m.path = Some(path));
            return self.save_to_current(window, cx);
        }
        if Prefs::get(cx).autosave == Autosave::Off {
            return true;
        }
        self.save_on_leave(window, cx)
    }

    /// The write follows on the writer thread (`files::write_later`), which clears the journal
    /// once on disk; should it fail, the file is unsaved again, with a journal, and an alert says so.
    fn save_on_leave(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (path, (text, cuts), version, before) = {
            let m = self.model.read(cx);
            let Some(p) = m.path.clone() else { return true };
            (p, format::serialize_for_disk(m), m.version, Saved { version: m.saved_version, text: m.disk_text.clone() })
        };
        if !self.may_write_over_file(cx) {
            return false;
        }
        // The cuts go to the cuts file first, and now: never a moment without them on disk.
        if let Some((file, cuts)) = cuts {
            if !self.store_chapter_cuts(&file, cuts, cx) {
                return false;
            }
        }
        // The journal holds these edits until they're on disk; the next page gets its own.
        let journal = if self.journaled_version != 0 { self.journal.take() } else { None };
        self.journaled_version = 0;
        let done = files::write_later(&path, text.clone().into_bytes(), Prefs::get(cx).backups, journal.clone());
        self.model.update(cx, |m, _| {
            m.saved_version = version;
            m.disk_text = Some(text);
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = done.recv().await.unwrap_or_else(|_| Err(std::io::ErrorKind::Interrupted.into()));
            if let Err(e) = result {
                let _ = this.update_in(cx, |this, window, cx| this.save_on_leave_failed(&path, version, before, journal, &e, window, cx));
            }
        })
        .detach();
        true
    }

    /// The file is unsaved again unless a later save has gone through; its words get a journal
    /// again, and where the page is gone the old one stays for recovery.
    #[allow(clippy::too_many_arguments)]
    fn save_on_leave_failed(
        &mut self,
        path: &Path,
        version: u64,
        before: Saved,
        journal: Option<PathBuf>,
        e: &std::io::Error,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let unsave = |m: &mut DocModel| {
            if m.saved_version == version {
                m.saved_version = before.version;
                m.disk_text = before.text;
            }
        };
        if self.model.read(cx).path.as_deref() == Some(path) {
            // Back on the page: its own journal takes the edits from here.
            self.model.update(cx, |m, _| unsave(m));
            self.write_journal(cx);
            if let Some(j) = journal.filter(|j| self.journal.as_ref() != Some(j)) {
                files::clear_journal(&j);
            }
            self.update_title(window, cx);
        } else if let Some(parked) = self.project.as_mut().and_then(|p| p.parked.get_mut(path)) {
            unsave(&mut parked.model);
            if parked.model.dirty() {
                let j = match (parked.journal.take(), journal) {
                    (Some(mine), Some(old)) => {
                        files::clear_journal(&old);
                        Some(mine)
                    }
                    (mine, old) => mine.or(old).or_else(workspace::new_journal),
                };
                if let Some(j) = &j {
                    let _ = files::write_journal(j, Some(path), &format::serialize(&parked.model));
                }
                parked.journal = j;
            }
        }
        let msg = tr!("Couldn't save {path}", path = path.display());
        drop(window.prompt(PromptLevel::Critical, &msg, Some(&e.to_string()), &[tr!("OK")], cx));
        cx.notify();
    }

    fn install(&mut self, incoming: Parked, window: &mut Window, cx: &mut Context<Self>) {
        let page = self.page.read(cx);
        let (sel, scroll) = (page.selection(), page.scroll_offset());
        let Parked { model: mut swapped, sel: new_sel, scroll: new_scroll, journal: new_journal } = incoming;
        let left_dirty = self.model.read(cx).dirty();
        if left_dirty {
            self.write_journal(cx);
        }
        self.model.update(cx, |m, cx| {
            std::mem::swap(m, &mut swapped);
            cx.emit(ModelEvent::Changed);
            cx.notify();
        });
        let left_journal = if left_dirty { self.journal.take() } else { None };
        self.journal = new_journal.or_else(|| self.journal.take()).or_else(workspace::new_journal);
        self.journaled_version = if self.model.read(cx).dirty() { self.model.read(cx).version } else { 0 };
        // An untouched page with no file leaves nothing behind; one whose first save is still
        // on its way stays, in case the save fails.
        let keep = swapped.path.as_ref().is_some_and(|p| p.exists() || files::write_queued(p)) || left_dirty;
        let left_path = swapped.path.clone();
        if let (true, Some(path), Some(p)) = (keep, left_path, &mut self.project) {
            p.parked.insert(path, Parked { model: swapped, sel, scroll, journal: left_journal });
        }
        self.page.update(cx, |p, cx| p.document_swapped(new_sel, new_scroll, cx));
        self.quiet_journal_page(cx);
        self.update_title(window, cx);
        self.watch_file(cx);
        let warnings = self.model.update(cx, |m, _| std::mem::take(&mut m.warnings));
        if let Some(w) = warnings.first() {
            self.flash_status(w.clone(), cx);
        }
        // It may have changed while it was parked; the watcher only follows the open file.
        if self.model.read(cx).path.as_ref().is_some_and(|p| p.exists()) && self.model.read(cx).disk_text.is_some() {
            self.disk_changed(cx);
        }
        let path = self.model.read(cx).path.clone();
        if let Some(p) = &mut self.project {
            // A note on the page opens the notes, so its row shows.
            if path.as_deref().is_some_and(|q| p.tree.is_note(q)) {
                let key = p.notes_key();
                p.collapsed.insert(key);
            }
            p.last = path;
        }
        self.remember_project();
        self.scroll_list_to_page(cx);
        cx.notify();
    }

    fn scroll_list_to_page(&mut self, cx: &mut Context<Self>) {
        let path = self.model.read(cx).path.clone();
        let Some(p) = &self.project else { return };
        let rows = p.rows(Day::today(), None);
        let Some(i) = rows.iter().position(|r| matches!(r, Row::File { path: q, .. } if Some(q) == path.as_ref())) else { return };
        let y: f32 = rows[..i].iter().map(row_height).sum();
        let view = f32::from(p.scroll.bounds().size.height);
        let shown = -f32::from(p.scroll.offset().y);
        let to = if y < shown { y } else if y + ROW_H > shown + view && view > 0. { y + ROW_H - view } else { shown };
        p.scroll.set_offset(point(px(0.), px(-to)));
    }

    fn step_in(&mut self, list: Vec<PathBuf>, delta: isize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let path = self.model.read(cx).path.clone();
        let next = match path.as_ref().and_then(|q| list.iter().position(|f| f == q)) {
            Some(i) => i.checked_add_signed(delta).and_then(|i| list.get(i)),
            None => list.first(),
        };
        match next.cloned() {
            Some(n) => self.switch_to(&n, window, cx),
            None => false,
        }
    }

    fn list_up(&mut self, _: &ListUp, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.project.as_ref().map(|p| p.visible_files(Day::today())).unwrap_or_default();
        self.step_in(list, -1, window, cx);
    }

    fn list_down(&mut self, _: &ListDown, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.project.as_ref().map(|p| p.visible_files(Day::today())).unwrap_or_default();
        self.step_in(list, 1, window, cx);
    }

    fn page_section(&self, cx: &App) -> Section {
        let note = self.model.read(cx).path.as_ref().is_some_and(|q| self.project.as_ref().is_some_and(|p| p.tree.is_note(q)));
        if note { Section::Notes } else { Section::Main }
    }

    /// A journal goes by the calendar, so Next Day is the row above.
    fn calendar_sign(&self) -> isize {
        if self.project_kind() == Some(Kind::Journal) { -1 } else { 1 }
    }

    pub fn previous_file(&mut self, _: &PreviousFile, window: &mut Window, cx: &mut Context<Self>) {
        let (section, sign) = (self.page_section(cx), self.calendar_sign());
        let list = self.project.as_ref().map(|p| p.files_in(section, Day::today())).unwrap_or_default();
        if !self.step_in(list, -sign, window, cx) {
            self.flash_status(tr!("That’s the first one"), cx);
        }
    }

    pub fn next_file(&mut self, _: &NextFile, window: &mut Window, cx: &mut Context<Self>) {
        let (section, sign) = (self.page_section(cx), self.calendar_sign());
        let list = self.project.as_ref().map(|p| p.files_in(section, Day::today())).unwrap_or_default();
        if !self.step_in(list, sign, window, cx) {
            self.flash_status(tr!("That’s the last one"), cx);
        }
    }

    pub fn panel_open(&self) -> bool {
        self.project.as_ref().map_or(self.file_panel.is_some(), |p| p.open)
    }

    fn focus_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.project.as_ref().map(|p| p.focus.clone()) {
            window.focus(&f, cx);
        }
    }

    pub(crate) fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
    }

    pub fn toggle_project(&mut self, _: &ToggleProject, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.project else { return };
        if p.open && p.focus.is_focused(window) {
            self.close_panel(window, cx);
            return;
        }
        p.open = true;
        self.lab.open = false;
        if self.sidebar.is_some() {
            self.close_sidebar(window, cx);
        }
        self.remember_project();
        self.focus_list(window, cx);
        self.scroll_list_to_page(cx);
        cx.notify();
    }

    /// The Project tab, with the keyboard left where it is (the panels a mode puts back).
    pub(crate) fn show_project_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.project else { return self.open_file_panel(window, cx) };
        p.open = true;
        self.lab.open = false;
        if self.sidebar.is_some() {
            self.close_sidebar(window, cx);
        }
        self.remember_project();
        self.scroll_list_to_page(cx);
        cx.notify();
    }

    pub fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_row_edit(cx);
        if let Some(p) = &mut self.project {
            p.open = false;
        }
        self.file_panel = None;
        self.remember_project();
        self.focus_page(window, cx);
        cx.notify();
    }

    fn list_return(&mut self, _: &ListReturn, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_page(window, cx);
        cx.notify();
    }

    fn list_close(&mut self, _: &ListClose, window: &mut Window, cx: &mut Context<Self>) {
        self.close_panel(window, cx);
    }

    fn toggle_part(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(p) = &mut self.project else { return };
        if !p.collapsed.remove(&dir) {
            p.collapsed.insert(dir);
        }
        self.remember_project();
        cx.notify();
    }

    pub fn new_in_project(&mut self, _: &NewDocument, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.project else { return };
        p.open = true;
        let root = p.tree.root.clone();
        match p.tree.kind {
            Kind::Journal => {
                let today = Day::today();
                let path = p.tree.entry_for_day(today).map_or_else(|| p.tree.today_path(today), |e| e.path.clone());
                self.switch_to(&path, window, cx);
                self.focus_page(window, cx);
            }
            Kind::Book => {
                let cur = self.model.read(cx).path.clone();
                let group = cur.as_ref().and_then(|c| p.tree.main.iter().find(|g| g.entries.iter().any(|e| e.path == *c)));
                let (dir, after) = match (group, cur.as_ref()) {
                    (Some(g), Some(c)) => (g.dir.clone(), Some(c.clone())),
                    _ => match p.tree.main.last() {
                        Some(g) => (g.dir.clone(), g.entries.last().map(|e| e.path.clone())),
                        None => (root, None),
                    },
                };
                self.begin_row_edit(EditTarget::New { dir, after, line: false }, "", window, cx);
            }
            Kind::Folder => self.begin_row_edit(EditTarget::New { dir: root, after: None, line: false }, "", window, cx),
        }
    }

    pub(crate) fn begin_row_edit(&mut self, target: EditTarget, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.end_row_edit(cx);
        let model = self.model.clone();
        let hint = match (&target, &self.project) {
            (EditTarget::New { dir, .. }, Some(p)) => p.adds_in(dir).name_hint().to_string(),
            _ => String::new(),
        };
        let field = cx.new(|cx| {
            let mut f = TextView::new(Mode::Field, model, cx);
            f.placeholder = hint.into();
            f.set_field_text(text, cx);
            let n = f.index().len();
            f.select(0..n, cx);
            f
        });
        let focus = field.read(cx).focus.clone();
        let sub = cx.subscribe_in(&field, window, Self::on_row_field);
        if let Some(p) = &mut self.project {
            p.open = true;
            p.edit = Some(RowEdit { target, field, _sub: sub });
        }
        window.focus(&focus, cx);
        cx.notify();
    }

    fn end_row_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.project {
            if p.edit.take().is_some() {
                cx.notify();
            }
        }
    }

    fn on_row_field(&mut self, field: &Entity<TextView>, e: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            ViewEvent::Confirm => {
                let name = field.read(cx).field_text().trim().to_string();
                self.commit_row_edit(&name, window, cx);
            }
            ViewEvent::Cancel => {
                self.end_row_edit(cx);
                self.focus_list(window, cx);
            }
            ViewEvent::Edited => cx.notify(),
            _ => {}
        }
    }

    fn commit_row_edit(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.project.as_mut().and_then(|p| p.edit.take()) else { return };
        match edit.target {
            EditTarget::New { dir, after, .. } => self.create_file(&dir, after.as_deref(), name, window, cx),
            EditTarget::Rename(path) => {
                if !name.is_empty() {
                    self.rename_file(&path, name, window, cx);
                }
                self.focus_list(window, cx);
            }
        }
        cx.notify();
    }

    fn create_file(&mut self, dir: &Path, after: Option<&Path>, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let (path, room) = if p.adds_in(dir) == Adds::Chapter {
            project::plan_new_chapter(&p.tree, dir, after, name)
        } else {
            (project::unused(&dir.join(format!("{}.md", project::slug(name)))), Vec::new())
        };
        // The notes folder may not be there yet: Move to Notes makes it too.
        if let Err(e) = std::fs::create_dir_all(dir) {
            self.flash_status(tr!("Couldn't create {path}: {error}", path = dir.display(), error = e), cx);
            return;
        }
        if !self.apply_renames(room, false, window, cx) {
            return;
        }
        // The rename may have just moved the file the row was to come after.
        if let Err(e) = std::fs::write(&path, "") {
            self.flash_status(tr!("Couldn't create {path}: {error}", path = path.display(), error = e), cx);
            return;
        }
        self.rescan(cx);
        if self.switch_to(&path, window, cx) {
            self.title_new_page(name, cx);
            self.focus_page(window, cx);
        }
    }

    /// An edit, not part of the file as made, so one ⌘Z takes it away.
    fn title_new_page(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim().trim_start_matches('#').trim();
        if name.is_empty() || !self.model.read(cx).doc.is_empty() {
            return;
        }
        let text = format!("# {name}\n\n");
        let titled = self.model.update(cx, |m, cx| {
            let ok = m.replace(cx, 0..0, &text);
            m.doc.break_undo_group();
            ok
        });
        if titled {
            let end = text.chars().count();
            self.page.update(cx, |v, cx| {
                v.refresh(cx);
                v.select(end..end, cx);
            });
        }
    }

    fn start_with(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else { return };
        let notes = self.project.as_ref().and_then(|p| p.tree.settings().notes);
        if !self.apply_settings(project::Settings { kind, notes, title: None, author: None, date: None }, cx) {
            return;
        }
        if kind == Kind::Journal {
            let path = self.project.as_ref().map_or_else(|| root.join(Day::today().file_name()), |p| p.tree.today_path(Day::today()));
            self.switch_to(&path, window, cx);
            self.focus_page(window, cx);
            return;
        }
        let name = "01-untitled.md".to_string();
        let path = project::unused(&root.join(name));
        if let Err(e) = std::fs::write(&path, "") {
            self.flash_status(tr!("Couldn't create {path}: {error}", path = path.display(), error = e), cx);
            return;
        }
        self.rescan(cx);
        if self.switch_to(&path, window, cx) {
            self.focus_page(window, cx);
        }
    }

    pub(crate) fn move_to_notes(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let dir = match &p.tree.notes_dir {
            Some(d) => d.clone(),
            None => {
                let s = project::Settings { kind: p.tree.kind, notes: Some("notes".into()), title: None, author: None, date: None };
                if !self.apply_settings(s, cx) {
                    return;
                }
                self.project_root().map(|r| r.join("notes")).unwrap_or_default()
            }
        };
        let Some(p) = &self.project else { return };
        let r = project::plan_to_notes(&p.tree, path, &dir);
        let name = p.tree.notes_name().unwrap_or_else(|| "notes/".into());
        if self.apply_renames(vec![r], true, window, cx) {
            self.flash_status(tr!("Moved to {name}. ⌘Z to undo", name = name), cx);
        }
    }

    pub(crate) fn add_to_list(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let r = project::plan_to_list(&p.tree, path);
        if r.to == r.from {
            return;
        }
        let done = match p.tree.kind {
            Kind::Book => tr!("Added to the book. ⌘Z to undo"),
            Kind::Journal => tr!("Added to the journal. ⌘Z to undo"),
            Kind::Folder => tr!("Added to the folder. ⌘Z to undo"),
        };
        if self.apply_renames(vec![r], true, window, cx) {
            self.flash_status(done, cx);
        }
    }

    pub(crate) fn rename_file(&mut self, path: &Path, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let to = project::renamed(&p.tree, path, name);
        if to == path {
            return;
        }
        self.apply_renames(vec![Rename { from: path.to_path_buf(), to }], true, window, cx);
    }

    /// Also the open document, what's parked, the backups and the list. `undoable` puts it on the list's ⌘Z.
    pub(crate) fn apply_renames(&mut self, renames: Vec<Rename>, undoable: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if renames.is_empty() {
            return true;
        }
        // What's on disk under the old names has to be current before they change.
        if self.model.read(cx).dirty() && self.model.read(cx).path.is_some() && !self.save_to_current(window, cx) {
            return false;
        }
        if let Err(e) = project::apply(&renames) {
            self.flash_status(tr!("Nothing was moved: {error}", error = e), cx);
            return false;
        }
        self.files_renamed(&renames, window, cx);
        if undoable {
            if let Some(p) = &mut self.project {
                p.undo.push(FileOp::Renamed(renames));
            }
        }
        true
    }

    fn files_renamed(&mut self, renames: &[Rename], window: &mut Window, cx: &mut Context<Self>) {
        self.drawer_follows_renames(renames, cx);
        for r in renames {
            files::file_moved(&r.from, &r.to);
            if self.model.read(cx).path.as_deref() == Some(r.from.as_path()) {
                self.model.update(cx, |m, _| m.path = Some(r.to.clone()));
            }
            if let Some(p) = &mut self.project {
                if let Some(mut parked) = p.parked.remove(&r.from) {
                    parked.model.path = Some(r.to.clone());
                    if let Some(j) = &parked.journal {
                        let _ = files::write_journal(j, Some(&r.to), &format::serialize(&parked.model));
                    }
                    p.parked.insert(r.to.clone(), parked);
                }
                if let Some(f) = p.facts.remove(&r.from) {
                    p.facts.insert(r.to.clone(), f);
                }
                if p.last.as_deref() == Some(r.from.as_path()) {
                    p.last = Some(r.to.clone());
                }
            }
        }
        // Force the next journal write to carry the new path.
        self.journaled_version = 0;
        self.watch_file(cx);
        self.update_title(window, cx);
        self.remember_project();
        self.rescan(cx);
        // Another window may have one of these open on its own.
        let renames = renames.to_vec();
        cx.defer(move |cx| crate::files_moved(&renames, cx));
    }

    pub(crate) fn trash_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let on_page = self.model.read(cx).path.as_deref() == Some(path);
        if on_page && self.model.read(cx).dirty() && !self.save_to_current(window, cx) {
            return;
        }
        let Some(p) = &self.project else { return };
        let today = Day::today();
        let section = if p.tree.is_note(path) { Section::Notes } else { Section::Main };
        let list = p.files_in(section, today);
        let at = list.iter().position(|f| f == path);
        let next = at.and_then(|i| list.get(i + 1).or(i.checked_sub(1).and_then(|i| list.get(i)))).cloned();
        // A save on its way would bring the file back after it's gone.
        files::wait_for_writes(Some(path));
        let to = match crate::platform::trash(path) {
            Ok(to) => to,
            Err(e) => {
                self.flash_status(tr!("Couldn't move it to the Trash: {error}", error = e), cx);
                return;
            }
        };
        if let Some(p) = &mut self.project {
            if let Some(parked) = p.parked.remove(path) {
                if let Some(j) = &parked.journal {
                    files::clear_journal(j);
                }
            }
            p.undo.push(FileOp::Trashed { from: path.to_path_buf(), to });
        }
        self.rescan(cx);
        if on_page {
            // The file has gone, so there's nothing to save and nothing to park.
            self.model.update(cx, |m, _| {
                m.path = None;
                m.saved_version = m.version;
            });
            match next {
                Some(n) => {
                    self.switch_to(&n, window, cx);
                }
                None => {
                    self.install(Parked::fresh(DocModel::new(doc_model::Document::new())), window, cx);
                }
            }
        }
        self.focus_list(window, cx);
        self.flash_status(tr!("Moved to the Trash. ⌘Z to undo"), cx);
    }

    pub fn list_trash(&mut self, _: &ListTrash, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.model.read(cx).path.clone() {
            if path.exists() {
                self.trash_file(&path, window, cx);
            }
        }
    }

    /// With nothing left it's the page's undo.
    fn list_undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        match self.project.as_mut().and_then(|p| p.undo.pop()) {
            Some(FileOp::Renamed(renames)) => {
                let back = project::reversed(&renames);
                match project::apply(&back) {
                    Ok(()) => self.files_renamed(&back, window, cx),
                    Err(e) => self.flash_status(tr!("Couldn't undo: {error}", error = e), cx),
                }
            }
            Some(FileOp::Trashed { from, to }) => match std::fs::rename(&to, &from) {
                Ok(()) => {
                    self.rescan(cx);
                    self.switch_to(&from, window, cx);
                    self.flash_status(tr!("Brought back from the Trash"), cx);
                }
                Err(e) => self.flash_status(tr!("Couldn't bring it back: {error}", error = e), cx),
            },
            None => self.page.update(cx, |p, cx| p.undo(cx)),
        }
    }

    fn resolve_drop(&self, at: &DropAt) -> Option<(PathBuf, Option<PathBuf>)> {
        let p = self.project.as_ref()?;
        let group_of = |path: &Path| p.tree.main.iter().find(|g| g.entries.iter().any(|e| e.path == path));
        match at {
            DropAt::Above(q) => Some((group_of(q)?.dir.clone(), Some(q.clone()))),
            DropAt::Below(q) => {
                let g = group_of(q)?;
                let i = g.entries.iter().position(|e| e.path == *q)?;
                Some((g.dir.clone(), g.entries.get(i + 1).map(|e| e.path.clone())))
            }
            DropAt::Top(dir) => {
                let g = p.tree.main.iter().find(|g| g.dir == *dir)?;
                Some((g.dir.clone(), g.entries.first().map(|e| e.path.clone())))
            }
        }
    }

    pub(crate) fn drop_row(&mut self, d: &RowDrag, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.project.as_mut().and_then(|p| p.drop_at.take()) else { return };
        let Some((dir, before)) = self.resolve_drop(&at) else { return };
        self.focus_list(window, cx);
        let Some(p) = &self.project else { return };
        match p.tree.kind {
            Kind::Book => {
                let plan = project::plan_move(&p.tree, &d.path, &dir, before.as_deref());
                self.apply_renames(plan, true, window, cx);
            }
            Kind::Folder => {
                let Some(g) = p.tree.main.iter().find(|g| g.dir == dir) else { return };
                let mut order: Vec<PathBuf> = g.entries.iter().map(|e| e.path.clone()).filter(|q| *q != d.path).collect();
                let i = before.and_then(|b| order.iter().position(|q| *q == b)).unwrap_or(order.len());
                order.insert(i, d.path.clone());
                let plan = project::plan_numbering(&order);
                let rx = window.prompt(
                    PromptLevel::Info,
                    tr!("Number these files so the order sticks?"),
                    Some(tr!("The order lives in the file names. Numbering them makes this folder a book. ⌘Z undoes the renames.")),
                    &[tr!("Number Files"), tr!("Cancel")],
                    cx,
                );
                let notes = p.tree.settings().notes;
                cx.spawn_in(window, async move |this, cx| {
                    if rx.await == Ok(0) {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if this.apply_renames(plan, true, window, cx) {
                                this.apply_settings(project::Settings { kind: Kind::Book, notes, title: None, author: None, date: None }, cx);
                            }
                        });
                    }
                })
                .detach();
            }
            Kind::Journal => {}
        }
        cx.notify();
    }

    pub(crate) fn clear_drop(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.project {
            if p.drop_at.take().is_some() {
                cx.notify();
            }
        }
    }

    fn open_row_menu(&mut self, path: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let (kind, place) = (p.tree.kind, p.place(&path));
        self.context_menu = Some(crate::context_menu::ContextMenu::project_row(path, kind, place, at));
        cx.notify();
    }

    pub(crate) fn open_row_in_window(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.model.read(cx).path.as_deref() == Some(path) {
            self.flash_status(tr!("That file is open here"), cx);
            return;
        }
        if self.project.as_ref().is_some_and(|p| p.parked.get(path).is_some_and(|q| q.model.dirty())) {
            self.flash_status(tr!("That file has unsaved changes here. Save it first"), cx);
            return;
        }
        if let Some(p) = &mut self.project {
            p.parked.remove(path);
        }
        crate::open_separately(path.to_path_buf(), cx);
    }

    /// Parked files and the project's drawer.
    pub(crate) fn parked_unsaved(&self, cx: &App) -> usize {
        self.project.as_ref().map_or(0, |p| p.parked.values().filter(|q| q.model.dirty()).count()) + usize::from(self.drawer_dirty(cx))
    }

    pub(crate) fn save_parked(&mut self, cx: &mut Context<Self>) {
        self.save_drawer(cx);
        let backups = Prefs::get(cx).backups;
        let Some(p) = &self.project else { return };
        let dirty: Vec<PathBuf> = p.parked.iter().filter(|(_, q)| q.model.dirty()).map(|(path, _)| path.clone()).collect();
        for path in dirty {
            let Some(q) = self.project.as_ref().and_then(|p| p.parked.get(&path)) else { continue };
            let (text, cuts) = format::serialize_for_disk(&q.model);
            // Its cuts are on disk before the chapter is written without them.
            if let Some((file, cuts)) = cuts {
                if !self.store_chapter_cuts(&file, cuts, cx) {
                    continue;
                }
            }
            let Some(q) = self.project.as_mut().and_then(|p| p.parked.get_mut(&path)) else { continue };
            let path = &path;
            if files::write_atomic(path, text.as_bytes()).is_ok() {
                if backups {
                    let _ = files::backup(path, text.as_bytes());
                }
                q.model.saved_version = q.model.version;
                q.model.disk_text = Some(text);
            }
        }
    }

    pub(crate) fn drop_parked_edits(&mut self, cx: &mut Context<Self>) {
        self.drop_drawer_edits(cx);
        if let Some(p) = &mut self.project {
            for q in p.parked.values_mut() {
                q.model.saved_version = q.model.version;
            }
        }
    }

    pub(crate) fn clear_parked_journals(&mut self) {
        self.clear_drawer_journal();
        if let Some(p) = &self.project {
            for j in p.parked.values().filter_map(|q| q.journal.as_ref()) {
                files::clear_journal(j);
            }
        }
    }

    pub(crate) fn project_name(&self) -> Option<String> {
        self.project_root().map(project::folder_name)
    }

    pub fn render_project(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.sidebar.is_some() || self.lab.open {
            return None;
        }
        let p = self.project.as_ref()?;
        if !p.open {
            return None;
        }
        let m = self.model.read(cx);
        let current = m.path.clone();
        let live_words = m.counts(None).words;
        let live = current.as_deref().map(|q| (q, live_words));
        let today = Day::today();
        let empty = p.tree.is_empty();
        let kind = p.tree.kind;
        let rows = p.rows(today, live);
        let summary = p.summary(live);
        let notes: HashSet<PathBuf> = p.tree.notes.iter().flat_map(|g| g.entries.iter()).map(|e| e.path.clone()).collect();
        let notes_key = p.tree.notes_dir.is_some().then(|| p.notes_key());
        let drop_at = if cx.has_active_drag() { p.drop_at.clone() } else { None };
        let draggable = matches!(kind, Kind::Book | Kind::Folder);
        let editing = p.edit.as_ref().map(|e| match &e.target {
            EditTarget::New { dir, after, line } => (Some((dir.clone(), after.clone(), *line)), None, e.field.clone()),
            EditTarget::Rename(q) => (None, Some(q.clone()), e.field.clone()),
        });
        let title = project::spoken(&project::folder_name(&p.tree.root));
        let kind_label = format!("{} ▾", kind.word());
        let focus = p.focus.clone();
        let scroll = p.scroll.clone();

        let line = |above: bool| {
            div().absolute().left_0().right_0().h(px(2.)).rounded(px(1.)).bg(c(theme::accent())).map(|d| if above { d.top(px(-1.)) } else { d.bottom(px(-1.)) })
        };
        let mut items: Vec<AnyElement> = Vec::new();
        let mut placed = false;
        let shown = -f32::from(scroll.offset().y);
        let view = f32::from(scroll.bounds().size.height).max(f32::from(window.viewport_size().height));
        let (mut y, mut above, mut below) = (0., 0., 0.);
        for row in rows {
            let h = row_height(&row);
            let near = y + h >= shown - OVERSCAN && y <= shown + view + OVERSCAN;
            // A row being typed into, or the one a new row follows (or the "+" line it replaces),
            // is built wherever it is.
            let anchor = match &row {
                Row::File { path, .. } => editing.as_ref().is_some_and(|(n, r, _)| r.as_ref() == Some(path) || n.as_ref().is_some_and(|(_, a, line)| !line && a.as_ref() == Some(path))),
                Row::Add { dir, after, .. } => editing.as_ref().is_some_and(|(n, ..)| n.as_ref().is_some_and(|(d, a, line)| *line && d == dir && a == after)),
                Row::Head { .. } => false,
            };
            y += h;
            if !near && !anchor {
                if items.is_empty() {
                    above += h;
                } else {
                    below += h;
                }
                continue;
            }
            match row {
                Row::Head { title, dir, detail, collapsed } => {
                    let toggle = dir.clone();
                    let top = drop_at.as_ref() == Some(&DropAt::Top(dir.clone()));
                    let target = dir.clone();
                    let new_here = editing.as_ref().and_then(|(n, ..)| n.as_ref()).is_some_and(|(d, after, line)| !line && *d == dir && after.is_none());
                    // The notes' heading has a name of its own: a folder in a plain folder
                    // could be called "notes" too.
                    let head_id = if Some(&dir) == notes_key.as_ref() { SharedString::from("notes-section") } else { SharedString::from(format!("head-{title}")) };
                    items.push(
                        div()
                            .id(head_id)
                            .relative()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .flex_none()
                            .h(px(HEAD_H - 14.))
                            .mt(px(11.))
                            .mb(px(3.))
                            .px(px(4.))
                            .text_size(px(10.5))
                            .line_height(px(15.))
                            .text_color(c(theme::muted()))
                            .when(collapsed.is_some(), |d| {
                                d.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.toggle_part(toggle.clone(), cx)))
                            })
                            .when(draggable, |d| {
                                d.on_drag_move::<RowDrag>(cx.listener(move |this, e: &DragMoveEvent<RowDrag>, _, cx| {
                                    if e.bounds.contains(&e.event.position) {
                                        this.set_drop(DropAt::Top(target.clone()), cx);
                                    }
                                }))
                            })
                            .children(collapsed.map(|shut| {
                                // › folds a part, turned down while it's open; ▾ is kept for menus.
                                div().w(px(14.)).flex_none().flex().justify_end().child(
                                    svg()
                                        .path("chrome/chevron.svg")
                                        .size(px(8.))
                                        .text_color(c(theme::faint()))
                                        .when(!shut, |s| s.with_transformation(Transformation::rotate(radians(std::f32::consts::FRAC_PI_2)))),
                                )
                            }))
                            .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                            .children(detail.map(|w| div().flex_none().text_color(c(theme::faint())).child(w)))
                            .when(top, |d| d.child(line(false)))
                            .test_support()
                            .into_any_element(),
                    );
                    if let (true, Some((.., f))) = (new_here, &editing) {
                        items.push(field_row("", f.clone()));
                        placed = true;
                    }
                }
                Row::File { path, no, label, detail, placeholder, loose } => {
                    let on = current.as_ref() == Some(&path);
                    let note = notes.contains(&path);
                    let journal = kind == Kind::Journal && !note && !loose;
                    let draggable = draggable && !note;
                    if editing.as_ref().and_then(|(_, r, _)| r.as_ref()) == Some(&path) {
                        if let Some((.., f)) = &editing {
                            items.push(field_row(&no, f.clone()));
                        }
                        continue;
                    }
                    let id = SharedString::from(format!("row-{}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    let (click, menu, moves) = (path.clone(), path.clone(), path.clone());
                    let over_above = drop_at.as_ref() == Some(&DropAt::Above(path.clone()));
                    let over_below = drop_at.as_ref() == Some(&DropAt::Below(path.clone()));
                    let drag = RowDrag { path: path.clone(), no: no.clone().into(), label: label.clone().into(), detail: detail.clone().into() };
                    items.push(
                        div()
                            .id(id)
                            .relative()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(ROW_H))
                            .flex_none()
                            .px(px(4.))
                            .rounded_md()
                            .text_size(px(if journal { 12.5 } else { 13. }))
                            .line_height(px(19.))
                            .cursor_pointer()
                            .hover(|d| d.bg(ca(theme::line(), 0.8)))
                            .text_color(if on { c(theme::text()) } else { c(theme::muted()) })
                            .role(Role::Button)
                            .aria_label(label.clone())
                            .aria_selected(on)
                            .on_click(cx.listener(move |this, e: &ClickEvent, w, cx| {
                                if e.modifiers().platform {
                                    this.open_row_in_window(&click, cx);
                                } else if this.switch_to(&click, w, cx) {
                                    this.focus_page(w, cx);
                                }
                            }))
                            .when(!placeholder, |d| {
                                d.on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                    this.open_row_menu(menu.clone(), e.position, cx);
                                }))
                            })
                            .when(draggable && !placeholder, |d| {
                                d.on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone())).on_drag_move::<RowDrag>(cx.listener(move |this, e: &DragMoveEvent<RowDrag>, _, cx| {
                                    if e.bounds.contains(&e.event.position) {
                                        let above = e.event.position.y < e.bounds.center().y;
                                        this.set_drop(if above { DropAt::Above(moves.clone()) } else { DropAt::Below(moves.clone()) }, cx);
                                    }
                                }))
                            })
                            .when(!no.is_empty() || !journal, |d| {
                                d.when(kind != Kind::Folder, |d| {
                                    d.child(
                                        div()
                                            .flex_none()
                                            .min_w(px(if journal { 42. } else { 14. }))
                                            .whitespace_nowrap()
                                            .when(!journal, |d| d.text_right())
                                            .text_size(px(11.))
                                            .text_color(if on { c(theme::accent()) } else if journal { c(theme::muted()) } else { c(theme::faint()) })
                                            .child(no.clone()),
                                    )
                                })
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when(placeholder, |d| d.text_color(c(theme::placeholder())))
                                    .when(loose, |d| d.italic())
                                    .child(label.clone()),
                            )
                            .when(!detail.is_empty(), |d| {
                                d.child(div().flex_none().text_size(px(10.5)).text_color(if on { c(theme::muted()) } else { c(theme::faint()) }).child(detail.clone()))
                            })
                            .when(over_above, |d| d.child(line(true)))
                            .when(over_below, |d| d.child(line(false)))
                            .test_support()
                            .into_any_element(),
                    );
                    if let Some((Some((_, after, false)), _, f)) = &editing {
                        if after.as_ref() == Some(&path) {
                            items.push(field_row("", f.clone()));
                            placed = true;
                        }
                    }
                }
                Row::Add { dir, after, what } => {
                    if let Some((n, _, f)) = &editing {
                        if !placed && n.as_ref().is_some_and(|(d, a, line)| *line && *d == dir && *a == after) {
                            items.push(field_row("", f.clone()));
                            placed = true;
                        }
                        continue;
                    }
                    let label = what.line_label();
                    let tip = match what {
                        Adds::Chapter => (tr!("At the end of the book"), Some(tr!("after this chapter"))),
                        Adds::File => (tr!("A new file"), Some("")),
                        Adds::Note => (tr!("A new note"), None),
                    };
                    items.push(
                        div()
                            .id(SharedString::from(format!("add-{}", what.noun())))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(ROW_H))
                            .flex_none()
                            .px(px(4.))
                            .rounded_md()
                            .text_size(px(12.5))
                            .line_height(px(19.))
                            .cursor_pointer()
                            .text_color(c(theme::faint()))
                            .hover(|d| d.bg(ca(theme::line(), 0.8)).text_color(c(theme::accent())))
                            .role(Role::Button)
                            .aria_label(label)
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.begin_row_edit(EditTarget::New { dir: dir.clone(), after: after.clone(), line: true }, "", w, cx);
                            }))
                            .tooltip(move |_, cx| {
                                // ⌘N adds too, but after the page's chapter: the tooltip says where.
                                let keys = tip.1.and_then(|then| {
                                    let k = crate::context_menu::shortcut_label(&NewDocument, cx)?;
                                    Some(if then.is_empty() { k } else { format!("{k}: {then}") })
                                });
                                cx.new(|_| workspace::Tip::new(tip.0, keys)).into()
                            })
                            .when(kind != Kind::Folder, |d| d.child(div().flex_none().min_w(px(14.))))
                            .child(div().flex_1().min_w_0().truncate().child(label))
                            .test_support()
                            .into_any_element(),
                    );
                }
            }
        }
        if let (Some((Some(_), _, f)), false) = (&editing, placed) {
            items.insert(0, field_row("", f.clone()));
        }

        let body: AnyElement = if empty && editing.is_none() {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(4.))
                .text_size(px(12.5))
                .text_color(c(theme::muted()))
                .child(tr!("nothing here yet"))
                .child(
                    div()
                        .id("first-chapter")
                        .cursor_pointer()
                        .text_color(c(theme::accent()))
                        .on_click(cx.listener(|this, _, w, cx| this.start_with(Kind::Book, w, cx)))
                        .child(tr!("+ first chapter"))
                        .test_support(),
                )
                .child(
                    div()
                        .id("todays-page")
                        .cursor_pointer()
                        .text_color(c(theme::accent()))
                        .on_click(cx.listener(|this, _, w, cx| this.start_with(Kind::Journal, w, cx)))
                        .child(tr!("+ today’s page"))
                        .test_support(),
                )
                .into_any_element()
        } else {
            div()
                .id("project-list")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .on_drop(cx.listener(|this, d: &RowDrag, w, cx| this.drop_row(d, w, cx)))
                .child(div().flex_none().h(px(above)))
                .children(items)
                .child(div().flex_none().h(px(below)))
                .into_any_element()
        };

        let hint = editing.is_some().then(|| tr!("return names it · esc cancels").to_string());

        let switch = self.left_switch(cx);
        let foot = self.render_panel_foot(cx);
        let el = div()
            .id("project")
            .test_support()
            .key_context("ProjectList")
            .track_focus(&focus)
            .absolute()
            .top(px(workspace::HEADER_H))
            .bottom_0()
            .left_0()
            .w(px(workspace::sidebar_width(cx)))
            .flex()
            .flex_col()
            .bg(theme::surface(theme::bg_panel()))
            .border_r_1()
            .border_color(c(theme::line()))
            .pt(px(160. - workspace::HEADER_H))
            .px(px(20.))
            .pb(px(crate::panel_foot::PANEL_PB))
            .gap_3()
            .on_action(cx.listener(Self::list_up))
            .on_action(cx.listener(Self::list_down))
            .on_action(cx.listener(Self::list_return))
            .on_action(cx.listener(Self::list_close))
            .on_action(cx.listener(Self::list_undo))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(workspace::resize_handle(PanelEdge::Sidebar))
            .children(switch.map(|s| div().absolute().top(px(12.)).left(px(20.)).child(s)))
            .child(
                div()
                    .id("project-title")
                    .absolute()
                    .top(px(76.))
                    .left(px(24.))
                    .right(px(18.))
                    .truncate()
                    .font_family(theme::hand())
                    .text_size(theme::hand_px(23.))
                    .text_color(c(theme::text_2()))
                    .child(title)
                    .test_support(),
            )
            .child(
                div()
                    .id("project-kind")
                    .absolute()
                    .top(px(106.))
                    .left(px(24.))
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(c(theme::faint()))
                    .hover(|d| d.text_color(c(theme::muted())))
                    .role(Role::Button)
                    .aria_label(kind_label.clone())
                    .on_click(cx.listener(|this, _, w, cx| this.show_project_settings(&ProjectSettings, w, cx)))
                    .child(kind_label)
                    .test_support(),
            )
            .child(body)
            .child(
                div()
                    .flex_none()
                    .mt_auto()
                    .px(px(4.))
                    .text_size(px(11.))
                    .text_color(c(theme::muted()))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(!empty, |d| d.child(div().id("project-summary").aria_label(summary.clone()).text_color(c(theme::text_2())).child(summary).test_support()))
                    .when_some(hint.filter(|_| !empty || editing.is_some()), |d, hint| d.child(div().text_color(c(theme::faint())).child(hint))),
            )
            .child(foot);
        Some(el.into_any_element())
    }

    fn set_drop(&mut self, at: DropAt, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.project {
            if p.drop_at.as_ref() != Some(&at) {
                p.drop_at = Some(at);
                cx.notify();
            }
        }
    }
}

fn field_row(no: &str, field: Entity<TextView>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_none()
        .h(px(ROW_H))
        .px(px(4.))
        .text_size(px(13.))
        .child(div().flex_none().min_w(px(14.)).whitespace_nowrap().text_right().text_size(px(11.)).text_color(c(theme::accent())).child(no.to_string()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px(px(5.))
                .rounded(px(4.))
                .bg(c(theme::bg_raised()))
                .border_1()
                .border_color(c(theme::accent()))
                .child(field),
        )
        .into_any_element()
}
