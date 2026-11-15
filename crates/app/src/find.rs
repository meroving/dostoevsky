use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use doc_model::{Document, GroupId};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::{ToggleMatchCase, ToggleWholeWords};
use crate::model::DocModel;
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The file on the page, with its drawer.
    File,
    Project,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindOptions {
    pub ghosts: bool,
    pub alternatives: bool,
    pub drawer: bool,
    pub notes: bool,
    pub case: bool,
    pub words: bool,
}

impl Default for FindOptions {
    fn default() -> Self {
        FindOptions { ghosts: true, alternatives: true, drawer: true, notes: true, case: false, words: false }
    }
}

/// Taken from a document on the UI thread, so the matching can run anywhere.
#[derive(Clone, Debug, Default)]
pub struct Searchable {
    pub page: String,
    pub ghosts: Vec<Range<usize>>,
    /// (group, version, where the group starts, text) for versions not on the page.
    pub hidden: Vec<(GroupId, usize, usize, String)>,
    pub scraps: Vec<String>,
}

impl Searchable {
    /// With `scraps`, its own drawer too (a single file's drawer lives in it).
    pub fn of(doc: &Document, scraps: bool) -> Searchable {
        let mut hidden = Vec::new();
        for id in doc.all_group_ids() {
            // A group inside a hidden version is found through that version's text.
            let (Some(at), Some(g)) = (doc.group_range(id), doc.group(id)) else { continue };
            hidden.extend(g.variants.iter().enumerate().filter(|(i, _)| *i != g.active).map(|(i, v)| (id, i, at.start, v.text.clone())));
        }
        hidden.sort_by_key(|h| h.2);
        Searchable {
            page: doc.text(),
            ghosts: doc.ghost_ranges(),
            hidden,
            scraps: if scraps { doc.notes().into_iter().map(|n| n.text).collect() } else { Vec::new() },
        }
    }

    pub fn scraps_of(doc: &Document) -> Searchable {
        Searchable { scraps: doc.notes().into_iter().map(|n| n.text).collect(), ..Default::default() }
    }

    pub fn text_of(&self, spot: &Spot) -> &str {
        match spot {
            Spot::Page(_) => &self.page,
            Spot::Hidden { group, variant, .. } => self.hidden.iter().find(|h| h.0 == *group && h.1 == *variant).map_or("", |h| h.3.as_str()),
            Spot::Scrap { note, .. } => self.scraps.get(*note).map_or("", String::as_str),
        }
    }

    pub fn in_ghost(&self, r: &Range<usize>) -> bool {
        self.ghosts.iter().any(|g| g.start < r.end && r.start < g.end)
    }
}

/// Where a match is. Ranges are chars of the text it's in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    Page(Range<usize>),
    Hidden { group: GroupId, variant: usize, range: Range<usize> },
    Scrap { note: usize, range: Range<usize> },
}

/// In reading order: the page, each hidden version where its group sits, then the scraps.
pub fn spots(s: &Searchable, q: &str, o: &FindOptions) -> Vec<Spot> {
    let mut keyed: Vec<(usize, Spot)> = find_matches(&s.page, q, o.case, o.words)
        .into_iter()
        .filter(|r| o.ghosts || !s.in_ghost(r))
        .map(|r| (r.start, Spot::Page(r)))
        .collect();
    if o.alternatives {
        for (group, variant, at, text) in &s.hidden {
            keyed.extend(find_matches(text, q, o.case, o.words).into_iter().map(|range| (*at, Spot::Hidden { group: *group, variant: *variant, range })));
        }
    }
    // Stable: page matches before a hidden version starting at the same place.
    keyed.sort_by_key(|(k, _)| *k);
    let mut out: Vec<Spot> = keyed.into_iter().map(|(_, s)| s).collect();
    if o.drawer {
        for (note, text) in s.scraps.iter().enumerate() {
            out.extend(find_matches(text, q, o.case, o.words).into_iter().map(|range| Spot::Scrap { note, range }));
        }
    }
    out
}

