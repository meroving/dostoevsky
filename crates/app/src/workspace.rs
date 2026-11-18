use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::files;
use crate::find::{self, FindBar, FindEvent, FindOptions, Job, Place, Scope, Searchable, Source, Spot};
use crate::project;
use crate::format;
use crate::model::{DocModel, ModelEvent};
use crate::prefs::Prefs;
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};

pub struct Workspace {
    pub model: Entity<DocModel>,
    pub page: Entity<TextView>,
    typing: bool,
    last_edit: Instant,
    /// The cut Esc would put back.
    pub(crate) last_cut: Option<crate::cuts::LastCut>,
    /// The mark whose text peeks out under the pointer, and whether the pointer is on the peek.
    pub(crate) peek: Option<(doc_model::CutId, bool)>,
    pub(crate) comment_edit: Option<crate::comments::CommentEdit>,
    pub(crate) comment_peek: Option<doc_model::CommentId>,
    pub(crate) right_tab: crate::comments::RightTab,
    pub(crate) comments_scope: crate::cuts_panel::Scope,
    pub(crate) comment_found: Option<doc_model::CommentId>,
    pub(crate) comment_files: crate::comments::DiskComments,
    /// The status line stays until the next action.
    pub(crate) status_held: bool,
    /// A change from it counts as the next action.
    pub(crate) status_held_sel: Option<std::ops::Range<usize>>,
    pub(crate) journal: Option<PathBuf>,
    pub(crate) journaled_version: u64,
    pub(crate) status: Option<(SharedString, Instant)>,
    pub(crate) status_reveal: Option<PathBuf>,
    find: Option<Entity<FindBar>>,
    /// Of the find bar last closed: ⌘G and ⇧⌘G go on with it, ⌘E sets it.
    last_find: String,
    pub(crate) find_opts: FindOptions,
    /// Find opened the Alternatives panel / the drawer to show a match, and closes it again.
    sidebar_by_find: bool,
    drawer_by_find: bool,
    pub(crate) sidebar: Option<crate::sidebar::Sidebar>,
    pub(crate) drawer: Option<crate::overflow::Drawer>,
    pub(crate) cuts_scope: crate::cuts_panel::Scope,
    pub(crate) context_menu: Option<crate::context_menu::ContextMenu>,
    pub(crate) count_open: bool,
    /// Since when text has been held over the closed Overflow's button (it springs open).
    pub(crate) spring: Option<std::time::Instant>,
    saved_at: Option<Instant>,
    pub(crate) overlay: crate::effects::Overlay,
    pub(crate) sheet: Option<crate::sheets::Sheet>,
    pub(crate) sheet_focus: FocusHandle,
    pub(crate) detached_dragging: bool,
    /// The search field of an open model menu in Preferences → AI.
    pub(crate) model_search: Option<crate::ai_prefs::ModelSearch>,
    pub(crate) watcher: Option<crate::external::Watch>,
    pub(crate) watched: Option<PathBuf>,
    pub(crate) pending_disk: Option<String>,
    /// The “changed on disk” question is open: nothing is saved over the file until it's answered.
    pub(crate) disk_prompt: bool,
    pub(crate) project: Option<crate::project_ui::ProjectWindow>,
    /// The Project tab of a window with no project.
    pub(crate) file_panel: Option<crate::panel_foot::FilePanel>,
    /// With no project, the panel button opens the tab used last: Alternatives if true.
    pub(crate) left_alts: bool,
    /// The left panel last showed Review.
    pub(crate) left_review: bool,
    pub(crate) slides: Slides,
    pub(crate) ideas: Option<crate::ideas::IdeasRun>,
    pub(crate) lab: crate::lab::Lab,
    /// When the window last went into editing, and a count so each pulse is an animation of its
    /// own.
    pub(crate) mode_pulse: Option<(Instant, usize)>,
    /// For code that has only the workspace (AI consent).
    pub(crate) window: AnyWindowHandle,
    _tasks: Vec<Task<()>>,
    pub(crate) _subs: Vec<Subscription>,
}

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn new_journal() -> Option<PathBuf> {
    files::journal_path(NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

impl Workspace {
    pub fn new(model: DocModel, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = cx.new(|_| model);
        let page = cx.new(|cx| TextView::new(Mode::Page, model.clone(), cx));
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(&page, window, Self::on_page_event));
        subs.push(cx.subscribe_in(&model, window, |this, _, _: &ModelEvent, window, cx| {
            // Any change waits for a pause before autosave, not only typing on the page.
            this.last_edit = Instant::now();
            this.update_title(window, cx);
            if this.find.is_some() {
                this.research(window, cx);
            }
            this.sync_sidebar(cx);
            this.lab_on_model_change(cx);
            this.comments_on_model_change(window, cx);
            if window.is_window_active() {
                crate::set_front_editing(this.model.read(cx).layer, cx);
            }
            cx.notify();
        }));
        subs.push(cx.observe_global::<crate::prefs::Prefs>(|this, cx| this.lab_on_prefs_change(cx)));
        subs.push(cx.observe_window_appearance(window, |_, window, cx| theme::apply_for(window.appearance(), cx)));
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                crate::set_front_window(this.project_kind(), this.model.read(cx).layer, cx);
            } else {
                this.autosave(window, cx);
            }
        }));
        let focus = page.read(cx).focus.clone();
        window.focus(&focus, cx);
        let mut ws = Workspace {
            model,
            page,
            typing: false,
            last_edit: Instant::now(),
            last_cut: None,
            status_held: false,
            status_held_sel: None,
            journal: files::journal_path(id),
            journaled_version: 0,
            status: None,
            status_reveal: None,
            find: None,
            last_find: String::new(),
            find_opts: FindOptions::default(),
            sidebar_by_find: false,
            drawer_by_find: false,
            sidebar: None,
            drawer: None,
            cuts_scope: crate::cuts_panel::Scope::Page,
            peek: None,
            comment_edit: None,
            comment_peek: None,
            right_tab: Default::default(),
            comments_scope: crate::cuts_panel::Scope::Page,
            comment_found: None,
            comment_files: Default::default(),
            context_menu: None,
            count_open: false,
            spring: None,
            saved_at: None,
            overlay: Default::default(),
            sheet: None,
            sheet_focus: cx.focus_handle(),
            detached_dragging: false,
            model_search: None,
            watcher: None,
            watched: None,
            pending_disk: None,
            disk_prompt: false,
            project: None,
            file_panel: None,
            left_alts: false,
            left_review: false,
            slides: Slides::default(),
            ideas: None,
            lab: crate::lab::Lab::new(cx),
            mode_pulse: None,
            window: window.window_handle(),
            _tasks: Vec::new(),
            _subs: subs,
        };
        ws.start_background(window, cx);
        ws.update_title(window, cx);
        let warnings = ws.model.update(cx, |m, _| std::mem::take(&mut m.warnings));
        if let Some(w) = warnings.first() {
            ws.status = Some((w.clone().into(), Instant::now()));
        }
        ws.watch_file(cx);
        ws
    }

    fn start_background(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task = cx.spawn_in(window, async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                ticks += 1;
                let ok = this.update_in(cx, |this, window, cx| {
                    let m = this.model.read(cx);
                    let idle = Prefs::get(cx).autosave == crate::prefs::Autosave::Idle;
                    if idle && m.dirty() && this.last_edit.elapsed() > Duration::from_secs(2) {
                        this.autosave(window, cx);
                    }
                    this.autosave_drawer(true, cx);
                    if ticks.is_multiple_of(10) {
                        this.write_journal(cx);
                        this.journal_drawer(cx);
                    }
                    if !this.status_held && this.status.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(6)) {
                        this.status = None;
                        cx.notify();
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        });
        self._tasks.push(task);
    }

    /// `fade` (typing) steps the chrome back; a drop, made with the pointer, doesn't.
    pub(crate) fn text_edited(&mut self, fade: bool, cx: &mut Context<Self>) {
        self.last_edit = Instant::now();
        if fade && !self.typing {
            self.typing = true;
            cx.notify();
        }
    }

    fn on_page_event(&mut self, _: &Entity<TextView>, e: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            ViewEvent::KeepCut(start) => self.keep_cut(*start, cx),
            ViewEvent::HistoryElsewhere(link) => self.history_elsewhere(*link, cx),
            ViewEvent::DragStarted => self.open_drawer_for_drag(window, cx),
            ViewEvent::Edited => {
                self.clear_held_status(cx);
                self.text_edited(true, cx)
            }
            ViewEvent::Cut(id) => self.on_cut(*id, cx),
            ViewEvent::PasteCut => self.paste_cut(window, cx),
            ViewEvent::SceneBreakHint => {
                // Once a session.
                static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                if !SHOWN.swap(true, std::sync::atomic::Ordering::Relaxed) || cfg!(test) {
                    self.flash_status(tr!("--- for a scene break"), cx);
                }
            }
            ViewEvent::LinkedHistory { link, redo } => self.linked_history(*link, *redo, cx),
            ViewEvent::PointerMoved => {
                if self.typing {
                    self.typing = false;
                    cx.notify();
                }
            }
            ViewEvent::Cycled { group, is_original } => {
                crate::sound::Sounds::play(cx, *is_original);
                if self.sidebar.as_ref().is_some_and(|s| s.group == Some(*group)) {
                    cx.notify();
                }
            }
            ViewEvent::SelectionChanged => {
                if self.status_held_sel.as_ref() != Some(&self.page.read(cx).selection()) {
                    self.clear_held_status(cx);
                }
                cx.notify()
            }
            ViewEvent::ContextMenu(at) => {
                let page = self.page.read(cx);
                let sel = page.selection();
                let text = page.index().slice(sel.clone()).to_string();
                let word = (!text.is_empty() && text.chars().all(english::is_word_char)).then(|| text.clone());
                let ghosts = self.model.read(cx).doc.ghost_ranges();
                let ghosted = ghosts.iter().any(|g| g.start <= sel.start && sel.end <= g.end && !(sel.is_empty() && g.end == sel.start));
                let spelling = page.misspelled.iter().find(|r| **r == sel).map(|r| crate::context_menu::Misspelling {
                    range: r.clone(),
                    word: text.clone(),
                    guesses: crate::spell::Spelling::guesses(cx, &text),
                });
                let look_up = Some(text.trim().to_string()).filter(|t| !t.is_empty() && t.chars().count() <= 200 && !t.contains('\n'));
                // A right-click inside a group offers that group, not a new one inside it.
                let group = page
                    .groups()
                    .iter()
                    .filter(|g| {
                        let inside = g.range.start <= sel.start && sel.end <= g.range.end;
                        inside && !g.range.is_empty() && (!sel.is_empty() || (g.range.start < sel.start && sel.end < g.range.end))
                    })
                    .max_by_key(|g| g.depth)
                    .map(|g| crate::context_menu::GroupTarget {
                        id: g.id,
                        text: page.index().slice(g.range.clone()).to_string(),
                        paragraph: g.scope == doc_model::Scope::Paragraph,
                    });
                let page = self.page.read(cx);
                let on_note = page.comment_rects.borrow().iter().any(|(_, b, tick)| (if *tick { b.dilate(px(5.)) } else { *b }).contains(at));
                let comment = page.comment_under(*at);
                if let (true, Some(id)) = (on_note, comment) {
                    self.comment_menu(id, *at, cx);
                    return;
                }
                if ghosted {
                    if let Some(id) = self.model.read(cx).doc.cut_at(sel.start) {
                        self.open_trace_menu(id, *at, cx);
                        return;
                    }
                }
                let mut menu = crate::context_menu::ContextMenu::page(*at, word, group, ghosted, !sel.is_empty(), spelling, look_up, crate::assist::on(cx), self.model.read(cx).layer);
                // Comments are editing's: writing's menu offers none.
                if self.model.read(cx).layer {
                    menu.add_comment_items(comment, !ghosted);
                }
                self.context_menu = Some(menu);
                cx.notify();
            }
            ViewEvent::TraceMenu(id, at) => self.open_trace_menu(*id, *at, cx),
            ViewEvent::MarkHover(id) => self.mark_hovered(*id, window, cx),
            ViewEvent::CommentHover(id) => self.comment_hovered(*id, cx),
            ViewEvent::EditComment(id) => self.write_comment(*id, window, cx),
            ViewEvent::OpenGroup(id) => {
                self.context_menu = None;
                self.open_group(*id, window, cx);
            }
            ViewEvent::Cancel => {
                if self.esc_puts_back(cx) {
                } else if self.context_menu.take().is_some() {
                    cx.notify();
                } else if self.count_open {
                    self.count_open = false;
                    cx.notify();
                } else if self.lab_back(window, cx) {
                } else if self.sidebar.is_some() {
                    self.sidebar_back(window, cx);
                    let f = self.page.read(cx).focus.clone();
                    window.focus(&f, cx);
                } else if self.find.is_some() {
                    self.on_find_close(window, cx);
                } else if self.panel_open() {
                    self.close_panel(window, cx);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn update_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        window.set_window_title(&window_title(m));
        window.set_window_edited(m.dirty());
        window.set_document_path(m.path.as_deref());
    }

    #[cfg(test)]
    pub fn find_bar(&self) -> Option<&Entity<FindBar>> {
        self.find.as_ref()
    }

    #[cfg(test)]
    pub fn chrome_dimmed(&self) -> bool {
        self.typing
    }

    #[cfg(any(test, feature = "shots"))]
    pub fn status_text(&self) -> Option<String> {
        self.status.as_ref().map(|(s, _)| s.to_string())
    }

    pub fn flash_status(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.flash_status_for(msg, 2600, cx);
    }

    /// Stays long enough to reach the Show in Finder link.
    pub(crate) fn flash_status_reveal(&mut self, msg: impl Into<SharedString>, path: PathBuf, cx: &mut Context<Self>) {
        self.flash_status_for(msg, 6000, cx);
        self.status_reveal = Some(path);
    }

    fn flash_status_for(&mut self, msg: impl Into<SharedString>, ms: u64, cx: &mut Context<Self>) {
        self.status = Some((msg.into(), Instant::now()));
        self.status_reveal = None;
        self.status_held = false;
        cx.notify();
        let t = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(ms)).await;
            let _ = this.update(cx, |this, cx| {
                if !this.status_held && this.status.as_ref().is_some_and(|(_, t)| t.elapsed() >= Duration::from_millis(ms - 100)) {
                    this.status = None;
                    cx.notify();
                }
            });
        });
        t.detach();
    }

    pub(crate) fn write_journal(&mut self, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let Some(j) = self.journal.clone() else { return };
        if !m.dirty() {
            if self.journaled_version != 0 {
                files::clear_journal(&j);
                self.journaled_version = 0;
            }
            return;
        }
        if m.version == self.journaled_version {
            return;
        }
        let text = format::serialize(m);
        let path = m.path.clone();
        let v = m.version;
        if files::write_journal(&j, path.as_deref(), &text).is_ok() {
            self.journaled_version = v;
        }
    }

    fn autosave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave_drawer(false, cx);
        let m = self.model.read(cx);
        if Prefs::get(cx).autosave != crate::prefs::Autosave::Off && m.dirty() && m.path.is_some() {
            self.save_to_current(window, cx);
        }
    }

    /// False if there is no path or writing failed.
    pub(crate) fn save_to_current(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (path, (text, cuts), version) = {
            let m = self.model.read(cx);
            let Some(p) = m.path.clone() else { return false };
            (p, format::serialize_for_disk(m), m.version)
        };
        if !self.may_write_over_file(cx) {
            return false;
        }
        // A chapter's cuts go to the project's cuts file first.
        if let Some((file, cuts)) = cuts {
            if !self.store_chapter_cuts(&file, cuts, cx) {
                return false;
            }
        }
        match files::write_atomic(&path, text.as_bytes()) {
            Ok(()) => {
                if Prefs::get(cx).backups {
                    let _ = files::backup(&path, text.as_bytes());
                }
                self.model.update(cx, |m, _| {
                    m.saved_version = version;
                    m.disk_text = Some(text);
                });
                self.saved_at = Some(Instant::now());
                if let Some(j) = &self.journal {
                    files::clear_journal(j);
                    self.journaled_version = 0;
                }
                self.update_title(window, cx);
                cx.notify();
                true
            }
            Err(e) => {
                let msg = tr!("Couldn't save {path}", path = path.display());
                let detail = e.to_string();
                drop(window.prompt(PromptLevel::Critical, &msg, Some(&detail), &[tr!("OK")], cx));
                false
            }
        }
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        self.save_drawer(cx);
        if self.model.read(cx).path.is_some() {
            self.save_to_current(window, cx);
        } else {
            self.save_as(&SaveAs, window, cx);
        }
    }

    pub fn save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let dir = m
            .path
            .as_ref()
            .or(m.suggested.as_ref())
            .and_then(|p| p.parent().map(PathBuf::from))
            .or_else(|| Prefs::get(cx).default_folder.clone())
            .or_else(|| crate::platform::home().map(|h| h.join("Documents")))
            .unwrap_or_else(|| PathBuf::from("."));
        let name = m.path.as_ref().or(m.suggested.as_ref()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled.md".into());
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    // What's there now was never ours: the save panel already asked to replace it.
                    this.model.update(cx, |m, _| {
                        m.path = Some(path.clone());
                        m.disk_text = None;
                    });
                    if this.save_to_current(window, cx) {
                        this.watch_file(cx);
                        files::add_recent(&path);
                        crate::refresh_menus(cx);
                    }
                });
            }
        })
        .detach();
    }

    fn revert(&mut self, _: &RevertToSaved, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.model.read(cx).path.clone() else { return };
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let loaded = self.load_page_text(&text, cx);
        self.model.update(cx, |m, cx| {
            m.doc = loaded.doc;
            m.layer = loaded.layer;
            m.present();
            m.kept = loaded.kept;
            m.disk_text = Some(text);
            m.version += 1;
            m.saved_version = m.version;
            cx.emit(ModelEvent::Changed);
            cx.notify();
        });
        self.update_title(window, cx);
    }

    fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_close(window, cx) {
            window.remove_window();
        }
    }

    /// True when the window may close now; otherwise asks and closes later.
    pub fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // Files left with unsaved edits (autosave off) get their say first.
        self.autosave_drawer(false, cx);
        let unsaved = self.parked_unsaved(cx);
        if unsaved > 0 {
            let msg = trn!("Save the changes in {n} other file?", "Save the changes in {n} other files?", unsaved);
            let rx = window.prompt(
                PromptLevel::Warning,
                &msg,
                Some(tr!("Your changes will be lost if you don't save them.")),
                &[tr!("Save All"), tr!("Cancel"), tr!("Don't Save")],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let Ok(answer) = rx.await else { return };
                let _ = this.update_in(cx, |this, window, cx| {
                    match answer {
                        0 => this.save_parked(cx),
                        2 => this.drop_parked_edits(cx),
                        _ => return,
                    }
                    if this.parked_unsaved(cx) == 0 && this.confirm_close(window, cx) {
                        window.remove_window();
                    }
                });
            })
            .detach();
            return false;
        }
        let m = self.model.read(cx);
        if !m.dirty() {
            self.cleanup();
            return true;
        }
        if m.path.is_some() && Prefs::get(cx).autosave != crate::prefs::Autosave::Off {
            let ok = self.save_to_current(window, cx);
            if ok {
                self.cleanup();
            }
            return ok;
        }
        let msg = tr!("Do you want to save the changes you made to “{name}”?", name = m.title());
        let rx = window.prompt(
            PromptLevel::Warning,
            &msg,
            Some(tr!("Your changes will be lost if you don't save them.")),
            &[tr!("Save"), tr!("Cancel"), tr!("Don't Save")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = rx.await else { return };
            let _ = this.update_in(cx, |this, window, cx| match answer {
                0 => {
                    if this.model.read(cx).path.is_some() {
                        if this.save_to_current(window, cx) {
                            this.cleanup();
                            window.remove_window();
                        }
                    } else {
                        this.save_as(&SaveAs, window, cx);
                    }
                }
                2 => {
                    this.model.update(cx, |m, _| m.saved_version = m.version);
                    this.cleanup();
                    window.remove_window();
                }
                _ => {}
            });
        })
        .detach();
        false
    }

    fn cleanup(&mut self) {
        if let Some(j) = &self.journal {
            files::clear_journal(j);
        }
        self.clear_parked_journals();
    }

    fn find_names(&self) -> crate::find::Names {
        use crate::find::Names;
        match self.project_kind() {
            None => Names::single(),
            Some(project::Kind::Book) => Names { this: "this chapter", whole: "whole book", one: "chapter", many: "chapters" },
            Some(project::Kind::Journal) => Names { this: "this day", whole: "whole journal", one: "entry", many: "entries" },
            Some(project::Kind::Folder) => Names { this: "this file", whole: "whole folder", one: "file", many: "files" },
        }
    }

    fn ensure_find_bar(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) -> Entity<FindBar> {
        match &self.find {
            Some(b) => {
                b.update(cx, |b, cx| {
                    b.show_replace |= replace;
                    cx.notify();
                });
                b.clone()
            }
            None => {
                let model = self.model.clone();
                let (opts, in_project, names) = (self.find_opts, self.project.is_some(), self.find_names());
                let b = cx.new(|cx| {
                    let mut b = FindBar::new(model, replace, cx);
                    b.opts = opts;
                    b.in_project = in_project;
                    b.names = names;
                    b
                });
                self._subs.push(cx.subscribe_in(&b, window, Self::on_find_event));
                self.find = Some(b.clone());
                b
            }
        }
    }

    fn open_find_in(&mut self, scope: Scope, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let bar = self.ensure_find_bar(replace, window, cx);
        let sel = self.page.read(cx).selection();
        if !sel.is_empty() && sel.len() < 200 {
            let t = self.page.read(cx).index().slice(sel).to_string();
            if !t.contains('\n') {
                let q = bar.read(cx).query.clone();
                q.update(cx, |q, cx| q.set_field_text(&t, cx));
            }
        }
        let in_project = self.project.is_some();
        bar.update(cx, |b, cx| {
            let scope = if in_project { scope } else { Scope::File };
            if scope == Scope::File && b.place == Place::Drawer {
                b.place = Place::File(PathBuf::new());
            }
            b.scope = scope;
            b.list_open = scope == Scope::Project;
            cx.notify();
        });
        // Select the whole query, so typing replaces it and Return finds it.
        let query = bar.read(cx).query.clone();
        query.update(cx, |q, cx| {
            let n = q.index().len();
            q.select(0..n, cx);
        });
        let f = query.read(cx).focus.clone();
        window.focus(&f, cx);
        self.refind(false, true, window, cx);
        cx.notify();
    }

    fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find_in(Scope::File, replace, window, cx);
    }

    fn find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(false, window, cx);
    }

    fn find_replace(&mut self, _: &FindReplace, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(true, window, cx);
    }

    fn find_in_project(&mut self, _: &FindInProject, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find_in(Scope::Project, false, window, cx);
    }

    pub(crate) fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_some() {
            self.on_find_close(window, cx);
        } else {
            self.open_find(false, window, cx);
        }
    }

    fn find_next(&mut self, _: &FindNext, window: &mut Window, cx: &mut Context<Self>) {
        self.find_step(true, window, cx);
    }

    fn find_previous(&mut self, _: &FindPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.find_step(false, window, cx);
    }

    /// With the card closed it comes back with the last query and the keyboard stays where it
    /// was; with no query yet it's ⌘F.
    fn find_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            if self.last_find.is_empty() {
                self.open_find(false, window, cx);
                return;
            }
            let b = self.ensure_find_bar(false, window, cx);
            let (q, t) = (b.read(cx).query.clone(), self.last_find.clone());
            q.update(cx, |q, cx| q.set_field_text(&t, cx));
            self.refind(false, true, window, cx);
        }
        self.find_move(forward, window, cx);
    }

    /// For "from the caret": a hidden version counts where its group is, a scrap after everything.
    fn spot_at(&self, spot: &Spot, cx: &App) -> usize {
        match spot {
            Spot::Page(r) => r.start,
            Spot::Hidden { group, .. } => self.model.read(cx).doc.group_range(*group).map_or(0, |r| r.start),
            Spot::Scrap { .. } => usize::MAX,
        }
    }

    /// After the current match if we're on it, else from the caret. Past the end of a place, the
    /// whole scope goes on to the next place with matches (saving the file left); one place wraps.
    fn find_move(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self.find.clone() else { return };
        let sel = self.page.read(cx).selection();
        let (n, current, on_it, positions, scope, in_drawer) = {
            let b = bar.read(cx);
            let on_it = match b.current_spot() {
                Some(Spot::Page(r)) => *r == sel,
                Some(_) => true,
                None => false,
            };
            let positions: Vec<usize> = b.spots.iter().map(|s| self.spot_at(s, cx)).collect();
            (b.spots.len(), b.current, on_it, positions, b.scope, b.place == Place::Drawer)
        };
        let next: Option<usize> = match (current, on_it, forward) {
            (Some(i), true, true) => Some(i + 1).filter(|i| *i < n),
            (Some(i), true, false) => i.checked_sub(1),
            _ if in_drawer => (n > 0).then(|| if forward { 0 } else { n - 1 }),
            (_, _, true) => {
                let from = if sel.is_empty() { sel.start } else { sel.end };
                positions.iter().position(|p| *p >= from && *p != usize::MAX).or_else(|| positions.iter().position(|p| *p == usize::MAX))
            }
            (_, _, false) => positions.iter().rposition(|p| *p < sel.start),
        };
        if let Some(i) = next {
            bar.update(cx, |b, _| {
                b.current = Some(i);
                b.reveal_current();
            });
            self.show_matches(true, window, cx);
            return;
        }
        if scope == Scope::Project && self.find_cross(forward, window, cx) {
            return;
        }
        if n > 0 {
            bar.update(cx, |b, _| {
                b.current = Some(if forward { 0 } else { n - 1 });
                b.reveal_current();
            });
            self.show_matches(true, window, cx);
        }
    }

    /// False when there is no other place with matches.
    fn find_cross(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(bar) = self.find.clone() else { return false };
        let (counts, here) = {
            let b = bar.read(cx);
            (b.counts(), b.place.clone())
        };
        if counts.is_empty() {
            return false;
        }
        let len = counts.len();
        let target = match counts.iter().position(|(p, _)| *p == here) {
            Some(i) => counts[if forward { (i + 1) % len } else { (i + len - 1) % len }].0.clone(),
            None => {
                // Not a place with matches: the first after it in reading order.
                let order = bar.read(cx).order.clone();
                let at = order.iter().position(|p| *p == here).unwrap_or(0);
                let after = |p: &Place| order.iter().position(|q| q == p).unwrap_or(0);
                if forward {
                    counts.iter().find(|(p, _)| after(p) > at).unwrap_or(&counts[0]).0.clone()
                } else {
                    counts.iter().rev().find(|(p, _)| after(p) < at).unwrap_or(&counts[len - 1]).0.clone()
                }
            }
        };
        if target == here {
            return false;
        }
        self.find_go(target, if forward { 0 } else { usize::MAX }, window, cx);
        true
    }

    /// The last match if `k` is past it; opens that file, or the drawer.
    fn find_go(&mut self, place: Place, k: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self.find.clone() else { return };
        if let Place::File(path) = &place {
            if self.model.read(cx).path.as_ref() != Some(path) && !self.switch_to(path, window, cx) {
                return;
            }
        }
        bar.update(cx, |b, _| b.place = place.clone());
        self.refind(false, false, window, cx);
        bar.update(cx, |b, _| {
            let n = b.spots.len();
            b.current = (n > 0).then(|| k.min(n - 1));
            b.reveal_current();
        });
        self.show_matches(true, window, cx);
        if let Some(f) = self.find.as_ref().map(|b| b.read(cx).query.read(cx).focus.clone()) {
            if self.find.as_ref().is_some_and(|b| b.read(cx).list_open) {
                window.focus(&f, cx);
            }
        }
    }

    fn use_selection_for_find(&mut self, _: &UseSelectionForFind, window: &mut Window, cx: &mut Context<Self>) {
        let sel = self.page.read(cx).selection();
        let t = self.page.read(cx).index().slice(sel.clone()).to_string();
        if sel.is_empty() || t.contains('\n') {
            window.play_system_bell();
            return;
        }
        self.last_find = t.clone();
        if let Some(bar) = self.find.clone() {
            let q = bar.read(cx).query.clone();
            q.update(cx, |q, cx| q.set_field_text(&t, cx));
            self.refind(false, true, window, cx);
            cx.notify();
        }
    }

    /// The page's matches again, the project left alone.
    fn research(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refind(false, false, window, cx);
    }

    fn page_label(&self, cx: &App) -> String {
        let path = self.model.read(cx).path.clone();
        let entry = path.as_deref().and_then(|p| self.project.as_ref().and_then(|q| q.tree.entry(p)).map(|e| e.label.clone()));
        entry
            .or_else(|| path.as_deref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| tr!("this file").into())
    }

    /// `jump` goes to the current match; `project` searches every other file too, off the UI
    /// thread.
    fn refind(&mut self, jump: bool, project: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self.find.clone() else { return };
        let (q, o, scope, at_drawer) = {
            let b = bar.read(cx);
            (b.query_text(cx), b.opts, b.scope, b.place == Place::Drawer)
        };
        let in_project = self.project.is_some();
        let at_drawer = at_drawer && in_project && scope == Scope::Project;
        let (place, found, label, skipped) = if at_drawer {
            let s = self.shared_drawer().map(|d| Searchable::scraps_of(&d.read(cx).doc)).unwrap_or_default();
            (Place::Drawer, find::spots(&s, &q, &FindOptions { drawer: true, ..o }), tr!("the project’s cuts").to_string(), 0)
        } else {
            // A file's cuts with no trace are in it (a chapter's too); a project's cuts of no
            // file are its own: searched after the chapter's in one chapter's scope, and as a
            // place of their own in the whole book's.
            let mut s = Searchable::of(&self.model.read(cx).doc, true);
            if in_project && scope == Scope::File {
                if let Some(d) = self.shared_drawer() {
                    s.scraps.extend(Searchable::scraps_of(&d.read(cx).doc).scraps);
                }
            }
            let found = find::spots(&s, &q, &o);
            let skipped = if o.case || o.words {
                let loose = FindOptions { case: false, words: false, alternatives: false, drawer: false, ..o };
                let tight = FindOptions { alternatives: false, drawer: false, ..o };
                find::spots(&s, &q, &loose).len().saturating_sub(find::spots(&s, &q, &tight).len())
            } else {
                0
            };
            let place = Place::File(self.model.read(cx).path.clone().unwrap_or_default());
            (place, found, self.page_label(cx), skipped)
        };
        let from = (!at_drawer).then(|| self.page.read(cx).selection().start);
        bar.update(cx, |b, cx| {
            b.place_label = label;
            b.skipped = skipped;
            b.set_spots(place, found, if jump { from } else { None });
            cx.notify();
        });
        self.show_matches(jump, window, cx);
        if project && in_project {
            self.search_project(cx);
        }
    }

    /// Off the UI thread. Words in memory are searched as they stand, ahead of the disk; a newer
    /// search makes this one's answer stale.
    fn search_project(&mut self, cx: &mut Context<Self>) {
        let (Some(bar), Some(p)) = (self.find.clone(), self.project.as_ref()) else { return };
        let (q, o) = (bar.read(cx).query_text(cx), bar.read(cx).opts);
        if q.is_empty() {
            bar.update(cx, |b, cx| {
                b.files.clear();
                b.order.clear();
                b.searching = false;
                cx.notify();
            });
            return;
        }
        let mut live: std::collections::HashMap<PathBuf, Searchable> = p.parked.iter().map(|(path, k)| (path.clone(), Searchable::of(&k.model.doc, true))).collect();
        if let Some(path) = self.model.read(cx).path.clone() {
            live.insert(path, Searchable::of(&self.model.read(cx).doc, true));
        }
        let mut jobs = Vec::new();
        let job = |e: &project::Entry, section: String, live: &mut std::collections::HashMap<PathBuf, Searchable>| {
            let source = live.remove(&e.path).map_or_else(|| Source::Disk(e.path.clone()), Source::Live);
            let no = e.number.map(|n| n.to_string()).or(e.day.map(project::Day::label)).unwrap_or_default();
            Job { place: Place::File(e.path.clone()), no, label: e.label.clone(), section, source }
        };
        for g in &p.tree.main {
            for e in &g.entries {
                jobs.push(job(e, g.title.clone().unwrap_or_default(), &mut live));
            }
        }
        for e in &p.tree.loose {
            let section = match p.tree.kind {
                project::Kind::Book => tr!("not in the book"),
                project::Kind::Journal => tr!("not in the journal"),
                project::Kind::Folder => tr!("not in the folder"),
            };
            jobs.push(job(e, section.to_string(), &mut live));
        }
        if o.drawer {
            let s = Searchable::scraps_of(&p.drawer.model.read(cx).doc);
            jobs.push(Job { place: Place::Drawer, no: String::new(), label: tr!("cuts").into(), section: tr!("cuts").into(), source: Source::Live(s) });
        }
        if o.notes {
            for e in p.tree.notes.iter().flat_map(|g| g.entries.iter()) {
                jobs.push(job(e, find::NOTES.to_string(), &mut live));
            }
        }
        let order: Vec<Place> = jobs.iter().map(|j| j.place.clone()).collect();
        let run = bar.update(cx, |b, _| {
            b.run += 1;
            b.searching = true;
            b.run
        });
        let work = cx.background_spawn(async move { find::search_jobs(jobs, &q, &o) });
        cx.spawn(async move |this, cx| {
            let files = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(bar) = this.find.clone() {
                    bar.update(cx, |b, cx| {
                        if b.run == run {
                            b.files = files;
                            b.order = order;
                            b.searching = false;
                            b.reveal_current();
                            cx.notify();
                        }
                    });
                }
            });
        })
        .detach();
    }

    /// `jump` goes to the current match, wherever it is (page, ghost, Alternatives panel, drawer).
    /// The keyboard stays where it was.
    fn show_matches(&mut self, jump: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self.find.clone() else { return };
        let (hl, current, spot) = {
            let b = bar.read(cx);
            (b.highlights(), b.current_page_match(), b.current_spot().cloned())
        };
        let ghosts = self.model.read(cx).doc.ghost_ranges();
        let raised = match &spot {
            Some(Spot::Page(r)) => ghosts.iter().find(|g| g.start < r.end && r.start < g.end).cloned(),
            _ => None,
        };
        self.page.update(cx, |p, cx| {
            p.highlights = hl;
            p.current_highlight = current;
            if jump {
                p.raised_ghost = raised;
            }
            cx.notify();
        });
        if !jump {
            return;
        }
        let focused = window.focused(cx);
        match spot {
            Some(Spot::Page(r)) => {
                self.put_back_what_find_opened(window, cx);
                self.page.update(cx, |p, cx| p.select(r, cx));
            }
            Some(Spot::Hidden { group, variant, .. }) => {
                if self.drawer_by_find {
                    self.drawer = None;
                    self.drawer_by_find = false;
                    self.sync_comment_ticks(cx);
                }
                if let Some(at) = self.model.read(cx).doc.group_range(group) {
                    self.page.update(cx, |p, cx| p.select(at.start..at.start, cx));
                }
                let was_open = self.sidebar.is_some();
                self.open_group_view(group, window, cx);
                self.sidebar_by_find |= !was_open;
                if let Some(s) = &mut self.sidebar {
                    s.found = Some((group, variant));
                }
            }
            Some(Spot::Scrap { note, .. }) => {
                if self.sidebar_by_find && self.sidebar.is_some() {
                    self.close_sidebar(window, cx);
                }
                self.sidebar_by_find = false;
                let was_open = self.drawer.is_some();
                self.open_overflow_for_drop(window, cx);
                self.drawer_by_find |= !was_open;
                let at_drawer = bar.read(cx).place == find::Place::Drawer;
                let found = self.found_cut(note, at_drawer, cx);
                if let Some(d) = &mut self.drawer {
                    d.found = found;
                }
            }
            None => {}
        }
        if let Some(f) = focused {
            window.focus(&f, cx);
        }
        cx.notify();
    }

    fn put_back_what_find_opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_by_find && self.sidebar.is_some() {
            self.close_sidebar(window, cx);
        }
        if self.drawer_by_find {
            self.drawer = None;
            self.sync_comment_ticks(cx);
        }
        self.sidebar_by_find = false;
        self.drawer_by_find = false;
        if let Some(s) = &mut self.sidebar {
            s.found = None;
        }
        if let Some(d) = &mut self.drawer {
            d.found = None;
        }
    }

    fn on_find_event(&mut self, bar: &Entity<FindBar>, e: &FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            FindEvent::Updated => {
                self.find_opts = bar.read(cx).opts;
                self.refind(false, true, window, cx);
            }
            FindEvent::Next => self.find_move(true, window, cx),
            FindEvent::Previous => self.find_move(false, window, cx),
            FindEvent::Scope(scope) => {
                let scope = *scope;
                bar.update(cx, |b, cx| {
                    b.scope = scope;
                    b.list_open = scope == Scope::Project;
                    if scope == Scope::File && b.place == Place::Drawer {
                        b.place = Place::File(PathBuf::new());
                    }
                    cx.notify();
                });
                self.refind(false, true, window, cx);
            }
            FindEvent::Fold => bar.update(cx, |b, cx| {
                b.list_open = !b.list_open && b.scope == Scope::Project;
                b.reveal_current();
                cx.notify();
            }),
            FindEvent::GoTo(place, k) => {
                let (place, k) = (place.clone(), *k);
                self.find_go(place, k, window, cx);
                bar.update(cx, |b, cx| {
                    b.list_open = false;
                    cx.notify();
                });
            }
            FindEvent::Replace => {
                let with = bar.read(cx).replacement.read(cx).field_text().to_string();
                let sel = self.page.read(cx).selection();
                if matches!(bar.read(cx).current_spot(), Some(Spot::Page(r)) if *r == sel) {
                    self.page.update(cx, |p, cx| {
                        p.replace(sel.clone(), &with, cx);
                    });
                }
                self.refind(false, false, window, cx);
                self.find_move(true, window, cx);
            }
            FindEvent::ReplaceAll => {
                let with = bar.read(cx).replacement.read(cx).field_text().to_string();
                let matches = bar.read(cx).matches.clone();
                let n = matches.len();
                self.model.update(cx, |m, _| m.doc.break_undo_group());
                self.page.update(cx, |p, cx| {
                    for m in matches.into_iter().rev() {
                        p.replace(m, &with, cx);
                    }
                });
                self.model.update(cx, |m, _| m.doc.break_undo_group());
                self.refind(false, true, window, cx);
                self.flash_status(trn!("Replaced {n}", "Replaced {n}", n), cx);
            }
            FindEvent::Close => self.on_find_close(window, cx),
        }
    }

    fn on_find_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bar) = self.find.take() {
            self.last_find = bar.read(cx).query_text(cx);
            self.find_opts = bar.read(cx).opts;
        }
        self.put_back_what_find_opened(window, cx);
        self.page.update(cx, |p, cx| {
            p.highlights = Default::default();
            p.current_highlight = None;
            p.raised_ghost = None;
            cx.notify();
        });
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
        cx.notify();
    }

    fn toggle_layer(&mut self, _: &ToggleLayer, window: &mut Window, cx: &mut Context<Self>) {
        let editing = !self.model.read(cx).layer;
        self.set_mode(editing, window, cx);
    }

    fn writing_mode(&mut self, _: &WritingMode, window: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(false, window, cx);
    }

    fn editing_mode(&mut self, _: &EditingMode, window: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(true, window, cx);
    }

    /// Writing's segment, clicked again in writing, clears the page: every panel closes.
    fn click_writing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.read(cx).layer {
            return self.set_mode(false, window, cx);
        }
        self.close_everything_for_writing(window, cx);
    }

    /// Picking the mode you are in does nothing. Each mode gets back the panels it had.
    pub(crate) fn set_mode(&mut self, editing: bool, window: &mut Window, cx: &mut Context<Self>) {
        if editing == self.model.read(cx).layer {
            return;
        }
        if editing {
            self.enter_editing(cx);
            return self.put_back_panels(true, window, cx);
        }
        self.remember_panels(true, cx);
        self.model.update(cx, |m, cx| m.set_layer(cx, false));
        self.lab_leave_editing(window, cx);
        self.close_everything_for_writing(window, cx);
        self.put_back_panels(false, window, cx);
    }

    /// What the mode being left has open, for when it comes back.
    fn remember_panels(&mut self, editing: bool, cx: &mut Context<Self>) {
        use crate::prefs::{LeftTab, Panels};
        let left = if self.lab.open {
            Some(LeftTab::Review)
        } else if self.sidebar.is_some() {
            Some(LeftTab::Alternatives)
        } else if self.panel_open() {
            Some(LeftTab::Project)
        } else {
            None
        };
        let now = Panels { left, cuts: self.drawer.is_some() };
        let all = Prefs::get(cx).panels;
        if (if editing { all.editing } else { all.writing }) != now {
            crate::sheets::set_prefs(cx, |p| *(if editing { &mut p.panels.editing } else { &mut p.panels.writing }) = now);
        }
    }

    /// Only by the mode switch: a command that goes to editing on its way brings its own panel.
    /// The keyboard stays on the page.
    fn put_back_panels(&mut self, editing: bool, window: &mut Window, cx: &mut Context<Self>) {
        use crate::prefs::LeftTab;
        let all = Prefs::get(cx).panels;
        let want = if editing { all.editing } else { all.writing };
        // Writing has closed everything first; editing only adds, so a list you go on using stays.
        match want.left {
            // Writing keeps to the Project tab.
            Some(LeftTab::Review) if editing => self.open_review(window, cx),
            Some(LeftTab::Alternatives) if editing => self.open_sidebar(None, window, cx),
            Some(_) => self.show_project_tab(window, cx),
            None => {}
        }
        if let Some(tab) = want.left.filter(|_| self.project.is_none()) {
            self.left_alts = tab == LeftTab::Alternatives;
        }
        if want.cuts {
            self.open_drawer(window, cx);
            self.sync_comment_ticks(cx);
        }
        self.focus_page(window, cx);
        cx.notify();
    }

    /// The mode switch pulses either way, so the underlines appearing have a reason you can see.
    pub(crate) fn enter_editing(&mut self, cx: &mut Context<Self>) {
        if self.model.read(cx).layer {
            return;
        }
        self.remember_panels(false, cx);
        self.model.update(cx, |m, cx| m.set_layer(cx, true));
        let n = self.mode_pulse.map_or(0, |(_, n)| n + 1);
        self.mode_pulse = Some((Instant::now(), n));
        cx.notify();
    }

    /// They act on groups writing doesn't show: with something to act on, go to editing first.
    fn editing_for_page_command(&mut self, groups_after_caret: bool, cx: &mut Context<Self>) {
        if self.model.read(cx).layer {
            return;
        }
        let page = self.page.read(cx);
        let some = if groups_after_caret { !page.groups().is_empty() } else { page.caret_group().is_some() };
        if some {
            self.enter_editing(cx);
        }
    }

    fn close_everything_for_writing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.count_open = false;
        if self.find.is_some() {
            self.on_find_close(window, cx);
        }
        if self.sidebar.is_some() || self.panel_open() || self.lab.open {
            self.toggle_left_panel(window, cx);
        }
        self.drawer = None;
        self.sync_comment_ticks(cx);
        self.focus_page(window, cx);
        cx.notify();
    }

    pub(crate) fn toggle_left_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar.is_some() || self.panel_open() || self.lab.open {
            if self.project.is_none() {
                self.left_alts = self.sidebar.is_some();
            }
            self.left_review = self.lab.open;
            if self.lab.open {
                self.close_review(window, cx);
            }
            if self.sidebar.is_some() {
                self.close_sidebar(window, cx);
            }
            if self.panel_open() {
                self.close_panel(window, cx);
            }
            return;
        }
        if self.left_review && self.model.read(cx).layer {
            return self.open_review(window, cx);
        }
        if self.project.is_some() {
            self.toggle_project(&ToggleProject, window, cx);
            return;
        }
        // Writing keeps to the Project tab: the alternatives are editing's.
        if self.left_alts && self.model.read(cx).layer {
            self.open_sidebar(None, window, cx);
        } else {
            self.open_file_panel(window, cx);
        }
    }

    /// The document menu's first line, and whether it will save itself.
    pub(crate) fn save_state(&self, cx: &App) -> (String, bool) {
        let m = self.model.read(cx);
        let autosave = Prefs::get(cx).autosave != crate::prefs::Autosave::Off;
        if m.path.is_none() {
            return (tr!("not saved yet").into(), m.dirty());
        }
        if m.dirty() {
            return if autosave { (tr!("unsaved changes · autosave on").into(), false) } else { (tr!("unsaved changes · autosave is off").into(), true) };
        }
        let when = match self.saved_at.map(|t| t.elapsed().as_secs()) {
            Some(s) if s < 60 => tr!("saved {n} s ago", n = s),
            Some(s) if s < 3600 => tr!("saved {n} min ago", n = s / 60),
            _ => tr!("saved").into(),
        };
        (format!("{when} · {}", if autosave { tr!("autosave on") } else { tr!("autosave is off") }), false)
    }

    pub(crate) fn open_document_menu(&mut self, at: Point<Pixels>, _: &mut Window, cx: &mut Context<Self>) {
        let (text, warn) = self.save_state(cx);
        let path = self.model.read(cx).path.clone();
        let row = self.project.as_ref().zip(path.as_deref()).is_some_and(|(p, path)| p.tree.entry(path).is_some());
        let at = point(at.x - px(14.), px(HEADER_H + 2.));
        self.count_open = false;
        self.context_menu = Some(crate::context_menu::ContextMenu::document(at, (text.into(), warn), path, row));
        cx.notify();
    }

    fn minimize(&mut self, _: &Minimize, window: &mut Window, _: &mut Context<Self>) {
        window.minimize_window();
    }

    fn zoom(&mut self, _: &Zoom, window: &mut Window, _: &mut Context<Self>) {
        window.zoom_window();
    }

    fn fullscreen(&mut self, _: &ToggleFullScreen, window: &mut Window, _: &mut Context<Self>) {
        window.toggle_fullscreen();
    }

    /// In full, then words only. Cuts are never counted.
    pub(crate) fn counter(&self, room: f32, cx: &App) -> String {
        let m = self.model.read(cx);
        let prefs = Prefs::get(cx);
        let sel = self.page.read(cx).selection();
        let selected = !sel.is_empty();
        let counts = m.counts(selected.then_some(sel));
        let text = |shows| crate::count::counter_text(shows, counts.words, counts.chars, selected);
        let plain = text(prefs.counter_shows);
        // Narrowest of all, the word "selected" goes: the counter's brighter colour still says it.
        let words = text(crate::prefs::CounterShows::Words);
        let bare = crate::count::counter_text(crate::prefs::CounterShows::Words, counts.words, counts.chars, false);
        [plain, words].into_iter().find(|s| fits(s, COUNTER_SIZE, room)).unwrap_or(bare)
    }
}

