use std::ops::Range;
use std::time::{Duration, Instant};

use doc_model::{Carried, EditError, Place, Scope};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::overflow::NoteDrag;
use crate::theme::{self, c, ca};
use crate::workspace::Workspace;

/// Hold time over the closed Overflow's button before it opens.
pub const SPRING: Duration = Duration::from_millis(500);

/// ALT-23: a sidebar row picked up to reorder the variants below the original.
#[derive(Clone)]
pub struct VariantDrag {
    pub group: doc_model::GroupId,
    pub from: usize,
    pub preview: SharedString,
    pub words: usize,
}

impl Render for VariantDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let words = trn!("{n} word", "{n} words", self.words);
        div()
            .max_w(px(240.))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(c(theme::bg_raised()))
            .border_1()
            .border_color(ca(theme::accent(), 0.6))
            .shadow_lg()
            .font_family(theme::UI)
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(div().text_size(px(12.)).text_color(c(theme::text_2())).child(self.preview.clone()))
            .child(div().text_size(px(10.5)).text_color(c(theme::muted())).child(words))
    }
}

/// FILE-6: a detached alternative group carried from the Detached sheet to its text.
#[derive(Clone)]
pub struct DetachedDrag {
    /// Index into the model's `kept.detached`.
    pub index: usize,
    pub scope: Scope,
    pub preview: SharedString,
    pub variants: usize,
}

impl Render for DetachedDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let detail = match self.scope {
            Scope::Word => trn!("word · {n} variant", "word · {n} variants", self.variants),
            Scope::Sentence => trn!("sentence · {n} variant", "sentence · {n} variants", self.variants),
            Scope::Paragraph => trn!("paragraph · {n} variant", "paragraph · {n} variants", self.variants),
        };
        div()
            .max_w(px(240.))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(c(theme::bg_raised()))
            .border_1()
            .border_color(ca(theme::accent(), 0.6))
            .shadow_lg()
            .font_family(theme::UI)
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(div().text_size(px(12.)).text_color(c(theme::text_2())).child(self.preview.clone()))
            .child(div().text_size(px(10.5)).text_color(c(theme::muted())).child(detail))
    }
}

#[derive(Clone)]
pub struct TextDrag {
    pub range: Range<usize>,
    pub preview: SharedString,
    pub words: usize,
    /// Whole paragraphs.
    pub block: bool,
    /// A drop after the model has changed since does nothing.
    pub version: u64,
    /// GPUI keeps this point of a drag view under the pointer; the card is offset by it to sit
    /// just below the pointer.
    pub grip: Point<Pixels>,
}

impl Render for TextDrag {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.grip.x + px(14.))
            .pt(self.grip.y + px(20.))
            .child(card(self.preview.clone(), self.words, window))
    }
}

pub fn card(preview: SharedString, words: usize, window: &Window) -> Div {
    let copy = window.modifiers().alt;
    let words = trn!("{n} word", "{n} words", words);
    div()
        .max_w(px(280.))
        .px_3()
        .py_2()
        .rounded_md()
        .bg(c(theme::bg_raised()))
        .border_1()
        .border_color(ca(theme::accent(), 0.6))
        .shadow_lg()
        .font_family(theme::UI)
        .flex()
        .flex_col()
        .gap(px(3.))
        .child(div().text_size(px(12.)).text_color(c(theme::text_2())).child(preview))
        .child(
            div()
                .flex()
                .gap(px(6.))
                .text_size(px(10.5))
                .text_color(c(theme::muted()))
                .child(words)
                .child("·")
                .child(div().when(copy, |d| d.text_color(c(theme::accent()))).child(if copy { tr!("copy") } else { tr!("move") })),
        )
}

pub fn preview(text: &str) -> SharedString {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = one.chars().take(60).collect();
    if one.chars().count() > 60 {
        s.push('…');
    }
    s.into()
}

