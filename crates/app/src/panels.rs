//! The side panels' tabs (TECH-NOTES "Panels"). Every tab lives on a side, in an order the
//! writer sets by dragging its label from one header to the other or along its own; the
//! layout is one for the whole app, kept in Prefs. A side shows one tab at a time; the Project
//! tab stays open under another tab of its side, as it always has under Alternatives.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};
use serde::{Deserialize, Serialize};

use crate::prefs::Prefs;
use crate::theme::{self, c, ca};
use crate::workspace::{self, PanelEdge, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelTab {
    Project,
    Alternatives,
    Review,
    Cuts,
    Comments,
}

impl PanelTab {
    pub const ALL: [PanelTab; 5] = [PanelTab::Project, PanelTab::Alternatives, PanelTab::Review, PanelTab::Cuts, PanelTab::Comments];

    /// Its label's element id.
    pub fn id(self) -> &'static str {
        match self {
            PanelTab::Project => "switch-project",
            PanelTab::Alternatives => "switch-alternatives",
            PanelTab::Review => "switch-review",
            PanelTab::Cuts => "tab-cuts",
            PanelTab::Comments => "tab-comments",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Project => tr!("Project"),
            PanelTab::Alternatives => tr!("Alternatives"),
            PanelTab::Review => l10n::trc!("pass menu", "Review"),
            PanelTab::Cuts => tr!("Cuts"),
            PanelTab::Comments => tr!("Comments"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            PanelTab::Project => "project",
            PanelTab::Alternatives => "alternatives",
            PanelTab::Review => "review",
            PanelTab::Cuts => "cuts",
            PanelTab::Comments => "comments",
        }
    }

    /// Review is editing's: writing's headers leave it out.
    pub fn editing_only(self) -> bool {
        self == PanelTab::Review
    }

    /// Opening it goes to editing first.
    pub fn needs_editing(self) -> bool {
        matches!(self, PanelTab::Alternatives | PanelTab::Review)
    }

    /// Of two open tabs on a side, the lower shows.
    fn rank(self) -> u8 {
        match self {
            PanelTab::Review => 0,
            PanelTab::Alternatives => 1,
            PanelTab::Comments => 2,
            PanelTab::Cuts => 3,
            PanelTab::Project => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    pub(crate) fn i(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }

    /// Its resize handle.
    pub fn edge(self) -> PanelEdge {
        match self {
            Side::Left => PanelEdge::Sidebar,
            Side::Right => PanelEdge::Overflow,
        }
    }
}

/// Which side each tab lives on, in header order. Every tab is on exactly one side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SavedLayout")]
pub struct TabLayout {
    pub left: Vec<PanelTab>,
    pub right: Vec<PanelTab>,
}

impl Default for TabLayout {
    fn default() -> Self {
        TabLayout { left: vec![PanelTab::Project, PanelTab::Alternatives, PanelTab::Review], right: vec![PanelTab::Cuts, PanelTab::Comments] }
    }
}

/// Names, so a tab this version doesn't know (a newer one's) is dropped, not the whole prefs file.
#[derive(Default, Deserialize)]
#[serde(default)]
struct SavedLayout {
    left: Vec<String>,
    right: Vec<String>,
}

impl From<SavedLayout> for TabLayout {
    fn from(s: SavedLayout) -> Self {
        let known = |names: Vec<String>| names.iter().filter_map(|n| PanelTab::named(n)).collect();
        TabLayout { left: known(s.left), right: known(s.right) }.normalized()
    }
}

impl TabLayout {
    pub fn tabs(&self, side: Side) -> &[PanelTab] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn tabs_mut(&mut self, side: Side) -> &mut Vec<PanelTab> {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    pub fn side_of(&self, tab: PanelTab) -> Side {
        if self.right.contains(&tab) {
            Side::Right
        } else {
            Side::Left
        }
    }

    /// `tab` put on `side` before `before` (`None`, or a tab not there: last). Dropped on
    /// itself, it stays.
    pub fn moved(&self, tab: PanelTab, side: Side, before: Option<PanelTab>) -> TabLayout {
        let mut l = self.clone();
        if before == Some(tab) && l.side_of(tab) == side {
            return l;
        }
        l.left.retain(|t| *t != tab);
        l.right.retain(|t| *t != tab);
        let list = l.tabs_mut(side);
        let at = before.and_then(|b| list.iter().position(|t| *t == b)).unwrap_or(list.len());
        list.insert(at, tab);
        l
    }

    /// Each tab once; one missing goes back to the end of its usual side.
    fn normalized(mut self) -> TabLayout {
        let mut seen = Vec::new();
        for side in [Side::Left, Side::Right] {
            self.tabs_mut(side).retain(|t| {
                let first = !seen.contains(t);
                seen.push(*t);
                first
            });
        }
        let usual = TabLayout::default();
        for t in PanelTab::ALL {
            if !seen.contains(&t) {
                self.tabs_mut(usual.side_of(t)).push(t);
            }
        }
        self
    }
}

/// A tab's label carried to a header.
#[derive(Clone)]
pub struct TabDrag {
    pub tab: PanelTab,
    pub count: usize,
}

impl Render for TabDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(26.))
            .px(px(9.))
            .rounded(px(6.))
            .bg(c(theme::bg_raised()))
            .border_1()
            .border_color(ca(theme::accent(), 0.6))
            .shadow_lg()
            .font_family(theme::UI)
            .text_size(px(12.))
            .text_color(c(theme::accent()))
            .child(self.tab.label())
            .when(self.count > 0, |d| d.child(div().text_size(px(10.)).child(self.count.to_string())))
    }
}

/// Where a carried tab would land: a side, before a tab of it (`None`: last).
pub type TabTarget = (Side, Option<PanelTab>);

pub struct TabDragState {
    pub tab: PanelTab,
    pub target: Option<TabTarget>,
}

/// The labels each header drew last frame, in window coordinates.
#[derive(Default)]
pub struct TabGeometry {
    sides: [Vec<(PanelTab, Bounds<Pixels>)>; 2],
}

pub type SharedGeometry = Rc<RefCell<TabGeometry>>;

#[cfg(any(test, feature = "shots"))]
impl TabGeometry {
    /// Where a tab's label was drawn last frame (scripts aim at it).
    pub fn label(&self, tab: PanelTab) -> Option<Bounds<Pixels>> {
        self.sides.iter().flatten().find(|(t, _)| *t == tab).map(|(_, b)| *b)
    }
}

impl PanelTab {
    /// By its saved name (`"alternatives"`).
    pub fn named(name: &str) -> Option<PanelTab> {
        PanelTab::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// How far below its labels a header still takes a dropped tab.
const BAND_BELOW: f32 = 14.;

/// A panel on its side: under the header, its side's width, a line on its inner edge.
pub fn frame<E: Styled>(d: E, side: Side, cx: &App) -> E {
    let d = d.absolute().top(px(workspace::HEADER_H)).bottom_0().w(px(side_width(side, cx))).bg(theme::surface(theme::bg_panel())).border_color(c(theme::line()));
    match side {
        Side::Left => d.left_0().border_r_1(),
        Side::Right => d.right_0().border_l_1(),
    }
}

/// A header laid over the panel's top, for panels whose content starts level with the page.
pub fn strip_at_top(strip: AnyElement) -> Div {
    div().absolute().top(px(12.)).left(px(20.)).right(px(20.)).child(strip)
}

/// A side's width while it's open.
pub fn side_width(side: Side, cx: &App) -> f32 {
    match side {
        Side::Left => workspace::sidebar_width(cx),
        Side::Right => workspace::overflow_width(cx),
    }
}

pub fn layout(cx: &App) -> &TabLayout {
    &Prefs::get(cx).panel_tabs
}

pub fn side_of(tab: PanelTab, cx: &App) -> Side {
    layout(cx).side_of(tab)
}

impl Workspace {
    pub(crate) fn cuts_open(&self, cx: &App) -> bool {
        // Comments over Cuts on one side keeps the Cuts' state, as switching tabs always has.
        self.drawer.is_some() && !(self.comments_open && side_of(PanelTab::Cuts, cx) == side_of(PanelTab::Comments, cx))
    }

    pub(crate) fn tab_open(&self, tab: PanelTab, cx: &App) -> bool {
        match tab {
            PanelTab::Project => self.panel_open(),
            PanelTab::Alternatives => self.sidebar.is_some(),
            PanelTab::Review => self.lab.open && self.model.read(cx).layer,
            PanelTab::Cuts => self.cuts_open(cx),
            PanelTab::Comments => self.comments_open,
        }
    }

    /// What a side shows; `None`: it's closed.
    pub(crate) fn shown_tab(&self, side: Side, cx: &App) -> Option<PanelTab> {
        layout(cx).tabs(side).iter().copied().filter(|t| self.tab_open(*t, cx)).min_by_key(|t| t.rank())
    }

    pub(crate) fn side_open(&self, side: Side, cx: &App) -> bool {
        self.shown_tab(side, cx).is_some()
    }

    /// The width a side takes from the page: 0 while closed.
    pub(crate) fn side_room(&self, side: Side, cx: &App) -> f32 {
        if self.side_open(side, cx) {
            side_width(side, cx)
        } else {
            0.
        }
    }

    /// Where a side's panel starts, in window coordinates.
    pub(crate) fn side_x(&self, side: Side, window_w: f32, cx: &App) -> f32 {
        match side {
            Side::Left => 0.,
            Side::Right => window_w - side_width(side, cx),
        }
    }

    /// What a side's button would open: the tab it last showed, else the Project tab, else
    /// its first. Writing keeps to tabs that don't need editing.
    pub(crate) fn would_open(&self, side: Side, cx: &App) -> Option<PanelTab> {
        let tabs = layout(cx).tabs(side);
        let editing = self.model.read(cx).layer;
        let fits = |t: &PanelTab| editing || !t.needs_editing();
        // In a project the Project tab comes before a remembered Alternatives.
        let remembered = self.last_tab[side.i()]
            .filter(|t| tabs.contains(t) && fits(t))
            .filter(|t| !(*t == PanelTab::Alternatives && self.project.is_some() && tabs.contains(&PanelTab::Project)));
        remembered
            .or_else(|| tabs.contains(&PanelTab::Project).then_some(PanelTab::Project))
            .or_else(|| tabs.iter().copied().find(fits))
            .or_else(|| tabs.iter().copied().find(|t| editing || !t.editing_only()))
    }

    /// The count a tab's label carries.
    pub(crate) fn tab_count(&self, tab: PanelTab, cx: &App) -> usize {
        match tab {
            PanelTab::Project => 0,
            PanelTab::Alternatives => self.page_groups(cx).len(),
            PanelTab::Review => self.lab.open_total(),
            PanelTab::Cuts => self.cuts_count_for_tab(cx),
            PanelTab::Comments => self.comments_count(cx),
        }
    }

    /// A side's toolbar button counts the cuts or comments it would show.
    pub(crate) fn button_count(&self, side: Side, cx: &App) -> usize {
        match self.shown_tab(side, cx).or_else(|| self.would_open(side, cx)) {
            Some(t @ (PanelTab::Cuts | PanelTab::Comments)) => self.tab_count(t, cx),
            _ => 0,
        }
    }

    /// A side button's spoken label, its tooltip, and the action whose key the tooltip shows.
    pub(crate) fn side_button_label(&self, side: Side, cx: &App) -> ((String, String), Option<Box<dyn Action>>) {
        use crate::actions::{ToggleOverflow, ToggleProject};
        let tabs = layout(cx).tabs(side);
        if tabs.contains(&PanelTab::Project) {
            let l = if self.project.is_some() { tr!("Project list") } else { tr!("Project and alternatives") };
            return ((l.into(), l.into()), Some(Box::new(ToggleProject)));
        }
        let cuts_here = side_of(PanelTab::Cuts, cx) == side;
        match self.shown_tab(side, cx).or_else(|| self.would_open(side, cx)) {
            Some(PanelTab::Cuts) => {
                let n = self.tab_count(PanelTab::Cuts, cx);
                let (spoken, tip) = match n {
                    0 => (tr!("Cuts").to_string(), tr!("Cuts").to_string()),
                    n => (trn!("Cuts, {n} cut", "Cuts, {n} cuts", n), trn!("Cuts · {n} cut", "Cuts · {n} cuts", n)),
                };
                ((spoken, tip), Some(Box::new(ToggleOverflow)))
            }
            Some(t) => {
                let key: Option<Box<dyn Action>> = (t == PanelTab::Comments && cuts_here).then(|| Box::new(ToggleOverflow) as Box<dyn Action>);
                ((t.label().into(), t.label().into()), key)
            }
            None => ((tr!("Drag a tab here").into(), tr!("Drag a tab here").into()), None),
        }
    }

    /// Closes the other tabs of `tab`'s side, but the Project tab, which stays under it.
    pub(crate) fn make_room_for(&mut self, tab: PanelTab, window: &mut Window, cx: &mut Context<Self>) {
        let side = side_of(tab, cx);
        for other in layout(cx).tabs(side).to_vec() {
            if other == tab || !self.tab_open(other, cx) {
                continue;
            }
            match other {
                PanelTab::Project => {}
                PanelTab::Alternatives => self.close_sidebar(window, cx),
                PanelTab::Review => self.lab.open = false,
                // Comments shows over Cuts; anything else closes them.
                PanelTab::Cuts if tab == PanelTab::Comments => {}
                PanelTab::Cuts => self.drop_drawer(cx),
                PanelTab::Comments => self.comments_open = false,
            }
        }
    }

    fn drop_drawer(&mut self, cx: &mut Context<Self>) {
        self.drawer = None;
        self.page.update(cx, |p, cx| {
            p.focus_range = None;
            cx.notify();
        });
    }

    /// As a click on its label: the tab opens on its side, with the keyboard where that tab
    /// takes it.
    pub(crate) fn open_tab(&mut self, tab: PanelTab, window: &mut Window, cx: &mut Context<Self>) {
        match tab {
            PanelTab::Project => {
                if self.project.is_some() && !self.panel_open() {
                    self.toggle_project(&crate::actions::ToggleProject, window, cx);
                } else {
                    self.show_project_tab(window, cx);
                }
            }
            PanelTab::Alternatives => {
                let at = self.page.read(cx).caret_group();
                self.open_overview(at, window, cx);
            }
            PanelTab::Review => self.open_review(window, cx),
            PanelTab::Cuts => self.open_drawer(window, cx),
            PanelTab::Comments => self.show_comments(window, cx),
        }
        self.sync_comment_ticks(cx);
        cx.notify();
    }

    pub(crate) fn show_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.make_room_for(PanelTab::Comments, window, cx);
        // Over Cuts, the Cuts stay underneath: text carried from the page still lands there.
        if side_of(PanelTab::Cuts, cx) == side_of(PanelTab::Comments, cx) {
            self.ensure_drawer(cx);
        }
        self.comments_open = true;
        self.sync_comment_ticks(cx);
    }

    /// Every tab of a side closed; its button opens the one it showed.
    pub(crate) fn close_side(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = self.shown_tab(side, cx) {
            self.last_tab[side.i()] = Some(t);
        }
        for tab in layout(cx).tabs(side).to_vec() {
            match tab {
                PanelTab::Project if self.panel_open() => self.close_panel(window, cx),
                PanelTab::Alternatives if self.sidebar.is_some() => self.close_sidebar(window, cx),
                PanelTab::Review if self.lab.open => self.close_review(window, cx),
                PanelTab::Cuts if self.drawer.is_some() => {
                    self.drop_drawer(cx);
                    self.focus_page(window, cx);
                }
                PanelTab::Comments => self.comments_open = false,
                _ => {}
            }
        }
        self.sync_comment_ticks(cx);
        cx.notify();
    }

    /// A side's toolbar button.
    pub(crate) fn toggle_side(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        if self.side_open(side, cx) {
            return self.close_side(side, window, cx);
        }
        if let Some(t) = self.would_open(side, cx) {
            self.open_tab(t, window, cx);
        }
    }

    /// ⌃⌘C: Cuts, or Comments if that was last, on the Cuts' side.
    pub(crate) fn toggle_cuts_side(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let side = side_of(PanelTab::Cuts, cx);
        let ours = |t: &PanelTab| matches!(t, PanelTab::Cuts | PanelTab::Comments);
        if self.shown_tab(side, cx).is_some_and(|t| ours(&t)) {
            return self.close_side(side, window, cx);
        }
        let comments_here = side_of(PanelTab::Comments, cx) == side;
        let tab = match self.last_tab[side.i()] {
            Some(PanelTab::Comments) if comments_here => PanelTab::Comments,
            _ => PanelTab::Cuts,
        };
        self.open_tab(tab, window, cx);
    }

    /// The tab dropped at `side`, before `before`: it shows there. The side it left falls back
    /// to a neighbour that opens without changing modes.
    pub(crate) fn move_tab(&mut self, tab: PanelTab, side: Side, before: Option<PanelTab>, window: &mut Window, cx: &mut Context<Self>) {
        let old = layout(cx).clone();
        let from = old.side_of(tab);
        let was_shown = self.shown_tab(from, cx) == Some(tab);
        let hole = old.tabs(from).iter().position(|t| *t == tab).unwrap_or(0);
        let new = old.moved(tab, side, before);
        if new != old {
            crate::sheets::set_prefs(cx, |p| p.panel_tabs = new.clone());
        }
        let focused = window.focused(cx);
        self.open_tab(tab, window, cx);
        if from != side && was_shown && !self.side_open(from, cx) {
            let editing = self.model.read(cx).layer;
            let rest = new.tabs(from);
            let fits = |t: &&PanelTab| editing || !t.needs_editing();
            let next = rest.get(hole).filter(fits).or_else(|| hole.checked_sub(1).and_then(|i| rest.get(i)).filter(fits)).copied();
            if let Some(n) = next {
                self.open_tab(n, window, cx);
                // The keyboard stays with the tab that moved.
                if let Some(f) = focused {
                    window.focus(&f, cx);
                }
            }
        }
        self.settle_sides(window, cx);
        cx.notify();
    }

    pub(crate) fn reset_panel_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if *layout(cx) != TabLayout::default() {
            crate::sheets::set_prefs(cx, |p| p.panel_tabs = TabLayout::default());
        }
        self.settle_sides(window, cx);
        cx.notify();
    }

    /// After the layout changed, a side keeps only the tab it shows (and the Project tab under it).
    fn settle_sides(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for side in [Side::Left, Side::Right] {
            if let Some(t) = self.shown_tab(side, cx) {
                self.make_room_for(t, window, cx);
            }
        }
        self.sync_comment_ticks(cx);
    }

    /// A side's header: its tabs' labels, each one a drag handle.
    pub(crate) fn tab_strip(&self, side: Side, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.model.read(cx).layer;
        let tabs: Vec<PanelTab> = layout(cx).tabs(side).iter().copied().filter(|t| editing || !t.editing_only()).collect();
        let shown = self.shown_tab(side, cx);
        let carried = self.tab_drag.as_ref().map(|d| d.tab);
        let geometry = self.tab_geometry.clone();
        let order = tabs.clone();
        let labels: Vec<AnyElement> = tabs.iter().map(|t| self.tab_label(*t, shown == Some(*t), carried == Some(*t), cx)).collect();
        div()
            .on_children_prepainted(move |bounds, _, _| {
                geometry.borrow_mut().sides[side.i()] = order.iter().copied().zip(bounds).collect();
            })
            .id(match side {
                Side::Left => "left-switch",
                Side::Right => "right-switch",
            })
            .flex()
            .flex_none()
            .flex_wrap()
            .gap_x(px(if tabs.len() >= 3 { 14. } else { 18. }))
            .children(labels)
            .into_any_element()
    }

    fn tab_label(&self, tab: PanelTab, on: bool, carried: bool, cx: &mut Context<Self>) -> AnyElement {
        let label = tab.label();
        let n = self.tab_count(tab, cx);
        let count = (n > 0).then(|| {
            let d = div().text_size(px(10.)).child(n.to_string());
            match tab {
                PanelTab::Alternatives => d.id("alt-count").aria_label(trn!("{n} in this file", "{n} in this file", n)).test_support().into_any_element(),
                PanelTab::Review => d.id("review-count").aria_label(trn!("{n} open", "{n} open", n)).test_support().into_any_element(),
                _ => d.into_any_element(),
            }
        });
        div()
            .id(tab.id())
            .relative()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(28.))
            .role(Role::Tab)
            .aria_label(label)
            .aria_selected(on)
            .text_size(px(12.))
            .text_color(if on { c(theme::accent()) } else { c(theme::muted()) })
            .when(!on, |d| d.cursor_pointer().hover(|d| d.text_color(c(theme::text_2()))))
            .child(label)
            .children(count)
            .when(on, |d| d.child(div().absolute().left_0().right_0().bottom(px(1.)).h(px(2.)).rounded(px(2.)).bg(c(theme::accent()))))
            // Its place in the header while it's carried.
            .when(carried, |d| d.opacity(0.25))
            .when(!on, |d| d.on_click(cx.listener(move |this, _, w, cx| this.open_tab(tab, w, cx))))
            .on_drag(TabDrag { tab, count: n }, |d: &TabDrag, _, _, cx| cx.new(|_| d.clone()))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    let to = side_of(tab, cx).other();
                    this.context_menu = Some(crate::context_menu::ContextMenu::panel_tab(tab, to, e.position));
                    cx.notify();
                }),
            )
            .test_support()
            .into_any_element()
    }

    /// Where a tab carried to `p` would land: a header (its band under the toolbar), or
    /// anywhere over a closed side.
    fn tab_target(&self, tab: PanelTab, p: Point<Pixels>, window_w: f32, cx: &App) -> Option<TabTarget> {
        let (x, y) = (f32::from(p.x), f32::from(p.y));
        if y < workspace::HEADER_H {
            return None;
        }
        for side in [Side::Left, Side::Right] {
            let x0 = self.side_x(side, window_w, cx);
            if x < x0 || x >= x0 + side_width(side, cx) {
                continue;
            }
            if !self.side_open(side, cx) {
                return Some((side, None));
            }
            let geometry = self.tab_geometry.borrow();
            let labels: Vec<&(PanelTab, Bounds<Pixels>)> = geometry.sides[side.i()].iter().filter(|(t, _)| *t != tab).collect();
            let bottom = geometry.sides[side.i()].iter().map(|(_, b)| f32::from(b.bottom())).fold(workspace::HEADER_H + 40., f32::max);
            if y > bottom + BAND_BELOW {
                return None;
            }
            // Rows wrap: a label on a row below, or right of the pointer on its row, comes after.
            let before = labels.iter().find(|(_, b)| y < f32::from(b.top()) || (y < f32::from(b.bottom()) && x < f32::from(b.center().x))).map(|(t, _)| *t);
            return Some((side, before));
        }
        None
    }

    pub(crate) fn drag_tab_over(&mut self, tab: PanelTab, p: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let window_w = f32::from(window.viewport_size().width);
        let target = self.tab_target(tab, p, window_w, cx);
        let changed = self.tab_drag.as_ref().is_none_or(|d| d.tab != tab || d.target != target);
        if changed {
            self.tab_drag = Some(TabDragState { tab, target });
            cx.notify();
        }
    }

    pub(crate) fn drop_tab(&mut self, tab: PanelTab, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.tab_drag.take().and_then(|d| d.target);
        if let Some((side, before)) = target {
            self.move_tab(tab, side, before, window, cx);
        }
        cx.notify();
    }

    /// While a tab is carried: every place it can go, lit from the start, the one under it
    /// more, and where it would land.
    pub(crate) fn render_tab_targets(&self, window: &Window, cx: &App) -> Vec<AnyElement> {
        let Some(drag) = &self.tab_drag else { return Vec::new() };
        let window_w = f32::from(window.viewport_size().width);
        let geometry = self.tab_geometry.borrow();
        let mut out = Vec::new();
        for side in [Side::Left, Side::Right] {
            let hot = drag.target.is_some_and(|(s, _)| s == side);
            let (x0, w) = (self.side_x(side, window_w, cx), side_width(side, cx));
            let wash = |d: Div| {
                d.rounded(px(7.))
                    .border_1()
                    .when(!hot, |d| d.border_dashed())
                    .border_color(ca(theme::accent(), if hot { 0.7 } else { 0.45 }))
                    .bg(ca(theme::accent(), if hot { 0.14 } else { 0.07 }))
            };
            if !self.side_open(side, cx) {
                // A panel's ground under the tint, so the page doesn't show through.
                let zone = div().absolute().top(px(workspace::HEADER_H)).bottom_0().left(px(x0)).w(px(w)).bg(c(theme::bg_panel())).child(wash(div().size_full()));
                out.push(zone.id(("tab-drop-side", side.i())).test_support().into_any_element());
                continue;
            }
            let labels = &geometry.sides[side.i()];
            let Some(top) = labels.iter().map(|(_, b)| f32::from(b.top())).reduce(f32::min) else { continue };
            let bottom = labels.iter().map(|(_, b)| f32::from(b.bottom())).fold(top, f32::max);
            let band = div().absolute().left(px(x0 + 12.)).w(px(w - 24.)).top(px(top - 4.)).h(px(bottom - top + 6.));
            out.push(wash(band).id(("tab-drop-header", side.i())).test_support().into_any_element());
            if let Some((_, before)) = drag.target.filter(|(s, _)| *s == side) {
                let others: Vec<&(PanelTab, Bounds<Pixels>)> = labels.iter().filter(|(t, _)| *t != drag.tab).collect();
                let at = match before.and_then(|b| others.iter().find(|(t, _)| *t == b)) {
                    Some((_, b)) => Some((f32::from(b.left()) - 8., f32::from(b.top()))),
                    None => others.last().map(|(_, b)| (f32::from(b.right()) + 6., f32::from(b.top()))),
                };
                if let Some((x, y)) = at {
                    let bar = div().id("tab-drop-bar").absolute().left(px(x)).top(px(y + 4.)).w(px(2.)).h(px(20.)).rounded(px(1.)).bg(c(theme::accent())).test_support();
                    out.push(bar.into_any_element());
                }
            }
        }
        out
    }

    /// The panel a side shows, drawn by its tab.
    pub(crate) fn render_side(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tab = self.shown_tab(side, cx)?;
        // Text carried from the page goes to Cuts, over Comments on their side.
        let carrying = cx.has_active_drag() && self.page.read(cx).carrying.is_some();
        match tab {
            PanelTab::Comments if carrying && self.drawer.is_some() && side_of(PanelTab::Cuts, cx) == side => self.render_drawer(side, cx),
            PanelTab::Comments => Some(self.render_comments_panel(side, cx)),
            PanelTab::Cuts => self.render_drawer(side, cx),
            PanelTab::Review => self.render_review(side, window, cx),
            PanelTab::Alternatives => self.render_sidebar(side, window, cx),
            PanelTab::Project if self.project.is_some() => self.render_project(side, window, cx),
            PanelTab::Project => self.render_file_panel(side, cx),
        }
    }

    /// The panel foot (Open, Save, Finder, Export, Settings) goes with the Project tab.
    pub(crate) fn foot_here(&self, side: Side, cx: &App) -> bool {
        side_of(PanelTab::Project, cx) == side
    }
}