/// Char ranges; a whole word has no letter or digit on either side.
pub fn find_matches(text: &str, q: &str, case: bool, words: bool) -> Vec<Range<usize>> {
    if q.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let (hay, needle): (Vec<char>, Vec<char>) = if case {
        (chars.clone(), q.chars().collect())
    } else {
        let hay: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
        // Lowercasing can change lengths (rare, e.g. 'İ'); fall back to exact matching then.
        if hay.len() == chars.len() { (hay, q.chars().flat_map(char::to_lowercase).collect()) } else { (chars.clone(), q.chars().collect()) }
    };
    let boundary = |i: usize| i == 0 || i >= chars.len() || !chars[i - 1].is_alphanumeric() || !chars[i].is_alphanumeric();
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] && (!words || (boundary(i) && boundary(i + needle.len()))) {
            out.push(i..i + needle.len());
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    File(PathBuf),
    Drawer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotKind {
    Page,
    Ghost,
    Alternative,
    Scrap,
}

/// A few words around a match. `hit` is in bytes of `text`.
#[derive(Clone, Debug)]
pub struct Snippet {
    pub kind: SpotKind,
    pub text: String,
    pub hit: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct FileHits {
    pub place: Place,
    pub no: String,
    pub label: String,
    /// The part (or month) it sits under, for the list's headings.
    pub section: String,
    pub count: usize,
    pub snippets: Vec<Snippet>,
}

pub enum Source {
    Live(Searchable),
    Disk(PathBuf),
}

pub struct Job {
    pub place: Place,
    pub no: String,
    pub label: String,
    pub section: String,
    pub source: Source,
}

/// Matches listed per place; the rest are only counted.
pub const LISTED: usize = 30;
const LEAD: usize = 24;
const TAIL: usize = 50;

pub fn snippet(s: &Searchable, spot: &Spot) -> Snippet {
    let (text, hit) = match spot {
        Spot::Page(r) => (s.page.as_str(), r.clone()),
        Spot::Hidden { range, .. } | Spot::Scrap { range, .. } => (s.text_of(spot), range.clone()),
    };
    let kind = match spot {
        Spot::Page(r) if s.in_ghost(r) => SpotKind::Ghost,
        Spot::Page(_) => SpotKind::Page,
        Spot::Hidden { .. } => SpotKind::Alternative,
        Spot::Scrap { .. } => SpotKind::Scrap,
    };
    let chars: Vec<char> = text.chars().collect();
    let hit = hit.start.min(chars.len())..hit.end.min(chars.len());
    let start = hit.start.saturating_sub(LEAD);
    let before: String = chars[start..hit.start].iter().collect();
    let found: String = chars[hit.clone()].iter().collect();
    let after: String = chars[hit.end..(hit.end + TAIL).min(chars.len())].iter().collect();
    let before = before.rsplit('\n').next().unwrap_or_default().to_string();
    let after = after.split('\n').next().unwrap_or_default().to_string();
    let lead = if start > 0 && before.chars().count() == LEAD { "…" } else { "" };
    let at = lead.len() + before.len();
    Snippet { kind, text: format!("{lead}{before}{found}{after}"), hit: at..at + found.len() }
}

/// Reads from disk what isn't live; run off the UI thread.
pub fn search_jobs(jobs: Vec<Job>, q: &str, o: &FindOptions) -> Vec<FileHits> {
    jobs.into_iter()
        .filter_map(|j| {
            let s = match j.source {
                Source::Live(s) => s,
                Source::Disk(path) => {
                    let text = std::fs::read_to_string(&path).ok()?;
                    Searchable::of(&crate::format::load(&text).doc, false)
                }
            };
            let found = spots(&s, q, o);
            (!found.is_empty()).then(|| FileHits {
                place: j.place,
                no: j.no,
                label: j.label,
                section: j.section,
                count: found.len(),
                snippets: found.iter().take(LISTED).map(|sp| snippet(&s, sp)).collect(),
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct Names {
    pub this: &'static str,
    pub whole: &'static str,
    pub one: &'static str,
    pub many: &'static str,
}

impl Names {
    pub fn single() -> Names {
        Names { this: "this file", whole: "", one: "file", many: "files" }
    }

    /// In the UI's language; the fields are English, as keys.
    fn this_label(&self) -> &'static str {
        match self.this {
            "this file" => tr!("this file"),
            "this chapter" => tr!("this chapter"),
            "this day" => tr!("this day"),
            other => other,
        }
    }

    fn whole_label(&self) -> &'static str {
        match self.whole {
            "whole book" => tr!("whole book"),
            "whole journal" => tr!("whole journal"),
            "whole folder" => tr!("whole folder"),
            other => other,
        }
    }

    fn count_files(&self, n: usize) -> String {
        match self.many {
            "chapters" => trn!("{n} chapter", "{n} chapters", n),
            "entries" => trn!("{n} entry", "{n} entries", n),
            "files" => trn!("{n} file", "{n} files", n),
            many => format!("{n} {}", if n == 1 { self.one } else { many }),
        }
    }

    fn in_whole(&self, n: usize) -> String {
        match self.whole {
            "whole book" => trn!("{n} in the book ›", "{n} in the book ›", n),
            "whole journal" => trn!("{n} in the journal ›", "{n} in the journal ›", n),
            "whole folder" => trn!("{n} in the folder ›", "{n} in the folder ›", n),
            whole => format!("{n} in the {} ›", whole.trim_start_matches("whole ").trim()),
        }
    }
}

pub struct FindBar {
    pub query: Entity<TextView>,
    pub replacement: Entity<TextView>,
    pub show_replace: bool,
    pub scope: Scope,
    pub opts: FindOptions,
    pub in_project: bool,
    pub names: Names,
    /// The file on the page, or (whole book) the project's drawer.
    pub place: Place,
    pub place_label: String,
    /// In order; in File scope the drawer's follow the file's.
    pub spots: Vec<Spot>,
    pub current: Option<usize>,
    pub matches: Vec<Range<usize>>,
    pub hidden_hits: usize,
    /// Page matches that *match case* / *whole words* leave out.
    pub skipped: usize,
    /// `order` is every place in reading order, so ⌘G knows what comes next.
    pub files: Vec<FileHits>,
    pub order: Vec<Place>,
    pub searching: bool,
    pub list_open: bool,
    pub run: u64,
    list_scroll: ScrollHandle,
    reveal: bool,
}

pub enum FindEvent {
    Updated,
    Next,
    Previous,
    Close,
    Replace,
    ReplaceAll,
    Scope(Scope),
    Fold,
    GoTo(Place, usize),
}

impl EventEmitter<FindEvent> for FindBar {}

impl FindBar {
    pub fn new(model: Entity<DocModel>, show_replace: bool, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            let mut v = TextView::new(Mode::Field, model.clone(), cx);
            v.placeholder = tr!("find").into();
            v
        });
        let replacement = cx.new(|cx| {
            let mut v = TextView::new(Mode::Field, model, cx);
            v.placeholder = tr!("replace with").into();
            v
        });
        cx.subscribe(&query, |this: &mut FindBar, _, e: &ViewEvent, cx| match e {
            ViewEvent::Edited => cx.emit(FindEvent::Updated),
            ViewEvent::Confirm if this.list_open => cx.emit(FindEvent::Fold),
            ViewEvent::Confirm => cx.emit(FindEvent::Next),
            ViewEvent::Arrow(d) if this.list_open => cx.emit(if *d < 0 { FindEvent::Previous } else { FindEvent::Next }),
            ViewEvent::Cancel if this.list_open => cx.emit(FindEvent::Fold),
            ViewEvent::Cancel => cx.emit(FindEvent::Close),
            _ => {}
        })
        .detach();
        cx.subscribe(&replacement, |_, _, e: &ViewEvent, cx| match e {
            ViewEvent::Confirm => cx.emit(FindEvent::Replace),
            ViewEvent::Cancel => cx.emit(FindEvent::Close),
            _ => {}
        })
        .detach();
        FindBar {
            query,
            replacement,
            show_replace,
            scope: Scope::File,
            opts: FindOptions::default(),
            in_project: false,
            names: Names::single(),
            place: Place::Drawer,
            place_label: String::new(),
            spots: Vec::new(),
            current: None,
            matches: Vec::new(),
            hidden_hits: 0,
            skipped: 0,
            files: Vec::new(),
            order: Vec::new(),
            searching: false,
            list_open: false,
            run: 0,
            list_scroll: ScrollHandle::new(),
            reveal: false,
        }
    }

    pub fn query_text(&self, cx: &App) -> String {
        self.query.read(cx).field_text().to_string()
    }

    /// `current` becomes the first at or after `from` on the page.
    pub fn set_spots(&mut self, place: Place, spots: Vec<Spot>, from: Option<usize>) {
        self.place = place;
        self.matches = spots.iter().filter_map(|s| if let Spot::Page(r) = s { Some(r.clone()) } else { None }).collect();
        self.hidden_hits = spots.iter().filter(|s| !matches!(s, Spot::Page(_))).count();
        self.current = match from {
            Some(from) => spots.iter().position(|s| matches!(s, Spot::Page(r) if r.start >= from)).or((!spots.is_empty()).then_some(0)),
            None => self.current.filter(|i| *i < spots.len()).or((!spots.is_empty()).then_some(0)),
        };
        self.spots = spots;
        self.reveal = true;
    }

    pub fn reveal_current(&mut self) {
        self.reveal = true;
    }

    pub fn current_spot(&self) -> Option<&Spot> {
        self.current.and_then(|i| self.spots.get(i))
    }

    pub fn current_page_match(&self) -> Option<usize> {
        let i = self.current?;
        matches!(self.spots.get(i), Some(Spot::Page(_))).then(|| self.spots[..i].iter().filter(|s| matches!(s, Spot::Page(_))).count())
    }

    pub fn highlights(&self) -> Rc<Vec<Range<usize>>> {
        Rc::new(self.matches.clone())
    }

    /// With the live count for `place`: the page is ahead of the background search.
    pub fn counts(&self) -> Vec<(Place, usize)> {
        let mut out: Vec<(Place, usize)> = self
            .order
            .iter()
            .filter_map(|p| {
                let n = if *p == self.place { self.spots.len() } else { self.files.iter().find(|f| f.place == *p).map_or(0, |f| f.count) };
                (n > 0).then(|| (p.clone(), n))
            })
            .collect();
        if !self.order.contains(&self.place) && !self.spots.is_empty() {
            out.insert(0, (self.place.clone(), self.spots.len()));
        }
        out
    }

    pub fn project_total(&self) -> usize {
        self.counts().iter().map(|(_, n)| n).sum()
    }

    fn count_label(&self, cx: &App) -> String {
        if self.query_text(cx).is_empty() {
            return String::new();
        }
        let (i, n) = match self.scope {
            Scope::File => (self.current, self.spots.len()),
            Scope::Project => {
                let counts = self.counts();
                let before: usize = counts.iter().take_while(|(p, _)| *p != self.place).map(|(_, n)| n).sum();
                (self.current.map(|c| before + c), counts.iter().map(|(_, n)| n).sum())
            }
        };
        match (i, n) {
            (_, 0) if self.searching && self.scope == Scope::Project => "…".into(),
            (_, 0) => tr!("nothing").into(),
            (Some(i), n) => format!("{} / {}", i + 1, n),
            (None, n) => n.to_string(),
        }
    }

    fn project_summary(&self) -> String {
        let total = self.project_total();
        if total == 0 {
            return if self.searching { tr!("searching…").into() } else { tr!("nothing found").into() };
        }
        let counts = self.counts();
        let notes = counts.iter().filter(|(p, _)| self.files.iter().any(|f| f.place == *p && f.section == NOTES)).count();
        let drawer = counts.iter().any(|(p, _)| *p == Place::Drawer);
        let files = counts.len() - notes - usize::from(drawer);
        let mut parts = Vec::new();
        if files > 0 {
            parts.push(self.names.count_files(files));
        }
        if drawer {
            parts.push(tr!("the project’s cuts").to_string());
        }
        if notes > 0 {
            parts.push(trn!("{n} note", "{n} notes", notes));
        }
        let list = match parts.as_slice() {
            [a, b] => tr!("{a} and {b}", a = a, b = b),
            [a, b, c] => tr!("{a}, {b} and {c}", a = a, b = b, c = c),
            _ => parts.join(", "),
        };
        trn!("{n} in {list}", "{n} in {list}", total, list = list)
    }
}

/// The section name that marks a project's notes in [`FileHits::section`].
pub const NOTES: &str = "notes";

fn toggle(id: &'static str, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex_none()
        .cursor_pointer()
        .text_color(if on { c(theme::accent()) } else { c(theme::muted()) })
        .hover(|s| s.text_color(if on { c(theme::accent()) } else { c(theme::text_2()) }))
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(crate::workspace::toggled(on))
        .when(on, |d| d.child(div().absolute().left_0().right_0().bottom(px(-1.)).h(px(1.5)).rounded(px(1.)).bg(c(theme::accent()))))
        .child(label)
}

impl Render for FindBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.count_label(cx);
        let has_query = !self.query_text(cx).is_empty();
        // The fields give way when the window is narrow (down to 120), the buttons don't.
        let field = |v: &Entity<TextView>| {
            div()
                .flex_1()
                .min_w(px(120.))
                .px_2()
                .py_1()
                .rounded_md()
                .bg(c(theme::bg()))
                .border_1()
                .border_color(c(theme::line()))
                .child(v.clone())
        };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .flex_none()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .text_color(c(theme::muted()))
                .hover(|s| s.text_color(c(theme::text())).bg(c(theme::line())))
                .child(label)
        };
        let opts = self.opts;
        let set = |f: fn(&mut FindOptions)| {
            cx.listener(move |this: &mut FindBar, _: &ClickEvent, _, cx| {
                f(&mut this.opts);
                cx.emit(FindEvent::Updated);
                cx.notify();
            })
        };
        let matching = div()
            .flex()
            .flex_none()
            .gap(px(9.))
            .text_size(px(11.))
            .child(toggle("opt-case", "Aa", opts.case).aria_label(tr!("Match case")).on_click(set(|o| o.case = !o.case)).test_support())
            .child(toggle("opt-words", tr!("word"), opts.words).aria_label(tr!("Whole words")).on_click(set(|o| o.words = !o.words)).test_support());
        let query_row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(120.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(c(theme::bg()))
                    .border_1()
                    .border_color(c(theme::line()))
                    .child(div().flex_1().min_w_0().child(self.query.clone()))
                    .child(matching),
            )
            .child(div().id("find-count").w(px(64.)).flex_none().whitespace_nowrap().text_color(c(theme::muted())).aria_label(count.clone()).child(count).test_support())
            .child(button("prev", "↑").on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Previous))).test_support())
            .child(button("next", "↓").on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Next))).test_support())
            .child(button("close", "esc").on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Close))).test_support());

        let scope = self.scope;
        let looks = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(14.))
            .gap_y(px(4.))
            .px(px(2.))
            .text_size(px(11.5))
            .when(self.in_project, |d| {
                d.child(toggle("scope-file", self.names.this_label(), scope == Scope::File).on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Scope(Scope::File)))).test_support())
                    .child(toggle("scope-project", self.names.whole_label(), scope == Scope::Project).on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Scope(Scope::Project)))).test_support())
                    .child(div().flex_none().w(px(1.)).h(px(14.)).bg(c(theme::line())))
            })
            // Cuts on the page and in the panel are one toggle.
            .child(toggle("opt-cuts", tr!("cuts"), opts.ghosts || opts.drawer).on_click(set(|o| {
                let on = !(o.ghosts || o.drawer);
                o.ghosts = on;
                o.drawer = on;
            })).test_support())
            .child(toggle("opt-alternatives", tr!("alternatives"), opts.alternatives).on_click(set(|o| o.alternatives = !o.alternatives)).test_support())
            .when(self.in_project && scope == Scope::Project, |d| d.child(toggle("opt-notes", tr!("notes"), opts.notes).on_click(set(|o| o.notes = !o.notes)).test_support()));

        let summary = has_query.then(|| {
            let whole = self.in_project.then(|| self.project_total()).filter(|n| *n > 0);
            let (left, right): (String, Option<String>) = match scope {
                Scope::File => {
                    let n = self.spots.len();
                    let place = &self.place_label;
                    let mut left = if n == 0 { tr!("nothing in {place}", place = place) } else { trn!("{n} in {place}", "{n} in {place}", n, place = place) };
                    if self.hidden_hits > 0 {
                        left.push_str(" · ");
                        left.push_str(&trn!("{n} out of sight", "{n} out of sight", self.hidden_hits));
                    }
                    if self.skipped > 0 {
                        let k = self.skipped;
                        let more = match (opts.case, opts.words) {
                            (true, false) => trn!("{n} more in another case", "{n} more in another case", k),
                            (false, true) => trn!("{n} more inside other words", "{n} more inside other words", k),
                            _ => trn!("{n} more without Aa or word", "{n} more without Aa or word", k),
                        };
                        left.push_str(" · ");
                        left.push_str(&more);
                    }
                    (left, whole.filter(|w| *w > n).map(|w| self.names.in_whole(w)))
                }
                Scope::Project => (self.project_summary(), None),
            };
            let fold = scope == Scope::Project && self.project_total() > 0;
            let open = self.list_open;
            div()
                .id("find-summary")
                .flex()
                .items_center()
                .gap_2()
                .mx(px(-8.))
                .px(px(10.))
                .pt(px(7.))
                .border_t_1()
                .border_color(c(theme::line()))
                .text_size(px(11.5))
                .text_color(c(theme::muted()))
                .whitespace_nowrap()
                .aria_label(left.clone())
                .when(fold, |d| {
                    d.cursor_pointer().on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Fold))).child(
                        svg()
                            .path("chrome/chevron.svg")
                            .size(px(8.))
                            .flex_none()
                            .text_color(c(theme::faint()))
                            .when(open, |s| s.with_transformation(Transformation::rotate(radians(std::f32::consts::FRAC_PI_2)))),
                    )
                })
                .child(div().flex_1().min_w_0().truncate().child(left))
                .children(right.map(|r| {
                    div()
                        .id("find-widen")
                        .flex_none()
                        .cursor_pointer()
                        .text_color(c(theme::accent()))
                        .hover(|s| s.text_color(c(theme::text())))
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(FindEvent::Scope(Scope::Project));
                        }))
                        .child(r)
                        .test_support()
                }))
                .test_support()
        });

        let list = (scope == Scope::Project && self.list_open && self.project_total() > 0).then(|| self.render_list(cx));
        let reveal_row = std::mem::take(&mut self.reveal);

        div()
            .id("find-bar")
            .key_context("FindBar")
            .on_action(cx.listener(|this, _: &ToggleMatchCase, _, cx| {
                this.opts.case = !this.opts.case;
                cx.emit(FindEvent::Updated);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleWholeWords, _, cx| {
                this.opts.words = !this.opts.words;
                cx.emit(FindEvent::Updated);
                cx.notify();
            }))
            // Clicks on the bar are the bar's: the page below must not move its caret or take focus.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p_2()
            .rounded_lg()
            .bg(c(theme::bg_raised()))
            .border_1()
            .border_color(c(theme::line()))
            .shadow_lg()
            .text_size(px(12.))
            .text_color(c(theme::text_2()))
            .child(query_row)
            .when(self.show_replace, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(field(&self.replacement))
                        .child(button("replace", tr!("Replace")).on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Replace))).test_support())
                        .child(button("replace-all", tr!("All")).on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::ReplaceAll))).test_support()),
                )
            })
            .child(looks)
            .children(summary)
            .children(list.map(|(l, at)| {
                if reveal_row {
                    if let Some(at) = at {
                        self.list_scroll.scroll_to_item(at);
                    }
                }
                l
            }))
            .test_support()
    }
}

