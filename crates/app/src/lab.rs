use std::collections::HashSet;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ai::cost::Slot;
use ai::lab::Pass;
use ai::trim::LEVELS;
use ai::{AiError, ServiceKind};
use doc_model::GroupId;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::theme::{self, c};
use crate::view::{LabPaint, PaintMark};
use crate::workspace::Workspace;

/// Skips sentences already in `long`.
pub fn passive_sentences(text: &str, sentences: &[ai::segment::Sentence], long: &[ai::lab::Mark]) -> Vec<ai::lab::Mark> {
    let (Some(first), Some(last)) = (sentences.first(), sentences.last()) else { return Vec::new() };
    let span = first.range.start..last.range.end;
    let body: String = text.chars().skip(span.start).take(span.len()).collect();
    let Some(lang) = prose::detect(&body).or_else(|| prose::detect(text)).filter(|l| prose::reads_passive(*l)) else { return Vec::new() };
    let tokens = crate::tagger::tokens(&body, lang);
    sentences
        .iter()
        .filter(|s| !long.iter().any(|m| m.range == s.range))
        .filter_map(|s| {
            let within = (s.range.start - span.start)..(s.range.end - span.start);
            let words: Vec<prose::Token> = tokens.iter().filter(|t| t.range.start >= within.start && t.range.end <= within.end).cloned().collect();
            prose::is_passive(&words, lang).then(|| ai::lab::Mark {
                range: s.range.clone(),
                note: tr!("Passive: who does it comes last, or not at all.").into(),
                fix: None,
                found: ai::lab::Found::Passive,
            })
        })
        .collect()
}