#[cfg(test)]
mod tests {
    use super::{PanelTab, Side, TabLayout};

    #[test]
    fn a_tab_moves_to_the_other_side_at_its_slot() {
        let l = TabLayout::default();
        let m = l.moved(PanelTab::Alternatives, Side::Right, Some(PanelTab::Comments));
        assert_eq!(m.left, vec![PanelTab::Project, PanelTab::Review]);
        assert_eq!(m.right, vec![PanelTab::Cuts, PanelTab::Alternatives, PanelTab::Comments]);
        assert_eq!(m.side_of(PanelTab::Alternatives), Side::Right);
        let first = l.moved(PanelTab::Comments, Side::Left, Some(PanelTab::Project));
        assert_eq!(first.left[0], PanelTab::Comments);
        let last = l.moved(PanelTab::Project, Side::Right, None);
        assert_eq!(last.right, vec![PanelTab::Cuts, PanelTab::Comments, PanelTab::Project]);
    }

    #[test]
    fn a_tab_reorders_along_its_own_side() {
        let l = TabLayout::default();
        let m = l.moved(PanelTab::Review, Side::Left, Some(PanelTab::Alternatives));
        assert_eq!(m.left, vec![PanelTab::Project, PanelTab::Review, PanelTab::Alternatives]);
        let back = m.moved(PanelTab::Project, Side::Left, None);
        assert_eq!(back.left, vec![PanelTab::Review, PanelTab::Alternatives, PanelTab::Project]);
        assert_eq!(l.moved(PanelTab::Review, Side::Left, Some(PanelTab::Review)), l, "dropped on itself");
    }

    #[test]
    fn a_saved_layout_keeps_every_tab_once() {
        let read = |json: &str| serde_json::from_str::<TabLayout>(json).unwrap();
        assert_eq!(read("{}"), TabLayout::default());
        let l = read(r#"{"left": ["cuts", "project", "cuts", "someday"], "right": ["project", "comments"]}"#);
        assert_eq!(l.left, vec![PanelTab::Cuts, PanelTab::Project, PanelTab::Alternatives, PanelTab::Review]);
        assert_eq!(l.right, vec![PanelTab::Comments]);
        let saved = serde_json::to_string(&TabLayout::default()).unwrap();
        assert_eq!(saved, r#"{"left":["project","alternatives","review"],"right":["cuts","comments"]}"#);
    }
}
