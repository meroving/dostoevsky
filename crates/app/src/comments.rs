use doc_model::CommentId;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::context_menu::{Command, ContextMenu};
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::Workspace;

pub(crate) struct CommentEdit {
    pub id: CommentId,
    pub editor: Entity<TextView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentOp {
    Edit,
    Copy,
    ShowInPanel,
    /// Notes ⇄ ticks, for every page (Preferences → Editing → Comments).
    SwitchLook,
    Delete,
}

/// RFC 3339, UTC.
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl Workspace {
    /// On the selection, or on the caret's sentence. Switches to editing first.
    pub fn add_comment(&mut self, _: &AddComment, window: &mut Window, cx: &mut Context<Self>) {
        self.end_comment(window, cx);
        self.enter_editing(cx);
        let page = self.page.read(cx);
        let mut range = page.selection();
        if range.is_empty() {
            match crate::sentence_keys::sentence_around(page.index(), &range) {
                Some(s) => range = s,
                None => {
                    self.flash_status(tr!("Select some text to comment on it"), cx);
                    return;
                }
            }
        }
        // Trailing spaces and line breaks aren't part of what it's about.
        let text = page.index().slice(range.clone()).to_string();
        let trail = text.chars().rev().take_while(|c| c.is_whitespace()).count();
        let lead = text.chars().take_while(|c| c.is_whitespace()).count();
        if lead + trail >= range.len() {
            self.flash_status(tr!("Select some text to comment on it"), cx);
            return;
        }
        let range = range.start + lead..range.end - trail;
        let made = self.model.update(cx, |m, cx| m.edit(cx, |d| d.add_comment(range, "", Some(now()))));
        match made {
            Ok(id) => self.write_comment(id, window, cx),
            Err(doc_model::EditError::InCut) => self.flash_status(tr!("A cut's text can't have a comment"), cx),
            Err(_) => self.flash_status(tr!("Select some text to comment on it"), cx),
        }
    }

    pub(crate) fn write_comment(&mut self, id: CommentId, window: &mut Window, cx: &mut Context<Self>) {
        if self.comment_edit.as_ref().is_some_and(|e| e.id == id) {
            let f = self.comment_edit.as_ref().map(|e| e.editor.read(cx).focus.clone());
            if let Some(f) = f {
                window.focus(&f, cx);
            }
            return;
        }
        self.end_comment(window, cx);
        self.comment_peek = None;
        self.context_menu = None;
        let model = self.model.clone();
        let editor = cx.new(|cx| {
            let mut v = TextView::new(Mode::Comment(id), model, cx);
            v.placeholder = tr!("Your comment").into();
            let n = v.index().len();
            v.select(n..n, cx);
            v
        });
        self._subs.push(cx.subscribe_in(&editor, window, Self::on_comment_event));
        let f = editor.read(cx).focus.clone();
        self._subs.push(cx.on_blur(&f, window, |this, window, cx| {
            // Clicking elsewhere ends it, after the click has had its say.
            let id = this.comment_edit.as_ref().map(|e| e.id);
            cx.defer_in(window, move |this, window, cx| {
                let still = this.comment_edit.as_ref().map(|e| e.id) == id;
                let focused = this.comment_edit.as_ref().is_some_and(|e| e.editor.read(cx).focus.is_focused(window));
                if still && !focused {
                    this.end_comment(window, cx);
                }
            });
        }));
        self.comment_edit = Some(CommentEdit { id, editor });
        self.page.update(cx, |p, cx| {
            p.editing_comment = Some(id);
            cx.notify();
        });
        window.focus(&f, cx);
        // Its field goes where the page paints its note: once the page has painted it.
        cx.on_next_frame(window, |_, _, cx| cx.notify());
        cx.notify();
    }

    /// A note left empty deletes its comment (with no undo step if it was new).
    pub(crate) fn end_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.comment_edit.take() else { return };
        self.model.update(cx, |m, cx| {
            let before = m.doc.has_comments();
            m.doc.finish_comment(edit.id);
            if before != m.doc.has_comments() || m.doc.comment_text(edit.id).is_none() {
                let _ = m.edit(cx, |_| Ok(()));
            }
        });
        self.page.update(cx, |p, cx| {
            p.editing_comment = None;
            cx.notify();
        });
        if edit.editor.read(cx).focus.is_focused(window) {
            let f = self.page.read(cx).focus.clone();
            window.focus(&f, cx);
        }
        cx.notify();
    }