/// With `DOSTOEVSKY_PERF` set, prints the first frame's Unix time in ms (scripts/perf_check.sh).
fn log_first_frame(window: &mut Window) {
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if std::env::var_os("DOSTOEVSKY_PERF").is_none() || LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    window.on_next_frame(|_, _| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        eprintln!("perf: first frame at {} ms", now.as_millis());
    });
}

pub fn window_title(m: &DocModel) -> String {
    if m.dirty() { tr!("{name} — Edited", name = m.title()) } else { m.title() }
}

pub const HEADER_H: f32 = 36.;
pub const UNDER_HEADER: f32 = HEADER_H + 8.;
/// 8 px past the traffic lights, which end at 74.
const HEADER_LEFT: f32 = 82.;
const HEADER_LEFT_FULL_SCREEN: f32 = 14.;
const HEADER_RIGHT: f32 = 14.;
const HEADER_GAP: f32 = 12.;
const PANEL_BUTTON_GAP: f32 = 8.;
const RIGHT_GAP: f32 = 6.;
const COUNTER_GAP: f32 = 12.;
const TITLE_SIZE: f32 = 12.;
const COUNTER_SIZE: f32 = 11.;
/// The mode switch: two segments, the gap between them, padding and border.
const HEADER_MIDDLE_W: f32 = 2. * MODE_SEG_W + 3. * MODE_SEG_GAP + 2.;
const MODE_SEG_GAP: f32 = 2.;
const ICON_BUTTON_W: f32 = 30.;
/// Fits a two-digit scrap count.
const OVERFLOW_BUTTON_W: f32 = 50.;

