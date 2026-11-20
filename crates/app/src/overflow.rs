use std::time::Instant;

use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::cuts_panel::CutRef;
use crate::prefs::Prefs;
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::Workspace;

/// The default; the user can drag the inner edge.
pub const WIDTH: f32 = 262.;

#[derive(Clone)]
pub struct NoteDrag {
    pub at: CutRef,
    pub preview: SharedString,
    pub words: usize,
    /// Whole paragraphs or more than one line: it lands between paragraphs.
    pub block: bool,
}

impl Render for NoteDrag {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::drag::card(self.preview.clone(), self.words, window)
    }
}

pub struct Drawer {
    /// Also the cut ⌘↩ uses.
    pub editing: Option<CutRef>,
    pub editor: Option<Entity<TextView>>,
    /// Window bounds from the last frame, in the panel's order.
    pub(crate) cards: std::rc::Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
    /// Page text is being dragged over the panel.
    pub hot: bool,
    /// Opened for a drag: closes when it ends, unless the text went in.
    pub for_drag: bool,
    /// The cut holding find's current match.
    pub found: Option<CutRef>,
    /// Clicked once: ⌘↩ uses it; double-click edits it.
    pub selected: Option<CutRef>,
    /// Other files' groups in the whole book that show all their cuts.
    pub unfolded: std::collections::HashSet<std::path::PathBuf>,
    /// Batches (a Trim's) that show their cuts under their entry.
    pub unfolded_batches: std::collections::HashSet<String>,
    /// Has the keyboard when a cut that can't be edited here is selected.
    pub focus: FocusHandle,
}

#[cfg(any(test, feature = "shots"))]
impl Drawer {
    pub fn card_bounds(&self) -> Vec<Bounds<Pixels>> {
        self.cards.borrow().clone()
    }
}

