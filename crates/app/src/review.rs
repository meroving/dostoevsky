//! The Review tab in the left panel (mockup ai-results, B): the passes, then one pass's
//! results as a list. A row's buttons act on that row; a section's act on all its rows and say
//! how many (mockup review-settle: Settle on [the fix] [mine] · Keep both).

use ai::cost::Slot;
use ai::lab::Pass;
use ai::trim::LEVELS;
use doc_model::Scope;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trc, trn};

use crate::actions::*;
use crate::lab::{busy, AiState, Outcome, MENU_ROWS, TRIM_ROW};
use crate::review_text::{self, Names, Part};
use crate::sidebar::{keys_footer, FooterKey};
use crate::theme::{self, c, ca};
use crate::workspace::Workspace;

/// A word or two inside a button's label.
fn quoted(t: &str) -> String {
    let t = t.trim();
    if t.chars().count() > 22 {
        format!("{}…", t.chars().take(21).collect::<String>().trim_end())
    } else {
        t.to_string()
    }
}

/// How a stretch of a row's text is drawn (mockup review-snippets).
#[derive(Clone, Copy)]
enum Look {
    /// The row's own colour.
    Inherit,
    /// The page's words around the result.
    Context,
    /// A sentence or a version that is the result itself.
    Plain,
    /// Words a fix took out: struck.
    Gone,
    /// Words a fix put in: on a wash.
    Added,
    /// Gone and Added in a row that was dealt with: dim, no wash.
    GoneDim,
    AddedDim,
    /// A marked word, underlined as on the page.
    Marked,
    /// What a version put in a word's place.
    Version,
    /// What a trim would cut: faded and struck.
    Cut,
    /// A piece of a trim that stays.
    Kept,
    Original,
}

fn look(l: Look) -> Option<HighlightStyle> {
    let line = |color: Hsla| Some(UnderlineStyle { thickness: px(1.5), color: Some(color), wavy: false });
    let strike = |color: Hsla| Some(StrikethroughStyle { thickness: px(1.5), color: Some(color) });
    let s = match l {
        Look::Inherit => return None,
        Look::Context => HighlightStyle { color: Some(c(theme::muted())), ..Default::default() },
        Look::Plain => HighlightStyle { color: Some(c(theme::text())), ..Default::default() },
        Look::Gone => HighlightStyle { color: Some(c(theme::muted())), strikethrough: strike(ca(theme::danger(), 0.75)), ..Default::default() },
        Look::Added => HighlightStyle { color: Some(c(theme::text())), background_color: Some(ca(theme::accent(), 0.26)), ..Default::default() },
        Look::GoneDim => HighlightStyle { color: Some(c(theme::muted())), strikethrough: strike(ca(theme::muted(), 0.8)), ..Default::default() },
        Look::AddedDim => HighlightStyle { color: Some(c(theme::muted())), ..Default::default() },
        Look::Marked => HighlightStyle { color: Some(c(theme::text())), underline: line(ca(theme::mark(), 0.75)), ..Default::default() },
        Look::Version => HighlightStyle { color: Some(c(theme::text())), underline: line(ca(theme::accent(), 0.7)), ..Default::default() },
        Look::Cut => HighlightStyle { color: Some(ca(theme::text(), 0.38)), strikethrough: strike(ca(theme::text(), 0.4)), ..Default::default() },
        Look::Kept => HighlightStyle { color: Some(c(theme::text_2())), underline: line(ca(theme::accent(), 0.7)), ..Default::default() },
        Look::Original => HighlightStyle { color: Some(c(theme::original())), ..Default::default() },
    };
    Some(s)
}

fn styled(parts: Vec<(String, Look)>) -> StyledText {
    let mut text = String::new();
    let mut marks = Vec::new();
    for (t, l) in parts {
        let at = text.len();
        text.push_str(&t);
        if let Some(h) = look(l).filter(|_| text.len() > at) {
            marks.push((at..text.len(), h));
        }
    }
    StyledText::new(text).with_highlights(marks)
}

/// A fix's words: the same ones as they are, the gone ones struck, the new ones washed.
fn diff_parts(d: &[Part], dim: bool) -> Vec<(String, Look)> {
    let mut out = Vec::new();
    for (i, p) in d.iter().enumerate() {
        match p {
            Part::Same(t) => out.push((t.clone(), Look::Context)),
            Part::Gone(t) => out.push((t.clone(), if dim { Look::GoneDim } else { Look::Gone })),
            Part::Added(t) => {
                if i > 0 && matches!(d[i - 1], Part::Gone(_)) {
                    out.push((" ".into(), Look::Context));
                }
                out.push((t.clone(), if dim { Look::AddedDim } else { Look::Added }));
            }
        }
    }
    out
}

fn diff_text(d: &[Part], dim: bool) -> StyledText {
    styled(diff_parts(d, dim))
}

/// The row's text: wraps; two lines unless it's the current row.
fn snippet_box(text: StyledText, current: bool) -> Div {
    div()
        .text_size(px(12.5))
        .line_height(px(18.))
        .text_color(c(theme::muted()))
        .when(!current, |d| d.line_clamp(2).text_ellipsis())
        .child(text)
}

/// The row's why: the kind of result, then the model's note.
fn why(kind: Option<&str>, note: &str) -> AnyElement {
    let mut parts = Vec::new();
    if let Some(k) = kind {
        parts.push((k.to_string(), Look::Inherit));
        if !note.is_empty() {
            parts.push((" · ".into(), Look::Inherit));
        }
    }
    let k = parts.iter().map(|(t, _)| t.len()).sum::<usize>();
    parts.push((note.to_string(), Look::Inherit));
    let text: String = parts.into_iter().map(|(t, _)| t).collect();
    let dim = HighlightStyle { color: Some(ca(theme::mark(), 0.65)), ..Default::default() };
    let el = div().text_size(px(11.)).line_height(px(15.)).mt(px(4.)).text_color(c(theme::mark())).child(StyledText::new(text).with_highlights((k > 0).then_some((0..k, dim))));
    if kind.is_some() {
        el.id("mark-kind").test_support().into_any_element()
    } else {
        el.into_any_element()
    }
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id.into())
        .flex_none()
        .px(px(10.))
        .py(px(3.))
        .rounded_md()
        .border_1()
        .text_size(px(12.))
        .line_height(px(18.))
        .cursor_pointer()
        .role(Role::Button)
        .aria_label(label.clone())
        .when(primary, |d| d.border_color(c(theme::accent())).bg(ca(theme::accent(), 0.18)).text_color(c(theme::text())))
        .when(!primary, |d| d.border_color(ca(theme::muted(), 0.45)).text_color(c(theme::text_2())))
        .hover(|d| d.border_color(c(theme::accent())))
        .child(label)
}