/// The UI font is monospaced: 0.6 em a character.
fn fits(text: &str, size: f32, room: f32) -> bool {
    text.chars().count() as f32 * size * 0.6 <= room
}

pub fn side_zone(w: f32) -> f32 {
    (w - HEADER_MIDDLE_W) / 2. - HEADER_GAP
}

pub fn header_left(full_screen: bool) -> f32 {
    if full_screen { HEADER_LEFT_FULL_SCREEN } else { HEADER_LEFT }
}

pub fn title_left(full_screen: bool) -> f32 {
    header_left(full_screen) + ICON_BUTTON_W + PANEL_BUTTON_GAP
}

/// Keeps room for the unsaved dot.
pub fn title_room(w: f32, full_screen: bool) -> f32 {
    side_zone(w) - title_left(full_screen) - TITLE_DOT_W
}

pub fn counter_room(w: f32) -> f32 {
    room_before_find(w) - if find_button_fits(w) { FIND_BUTTON } else { 0. }
}

fn room_before_find(w: f32) -> f32 {
    side_zone(w) - HEADER_RIGHT - OVERFLOW_BUTTON_W - COUNTER_GAP
}

/// In a narrow window the find button gives way, so the counter keeps at least "12,345 words".
pub fn find_button_fits(w: f32) -> bool {
    room_before_find(w) - FIND_BUTTON >= 12. * COUNTER_SIZE * 0.6
}