impl Workspace {
    /// Also scrolls when the pointer nears the top or bottom of the page.
    pub(crate) fn drag_over_page(&mut self, p: Point<Pixels>, bounds: Bounds<Pixels>, block: bool, cx: &mut Context<Self>) {
        let nudge = edge_nudge(p, bounds);
        self.page.update(cx, |v, cx| {
            if nudge != px(0.) {
                v.nudge_scroll(nudge, cx);
            }
            let place = if bounds.contains(&p) { v.drop_place(p, block) } else { None };
            if place != v.drop_mark {
                v.drop_mark = place;
                cx.notify();
            }
        });
    }

    /// FILE-6: a detached group over the page washes the span it would attach to.
    pub(crate) fn drag_over_detached(&mut self, p: Point<Pixels>, bounds: Bounds<Pixels>, scope: Scope, cx: &mut Context<Self>) {
        let nudge = edge_nudge(p, bounds);
        self.page.update(cx, |v, cx| {
            if nudge != px(0.) {
                v.nudge_scroll(nudge, cx);
            }
            let span = if bounds.contains(&p) { v.drop_span_at(p, scope) } else { None };
            if span != v.drop_span {
                v.drop_span = span;
                cx.notify();
            }
        });
    }

    pub(crate) fn drop_text(&mut self, d: &TextDrag, window: &mut Window, cx: &mut Context<Self>) {
        let place = self.take_place(d.block, window, cx);
        if self.model.read(cx).version != d.version {
            return;
        }
        let Some(place) = place else { return };
        let copy = window.modifiers().alt;
        let r = self.model.update(cx, |m, cx| m.edit(cx, |doc| doc.drop_text(Carried::Page(d.range.clone()), place, copy)));
        self.after_drop(r, window, cx);
    }

    pub(crate) fn drop_variant(&mut self, d: &VariantDrag, _: &mut Window, cx: &mut Context<Self>) {
        let same = self.sidebar.as_ref().is_some_and(|s| s.group == Some(d.group));
        let gap = self.sidebar.as_mut().and_then(|s| s.drop_at.take()).filter(|_| same);
        cx.notify();
        let Some(gap) = gap else { return };
        let to = if gap > d.from { gap - 1 } else { gap };
        let r = self.model.update(cx, |m, cx| m.edit(cx, |doc| doc.move_variant(d.group, d.from, to)));
        if let Err(e) = r {
            self.flash_status(e.to_string(), cx);
        }
    }

    pub(crate) fn drop_detached(&mut self, d: &DetachedDrag, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        let span = self.page.update(cx, |v, cx| {
            let shown = v.drop_span.take();
            cx.notify();
            shown.or_else(|| v.drop_span_at(at, d.scope))
        });
        let Some(range) = span else { return };
        self.reattach_detached_at(d.index, range, cx);
    }