pub fn echo_marks(text: &str, sentences: &[ai::segment::Sentence], title: Option<&str>) -> Vec<ai::lab::Mark> {
    let (Some(first), Some(last)) = (sentences.first(), sentences.last()) else { return Vec::new() };
    let span = first.range.start..last.range.end;
    let body: String = text.chars().skip(span.start).take(span.len()).collect();
    let Some(lang) = prose::detect(&body).or_else(|| prose::detect(text)) else { return Vec::new() };
    let read = |s: &str| {
        let t = crate::tagger::tokens(s, lang);
        if t.is_empty() { prose::plain_tokens(s) } else { t }
    };
    let mut skip: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(title) = title {
        for t in read(title) {
            skip.insert(t.word.to_lowercase());
            skip.insert(t.base());
        }
    }
    let tokens: Vec<prose::Token> = read(&body).into_iter().filter(|t| sentences.iter().any(|s| s.range.start <= t.range.start + span.start && t.range.end + span.start <= s.range.end)).collect();
    prose::echoes(&tokens, lang, &skip)
        .into_iter()
        .map(|e| {
            let n = e.between + 1;
            ai::lab::Mark {
                range: (e.again.start + span.start)..(e.again.end + span.start),
                note: trn!("“{word}” again, {n} word after “{first}”.", "“{word}” again, {n} words after “{first}”.", n, word = e.word, first = e.first_word),
                fix: None,
                found: ai::lab::Found::Echo,
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct Mark {
    pub range: Range<usize>,
    pub quote: String,
    pub note: String,
    pub found: ai::lab::Found,
}

pub struct Marks {
    pub pass: Pass,
    pub items: Vec<Mark>,
    pub current: usize,
}

pub struct Made {
    pub pass: Pass,
    pub groups: Vec<GroupId>,
    /// The first `fixed` of `groups` are fixes; the rest are versions.
    pub fixed: usize,
    pub notes: Vec<(GroupId, String)>,
    pub current: usize,
}

impl Made {
    pub(crate) fn note_for(&self, g: GroupId) -> Option<&str> {
        self.notes.iter().find(|(id, _)| *id == g).map(|(_, n)| n.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct Cut {
    pub range: Range<usize>,
    pub quote: String,
    pub words: usize,
}

pub struct Trim {
    pub cuts: Vec<Cut>,
    pub total: usize,
    /// Percent of `total` to cut, 0 to `ai::trim::MAX`.
    pub percent: usize,
    pub kept: HashSet<usize>,
    /// Cuts whose words were rewritten: they no longer show anywhere.
    pub gone: HashSet<usize>,
    /// The list's current row, an index into `list()`.
    pub at: usize,
}

impl Trim {
    pub fn new(cuts: Vec<Cut>, total: usize, percent: usize) -> Self {
        Trim { cuts, total, percent, kept: HashSet::new(), gone: HashSet::new(), at: 0 }
    }

    /// Indices into `cuts`, in rank order.
    pub fn showing(&self) -> Vec<usize> {
        let pc = self.percent;
        let ranked: Vec<ai::trim::Cut> = self.cuts.iter().map(|c| ai::trim::Cut { range: c.range.clone(), words: c.words }).collect();
        let skip: HashSet<usize> = self.kept.union(&self.gone).copied().collect();
        ai::trim::pick(&ranked, self.total, pc, &skip)
    }

    /// What the list shows, in page order: the cuts that would go and the ones kept.
    pub fn list(&self) -> Vec<usize> {
        let mut l = self.showing();
        l.extend(self.kept.iter().copied().filter(|i| !self.gone.contains(i)));
        l.sort_by_key(|&i| (self.cuts[i].range.start, i));
        l.dedup();
        l
    }

    pub fn words_cut(&self) -> usize {
        self.showing().iter().map(|&i| self.cuts[i].words).sum()
    }

    /// Some cuts kept: not the slider's.
    pub fn yours(&self) -> bool {
        !self.kept.is_empty()
    }

    /// The cut on the list's current row.
    pub fn current(&self) -> Option<usize> {
        let l = self.list();
        l.get(self.at.min(l.len().saturating_sub(1))).copied()
    }
}

/// What became of a result that was dealt with in the Review tab; it stays in the list, dimmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Settled on the fix or the version that showed.
    Settled,
    /// Settled on the original.
    Mine,
    /// Left in the text with its alternatives (mockup review-settle).
    Kept,
    Dismissed,
    Comment,
}

#[derive(Clone, Debug)]
pub struct Decided {
    /// Where it was, for its place in the list.
    pub at: usize,
    pub label: String,
    /// A fix: what it was and what the fix put there, drawn as a word diff.
    pub diff: Option<(String, String)>,
    /// In the list of marks (else of changes).
    pub mark: bool,
    pub outcome: Outcome,
}

/// A pass's results while another pass's are in front.
#[derive(Default)]
pub struct Parked {
    pub marks: Option<Marks>,
    pub made: Option<Made>,
    pub trim: Option<Trim>,
    pub on_marks: bool,
    pub decided: Vec<Decided>,
}

impl Parked {
    pub fn open(&self) -> usize {
        open_count(self.marks.as_ref(), self.made.as_ref(), self.trim.as_ref())
    }
}

fn open_count(marks: Option<&Marks>, made: Option<&Made>, trim: Option<&Trim>) -> usize {
    marks.map_or(0, |m| m.items.len()) + made.map_or(0, |t| t.groups.len()) + trim.map_or(0, |t| t.showing().len())
}

pub struct Running {
    pub label: String,
    pub model: String,
    /// Hard to read: the long sentences already show; the model's join them.
    pub beside: bool,
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
}

pub struct Lab {
    /// The Review tab is showing in the left panel.
    pub open: bool,
    /// The pass row chosen in the tab (as in `MENU_ROWS`).
    pub at: usize,
    /// The tab shows the results of `active` rather than the passes.
    pub results: bool,
    /// The pass row whose results are in `marks`, `made`, `trim`, `error` and `running`.
    pub active: Option<usize>,
    /// The other passes' results.
    pub parked: Vec<(usize, Parked)>,
    pub decided: Vec<Decided>,
    pub trim_pc: usize,
    pub on_marks: bool,
    pub running: Option<Running>,
    pub marks: Option<Marks>,
    pub made: Option<Made>,
    pub trim: Option<Trim>,
    pub error: Option<(String, bool)>,
    pub focus: FocusHandle,
    pub scroll: ScrollHandle,
    /// The list scrolls to the current row on its next draw.
    pub reveal: bool,
}

impl Lab {
    pub fn new(cx: &mut App) -> Self {
        Lab {
            open: false,
            at: 0,
            results: false,
            active: None,
            parked: Vec::new(),
            decided: Vec::new(),
            trim_pc: LEVELS[1],
            on_marks: false,
            running: None,
            marks: None,
            made: None,
            trim: None,
            error: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            reveal: false,
        }
    }

    /// The pass in front has something to show.
    pub fn waiting(&self) -> bool {
        self.running.is_some() || self.marks.is_some() || self.made.is_some() || self.trim.is_some() || self.error.is_some()
    }

    /// Results still to deal with, in the pass in front.
    pub fn open_here(&self) -> usize {
        open_count(self.marks.as_ref(), self.made.as_ref(), self.trim.as_ref())
    }

    /// Results still to deal with, a pass row's.
    pub fn open_in(&self, row: usize) -> usize {
        if self.active == Some(row) {
            return self.open_here();
        }
        self.parked.iter().find(|(r, _)| *r == row).map_or(0, |(_, p)| p.open())
    }

    /// Results still to deal with, all passes'.
    pub fn open_total(&self) -> usize {
        self.open_here() + self.parked.iter().map(|(_, p)| p.open()).sum::<usize>()
    }

    /// The results layer has something to show.
    pub fn showing_results(&self) -> bool {
        self.results && self.active.is_some() && (self.waiting() || !self.decided.is_empty())
    }

    /// Puts the results in front away, under their row.
    fn park(&mut self) {
        let Some(row) = self.active.take() else { return };
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.error = None;
        let p = Parked { marks: self.marks.take(), made: self.made.take(), trim: self.trim.take(), on_marks: std::mem::take(&mut self.on_marks), decided: std::mem::take(&mut self.decided) };
        if p.open() > 0 {
            self.parked.push((row, p));
        }
    }

    /// Brings a row's results to the front; `fresh` drops what it had.
    pub fn front(&mut self, row: usize, fresh: bool) {
        if self.active == Some(row) {
            if fresh {
                self.drop_front();
                self.active = Some(row);
            }
            return;
        }
        self.park();
        let p = self.parked.iter().position(|(r, _)| *r == row).map(|i| self.parked.remove(i).1);
        if let Some(p) = p.filter(|_| !fresh) {
            self.marks = p.marks;
            self.made = p.made;
            self.trim = p.trim;
            self.on_marks = p.on_marks;
            self.decided = p.decided;
        }
        self.active = Some(row);
    }

    fn drop_front(&mut self) {
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.on_marks = false;
        self.marks = None;
        self.made = None;
        self.trim = None;
        self.error = None;
        self.decided.clear();
        self.active = None;
    }

    /// (position, count) of the card over the groups, then the marks.
    pub fn seq(&self) -> (usize, usize) {
        let g = self.made.as_ref().map_or(0, |t| t.groups.len());
        let m = self.marks.as_ref().map_or(0, |m| m.items.len());
        let at = match (&self.made, &self.marks) {
            (Some(_), Some(m)) if self.on_marks => g + m.current,
            (Some(t), _) => t.current,
            (None, Some(m)) => m.current,
            (None, None) => 0,
        };
        (at, g + m)
    }

    pub fn at_mark(&self) -> bool {
        self.marks.is_some() && (self.on_marks || self.made.is_none())
    }
}

pub const MENU_ROWS: usize = Pass::ALL.len() + 1;
pub const TRIM_ROW: usize = Pass::ALL.len();

pub fn pass_row(p: Pass) -> usize {
    Pass::ALL.iter().position(|&q| q == p).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiState {
    Ready,
    NoModel,
    Off,
}

impl AiState {
    pub fn of(cx: &App) -> AiState {
        if !crate::assist::on(cx) {
            AiState::Off
        } else if !crate::assist::connected(cx) {
            AiState::NoModel
        } else {
            AiState::Ready
        }
    }

    pub(crate) fn rows(self) -> Vec<usize> {
        match self {
            AiState::Ready | AiState::NoModel => (0..MENU_ROWS).collect(),
            AiState::Off => vec![1, 2],
        }
    }

    pub(crate) fn runs(self, row: usize) -> bool {
        self == AiState::Ready || matches!(Pass::ALL.get(row), Some(Pass::HardToRead | Pass::Tone))
    }
}


/// Where `quote` is now, nearest to `old`.
/// The text either side of `range`, as far as an idea could repeat it.
pub(crate) fn near(text: &[char], range: &Range<usize>) -> (String, String) {
    const REACH: usize = 120;
    let end = range.end.min(text.len());
    let start = range.start.min(end);
    (text[start.saturating_sub(REACH)..start].iter().collect(), text[end..(end + REACH).min(text.len())].iter().collect())
}

fn refind(text: &[char], old: &Range<usize>, quote: &str) -> Option<Range<usize>> {
    let q: Vec<char> = quote.chars().collect();
    if q.is_empty() || q.len() > text.len() {
        return None;
    }
    if text.get(old.clone()) == Some(&q[..]) {
        return Some(old.clone());
    }
    let mut best: Option<usize> = None;
    for i in 0..=text.len() - q.len() {
        if text[i..i + q.len()] == q[..] && best.is_none_or(|b| i.abs_diff(old.start) < b.abs_diff(old.start)) {
            best = Some(i);
        }
    }
    best.map(|i| i..i + q.len())
}

/// Results find their words again after an edit; what can't be found goes.
fn follow(marks: &mut Option<Marks>, made: &mut Option<Made>, trim: &mut Option<Trim>, on_marks: &mut bool, text: &[char], doc: &doc_model::Document) {
    if let Some(m) = marks {
        m.items = m.items.drain(..).filter_map(|mut k| refind(text, &k.range, &k.quote).map(|r| { k.range = r; k })).collect();
        if m.items.is_empty() {
            *marks = None;
            *on_marks = false;
        } else {
            m.current = m.current.min(m.items.len() - 1);
        }
    }
    if let Some(t) = trim {
        for (i, c) in t.cuts.iter_mut().enumerate() {
            match refind(text, &c.range, &c.quote) {
                Some(r) => c.range = r,
                None => {
                    t.gone.insert(i);
                }
            }
        }
        if t.showing().is_empty() && t.kept.is_empty() {
            *trim = None;
        }
    }
    if let Some(ty) = made {
        let before = ty.groups.len();
        let fixes: Vec<GroupId> = ty.groups[..ty.fixed].to_vec();
        ty.groups.retain(|g| doc.group(*g).is_some());
        ty.fixed = fixes.iter().filter(|g| ty.groups.contains(g)).count();
        if ty.groups.len() != before && ty.groups.is_empty() && marks.is_some() {
            *on_marks = true;
        }
        if ty.groups.is_empty() {
            *made = None;
        } else {
            ty.current = ty.current.min(ty.groups.len() - 1);
        }
    }
}

enum Reply {
    Pass(Pass, Result<String, AiError>),
    Trim(Result<String, AiError>),
    Alts(Vec<Result<String, AiError>>),
}

pub const SUGGESTIONS: usize = 3;
const AT_ONCE: usize = 6;

impl Workspace {
    pub(crate) fn lab_ease(&self, cx: &App) -> Option<u8> {
        self.model.read(cx).reading(Some(self.lab_range(cx))).and_then(|(lang, s)| prose::ease(s, lang))
    }

    pub(crate) fn lab_range(&self, cx: &App) -> Range<usize> {
        let sel = self.page.read(cx).selection();
        if sel.is_empty() {
            0..self.model.read(cx).doc.len()
        } else {
            sel
        }
    }

    /// ⌥⌘L: the Review tab, opened or closed.
    pub fn toggle_lab(&mut self, _: &ToggleLab, window: &mut Window, cx: &mut Context<Self>) {
        if self.lab.open && self.lab.focus.is_focused(window) {
            return self.close_review(window, cx);
        }
        self.open_review(window, cx);
    }

    /// Writing has no Review: it goes to editing first.
    pub(crate) fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enter_editing(cx);
        if self.sidebar.is_some() {
            self.close_sidebar(window, cx);
        }
        self.lab.open = true;
        self.lab.error = self.lab.error.take().filter(|_| self.lab.results);
        if !self.lab.results {
            let state = AiState::of(cx);
            if !state.runs(self.lab.at) || !state.rows().contains(&self.lab.at) {
                self.lab.at = state.rows().into_iter().find(|&r| state.runs(r)).unwrap_or(0);
            }
        }
        window.focus(&self.lab.focus, cx);
        self.sync_lab(cx);
    }

    pub(crate) fn close_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.lab.open {
            return;
        }
        self.lab.open = false;
        self.lab_page_keys(window, cx);
        cx.notify();
    }

    fn lab_page_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
    }

    pub(crate) fn lab_on_prefs_change(&mut self, cx: &mut Context<Self>) {
        if !crate::assist::on(cx) && self.ideas.is_some() {
            self.stop_ideas(cx);
            self.ideas = None;
            cx.notify();
        }
        if (self.lab.waiting() || !self.lab.parked.is_empty()) && !crate::assist::connected(cx) {
            self.lab_clear_all(cx);
        }
    }

    /// The results in front go; the other passes' stay.
    pub(crate) fn lab_clear(&mut self, cx: &mut Context<Self>) {
        self.lab.drop_front();
        self.lab.results = false;
        self.sync_lab(cx);
    }

    fn lab_clear_all(&mut self, cx: &mut Context<Self>) {
        self.lab.parked.clear();
        self.lab_clear(cx);
    }

    /// Esc from the page. Returns whether it had something to do.
    pub(crate) fn lab_back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.model.read(cx).layer {
            return false;
        }
        if let Some(r) = self.lab.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
            self.sync_lab(cx);
            return true;
        }
        if !self.lab.open {
            return false;
        }
        if self.lab.showing_results() {
            self.lab.results = false;
            cx.notify();
        } else {
            self.close_review(window, cx);
        }
        true
    }

    /// Writing hides the tab and the marks; the results wait.
    pub(crate) fn lab_leave_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_review(window, cx);
        self.sync_lab(cx);
    }

    pub(crate) fn lab_on_model_change(&mut self, cx: &mut Context<Self>) {
        if self.lab.marks.is_none() && self.lab.trim.is_none() && self.lab.made.is_none() && self.lab.parked.is_empty() {
            if *self.page.read(cx).lab != LabPaint::default() {
                self.sync_lab(cx);
            }
            return;
        }
        let text: Vec<char> = self.model.read(cx).doc.text().chars().collect();
        let doc = &self.model.read(cx).doc;
        let lab = &mut self.lab;
        follow(&mut lab.marks, &mut lab.made, &mut lab.trim, &mut lab.on_marks, &text, doc);
        for (_, p) in &mut lab.parked {
            follow(&mut p.marks, &mut p.made, &mut p.trim, &mut p.on_marks, &text, doc);
        }
        lab.parked.retain(|(_, p)| p.open() > 0);
        self.lab_tidy(cx);
    }

    pub(crate) fn sync_lab(&mut self, cx: &mut Context<Self>) {
        let mut paint = LabPaint::default();
        let editing = self.model.read(cx).layer;
        let on_mark = self.lab.at_mark();
        if let Some(m) = self.lab.marks.as_ref().filter(|_| editing) {
            let word = m.pass.word_level();
            for (i, k) in m.items.iter().enumerate() {
                paint.marks.push(PaintMark { range: k.range.clone(), word, current: on_mark && i == m.current, note: (!k.note.is_empty()).then(|| k.note.clone()) });
            }
        }
        if let Some(ty) = self.lab.made.as_ref().filter(|_| editing) {
            let groups = self.page.read(cx).groups().to_vec();
            let on = ty.groups.get(ty.current).copied().filter(|_| !on_mark);
            for (g, note) in &ty.notes {
                if let Some(v) = groups.iter().find(|v| v.id == *g) {
                    // The groups' own underlines are enough on the page; keep only the notes.
                    let at = v.range.start;
                    paint.marks.push(PaintMark { range: at..at, word: false, current: Some(*g) == on, note: Some(note.clone()) });
                }
            }
        }
        if let Some(t) = self.lab.trim.as_ref().filter(|_| editing) {
            let current = t.current().filter(|_| self.lab.open && self.lab.showing_results());
            for i in t.showing() {
                paint.cuts.push((t.cuts[i].range.clone(), Some(i) == current));
            }
        }
        let paint = std::rc::Rc::new(paint);
        self.page.update(cx, |p, cx| {
            if *p.lab != *paint {
                p.lab = paint;
                cx.notify();
            }
        });
        cx.notify();
    }

    pub(crate) fn run_pass(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.lab.front(pass_row(pass), true);
        self.lab.results = true;
        self.sync_lab(cx);
        let range = self.lab_range(cx);
        let text = self.model.read(cx).doc.text();
        let sentences = ai::segment::sentences(&text, range);
        if sentences.is_empty() {
            self.flash_status(tr!("Nothing to read here"), cx);
            return;
        }
        let deep = if AiState::of(cx) == AiState::Ready { Some(crate::assist::for_slot(Slot::Deep, cx)) } else { None };
        let (ep, m) = match deep {
            Some(Ok(x)) => x,
            Some(Err(e)) => return self.lab_failed(e, ServiceKind::OpenRouter, cx),
            None if pass == Pass::Mistakes => return,
            None => return self.local_pass(pass, &text, &sentences, cx),
        };
        if !self.ai_allowed(ep.kind, cx, move |this, cx| this.run_pass(pass, cx)) {
            return;
        }
        let idx = self.page.read(cx).index().clone();
        let title = crate::ideas::doc_title(&idx);
        let found = self.found_on_mac(pass, &text, &sentences, cx);
        let beside = !found.is_empty() && self.show_marks(pass, found, &text, cx);
        let req = ai::lab::request(pass, &m.model, title.as_deref(), &sentences);
        self.start_lab_request(pass.working_label(), crate::assist::model_label(&m, cx), cx, move |cancel| Reply::Pass(pass, ai::client::chat(&ep, &req, cancel)), move |this, reply, cx| {
            let Reply::Pass(pass, r) = reply else { return };
            let kind = this.lab_kind(cx);
            match r.and_then(|reply| {
                let chars: Vec<char> = text.chars().collect();
                ai::lab::parse(pass, &reply, &sentences, &chars)
            }) {
                Ok(marks) if pass == Pass::Mistakes => this.apply_mistakes(marks, &text, cx),
                Ok(marks) if pass != Pass::Mistakes && this.lab.marks.is_some() => this.join_marks(marks, &text, cx),
                Ok(marks) => {
                    this.show_marks(pass, marks, &text, cx);
                }
                // The Mac's marks still show: report the error and keep them.
                Err(e) if pass != Pass::Mistakes && this.lab.marks.is_some() => {
                    if e != AiError::Cancelled {
                        this.flash_status(e.message(kind), cx);
                    }
                }
                Err(e) => this.lab_failed(e, kind, cx),
            }
        });
        if let Some(r) = self.lab.running.as_mut() {
            r.beside = beside;
        }
    }

    fn found_on_mac(&self, pass: Pass, text: &str, sentences: &[ai::segment::Sentence], cx: &App) -> Vec<ai::lab::Mark> {
        match pass {
            Pass::HardToRead => {
                let long = ai::lab::long_sentences(sentences);
                let mut found = passive_sentences(text, sentences, &long);
                found.extend(long);
                found.sort_by_key(|m| m.range.start);
                found
            }
            Pass::Tone => echo_marks(text, sentences, crate::ideas::doc_title(self.page.read(cx).index()).as_deref()),
            Pass::Mistakes => Vec::new(),
        }
    }

    fn local_pass(&mut self, pass: Pass, text: &str, sentences: &[ai::segment::Sentence], cx: &mut Context<Self>) {
        let found = self.found_on_mac(pass, text, sentences, cx);
        if found.is_empty() {
            return self.flash_status(pass.nothing_label(), cx);
        }
        self.show_marks(pass, found, text, cx);
    }

    fn lab_kind(&self, cx: &App) -> ServiceKind {
        crate::prefs::Prefs::get(cx).ai.deep.as_ref().map(|m| m.service).unwrap_or(ServiceKind::OpenRouter)
    }

    fn lab_failed(&mut self, e: AiError, kind: ServiceKind, cx: &mut Context<Self>) {
        self.lab.running = None;
        if e != AiError::Cancelled {
            self.lab.error = Some((e.message(kind), matches!(e, AiError::NotSetUp | AiError::Auth | AiError::Refused | AiError::OutOfRoom | AiError::BadReply(_))));
        }
        self.sync_lab(cx);
    }

    fn start_lab_request(
        &mut self,
        label: String,
        model: String,
        cx: &mut Context<Self>,
        job: impl FnOnce(&AtomicBool) -> Reply + Send + 'static,
        done: impl FnOnce(&mut Workspace, Reply, &mut Context<Workspace>) + 'static,
    ) {
        // One request at a time: a new one stops the running one.
        if let Some(r) = self.lab.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let fut = crate::assist::off_thread(move || job(&c2));
        let c3 = cancel.clone();
        let task = cx.spawn(async move |this, cx| {
            let Some(reply) = fut.await else { return };
            if c3.load(Ordering::Relaxed) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.lab.running = None;
                done(this, reply, cx);
                cx.notify();
            });
        });
        self.lab.error = None;
        self.lab.running = Some(Running { label, model, beside: false, cancel, _task: task });
        cx.notify();
    }

    /// The request in front stops; what the Mac found stays.
    pub(crate) fn stop_lab_request(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.lab.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.lab_tidy(cx);
    }

    fn lab_text_now(&self, cx: &App) -> Vec<char> {
        self.model.read(cx).doc.text().chars().collect()
    }

    /// `range` in `then`, found again in `now`, with its words; `None` if gone.
    fn reanchor(then: &[char], now: &[char], range: &Range<usize>) -> Option<(Range<usize>, String)> {
        let quote: String = then.get(range.clone())?.iter().collect();
        Some((refind(now, range, &quote)?, quote))
    }

    fn anchored(marks: Vec<ai::lab::Mark>, text: &str, now: &[char]) -> Vec<Mark> {
        let then: Vec<char> = text.chars().collect();
        marks.into_iter().filter_map(|m| Self::reanchor(&then, now, &m.range).map(|(range, quote)| Mark { range, quote, note: m.note, found: m.found })).collect()
    }

    /// False if none were left to show.
    fn show_marks(&mut self, pass: Pass, marks: Vec<ai::lab::Mark>, text: &str, cx: &mut Context<Self>) -> bool {
        let now = self.lab_text_now(cx);
        let items = Self::anchored(marks, text, &now);
        if items.is_empty() {
            self.flash_status(pass.nothing_label(), cx);
            self.sync_lab(cx);
            return false;
        }
        let first = items[0].range.clone();
        self.lab.marks = Some(Marks { pass, items, current: 0 });
        self.page.update(cx, |p, cx| p.centre_on(first, cx));
        self.sync_lab(cx);
        true
    }

    fn join_marks(&mut self, marks: Vec<ai::lab::Mark>, text: &str, cx: &mut Context<Self>) {
        let now = self.lab_text_now(cx);
        let found = Self::anchored(marks, text, &now);
        let Some(m) = &mut self.lab.marks else { return };
        let on = m.items.get(m.current).map(|k| k.range.start);
        for k in found {
            if !m.items.iter().any(|o| o.range.start < k.range.end && k.range.start < o.range.end) {
                m.items.push(k);
            }
        }
        m.items.sort_by_key(|k| k.range.start);
        m.current = on.and_then(|s| m.items.iter().position(|k| k.range.start == s)).unwrap_or(0);
        self.sync_lab(cx);
    }

    fn apply_mistakes(&mut self, marks: Vec<ai::lab::Mark>, text: &str, cx: &mut Context<Self>) {
        // The text may have changed while the pass ran: fix only words still there.
        let then: Vec<char> = text.chars().collect();
        let now = self.lab_text_now(cx);
        let (fixes, rebuild): (Vec<ai::lab::Mark>, Vec<ai::lab::Mark>) = marks.into_iter().partition(|m| m.fix.is_some());
        let anchored: Vec<(Range<usize>, String, String)> =
            fixes.iter().filter_map(|m| Some((Self::reanchor(&then, &now, &m.range)?.0, m.fix.clone()?, m.note.clone()))).collect();
        let rebuild = Self::anchored(rebuild, text, &now);
        if anchored.is_empty() && rebuild.is_empty() {
            self.flash_status(Pass::Mistakes.nothing_label(), cx);
            return;
        }
        let pairs: Vec<(Range<usize>, String)> = anchored.iter().map(|(r, f, _)| (r.clone(), f.clone())).collect();
        let ids = if pairs.is_empty() { Vec::new() } else { self.model.update(cx, |m, cx| m.edit(cx, |d| d.fix_words(&pairs))).unwrap_or_default() };
        if ids.is_empty() && rebuild.is_empty() {
            self.flash_status(tr!("The fixes overlap other alternatives; nothing was changed"), cx);
            return;
        }
        self.enter_editing(cx);
        let notes: Vec<(GroupId, String)> = ids
            .iter()
            .filter_map(|g| {
                let info = self.model.read(cx).doc.group(*g)?;
                let fix = info.variants.get(1)?.text.clone();
                // The note names the mistake ("spelling"); the row draws the change itself.
                let note = anchored.iter().find(|(_, f, _)| *f == fix).map(|(_, _, n)| n.clone()).filter(|n| !n.trim().is_empty())?;
                Some((*g, note))
            })
            .collect();
        let fixed = ids.len();
        if !ids.is_empty() {
            self.lab.made = Some(Made { pass: Pass::Mistakes, groups: ids, fixed, notes, current: 0 });
        }
        if !rebuild.is_empty() {
            self.lab.marks = Some(Marks { pass: Pass::Mistakes, items: rebuild, current: 0 });
        }
        self.lab.on_marks = self.lab.made.is_none();
        if self.lab.on_marks {
            self.centre_mark(cx);
        } else {
            self.centre_made(cx);
        }
        self.sync_lab(cx);
    }

    pub fn next_mark(&mut self, _: &NextMark, _: &mut Window, cx: &mut Context<Self>) {
        self.step_mark(1, cx);
    }

    pub fn prev_mark(&mut self, _: &PrevMark, _: &mut Window, cx: &mut Context<Self>) {
        self.step_mark(-1, cx);
    }

    pub(crate) fn centre_mark(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.lab.marks.as_ref().and_then(|m| m.items.get(m.current)).map(|k| k.range.clone()) else { return };
        self.page.update(cx, |p, cx| p.centre_on(r, cx));
    }

    pub(crate) fn step_mark(&mut self, d: isize, cx: &mut Context<Self>) {
        if !self.model.read(cx).layer {
            return;
        }
        if let Some(t) = &mut self.lab.trim {
            let n = t.list().len();
            if n > 0 {
                t.at = (t.at.min(n - 1) as isize + d).rem_euclid(n as isize) as usize;
                self.centre_trim_cut(cx);
            }
            return;
        }
        let (at, n) = self.lab.seq();
        if n == 0 {
            return;
        }
        self.lab_go_to((at as isize + d).rem_euclid(n as isize) as usize, cx);
    }

    /// `to` counts the groups first, then the marks, as `Lab::seq`.
    pub(crate) fn lab_go_to(&mut self, to: usize, cx: &mut Context<Self>) {
        let g = self.lab.made.as_ref().map_or(0, |t| t.groups.len());
        if to < g {
            self.lab.on_marks = false;
            if let Some(t) = &mut self.lab.made {
                t.current = to;
            }
            self.centre_made(cx);
        } else {
            self.lab.on_marks = true;
            if let Some(m) = &mut self.lab.marks {
                m.current = (to - g).min(m.items.len().saturating_sub(1));
            }
            self.centre_mark(cx);
        }
        self.lab.scroll.scroll_to_item(to);
        self.sync_lab(cx);
    }

    pub(crate) fn centre_trim_cut(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.lab.trim.as_ref().and_then(|t| t.current().map(|i| t.cuts[i].range.clone())) else { return };
        if let Some(t) = &self.lab.trim {
            self.lab.scroll.scroll_to_item(t.at);
        }
        self.page.update(cx, |p, cx| p.centre_on(r, cx));
        self.sync_lab(cx);
    }

    pub(crate) fn centre_made(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.lab.made.as_ref().and_then(|t| t.groups.get(t.current).copied()) else { return };
        let Some(r) = self.model.read(cx).doc.group_range(g) else { return };
        self.page.update(cx, |p, cx| p.centre_on(r, cx));
    }

    /// The current mark goes, the text isn't touched.
    pub(crate) fn dismiss_mark(&mut self, cx: &mut Context<Self>) {
        self.drop_mark(Outcome::Dismissed, cx);
    }

    fn drop_mark(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        let Some(m) = &mut self.lab.marks else { return };
        if m.current < m.items.len() {
            let k = m.items.remove(m.current);
            self.lab.decided.push(Decided { at: k.range.start, label: k.quote.trim().to_string(), diff: None, mark: true, outcome });
        }
        if m.items.is_empty() {
            self.lab.marks = None;
            self.lab.on_marks = false;
            self.centre_made(cx);
        } else {
            m.current = m.current.min(m.items.len() - 1);
            self.centre_mark(cx);
        }
        self.lab_tidy(cx);
    }

    /// Every open mark goes.
    pub(crate) fn dismiss_all(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.lab.marks.take() else { return };
        for k in m.items {
            self.lab.decided.push(Decided { at: k.range.start, label: k.quote.trim().to_string(), diff: None, mark: true, outcome: Outcome::Dismissed });
        }
        self.lab.on_marks = false;
        self.lab_tidy(cx);
    }

    /// Nothing left to deal with: the results go and the tab shows the passes.
    fn lab_tidy(&mut self, cx: &mut Context<Self>) {
        let l = &self.lab;
        if l.active.is_some() && l.open_here() == 0 && l.running.is_none() && l.error.is_none() {
            self.lab.drop_front();
            self.lab.results = false;
        }
        self.sync_lab(cx);
    }

    /// One group settles: on variant `index`, or on what it shows.
    pub(crate) fn settle_lab_group(&mut self, g: GroupId, index: Option<usize>, cx: &mut Context<Self>) {
        self.decide_groups(&[g], index, false, cx);
        let _ = self.model.update(cx, |m, cx| m.edit(cx, |d| match index {
            Some(i) => d.settle_to(&[g], i).map(|_| ()),
            None => d.settle(g),
        }));
        self.lab_tidy(cx);
    }

    /// Every open group settles: on its original (`mine`) or on what it shows.
    pub(crate) fn settle_all(&mut self, mine: bool, cx: &mut Context<Self>) {
        let Some(ids) = self.lab.made.as_ref().map(|t| t.groups.clone()) else { return };
        self.decide_groups(&ids, mine.then_some(0), false, cx);
        let _ = self.model.update(cx, |m, cx| m.edit(cx, |d| if mine { d.settle_to(&ids, 0) } else { d.settle_many(&ids) }));
        self.lab_tidy(cx);
    }

    /// Groups leave the list with their alternatives, the AI's choice showing; `ids` that can't
    /// be kept (typos) stay open.
    pub(crate) fn keep_lab_groups(&mut self, ids: &[GroupId], cx: &mut Context<Self>) {
        let ids: Vec<GroupId> = ids.iter().copied().filter(|g| self.lab_keepable(*g, cx)).collect();
        if ids.is_empty() {
            return;
        }
        self.decide_groups(&ids, None, true, cx);
        let Some(ty) = &mut self.lab.made else { return };
        let fixes: Vec<GroupId> = ty.groups[..ty.fixed].to_vec();
        ty.groups.retain(|g| !ids.contains(g));
        ty.fixed = fixes.iter().filter(|g| ty.groups.contains(g)).count();
        if ty.groups.is_empty() {
            self.lab.made = None;
            self.lab.on_marks = self.lab.marks.is_some();
        } else {
            ty.current = ty.current.min(ty.groups.len() - 1);
        }
        self.lab_tidy(cx);
    }

    /// Every open group that can be kept.
    pub(crate) fn lab_keepable_groups(&self, cx: &App) -> Vec<GroupId> {
        self.lab.made.as_ref().map_or(Vec::new(), |t| t.groups.iter().copied().filter(|g| self.lab_keepable(*g, cx)).collect())
    }

    /// A group can stay with its alternatives unless it's a typo's fix.
    pub(crate) fn lab_keepable(&self, g: GroupId, cx: &App) -> bool {
        let Some(made) = &self.lab.made else { return false };
        let Some(i) = made.groups.iter().position(|x| *x == g) else { return false };
        if i >= made.fixed {
            return true;
        }
        let doc = &self.model.read(cx).doc;
        let (Some(info), Some(range)) = (doc.group(g), doc.group_range(g)) else { return false };
        let (Some(was), Some(now)) = (info.variants.first(), info.variants.get(1)) else { return false };
        let parts = crate::review_text::word_diff(&was.text, &now.text);
        // Checked in its sentence: alone, a word can pass as another language's ("teh" is tea).
        let idx = self.page.read(cx).index();
        let (before, after) = crate::review_text::around(idx, range, crate::review_text::BEFORE, crate::review_text::AFTER);
        let line = format!("{before}{}{after}", was.text);
        let wrong: Vec<String> = crate::spell::Spelling::misspelled(cx, &line).into_iter().map(|r| line.chars().skip(r.start).take(r.len()).collect()).collect();
        !crate::review_text::typo(&parts, |w| wrong.iter().any(|x| x == w))
    }

    fn decide_groups(&mut self, ids: &[GroupId], index: Option<usize>, kept: bool, cx: &App) {
        let Some(made) = &self.lab.made else { return };
        let fixes: Vec<GroupId> = made.groups[..made.fixed].to_vec();
        let doc = &self.model.read(cx).doc;
        let mut out = Vec::new();
        for g in ids {
            let (Some(info), Some(r)) = (doc.group(*g), doc.group_range(*g)) else { continue };
            let chosen = index.filter(|&i| i < info.variants.len()).unwrap_or(info.active);
            let fix = fixes.contains(g);
            let to = info.variants.get(1).map_or("", |v| v.text.as_str());
            let label = if fix { format!("{} → {to}", info.variants[0].text) } else { info.variants[chosen].text.clone() };
            let diff = fix.then(|| (info.variants[0].text.clone(), to.to_string()));
            let outcome = if kept {
                Outcome::Kept
            } else if chosen == 0 {
                Outcome::Mine
            } else {
                Outcome::Settled
            };
            out.push(Decided { at: r.start, label, diff, mark: false, outcome });
        }
        self.lab.decided.extend(out);
    }

    pub(crate) fn keep_mark_as_comment(&mut self, cx: &mut Context<Self>) {
        let Some(k) = self.lab.marks.as_ref().and_then(|m| m.items.get(m.current)) else { return };
        let (range, note) = (k.range.clone(), k.note.trim().to_string());
        if note.is_empty() || range.is_empty() {
            return;
        }
        let created = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let kept = self.model.update(cx, |m, cx| {
            m.doc.break_undo_group();
            let r = m.edit(cx, |d| d.add_comment(range, &note, Some(created)));
            m.doc.break_undo_group();
            r
        });
        match kept {
            Ok(_) => {
                self.drop_mark(Outcome::Comment, cx);
                self.flash_status(tr!("Kept as a comment · ⌘Z takes it back"), cx);
            }
            Err(_) => self.flash_status(tr!("A cut's text can't have a comment"), cx),
        }
    }

    /// Versions for one mark (`only`, an index into the marks) or all of them.
    pub(crate) fn suggest_for_marks(&mut self, only: Option<usize>, cx: &mut Context<Self>) {
        let (ep, model) = match crate::assist::for_slot(Slot::Quick, cx) {
            Ok(x) => x,
            Err(e) => return self.lab_failed(e, ServiceKind::OpenRouter, cx),
        };
        if !self.ai_allowed(ep.kind, cx, move |this, cx| this.suggest_for_marks(only, cx)) {
            return;
        }
        let Some(m) = &self.lab.marks else { return };
        let pass = m.pass;
        let scope = if pass.word_level() { ai::ideas::Scope::Word } else { ai::ideas::Scope::Sentence };
        // The text may change before the replies land: keep what was asked about to find again.
        let asked: Vec<Mark> = match only {
            Some(i) => m.items.get(i).cloned().into_iter().collect(),
            None => m.items.clone(),
        };
        if asked.is_empty() {
            return;
        }
        let reqs: Vec<ai::ChatRequest> = asked
            .iter()
            .map(|k| {
                let ctx = self.ideas_context(k.range.clone(), cx);
                let existing = vec![k.quote.clone()];
                let steer = Some(pass.steer(&k.note, k.found));
                let ask = ai::ideas::Ask {
                    title: Some(&ctx.title),
                    before: &ctx.before,
                    paragraph: &ctx.paragraph,
                    after: &ctx.after,
                    current: &k.quote,
                    scope,
                    existing: &existing,
                    steer: steer.as_deref(),
                    count: SUGGESTIONS,
                };
                ai::ideas::request(&model.model, &ask)
            })
            .collect();
        let kind = ep.kind;
        let n = reqs.len();
        let label = match (scope, n) {
            (ai::ideas::Scope::Word, n) => trn!("Finding alternatives for {n} word…", "Finding alternatives for {n} words…", n),
            (_, n) => trn!("Finding versions of {n} sentence…", "Finding versions of {n} sentences…", n),
        };
        self.start_lab_request(
            label,
            crate::assist::model_label(&model, cx),
            cx,
            move |cancel| {
                Reply::Alts(
                    reqs.chunks(AT_ONCE)
                        .flat_map(|batch| {
                            std::thread::scope(|s| {
                                let hs: Vec<_> = batch.iter().map(|r| s.spawn(|| ai::client::chat(&ep, r, cancel))).collect();
                                hs.into_iter().map(|h| h.join().unwrap_or(Err(AiError::Cancelled))).collect::<Vec<_>>()
                            })
                        })
                        .collect(),
                )
            },
            move |this, reply, cx| {
                let Reply::Alts(replies) = reply else { return };
                this.offer_alternatives(pass, asked, replies, kind, cx);
            },
        );
    }

    fn offer_alternatives(&mut self, pass: Pass, asked: Vec<Mark>, replies: Vec<Result<String, AiError>>, kind: ServiceKind, cx: &mut Context<Self>) {
        let scope = if pass.word_level() { ai::ideas::Scope::Word } else { ai::ideas::Scope::Sentence };
        let text = self.lab_text_now(cx);
        let mut offers: Vec<(Range<usize>, Vec<String>)> = Vec::new();
        let mut notes: Vec<(usize, String)> = Vec::new();
        let mut failed: Option<AiError> = None;
        for (k, r) in asked.iter().zip(replies) {
            let reply = match r {
                Ok(reply) => reply,
                Err(e) => {
                    failed.get_or_insert(e);
                    continue;
                }
            };
            let Some(range) = refind(&text, &k.range, &k.quote) else { continue };
            let mut lines = ai::ideas::Lines::default();
            let mut all = lines.feed(&reply);
            all.extend(lines.finish());
            let mut have = vec![k.quote.clone()];
            let (before, after) = near(&text, &range);
            for l in all {
                if have.len() > SUGGESTIONS {
                    break;
                }
                let l = if scope == ai::ideas::Scope::Word { ai::ideas::unrepeat(&l, &before, &after) } else { Some(l) };
                if let Some(a) = l.and_then(|l| ai::ideas::clean(&l, scope, &have)) {
                    have.push(a);
                }
            }
            if have.len() > 1 {
                notes.push((range.start, k.note.clone()));
                offers.push((range, have.split_off(1)));
            }
        }
        if offers.is_empty() {
            match failed {
                Some(e) => self.lab_failed(e, kind, cx),
                None => {
                    self.flash_status(tr!("No alternatives came back"), cx);
                    self.sync_lab(cx);
                }
            }
            return;
        }
        let ids = self
            .model
            .update(cx, |m, cx| m.edit(cx, |d| if scope == ai::ideas::Scope::Word { d.offer_words(&offers) } else { d.offer_sentences(&offers) }))
            .unwrap_or_default();
        if ids.is_empty() {
            self.flash_status(tr!("They overlap other alternatives; nothing was changed"), cx);
            self.sync_lab(cx);
            return;
        }
        let doc = &self.model.read(cx).doc;
        let made: Vec<Range<usize>> = ids.iter().filter_map(|g| doc.group_range(*g)).collect();
        if let Some(m) = &mut self.lab.marks {
            m.items.retain(|k| !made.iter().any(|r| r.start < k.range.end && k.range.start < r.end));
            m.current = m.current.min(m.items.len().saturating_sub(1));
        }
        if self.lab.marks.as_ref().is_some_and(|m| m.items.is_empty()) {
            self.lab.marks = None;
        }
        self.lab.on_marks = false;
        let notes: Vec<(GroupId, String)> = ids
            .iter()
            .filter_map(|g| {
                let start = doc.group_range(*g)?.start;
                let note = notes.iter().find(|(s, _)| *s == start).map(|(_, n)| n.clone()).filter(|n| !n.is_empty())?;
                Some((*g, note))
            })
            .collect();
        let n = ids.len();
        match &mut self.lab.made {
            Some(t) if t.pass == pass => {
                t.current = t.groups.len();
                t.groups.extend(ids);
                t.notes.extend(notes);
            }
            _ => self.lab.made = Some(Made { pass, groups: ids, fixed: 0, notes, current: 0 }),
        }
        if n < asked.len() {
            let total = asked.len();
            let msg = match (scope == ai::ideas::Scope::Word, failed.map(|e| e.message(kind))) {
                (true, None) => trn!("Alternatives for {got} of {n} word", "Alternatives for {got} of {n} words", total, got = n),
                (true, Some(why)) => trn!("Alternatives for {got} of {n} word: {why}", "Alternatives for {got} of {n} words: {why}", total, got = n, why = why),
                (false, None) => trn!("Alternatives for {got} of {n} sentence", "Alternatives for {got} of {n} sentences", total, got = n),
                (false, Some(why)) => trn!("Alternatives for {got} of {n} sentence: {why}", "Alternatives for {got} of {n} sentences: {why}", total, got = n, why = why),
            };
            self.flash_status(msg, cx);
        }
        self.centre_made(cx);
        self.sync_lab(cx);
    }

    /// Asks the model once; the slider then picks from the ranked cuts without asking again.
    pub(crate) fn trim_to(&mut self, percent: usize, cx: &mut Context<Self>) {
        if percent == 0 {
            return;
        }
        self.lab.trim_pc = percent;
        self.lab.front(TRIM_ROW, true);
        self.lab.results = true;
        self.sync_lab(cx);
        let range = self.lab_range(cx);
        let text = self.model.read(cx).doc.text();
        let sentences = ai::segment::sentences(&text, range);
        if sentences.is_empty() {
            self.flash_status(tr!("Nothing to read here"), cx);
            return;
        }
        let (ep, m) = match crate::assist::for_slot(Slot::Deep, cx) {
            Ok(x) => x,
            Err(e) => return self.lab_failed(e, ServiceKind::OpenRouter, cx),
        };
        if !self.ai_allowed(ep.kind, cx, move |this, cx| this.trim_to(percent, cx)) {
            return;
        }
        let idx = self.page.read(cx).index().clone();
        let title = crate::ideas::doc_title(&idx);
        let req = ai::trim::request(&m.model, title.as_deref(), &sentences);
        let model = crate::assist::model_label(&m, cx);
        self.start_lab_request(tr!("Finding what could go…").into(), model, cx, move |cancel| Reply::Trim(ai::client::chat(&ep, &req, cancel)), move |this, reply, cx| {
            let Reply::Trim(r) = reply else { return };
            let kind = this.lab_kind(cx);
            let chars: Vec<char> = text.chars().collect();
            match r.and_then(|reply| ai::trim::parse(&reply, &sentences, &chars)) {
                Ok(cuts) if cuts.is_empty() => this.flash_status(tr!("Nothing to cut"), cx),
                Ok(cuts) => {
                    let now = this.lab_text_now(cx);
                    let cuts: Vec<Cut> = cuts.into_iter().filter_map(|c| Self::reanchor(&chars, &now, &c.range).map(|(range, quote)| Cut { range, quote, words: c.words })).collect();
                    if cuts.is_empty() {
                        return this.flash_status(tr!("Nothing to cut"), cx);
                    }
                    let total = ai::segment::word_count(&sentences);
                    this.lab.trim = Some(Trim::new(cuts, total, percent));
                    this.sync_lab(cx);
                }
                Err(e) => this.lab_failed(e, kind, cx),
            }
        });
    }

    pub(crate) fn set_trim(&mut self, percent: usize, cx: &mut Context<Self>) {
        let Some(t) = &mut self.lab.trim else { return };
        let percent = percent.min(ai::trim::MAX);
        if t.percent == percent {
            return;
        }
        t.percent = percent;
        if percent > 0 {
            self.lab.trim_pc = percent;
        }
        self.sync_lab(cx);
    }

    pub(crate) fn nudge_trim(&mut self, d: isize, fine: bool, cx: &mut Context<Self>) {
        let Some(t) = &self.lab.trim else { return };
        let at = t.percent;
        let to = if fine {
            (at as isize + d).clamp(0, ai::trim::MAX as isize) as usize
        } else if d > 0 {
            LEVELS.iter().copied().find(|&l| l > at).unwrap_or(ai::trim::MAX)
        } else {
            LEVELS.iter().rev().copied().find(|&l| l < at).unwrap_or(0)
        };
        self.set_trim(to, cx);
    }

    pub fn trim_less_fine(&mut self, _: &TrimLessFine, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge_trim(-1, true, cx);
    }

    pub fn trim_more_fine(&mut self, _: &TrimMoreFine, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge_trim(1, true, cx);
    }

    pub(crate) fn keep_cut(&mut self, start: usize, cx: &mut Context<Self>) {
        let Some(t) = &mut self.lab.trim else { return };
        if let Some(i) = t.cuts.iter().position(|c| c.range.start == start) {
            t.kept.insert(i);
            if let Some(n) = t.list().iter().position(|&x| x == i) {
                t.at = n;
            }
        }
        self.sync_lab(cx);
    }

    pub(crate) fn make_cuts(&mut self, cx: &mut Context<Self>) {
        let Some(t) = &self.lab.trim else { return };
        let ranges: Vec<Range<usize>> = t.showing().iter().map(|&i| t.cuts[i].range.clone()).collect();
        let words = t.words_cut();
        if ranges.is_empty() {
            return;
        }
        // One batch, so Put Back undoes the whole trim.
        let batch = doc_model::Batch { id: format!("t{:x}", doc_model::new_link()), name: tr!("Trim −{pc}%", pc = t.percent).to_string() };
        if self.model.update(cx, |m, cx| m.edit(cx, |d| d.cut_many(&ranges, Some(batch)))).is_ok() {
            self.lab.trim = None;
            self.lab_tidy(cx);
            self.flash_status(trn!("{n} word cut · ⌘Z brings it back · ⌘/ puts it back", "{n} words cut · ⌘Z brings them back · ⌘/ puts one back", words), cx);
        }
    }

    /// Snaps to a level within a percent of it.
    pub(crate) fn trim_slider(at: usize, on_level: bool, cx: &mut Context<Self>) -> impl IntoElement {
        use std::cell::Cell;
        use std::rc::Rc;
        let max = ai::trim::MAX as f32;
        let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
        let percent_at = move |b: &Rc<Cell<Bounds<Pixels>>>, x: Pixels| -> Option<usize> {
            let b = b.get();
            if b.size.width <= px(0.) {
                return None;
            }
            let pc = (((x - b.left()) / b.size.width).clamp(0., 1.) * max).round() as usize;
            Some(LEVELS.iter().copied().find(|&l| l.abs_diff(pc) <= 1).unwrap_or(pc))
        };
        let (b1, b2) = (bounds.clone(), bounds.clone());
        let k = at as f32 / max;
        const LABEL_W: f32 = 40.;
        let track = div()
            .relative()
            .w_full()
            .h(px(4.))
            .rounded_full()
            .bg(c(theme::line()))
            .child(div().absolute().left_0().top_0().h_full().w(relative(k)).rounded_full().bg(c(theme::accent())))
            .children(LEVELS.iter().map(|&l| div().absolute().top(px(-2.)).left(relative(l as f32 / max)).ml(px(-1.)).w(px(2.)).h(px(8.)).rounded(px(1.)).bg(c(theme::faint()))))
            .child(
                div()
                    .absolute()
                    .top(px(-7.))
                    .left(relative(k))
                    .ml(px(-9.))
                    .size(px(18.))
                    .rounded_full()
                    .border_2()
                    .border_color(c(theme::accent()))
                    .bg(c(theme::bg_raised())),
            );
        let labels = div().relative().w_full().h(px(16.)).children(LEVELS.iter().enumerate().map(|(i, &l)| {
            let lit = on_level && l == at;
            let label = if l == 0 { "0".to_string() } else { tr!("−{pc}%", pc = l) };
            let pos = div().absolute().top_0().left(relative(l as f32 / max)).w(px(LABEL_W)).flex().flex_col();
            let pos = if i == 0 { pos.items_start() } else if l == ai::trim::MAX { pos.ml(px(-LABEL_W)).items_end() } else { pos.ml(px(-LABEL_W / 2.)).items_center() };
            pos.id(("trim-tick", i))
                .cursor_pointer()
                .text_size(px(11.))
                .text_color(if lit { c(theme::accent()) } else { c(theme::muted()) })
                .on_click(cx.listener(move |this, _, _, cx| this.set_trim(l, cx)))
                .child(label)
                .test_support()
        }));
        div()
            .id("trim-slider")
            .role(Role::Slider)
            .aria_label(tr!("How much to cut"))
            .aria_numeric_value(at as f64)
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(max as f64)
            .w_full()
            .px(px(9.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .on_children_prepainted({
                        let bounds = bounds.clone();
                        move |b, _, _| {
                            if let Some(b) = b.first() {
                                bounds.set(*b);
                            }
                        }
                    })
                    .id("trim-track")
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            if let Some(pc) = percent_at(&b1, e.position.x) {
                                this.set_trim(pc, cx);
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                        if e.pressed_button == Some(MouseButton::Left) {
                            if let Some(pc) = percent_at(&b2, e.position.x) {
                                this.set_trim(pc, cx);
                            }
                        }
                    }))
                    .child(track)
                    .test_support(),
            )
            .child(labels)
            .test_support()
    }
}