    fn on_comment_event(&mut self, _: &Entity<TextView>, e: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            ViewEvent::Confirm | ViewEvent::Cancel => self.end_comment(window, cx),
            ViewEvent::Edited => cx.notify(),
            _ => {}
        }
    }

    pub fn delete_comment(&mut self, _: &DeleteComment, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.comment_edit.as_ref().map(|e| e.id) else { return };
        self.comment_edit = None;
        self.model.update(cx, |m, cx| {
            m.doc.break_undo_group();
            let _ = m.edit(cx, |d| d.delete_comment(id));
        });
        self.page.update(cx, |p, cx| {
            p.editing_comment = None;
            cx.notify();
        });
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
        self.flash_status(tr!("Comment deleted · ⌘Z brings it back"), cx);
    }

    pub fn delete_page_comments(&mut self, _: &DeleteAllComments, window: &mut Window, cx: &mut Context<Self>) {
        self.end_comment(window, cx);
        let n = self.model.read(cx).doc.comments().len();
        if !self.model.read(cx).doc.has_comments() {
            self.flash_status(tr!("There are no comments on this page"), cx);
            return;
        }
        let rx = window.prompt(
            PromptLevel::Warning,
            &if n == 0 { tr!("Delete the comments on this page?").to_string() } else { trn!("Delete the comment on this page?", "Delete all {n} comments on this page?", n) },
            Some(tr!("You can undo this with ⌘Z.")),
            &[tr!("Delete"), tr!("Cancel")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if rx.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    this.model.update(cx, |m, cx| {
                        let _ = m.edit(cx, |d| d.delete_page_comments());
                    });
                });
            }
        })
        .detach();
    }

    pub(crate) fn comments_on_model_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let gone = self.comment_edit.as_ref().is_some_and(|e| self.model.read(cx).doc.comment_text(e.id).is_none());
        if gone {
            self.comment_edit = None;
            self.page.update(cx, |p, cx| {
                p.editing_comment = None;
                cx.notify();
            });
            let f = self.page.read(cx).focus.clone();
            window.focus(&f, cx);
        }
        if self.comment_peek.is_some_and(|id| !self.page.read(cx).comments.iter().any(|k| k.id == id)) {
            self.comment_peek = None;
        }
    }

    pub(crate) fn comment_hovered(&mut self, id: Option<CommentId>, cx: &mut Context<Self>) {
        let ticked = |id: CommentId, this: &Self, cx: &App| this.page.read(cx).comment_rects.borrow().iter().any(|(k, _, tick)| *k == id && *tick);
        let next = id.filter(|id| ticked(*id, self, cx));
        if next != self.comment_peek {
            self.comment_peek = next;
            cx.notify();
        }
    }

    pub(crate) fn comment_menu(&mut self, id: CommentId, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.context_menu = Some(ContextMenu::comment(id, at, crate::prefs::Prefs::get(cx).comment_look));
        cx.notify();
    }

    pub(crate) fn comment_op(&mut self, id: CommentId, op: CommentOp, window: &mut Window, cx: &mut Context<Self>) {
        match op {
            CommentOp::Edit => self.write_comment(id, window, cx),
            CommentOp::Copy => {
                if let Some(t) = self.model.read(cx).doc.comment_text(id) {
                    cx.write_to_clipboard(ClipboardItem::new_string(t));
                }
            }
            CommentOp::ShowInPanel => self.show_comments_panel(Some(id), window, cx),
            CommentOp::SwitchLook => {
                use crate::prefs::CommentLook;
                self.comment_peek = None;
                crate::sheets::set_prefs(cx, |p| p.comment_look = if p.comment_look == CommentLook::Ticks { CommentLook::Notes } else { CommentLook::Ticks });
            }
            CommentOp::Delete => {
                self.end_comment(window, cx);
                self.model.update(cx, |m, cx| {
                    m.doc.break_undo_group();
                    let _ = m.edit(cx, |d| d.delete_comment(id));
                });
                self.flash_status(tr!("Comment deleted · ⌘Z brings it back"), cx);
            }
        }
    }

    pub(crate) fn render_comment_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let edit = self.comment_edit.as_ref()?;
        let (id, editor) = (edit.id, edit.editor.clone());
        let page = self.page.read(cx);
        let found = page.comment_rects.borrow().iter().find(|(k, _, _)| *k == id).map(|(_, b, t)| (*b, *t));
        let Some((rect, tick)) = found else {
            // Not painted yet (it was just made): next frame.
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            return None;
        };
        let follow = FollowPage::new(&self.page, cx);
        if !tick {
            return Some(
                follow
                    .child(
                        div()
                            .id("comment-field")
                            .absolute()
                            .left(rect.left() + px(10.))
                            .top(rect.top() - crate::view::COMMENT_PAD)
                            .w(rect.size.width - px(10.))
                            .occlude()
                            .child(editor)
                            .test_support(),
                    )
                    .into_any_element(),
            );
        }
        let lh = page.layout.as_ref().map_or(px(30.), |l| l.line_height);
        Some(follow.child(card("comment-card", rect, lh).child(editor).child(hints(true)).test_support()).into_any_element())
    }

    pub(crate) fn render_comment_peek(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.comment_peek?;
        if self.comment_edit.as_ref().is_some_and(|e| e.id == id) {
            return None;
        }
        let page = self.page.read(cx);
        let rect = page.comment_rects.borrow().iter().find(|(k, _, t)| *k == id && *t).map(|(_, b, _)| *b)?;
        let text = page.comments.iter().find(|k| k.id == id)?.text.clone();
        let lh = page.layout.as_ref().map_or(px(30.), |l| l.line_height);
        Some(
            FollowPage::new(&self.page, cx)
                .child(
                    card("comment-peek", rect, lh)
                        .child(div().pl(px(2.)).font_family(theme::UI).text_size(px(theme::NOTE_PX)).line_height(px(theme::NOTE_LINE)).text_color(c(theme::comment())).child(text))
                        .child(hints(false))
                        .test_support(),
                )
                .into_any_element(),
        )
    }
}