    pub(crate) fn begin_detached_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.detached_dragging = true;
        self.close_sheet(window, cx);
    }

    pub(crate) fn finish_drags(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.has_active_drag() {
            return;
        }
        if let Some(s) = &mut self.sidebar {
            s.drop_at = None;
        }
        if !self.detached_dragging {
            return;
        }
        self.detached_dragging = false;
        self.page.update(cx, |v, cx| {
            if v.drop_span.take().is_some() {
                cx.notify();
            }
        });
        if self.sheet.is_none() && !self.model.read(cx).kept.detached.is_empty() {
            self.open_sheet(crate::sheets::Sheet::Detached, window, cx);
        }
    }

    pub(crate) fn move_unit(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.model.read(cx).layer {
            return;
        }
        let (sel, place) = {
            let p = self.page.read(cx);
            let groups: Vec<Range<usize>> = p.groups().iter().map(|g| g.range.clone()).collect();
            let sel = p.selection();
            (sel.clone(), crate::sentence_keys::move_target(p.index(), &groups, &sel, up))
        };
        let Some(place) = place else { return };
        let r = self.model.update(cx, |m, cx| m.edit(cx, |doc| doc.drop_text(Carried::Page(sel), place, false)));
        self.after_drop(r, window, cx);
    }

    pub(crate) fn move_unit_up(&mut self, _: &crate::actions::MoveUnitUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_unit(true, window, cx);
    }

    pub(crate) fn move_unit_down(&mut self, _: &crate::actions::MoveUnitDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_unit(false, window, cx);
    }

    pub fn drop_note(&mut self, drag: &NoteDrag, window: &mut Window, cx: &mut Context<Self>) {
        let place = self.take_place(drag.block, window, cx);
        let Some(place) = place else { return };
        let copy = window.modifiers().alt;
        let r = self.use_cut(drag.at.clone(), crate::drawer::ToPage::Place(place), copy, cx);
        if r.is_ok() {
            if let Some(d) = &mut self.drawer {
                d.editing = None;
                d.editor = None;
            }
        }
        self.after_drop(r, window, cx);
    }

    /// The place shown while dragging, else the one under the pointer.
    fn take_place(&mut self, block: bool, window: &Window, cx: &mut Context<Self>) -> Option<Place> {
        let pos = window.mouse_position();
        self.page.update(cx, |v, cx| {
            let shown = v.drop_mark.take();
            v.carrying = None;
            cx.notify();
            shown.or_else(|| v.drop_place(pos, block))
        })
    }

    pub(crate) fn after_drop(&mut self, r: Result<Range<usize>, EditError>, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(r) = r {
            self.text_edited(false, cx);
            self.page.update(cx, |p, cx| {
                // Catch up with the edit first, or the old selection would be mapped through it.
                p.refresh(cx);
                p.select(r.clone(), cx);
                p.flash = Some((r, Instant::now()));
            });
        }
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
        cx.notify();
    }

    pub(crate) fn drop_text_on_overflow(&mut self, d: &TextDrag, window: &mut Window, cx: &mut Context<Self>) {
        self.page.update(cx, |v, cx| {
            v.carrying = None;
            v.drop_mark = None;
            cx.notify();
        });
        if let Some(dr) = &mut self.drawer {
            dr.hot = false;
            // It took the text: it stays open to show it.
            dr.for_drag = false;
        }
        if self.model.read(cx).version != d.version {
            return;
        }
        let copy = window.modifiers().alt;
        let r = d.range.clone();
        let ok = self.stash_range(r, copy, cx);
        if ok {
            if let Some(dr) = &mut self.drawer {
                dr.editing = None;
            }
            if !copy {
                self.text_edited(false, cx);
                let at = d.range.start;
                self.page.update(cx, |p, cx| {
                    p.refresh(cx);
                    p.select(at..at, cx);
                });
            }
        }
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
        cx.notify();
    }

    pub(crate) fn spring_overflow(&mut self, e: &DragMoveEvent<TextDrag>, window: &mut Window, cx: &mut Context<Self>) {
        let over = e.bounds.contains(&e.event.position) && self.drawer.is_none();
        if !over {
            if self.spring.take().is_some() {
                cx.notify();
            }
            return;
        }
        if self.spring.is_some() {
            return;
        }
        let since = Instant::now();
        self.spring = Some(since);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SPRING).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.spring == Some(since) && cx.has_active_drag() {
                    this.spring = None;
                    this.open_overflow_for_drop(window, cx);
                }
            });
        })
        .detach();
    }
}

/// How fast the page scrolls while a drag waits near its top or bottom edge.
fn edge_nudge(p: Point<Pixels>, bounds: Bounds<Pixels>) -> Pixels {
    let top = bounds.top() + px(crate::workspace::HEADER_H);
    let band = px(56.);
    if !bounds.contains(&p) {
        px(0.)
    } else if p.y < top + band {
        -(top + band - p.y).max(px(0.)) * 0.4
    } else if p.y > bounds.bottom() - band {
        (p.y - (bounds.bottom() - band)) * 0.4
    } else {
        px(0.)
    }
}