/// All arcs share one clock, so they turn in step.
pub(crate) fn busy(id: &'static str, size: f32, cx: &App) -> AnyElement {
    use std::time::Duration;
    if crate::prefs::Prefs::get(cx).motion() == crate::prefs::Effects::Off {
        return div()
            .id(id)
            .flex_none()
            .size(px(size))
            .rounded_full()
            .border(px(size / 6.))
            .border_color(c(theme::accent()))
            .test_support()
            .with_animation(id, Animation::new(Duration::from_secs(2)).repeat_synced(), |d, t| {
                d.opacity(0.35 + 0.65 * (0.5 - 0.5 * (t * std::f32::consts::TAU).cos()))
            })
            .into_any_element();
    }
    div()
        .id(id)
        .flex_none()
        .size(px(size))
        .test_support()
        .child(
            svg()
                .path("chrome/spinner.svg")
                .size(px(size))
                .text_color(c(theme::accent()))
                .with_animation(id, Animation::new(Duration::from_millis(900)).repeat_synced(), |s, t| s.with_transformation(Transformation::rotate(percentage(t)))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::refind;

    #[test]
    fn marks_follow_their_words() {
        let t: Vec<char> = "xx basically yy basically".chars().collect();
        assert_eq!(refind(&t, &(3..12), "basically"), Some(3..12));
        let t2: Vec<char> = "new xx basically yy basically".chars().collect();
        assert_eq!(refind(&t2, &(3..12), "basically"), Some(7..16), "the nearest one");
        assert_eq!(refind(&t2, &(3..12), "gone"), None);
    }
}
