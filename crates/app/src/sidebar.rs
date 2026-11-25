use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use doc_model::{EditError, GroupId, GroupInfo, Scope, Source, VariantId, VisibleGroup};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::markdown::{self, BlockKind};
use crate::prefs::Prefs;
use crate::text::TextIndex;
use crate::theme::{self, c, ca};
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::Workspace;

/// The default; a dragged width is `Prefs::sidebar_width`.
pub const WIDTH: f32 = 262.;

pub struct Sidebar {
    /// `None` shows the list of every group.
    pub group: Option<GroupId>,
    /// The group ↑/↓ reached, or the one just left.
    pub cursor: Option<GroupId>,
    pub list_focus: FocusHandle,
    list_scroll: ScrollHandle,
    reveal_cursor: bool,
    pub input: Entity<TextView>,
    rows: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    dance: Option<Instant>,
    pub note: Option<SharedString>,
    pub editing: Option<VariantEdit>,
    pub found: Option<(GroupId, usize)>,
    /// ALT-23: the gap a dragged variant would land in, between rows; never above the original.
    pub drop_at: Option<usize>,
}

/// Follows the variant, not its row, so removing or reordering others doesn't move the edit.
pub struct VariantEdit {
    pub group: GroupId,
    pub variant: VariantId,
    pub field: Entity<TextView>,
    _sub: Subscription,
}

#[cfg(test)]
impl Sidebar {
    pub fn row_bounds(&self) -> Vec<Bounds<Pixels>> {
        self.rows.borrow().clone()
    }
}

/// Up to 4 words with no sentence break → word; the whole block → paragraph; else sentence
/// (headings too). The caller first takes exactly one whole sentence as a sentence.
pub fn detect_scope(selected: &str, whole_block: bool, heading: bool) -> Scope {
    if heading {
        return Scope::Sentence;
    }
    if whole_block {
        return Scope::Paragraph;
    }
    let t = selected.trim();
    let inner_break = t
        .char_indices()
        .any(|(i, ch)| matches!(ch, '.' | '!' | '?' | '\n') && i + ch.len_utf8() < t.len());
    if english::word_count(t) <= 4 && !inner_break {
        Scope::Word
    } else {
        Scope::Sentence
    }
}

pub fn scope_label(s: Scope) -> &'static str {
    match s {
        Scope::Word => tr!("Word"),
        Scope::Sentence => tr!("Sentence"),
        Scope::Paragraph => tr!("Paragraph"),
    }
}

/// Returns the text and the group's byte range in it.
pub fn snippet(idx: &TextIndex, range: Range<usize>, around: usize, most: usize) -> (String, Range<usize>) {
    let flat = |s: &str| s.replace('\n', " ");
    let block = idx.paragraph_at(range.start);
    let mut out = String::new();
    if around > 0 {
        let from = range.start.saturating_sub(around).max(block.start);
        let mut before = flat(idx.slice(from..range.start));
        if from > block.start {
            before = match before.find(' ') {
                Some(i) => format!("…{}", &before[i + 1..]),
                None => format!("…{before}"),
            };
        }
        out.push_str(&before);
    }
    let start = out.len();
    let mut group = flat(idx.slice(range.clone()));
    if group.chars().count() > most {
        group = group.chars().take(most).collect::<String>().trim_end().to_string() + "…";
    }
    out.push_str(&group);
    let end = out.len();
    if around > 0 {
        let to = (range.end + around).min(block.end);
        let mut after = flat(idx.slice(range.end..to));
        if to < block.end {
            after = match after.rfind(' ') {
                Some(i) if i > 0 => format!("{}…", &after[..i]),
                _ => format!("{after}…"),
            };
        }
        out.push_str(after.trim_end());
    }
    (out, start..end)
}

fn headings(idx: &TextIndex) -> Vec<(usize, String)> {
    (0..idx.lines.len())
        .filter_map(|i| {
            let line = idx.line_str(i);
            let style = markdown::style_line(line);
            matches!(style.kind, BlockKind::Heading(_)).then(|| (idx.lines[i].char_start, line[style.hidden_prefix..].trim().to_lowercase()))
        })
        .filter(|(_, t)| !t.is_empty())
        .collect()
}

/// Yours, most used first, then this scope's defaults that aren't among them.
pub(crate) fn steer_candidates(scope: Scope, cx: &App) -> Vec<String> {
    let defaults: [&str; 3] = match scope {
        Scope::Paragraph => [tr!("shorter"), tr!("warmer"), tr!("more direct")],
        Scope::Sentence => [tr!("shorter"), tr!("plainer"), tr!("more active")],
        _ => [tr!("plainer"), tr!("warmer"), tr!("more concrete")],
    };
    let mut out = crate::memory::Memory::steers(cx);
    for d in defaults {
        if !out.iter().any(|s| s.to_lowercase() == d.to_lowercase()) {
            out.push(d.to_string());
        }
    }
    out
}

/// The first candidate when none is typed; the next one when it is a candidate; else the one it begins.
pub(crate) fn next_steer(typed: &str, candidates: &[String]) -> Option<String> {
    let low = typed.to_lowercase();
    if low.is_empty() {
        return candidates.first().cloned();
    }
    if let Some(i) = candidates.iter().position(|c| c.to_lowercase() == low) {
        return candidates.get((i + 1) % candidates.len()).cloned();
    }
    candidates.iter().find(|c| c.to_lowercase().starts_with(&low)).cloned()
}

pub(crate) fn ideas_trigger(text: &str) -> Option<&str> {
    text.trim().strip_prefix("??").map(str::trim)
}