/// A section's button: smaller than a row's, so the rows stay what you read.
fn small_button(id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool) -> Stateful<Div> {
    button(id, label, primary).px(px(7.)).py(px(1.)).text_size(px(11.)).line_height(px(16.))
}

/// The verb before the buttons that say what to settle on.
fn lead(t: &str, size: f32) -> AnyElement {
    div().flex_none().text_size(px(size)).text_color(c(theme::muted())).child(t.to_string()).into_any_element()
}

/// The words fit on one line in the panel, beside the verb (else the buttons say the fix / mine).
fn fits(cx: &App, verb: &str, labels: &[&str]) -> bool {
    let chars: usize = verb.chars().count() + labels.iter().map(|l| l.chars().count()).sum::<usize>();
    // Plex Mono at 12 px is 7.2 px a character; a button adds 22 and the gap 6; the row is the
    // panel less 41.
    chars as f32 * 7.2 + labels.len() as f32 * 28. <= crate::panels::side_width(crate::panels::side_of(crate::panels::PanelTab::Review, cx), cx) - 42.
}

fn in_quotes(t: &str) -> String {
    tr!("“{text}”", text = quoted(t))
}

fn section(id: &'static str, name: &str, n: usize) -> AnyElement {
    div()
        .id(id)
        .pt(px(10.))
        .pb(px(2.))
        .text_size(px(10.5))
        .text_color(c(theme::faint()))
        .child(format!("{} · {n}", name.to_uppercase()))
        .test_support()
        .into_any_element()
}

fn text_key(t: &'static str) -> AnyElement {
    div().child(t).into_any_element()
}

/// What kind of mark, where the pass says more than one kind.
fn kind(pass: Pass, found: ai::lab::Found) -> Option<&'static str> {
    match pass {
        Pass::Mistakes => Some(tr!("to rebuild")),
        Pass::HardToRead => Some(match found {
            ai::lab::Found::Long => tr!("long"),
            ai::lab::Found::Passive => tr!("passive"),
            _ => tr!("tangled"),
        }),
        Pass::Tone => (found == ai::lab::Found::Echo).then(|| tr!("echo")),
    }
}

