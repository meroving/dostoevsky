use std::collections::HashMap;
use std::ops::Range;

use doc_model::{Carried, CutId, CutPlace, EditError, Place};
use gpui_kit::*;
use l10n::tr;

use crate::actions::GhostToggle;
use crate::workspace::Workspace;

/// The cut esc would put back: made just now, nothing edited or moved since.
#[derive(Clone, Debug)]
pub struct LastCut {
    pub id: CutId,
    pub uid: u64,
    pub version: u64,
    pub sel: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceOp {
    PutBack,
    ShowText,
    ShowMark,
    ShowInCuts,
    Forget,
    Delete,
}

/// Undo steps spanning two chapters, by link: the two documents' uids.
#[derive(Default)]
pub struct CrossLinks(pub HashMap<u64, (u64, u64)>);

impl Global for CrossLinks {}

pub fn is_cross(link: u64, cx: &App) -> bool {
    cx.try_global::<CrossLinks>().is_some_and(|c| c.0.contains_key(&link))
}

/// Between words; whole paragraphs go before the paragraph the caret starts, else after its own.
pub fn place_at(text: &[char], at: usize, block: bool) -> Place {
    if !block {
        return Place::Inline(at);
    }
    let at = at.min(text.len());
    let line_start = text[..at].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
    if text[line_start..at].iter().all(|c| c.is_whitespace()) {
        return Place::Block(line_start);
    }
    let mut i = at;
    while i < text.len() {
        if text[i] == '\n' && text.get(i + 1) == Some(&'\n') {
            let next = text[i..].iter().position(|c| !c.is_whitespace()).map(|k| i + k);
            return Place::Block(next.unwrap_or(text.len()));
        }
        i += 1;
    }
    Place::Block(text.len())
}

impl Workspace {
    pub(crate) fn on_cut(&mut self, id: CutId, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        self.last_cut = Some(LastCut { id, uid: m.uid, version: m.version, sel: self.page.read(cx).selection() });
        self.hold_status(tr!("Cut · {paste} moves it · esc puts it back", paste = crate::os_text::key("⌘V")), cx);
    }

    pub(crate) fn esc_puts_back(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(last) = self.last_cut.take() else { return false };
        let m = self.model.read(cx);
        if m.uid != last.uid || m.version != last.version || self.page.read(cx).selection() != last.sel {
            return false;
        }
        self.put_back_cut(last.id, cx);
        true
    }

    pub(crate) fn put_back_cut(&mut self, id: CutId, cx: &mut Context<Self>) -> bool {
        let shown = self.model.read(cx).doc.cut_info(id).is_some_and(|c| matches!(c.place, CutPlace::Page { shown: true, .. }));
        let r = self.model.update(cx, |m, cx| m.edit(cx, |d| d.put_back(id)));
        let Ok(r) = r else { return false };
        if let Some(r) = r {
            self.page.update(cx, |p, cx| {
                p.refresh(cx);
                if shown {
                    p.fade_ghost(r.clone(), false, cx);
                } else {
                    p.flash = Some((r, std::time::Instant::now()));
                }
            });
        }
        self.clear_held_status(cx);
        cx.notify();
        true
    }

    /// The cut the caret or the whole selection is in: ⌘/ puts it back.
    pub(crate) fn selection_in_cut(&self, cx: &App) -> Option<doc_model::CutId> {
        let sel = self.page.read(cx).selection();
        let doc = &self.model.read(cx).doc;
        doc.cut_at(sel.start).filter(|id| sel.is_empty() || doc.traces().iter().any(|t| t.id == *id && t.range.start <= sel.start && sel.end <= t.range.end))
    }

    pub fn cut_toggle(&mut self, _: &GhostToggle, _: &mut Window, cx: &mut Context<Self>) {
        let sel = self.page.read(cx).selection();
        if let Some(id) = self.selection_in_cut(cx) {
            self.put_back_cut(id, cx);
            return;
        }
        if sel.is_empty() {
            self.flash_status(tr!("Select some text to cut it"), cx);
            return;
        }
        if let Ok((id, r)) = self.model.update(cx, |m, cx| m.edit(cx, |d| d.cut(sel, None))) {
            let m = self.model.read(cx);
            self.last_cut = Some(LastCut { id, uid: m.uid, version: m.version, sel: r.end..r.end });
            self.page.update(cx, |p, cx| {
                p.refresh(cx);
                p.select(r.end..r.end, cx);
                p.fade_ghost(r, true, cx);
            });
            cx.notify();
        }
    }