impl FindBar {
    fn render_list(&mut self, cx: &mut Context<Self>) -> (AnyElement, Option<usize>) {
        let text = c(theme::text());
        let tag = c(theme::original());
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut at = None;
        let mut section: Option<String> = None;
        for f in &self.files {
            if section.as_deref() != Some(f.section.as_str()) && !(f.section.is_empty() && section.is_none()) {
                section = Some(f.section.clone());
                if !f.section.is_empty() {
                    let heading = if f.section == NOTES { tr!("notes").to_string() } else { f.section.clone() };
                    rows.push(div().mt(px(6.)).px(px(4.)).text_size(px(10.5)).text_color(c(theme::faint())).child(heading).into_any_element());
                }
            }
            let here = f.place == self.place;
            let count = if here { self.spots.len() } else { f.count };
            if f.place != Place::Drawer {
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(4.))
                        .py(px(1.))
                        .text_size(px(12.))
                        .text_color(if here { c(theme::text()) } else { c(theme::text_2()) })
                        .child(div().flex_none().min_w(px(16.)).text_right().text_size(px(11.)).text_color(if here { c(theme::accent()) } else { c(theme::faint()) }).child(f.no.clone()))
                        .child(div().flex_1().min_w_0().truncate().child(f.label.clone()))
                        .child(div().flex_none().text_size(px(10.5)).text_color(c(theme::faint())).child(count.to_string()))
                        .into_any_element(),
                );
            }
            for (j, s) in f.snippets.iter().enumerate() {
                let on = here && self.current == Some(j);
                if on {
                    at = Some(rows.len());
                }
                let tag_word = match s.kind {
                    SpotKind::Page => "",
                    SpotKind::Ghost => tr!("cut"),
                    SpotKind::Alternative => tr!("alternative"),
                    SpotKind::Scrap => tr!("cut"),
                };
                let label = if tag_word.is_empty() { String::new() } else { format!("{tag_word} ") };
                let line = format!("{label}{}", s.text);
                let hit = label.len() + s.hit.start..label.len() + s.hit.end;
                let mut marks = vec![(hit, HighlightStyle { color: Some(text), ..Default::default() })];
                if !label.is_empty() {
                    marks.insert(0, (0..label.len() - 1, HighlightStyle { color: Some(tag), ..Default::default() }));
                }
                let place = f.place.clone();
                rows.push(
                    div()
                        .id(SharedString::from(format!("hit-{}-{j}", f.label)))
                        .pl(px(28.))
                        .pr(px(4.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .text_color(c(theme::muted()))
                        .truncate()
                        .when(on, |d| d.bg(ca(theme::line(), 0.9)).text_color(c(theme::text_2())))
                        .hover(|d| d.bg(ca(theme::line(), 0.6)))
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(FindEvent::GoTo(place.clone(), j))))
                        .child(StyledText::new(line).with_highlights(marks))
                        .test_support()
                        .into_any_element(),
                );
            }
            if f.count > f.snippets.len() {
                rows.push(div().pl(px(28.)).text_size(px(11.)).text_color(c(theme::faint())).child(trn!("{n} more", "{n} more", f.count - f.snippets.len())).into_any_element());
            }
        }
        let el = div()
            .id("find-list")
            .flex()
            .flex_col()
            .max_h(px(440.))
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .children(rows)
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .mx(px(-8.))
                    .mt(px(6.))
                    .px(px(10.))
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(c(theme::line()))
                    .text_size(px(10.5))
                    .text_color(c(theme::faint()))
                    .child(tr!("↑ ↓ go to"))
                    .child(tr!("↩ fold"))
                    .child(tr!("esc fold, then close")),
            )
            .test_support()
            .into_any_element();
        (el, at)
    }
}