impl Workspace {
    pub(crate) fn render_review(&mut self, side: crate::panels::Side, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.lab.open || !self.model.read(cx).layer {
            return None;
        }
        let switch = self.tab_strip(side, cx);
        let (body, keys) = if self.lab.showing_results() { self.review_results(window, cx) } else { self.review_passes(cx) };
        let foot = self.foot_here(side, cx).then(|| self.render_panel_foot(cx));
        let el = crate::panels::frame(div().id("review").test_support(), side, cx)
            .flex()
            .flex_col()
            .pt(px(12.))
            .px(px(20.))
            .pb(px(if foot.is_some() { crate::panel_foot::PANEL_PB } else { 20. }))
            .gap_3()
            .track_focus(&self.lab.focus)
            .key_context("Review")
            .on_action(cx.listener(Self::review_up))
            .on_action(cx.listener(Self::review_down))
            .on_action(cx.listener(Self::review_left))
            .on_action(cx.listener(Self::review_right))
            .on_action(cx.listener(Self::review_run))
            .on_action(cx.listener(Self::review_1))
            .on_action(cx.listener(Self::review_2))
            .on_action(cx.listener(Self::review_3))
            .on_action(cx.listener(Self::review_4))
            .on_action(cx.listener(Self::review_escape))
            .on_action(cx.listener(Self::review_secondary))
            .on_action(cx.listener(Self::review_keep))
            .on_action(cx.listener(Self::review_comment))
            .on_action(cx.listener(Self::trim_less_fine))
            .on_action(cx.listener(Self::trim_more_fine))
            // ⌘Z from the tab undoes on the page, as from the Alternatives list.
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.page.update(cx, |p, cx| p.undo(cx))))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.page.update(cx, |p, cx| p.redo(cx))))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, w, cx| {
                    cx.stop_propagation();
                    w.focus(&this.lab.focus, cx);
                }),
            )
            .child(crate::workspace::resize_handle(side.edge()))
            .child(switch)
            .child(body)
            .child(keys_footer(keys))
            .children(foot);
        Some(el.into_any_element())
    }

    // ---- the passes ----

    fn review_passes(&mut self, cx: &mut Context<Self>) -> (AnyElement, Vec<FooterKey>) {
        let state = AiState::of(cx);
        let deep = crate::prefs::Prefs::get(cx).ai.deep.clone();
        let cost = deep.as_ref().and_then(|m| crate::assist::models_of(m.service, cx).iter().find(|x| x.id == m.model).and_then(|x| ai::cost::per_use(x, Slot::Deep)));
        let model: SharedString = match (state, &deep, cost) {
            (AiState::Off, ..) => "".into(),
            (AiState::NoModel, ..) | (_, None, _) => tr!("no AI set up").into(),
            (_, Some(m), Some(cost)) => tr!("Deep: {model} · {cost} a pass", model = crate::assist::model_label(m, cx), cost = cost).into(),
            (_, Some(m), None) => tr!("Deep: {model}", model = crate::assist::model_label(m, cx)).into(),
        };
        let sel = !self.page.read(cx).selection().is_empty();
        let range = self.lab_range(cx);
        let words = english::word_count(self.page.read(cx).index().slice(range.clone()));
        let scope = if sel { trn!("Selection · {n} word", "Selection · {n} words", words) } else { trn!("Whole document · {n} word", "Whole document · {n} words", words) };
        let ease = self.lab_ease(cx);
        let passive = {
            let text = self.model.read(cx).doc.text();
            let body: String = text.chars().skip(range.start).take(range.len()).collect();
            prose::detect(&body).or_else(|| prose::detect(&text)).is_some_and(prose::reads_passive)
        };
        let rows = state.rows();
        let lit = self.lab.at;
        let hint = |p: Pass| -> SharedString {
            let base: SharedString = match (state, p) {
                (AiState::Off, Pass::HardToRead) if passive => tr!("Long or passive sentences").into(),
                (AiState::Off, Pass::HardToRead) => tr!("Long sentences").into(),
                (AiState::Off, Pass::Tone) => tr!("Words that echo").into(),
                _ => p.menu_hint().into(),
            };
            match ease {
                Some(e) if p == Pass::HardToRead => tr!("{hint} · ease {score} now", hint = base, score = e).into(),
                _ => base,
            }
        };
        // Only where the pass can do less than its name says.
        let limit = |row: usize| -> Option<&'static str> {
            match (state, Pass::ALL.get(row)) {
                (AiState::Ready, _) | (AiState::Off, _) => None,
                (_, Some(Pass::HardToRead)) if passive => Some(tr!("long and passive only")),
                (_, Some(Pass::HardToRead)) => Some(tr!("long ones only")),
                (_, Some(Pass::Tone)) => Some(tr!("echoes only")),
                _ => Some(tr!("needs AI")),
            }
        };
        let mut list: Vec<AnyElement> = Vec::new();
        for (n, &row) in rows.iter().enumerate() {
            let runs = state.runs(row);
            let on = row == lit && runs;
            let (name, desc): (&'static str, SharedString) = match Pass::ALL.get(row) {
                Some(&p) => (p.menu_label(), hint(p)),
                None => (tr!("Trim"), tr!("Hedges and filler first, then what repeats").into()),
            };
            let open = self.lab.open_in(row);
            let key = div()
                .flex_none()
                .mt(px(1.))
                .w(px(18.))
                .h(px(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(if on { c(theme::accent()) } else { c(theme::line()) })
                .text_size(px(10.))
                .text_color(if on { c(theme::accent()) } else { c(theme::faint()) })
                .child((n + 1).to_string());
            let chips = (row == TRIM_ROW && runs).then(|| {
                let chosen = self.lab.trim_pc;
                div().flex().mt(px(7.)).child(
                    div().flex().rounded(px(6.)).border_1().border_color(c(theme::line())).overflow_hidden().children(LEVELS.iter().enumerate().skip(1).map(|(i, &l)| {
                        let lit = l == chosen;
                        div()
                            .id(("lab-trim", i))
                            .px(px(8.))
                            .py(px(2.))
                            .text_size(px(11.))
                            .cursor_pointer()
                            .when(i > 1, |d| d.border_l_1().border_color(c(theme::line())))
                            .when(lit, |d| d.bg(ca(theme::accent(), 0.2)).text_color(c(theme::text())))
                            .when(!lit, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text()))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.lab.trim_pc = l;
                                this.lab.at = TRIM_ROW;
                                cx.notify();
                            }))
                            .child(format!("−{l}"))
                            .test_support()
                    })),
                )
            });
            let run = on.then(|| {
                div().flex().mt(px(9.)).child(
                    button("lab-run", tr!("Run"), true)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.run_row(row, cx);
                        }))
                        .test_support(),
                )
            });
            let id: ElementId = if row == TRIM_ROW { "lab-trim-row".into() } else { ElementId::NamedInteger("lab-pass".into(), row as u64) };
            list.push(
                div()
                    .id(id)
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .mx(px(-8.))
                    .px(px(8.))
                    .py(px(9.))
                    .rounded_md()
                    .when(on, |d| d.bg(ca(theme::accent(), 0.11)))
                    .when(runs && !on, |d| d.cursor_pointer().hover(|d| d.bg(ca(theme::line(), 0.6))))
                    .when(runs, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            this.lab.at = row;
                            cx.notify();
                        }))
                    })
                    .child(key)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(13.)).text_color(c(theme::text())).when(!runs, |d| d.opacity(0.4)).child(name))
                            .child(div().text_size(px(11.)).line_height(px(15.)).mt(px(2.)).text_color(c(theme::muted())).child(desc))
                            .children(limit(row).map(|t| div().text_size(px(11.)).line_height(px(15.)).text_color(c(theme::faint())).child(t)))
                            .children(chips)
                            .children(run),
                    )
                    .when(open > 0, |d| {
                        d.child(
                            div()
                                .id(("lab-open", row))
                                .flex_none()
                                .mt(px(1.))
                                .text_size(px(11.5))
                                .text_color(c(theme::accent()))
                                .cursor_pointer()
                                .hover(|d| d.underline())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.open_results(row, cx);
                                }))
                                .child(trn!("{n} open ›", "{n} open ›", open))
                                .test_support(),
                        )
                    })
                    .test_support()
                    .into_any_element(),
            );
        }
        let head = div()
            .flex()
            .flex_col()
            .flex_none()
            .child(div().font_family(theme::hand()).text_size(theme::hand_px(24.)).text_color(c(theme::accent())).child(trc!("pass menu", "Review")))
            .child(div().id("lab-scope").text_size(px(11.5)).text_color(if sel { c(theme::accent()) } else { c(theme::text_2()) }).child(scope).test_support())
            .when(!model.is_empty(), |d| d.child(div().text_size(px(11.)).text_color(c(theme::faint())).child(model)));
        let body = div()
            .id("review-passes")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(head)
            .child(div().id("review-pass-list").flex().flex_col().flex_1().min_h_0().overflow_y_scroll().children(list))
            .when(state == AiState::NoModel, |d| {
                d.child(
                    div()
                        .id("lab-ai-settings")
                        .flex_none()
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .text_color(c(theme::accent()))
                        .hover(|d| d.underline())
                        .on_click(cx.listener(|this, _, w, cx| this.open_ai_prefs(w, cx)))
                        .child(tr!("AI settings…"))
                        .test_support(),
                )
            })
            .test_support()
            .into_any_element();
        let numbers = if rows.len() == 2 { "1–2" } else { "1–4" };
        let keys = vec![("↑↓", text_key(tr!("choose")), false), ("↩", text_key(tr!("run")), false), (numbers, text_key(tr!("run that one")), false)];
        (body, keys)
    }

    pub(crate) fn run_row(&mut self, row: usize, cx: &mut Context<Self>) {
        if !AiState::of(cx).runs(row) || row >= MENU_ROWS {
            return;
        }
        self.lab.at = row;
        match Pass::ALL.get(row) {
            Some(&p) => self.run_pass(p, cx),
            None => self.trim_to(self.lab.trim_pc, cx),
        }
    }

    /// A pass's results to the front, as they were left.
    pub(crate) fn open_results(&mut self, row: usize, cx: &mut Context<Self>) {
        self.lab.front(row, false);
        self.lab.results = true;
        self.lab.reveal = true;
        if self.lab.trim.is_some() {
            self.centre_trim_cut(cx);
        } else if self.lab.at_mark() {
            self.centre_mark(cx);
        } else {
            self.centre_made(cx);
        }
        self.sync_lab(cx);
    }

    // ---- one pass's results ----

    fn review_results(&mut self, window: &mut Window, cx: &mut Context<Self>) -> (AnyElement, Vec<FooterKey>) {
        let row = self.lab.active.unwrap_or(0);
        let pass = Pass::ALL.get(row).copied();
        let title: &'static str = pass.map_or(tr!("Trim"), |p| p.menu_label());
        let back = div()
            .id("review-back")
            .flex()
            .flex_none()
            .gap(px(6.))
            .text_size(px(11.5))
            .cursor_pointer()
            .text_color(c(theme::text_2()))
            .hover(|d| d.text_color(c(theme::text())))
            .on_click(cx.listener(|this, _, _, cx| {
                this.lab.results = false;
                cx.notify();
            }))
            .child(div().text_color(c(theme::accent())).child("‹"))
            .child(trc!("pass menu", "Review"))
            .test_support();

        // What's in front, said once.
        let mut sum: Vec<String> = Vec::new();
        if let Some(ty) = &self.lab.made {
            let (fixed, versions) = (ty.fixed, ty.groups.len() - ty.fixed);
            if fixed > 0 {
                sum.push(trn!("{n} fixed", "{n} fixed", fixed));
            }
            if versions > 0 {
                sum.push(if ty.pass.word_level() {
                    trn!("{n} word with alternatives", "{n} words with alternatives", versions)
                } else {
                    trn!("{n} sentence with versions", "{n} sentences with versions", versions)
                });
            }
        }
        if let Some(m) = &self.lab.marks {
            let k = m.items.len();
            sum.push(if m.pass == Pass::Mistakes { trn!("{n} to rebuild", "{n} to rebuild", k) } else { trn!("{n} marked", "{n} marked", k) });
        }
        if let Some(t) = &self.lab.trim {
            let showing = t.words_cut();
            let after = t.total.saturating_sub(showing);
            let pc = (showing * 100 + t.total / 2).checked_div(t.total).unwrap_or(0);
            sum.push(trn!("{total} → {n} word · −{pc}%", "{total} → {n} words · −{pc}%", after, total = t.total, pc = pc));
            if !t.kept.is_empty() {
                sum.push(trn!("{n} kept", "{n} kept", t.kept.len()));
            }
        }
        let running = self.lab.running.as_ref().map(|r| (r.label.clone(), r.model.clone()));
        let error = self.lab.error.clone();
        let summary = match (&running, &error) {
            (Some((label, model)), _) => div()
                .id("review-sum")
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(11.5))
                .text_color(c(theme::muted()))
                .child(busy("review-busy", 11., cx))
                .child(div().min_w_0().flex_1().child(format!("{label} · {model}")))
                .test_support(),
            (None, Some((msg, _))) => div().id("review-sum").text_size(px(11.5)).text_color(c(theme::danger())).child(msg.clone()).test_support(),
            _ => div().id("review-sum").text_size(px(11.5)).text_color(c(theme::muted())).child(sum.join(" · ")).test_support(),
        };

        // The head's buttons: all of them, with the count.
        let mut all: Vec<AnyElement> = Vec::new();
        if running.is_some() {
            all.push(button("review-stop", tr!("Stop"), false).on_click(cx.listener(|this, _, _, cx| this.stop_lab_request(cx))).test_support().into_any_element());
        } else if let Some((_, setup)) = &error {
            if *setup {
                all.push(button("lab-setup", tr!("AI settings…"), true).on_click(cx.listener(|this, _, w, cx| this.open_ai_prefs(w, cx))).test_support().into_any_element());
            }
            all.push(button("review-retry", tr!("Try again"), !*setup).on_click(cx.listener(move |this, _, _, cx| this.run_row(row, cx))).test_support().into_any_element());
        } else {
            if let Some(t) = &self.lab.trim {
                let n = t.words_cut();
                if n > 0 {
                    all.push(button("cut-words", trn!("Cut {n} word", "Cut {n} words", n), true).on_click(cx.listener(|this, _, _, cx| this.make_cuts(cx))).test_support().into_any_element());
                }
            }
        }
        let slider = self.lab.trim.as_ref().map(|t| {
            let at = t.percent;
            Self::trim_slider(at, !t.yours(), cx)
        });
        let head = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(2.))
            .child(back)
            .child(div().id("review-title").mt(px(6.)).font_family(theme::hand()).text_size(theme::hand_px(24.)).text_color(c(theme::accent())).child(title).test_support())
            .child(summary)
            .children(slider.map(|s| div().mt(px(6.)).child(s)))
            .when(!all.is_empty(), |d| d.child(div().id("review-all").mt(px(8.)).flex().flex_wrap().gap(px(6.)).children(all).test_support()));

        let (rows, reveal_at, keys) = if self.lab.trim.is_some() { self.review_cut_rows(cx) } else { self.review_item_rows(window, cx) };
        if std::mem::take(&mut self.lab.reveal) {
            if let Some(i) = reveal_at {
                self.lab.scroll.scroll_to_item(i);
            }
        }
        let list = div().id("review-list").flex().flex_col().flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.lab.scroll).children(rows).test_support();
        // Close: the text and its groups stay as they are; the list and the marks go.
        let clear = div().flex().flex_none().pt(px(4.)).child(
            button("review-close", tr!("Close"), false)
                .on_click(cx.listener(|this, _, _, cx| this.lab_clear(cx)))
                .test_support(),
        );
        let body = div().id("review-results").flex().flex_col().flex_1().min_h_0().gap_2().child(head).child(div().h(px(1.)).flex_none().bg(c(theme::line()))).child(list).child(clear).test_support().into_any_element();
        (body, keys)
    }

    /// The groups, then the marks, then what was dealt with; the index of the current row.
    fn review_item_rows(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> (Vec<AnyElement>, Option<usize>, Vec<FooterKey>) {
        let state = AiState::of(cx);
        let on_mark = self.lab.at_mark();
        let idx = self.page.read(cx).index().clone();
        let doc = self.model.read(cx).doc.clone_groups_view();
        let mut out: Vec<AnyElement> = Vec::new();
        let mut current_row = None;
        let mut keys: Vec<FooterKey> = vec![("↑↓", text_key(tr!("go to")), false)];
        let dealt = |mark: bool, out: &mut Vec<AnyElement>, decided: &[crate::lab::Decided]| {
            let mut d: Vec<(usize, &crate::lab::Decided)> = decided.iter().enumerate().filter(|(_, d)| d.mark == mark).collect();
            d.sort_by_key(|(_, d)| d.at);
            for (i, x) in d {
                let (word, colour) = match x.outcome {
                    Outcome::Settled => (tr!("settled"), c(theme::accent())),
                    Outcome::Mine => (tr!("mine"), c(theme::accent())),
                    // Amber: its alternatives are still on the page.
                    Outcome::Kept => (tr!("kept"), c(theme::mark())),
                    Outcome::Comment => (tr!("comment"), c(theme::accent())),
                    Outcome::Dismissed => (tr!("dismissed"), c(theme::faint())),
                };
                let what = match &x.diff {
                    Some((was, now)) => diff_text(&review_text::word_diff(was, now), true),
                    None => styled(vec![(x.label.clone(), Look::Context)]),
                };
                out.push(
                    div()
                        .id(("review-done", i))
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .py(px(5.))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(c(theme::muted())).child(what))
                        .child(div().flex_none().text_size(px(10.5)).text_color(colour).child(word))
                        .test_support()
                        .into_any_element(),
                );
            }
        };
        let decided = self.lab.decided.clone();

        let changes_done = decided.iter().filter(|d| !d.mark).count();
        if self.lab.made.is_none() && changes_done > 0 {
            out.push(section("review-changes", tr!("Changes"), changes_done));
            dealt(false, &mut out, &decided);
        }
        if let Some(made) = &self.lab.made {
            let n = made.groups.len() + changes_done;
            out.push(section("review-changes", tr!("Changes"), n));
            if made.groups.len() > 1 {
                let keep = self.lab_keepable_groups(cx).len();
                let on = if made.fixed == made.groups.len() { tr!("fixes") } else { tr!("chosen") };
                out.push(
                    div()
                        .id("review-changes-all")
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(5.))
                        .pb(px(4.))
                        .child(lead(tr!("Settle all on"), 11.))
                        .child(small_button("settle-all", on, false).on_click(cx.listener(|this, _, _, cx| this.settle_all(false, cx))).test_support())
                        .child(small_button("settle-all-mine", tr!("mine"), false).on_click(cx.listener(|this, _, _, cx| this.settle_all(true, cx))).test_support())
                        .when(keep > 1, |d| {
                            d.child(div().w(px(4.))).child(small_button("keep-all", trn!("Keep all {n}", "Keep all {n}", keep), false).on_click(cx.listener(|this, _, _, cx| {
                                let ids = this.lab_keepable_groups(cx);
                                this.keep_lab_groups(&ids, cx);
                            })).test_support())
                        })
                        .test_support()
                        .into_any_element(),
                );
            }
            for (i, &g) in made.groups.iter().enumerate() {
                let Some((info, range)) = doc.iter().find(|(info, _)| info.id == g).cloned() else { continue };
                let fix = i < made.fixed;
                let current = !on_mark && made.current == i;
                if current {
                    current_row = Some(out.len());
                }
                let word = info.scope == Scope::Word;
                let shown = info.variants.get(info.active).map(|v| v.text.clone()).unwrap_or_default();
                let orig = info.variants.first().map(|v| v.text.clone()).unwrap_or_default();
                let to = info.variants.get(1).map(|v| v.text.clone()).unwrap_or_default();
                let diff = fix.then(|| review_text::word_diff(&orig, &to));
                // The change in its sentence; a version of a whole sentence is the snippet itself,
                // and on the current row the list of versions says it.
                let text = if let Some(d) = &diff {
                    let (b, a) = review_text::around(&idx, range.clone(), review_text::BEFORE, review_text::AFTER);
                    let mut parts = vec![(b, Look::Context)];
                    parts.extend(diff_parts(d, false));
                    parts.push((a, Look::Context));
                    Some(styled(parts))
                } else if word {
                    let (b, a) = review_text::around(&idx, range.clone(), review_text::BEFORE, review_text::AFTER);
                    Some(styled(vec![(b, Look::Context), (shown.clone(), Look::Version), (a, Look::Context)]))
                } else {
                    (!current).then(|| styled(vec![(shown.clone(), Look::Plain)]))
                };
                let note = made.note_for(g).filter(|n| !n.trim().is_empty()).map(str::to_string);
                let gid = g;
                let mut row = div()
                    .id(("review-row", i))
                    .flex()
                    .flex_col()
                    .mx(px(-8.))
                    .px(px(8.))
                    .py(px(7.))
                    .rounded_md()
                    .cursor_pointer()
                    .aria_selected(current)
                    .when(current, |d| d.bg(ca(theme::accent(), 0.11)))
                    .when(!current, |d| d.hover(|d| d.bg(ca(theme::line(), 0.6))))
                    .on_click(cx.listener(move |this, _, _, cx| this.lab_go_to(i, cx)))
                    .children(text.map(|t| snippet_box(t, current)))
                    .children(note.map(|n| why(None, &n)));
                if current {
                    if !fix {
                        row = row.child(div().flex().flex_col().gap(px(5.)).mt(px(7.)).children(info.variants.iter().enumerate().map(|(k, v)| {
                            let on = k == info.active;
                            let mut parts = vec![(v.text.clone(), Look::Inherit)];
                            if k == 0 {
                                parts.push(("  ".into(), Look::Inherit));
                                parts.push((tr!("original").to_string(), Look::Original));
                            }
                            div()
                                .id(("row-variant", k))
                                .flex()
                                .items_start()
                                .gap(px(7.))
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .cursor_pointer()
                                .text_color(if on { c(theme::text()) } else { c(theme::text_2()) })
                                .hover(|d| d.text_color(c(theme::text())))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.model.update(cx, |m, cx| m.set_active(cx, gid, k));
                                    this.sync_lab(cx);
                                }))
                                .child(div().flex_none().w(px(10.)).text_color(if on { c(theme::accent()) } else { c(theme::muted()) }).child(if on { "●" } else { "○" }))
                                .child(div().flex_1().min_w_0().child(styled(parts)))
                                .test_support()
                        })));
                    }
                    // Settle on [the fix] [mine] · Keep both: the words when they fit.
                    let verb = tr!("Settle on");
                    let generic = || if fix { (tr!("the fix").to_string(), tr!("mine").to_string()) } else { (tr!("this").to_string(), tr!("mine").to_string()) };
                    let (this_one, mine) = match &diff {
                        Some(d) => match review_text::names(d) {
                            Names::Replace { gone, added } => (in_quotes(&added), in_quotes(&gone)),
                            Names::Remove(gone) => (tr!("no “{text}”", text = quoted(&gone)), in_quotes(&gone)),
                            Names::Add(added) => (in_quotes(&added), tr!("no “{text}”", text = quoted(&added))),
                            Names::Whole => generic(),
                        },
                        None if word => (in_quotes(&shown), in_quotes(&orig)),
                        None => generic(),
                    };
                    let (this_one, mine) = if fits(cx, verb, &[&this_one, &mine]) { (this_one, mine) } else { generic() };
                    let can_settle = fix || info.active != 0;
                    let keepable = self.lab_keepable(gid, cx);
                    let keep_label = if info.variants.len() == 2 { tr!("Keep both").to_string() } else { trn!("Keep all {n}", "Keep all {n}", info.variants.len()) };
                    let use_at = fix.then_some(1);
                    row = row.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .mt(px(8.))
                            .child(lead(verb, 12.))
                            .when(can_settle, |d| {
                                d.child(button("row-settle", this_one, fix || !keepable).on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.settle_lab_group(gid, use_at, cx);
                                })).test_support())
                            })
                            .child(button("row-mine", mine, !can_settle && !keepable).on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.settle_lab_group(gid, Some(0), cx);
                            })).test_support())
                            .when(keepable, |d| {
                                d.child(div().w(px(4.))).child(button("row-keep", keep_label, !fix).on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.keep_lab_groups(&[gid], cx);
                                })).test_support())
                            }),
                    );
                    if fix {
                        keys.push(("↩", text_key(tr!("settle")), false));
                        keys.push(("⌫", text_key(tr!("mine")), false));
                        if keepable {
                            keys.push(("K", text_key(tr!("keep")), false));
                        }
                    } else {
                        keys.push(("↩", text_key(tr!("keep")), false));
                        keys.push(("⌫", text_key(tr!("mine")), false));
                        keys.push(("⌥↑↓", text_key(tr!("try a version")), false));
                    }
                }
                out.push(row.test_support().into_any_element());
            }
            dealt(false, &mut out, &decided);
        }

        if let Some(m) = &self.lab.marks {
            let g = self.lab.made.as_ref().map_or(0, |t| t.groups.len());
            let n = m.items.len() + decided.iter().filter(|d| d.mark).count();
            out.push(section("review-marked", if m.pass == Pass::Mistakes { tr!("To rebuild") } else { tr!("Marked") }, n));
            let word = m.pass.word_level();
            if m.items.len() > 1 {
                let k = m.items.len();
                let suggest = if word { trn!("Suggest alternatives for all {n}", "Suggest alternatives for all {n}", k) } else { trn!("Suggest versions for all {n}", "Suggest versions for all {n}", k) };
                out.push(
                    div()
                        .id("review-marked-all")
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(5.))
                        .pb(px(4.))
                        .when(state == AiState::Ready, |d| d.child(small_button("suggest-all", suggest, false).on_click(cx.listener(|this, _, _, cx| this.suggest_for_marks(None, cx))).test_support()))
                        .child(small_button("dismiss-all", tr!("Dismiss all"), false).on_click(cx.listener(|this, _, _, cx| this.dismiss_all(cx))).test_support())
                        .test_support()
                        .into_any_element(),
                );
            }
            for (j, k) in m.items.iter().enumerate() {
                let current = on_mark && m.current == j;
                if current {
                    current_row = Some(out.len());
                }
                // To rebuild says so in its section.
                let tag = kind(m.pass, k.found).filter(|_| m.pass != Pass::Mistakes);
                // A passive mark's note starts "Passive: …"; the kind already says it.
                let note = match k.note.trim().split_once(':') {
                    Some((_, rest)) if k.found == ai::lab::Found::Passive => {
                        let rest = rest.trim();
                        let mut cs = rest.chars();
                        cs.next().map(|f| f.to_lowercase().chain(cs).collect::<String>()).unwrap_or_default()
                    }
                    _ => k.note.trim().to_string(),
                };
                // A word in its sentence, underlined as on the page; a sentence on its own.
                let text = if word {
                    let range = review_text::tight(&idx, k.range.clone());
                    let (b, a) = review_text::around(&idx, range.clone(), review_text::BEFORE, review_text::AFTER);
                    let quote = idx.slice(range).replace('\n', " ");
                    styled(vec![(b, Look::Context), (quote, Look::Marked), (a, Look::Context)])
                } else {
                    styled(vec![(k.quote.trim().to_string(), Look::Plain)])
                };
                let has_note = !k.note.trim().is_empty();
                let mut row = div()
                    .id(("review-row", g + j))
                    .flex()
                    .flex_col()
                    .mx(px(-8.))
                    .px(px(8.))
                    .py(px(7.))
                    .rounded_md()
                    .cursor_pointer()
                    .aria_selected(current)
                    .when(current, |d| d.bg(ca(theme::accent(), 0.11)))
                    .when(!current, |d| d.hover(|d| d.bg(ca(theme::line(), 0.6))))
                    .on_click(cx.listener(move |this, _, _, cx| this.lab_go_to(g + j, cx)))
                    .child(snippet_box(text, current))
                    .when(has_note || tag.is_some(), |d| d.child(why(tag, &note)));
                if current {
                    let suggest = if word { tr!("Suggest alternatives") } else { tr!("Suggest versions") };
                    row = row.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .mt(px(8.))
                            .map(|d| match state {
                                AiState::Ready => d.child(button("row-suggest", suggest, true).on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.suggest_for_marks(Some(j), cx);
                                })).test_support()),
                                AiState::NoModel => d.child(div().opacity(0.4).child(button("row-suggest-off", suggest, false).test_support())),
                                AiState::Off => d,
                            })
                            .when(has_note, |d| {
                                d.child(button("row-comment", tr!("Make a comment"), false).on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.keep_mark_as_comment(cx);
                                })).test_support())
                            })
                            .child(button("row-dismiss", tr!("Dismiss"), false).on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.dismiss_mark(cx);
                            })).test_support()),
                    );
                    if state == AiState::Ready {
                        keys.push(("↩", text_key(tr!("suggest")), false));
                    }
                    keys.push(("⌫", text_key(tr!("dismiss")), false));
                    if has_note {
                        keys.push(("C", text_key(tr!("comment")), false));
                    }
                }
                out.push(row.test_support().into_any_element());
            }
            dealt(true, &mut out, &decided);
        } else if decided.iter().any(|d| d.mark) {
            out.push(section("review-marked", tr!("Marked"), decided.iter().filter(|d| d.mark).count()));
            dealt(true, &mut out, &decided);
        }
        keys.push(("esc", text_key(tr!("back")), false));
        (out, current_row, keys)
    }

    fn review_cut_rows(&mut self, cx: &mut Context<Self>) -> (Vec<AnyElement>, Option<usize>, Vec<FooterKey>) {
        let Some(t) = &self.lab.trim else { return (Vec::new(), None, Vec::new()) };
        let list = t.list();
        let at = t.at.min(list.len().saturating_sub(1));
        // What would go, faded and struck in its sentence as on the page; a kept piece underlined.
        let texts: Vec<(StyledText, String, bool)> = {
            let page = self.page.read(cx);
            let idx = page.index();
            list.iter()
                .map(|&i| {
                    let cut = &t.cuts[i];
                    let kept = t.kept.contains(&i);
                    let range = review_text::tight(idx, cut.range.clone());
                    let (b, a) = review_text::around(idx, range.clone(), review_text::BEFORE, review_text::AFTER);
                    let quote = idx.slice(range).replace('\n', " ");
                    let text = styled(vec![(b, Look::Context), (quote, if kept { Look::Kept } else { Look::Cut }), (a, Look::Context)]);
                    let count = if kept { tr!("kept").to_string() } else { trn!("−{n} word", "−{n} words", cut.words) };
                    (text, count, kept)
                })
                .collect()
        };
        let mut out: Vec<AnyElement> = vec![section("review-would-go", tr!("Would go"), list.len())];
        let mut current_row = None;
        for ((n, &i), (text, count, kept)) in list.iter().enumerate().zip(texts) {
            let current = n == at;
            if current {
                current_row = Some(out.len());
            }
            out.push(
                div()
                    .id(("review-cut", n))
                    .flex()
                    .flex_col()
                    .mx(px(-8.))
                    .px(px(8.))
                    .py(px(7.))
                    .rounded_md()
                    .cursor_pointer()
                    .aria_selected(current)
                    .when(current, |d| d.bg(ca(theme::accent(), 0.11)))
                    .when(!current, |d| d.hover(|d| d.bg(ca(theme::line(), 0.6))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(t) = &mut this.lab.trim {
                            t.at = n;
                        }
                        this.centre_trim_cut(cx);
                    }))
                    .child(snippet_box(text, current))
                    .child(div().text_size(px(11.)).line_height(px(15.)).mt(px(3.)).text_color(if kept { c(theme::accent()) } else { c(theme::mark()) }).child(count))
                    .when(current, |d| {
                        d.child(div().flex().mt(px(8.)).child(button("cut-keep", if kept { tr!("Cut it") } else { tr!("Keep it") }, false).on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_keep_cut(i, cx);
                        })).test_support()))
                    })
                    .test_support()
                    .into_any_element(),
            );
        }
        let keys = vec![("↑↓", text_key(tr!("go to")), false), ("K", text_key(tr!("keep or cut")), false), ("← →", text_key(tr!("amount")), false), ("esc", text_key(tr!("back")), false)];
        (out, current_row, keys)
    }

    pub(crate) fn toggle_keep_cut(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(t) = &mut self.lab.trim else { return };
        if !t.kept.remove(&i) {
            t.kept.insert(i);
        }
        if let Some(n) = t.list().iter().position(|&x| x == i) {
            t.at = n;
        }
        self.sync_lab(cx);
    }

    // ---- keys ----

    fn pass_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let state = AiState::of(cx);
        let rows: Vec<usize> = state.rows().into_iter().filter(|&r| state.runs(r)).collect();
        if rows.is_empty() {
            return;
        }
        let at = rows.iter().position(|&r| r == self.lab.at).unwrap_or(0);
        let n = rows.len();
        self.lab.at = rows[if down { (at + 1) % n } else { (at + n - 1) % n }];
        cx.notify();
    }

    fn review_up(&mut self, _: &LabMenuUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.lab.showing_results() {
            self.lab.reveal = true;
            self.step_mark(-1, cx);
        } else {
            self.pass_step(false, cx);
        }
    }

    fn review_down(&mut self, _: &LabMenuDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.lab.showing_results() {
            self.lab.reveal = true;
            self.step_mark(1, cx);
        } else {
            self.pass_step(true, cx);
        }
    }

    fn review_side(&mut self, d: isize, cx: &mut Context<Self>) {
        if self.lab.showing_results() {
            if self.lab.trim.is_some() {
                self.nudge_trim(d, false, cx);
            }
            return;
        }
        if self.lab.at != TRIM_ROW || !AiState::of(cx).runs(TRIM_ROW) {
            return;
        }
        let levels = &LEVELS[1..];
        let at = levels.iter().position(|&l| l == self.lab.trim_pc).unwrap_or(0) as isize;
        self.lab.trim_pc = levels[(at + d).clamp(0, levels.len() as isize - 1) as usize];
        cx.notify();
    }

    fn review_left(&mut self, _: &LabMenuLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.review_side(-1, cx);
    }

    fn review_right(&mut self, _: &LabMenuRight, _: &mut Window, cx: &mut Context<Self>) {
        self.review_side(1, cx);
    }

    /// ↩: runs the chosen pass, or does the current result's first thing.
    fn review_run(&mut self, _: &LabMenuRun, _: &mut Window, cx: &mut Context<Self>) {
        if !self.lab.showing_results() {
            return self.run_row(self.lab.at, cx);
        }
        if self.lab.running.is_some() || self.lab.trim.is_some() {
            return;
        }
        if self.lab.at_mark() {
            if AiState::of(cx) == AiState::Ready {
                let j = self.lab.marks.as_ref().map_or(0, |m| m.current);
                self.suggest_for_marks(Some(j), cx);
            }
            return;
        }
        let Some((g, fix)) = self.lab.made.as_ref().and_then(|t| t.groups.get(t.current).map(|g| (*g, t.current < t.fixed))) else { return };
        // A fix settles; versions stay with their alternatives (mockup review-settle).
        if fix {
            self.settle_lab_group(g, Some(1), cx);
        } else {
            self.keep_lab_groups(&[g], cx);
        }
    }

    /// ⌫: the current result's second thing: settle on mine, or dismiss.
    fn review_secondary(&mut self, _: &ReviewSecondary, _: &mut Window, cx: &mut Context<Self>) {
        if !self.lab.showing_results() || self.lab.running.is_some() || self.lab.trim.is_some() {
            return;
        }
        if self.lab.at_mark() {
            return self.dismiss_mark(cx);
        }
        if let Some(g) = self.lab.made.as_ref().and_then(|t| t.groups.get(t.current).copied()) {
            self.settle_lab_group(g, Some(0), cx);
        }
    }

    /// K: keep or cut a trim's piece; keep a change with its alternatives.
    fn review_keep(&mut self, _: &ReviewKeep, _: &mut Window, cx: &mut Context<Self>) {
        if !self.lab.showing_results() || self.lab.running.is_some() {
            return;
        }
        if let Some(i) = self.lab.trim.as_ref().and_then(|t| t.current()) {
            return self.toggle_keep_cut(i, cx);
        }
        if self.lab.at_mark() {
            return;
        }
        if let Some(g) = self.lab.made.as_ref().and_then(|t| t.groups.get(t.current).copied()) {
            self.keep_lab_groups(&[g], cx);
        }
    }

    fn review_comment(&mut self, _: &ReviewComment, _: &mut Window, cx: &mut Context<Self>) {
        if self.lab.showing_results() && self.lab.at_mark() {
            self.keep_mark_as_comment(cx);
        }
    }

    fn review_number(&mut self, n: usize, cx: &mut Context<Self>) {
        if self.lab.showing_results() {
            return;
        }
        if let Some(&row) = AiState::of(cx).rows().get(n - 1) {
            self.run_row(row, cx);
        }
    }

    fn review_1(&mut self, _: &LabMenu1, _: &mut Window, cx: &mut Context<Self>) {
        self.review_number(1, cx);
    }

    fn review_2(&mut self, _: &LabMenu2, _: &mut Window, cx: &mut Context<Self>) {
        self.review_number(2, cx);
    }

    fn review_3(&mut self, _: &LabMenu3, _: &mut Window, cx: &mut Context<Self>) {
        self.review_number(3, cx);
    }

    fn review_4(&mut self, _: &LabMenu4, _: &mut Window, cx: &mut Context<Self>) {
        self.review_number(4, cx);
    }

    /// Stops a request, then goes back to the passes, then closes the tab.
    fn review_escape(&mut self, _: &LabEscape, window: &mut Window, cx: &mut Context<Self>) {
        if self.lab.running.is_some() {
            return self.stop_lab_request(cx);
        }
        if self.lab.showing_results() {
            self.lab.results = false;
            cx.notify();
            return;
        }
        self.close_review(window, cx);
    }
}

/// So `review_item_rows` can read groups while it borrows `self.lab`.
trait GroupsView {
    fn clone_groups_view(&self) -> Vec<(doc_model::GroupInfo, std::ops::Range<usize>)>;
}

impl GroupsView for doc_model::Document {
    fn clone_groups_view(&self) -> Vec<(doc_model::GroupInfo, std::ops::Range<usize>)> {
        self.visible_groups().into_iter().filter_map(|v| Some((self.group(v.id)?, self.group_range(v.id)?))).collect()
    }
}