    pub(crate) fn paste_cut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(clip) = crate::clipboard::cut_on_clipboard(cx) else { return };
        let uid = clip.uid;
        let here = clip.is_in(self.model.read(cx));
        let parked = self.project.as_ref().is_some_and(|p| p.parked.values().any(|q| clip.is_in(&q.model)));
        if !here && !parked {
            // The cut is gone (put back, deleted, its file closed): an ordinary paste.
            crate::clipboard::forget_cut(cx);
            self.page.update(cx, crate::clipboard::paste);
            return;
        }
        let sel = self.page.read(cx).selection();
        if !sel.is_empty() {
            self.page.update(cx, |p, cx| {
                p.replace(sel.clone(), "", cx);
            });
        }
        let at = sel.start;
        let r = if here {
            let doc = &self.model.read(cx).doc;
            let block = doc.cut_info(clip.id).is_some_and(|c| c.paragraph);
            let text: Vec<char> = doc.text().chars().collect();
            let place = place_at(&text, at, block);
            self.model.update(cx, |m, cx| m.edit(cx, |d| d.drop_text(Carried::Cut(clip.id), place, false)))
        } else {
            let block = self.project.as_ref().and_then(|p| p.parked.values().find(|q| q.model.uid == uid)).and_then(|q| q.model.doc.cut_info(clip.id)).is_some_and(|c| c.paragraph);
            let text: Vec<char> = self.model.read(cx).doc.text().chars().collect();
            self.move_cut_from_parked(uid, clip.id, place_at(&text, at, block), cx)
        };
        crate::clipboard::forget_cut(cx);
        if let Ok(r) = r {
            self.text_edited(false, cx);
            self.page.update(cx, |p, cx| {
                p.refresh(cx);
                p.select(r.end..r.end, cx);
                p.flash = Some((r, std::time::Instant::now()));
            });
            let f = self.page.read(cx).focus.clone();
            window.focus(&f, cx);
            self.hold_status(tr!("Moved · {undo} puts it back", undo = crate::os_text::key("⌘Z")), cx);
            cx.notify();
        }
    }

    pub(crate) fn move_cut_from_parked(&mut self, uid: u64, id: CutId, place: Place, cx: &mut Context<Self>) -> Result<Range<usize>, EditError> {
        let Some(p) = &mut self.project else { return Err(EditError::NoSuchCut) };
        let Some(other) = p.parked.values_mut().find(|q| q.model.uid == uid) else { return Err(EditError::NoSuchCut) };
        let scrap = other.model.doc.take_cut(id)?;
        other.model.version += 1;
        let r = self.model.update(cx, |m, cx| m.edit(cx, |d| d.drop_text(Carried::Fragment(scrap.fragment.trimmed()), place, true)));
        let Some(p) = &mut self.project else { return r };
        let Some(other) = p.parked.values_mut().find(|q| q.model.uid == uid) else { return r };
        if r.is_err() {
            // Never leave the text in neither place.
            other.model.doc.undo();
            return r;
        }
        let link = doc_model::new_link();
        other.model.doc.link_last_step(link);
        let here = self.model.update(cx, |m, _| {
            m.doc.link_last_step(link);
            m.uid
        });
        cx.default_global::<CrossLinks>().0.insert(link, (here, uid));
        r
    }

    /// Both halves go together when the other chapter's next step is its half; else it waits.
    pub(crate) fn linked_history(&mut self, link: u64, redo: bool, cx: &mut Context<Self>) {
        let Some(&(a, b)) = cx.try_global::<CrossLinks>().and_then(|c| c.0.get(&link)) else { return };
        let here = self.model.read(cx).uid;
        let other_uid = if here == a { b } else { a };
        let mut done = false;
        if let Some(p) = &mut self.project {
            if let Some(q) = p.parked.values_mut().find(|q| q.model.uid == other_uid) {
                let tag = if redo { q.model.doc.redo_tag() } else { q.model.doc.undo_tag() };
                if tag.and_then(|t| t.link) == Some(link) {
                    if redo {
                        q.model.doc.redo();
                    } else {
                        q.model.doc.undo();
                    }
                    q.model.version += 1;
                    done = true;
                }
            }
        }
        if done {
            self.model.update(cx, |m, cx| if redo { m.redo(cx) } else { m.undo(cx) });
            self.page.update(cx, |p, cx| p.refresh(cx));
        } else {
            self.flash_status(tr!("This change moved a cut between two files: undo it in the other file first"), cx);
        }
        cx.notify();
    }

