use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ai::cost::Slot;
use ai::AiError;
use doc_model::{GroupId, Scope};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::tr;

use crate::actions::*;
use crate::markdown::{self, BlockKind};
use crate::sheets::{PrefSection, Sheet};
use crate::text::TextIndex;
use crate::theme::{self, c, ca};
use crate::workspace::Workspace;

pub fn per_ask(cx: &App) -> usize {
    crate::prefs::Prefs::get(cx).ai.ideas()
}

pub struct IdeasRun {
    pub group: GroupId,
    pub label: String,
    /// Fixed when the run starts.
    pub count: usize,
    pub got: usize,
    pub running: bool,
    pub error: Option<IdeasError>,
    /// The words after `??`, remembered once an idea for them arrives.
    steer: Option<String>,
    cancel: Arc<AtomicBool>,
    _task: Option<Task<()>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IdeasError {
    pub message: String,
    pub not_set_up: bool,
    /// Declined or failed: offer another model.
    pub other_model: bool,
}

enum Msg {
    Line(String),
    Done(Option<AiError>),
}

fn neighbour(idx: &TextIndex, block: &Range<usize>, forward: bool) -> String {
    let mut line = idx.line_of(if forward { block.end } else { block.start });
    loop {
        if forward {
            line += 1;
            if line >= idx.lines.len() {
                return String::new();
            }
        } else {
            if line == 0 {
                return String::new();
            }
            line -= 1;
        }
        let t = idx.line_str(line).trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
}

pub fn doc_title(idx: &TextIndex) -> Option<String> {
    (0..idx.lines.len().min(40)).find_map(|i| {
        let line = idx.line_str(i);
        let style = markdown::style_line(line);
        matches!(style.kind, BlockKind::Heading(_)).then(|| line[style.hidden_prefix..].trim().to_string()).filter(|t| !t.is_empty())
    })
}

pub struct IdeasContext {
    pub title: String,
    pub before: String,
    pub paragraph: String,
    pub after: String,
}

impl Workspace {
    pub(crate) fn ideas_context(&self, range: Range<usize>, cx: &App) -> IdeasContext {
        let idx = self.page.read(cx).index().clone();
        let block = idx.paragraph_at(range.start);
        let block_text = idx.slice(block.clone()).to_string();
        let paragraph = ai::ideas::mark(&block_text, range.start.saturating_sub(block.start), range.end.saturating_sub(block.start).min(block.len()));
        let (before, after) = (neighbour(&idx, &block, false), neighbour(&idx, &block, true));
        let title = doc_title(&idx).unwrap_or_else(|| self.model.read(cx).title());
        IdeasContext { title, before, paragraph, after }
    }

    pub fn ai_alternatives(&mut self, _: &AiAlternatives, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.page.update(cx, |v, cx| v.hide_selection_buttons(cx));
        if !crate::assist::on(cx) {
            return self.flash_status(tr!("AI is off (Preferences → AI)"), cx);
        }
        let sel_empty = self.page.read(cx).selection().is_empty();
        let open = self.sidebar.as_ref().and_then(|s| s.group);
        let id = match open {
            Some(id) if sel_empty => Some(id),
            _ => {
                self.alternatives_for_selection(&AlternativesForSelection, window, cx);
                self.sidebar.as_ref().and_then(|s| s.group)
            }
        };
        match id {
            Some(id) => {
                if open != Some(id) {
                    self.open_group_view(id, window, cx);
                }
                self.start_ideas(id, None, cx);
            }
            None => self.flash_status(tr!("Select text first"), cx),
        }
    }

    pub(crate) fn ai_alternatives_for_group(&mut self, id: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if !crate::assist::on(cx) {
            return self.flash_status(tr!("AI is off (Preferences → AI)"), cx);
        }
        if !self.page.read(cx).groups().iter().any(|g| g.id == id) {
            return;
        }
        if self.sidebar_group() != Some(id) {
            self.open_group_view(id, window, cx);
        }
        self.start_ideas(id, None, cx);
    }

    pub(crate) fn stop_ideas(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = &mut self.ideas {
            r.cancel.store(true, Ordering::Relaxed);
            if r.running {
                r.running = false;
                cx.notify();
            }
        }
    }

    /// `steer` is what followed `??`.
    pub(crate) fn start_ideas(&mut self, id: GroupId, steer: Option<String>, cx: &mut Context<Self>) {
        self.stop_ideas(cx);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut run = IdeasRun { group: id, label: String::new(), count: per_ask(cx), got: 0, running: false, error: None, steer: steer.clone(), cancel: cancel.clone(), _task: None };
        let (ep, m) = match crate::assist::for_slot(Slot::Quick, cx) {
            Ok(x) => x,
            Err(e) => {
                run.error = Some(IdeasError { message: e.message(ai::ServiceKind::OpenRouter), not_set_up: e == AiError::NotSetUp, other_model: false });
                self.ideas = Some(run);
                cx.notify();
                return;
            }
        };
        let again = steer.clone();
        if !self.ai_allowed(ep.kind, cx, move |this, cx| this.start_ideas(id, again, cx)) {
            return;
        }
        run.label = crate::assist::model_label(&m, cx);
        let kind = ep.kind;
        let Some(info) = self.model.read(cx).doc.group(id) else { return };
        let Some(range) = self.page.read(cx).groups().iter().find(|g| g.id == id).map(|g| g.range.clone()).or_else(|| self.model.read(cx).doc.group_range(id)) else {
            return;
        };
        let IdeasContext { title, before, paragraph, after } = self.ideas_context(range, cx);
        // A version without the words reads as ∅ to the model: it has that one.
        let existing: Vec<String> = info.variants.iter().map(|v| if v.cut { ai::ideas::NONE.to_string() } else { v.text.clone() }).collect();
        let current = info.variants[info.active].text.clone();
        let scope = match info.scope {
            Scope::Word => ai::ideas::Scope::Word,
            Scope::Sentence => ai::ideas::Scope::Sentence,
            Scope::Paragraph => ai::ideas::Scope::Paragraph,
        };
        let ask = ai::ideas::Ask {
            title: Some(&title),
            before: &before,
            paragraph: &paragraph,
            after: &after,
            current: &current,
            scope,
            existing: &existing,
            steer: steer.as_deref(),
            count: run.count,
        };
        let req = ai::ideas::request(&m.model, &ask);
        let (tx, rx) = smol::channel::unbounded::<Msg>();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            let mut lines = ai::ideas::Lines::default();
            let r = ai::client::chat_stream(&ep, &req, &c2, |piece| {
                for l in lines.feed(piece) {
                    let _ = tx.send_blocking(Msg::Line(l));
                }
            });
            if let Some(l) = lines.finish() {
                let _ = tx.send_blocking(Msg::Line(l));
            }
            let _ = tx.send_blocking(Msg::Done(r.err()));
        });
        let task = cx.spawn(async move |this, cx| {
            while let Ok(msg) = rx.recv().await {
                let go_on = this.update(cx, |this, cx| this.on_idea(id, kind, scope, msg, cx)).unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        });
        run.running = true;
        run._task = Some(task);
        self.ideas = Some(run);
        cx.notify();
    }

    /// Returns false when the run is over.
    fn on_idea(&mut self, id: GroupId, kind: ai::ServiceKind, scope: ai::ideas::Scope, msg: Msg, cx: &mut Context<Self>) -> bool {
        let Some(run) = self.ideas.as_ref().filter(|r| r.group == id && r.running) else { return false };
        let cancel = run.cancel.clone();
        match msg {
            Msg::Line(line) => {
                if run.got >= run.count {
                    return true;
                }
                let existing: Vec<String> = self.model.read(cx).doc.group(id).map(|g| g.variants.iter().map(|v| v.text.clone()).collect()).unwrap_or_default();
                // ∅: a version without the words (one per group; a second one is no idea).
                if ai::ideas::is_none(&line, scope) {
                    let added = self.model.update(cx, |m, cx| m.edit(cx, |d| d.add_cut_idea(id))).is_ok();
                    return if added { self.idea_arrived(cx) } else { true };
                }
                let line = if scope == ai::ideas::Scope::Word {
                    // A word's ideas sometimes bring the words around it along.
                    let doc = &self.model.read(cx).doc;
                    let Some(range) = doc.group_range(id) else { return true };
                    let text: Vec<char> = doc.text().chars().collect();
                    let (before, after) = crate::lab::near(&text, &range);
                    match ai::ideas::unrepeat(&line, &before, &after) {
                        Some(l) => l,
                        None => return true,
                    }
                } else {
                    line
                };
                let Some(idea) = ai::ideas::clean(&line, scope, &existing) else { return true };
                let added = self.model.update(cx, |m, cx| m.edit(cx, |d| d.add_idea(id, &idea))).is_ok();
                let Some(run) = self.ideas.as_mut() else { return false };
                if !added {
                    // The group went away.
                    run.running = false;
                    cancel.store(true, Ordering::Relaxed);
                    cx.notify();
                    return false;
                }
                self.idea_arrived(cx)
            }
            Msg::Done(err) => {
                let Some(run) = self.ideas.as_mut() else { return false };
                run.running = false;
                match err {
                    Some(AiError::Cancelled) | None => {
                        if run.got == 0 {
                            run.error = Some(IdeasError { message: tr!("No ideas came back.").into(), not_set_up: false, other_model: true });
                        }
                    }
                    // What arrived before a failure stays; the line says what happened.
                    Some(e) => {
                        let other = matches!(e, AiError::Refused | AiError::OutOfRoom | AiError::BadReply(_) | AiError::Server(..) | AiError::RateLimited);
                        run.error = Some(IdeasError { message: e.message(kind), not_set_up: e == AiError::NotSetUp || e == AiError::Auth, other_model: other })
                    }
                }
                cx.notify();
                false
            }
        }
    }

    /// Counts an idea that was added; false when that was the last one asked for.
    fn idea_arrived(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = self.ideas.as_mut() else { return false };
        run.got += 1;
        if run.got == 1 {
            if let Some(steer) = run.steer.clone() {
                crate::memory::Memory::record_steer(cx, &steer);
            }
        }
        let Some(run) = self.ideas.as_mut() else { return false };
        if run.got >= run.count {
            run.running = false;
            run.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
        true
    }

    pub(crate) fn open_ai_prefs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_sheet(Sheet::Prefs { section: PrefSection::Ai }, window, cx);
        crate::assist::on_prefs_open(cx);
    }

    pub(crate) fn render_ideas(&self, gid: GroupId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let run = self.ideas.as_ref().filter(|r| r.group == gid)?;
        let status = if run.label.is_empty() {
            String::new()
        } else if run.running {
            tr!("{model} · {got} of {n}", model = run.label, got = run.got, n = run.count)
        } else {
            run.label.clone()
        };
        let head = div()
            .flex()
            .justify_between()
            .px(px(4.))
            .text_size(px(10.5))
            .child(div().text_color(c(theme::faint())).child(tr!("ideas")))
            .child(div().id("ideas-status").text_color(c(theme::muted())).child(status).test_support());
        let waiting = if run.running { run.count.saturating_sub(run.got) } else { 0 };
        let bars = (0..waiting).map(|i| {
            let w = [120., 90., 140., 100., 130.][i % 5];
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .px(px(4.))
                .py(px(4.8))
                .child(div().w(px(10.)).flex().flex_none().justify_center().child(robot(false)))
                .child(div().w(px(w)).h(px(9.)).rounded_full().bg(ca(theme::line(), 0.9)))
        });
        let error = run.error.clone().map(|e| {
            div()
                .id("ideas-error")
                .mx(px(4.))
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(c(theme::line()))
                .text_size(px(12.))
                .text_color(c(theme::muted()))
                .child(div().text_color(c(theme::text_2())).child(e.message.clone()))
                .when(e.not_set_up || e.other_model, |d| {
                    let label = if e.not_set_up { tr!("Set up AI…") } else { tr!("Try another model…") };
                    d.child(
                        div()
                            .id("ideas-setup")
                            .mt_2()
                            .cursor_pointer()
                            .text_color(c(theme::accent()))
                            .on_click(cx.listener(|this, _, w, cx| this.open_ai_prefs(w, cx)))
                            .child(label)
                            .test_support(),
                    )
                })
                .test_support()
        });
        Some(div().flex().flex_col().gap_1().child(head).children(bars).children(error).into_any_element())
    }
}

pub fn robot(active: bool) -> Svg {
    svg()
        .path("chrome/robot.svg")
        .size(px(11.))
        .flex_none()
        .text_color(if active { c(theme::accent()) } else { c(theme::bullet()) })
}