/// Holds something the window draws over a comment's note or tick (the field it is written in,
/// a tick's card) where the page has scrolled it to *this* frame. Its place comes from the page's
/// last paint, and scrolling only redraws the page, so without this the field stays behind and
/// lands on the next note until something else redraws the window. Clipped to the page, like the notes.
pub(crate) struct FollowPage {
    page: Entity<TextView>,
    painted_at: Pixels,
    child: Option<AnyElement>,
}

impl FollowPage {
    fn new(page: &Entity<TextView>, cx: &App) -> Self {
        Self { page: page.clone(), painted_at: page.read(cx).comment_rects_scroll.get(), child: None }
    }

    fn child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(child.into_any_element());
        self
    }

    /// The page is laid out and scrolled before this (it comes first in the window's tree).
    fn place(&self, cx: &App) -> (Point<Pixels>, Option<Bounds<Pixels>>) {
        let page = self.page.read(cx);
        (point(px(0.), self.painted_at - page.scroll_offset()), page.layout.as_ref().map(|l| l.bounds))
    }
}

impl IntoElement for FollowPage {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FollowPage {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let child = self.child.as_mut().expect("FollowPage without a child");
        (child.request_layout(window, cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        let (offset, clip) = self.place(cx);
        let Some(child) = self.child.as_mut() else { return };
        window.with_content_mask(clip.map(|bounds| ContentMask { bounds }), |window| {
            window.with_element_offset(offset, |window| child.prepaint(window, cx));
        });
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        let clip = self.page.read(cx).layout.as_ref().map(|l| l.bounds);
        let Some(child) = self.child.as_mut() else { return };
        window.with_content_mask(clip.map(|bounds| ContentMask { bounds }), |window| child.paint(window, cx));
    }
}

fn card(id: &'static str, tick: Bounds<Pixels>, lh: Pixels) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .left(tick.left() - px(320.))
        .top(tick.top() + lh)
        .w(px(300.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(8.))
        .border_1()
        .border_color(c(theme::line()))
        .bg(c(theme::bg_raised()))
        .shadow_md()
        .occlude()
}

fn hints(writing: bool) -> Div {
    let line = if writing { tr!("↩ done · ⇧↩ new line · ⌘⌫ delete") } else { tr!("Click to edit · right-click for more") };
    div().text_size(px(11.)).text_color(ca(theme::muted(), 1.)).child(line)
}

impl ContextMenu {
    pub fn comment(id: CommentId, at: Point<Pixels>, look: crate::prefs::CommentLook) -> Self {
        use crate::context_menu::Item;
        let op = |label: String, keys: Option<&'static str>, o: CommentOp| Some(Item { label: label.into(), shortcut: None, keys, command: Some(Command::Comment(id, o)), badge: None });
        let items = vec![
            op(tr!("Edit Comment").to_string(), Some("↩"), CommentOp::Edit),
            op(tr!("Copy Comment").to_string(), None, CommentOp::Copy),
            op(tr!("Show in Comments").to_string(), None, CommentOp::ShowInPanel),
            None,
            op(
                match look {
                    crate::prefs::CommentLook::Notes => tr!("Show Comments as Ticks").to_string(),
                    crate::prefs::CommentLook::Ticks => tr!("Show Comments as Notes").to_string(),
                },
                None,
                CommentOp::SwitchLook,
            ),
            None,
            op(tr!("Delete Comment").to_string(), Some("⌘⌫"), CommentOp::Delete),
        ];
        ContextMenu { at, heading: None, items, up: false }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RightTab {
    #[default]
    Cuts,
    Comments,
}

#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub id: Option<CommentId>,
    /// `None`: the page.
    pub file: Option<std::path::PathBuf>,
    pub quote: String,
    pub text: String,
    pub chip: String,
    pub date: Option<String>,
    pub range: std::ops::Range<usize>,
}

/// Chapters that aren't open, by file: (modified time when read, rows).
pub(crate) type DiskComments = std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, (Option<std::time::SystemTime>, Vec<Row>)>>;

pub(crate) struct Section {
    pub title: Option<String>,
    pub rows: Vec<Row>,
}

fn date_of(created: Option<&str>) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc3339(created?).ok()?;
    let t: std::time::SystemTime = t.into();
    let today = chrono::Local::now().date_naive();
    let d: chrono::DateTime<chrono::Local> = t.into();
    if d.date_naive() + chrono::Days::new(1) == today {
        return Some(tr!("yesterday").to_string());
    }
    Some(crate::project_ui::when(t))
}

/// `label` names the file, for the whole book.
fn rows_of(doc: &doc_model::Document, label: Option<&str>, file: Option<&std::path::Path>, ids: bool) -> Vec<Row> {
    let text: Vec<char> = doc.text().chars().collect();
    doc.comments()
        .into_iter()
        .map(|k| {
            let quote: String = text.get(k.range.clone()).map(|t| t.iter().collect::<String>()).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
            let chip = crate::cuts_panel::chip_at(&text, k.range.start, false);
            let chip = match label {
                Some(l) => format!("{l} {chip}"),
                None => chip,
            };
            Row { id: ids.then_some(k.id), file: file.map(|f| f.to_path_buf()), quote, text: k.text, chip, date: date_of(k.created.as_deref()), range: k.range }
        })
        .collect()
}

impl Workspace {
    pub(crate) fn comments_count(&self, cx: &App) -> usize {
        self.comment_sections(cx).iter().map(|s| s.rows.len()).sum()
    }

    /// Whole book: every chapter's in book order, this one first.
    pub(crate) fn comment_sections(&self, cx: &App) -> Vec<Section> {
        let here = self.model.read(cx).path.clone();
        let page = rows_of(&self.model.read(cx).doc, None, None, true);
        let Some(p) = self.project.as_ref().filter(|_| self.comments_scope == crate::cuts_panel::Scope::Book) else {
            return vec![Section { title: None, rows: page }];
        };
        let title = |e: &crate::project::Entry| match e.number {
            Some(n) => format!("{n} · {}", e.label.to_uppercase()),
            None => e.label.to_uppercase(),
        };
        let mut out = vec![Section { title: here.as_ref().and_then(|h| p.tree.entry(h)).map(title).or_else(|| Some(tr!("THIS PAGE").to_string())), rows: page }];
        for e in p.tree.all() {
            if Some(&e.path) == here.as_ref() {
                continue;
            }
            let rows = match p.parked.get(&e.path) {
                Some(q) => rows_of(&q.model.doc, Some(&e.label), Some(&e.path), false),
                None => self.comments_on_disk(&e.path, &e.label),
            };
            if !rows.is_empty() {
                out.push(Section { title: Some(title(e)), rows });
            }
        }
        out
    }

    /// Read from the file once per change on disk.
    fn comments_on_disk(&self, path: &std::path::Path, label: &str) -> Vec<Row> {
        let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some((s, rows)) = self.comment_files.borrow().get(path) {
            if *s == stamp {
                return rows.clone();
            }
        }
        let rows = std::fs::read_to_string(path)
            .ok()
            .filter(|t| t.contains("\"kind\":\"comment\""))
            .map(|t| rows_of(&doc_format::parse(&t).doc, Some(label), Some(path), false))
            .unwrap_or_default();
        self.comment_files.borrow_mut().insert(path.to_path_buf(), (stamp, rows.clone()));
        rows
    }

    pub(crate) fn show_comments_panel(&mut self, id: Option<CommentId>, window: &mut Window, cx: &mut Context<Self>) {
        self.open_drawer(window, cx);
        self.right_tab = RightTab::Comments;
        self.comment_found = id;
        self.sync_comment_ticks(cx);
        cx.notify();
    }

    pub(crate) fn set_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.right_tab = tab;
        self.sync_comment_ticks(cx);
        cx.notify();
    }

    /// With the Comments tab open the list has the notes, so the page shows ticks instead.
    pub(crate) fn sync_comment_ticks(&mut self, cx: &mut Context<Self>) {
        let ticks = self.drawer.is_some() && self.right_tab == RightTab::Comments;
        self.page.update(cx, |p, cx| {
            if p.comment_ticks != ticks {
                p.comment_ticks = ticks;
                cx.notify();
            }
        });
    }

    fn go_to_comment(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        self.enter_editing(cx);
        let id = match (&row.file, row.id) {
            (None, Some(id)) => Some(id),
            (Some(path), _) => {
                if !self.switch_to(path, window, cx) {
                    return;
                }
                let doc = &self.model.read(cx).doc;
                doc.comments().into_iter().find(|k| k.range == row.range && k.text == row.text).or_else(|| doc.comments().into_iter().find(|k| k.text == row.text)).map(|k| k.id)
            }
            _ => None,
        };
        let Some(id) = id else { return };
        let Some(range) = self.model.read(cx).doc.comments().into_iter().find(|k| k.id == id).map(|k| k.range) else { return };
        self.comment_found = Some(id);
        self.page.update(cx, |p, cx| {
            p.reveal(range.start, cx);
            p.flash = Some((range, std::time::Instant::now()));
            cx.notify();
        });
        self.write_comment(id, window, cx);
    }

    pub(crate) fn render_right_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.right_tab;
        let cuts = self.cuts_count_for_tab(cx);
        let comments = self.comments_count(cx);
        let one = |id: &'static str, label: String, n: usize, on: bool, which: RightTab, cx: &mut Context<Self>| {
            div()
                .id(id)
                .relative()
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(28.))
                .role(Role::Tab)
                .aria_selected(on)
                .cursor_pointer()
                .text_color(if on { c(theme::accent()) } else { c(theme::muted()) })
                .hover(|s| s.text_color(c(theme::text_2())))
                .on_click(cx.listener(move |this, _, _, cx| this.set_right_tab(which, cx)))
                .child(label)
                .when(n > 0, |d| d.child(div().text_size(px(10.)).child(n.to_string())))
                .when(on, |d| d.child(div().absolute().left_0().right_0().bottom(px(1.)).h(px(2.)).rounded(px(2.)).bg(c(theme::accent()))))
                .test_support()
        };
        div()
            .absolute()
            .top(px(12.))
            .left(px(20.))
            .flex()
            .gap(px(18.))
            .text_size(px(12.))
            .child(one("tab-cuts", tr!("Cuts").to_string(), cuts, tab == RightTab::Cuts, RightTab::Cuts, cx))
            .child(one("tab-comments", tr!("Comments").to_string(), comments, tab == RightTab::Comments, RightTab::Comments, cx))
            .into_any_element()
    }

    /// The Comments tab (COM-5).
    pub(crate) fn render_comments_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let sections = self.comment_sections(cx);
        let book = sections.len() > 1 || sections.first().is_some_and(|s| s.title.is_some());
        let found = self.comment_found;
        let switch = self.project_kind().map(|k| {
            let whole = match k {
                crate::project::Kind::Book => tr!("whole book"),
                crate::project::Kind::Journal => tr!("whole journal"),
                crate::project::Kind::Folder => tr!("whole folder"),
            };
            crate::list_panel::scope_switch(
                "comments-scope",
                [tr!("this page").into(), whole.into()],
                usize::from(self.comments_scope == crate::cuts_panel::Scope::Book),
                cx.listener(|this, i: &usize, _, cx| {
                    this.comments_scope = if *i == 1 { crate::cuts_panel::Scope::Book } else { crate::cuts_panel::Scope::Page };
                    cx.notify();
                }),
            )
        });
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut n = 0usize;
        for (si, s) in sections.into_iter().enumerate() {
            if s.rows.is_empty() {
                continue;
            }
            if book {
                rows.push(crate::list_panel::section_header(("comments-section", si), s.title.clone().unwrap_or_default(), s.rows.len()));
            }
            for r in s.rows {
                let lit = r.id.is_some() && r.id == found;
                let id = r.id;
                let go = r.clone();
                rows.push(
                    div()
                        .id(("comment-row", n))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .px(px(8.))
                        .py(px(7.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(lit, |d| d.bg(ca(theme::comment(), 0.10)))
                        .hover(|d| d.bg(ca(theme::line(), 0.8)))
                        .on_click(cx.listener(move |this, _, window, cx| this.go_to_comment(go.clone(), window, cx)))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                if let Some(id) = id {
                                    this.comment_menu(id, e.position, cx);
                                }
                            }),
                        )
                        .child(div().text_size(px(11.)).line_height(px(16.)).text_color(c(theme::muted())).truncate().child(format!("“{}”", r.quote)))
                        .child(
                            div()
                                .pl(px(2.))
                                .font_family(theme::UI)
                                .text_size(px(theme::NOTE_PX))
                                .line_height(px(theme::NOTE_LINE))
                                .text_color(c(theme::comment()))
                                .line_clamp(4)
                                .text_ellipsis()
                                .child(if r.text.is_empty() { tr!("Your comment").to_string() } else { r.text.clone() }),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .items_center()
                                .text_size(px(10.5))
                                .line_height(px(15.))
                                .text_color(c(theme::muted()))
                                .child(div().px(px(5.)).rounded(px(4.)).border_1().border_color(c(theme::origin_line())).text_color(c(theme::origin_text())).child(r.chip.clone()))
                                .children(r.date.clone()),
                        )
                        .test_support()
                        .into_any_element(),
                );
                n += 1;
            }
        }
        let empty = n == 0;
        div()
            .id("comments-panel")
            .test_support()
            .absolute()
            .top(px(crate::workspace::HEADER_H))
            .bottom_0()
            .right_0()
            .w(px(crate::workspace::overflow_width(cx)))
            .flex()
            .flex_col()
            .bg(theme::surface(theme::bg_panel()))
            .border_l_1()
            .border_color(c(theme::line()))
            .pt(px(160. - crate::workspace::HEADER_H))
            .px(px(13.))
            .pb(px(64.))
            .gap_3()
            .text_size(px(14.))
            .line_height(px(21.))
            .child(crate::workspace::resize_handle(crate::workspace::PanelEdge::Overflow))
            .child(self.render_right_tabs(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .pl(px(8.))
                    .child(div().font_family(theme::hand()).text_size(theme::hand_px(19.)).text_color(c(theme::accent())).child(tr!("Comments")))
                    .children(switch),
            )
            .child(
                div()
                    .id("comment-rows")
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |d| {
                        d.child(
                            div()
                                .id("comments-empty")
                                .px(px(8.))
                                .text_color(c(theme::faint()))
                                .text_size(px(12.))
                                .child(tr!("Notes about the text wait here, each on its words. Select some and press ⌥⌘M."))
                                .test_support(),
                        )
                    }),
            )
            .child(div().pl(px(8.)).text_size(px(11.)).text_color(c(theme::faint())).child(tr!("⌥⌘M comments on the selection")))
            .into_any_element()
    }
}