const FIND_W: f32 = 520.;

const FIND_BUTTON: f32 = ICON_BUTTON_W + RIGHT_GAP;

pub fn count_right(w: f32) -> f32 {
    HEADER_RIGHT + OVERFLOW_BUTTON_W + COUNTER_GAP - 12. + if find_button_fits(w) { FIND_BUTTON } else { 0. }
}

const HEADER_FADE: f32 = 78.;
/// On a see-through window the page stops this far above the bottom edge: the fades can't hide
/// text there.
const GLASS_BOTTOM: f32 = 46.;

/// "— Edited" gives way before the name is cut, and a dot after the name says it instead.
pub fn header_title(m: &DocModel, room: f32) -> String {
    let full = window_title(m);
    if fits(&full, TITLE_SIZE, room) { full } else { m.title() }
}

const PATH_GAP: f32 = 7.2;
const TITLE_DOT_W: f32 = 12.;

/// The file's name is never cut: "— Edited" goes first, then the folder's name shortens, then
/// it goes.
pub fn header_path(folder: &str, m: &DocModel, room: f32) -> (Option<String>, String) {
    let char_w = TITLE_SIZE * 0.6;
    let beside = |name: &str| room - (name.chars().count() as f32 * char_w + char_w + 2. * PATH_GAP);
    let (full, plain) = (window_title(m), m.title());
    for name in [&full, &plain] {
        if fits(folder, TITLE_SIZE, beside(name)) {
            return (Some(folder.to_string()), name.clone());
        }
    }
    let chars = (beside(&plain) / char_w).floor() as usize;
    if chars >= 5 {
        let cut: String = folder.chars().take(chars - 1).collect();
        return (Some(format!("{cut}…")), plain);
    }
    (None, plain)
}

