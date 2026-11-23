use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::files;
use crate::project::{self, Kind, Rename, Settings};
use crate::sheets::{row, PrefSection, Sheet};
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::Workspace;

pub struct NewProjectForm {
    pub name: Entity<TextView>,
    pub place: PathBuf,
    pub kind: Kind,
    pub notes: bool,
    pub parts: bool,
    pub years: bool,
    pub error: Option<String>,
    _sub: Subscription,
}

#[derive(Clone, Debug)]
pub struct FirstOpen {
    pub kind: Kind,
    pub notes: Option<String>,
    pub move_loose: bool,
    /// The guess from the file names, and how many files back it.
    pub guess: Kind,
    pub counts: project::Counts,
    pub folders: Vec<String>,
    pub loose: Vec<PathBuf>,
}

fn describe(kind: Kind) -> &'static str {
    match kind {
        Kind::Book => tr!("Chapters in order, counted together. Subfolders are parts."),
        Kind::Journal => tr!("A page a day, newest first. Today opens blank."),
        Kind::Folder => tr!("Essays, posts, letters. Last changed first."),
    }
}

fn kind_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Book => tr!("Book"),
        Kind::Journal => tr!("Journal"),
        Kind::Folder => tr!("Folder"),
    }
}

fn open_as(kind: Kind) -> &'static str {
    match kind {
        Kind::Book => tr!("Open as Book"),
        Kind::Journal => tr!("Open as Journal"),
        Kind::Folder => tr!("Open as Folder"),
    }
}

fn label(text: impl Into<SharedString>) -> Div {
    div().mt_3().mb_1().text_size(px(10.5)).text_color(c(theme::muted())).child(text.into())
}

fn card(id: &'static str, kind: Kind, on: bool, badge: Option<&'static str>, lines: Vec<String>) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_lg()
        .cursor_pointer()
        .bg(c(theme::bg()))
        .border_1()
        .border_color(if on { c(theme::accent()) } else { c(theme::line()) })
        .hover(|d| d.border_color(if on { c(theme::accent()) } else { c(theme::muted()) }))
        .role(Role::RadioButton)
        .aria_label(kind_title(kind))
        .aria_selected(on)
        .children(badge.map(|b| {
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .px_1()
                .rounded_sm()
                .border_1()
                .border_color(ca(theme::original(), 0.5))
                .text_size(px(9.5))
                .text_color(c(theme::original()))
                .child(b)
        }))
        .child(div().font_family(theme::hand()).text_size(theme::hand_px(23.)).text_color(if on { c(theme::accent()) } else { c(theme::text_2()) }).child(kind_title(kind)))
        .child(div().min_h(px(51.)).text_size(px(11.)).line_height(px(17.)).text_color(c(theme::muted())).child(describe(kind)))
        .child(
            div()
                .mt_1()
                .pt_2()
                .border_t_1()
                .border_color(c(theme::line()))
                .flex()
                .flex_col()
                .text_size(px(11.))
                .line_height(px(18.))
                .children(lines.into_iter().map(|l| {
                    let faint = l.ends_with('/') || l.starts_with('(');
                    div().truncate().text_color(if faint { c(theme::faint()) } else { c(theme::text_2()) }).child(l)
                })),
        )
}

fn check(id: &'static str, on: bool, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .py(px(3.))
        .cursor_pointer()
        .text_color(c(theme::text_2()))
        .role(Role::CheckBox)
        .aria_label(text.clone())
        .aria_toggled(crate::workspace::toggled(on))
        .child(
            div()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .border_color(if on { c(theme::accent()) } else { c(theme::icon_line()) })
                .when(on, |d| d.bg(c(theme::accent())).text_size(px(10.)).text_color(c(theme::bg())).child("✓")),
        )
        .child(text)
}