#[cfg(test)]
mod tests {
    use super::{find_matches, spots, FindOptions, Searchable, Spot};

    fn find_all(text: &str, q: &str) -> Vec<std::ops::Range<usize>> {
        find_matches(text, q, false, false)
    }
    use doc_model::GroupId;

    #[test]
    fn finds_case_insensitively() {
        assert_eq!(find_all("Friction, friction", "FRICTION"), vec![0..8, 10..18]);
        assert_eq!(find_all("aaa", "aa"), vec![0..2]);
        assert!(find_all("x", "").is_empty());
    }

    #[test]
    fn matches_case_and_whole_words() {
        assert_eq!(find_matches("Her there, her", "her", true, false), vec![5..8, 11..14]);
        assert_eq!(find_matches("Her there, her", "her", false, true), vec![0..3, 11..14]);
        assert_eq!(find_matches("the landlady’s door", "landlady", false, true), vec![4..12], "an apostrophe ends a word");
        assert_eq!(find_matches("Hermione", "her", false, true), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn spots_come_in_reading_order() {
        let s = Searchable {
            page: "one two one".into(),
            ghosts: std::iter::once(8..11).collect(),
            hidden: vec![(GroupId(1), 1, 4, "one more".into())],
            scraps: vec!["and one".into()],
        };
        let all = spots(&s, "one", &FindOptions::default());
        assert_eq!(
            all,
            vec![
                Spot::Page(0..3),
                Spot::Hidden { group: GroupId(1), variant: 1, range: 0..3 },
                Spot::Page(8..11),
                Spot::Scrap { note: 0, range: 4..7 }
            ]
        );
        let o = FindOptions { ghosts: false, alternatives: false, drawer: false, ..Default::default() };
        assert_eq!(spots(&s, "one", &o), vec![Spot::Page(0..3)]);
    }
}