pub fn toggled(on: bool) -> accesskit::Toggled {
    if on { accesskit::Toggled::True } else { accesskit::Toggled::False }
}

/// `id` is the button's group, for the hover colour.
fn icon(id: &'static str, path: &'static str, w: f32, h: f32, active: bool) -> Svg {
    svg()
        .path(path)
        .w(px(w))
        .h(px(h))
        .flex_none()
        .text_color(if active { c(theme::accent()) } else { c(theme::icon()) })
        .when(!active, |s| s.group_hover(id, |s| s.text_color(c(theme::icon_hover()))))
}

const MODE_SEG_W: f32 = 34.;
const MODE_SEG_H: f32 = 20.;

impl Workspace {
    /// Writing and editing side by side: the lit segment is where you are, and a click picks a
    /// mode rather than flipping it.
    fn render_mode_switch(&self, editing: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let segment = |id: &'static str, on: bool, path: &'static str, w: f32| {
            let glyph = match (on, editing) {
                (false, _) => theme::icon(),
                (true, true) => theme::accent(),
                (true, false) => theme::text_2(),
            };
            div()
                .id(id)
                .group(id)
                .role(Role::RadioButton)
                .aria_selected(on)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(MODE_SEG_W))
                .h(px(MODE_SEG_H))
                .rounded(px(5.))
                // Over the header's drag area: a click here isn't a window drag.
                .occlude()
                .cursor_pointer()
                .when(on, |d| d.bg(if editing { ca(theme::accent(), 0.16) } else { ca(theme::wash(), 0.07) }))
                .when(!on, |d| d.hover(|s| s.bg(ca(theme::wash(), 0.03))))
                .child(
                    svg()
                        .path(path)
                        .w(px(w))
                        .h(px(16.))
                        .flex_none()
                        .text_color(c(glyph))
                        .when(!on, |s| s.group_hover(id, |s| s.text_color(c(theme::icon_hover())))),
                )
        };
        div()
            .id("layer")
            .role(Role::RadioGroup)
            .aria_label(tr!("Mode"))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(MODE_SEG_GAP))
            .p(px(MODE_SEG_GAP))
            .rounded(px(7.))
            .border_1()
            .border_color(c(theme::menu_border()))
            .child(
                segment("mode-writing", !editing, "chrome/pen.svg", 16.)
                    .aria_label(tr!("Writing"))
                    .tooltip(tip(tr!("Writing"), Box::new(WritingMode)))
                    .on_click(cx.listener(|this, _, w, cx| this.click_writing(w, cx)))
                    .test_support(),
            )
            .child(
                segment("mode-editing", editing, "chrome/marked.svg", 22.)
                    .aria_label(tr!("Editing"))
                    .tooltip(tip(tr!("Editing"), Box::new(EditingMode)))
                    .on_click(cx.listener(|this, _, w, cx| this.set_mode(true, w, cx)))
                    .test_support(),
            )
            .test_support()
    }
}