fn radio(id: (&'static str, usize), on: bool, enabled: bool, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .py(px(3.))
        .when(enabled, |d| d.cursor_pointer())
        .text_color(if enabled { c(theme::text_2()) } else { c(theme::faint()) })
        .role(Role::RadioButton)
        .aria_label(text.clone())
        .aria_selected(on)
        .child(
            div()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(if on { c(theme::accent()) } else { c(theme::icon_line()) })
                .when(on, |d| d.child(div().size(px(6.)).rounded_full().bg(c(theme::accent())))),
        )
        .child(text)
}

fn chip(id: (&'static str, usize), on: bool, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .px_2()
        .py(px(2.))
        .rounded_md()
        .cursor_pointer()
        .border_1()
        .border_color(if on { c(theme::accent()) } else { c(theme::line()) })
        .when(on, |d| d.bg(ca(theme::accent(), 0.18)).text_color(c(theme::text())))
        .when(!on, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text_2()))))
        .role(Role::RadioButton)
        .aria_label(text.clone())
        .aria_selected(on)
        .child(text)
}

fn button(id: &'static str, text: impl Into<SharedString>, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_4()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .border_1()
        .border_color(if primary { c(theme::accent()) } else { c(theme::line()) })
        .when(primary, |d| d.bg(c(theme::accent())).text_color(c(theme::bg())))
        .when(!primary, |d| d.text_color(c(theme::text_2())).hover(|d| d.text_color(c(theme::text()))))
        .child(text.into())
}

fn buttons(children: Vec<AnyElement>) -> Div {
    div().mt_4().flex().items_center().justify_end().gap_3().children(children)
}

fn tilde(path: &Path) -> String {
    match crate::platform::home() {
        Some(home) if path.starts_with(&home) => format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display()).trim_end_matches('/').to_string(),
        _ => path.display().to_string(),
    }
}

fn notes_choices(folders: &[String], current: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = folders.iter().take(8).cloned().collect();
    if !out.iter().any(|f| f.eq_ignore_ascii_case("notes")) {
        out.push("notes".into());
    }
    if let Some(cur) = current {
        if !out.iter().any(|f| f == cur) {
            out.push(cur.to_string());
        }
    }
    out
}

/// Targets for moving `paths` into `dir`, none landing on another.
fn into_notes(tree: &project::Tree, paths: &[PathBuf], dir: &Path) -> Vec<Rename> {
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        let mut r = project::plan_to_notes(tree, p, dir);
        if taken.contains(&r.to) {
            let stem = r.to.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = r.to.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "md".into());
            r.to = (2..).map(|i| dir.join(format!("{stem}-{i}.{ext}"))).find(|q| !taken.contains(q) && !q.exists()).unwrap_or(r.to);
        }
        taken.insert(r.to.clone());
        out.push(r);
    }
    out
}

impl FirstOpen {
    fn new(tree: &project::Tree) -> FirstOpen {
        let s = tree.settings();
        let mut f = FirstOpen {
            kind: s.kind,
            notes: s.notes,
            move_loose: false,
            guess: tree.kind,
            counts: tree.counts,
            folders: project::notes_candidates(&tree.root, tree.kind),
            loose: Vec::new(),
        };
        f.loose = tree.loose.iter().map(|e| e.path.clone()).collect();
        f
    }

    fn settings(&self) -> Settings {
        Settings { kind: self.kind, notes: self.notes.clone(), title: None, author: None, date: None }
    }

    fn reason(&self) -> String {
        let n = self.counts;
        match self.guess {
            Kind::Book => trn!("{count} of its {n} file starts with a number, so Book is picked.", "{count} of its {n} files start with a number, so Book is picked.", n.total, count = n.numbered),
            Kind::Journal => trn!("{count} of its {n} file starts with a date, so Journal is picked.", "{count} of its {n} files start with a date, so Journal is picked.", n.total, count = n.dated),
            Kind::Folder => tr!("Its files aren’t numbered or dated, so Folder is picked.").into(),
        }
    }
}

impl Workspace {
    pub(crate) fn ask_project_type(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.project else { return };
        let form = FirstOpen::new(&p.tree);
        self.open_sheet(Sheet::ProjectType(form), window, cx);
    }

    fn change_first_open(&mut self, f: impl FnOnce(&mut FirstOpen), cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else { return };
        let Some(Sheet::ProjectType(form)) = &mut self.sheet else { return };
        let was = form.kind;
        f(form);
        if was == Kind::Folder && form.notes.is_none() {
            form.notes = project::notes_guess(&root, form.kind);
        }
        form.folders = project::notes_candidates(&root, form.kind);
        if form.notes.as_ref().is_some_and(|n| root.join(n).is_dir() && !form.folders.contains(n)) {
            form.notes = None;
        }
        let tree = project::scan_as(&root, Some(form.settings()));
        form.loose = tree.loose.iter().map(|e| e.path.clone()).collect();
        if form.notes.is_none() || form.kind == Kind::Folder {
            form.move_loose = false;
        }
        cx.notify();
    }