/// The key, what it does, and whether it is lit.
pub(crate) type FooterKey = (&'static str, AnyElement, bool);

pub(crate) fn keys_footer(keys: Vec<FooterKey>) -> impl IntoElement {
    div()
        .id("sidebar-keys")
        .flex_none()
        .flex()
        .flex_wrap()
        .gap_x(px(14.))
        .text_size(px(11.))
        .line_height(px(20.))
        .text_color(c(theme::faint()))
        .children(keys.into_iter().map(|(k, what, lit)| {
            div()
                .flex()
                .gap(px(5.))
                .when(lit, |d| d.text_color(c(theme::accent())))
                .child(div().text_color(c(if lit { theme::accent() } else { theme::muted() })).child(crate::os_text::key(k)))
                .child(what)
        }))
        .test_support()
}

impl Workspace {
    pub(crate) fn page_groups(&self, cx: &App) -> Vec<VisibleGroup> {
        self.model.read(cx).doc.visible_groups()
    }

    pub(crate) fn ordered_groups(&self, cx: &App) -> Vec<VisibleGroup> {
        let mut g = self.page_groups(cx);
        g.sort_by_key(|g| (g.range.start, std::cmp::Reverse(g.range.end)));
        g
    }

    fn ordered_ids(&self, cx: &App) -> Vec<GroupId> {
        self.ordered_groups(cx).into_iter().map(|g| g.id).collect()
    }

    /// Display chars.
    fn block_at(&self, c: usize, cx: &App) -> Range<usize> {
        self.page.read(cx).index().paragraph_at(c)
    }

    fn ensure_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Writing hides groups, so opening the panel goes to editing.
        self.enter_editing(cx);
        self.make_room_for(crate::panels::PanelTab::Alternatives, window, cx);
        if self.sidebar.is_some() {
            self.end_variant_edit(true, cx);
            return;
        }
        let model = self.model.clone();
        let input = cx.new(|cx| TextView::new(Mode::Field, model, cx));
        self._subs.push(cx.subscribe_in(&input, window, Self::on_sidebar_input));
        self.sidebar = Some(Sidebar {
            group: None,
            cursor: None,
            list_focus: cx.focus_handle(),
            list_scroll: ScrollHandle::new(),
            reveal_cursor: false,
            input,
            rows: Rc::new(RefCell::new(Vec::new())),
            dance: None,
            note: None,
            editing: None,
            found: None,
            drop_at: None,
        });
    }

    pub fn open_sidebar(&mut self, group: Option<GroupId>, window: &mut Window, cx: &mut Context<Self>) {
        match group {
            Some(id) => self.open_group_view(id, window, cx),
            None => {
                let at = self.page.read(cx).caret_group();
                self.open_overview(at, window, cx)
            }
        }
    }

    pub(crate) fn open_group_view(&mut self, id: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_sidebar(window, cx);
        if let Some(s) = &mut self.sidebar {
            s.group = Some(id);
            s.cursor = Some(id);
            s.note = None;
        }
        self.sync_sidebar(cx);
        if let Some(s) = &self.sidebar {
            s.input.update(cx, |v, _| v.placeholder = tr!("add a version…").into());
            let f = s.input.read(cx).focus.clone();
            window.focus(&f, cx);
        }
        cx.notify();
    }

    pub(crate) fn open_overview(&mut self, cursor: Option<GroupId>, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_sidebar(window, cx);
        if let Some(s) = &mut self.sidebar {
            s.group = None;
            s.cursor = cursor;
            s.note = None;
            s.reveal_cursor = true;
        }
        self.sync_sidebar(cx);
        if let Some(s) = &self.sidebar {
            let f = s.list_focus.clone();
            window.focus(&f, cx);
        }
        cx.notify();
    }

    pub(crate) fn sidebar_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.sidebar.as_ref().map(|s| s.group) {
            Some(Some(id)) => self.open_overview(Some(id), window, cx),
            Some(None) => self.close_sidebar(window, cx),
            None => {}
        }
    }

    pub fn close_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_variant_edit(true, cx);
        self.sidebar = None;
        self.page.update(cx, |p, cx| {
            p.focus_range = None;
            p.spotlight = None;
            cx.notify();
        });
        // The project list under it on its side takes the keyboard back.
        let under = crate::panels::side_of(crate::panels::PanelTab::Project, cx) == crate::panels::side_of(crate::panels::PanelTab::Alternatives, cx);
        match self.project.as_ref().filter(|p| p.open && under) {
            Some(p) => window.focus(&p.focus.clone(), cx),
            None => window.focus(&self.page.read(cx).focus.clone(), cx),
        }
        cx.notify();
    }

    pub fn sync_sidebar(&mut self, cx: &mut Context<Self>) {
        if self.sidebar.is_none() {
            return;
        }
        self.page.update(cx, |p, cx| p.refresh(cx));
        let groups = self.page_groups(cx);
        let find = |id: Option<GroupId>| id.and_then(|id| groups.iter().find(|g| g.id == id));
        let Some(s) = &self.sidebar else { return };
        let group = find(s.group).map(|g| g.id);
        let cursor = find(s.cursor).map(|g| g.id);
        let focus = find(group).map(|g| {
            let block = self.block_at(g.range.start, cx);
            g.range.start.min(block.start)..g.range.end.max(block.end)
        });
        if let Some(s) = &mut self.sidebar {
            s.group = group;
            s.cursor = cursor;
        }
        self.page.update(cx, |p, cx| {
            p.focus_range = focus;
            p.spotlight = group.or(cursor);
            cx.notify();
        });
    }

    fn centre_group(&mut self, id: GroupId, cx: &mut Context<Self>) {
        let Some(r) = self.page_groups(cx).into_iter().find(|g| g.id == id).map(|g| g.range) else { return };
        self.page.update(cx, |p, cx| p.centre_on(r, cx));
    }

    fn move_list_cursor(&mut self, delta: isize, cx: &mut Context<Self>) {
        let ids = self.ordered_ids(cx);
        let Some(s) = &mut self.sidebar else { return };
        if ids.is_empty() {
            return;
        }
        let at = s.cursor.and_then(|c| ids.iter().position(|id| *id == c));
        let next = match at {
            None if delta > 0 => 0,
            None => ids.len() - 1,
            Some(i) => i.saturating_add_signed(delta).min(ids.len() - 1),
        };
        s.cursor = Some(ids[next]);
        s.reveal_cursor = true;
        self.sync_sidebar(cx);
        self.centre_group(ids[next], cx);
        cx.notify();
    }

    fn alt_list_up(&mut self, _: &AltListUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_list_cursor(-1, cx);
    }

    fn alt_list_down(&mut self, _: &AltListDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_list_cursor(1, cx);
    }

    fn alt_list_open(&mut self, _: &AltListOpen, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.sidebar.as_ref().and_then(|s| s.cursor) {
            self.open_from_list(id, window, cx);
        }
    }

    fn alt_list_back(&mut self, _: &AltListBack, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_back(window, cx);
    }

    pub(crate) fn open_from_list(&mut self, id: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        self.open_group_view(id, window, cx);
        self.centre_group(id, cx);
    }

    fn step_group(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.sidebar_group() else { return };
        let ids = self.ordered_ids(cx);
        let Some(i) = ids.iter().position(|id| *id == current) else { return };
        let n = ids.len() as isize;
        let j = (i as isize + delta).rem_euclid(n) as usize;
        self.open_from_list(ids[j], window, cx);
    }

    pub fn sidebar_prev_group(&mut self, _: &SidebarPrevGroup, w: &mut Window, cx: &mut Context<Self>) {
        self.step_group(-1, w, cx);
    }

    pub fn sidebar_next_group(&mut self, _: &SidebarNextGroup, w: &mut Window, cx: &mut Context<Self>) {
        self.step_group(1, w, cx);
    }

    pub fn all_alternatives(&mut self, _: &AllAlternatives, w: &mut Window, cx: &mut Context<Self>) {
        let at = self.page.read(cx).caret_group();
        self.open_overview(at, w, cx);
    }

    pub fn alternatives_for_selection(&mut self, _: &AlternativesForSelection, window: &mut Window, cx: &mut Context<Self>) {
        self.page.update(cx, |v, cx| v.hide_selection_buttons(cx));
        let page = self.page.read(cx);
        let sel = page.selection();
        if sel.is_empty() {
            let g = page
                .hovered_group
                .filter(|_| self.model.read(cx).layer)
                .or(page.caret_group())
                .filter(|id| page.groups().iter().any(|g| g.id == *id));
            match g {
                Some(id) => self.open_group_view(id, window, cx),
                None => self.open_overview(None, window, cx),
            }
            return;
        }
        let idx = page.index().clone();
        // Trim surrounding whitespace so a double-click-and-drag doesn't grab the spaces.
        let text = idx.slice(sel.clone()).to_string();
        let lead = text.chars().take_while(|c| c.is_whitespace()).count();
        let trail = text.chars().rev().take_while(|c| c.is_whitespace()).count();
        let mut range = sel.start + lead..sel.end.saturating_sub(trail).max(sel.start + lead);
        if range.is_empty() {
            return;
        }
        let block = idx.paragraph_at(range.start);
        let line = idx.line_of(range.start);
        let style = markdown::style_line(idx.line_str(line));
        let heading = matches!(style.kind, BlockKind::Heading(_));
        let whole = range == block;
            // Exactly one whole sentence is a sentence however short (“He left.”).
        let scope = if !whole && !heading && crate::sentence_keys::is_sentence(&idx, &range) {
            Scope::Sentence
        } else {
            detect_scope(idx.slice(range.clone()), whole, heading)
        };
        if heading {
            // Keep the `# ` marker outside the group so every variant stays a heading.
            let marker_end = idx.lines[line].char_start + idx.line_str(line)[..style.hidden_prefix].chars().count();
            range.start = range.start.max(marker_end);
        }
        let r = self.model.update(cx, |m, cx| m.create_group(cx, range.clone(), scope));
        match r {
            Ok(id) | Err(EditError::AlreadyGroup(id)) => self.open_group_view(id, window, cx),
            Err(EditError::PartialOverlap) => {
                self.flash_status(tr!("Overlaps another group"), cx);
            }
            Err(e) => self.flash_status(e.to_string(), cx),
        }
    }

    pub(crate) fn open_group(&mut self, id: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.read(cx).groups().iter().any(|g| g.id == id) {
            self.open_group_view(id, window, cx);
        }
    }

    pub(crate) fn sidebar_group(&self) -> Option<GroupId> {
        self.sidebar.as_ref().and_then(|s| s.group)
    }

    fn apply_variant(&mut self, id: GroupId, index: usize, cx: &mut Context<Self>) {
        let r = self.model.update(cx, |m, cx| m.set_active(cx, id, index));
        if let Some(r) = r {
            self.page.update(cx, |p, cx| p.after_cycle(id, r.is_original, cx));
        }
    }

    fn on_sidebar_input(&mut self, input: &Entity<TextView>, e: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            ViewEvent::Tab => {
                let text = input.read(cx).field_text().to_string();
                let Some(typed) = ideas_trigger(&text) else { return };
                let Some(scope) = self.sidebar_group().and_then(|id| self.model.read(cx).doc.group(id)).map(|g| g.scope) else { return };
                if !crate::assist::on(cx) {
                    return;
                }
                let Some(steer) = next_steer(typed, &steer_candidates(scope, cx)) else { return };
                // The steer is selected, so typing replaces it and ⇥ goes on to the next.
                input.update(cx, |i, cx| {
                    i.set_field_text(&format!("?? {steer}"), cx);
                    i.select(3..3 + steer.chars().count(), cx);
                });
                cx.notify();
            }
            ViewEvent::Confirm => {
                let text = input.read(cx).field_text().trim().to_string();
                if text.is_empty() {
                    let active = self.sidebar_group().and_then(|id| Some((id, self.model.read(cx).doc.group(id)?.active)));
                    if let Some((id, i)) = active {
                        self.edit_variant_in_sidebar(id, i, window, cx);
                    }
                    return;
                }
                if let Some(steer) = ideas_trigger(&text) {
                    if !crate::assist::on(cx) {
                        return self.flash_status(tr!("AI is off (Preferences → AI)"), cx);
                    }
                    let steer = steer.to_string();
                    if let Some(s) = &mut self.sidebar {
                        s.dance = Some(Instant::now());
                        s.note = None;
                    }
                    input.update(cx, |i, cx| i.set_field_text("", cx));
                    match self.sidebar_group() {
                        Some(id) => self.start_ideas(id, (!steer.is_empty()).then_some(steer), cx),
                        None => self.flash_status(tr!("Open a group first"), cx),
                    }
                    cx.notify();
                    return;
                }
                let Some(id) = self.sidebar_group() else {
                    self.flash_status(tr!("Select text first"), cx);
                    return;
                };
                self.add_to_group(id, &text, cx);
                input.update(cx, |i, cx| i.set_field_text("", cx));
            }
            ViewEvent::Arrow(d) => {
                if let Some(id) = self.sidebar_group() {
                    self.page.update(cx, |p, cx| p.cycle(id, *d, cx));
                }
            }
            ViewEvent::DeleteLineEmpty => self.remove_active_variant(window, cx),
            ViewEvent::History { redo } => {
                // The field still has focus right after a removal, when "⌘Z to undo" is offered.
                let redo = *redo;
                self.page.update(cx, |p, cx| if redo { p.redo(cx) } else { p.undo(cx) });
                self.sync_sidebar(cx);
            }
            ViewEvent::Cancel => self.sidebar_back(window, cx),
            ViewEvent::Edited => cx.notify(),
            _ => {}
        }
    }

    pub fn edit_variant_in_sidebar(&mut self, id: GroupId, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_group() != Some(id) {
            return;
        }
        let Some(v) = self.model.read(cx).doc.group(id).and_then(|g| g.variants.get(index).cloned()) else { return };
        // A version without the words has no text of its own to edit.
        if v.cut {
            return;
        }
        self.end_variant_edit(true, cx);
        let model = self.model.clone();
        let field = cx.new(|cx| {
            let mut f = TextView::new(Mode::Field, model, cx);
            f.set_field_text(&v.text, cx);
            f
        });
        let focus = field.read(cx).focus.clone();
        let sub = cx.subscribe_in(&field, window, Self::on_variant_field);
        if let Some(s) = &mut self.sidebar {
            s.editing = Some(VariantEdit { group: id, variant: v.id, field, _sub: sub });
        }
        window.focus(&focus, cx);
        cx.notify();
    }

    fn on_variant_field(&mut self, _: &Entity<TextView>, e: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        let save = match e {
            ViewEvent::Confirm => true,
            ViewEvent::Cancel => false,
            ViewEvent::Edited => return cx.notify(),
            _ => return,
        };
        self.end_variant_edit(save, cx);
        if let Some(s) = &self.sidebar {
            let f = s.input.read(cx).focus.clone();
            window.focus(&f, cx);
        }
    }

    /// Clicking outside the sidebar saves too (the workspace's mouse-down).
    pub(crate) fn end_variant_edit(&mut self, save: bool, cx: &mut Context<Self>) {
        let Some(edit) = self.sidebar.as_mut().and_then(|s| s.editing.take()) else { return };
        cx.notify();
        let text = edit.field.read(cx).field_text().trim().to_string();
        let index = self
            .model
            .read(cx)
            .doc
            .group(edit.group)
            .and_then(|g| g.variants.iter().position(|v| v.id == edit.variant));
        let (true, false, Some(index)) = (save, text.is_empty(), index) else { return };
        self.model.update(cx, |m, cx| {
            let _ = m.edit(cx, |d| d.edit_variant(edit.group, index, &text));
        });
    }

    fn add_to_group(&mut self, id: GroupId, text: &str, cx: &mut Context<Self>) {
        let Some(g) = self.model.read(cx).doc.group(id) else { return };
        let original = g.variants[0].text.clone();
        let r = self.model.update(cx, |m, cx| m.add_variant_and_show(cx, id, text));
        if let Some(r) = r {
            self.page.update(cx, |p, cx| p.after_cycle(id, r.is_original, cx));
            if g.scope == Scope::Word {
                crate::memory::Memory::record(cx, &original, text);
            }
        }
    }

    /// Adds the version without the words and shows it.
    pub(crate) fn add_cut_version(&mut self, id: GroupId, cx: &mut Context<Self>) {
        let added = self.model.update(cx, |m, cx| m.edit(cx, |d| d.add_cut_version_and_show(id, doc_model::Source::Me)).is_ok());
        if added {
            self.page.update(cx, |p, cx| p.after_cycle(id, false, cx));
        }
    }

    pub fn remove_variant_at(&mut self, id: GroupId, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.end_variant_edit(true, cx);
        let origin = self
            .sidebar
            .as_ref()
            .and_then(|s| s.rows.borrow().get(index).copied())
            .map(|b| point(b.left() + px(60.), b.center().y));
        let r = self.model.update(cx, |m, cx| m.remove_variant(cx, id, index));
        let Some(r) = r else { return };
        let mode = Prefs::get(cx).motion();
        if let Some(o) = origin {
            self.overlay.explode(o, r.text.chars().take(40).collect::<String>(), mode);
        }
        if r.dissolved {
            self.flash_status(tr!("Removed. ⌘Z to undo"), cx);
        } else if index == 0 {
            self.flash_status(tr!("Original removed. ⌘Z to undo"), cx);
        }
        self.sync_sidebar(cx);
        if self.sidebar.as_ref().is_some_and(|s| s.group.is_none()) {
            // The field is gone: give the keyboard back to the page so Esc and ⌘Z keep working.
            let f = self.page.read(cx).focus.clone();
            window.focus(&f, cx);
        }
        cx.notify();
    }

    fn remove_active_variant(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.sidebar_group() else { return };
        let Some(g) = self.model.read(cx).doc.group(id) else { return };
        self.remove_variant_at(id, g.active, window, cx);
    }

    pub fn settle_on_this(&mut self, _: &SettleOnThis, window: &mut Window, cx: &mut Context<Self>) {
        let in_field = self.sidebar.as_ref().is_some_and(|s| s.input.read(cx).focus.is_focused(window) || s.editing.is_some());
        // Settling while a variant is open for editing settles on the edited text.
        if self.sidebar.as_ref().is_some_and(|s| s.editing.is_some()) {
            self.end_variant_edit(true, cx);
        }
        let page = self.page.read(cx);
        let id = self
            .sidebar_group()
            .or(page.caret_group())
            .or(page.hovered_group);
        let Some(id) = id else {
            self.flash_status(tr!("Nothing to settle here"), cx);
            return;
        };
        self.settle_group_from(id, in_field, window, cx);
    }

    pub fn settle_group(&mut self, id: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar.as_ref().is_some_and(|s| s.editing.is_some()) {
            self.end_variant_edit(true, cx);
        }
        self.settle_group_from(id, false, window, cx);
    }

    fn settle_group_from(&mut self, id: GroupId, in_field: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.update(cx, |m, cx| m.edit(cx, |d| d.settle(id)).is_ok()) {
            self.flash_status(tr!("Settled. ⌘Z to undo"), cx);
            if let Some(s) = self.sidebar.as_mut().filter(|s| s.group == Some(id)) {
                s.group = None;
            }
            self.sync_sidebar(cx);
            if let Some(f) = self.sidebar.as_ref().filter(|_| in_field).map(|s| s.list_focus.clone()) {
                window.focus(&f, cx);
            }
        }
    }

    pub fn render_sidebar(&mut self, side: crate::panels::Side, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let s = self.sidebar.as_ref()?;
        let info = s.group.and_then(|id| self.model.read(cx).doc.group(id));
        let switch = self.tab_strip(side, cx);
        let (body, keys) = match info {
            Some(g) => self.render_versions(g, window, cx),
            None => self.render_overview(cx),
        };
        let foot = self.foot_here(side, cx).then(|| self.render_panel_foot(cx));
        let el = crate::panels::frame(div().id("sidebar").test_support(), side, cx)
            .flex()
            .flex_col()
            .pt(px(12.))
            .px(px(20.))
            .pb(px(if foot.is_some() { crate::panel_foot::PANEL_PB } else { 20. }))
            .gap_3()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(crate::workspace::resize_handle(side.edge()))
            .child(switch)
            .child(body)
            .child(keys_footer(keys))
            .children(foot);
        Some(el.into_any_element())
    }

    fn render_overview(&mut self, cx: &mut Context<Self>) -> (AnyElement, Vec<FooterKey>) {
        let groups = self.ordered_groups(cx);
        let Some(s) = self.sidebar.as_mut() else { return (Empty.into_any_element(), Vec::new()) };
        let reveal = std::mem::take(&mut s.reveal_cursor);
        let (cursor, focus, scroll) = (s.cursor, s.list_focus.clone(), s.list_scroll.clone());
        let idx = self.page.read(cx).index().clone();
        let heads = headings(&idx);
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut cursor_row = None;
        let mut last: Option<String> = None;
        for (i, g) in groups.iter().enumerate() {
            let head = heads.iter().rev().find(|(at, _)| *at <= g.range.start).map(|(_, t)| t.clone());
            if let Some(h) = head.as_ref().filter(|_| head != last) {
                let first = rows.is_empty();
                rows.push(div().pt(px(if first { 0. } else { 10. })).pb(px(2.)).text_size(px(10.5)).text_color(c(theme::faint())).truncate().child(h.clone()).into_any_element());
            }
            last = head;
            let selected = cursor == Some(g.id);
            if selected {
                cursor_row = Some(rows.len());
            }
            let (text, hl) = match g.scope {
                Scope::Word => snippet(&idx, g.range.clone(), 28, 40),
                _ => snippet(&idx, g.range.clone(), 0, 80),
            };
            let (base, strong, meta) = if selected {
                (c(theme::text_2()), c(theme::text()), c(theme::muted()))
            } else {
                (ca(theme::muted(), 0.85), ca(theme::text_2(), 0.85), c(theme::faint()))
            };
            // A filled dot: a version other than the original is on the page.
            let bullet = if g.active != 0 {
                div().size(px(6.)).rounded_full().bg(c(theme::accent()))
            } else {
                div().size(px(7.)).rounded_full().border(px(1.2)).border_color(c(theme::muted()))
            };
            let id = g.id;
            rows.push(
                div()
                    .id(("alt-row", i))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .py(px(6.))
                    .cursor_pointer()
                    .aria_selected(selected)
                    .on_click(cx.listener(move |this, _, w, cx| this.open_from_list(id, w, cx)))
                    .child(div().w(px(10.)).h(px(18.)).flex().flex_none().items_center().justify_center().child(bullet))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(theme::NOTE_PX))
                                    .line_height(px(theme::NOTE_LINE))
                                    .text_color(base)
                                    .child(StyledText::new(text).with_highlights(vec![(hl, HighlightStyle { color: Some(strong), ..Default::default() })])),
                            )
                            .child(div().text_size(px(10.5)).line_height(px(14.)).text_color(meta).child(match g.scope {
                                Scope::Word => trn!("word · {n} version", "word · {n} versions", g.variant_count),
                                Scope::Sentence => trn!("sentence · {n} version", "sentence · {n} versions", g.variant_count),
                                Scope::Paragraph => trn!("paragraph · {n} version", "paragraph · {n} versions", g.variant_count),
                            })),
                    )
                    .test_support()
                    .into_any_element(),
            );
        }
        if reveal {
            if let Some(r) = cursor_row {
                scroll.scroll_to_item(r);
            }
        }
        let n = groups.len();
        let list = if n == 0 {
            div()
                .id("alt-empty")
                .text_size(px(12.5))
                .line_height(px(21.))
                .text_color(c(theme::muted()))
                .child(tr!("Select text, then ⇧⌘A."))
                .test_support()
                .into_any_element()
        } else {
            div().id("alt-list").flex().flex_col().flex_1().min_h_0().overflow_y_scroll().track_scroll(&scroll).children(rows).into_any_element()
        };
        let body = div()
            .id("alt-overview")
            .key_context("AltList")
            .track_focus(&focus)
            .on_action(cx.listener(Self::alt_list_up))
            .on_action(cx.listener(Self::alt_list_down))
            .on_action(cx.listener(Self::alt_list_open))
            .on_action(cx.listener(Self::alt_list_back))
            .on_action(cx.listener(|this, _: &Undo, _, cx| {
                this.page.update(cx, |p, cx| p.undo(cx));
                this.sync_sidebar(cx);
            }))
            .on_action(cx.listener(|this, _: &Redo, _, cx| {
                this.page.update(cx, |p, cx| p.redo(cx));
                this.sync_sidebar(cx);
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_3()
            .child(list)
            .test_support()
            .into_any_element();
        let text = |t: &'static str| div().child(t).into_any_element();
        let keys = vec![("↑↓", text(tr!("go to")), false), ("↩", text(tr!("open")), false)];
        (body, keys)
    }

    fn render_versions(&mut self, g: GroupInfo, window: &mut Window, cx: &mut Context<Self>) -> (AnyElement, Vec<FooterKey>) {
        let ids = self.ordered_ids(cx);
        let pos = ids.iter().position(|id| *id == g.id).map_or(0, |i| i + 1);
        let total = ids.len();
        let tab = g.scope;
        let gid = g.id;
        let context = self
            .page_groups(cx)
            .into_iter()
            .find(|v| v.id == gid)
            .filter(|_| tab != Scope::Paragraph)
            .map(|v| snippet(&self.page.read(cx).index().clone(), v.range, 48, 120));
        let Some(s) = self.sidebar.as_ref() else { return (Empty.into_any_element(), Vec::new()) };
        let rows = s.rows.clone();
        let dancing = s.dance.is_some_and(|t| t.elapsed() < Duration::from_millis(600));
        let dance_x = s.dance.map_or(0., |t| {
            let k = t.elapsed().as_secs_f32();
            if k < 0.6 { (k * 40.).sin() * 6. * (1. - k / 0.6) } else { 0. }
        });
        let note = s.note.clone();
        let input = s.input.clone();
        let add_field = s.input.clone();
        let editing = s.editing.as_ref().map(|e| (e.group, e.variant, e.field.clone()));
        let chips: Vec<String> = if g.scope == Scope::Word {
            let existing: Vec<String> = g.variants.iter().map(|v| v.text.clone()).collect();
            crate::memory::Memory::suggestions(cx, &g.variants[0].text, &existing)
        } else {
            Vec::new()
        };
        let gid_for_chips = Some(gid);
        let ideas = self.render_ideas(gid, cx);
        if dancing {
            window.request_animation_frame();
        }
        let info = Some(g);

        let glyph = |id: &'static str, t: &'static str, delta: isize| {
            let tip: Box<dyn Action> = if delta < 0 { Box::new(SidebarPrevGroup) } else { Box::new(SidebarNextGroup) };
            div()
                .id(id)
                .tooltip(crate::workspace::tip(if delta < 0 { tr!("Previous group") } else { tr!("Next group") }, tip))
                .px_1()
                .cursor_pointer()
                .text_color(c(theme::text_2()))
                .hover(|d| d.text_color(c(theme::text())))
                .on_click(cx.listener(move |this, _, w, cx| this.step_group(delta, w, cx)))
                .child(t)
                .test_support()
        };
        let crumb = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .text_size(px(11.5))
            .text_color(c(theme::muted()))
            .child(
                div()
                    .id("to-overview")
                    .flex()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_color(c(theme::text_2()))
                    .hover(|d| d.text_color(c(theme::text())))
                    .on_click(cx.listener(move |this, _, w, cx| this.open_overview(Some(gid), w, cx)))
                    .tooltip(|_, cx| cx.new(|_| crate::workspace::Tip::new(tr!("All alternatives"), Some("esc".into()))).into())
                    .child(div().text_color(c(theme::accent())).child("‹"))
                    .child(tr!("all alternatives"))
                    .test_support(),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(glyph("prev-group", "‹", -1))
                    .child({
                        let at = tr!("{at} of {n}", at = pos, n = total);
                        div().id("group-position").aria_label(at.clone()).child(at).test_support()
                    })
                    .child(glyph("next-group", "›", 1)),
            );
        let label = div().flex_none().font_family(theme::hand()).text_size(theme::hand_px(19.)).text_color(c(theme::accent())).child(scope_label(tab));
        let context = context.map(|(t, hl)| {
            div()
                .id("group-context")
                .flex_none()
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(ca(theme::muted(), 0.85))
                .child(StyledText::new(t).with_highlights(vec![(hl, HighlightStyle { color: Some(c(theme::text())), ..Default::default() })]))
                .test_support()
        });

        let found = self.sidebar.as_ref().and_then(|s| s.found);
        let drop_at = self.sidebar.as_ref().and_then(|s| s.drop_at);
        let n = info.as_ref().map(|g| g.variants.len()).unwrap_or(0);
        let list: AnyElement = match &info {
            Some(g) => {
                let gid = g.id;
                let active = g.active;
                let paragraph = tab == Scope::Paragraph;
                div()
                    .flex()
                    .flex_col()
                    .on_children_prepainted(move |b, _, _| *rows.borrow_mut() = b)
                    .on_drag_move(cx.listener(move |this, e: &DragMoveEvent<crate::drag::VariantDrag>, _, cx| {
                        let d = e.drag(cx);
                        let gap = e.bounds.contains(&e.event.position).then(|| {
                            let bounds = this.sidebar.as_ref().map(|s| s.rows.borrow().clone()).unwrap_or_default();
                            let n = bounds.len().max(1);
                            bounds.iter().filter(|b| e.event.position.y > b.center().y).count().clamp(1, n)
                        });
                        let same = this.sidebar.as_ref().is_some_and(|s| s.group == Some(d.group));
                        if let Some(s) = &mut this.sidebar {
                            if same && s.drop_at != gap {
                                s.drop_at = gap;
                                cx.notify();
                            } else if !same {
                                s.drop_at = None;
                            }
                        }
                    }))
                    .on_drop(cx.listener(|this, d: &crate::drag::VariantDrag, w, cx| this.drop_variant(d, w, cx)))
                    .children(g.variants.iter().enumerate().map(|(i, v)| {
                        let is_active = i == active;
                        let field = editing.as_ref().filter(|(g, vid, _)| *g == gid && *vid == v.id).map(|(.., f)| f.clone());
                        let mut text: String = v.text.clone();
                        if paragraph && text.chars().count() > 150 {
                            text = text.chars().take(150).collect::<String>() + "…";
                        }
                        let bullet = match v.by {
                            Source::Me if is_active => div().size(px(if i == 0 { 8. } else { 7. })).rounded_full().bg(c(theme::accent())).into_any_element(),
                            Source::Me if i == 0 => div().size(px(8.)).rounded_full().border(px(1.3)).border_color(c(theme::muted())).into_any_element(),
                            Source::Me => div().size(px(6.)).rounded_full().border(px(1.2)).border_color(c(theme::pen())).into_any_element(),
                            Source::Ai | Source::Import => crate::ideas::robot(is_active).into_any_element(),
                        };
                        let row = div()
                            .id(("variant", i))
                            .flex()
                            .relative()
                            .items_start()
                            .gap(px(5.))
                            .py(px(4.8))
                            .px(px(4.))
                            .rounded_md()
                            .when(found == Some((gid, i)), |d| d.bg(ca(theme::line(), 0.9)));
                        // ALT-23: the landing gap shows as an accent line on the row below it.
                        let row = row.when(drop_at == Some(i), |d| {
                            d.child(
                                div()
                                    .id(("variant-drop", i))
                                    .absolute()
                                    .left(px(2.))
                                    .right(px(2.))
                                    .top(px(-1.))
                                    .h(px(2.))
                                    .rounded(px(1.))
                                    .bg(c(theme::accent()))
                                    .test_support(),
                            )
                        });
                        let last_gap = i == n - 1 && drop_at == Some(n);
                        let row = row.when(last_gap, |d| {
                            d.child(
                                div()
                                    .id(("variant-drop", n))
                                    .absolute()
                                    .left(px(2.))
                                    .right(px(2.))
                                    .bottom(px(-1.))
                                    .h(px(2.))
                                    .rounded(px(1.))
                                    .bg(c(theme::accent()))
                                    .test_support(),
                            )
                        });
                        // The original keeps its slot; the menu's Make original changes it.
                        let drag = (i > 0)
                            .then(|| crate::drag::VariantDrag {
                                group: gid,
                                from: i,
                                preview: crate::drag::preview(&v.text),
                                words: english::word_count(&v.text),
                            })
                            .filter(|_| field.is_none());
                        let row = match field {
                            // The field takes the clicks: they place its caret.
                            Some(_) => row.bg(ca(theme::line(), 0.8)),
                            None => row
                                .cursor_pointer()
                                .hover(|d| d.bg(ca(theme::line(), 0.8)))
                                .when_some(drag, |d, drag| {
                                    d.on_drag(drag, |d: &crate::drag::VariantDrag, _, _, cx| cx.new(|_| d.clone()))
                                })
                                .on_click(cx.listener(move |this, e: &ClickEvent, w, cx| {
                                    if e.click_count() >= 2 {
                                        this.edit_variant_in_sidebar(gid, i, w, cx);
                                    } else {
                                        this.apply_variant(gid, i, cx);
                                    }
                                }))
                                .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                    this.open_variant_menu(gid, i, e.position, cx);
                                })),
                        };
                        row
                            .child(
                                div()
                                    .w(px(10.))
                                    .h(px(20.))
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .child(bullet),
                            )
                            .child(match field {
                                Some(f) => div().id("variant-field").flex_1().min_w_0().text_size(px(14.)).child(f).test_support().into_any_element(),
                                None => div()
                                    .id(("variant-text", i))
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(if paragraph { 12.5 } else { 14. }))
                                    .line_height(px(20.))
                                    .text_color(if is_active { c(theme::text()) } else { c(theme::muted()) })
                                    // A version without the words: the words, struck through.
                                    .when(v.cut, |d| d.line_through().aria_label(tr!("without “{words}”", words = text.clone())))
                                    .child(text)
                                    .test_support()
                                    .into_any_element(),
                            })
                            .when(i == 0, |d| {
                                d.child(div().id("original-label").flex_none().text_size(px(10.5)).pt(px(3.)).text_color(c(theme::original())).child(tr!("original")).test_support())
                            })
                    }))
                    .into_any_element()
            }
            None => Empty.into_any_element(),
        };

        let body = div()
            .id("versions")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_3()
            .child(crumb)
            .child(label)
            .children(context)
            .child(div().id("variants").flex().flex_col().gap_1().overflow_y_scroll().child(list))
            .children(ideas)
            .when(info.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .px(px(4.))
                        .ml(px(dance_x))
                        .child(div().w(px(10.)).flex().flex_none().justify_center().text_size(px(13.)).text_color(c(theme::faint())).child("+"))
                        .child(div().id("add-field").flex_1().min_w_0().text_size(px(14.)).child(input).test_support()),
                )
            })
            // Asking AI, in plain sight under the field (mockup selection-bar, P2).
            .when(info.is_some() && crate::assist::on(cx), |d| {
                let asking = self.ideas.as_ref().is_some_and(|r| r.group == gid && r.running);
                let keys = crate::context_menu::shortcut_label(&AiAlternatives, cx);
                d.child(
                    div()
                        .id("ideas-button")
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .h(px(32.))
                        .mx(px(4.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(c(theme::line()))
                        .bg(c(theme::bg_raised()))
                        .text_size(px(12.5))
                        .text_color(c(if asking { theme::muted() } else { theme::accent() }))
                        .when(!asking, |d| {
                            d.cursor_pointer()
                                .hover(|d| d.border_color(ca(theme::accent(), 0.6)))
                                .tooltip(crate::workspace::tip(tr!("AI ideas, or ?? in the field"), Box::new(AiAlternatives)))
                                .on_click(cx.listener(move |this, _, w, cx| this.ai_alternatives_for_group(gid, w, cx)))
                        })
                        .child(svg().path("chrome/robot.svg").size(px(13.)).flex_none().text_color(c(if asking { theme::muted() } else { theme::accent() })))
                        .child(if asking { tr!("Asking…") } else { tr!("Ask AI for ideas") })
                        .children(keys.filter(|_| !asking).map(|k| div().text_size(px(10.5)).text_color(c(theme::muted())).child(k)))
                        .test_support(),
                )
            })
            .when(!chips.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_2()
                        .text_size(px(11.5))
                        .child(div().text_color(c(theme::muted())).child(tr!("used before")))
                        .child(div().flex().flex_wrap().gap_1().children(chips.into_iter().enumerate().map(|(i, chip)| {
                            let text = chip.clone();
                            div()
                                .id(("chip", i))
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .border_1()
                                .border_dashed()
                                .border_color(c(theme::faint()))
                                .cursor_pointer()
                                .text_color(c(theme::text_2()))
                                .hover(|d| d.border_color(c(theme::accent())))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(id) = gid_for_chips {
                                        this.add_to_group(id, &text, cx);
                                    }
                                }))
                                .child(format!("+ {chip}"))
                                .test_support()
                        }))),
                )
            })
            .children(note.map(|n| div().px_2().text_size(px(12.)).text_color(c(theme::accent())).child(n)))
            .into_any_element();
        let text = |t: &'static str| div().child(t).into_any_element();
        let settle = div()
            .id("settle")
            .cursor_pointer()
            .hover(|d| d.text_color(c(theme::accent())))
            .on_click(cx.listener(|this, _, w, cx| this.settle_on_this(&SettleOnThis, w, cx)))
            .child(tr!("settle"))
            .test_support()
            .into_any_element();
        let ai = crate::assist::on(cx);
        let typed = add_field.read(cx).field_text().to_string();
        let ask = ideas_trigger(&typed);
        let middle: FooterKey = match (ai, typed.trim().is_empty(), ask) {
            (false, _, _) => ("↩", text(tr!("add")), false),
            (true, true, _) => (
                "??",
                div()
                    .id("ideas-key")
                    .cursor_pointer()
                    .hover(|d| d.text_color(c(theme::accent())))
                    .on_click(cx.listener(|this, _, w, cx| this.type_ideas_trigger(w, cx)))
                    .child(tr!("steer"))
                    .test_support()
                    .into_any_element(),
                false,
            ),
            (true, false, Some(_)) => {
                ("↩", div().id("enter-ideas").child(trn!("{n} idea", "{n} ideas", crate::ideas::per_ask(cx))).test_support().into_any_element(), true)
            }
            (true, false, None) => ("↩", div().id("enter-add").child(tr!("add")).test_support().into_any_element(), true),
        };
        let hint: SharedString = match ask {
            Some(steer) if ai => {
                let candidates = steer_candidates(tab, cx);
                if steer.is_empty() {
                    candidates.first().map(|c| if typed.ends_with(' ') { format!("{c} ⇥") } else { format!(" {c} ⇥") }).unwrap_or_default().into()
                } else if typed.ends_with(char::is_whitespace) || candidates.iter().any(|c| c.to_lowercase() == steer.to_lowercase()) {
                    SharedString::default()
                } else {
                    let low = steer.to_lowercase();
                    candidates
                        .iter()
                        .find(|c| c.to_lowercase().starts_with(&low) && c.chars().count() > steer.chars().count())
                        .map(|c| format!("{} ⇥", c.chars().skip(steer.chars().count()).collect::<String>()))
                        .unwrap_or_default()
                        .into()
                }
            }
            _ => SharedString::default(),
        };
        if add_field.read(cx).hint_after != hint {
            add_field.update(cx, |i, _| i.hint_after = hint);
        }
        let keys = vec![("↑↓", text(tr!("try")), false), middle, ("⌥⌘↩", settle, false)];
        (body, keys)
    }

    fn type_ideas_trigger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.sidebar.as_ref().map(|s| s.input.clone()) else { return };
        input.update(cx, |i, cx| i.set_field_text("?? ", cx));
        let f = input.read(cx).focus.clone();
        window.focus(&f, cx);
        cx.notify();
    }

    fn open_variant_menu(&mut self, id: GroupId, index: usize, at: Point<Pixels>, cx: &mut Context<Self>) {
        let row = self.model.read(cx).doc.group(id).and_then(|g| {
            let without = index == 0 && g.scope == Scope::Word && !g.ghosted && !g.variants.iter().any(|v| v.cut);
            g.variants.get(index).map(|v| (v.text.clone(), g.active == index, v.cut, without))
        });
        let Some((text, active, cut, without)) = row else { return };
        self.context_menu = Some(crate::context_menu::ContextMenu::variant(id, index, active, cut, without, text, at));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_scope, ideas_trigger, next_steer};
    use doc_model::Scope;

    #[test]
    fn question_marks_ask_for_ideas() {
        assert_eq!(ideas_trigger("??"), Some(""));
        let c: Vec<String> = ["more concrete", "plainer", "less formal"].map(String::from).to_vec();
        assert_eq!(next_steer("", &c).as_deref(), Some("more concrete"), "⇥ on a bare ?? takes the first");
        assert_eq!(next_steer("more concrete", &c).as_deref(), Some("plainer"), "⇥ again, the next");
        assert_eq!(next_steer("less formal", &c).as_deref(), Some("more concrete"), "and round");
        assert_eq!(next_steer("le", &c).as_deref(), Some("less formal"), "completes what is typed");
        assert_eq!(next_steer("one syllable", &c), None);
        assert_eq!(ideas_trigger(" ?? "), Some(""));
        assert_eq!(ideas_trigger("?? plainer"), Some("plainer"));
        assert_eq!(ideas_trigger("??plainer"), Some("plainer"));
        assert_eq!(ideas_trigger("?? one syllable "), Some("one syllable"));
        assert_eq!(ideas_trigger("toil"), None);
        assert_eq!(ideas_trigger("why??"), None);
        assert_eq!(ideas_trigger("?"), None);
    }

    #[test]
    fn scope_detection() {
        assert_eq!(detect_scope("friction", false, false), Scope::Word);
        assert_eq!(detect_scope("the other candidates", false, false), Scope::Word);
        assert_eq!(detect_scope("Editors have always known this.", false, false), Scope::Sentence);
        assert_eq!(detect_scope("one. two", false, false), Scope::Sentence);
        assert_eq!(detect_scope("Whole block.", true, false), Scope::Paragraph);
        assert_eq!(detect_scope("Title", true, true), Scope::Sentence);
    }
}