const MODE_PULSE: Duration = Duration::from_millis(900);

impl Workspace {
    fn render_mode_pulse(&mut self, layer: bool, cx: &App) -> Option<AnyElement> {
        let (at, n) = self.mode_pulse?;
        let motion = Prefs::get(cx).motion();
        if !layer || at.elapsed() >= MODE_PULSE || motion == crate::prefs::Effects::Off {
            self.mode_pulse = self.mode_pulse.filter(|_| layer && at.elapsed() < MODE_PULSE);
            return None;
        }
        let ring = motion == crate::prefs::Effects::On;
        let accent = theme::accent();
        Some(
            div()
                .absolute()
                .inset_0()
                .rounded(px(6.))
                .with_animation(("mode-pulse", n), Animation::new(MODE_PULSE), move |d, t| {
                    let fade = (1. - t).powi(2);
                    let d = d.bg(ca(accent, 0.18 * fade));
                    if !ring {
                        return d;
                    }
                    let out = 1. - (1. - t).powi(3);
                    let spread = px(-7. * out);
                    d.top(spread).bottom(spread).left(spread).right(spread).rounded(px(6. + 7. * out)).border_1().border_color(ca(accent, 0.8 * fade))
                })
                .into_any_element(),
        )
    }

    #[cfg(test)]
    pub(crate) fn mode_pulsed(&self) -> bool {
        self.mode_pulse.is_some()
    }
}

const SLIDE: Duration = Duration::from_millis(180);

/// Only the panel moves; the page is laid out where it will end up, so it never chases it.
pub(crate) struct Slides {
    /// Off in tests and the screenshot runner, which want panels where they end up.
    pub(crate) enabled: bool,
    left: Option<Instant>,
    right: Option<Instant>,
    was_left: bool,
    was_right: bool,
}

impl Default for Slides {
    fn default() -> Self {
        let enabled = !cfg!(test) && std::env::var_os("DOSTOEVSKY_SCRIPT").is_none();
        Slides { enabled, left: None, right: None, was_left: false, was_right: false }
    }
}

impl Slides {
    /// Notes which panels just opened; returns how far each has come in (0 hidden, 1 in place).
    pub(crate) fn step(&mut self, left: bool, right: bool, motion: crate::prefs::Effects) -> (f32, f32) {
        let animate = self.enabled && motion != crate::prefs::Effects::Off;
        let now = Instant::now();
        if left && !self.was_left {
            self.left = animate.then_some(now);
        }
        if right && !self.was_right {
            self.right = animate.then_some(now);
        }
        (self.was_left, self.was_right) = (left, right);
        (Self::progress(&mut self.left, now), Self::progress(&mut self.right, now))
    }

    fn progress(start: &mut Option<Instant>, now: Instant) -> f32 {
        let Some(t) = *start else { return 1. };
        let k = now.duration_since(t).as_secs_f32() / SLIDE.as_secs_f32();
        if k >= 1. {
            *start = None;
            return 1.;
        }
        1. - (1. - k).powi(3)
    }
}

/// Its drag preview is empty: the panel itself follows the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelEdge {
    Sidebar,
    Overflow,
}

impl Render for PanelEdge {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The sidebar's tab header (with counts) needs its whole default width.
pub const SIDEBAR_MIN: f32 = crate::sidebar::WIDTH;
pub const OVERFLOW_MIN: f32 = 200.;
pub const PANEL_MAX: f32 = 520.;
const PAGE_MIN: f32 = 320.;

pub fn panel_width(edge: PanelEdge, x: f32, window_w: f32, other: f32) -> f32 {
    let (w, min) = match edge {
        PanelEdge::Sidebar => (x, SIDEBAR_MIN),
        PanelEdge::Overflow => (window_w - x, OVERFLOW_MIN),
    };
    w.min(window_w - other - PAGE_MIN).clamp(min, PANEL_MAX).round()
}

pub fn sidebar_width(cx: &App) -> f32 {
    Prefs::get(cx).sidebar_width.clamp(SIDEBAR_MIN, PANEL_MAX)
}

pub fn overflow_width(cx: &App) -> f32 {
    Prefs::get(cx).overflow_width.clamp(OVERFLOW_MIN, PANEL_MAX)
}

pub fn resize_handle(edge: PanelEdge) -> impl IntoElement {
    let id = match edge {
        PanelEdge::Sidebar => "sidebar-edge",
        PanelEdge::Overflow => "overflow-edge",
    };
    let d = div().id(id).absolute().top_0().bottom_0().w(px(7.)).cursor_col_resize();
    let d = match edge {
        PanelEdge::Sidebar => d.right(px(-4.)),
        PanelEdge::Overflow => d.left(px(-4.)),
    };
    d.on_drag(edge, |edge, _, _, cx| cx.new(|_| *edge)).test_support()
}

/// `underline` marks a toggle that is on.
pub fn icon_button(id: &'static str, label: impl IntoElement, underline: bool) -> Stateful<Div> {
    let spoken = match id {
        "panel" => tr!("Left panel"),
        "overflow-button" => tr!("Cuts"),
        "find-button" => tr!("Find"),
        other => other,
    };
    div()
        .id(id)
        .group(id)
        .role(Role::Button)
        .aria_label(spoken)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(ICON_BUTTON_W))
        .h(px(28.))
        .rounded(px(6.))
        // Over the header's drag area: a click here isn't a window drag.
        .occlude()
        .cursor_pointer()
        .hover(|s| s.bg(ca(theme::wash(), 0.03)))
        .child(label)
        .when(underline, |d| {
            d.child(div().absolute().left(px(4.)).right(px(4.)).bottom(px(1.)).h(px(2.)).rounded(px(2.)).bg(c(theme::accent())))
        })
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        log_first_frame(window);
        if self.pending_disk.is_some() {
            self.resolve_disk_change(window, cx);
        }
        self.close_drawer_after_drag(cx);
        self.finish_drags(window, cx);
        let m = self.model.read(cx);
        let layer = m.layer;
        let dirty = m.dirty();
        let window_w = f32::from(window.viewport_size().width);
        let full_screen = window.is_fullscreen();
        let header_left = header_left(full_screen);
        let title_room = title_room(window_w, full_screen);
        let (folder, title) = match self.project_name() {
            Some(name) => header_path(&name, m, title_room),
            None => (None, header_title(m, title_room)),
        };
        // "— Edited" gave way: a dot says it instead.
        let dot = dirty && title != window_title(m);
        let scraps = self.panel_count(cx);
        let chrome_alpha = if self.typing { 0.15 } else { 1. };
        let drawer_open = self.drawer.is_some();
        let find_open = self.find.is_some();
        let find_button = find_button_fits(window_w);
        let left_open = self.sidebar.is_some() || self.panel_open() || self.lab.open;
        let menu_open = self.context_menu.as_ref().is_some_and(|m| m.heading.is_some());
        let selecting = !self.page.read(cx).selection().is_empty();
        let counter = self.counter(counter_room(window_w), cx);
        let status = self.status.as_ref().map(|(s, _)| s.clone());
        let reveal = self.status_reveal.clone();
        // The update check's line: in the status line's place, never over a status or while typing.
        #[cfg(feature = "updates")]
        let update_line = (status.is_none() && !self.typing).then(|| crate::updates::line(cx)).flatten();
        #[cfg(not(feature = "updates"))]
        let update_line: Option<AnyElement> = None;