    pub(crate) fn confirm_first_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::ProjectType(form)) = self.sheet.take() else { return };
        self.close_sheet(window, cx);
        let s = form.settings();
        if !self.apply_settings(s, cx) {
            return;
        }
        let Some(p) = &self.project else { return };
        if form.move_loose {
            if let Some(dir) = p.tree.notes_dir.clone() {
                let renames = into_notes(&p.tree, &form.loose, &dir);
                if self.apply_renames(renames, true, window, cx) {
                    self.flash_status(tr!("Moved into the notes folder. ⌘Z to undo"), cx);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn render_first_open(&mut self, form: FirstOpen, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let name = self.project_name().unwrap_or_default();
        let title = div().font_family(theme::hand()).text_size(theme::hand_px(25.)).text_color(c(theme::accent())).child(tr!("Open “{name}” as", name = name)).into_any_element();
        let lines = |k: Kind| -> Vec<String> {
            let n = form.counts;
            match k {
                Kind::Book => vec![trn!("{n} numbered file", "{n} numbered files", n.numbered)],
                Kind::Journal => vec![trn!("{n} dated file", "{n} dated files", n.dated)],
                Kind::Folder => vec![trn!("{n} file", "{n} files", n.total)],
            }
        };
        let cards = div().flex().gap_3().children(Kind::ALL.into_iter().map(|k| {
            let id = match k {
                Kind::Book => "kind-book",
                Kind::Journal => "kind-journal",
                Kind::Folder => "kind-folder",
            };
            card(id, k, form.kind == k, (k == form.guess).then_some(tr!("LOOKS LIKE THIS")), lines(k))
                .on_click(cx.listener(move |this, _, _, cx| this.change_first_open(|f| f.kind = k, cx)))
                .test_support()
        }));
        let choices = notes_choices(&form.folders, form.notes.as_deref());
        let notes_row = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(choices.into_iter().enumerate().map(|(i, n)| {
                let on = form.notes.as_deref() == Some(n.as_str());
                let made = if form.folders.contains(&n) { format!("{n}/") } else { tr!("{folder}/ (new)", folder = n) };
                let pick = n.clone();
                chip(("notes-folder", i), on, made).on_click(cx.listener(move |this, _, _, cx| {
                    let pick = pick.clone();
                    this.change_first_open(|f| f.notes = Some(pick), cx)
                })).test_support()
            }))
            .child(chip(("notes-folder", 99), form.notes.is_none(), tr!("none")).on_click(cx.listener(|this, _, _, cx| this.change_first_open(|f| f.notes = None, cx))).test_support());
        let loose_names: Vec<String> = form.loose.iter().take(6).filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).collect();
        let more = form.loose.len().saturating_sub(loose_names.len());
        let (leave_it, leave_them) = match form.kind {
            Kind::Book => (tr!("Leave it where it is, listed as not in the book"), tr!("Leave them where they are, listed as not in the book")),
            Kind::Journal => (tr!("Leave it where it is, listed as not in the journal"), tr!("Leave them where they are, listed as not in the journal")),
            Kind::Folder => (tr!("Leave it where it is, listed as not in the folder"), tr!("Leave them where they are, listed as not in the folder")),
        };
        let notes_name = form.notes.clone().unwrap_or_else(|| "notes".into());
        let can_move = form.notes.is_some();
        let body = div()
            .flex()
            .flex_col()
            .text_size(px(12.5))
            .child(div().text_color(c(theme::muted())).line_height(px(19.)).child(tr!("{reason} Nothing is renamed or moved unless you say so.", reason = form.reason())))
            .child(div().mt_3().child(cards))
            // A plain folder is just its tree: nothing in it is set apart as notes.
            .when(form.kind != Kind::Folder, |d| d.child(label(tr!("NOTES"))).child(notes_row))
            .when(form.kind != Kind::Folder && !form.loose.is_empty(), |d| {
                d.child(label(trn!("THE OTHER FILE", "THE OTHER {n} FILES", form.loose.len())))
                    .child(
                        div()
                            .pb_1()
                            .text_color(c(theme::muted()))
                            .truncate()
                            .child(if more > 0 { tr!("{names}, and {more} more", names = loose_names.join(", "), more = more) } else { loose_names.join(", ") }),
                    )
                    .child(radio(("loose", 0), !form.move_loose, true, if form.loose.len() == 1 { leave_it } else { leave_them }).on_click(cx.listener(|this, _, _, cx| this.change_first_open(|f| f.move_loose = false, cx))).test_support())
                    .child(radio(("loose", 1), form.move_loose, can_move, if form.loose.len() == 1 { tr!("Move it into {folder}/", folder = notes_name) } else { tr!("Move them into {folder}/", folder = notes_name) }).when(can_move, |d| d.on_click(cx.listener(|this, _, _, cx| this.change_first_open(|f| f.move_loose = true, cx)))).test_support())
            })
            .child(buttons(vec![
                button("first-open-cancel", tr!("Not Now"), false).on_click(cx.listener(|this, _, w, cx| this.close_sheet(w, cx))).test_support().into_any_element(),
                button("first-open-confirm", open_as(form.kind), true).on_click(cx.listener(|this, _, w, cx| this.confirm_first_open(w, cx))).test_support().into_any_element(),
            ]))
            .into_any_element();
        (title, body)
    }

    pub fn show_project_settings(&mut self, _: &ProjectSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.is_none() {
            return;
        }
        self.open_sheet(Sheet::Prefs { section: PrefSection::Project }, window, cx);
    }

    pub(crate) fn project_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(p) = &self.project else { return Vec::new() };
        let name = project::spoken(&project::folder_name(&p.tree.root));
        let s = p.tree.settings();
        let folders = project::notes_candidates(&p.tree.root, p.tree.kind);
        let in_folder = project::keeps_settings_file(&p.tree.root, &s);
        let notes = s.notes.clone();
        let kinds = div()
            .flex()
            .rounded_md()
            .border_1()
            .border_color(c(theme::line()))
            .overflow_hidden()
            .children(Kind::ALL.into_iter().enumerate().map(|(i, k)| {
                let on = k == s.kind;
                let notes = notes.clone();
                div()
                    .id(("project-type", i))
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .when(i > 0, |d| d.border_l_1().border_color(c(theme::line())))
                    .when(on, |d| d.bg(ca(theme::accent(), 0.22)).text_color(c(theme::text())))
                    .when(!on, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text_2()))))
                    .role(Role::RadioButton)
                    .aria_label(kind_title(k))
                    .aria_selected(on)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(root) = this.project_root().map(Path::to_path_buf) else { return };
                        let notes = notes.clone().or_else(|| project::notes_guess(&root, k));
                        this.apply_settings(Settings { kind: k, notes, title: None, author: None, date: None }, cx);
                    }))
                    .child(kind_title(k))
                    .test_support()
            }));
        let kind = s.kind;
        let chips = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(notes_choices(&folders, s.notes.as_deref()).into_iter().enumerate().map(|(i, n)| {
                let on = s.notes.as_deref() == Some(n.as_str());
                let shown = if folders.contains(&n) { format!("{n}/") } else { tr!("{folder}/ (new)", folder = n) };
                let pick = n.clone();
                chip(("settings-notes", i), on, shown)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_settings(Settings { kind, notes: Some(pick.clone()), title: None, author: None, date: None }, cx);
                    }))
                    .test_support()
            }))
            .child(
                chip(("settings-notes", 99), s.notes.is_none(), tr!("none"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_settings(Settings { kind, notes: None, title: None, author: None, date: None }, cx);
                    }))
                    .test_support(),
            );
        // A journal's: today's date in each form, then none (PROJ-6).
        let dates = (kind == Kind::Journal).then(|| {
            let on = project::date_heading(&p.tree.root);
            let today = project::Day::today();
            let notes = s.notes.clone();
            div().flex().flex_wrap().gap_2().children(project::DateHeading::ALL.into_iter().enumerate().map(|(i, d)| {
                let notes = notes.clone();
                chip(("settings-date", i), d == on, today.heading(d).unwrap_or_else(|| l10n::trc!("date heading", "none").into()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_settings(Settings { kind, notes: notes.clone(), title: None, author: None, date: Some(d) }, cx);
                    }))
                    .test_support()
            }))
        });
        vec![
            div().id("project-settings-name").pb_1().font_family(theme::hand()).text_size(theme::hand_px(24.)).text_color(c(theme::accent())).child(name).test_support().into_any_element(),
            row(tr!("Type"), Some(tr!("renames nothing")), kinds).into_any_element(),
        ]
        .into_iter()
        .chain((kind != Kind::Folder).then(|| row(tr!("Notes folder"), None, chips).into_any_element()))
        .chain(dates.map(|d| row(tr!("Date at the top"), Some(tr!("of each new day")), d).into_any_element()))
        .chain([
            div()
                .pt_3()
                .text_size(px(11.))
                .text_color(c(theme::faint()))
                .child(if in_folder { tr!("saved in the folder") } else { tr!("kept on this Mac") })
                .into_any_element(),
        ])
        .collect()
    }

    pub fn new_project(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let name = cx.new(|cx| {
            let mut f = TextView::new(Mode::Field, model, cx);
            f.placeholder = tr!("untitled").into();
            f
        });
        let sub = cx.subscribe_in(&name, window, |this, _, e: &ViewEvent, window, cx| match e {
            ViewEvent::Confirm => this.create_project(window, cx),
            ViewEvent::Cancel => this.close_sheet(window, cx),
            ViewEvent::Edited => cx.notify(),
            _ => {}
        });
        let place = self
            .project_root()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(crate::bookmarks::last_place)
            .or_else(|| crate::platform::home().map(|h| h.join("Documents")))
            .unwrap_or_else(|| PathBuf::from("/"));
        let focus = name.read(cx).focus.clone();
        let form = NewProjectForm { name, place, kind: Kind::Book, notes: true, parts: false, years: true, error: None, _sub: sub };
        self.open_sheet(Sheet::NewProject(Box::new(form)), window, cx);
        window.focus(&focus, cx);
    }

    fn change_new_project(&mut self, f: impl FnOnce(&mut NewProjectForm), cx: &mut Context<Self>) {
        if let Some(Sheet::NewProject(form)) = &mut self.sheet {
            form.error = None;
            f(form);
            cx.notify();
        }
    }

    #[cfg_attr(not(feature = "shots"), allow(dead_code))]
    pub(crate) fn set_new_project_place(&mut self, place: PathBuf, cx: &mut Context<Self>) {
        self.change_new_project(|f| f.place = place, cx);
    }

    fn choose_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(tr!("Choose").into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(place) = paths.into_iter().next() {
                    // Chosen in a panel, so the sandbox lets it be written: for good, with this.
                    crate::bookmarks::remember_place(&place);
                    let _ = this.update(cx, |this, cx| this.change_new_project(|f| f.place = place, cx));
                }
            }
        })
        .detach();
    }

    fn new_project_plan(form: &NewProjectForm, name: &str) -> (PathBuf, Vec<(String, &'static str)>) {
        let root = form.place.join(project::slug(if name.is_empty() { "untitled" } else { name }));
        let folder = format!("{}/", root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let mut lines = vec![(folder, "")];
        let year = project::Day::today().year;
        match form.kind {
            Kind::Book if form.parts => {
                lines.push(("  part-1/".into(), ""));
                lines.push(("    01-untitled.md".into(), tr!("the first chapter, open and ready")));
            }
            Kind::Book => lines.push(("  01-untitled.md".into(), tr!("the first chapter, open and ready"))),
            Kind::Journal if form.years => {
                lines.push((format!("  {year}/"), ""));
                lines.push((format!("    {}", project::Day::today().file_name()), tr!("today, dated on creation")));
            }
            Kind::Journal => lines.push((format!("  {}", project::Day::today().file_name()), tr!("today, dated on creation"))),
            Kind::Folder => lines.push(("  untitled.md".into(), "")),
        }
        if form.notes {
            lines.push(("  notes/".into(), ""));
        }
        if form.kind != Kind::Folder {
            lines.push(("  .dostoevsky".into(), tr!("the type, hidden in Finder")));
        }
        (root, lines)
    }

    /// Under the sandbox an unallowed place is asked for once in a panel; its bookmark keeps it.
    pub(crate) fn create_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.create_project_in_place(true, window, cx);
    }

    fn create_project_in_place(&mut self, may_ask: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::NewProject(form)) = &self.sheet else { return };
        let name = form.name.read(cx).field_text().trim().to_string();
        let (root, _) = Self::new_project_plan(form, &name);
        let place = form.place.clone();
        if name.is_empty() {
            return self.change_new_project(|f| f.error = Some(tr!("Give it a name").into()), cx);
        }
        match self.make_project(&root) {
            Err(e) if may_ask && e.kind() == std::io::ErrorKind::PermissionDenied && crate::bookmarks::sandboxed() => {
                self.change_new_project(|_| {}, cx);
                let rx = crate::platform::ask_for_folder(&place, tr!("Allow Dostoevsky to make projects in this folder. It asks only once."), tr!("Allow"), cx);
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Some(place)) = rx.recv().await {
                        crate::bookmarks::remember_place(&place);
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.change_new_project(|f| f.place = place, cx);
                            this.create_project_in_place(false, window, cx);
                        });
                    }
                })
                .detach();
            }
            Err(e) => self.change_new_project(|f| f.error = Some(e.to_string()), cx),
            Ok(()) => self.project_made(root, window, cx),
        }
    }

    fn project_made(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.close_sheet(window, cx);
        cx.defer(move |cx| crate::open_project(root, cx));
    }

    /// A permission error keeps its kind, so Create can ask for the place.
    fn make_project(&self, root: &Path) -> std::io::Result<()> {
        use std::io::{Error, ErrorKind};
        let Some(Sheet::NewProject(form)) = &self.sheet else { return Ok(()) };
        let (kind, notes, parts, years) = (form.kind, form.notes, form.parts, form.years);
        if root.exists() && std::fs::read_dir(root).map(|mut d| d.next().is_some()).unwrap_or(true) {
            let n = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(Error::other(tr!("There’s already a folder called “{name}” there", name = n)));
        }
        let s = Settings { kind, notes: notes.then(|| "notes".to_string()), title: None, author: None, date: None };
        let made = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(root)?;
            if notes {
                std::fs::create_dir_all(root.join("notes"))?;
            }
            match kind {
                Kind::Book => {
                    let dir = if parts { root.join("part-1") } else { root.to_path_buf() };
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(dir.join("01-untitled.md"), "")?;
                }
                Kind::Journal => {
                    if years {
                        std::fs::create_dir_all(root.join(project::Day::today().year.to_string()))?;
                    }
                }
                Kind::Folder => std::fs::write(root.join("untitled.md"), "")?,
            }
            if project::keeps_settings_file(root, &s) {
                project::write_settings(root, &s)?;
            } else {
                let memory = files::ProjectMemory { kind: Some(s.kind.key().into()), notes: s.notes.clone(), ..Default::default() };
                files::save_project_memory(root, &memory);
            }
            Ok(())
        })();
        made.map_err(|e| Error::new(if e.kind() == ErrorKind::PermissionDenied { e.kind() } else { ErrorKind::Other }, tr!("Couldn’t make the folder: {error}", error = e)))
    }

    pub(crate) fn render_new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let _ = window;
        let Some(Sheet::NewProject(form)) = &self.sheet else { return (Empty.into_any_element(), Empty.into_any_element()) };
        let name_text = form.name.read(cx).field_text().trim().to_string();
        let (_, plan) = Self::new_project_plan(form, &name_text);
        let (field, place, kind, notes, parts, years, error) = (form.name.clone(), tilde(&form.place), form.kind, form.notes, form.parts, form.years, form.error.clone());
        let title = div().font_family(theme::hand()).text_size(theme::hand_px(25.)).text_color(c(theme::accent())).child(tr!("New Project")).into_any_element();
        let field_row = |label: &'static str, control: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .mb_2()
                .child(div().w(px(56.)).flex_none().text_right().text_color(c(theme::muted())).child(label))
                .child(control)
        };
        let name_box = div()
            .id("new-project-name")
            .flex_1()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(c(theme::bg()))
            .border_1()
            .border_color(c(theme::accent()))
            .child(field)
            .test_support()
            .into_any_element();
        let place_box = div()
            .flex_1()
            .flex()
            .items_center()
            .gap_3()
            .child(div().flex_1().min_w_0().px_2().py_1().rounded_md().bg(c(theme::bg())).border_1().border_color(c(theme::line())).truncate().text_color(c(theme::text_2())).child(place))
            .child(
                div()
                    .id("new-project-place")
                    .cursor_pointer()
                    .text_color(c(theme::accent()))
                    .on_click(cx.listener(|this, _, w, cx| this.choose_place(w, cx)))
                    .child(tr!("Choose…"))
                    .test_support(),
            )
            .into_any_element();
        let mini = |k: Kind| -> Vec<String> {
            match k {
                // `untitled` is the file's name on disk (`01-untitled.md`), so the row says it too.
                Kind::Book => vec!["01 untitled".into(), "02 …".into(), "notes/".into()],
                Kind::Journal => vec![tr!("today…").into(), tr!("yesterday").into(), "notes/".into()],
                Kind::Folder => vec!["untitled".into(), tr!("(last changed first)").into(), String::new()],
            }
        };
        let cards = div().flex().gap_3().children(Kind::ALL.into_iter().map(|k| {
            let id = match k {
                Kind::Book => "new-book",
                Kind::Journal => "new-journal",
                Kind::Folder => "new-folder",
            };
            card(id, k, kind == k, None, mini(k))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_new_project(
                        |f| {
                            f.kind = k;
                            f.notes = k != Kind::Folder;
                        },
                        cx,
                    )
                }))
                .test_support()
        }));
        let mut options: Vec<AnyElement> = Vec::new();
        match kind {
            Kind::Book => {
                options.push(check("new-notes", notes, tr!("A notes folder, for people, places and plans")).on_click(cx.listener(|this, _, _, cx| this.change_new_project(|f| f.notes = !f.notes, cx))).test_support().into_any_element());
                options.push(check("new-parts", parts, tr!("Start with parts")).on_click(cx.listener(|this, _, _, cx| this.change_new_project(|f| f.parts = !f.parts, cx))).test_support().into_any_element());
            }
            Kind::Journal => {
                options.push(check("new-years", years, tr!("A folder for each year")).on_click(cx.listener(|this, _, _, cx| this.change_new_project(|f| f.years = !f.years, cx))).test_support().into_any_element());
                options.push(check("new-notes", notes, tr!("A notes folder")).on_click(cx.listener(|this, _, _, cx| this.change_new_project(|f| f.notes = !f.notes, cx))).test_support().into_any_element());
            }
            Kind::Folder => {
                options.push(check("new-notes", notes, tr!("A notes folder")).on_click(cx.listener(|this, _, _, cx| this.change_new_project(|f| f.notes = !f.notes, cx))).test_support().into_any_element());
            }
        }
        let tree = div()
            .p_3()
            .rounded_md()
            .bg(c(theme::bg()))
            .border_1()
            .border_color(c(theme::line()))
            .text_size(px(12.))
            .line_height(px(20.))
            .children(plan.into_iter().map(|(line, note)| {
                let hidden = line.trim_start().starts_with('.');
                div()
                    .flex()
                    .gap_4()
                    .child(div().w(px(220.)).flex_none().whitespace_nowrap().text_color(if hidden { c(theme::faint()) } else { c(theme::text_2()) }).child(line))
                    .child(div().truncate().text_color(c(theme::faint())).child(note))
            }));
        let body = div()
            .flex()
            .flex_col()
            .text_size(px(12.5))
            .child(field_row(tr!("Name"), name_box))
            .child(field_row(tr!("In"), place_box))
            .child(label(tr!("WHAT IS IT")))
            .child(cards)
            .child(div().mt_3().flex().flex_col().children(options))
            .child(label(tr!("MAKES")))
            .child(tree)
            .children(error.map(|e| div().id("new-project-error").mt_2().text_color(c(theme::danger())).child(e).test_support()))
            .child(buttons(vec![
                button("new-project-cancel", tr!("Cancel"), false).on_click(cx.listener(|this, _, w, cx| this.close_sheet(w, cx))).test_support().into_any_element(),
                button("new-project-create", tr!("Create"), true).on_click(cx.listener(|this, _, w, cx| this.create_project(w, cx))).test_support().into_any_element(),
            ]))
            .into_any_element();
        (title, body)
    }
}