    /// A presentation change, not an edit.
    pub(crate) fn represent_cuts(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| {
            m.present();
            cx.emit(crate::model::ModelEvent::Changed);
            cx.notify();
        });
        if let Some(p) = &mut self.project {
            for q in p.parked.values_mut() {
                q.model.present();
            }
            p.drawer.model.update(cx, |m, _| m.present());
        }
        self.page.update(cx, |p, cx| p.refresh(cx));
        cx.notify();
    }

    pub(crate) fn open_trace_menu(&mut self, id: CutId, at: gpui_kit::Point<Pixels>, cx: &mut Context<Self>) {
        let doc = &self.model.read(cx).doc;
        let mark = doc.traces().iter().any(|t| t.id == id && !t.shown);
        let version = doc.cut_info(id).is_some_and(|c| c.version.is_some());
        self.peek = None;
        self.context_menu = Some(crate::context_menu::ContextMenu::trace(id, mark, version, at));
        cx.notify();
    }

    pub(crate) fn trace_op(&mut self, id: CutId, op: TraceOp, window: &mut Window, cx: &mut Context<Self>) {
        self.peek = None;
        match op {
            TraceOp::PutBack => {
                self.put_back_cut(id, cx);
            }
            TraceOp::ShowText | TraceOp::ShowMark => {
                let look = if op == TraceOp::ShowMark { doc_model::Look::Mark } else { doc_model::Look::Text };
                let _ = self.model.update(cx, |m, cx| m.edit(cx, |d| d.set_look(id, Some(look))));
            }
            TraceOp::ShowInCuts => {
                self.open_drawer(window, cx);
                if let Some(d) = &mut self.drawer {
                    d.found = Some(crate::cuts_panel::CutRef::Page(id));
                }
                let f = self.page.read(cx).focus.clone();
                window.focus(&f, cx);
            }
            TraceOp::Forget => {
                let _ = self.model.update(cx, |m, cx| m.edit(cx, |d| d.forget_spot(id)));
            }
            TraceOp::Delete => {
                let _ = self.model.update(cx, |m, cx| m.edit(cx, |d| d.delete_cut(id)));
            }
        }
        self.page.update(cx, |p, cx| p.refresh(cx));
        cx.notify();
    }

    /// The peek lingers so the pointer can reach it.
    pub(crate) fn mark_hovered(&mut self, id: Option<CutId>, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            Some(id) => {
                self.peek = Some((id, false));
                cx.notify();
            }
            None => {
                let Some((was, _)) = self.peek else { return };
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(350)).await;
                    let _ = this.update(cx, |this, cx| {
                        let on_mark = this.page.read(cx).hovered_mark == Some(was);
                        if this.peek.is_some_and(|(id, on_peek)| id == was && !on_peek && !on_mark) {
                            this.peek = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
    }

    pub(crate) fn render_peek(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::theme::{self, c};
        let (id, _) = self.peek?;
        let page = self.page.read(cx);
        let &(_, at, paragraph) = page.marks.iter().find(|(m, _, _)| *m == id)?;
        let rect = page.mark_rect(at, paragraph, cx)?;
        let text = self.model.read(cx).doc.cut_info(id)?.text;
        let lh = page.layout.as_ref()?.line_height;
        let top = if paragraph { rect.bottom() + px(6.) } else { rect.top() + lh };
        let left = if paragraph { rect.left() } else { rect.left() - px(12.) };
        let button = |key: &'static str, label: SharedString, op: TraceOp, cx: &mut Context<Self>| {
            div()
                .id(key)
                .px(px(6.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(c(theme::accent()))
                .hover(|s| s.bg(c(theme::line())))
                .on_click(cx.listener(move |this, _, w, cx| this.trace_op(id, op, w, cx)))
                .child(label)
                .test_support()
        };
        let put_back = button("peek-put-back", tr!("Put Back {put_back}", put_back = crate::os_text::key("⌘/")).into(), TraceOp::PutBack, cx);
        let show = button("peek-show-text", tr!("Show the Text").into(), TraceOp::ShowText, cx);
        let in_cuts = button("peek-show-in-cuts", tr!("Show in Cuts").into(), TraceOp::ShowInCuts, cx);
        Some(
            div()
                .id("mark-peek")
                .absolute()
                .top(top)
                .left(left)
                .w(px(320.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(10.))
                .rounded(px(6.))
                .border_1()
                .border_color(c(theme::line()))
                .bg(c(theme::bg_raised()))
                .shadow_md()
                .text_size(px(13.))
                .line_height(px(19.))
                .occlude()
                .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                    match &mut this.peek {
                        Some((p, on_peek)) if *p == id => *on_peek = *on,
                        _ => {}
                    }
                    if !*on && this.page.read(cx).hovered_mark != Some(id) {
                        this.peek = None;
                    }
                    cx.notify();
                }))
                .child(div().text_color(c(theme::text_2())).line_clamp(4).text_ellipsis().child(text))
                .child(div().flex().gap(px(4.)).text_size(px(11.5)).child(put_back).child(show).child(in_cuts))
                .test_support()
                .into_any_element(),
        )
    }

    pub(crate) fn hold_status(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.flash_status(msg, cx);
        self.status_held = true;
        self.status_held_sel = Some(self.page.read(cx).selection());
    }

    pub(crate) fn clear_held_status(&mut self, cx: &mut Context<Self>) {
        if self.status_held {
            self.status_held = false;
            self.status = None;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::place_at;
    use doc_model::Place;

    #[test]
    fn cuts_land_by_ed9() {
        let t: Vec<char> = "One two.\n\nThree four.\n\nFive.".chars().collect();
        assert_eq!(place_at(&t, 4, false), Place::Inline(4));
        assert_eq!(place_at(&t, 10, true), Place::Block(10), "at a paragraph's start: before it");
        assert_eq!(place_at(&t, 13, true), Place::Block(23), "inside one: after it");
        assert_eq!(place_at(&t, 26, true), Place::Block(28), "in the last: at the end");
    }
}