        let mut ctx = KeyContext::new_with_defaults();
        ctx.add("Workspace");
        if self.sidebar.is_some() {
            ctx.add("sidebar");
        }
        if self.panel_open() && self.sidebar.is_none() {
            ctx.add("project_panel");
        }
        let glass = theme::glass();
        let left = if left_open { sidebar_width(cx) } else { 0. };
        let right = if self.drawer.is_some() { overflow_width(cx) } else { 0. };
        let (left_in, right_in) = self.slides.step(left > 0., right > 0., Prefs::get(cx).motion());
        if left_in < 1. || right_in < 1. {
            window.request_animation_frame();
        }
        // Each panel draws at its edge; a sliding one is drawn in a strip that starts off screen.
        let slide = |el: AnyElement, from_left: bool, w: f32, k: f32| -> AnyElement {
            if k >= 1. {
                return el;
            }
            let d = div().absolute().top_0().bottom_0().w(px(w));
            let off = px(-(1. - k) * w);
            let d = if from_left { d.left(off) } else { d.right(off) };
            d.child(el).into_any_element()
        };
        let sidebar = self.render_sidebar(window, cx).map(|e| slide(e, true, left, left_in));
        let review = self.render_review(window, cx).map(|e| slide(e, true, left, left_in));
        let project_panel = self.render_project(window, cx).map(|e| slide(e, true, left, left_in));
        let file_panel = self.render_file_panel(cx).map(|e| slide(e, true, left, left_in));
        let in_project = self.project.is_some();
        let has_path = self.model.read(cx).path.is_some();
        let drawer = self.render_drawer(cx).map(|e| slide(e, false, right, right_in));
        let menu = self.render_context_menu(window, cx);
        let peek = self.render_peek(cx);
        let comment_field = self.render_comment_field(window, cx);
        let comment_peek = self.render_comment_peek(cx);
        let selection_buttons = self.render_selection_buttons(cx);
        let count = self.render_count(window, cx);
        let overlay = self.overlay.is_active().then(|| self.overlay.element());
        let sheet = self.render_sheet(window, cx);
        let detached = self.model.read(cx).kept.detached.len();
        let lab_waiting = !layer && (self.lab.waiting() || !self.lab.parked.is_empty());
        let mode_pulse = self.render_mode_pulse(layer, cx);
        let mode_switch = self.render_mode_switch(layer, cx);