impl Workspace {
    /// Asks first; one undo step. Cuts with no trace stay in the panel.
    pub fn remove_all_ghosted(&mut self, _: &RemoveAllGhosted, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.model.read(cx).doc.listed_cuts().iter().filter(|c| !matches!(c.place, doc_model::CutPlace::Note(_))).count();
        if n == 0 {
            self.flash_status(tr!("There are no cuts on this page"), cx);
            return;
        }
        let rx = window.prompt(
            PromptLevel::Warning,
            &trn!("Delete the cut on this page?", "Delete all {n} cuts on this page?", n),
            Some(tr!("You can undo this with ⌘Z.")),
            &[tr!("Delete"), tr!("Cancel")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if rx.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    this.model.update(cx, |m, cx| {
                        let _ = m.edit(cx, |d| d.delete_page_cuts());
                    });
                    this.page.update(cx, |p, cx| p.refresh(cx));
                });
            }
        })
        .detach();
    }

    pub fn toggle_overflow(&mut self, _: &ToggleOverflow, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_cuts_side(window, cx);
        self.sync_comment_ticks(cx);
        cx.notify();
    }

    /// The Cuts tab, on whichever side it lives.
    pub(crate) fn open_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.make_room_for(crate::panels::PanelTab::Cuts, window, cx);
        self.ensure_drawer(cx);
    }

    pub(crate) fn ensure_drawer(&mut self, cx: &mut Context<Self>) {
        if self.drawer.is_some() {
            return;
        }
        let focus = cx.focus_handle();
        self.drawer = Some(Drawer { editing: None, editor: None, cards: Default::default(), hot: false, for_drag: false, found: None, selected: None, unfolded: Default::default(), unfolded_batches: Default::default(), focus });
    }

    /// Opens a closed panel without the keyboard, only if the page can stay where it is: text
    /// that moved under the pointer as it was picked up would land in the wrong place.
    pub(crate) fn open_drawer_for_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::panels::{side_of, side_width, PanelTab, Side};
        let side = side_of(PanelTab::Cuts, cx);
        if self.side_open(side, cx) {
            return;
        }
        let Some((origin, width)) = self.page.read(cx).layout.as_ref().map(|l| l.column()) else { return };
        // The page keeps 40 px clear of a panel (`compute_layout`).
        let panel = px(side_width(side, cx)) + px(40.);
        let fits = match side {
            Side::Right => origin.x + width <= window.viewport_size().width - panel,
            Side::Left => origin.x >= panel,
        };
        if !fits {
            return;
        }
        self.open_drawer(window, cx);
        if let Some(d) = &mut self.drawer {
            d.for_drag = true;
        }
        cx.notify();
    }

    pub(crate) fn close_drawer_after_drag(&mut self, cx: &App) {
        if !cx.has_active_drag() && self.drawer.as_ref().is_some_and(|d| d.for_drag) {
            self.drawer = None;
        }
    }

    /// Without taking the keyboard.
    pub(crate) fn open_overflow_for_drop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_drawer(window, cx);
        cx.notify();
    }

    fn on_note_event(&mut self, _: &Entity<TextView>, e: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            ViewEvent::Cancel => self.stop_editing_note(window, cx),
            ViewEvent::HistoryElsewhere(link) => self.history_elsewhere(*link, cx),
            _ => {}
        }
    }

    /// In place when its document is open in this window; either way it becomes the cut ⌘↩ uses.
    pub(crate) fn edit_note(&mut self, at: CutRef, window: &mut Window, cx: &mut Context<Self>) {
        let (model, drawer_side) = match &at {
            CutRef::Page(_) => (Some(self.model.clone()), false),
            CutRef::Drawer(_) => (self.shared_drawer(), true),
            _ => (None, false),
        };
        let id = at.id();
        let Some(d) = &mut self.drawer else { return };
        d.selected = Some(at.clone());
        d.editing = Some(at);
        d.editor = None;
        let (Some(model), Some(id)) = (model, id) else {
            // Only selected: its text is edited on its own page.
            let f = d.focus.clone();
            window.focus(&f, cx);
            cx.notify();
            return;
        };
        let partner = if drawer_side { Some(self.model.clone()) } else { self.shared_drawer() };
        let editor = cx.new(|cx| {
            let mut v = TextView::new(Mode::Cut(id), model, cx);
            v.partner = partner;
            v.drawer_side = drawer_side;
            v
        });
        self._subs.push(cx.subscribe_in(&editor, window, Self::on_note_event));
        let f = editor.read(cx).focus.clone();
        if let Some(d) = &mut self.drawer {
            d.editor = Some(editor);
        }
        window.focus(&f, cx);
        cx.notify();
    }

    pub(crate) fn stop_editing_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = &mut self.drawer {
            d.editing = None;
            d.editor = None;
        }
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
        cx.notify();
    }

    pub fn stash(&mut self, _: &StashToOverflow, window: &mut Window, cx: &mut Context<Self>) {
        let page = self.page.read(cx);
        let sel = page.selection();
        if sel.is_empty() {
            self.flash_status(tr!("Select some text to cut it"), cx);
            return;
        }
        let text = page.index().slice(sel.clone()).to_string();
        let from = page.layout.as_ref().and_then(|l| l.point_for(sel.start));
        if !self.stash_range(sel, false, cx) {
            return;
        }
        self.open_drawer(window, cx);
        if let Some(d) = &mut self.drawer {
            d.editing = None;
            d.editor = None;
        }
        let size = window.viewport_size();
        if let Some(from) = from {
            let side = crate::panels::side_of(crate::panels::PanelTab::Cuts, cx);
            let to = point(px(self.side_x(side, f32::from(size.width), cx) + 24.), px(150.));
            self.overlay.fly(from, to, text, Prefs::get(cx).motion());
        }
        cx.notify();
    }

    /// The cut being edited, else the selected one, else the first.
    pub fn insert_overflow_note(&mut self, _: &InsertOverflowNote, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.panel_items(cx);
        let chosen = self.drawer.as_ref().and_then(|d| d.editing.clone().or_else(|| d.selected.clone()));
        let at = chosen.filter(|at| items.iter().any(|i| &i.at == at)).or_else(|| items.into_iter().next().map(|i| i.at));
        let Some(at) = at else { return };
        let caret = self.page.read(cx).caret();
        self.unstash_note(at, caret, false, window, cx);
    }

    /// By the drop rule; `copy` copies instead of moving.
    pub fn unstash_note(&mut self, at: CutRef, caret: usize, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(r) = self.use_cut(at, crate::drawer::ToPage::At(caret), copy, cx) {
            if let Some(d) = &mut self.drawer {
                d.editing = None;
                d.editor = None;
            }
            self.text_edited(false, cx);
            self.page.update(cx, |p, cx| {
                // Catch up with the inserted text first, or the old selection would be mapped
                // through the insert afterwards and stretch past the cut.
                p.refresh(cx);
                p.select(r.clone(), cx);
                p.flash = Some((r, Instant::now()));
            });
            let f = self.page.read(cx).focus.clone();
            window.focus(&f, cx);
        }
        cx.notify();
    }

    /// The paragraph a cut came from, found by its quote on the page.
    pub(crate) fn origin_paragraph(&self, quote: &str, cx: &App) -> Option<(usize, usize)> {
        let idx = self.page.read(cx).index();
        crate::cuts_panel::origin_in(&idx.text, quote)
    }

    pub(crate) fn follow_origin_in(&mut self, source: &str, quote: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(std::path::Path::to_path_buf) else { return };
        let path = source.split('/').fold(root, |p, part| p.join(part));
        if !path.exists() {
            self.flash_status(tr!("{file} isn’t in the project any more", file = source), cx);
            return;
        }
        if self.switch_to(&path, window, cx) {
            if let Some((_, start)) = quote.and_then(|q| self.origin_paragraph(q, cx)) {
                self.follow_origin(start, cx);
            }
        }
    }

    pub(crate) fn follow_origin(&mut self, start: usize, cx: &mut Context<Self>) {
        self.page.update(cx, |p, cx| {
            let r = p.index().paragraph_at(start);
            p.reveal(start, cx);
            p.flash = Some((r, Instant::now()));
            cx.notify();
        });
    }
}