        div()
            .key_context(ctx)
            .size_full()
            .relative()
            // See-through windows paint the page and the header strip separately, so no part
            // of the background is painted twice (twice would be more solid there).
            .when(!glass, |d| d.bg(c(theme::bg())))
            .font_family(theme::UI)
            .font_features(FontFeatures::disable_ligatures())
            .text_color(c(theme::text()))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::revert))
            .on_action(cx.listener(Self::close_window))
            .on_action(cx.listener(Self::toggle_layer))
            .on_action(cx.listener(Self::writing_mode))
            .on_action(cx.listener(Self::editing_mode))
            // Before the page runs them: writing switches to editing first.
            .capture_action(cx.listener(|this, _: &CycleAtCaretNext, _, cx| this.editing_for_page_command(false, cx)))
            .capture_action(cx.listener(|this, _: &CycleAtCaretPrev, _, cx| this.editing_for_page_command(false, cx)))
            .capture_action(cx.listener(|this, _: &BackToOriginal, _, cx| this.editing_for_page_command(false, cx)))
            .capture_action(cx.listener(|this, _: &SettleOnThis, _, cx| this.editing_for_page_command(false, cx)))
            .capture_action(cx.listener(|this, _: &NextGroup, _, cx| this.editing_for_page_command(true, cx)))
            .capture_action(cx.listener(|this, _: &PrevGroup, _, cx| this.editing_for_page_command(true, cx)))
            .on_action(cx.listener(Self::find))
            .on_action(cx.listener(Self::find_replace))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_previous))
            .on_action(cx.listener(Self::use_selection_for_find))
            .on_action(cx.listener(Self::alternatives_for_selection))
            .on_action(cx.listener(Self::settle_on_this))
            .on_action(cx.listener(Self::all_alternatives))
            .on_action(cx.listener(Self::sidebar_prev_group))
            .on_action(cx.listener(Self::sidebar_next_group))
            .on_action(cx.listener(Self::cut_toggle))
            .on_action(cx.listener(Self::move_unit_up))
            .on_action(cx.listener(Self::move_unit_down))
            .on_action(cx.listener(Self::remove_all_ghosted))
            .on_action(cx.listener(Self::toggle_overflow))
            .on_action(cx.listener(Self::stash))
            .on_action(cx.listener(Self::insert_overflow_note))
            .on_action(cx.listener(Self::view_source))
            .on_action(cx.listener(Self::show_shortcuts))
            .on_action(cx.listener(Self::show_preferences))
            .on_action(cx.listener(Self::copy_final_markdown))
            .on_action(cx.listener(Self::export))
            .on_action(cx.listener(Self::print))
            .on_action(cx.listener(Self::ai_alternatives))
            .on_action(cx.listener(Self::toggle_lab))
            .on_action(cx.listener(Self::add_comment))
            .on_action(cx.listener(Self::delete_comment))
            .on_action(cx.listener(Self::delete_page_comments))
            .on_action(cx.listener(Self::next_mark))
            .on_action(cx.listener(Self::prev_mark))
            .on_action(cx.listener(Self::minimize))
            .on_action(cx.listener(Self::zoom))
            .on_action(cx.listener(Self::fullscreen))
            .when(in_project, |d| {
                d.on_action(cx.listener(Self::toggle_project))
                    .on_action(cx.listener(Self::previous_file))
                    .on_action(cx.listener(Self::next_file))
                    .on_action(cx.listener(Self::new_in_project))
                    .on_action(cx.listener(Self::find_in_project))
                    .on_action(cx.listener(Self::list_trash))
                    .on_action(cx.listener(Self::show_project_settings))
            })
            .on_action(cx.listener(Self::new_project))
            // ⌘P is the panel button in a single file too: the list of every group.
            .when(!in_project, |d| d.on_action(cx.listener(|this, _: &ToggleProject, w, cx| this.toggle_left_panel(w, cx))))
            .when(!in_project && has_path, |d| d.on_action(cx.listener(Self::open_enclosing_folder)))
            .on_drop(cx.listener(|this, _: &crate::project_ui::RowDrag, _, cx| this.clear_drop(cx)))
            .capture_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, _, cx| {
                // A click outside the sidebar saves a variant being edited there.
                if f32::from(e.position.x) > sidebar_width(cx) {
                    this.end_variant_edit(true, cx);
                }
            }))
            .on_drop(|paths: &ExternalPaths, _, cx| {
                for p in paths.paths() {
                    if files::is_markdown(p) || p.is_dir() {
                        crate::open_path(p.clone(), cx);
                    }
                }
            })
            .on_drag_move(cx.listener(|_, e: &DragMoveEvent<PanelEdge>, _, cx| {
                let edge = *e.drag(cx);
                let mut p = Prefs::get(cx).clone();
                let (x, window_w) = (f32::from(e.event.position.x - e.bounds.left()), f32::from(e.bounds.size.width));
                match edge {
                    PanelEdge::Sidebar => p.sidebar_width = panel_width(edge, x, window_w, overflow_width(cx)),
                    PanelEdge::Overflow => p.overflow_width = panel_width(edge, x, window_w, sidebar_width(cx)),
                }
                if p != *Prefs::get(cx) {
                    cx.set_global(p);
                    cx.notify();
                }
            }))
            .on_drop(|_: &PanelEdge, _, cx| {
                Prefs::get(cx).save();
                cx.refresh_windows();
            })
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                if this.typing {
                    this.typing = false;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("page-area")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(left))
                    .right(px(right))
                    .on_drop(cx.listener(|this, d: &crate::overflow::NoteDrag, window, cx| this.drop_note(d, window, cx)))
                    .on_drop(cx.listener(|this, d: &crate::drag::TextDrag, window, cx| this.drop_text(d, window, cx)))
                    .on_drop(cx.listener(|this, d: &crate::drag::DetachedDrag, window, cx| this.drop_detached(d, window, cx)))
                    .on_drag_move(cx.listener(|this, e: &DragMoveEvent<crate::drag::DetachedDrag>, _, cx| {
                        let scope = e.drag(cx).scope;
                        this.drag_over_detached(e.event.position, e.bounds, scope, cx);
                    }))
                    .on_drag_move(cx.listener(|this, e: &DragMoveEvent<crate::drag::TextDrag>, _, cx| {
                        let block = e.drag(cx).block;
                        this.drag_over_page(e.event.position, e.bounds, block, cx);
                    }))
                    .on_drag_move(cx.listener(|this, e: &DragMoveEvent<crate::overflow::NoteDrag>, _, cx| {
                        let block = e.drag(cx).block;
                        this.drag_over_page(e.event.position, e.bounds, block, cx);
                    }))
                    .when(glass, |d| {
                        d.child(div().absolute().top(px(HEADER_H)).bottom_0().left_0().right_0().bg(theme::surface(theme::bg())))
                    })
                    .child(if glass {
                        // A fade over a see-through page can't hide the text under it, so the
                        // page is cut off below the header and above the bottom chrome instead.
                        div()
                            .absolute()
                            .top(px(HEADER_H))
                            .bottom(px(GLASS_BOTTOM))
                            .left_0()
                            .right_0()
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-HEADER_H))
                                    .bottom(px(-GLASS_BOTTOM))
                                    .left_0()
                                    .right_0()
                                    .child(self.page.clone()),
                            )
                            .into_any_element()
                    } else {
                        self.page.clone().into_any_element()
                    })
                    // The page fades out under the header and the bottom chrome: solid behind
                    // the header, which has no border, then gone a little below it.
                    .when(!glass, |d| d.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top_0()
                            .h(px(HEADER_FADE))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(c(theme::bg()), HEADER_H / HEADER_FADE),
                                linear_color_stop(ca(theme::bg(), 0.), 1.),
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(90.))
                            .bg(linear_gradient(180., linear_color_stop(ca(theme::bg(), 0.), 0.), linear_color_stop(c(theme::bg()), 0.85))),
                    )),
            )
            .when(glass, |d| d.child(div().absolute().top_0().left_0().right_0().h(px(HEADER_H)).bg(theme::surface(theme::bg()))))
            .children(sidebar)
            .children(review)
            .children(project_panel)
            .children(file_panel)
            .children(drawer)
            .when(detached > 0 && self.sheet.is_none(), |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(UNDER_HEADER))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("detached-banner")
                                .flex()
                                .gap_3()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(c(theme::bg_raised()))
                                .border_1()
                                .border_color(ca(theme::para_bar(), 0.6))
                                .text_size(px(12.))
                                .text_color(c(theme::text_2()))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, w, cx| this.open_sheet(crate::sheets::Sheet::Detached, w, cx)))
                                .child(trn!("{n} alternative group lost its place.", "{n} alternative groups lost their place.", detached))
                                .child(div().text_color(c(theme::accent())).child(tr!("Review")))
                                .test_support(),
                        ),
                )
            })
            .children(selection_buttons)
            .children(sheet)
            .children(peek)
            .children(comment_field)
            .children(comment_peek)
            .children(menu)
            .children(overlay)
            // Never wider than the room left of an open drawer, so a narrow window squeezes its
            // fields instead of pushing it off the left edge.
            .children(self.find.clone().map(|f| {
                let edge = 24. + right * right_in;
                let room = (window_w - left * left_in - edge - 24.).max(0.);
                div().absolute().top(px(UNDER_HEADER)).right(px(edge)).w(px(FIND_W.min(room))).child(f)
            }))
            .children(count)
            // The two sides take equal shares, so the mode switch stays on the window's centre;
            // their padding lives inside them, where it can't tip the balance.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(HEADER_H))
                    .flex()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |e, window, _| {
                        if e.click_count == 2 {
                            window.zoom_window();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            div()
                                .pl(px(header_left))
                                .pr(px(HEADER_GAP))
                                .flex()
                                .items_center()
                                .gap(px(PANEL_BUTTON_GAP))
                                .opacity(chrome_alpha)
                                .child(
                                    icon_button("panel", icon("panel", "chrome/panel-left.svg", 20., 16., left_open), left_open)
                                        .aria_label(if in_project { tr!("Project list") } else { tr!("Project and alternatives") })
                                        .aria_toggled(toggled(left_open))
                                        .tooltip(tip(if in_project { tr!("Project list") } else { tr!("Project and alternatives") }, Box::new(ToggleProject)))
                                        .on_click(cx.listener(|this, _, w, cx| this.toggle_left_panel(w, cx)))
                                        .test_support(),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(TITLE_SIZE))
                                        .text_color(c(theme::muted()))
                                        .flex()
                                        .items_center()
                                        .children(folder.map(|name| {
                                            div()
                                                .flex()
                                                .flex_none()
                                                .items_center()
                                                .gap(px(PATH_GAP))
                                                .pr(px(PATH_GAP))
                                                .child(
                                                    div()
                                                        .id("project-name")
                                                        .role(Role::Button)
                                                        .aria_label(tr!("Show the project"))
                                                        .occlude()
                                                        .cursor_pointer()
                                                        .hover(|d| d.text_color(c(theme::text_2())))
                                                        .on_click(|_, w, cx| w.dispatch_action(Box::new(ToggleProject), cx))
                                                        .child(name)
                                                        .test_support(),
                                                )
                                                .child(div().text_color(c(theme::faint())).child("/"))
                                        }))
                                        .child(
                                            div()
                                                .id("title")
                                                .group("title")
                                                .role(Role::Button)
                                                .aria_label(tr!("Document menu"))
                                                .min_w_0()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .occlude()
                                                .cursor_pointer()
                                                .hover(|d| d.text_color(c(theme::text_2())))
                                                .when(menu_open, |d| d.text_color(c(theme::text_2())))
                                                .on_click(cx.listener(|this, e: &ClickEvent, w, cx| this.open_document_menu(e.position(), w, cx)))
                                                .child(div().min_w_0().truncate().child(title))
                                                .when(dot, |d| {
                                                    d.child(div().id("unsaved-dot").flex_none().size(px(6.)).rounded_full().bg(c(theme::accent())).test_support())
                                                })
                                                .child(
                                                    div()
                                                        .flex_none()
                                                        .text_size(px(9.))
                                                        .text_color(if menu_open { c(theme::text_2()) } else { ca(theme::text_2(), 0.) })
                                                        .group_hover("title", |s| s.text_color(c(theme::text_2())))
                                                        .child("▾"),
                                                )
                                                .test_support(),
                                        ),
                                ),
                        ),
                    )
                    // The waiting dot hangs off the mode switch’s right side (writing only), so the mode
                    // switch stays on the window's centre.
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_none()
                            .items_center()
                            .opacity(chrome_alpha)
                            .children(mode_pulse)
                            .child(mode_switch)
                            .when(lab_waiting, |d| {
                                d.child(
                                    div().absolute().left(relative(1.)).top_0().bottom_0().flex().items_center().child(
                                        div().id("lab-waiting").size(px(5.)).rounded_full().bg(c(theme::mark())).aria_label(tr!("AI results are waiting in editing")).test_support(),
                                    ),
                                )
                            }),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            div()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(RIGHT_GAP))
                                .pl(px(HEADER_GAP))
                                .pr(px(HEADER_RIGHT))
                                .opacity(chrome_alpha)
                                .child(
                                    div()
                                        .id("counter")
                                        .role(Role::Button)
                                        .aria_label(tr!("Word count; opens Statistics"))
                                        .relative()
                                        .flex_none()
                                        .h(px(28.))
                                        .flex()
                                        .items_center()
                                        .mr(px(COUNTER_GAP - RIGHT_GAP))
                                        .occlude()
                                        .text_size(px(COUNTER_SIZE))
                                        .cursor_pointer()
                                        .text_color(if self.count_open { c(theme::accent()) } else if selecting { c(theme::muted()) } else { c(theme::counter()) })
                                        .when(!self.count_open, |d| d.hover(|s| s.text_color(c(theme::counter_hover()))))
                                        .on_click(cx.listener(|this, _, _, cx| this.toggle_count(cx)))
                                        .child(counter)
                                        .when(self.count_open, |d| {
                                            d.child(div().absolute().left_0().right_0().bottom(px(1.)).h(px(2.)).rounded(px(2.)).bg(c(theme::accent())))
                                        })
                                        .aria_toggled(toggled(self.count_open))
                                        .test_support(),
                                )
                                .when(find_button, |d| {
                                    d.child(
                                        icon_button("find-button", icon("find-button", "chrome/find.svg", 18., 18., find_open), find_open)
                                            .aria_label(tr!("Find"))
                                            .aria_toggled(toggled(find_open))
                                            .tooltip(tip(tr!("Find"), Box::new(Find)))
                                            .on_click(cx.listener(|this, _, w, cx| this.toggle_find(w, cx)))
                                            .test_support(),
                                    )
                                })
                                .child(
                                    icon_button(
                                        "overflow-button",
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(icon("overflow-button", "chrome/panel-right.svg", 20., 16., drawer_open))
                                            .when(scraps > 0, |d| {
                                                d.child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(if drawer_open { c(theme::accent()) } else { c(theme::muted()) })
                                                        .child(scraps.to_string()),
                                                )
                                            }),
                                        drawer_open,
                                    )
                                    .w_auto()
                                    .px(px(5.))
                                    .aria_label(match scraps {
                                        0 => tr!("Cuts").to_string(),
                                        n => trn!("Cuts, {n} cut", "Cuts, {n} cuts", n),
                                    })
                                    .aria_toggled(toggled(drawer_open))
                                    .tooltip(tip(
                                        match scraps {
                                            0 => tr!("Cuts").to_string(),
                                            n => trn!("Cuts · {n} cut", "Cuts · {n} cuts", n),
                                        },
                                        Box::new(ToggleOverflow),
                                    ))
                                    .on_click(|_, w, cx| w.dispatch_action(Box::new(ToggleOverflow), cx))
                                    .on_drag_move(cx.listener(Self::spring_overflow))
                                    .when(self.spring.is_some() && cx.has_active_drag(), |d| {
                                        d.rounded(px(7.)).border_1().border_color(ca(theme::accent(), 0.7)).bg(ca(theme::accent(), 0.12))
                                    })
                                    .test_support(),
                                ),
                        ),
                    ),
            )
            .children(status.map(|s| {
                div()
                    .absolute()
                    .bottom(px(22.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .gap(px(12.))
                    .text_size(px(12.))
                    .text_color(c(theme::muted()))
                    .child(s)
                    .children(reveal.map(|p| {
                        div()
                            .id("status-reveal")
                            .role(Role::Link)
                            .aria_label(tr!("Show in Finder"))
                            .cursor_pointer()
                            .text_color(c(theme::accent()))
                            .hover(|d| d.underline())
                            .child(tr!("Show in Finder"))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, _, cx| cx.reveal_path(&p))
                            .test_support()
                    }))
            }))
            .children(update_line.map(|line| {
                div()
                    .absolute()
                    .bottom(px(22.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(c(theme::muted()))
                    .child(line)
            }))
    }
}

pub struct Tip {
    label: SharedString,
    keys: Option<String>,
}

impl Tip {
    pub(crate) fn new(label: impl Into<SharedString>, keys: Option<String>) -> Tip {
        Tip { label: label.into(), keys }
    }
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap(px(10.))
            .px(px(8.))
            .py(px(3.))
            .rounded(px(5.))
            .bg(c(theme::bg_raised()))
            .border_1()
            .border_color(c(theme::menu_border()))
            .font_family(theme::UI)
            .text_size(px(11.))
            .line_height(px(16.))
            .text_color(c(theme::text_2()))
            .child(self.label.clone())
            .children(self.keys.clone().map(|k| div().text_color(c(theme::muted())).child(k)))
    }
}

pub(crate) fn tip(label: impl Into<SharedString>, action: Box<dyn Action>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| {
        let keys = crate::context_menu::shortcut_label(action.as_ref(), cx);
        let label = label.clone();
        cx.new(|_| Tip { label, keys }).into()
    }
}

#[cfg(test)]
mod slide_tests {
    use super::Slides;
    use crate::prefs::Effects;

    fn on() -> Slides {
        Slides { enabled: true, ..Slides::default() }
    }

    /// ALT-13, CUT-9
    #[test]
    fn a_panel_slides_in_when_it_opens() {
        let mut s = on();
        let (l, r) = s.step(true, false, Effects::On);
        assert!(l < 0.2, "{l}");
        assert_eq!(r, 1.);
        std::thread::sleep(super::SLIDE);
        assert_eq!(s.step(true, false, Effects::On), (1., 1.));
        s.step(false, false, Effects::On);
        assert!(s.step(false, true, Effects::On).1 < 0.2);
    }

    #[test]
    fn no_slide_with_effects_off() {
        let mut s = on();
        assert_eq!(s.step(true, true, Effects::Off), (1., 1.));
    }

    #[test]
    fn tests_never_slide() {
        let mut s = Slides::default();
        assert_eq!(s.step(true, true, Effects::On), (1., 1.));
    }
}
