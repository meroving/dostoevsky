//! The menu bar off the Mac (dsky-fbso.9): GPUI draws no menu bar from `set_menus` on Windows
//! and Linux, so the header draws the same menus `actions::menus` gives the Mac, for this
//! window's project and mode. Items, their keys (Ctrl+S), checks and greyed-out items match the
//! Mac's. A click opens a menu and the pointer moves between them; Alt or F10 alone puts the
//! keys on the bar, Alt+letter opens a menu, arrows walk it, Enter runs an item, Esc steps back.
//! In a window too narrow for the names, one ≡ button holds them all.
//!
//! The UI font is monospaced, so every size here is a sum of characters: the menus' places are
//! known before they're drawn, which the submenus' places and the tests use.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::tr;

use crate::actions::*;
use crate::theme::{self, c, ca};
use crate::workspace::{Workspace, HEADER_H};

const TEXT: f32 = 12.;
const CHAR_W: f32 = TEXT * 0.6;
/// Either side of a menu's name.
const TITLE_PAD: f32 = 7.;
const TITLE_H: f32 = 26.;
/// Before the first name.
pub const BAR_LEFT: f32 = 6.;
/// Between the menus and the left panel's button.
pub const BAR_GAP: f32 = 8.;
/// The ≡ button that holds the menus in a narrow window.
const COLLAPSED_W: f32 = 30.;
/// The document's title keeps at least this much room beside the menus, or they fold into ≡.
pub const MIN_TITLE_ROOM: f32 = 16. * 12. * 0.6;

const ROW_H: f32 = 24.;
const SEP_H: f32 = 9.;
const PANEL_PAD: f32 = 4.;
const BORDER: f32 = 1.;
/// The check mark's column.
const CHECK_W: f32 = 24.;
const KEYS_GAP: f32 = 32.;
const ARROW_W: f32 = 18.;
const ROW_END: f32 = 12.;
/// Room for a character the font lacks, drawn from a wider face.
const ROW_SLACK: f32 = 8.;
const MIN_PANEL_W: f32 = 200.;
/// A menu's top edge, just under its name.
const PANEL_TOP: f32 = (HEADER_H + TITLE_H) / 2. + 1.;
/// A submenu overlaps its menu by this much.
const SUBMENU_OVERLAP: f32 = 3.;

fn text_w(s: &str) -> f32 {
    s.chars().count() as f32 * CHAR_W
}

pub fn title_w(title: &str) -> f32 {
    text_w(title) + 2. * TITLE_PAD
}

/// The bar's width: every name, or the ≡ button alone.
pub fn bar_width(titles: &[&str], collapsed: bool) -> f32 {
    if collapsed { COLLAPSED_W } else { titles.iter().map(|t| title_w(t)).sum() }
}

/// Each title's left edge in the window.
pub fn title_x(titles: &[&str], i: usize) -> f32 {
    BAR_LEFT + titles[..i].iter().map(|t| title_w(t)).sum::<f32>()
}

/// Each menu's access key: the first letter of its name no menu before it has and no Alt
/// binding already uses (`taken`), as an index into its characters.
pub fn mnemonics(titles: &[&str], taken: impl Fn(char) -> bool) -> Vec<Option<usize>> {
    let mut used: Vec<char> = Vec::new();
    titles
        .iter()
        .map(|t| {
            let found = t.chars().enumerate().find(|(_, ch)| {
                let l = ch.to_lowercase().next().unwrap_or(*ch);
                l.is_alphanumeric() && !used.contains(&l) && !taken(l)
            });
            found.map(|(i, ch)| {
                used.push(ch.to_lowercase().next().unwrap_or(ch));
                i
            })
        })
        .collect()
}

/// The letters Alt already presses something with, so no menu takes them.
fn alt_letters(cx: &App) -> Vec<char> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap
        .bindings()
        .filter_map(|b| match b.keystrokes() {
            [k] if k.modifiers().alt && !k.modifiers().control && !k.modifiers().platform && !k.modifiers().shift => {
                let mut chars = k.key().chars();
                let ch = chars.next()?;
                chars.next().is_none().then(|| ch.to_lowercase().next().unwrap_or(ch))
            }
            _ => None,
        })
        .collect()
}

/// One row of an open menu, as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub keys: Option<String>,
    pub checked: bool,
    pub enabled: bool,
    pub submenu: bool,
    pub separator: bool,
}

impl Row {
    fn height(&self) -> f32 {
        if self.separator { SEP_H } else { ROW_H }
    }

    fn width(&self) -> f32 {
        CHECK_W + text_w(&self.label) + self.keys.as_deref().map_or(0., |k| KEYS_GAP + text_w(k)) + if self.submenu { ARROW_W } else { 0. } + ROW_END + ROW_SLACK
    }
}

/// A menu's box and rows, where it's drawn.
#[derive(Clone, Debug)]
pub struct Panel {
    pub bounds: Bounds<Pixels>,
    pub rows: Vec<Row>,
    /// Taller than the window has room for: it scrolls.
    pub scrolls: bool,
}

pub fn panel_size(rows: &[Row]) -> Size<Pixels> {
    let w = rows.iter().map(Row::width).fold(MIN_PANEL_W, f32::max) + 2. * (PANEL_PAD + BORDER);
    let h = rows.iter().map(Row::height).sum::<f32>() + 2. * (PANEL_PAD + BORDER);
    size(px(w), px(h))
}

/// Where row `i` starts, from the panel's top.
pub fn row_top(rows: &[Row], i: usize) -> f32 {
    BORDER + PANEL_PAD + rows[..i].iter().map(Row::height).sum::<f32>()
}

/// A menu's box: under its name (`at` is the name's left edge) or beside its row in the menu
/// before, turned back to the left where the window ends, and moved up where it would run off
/// the bottom; taller than the window, it scrolls.
pub fn place(at: Point<Pixels>, beside: Option<Bounds<Pixels>>, size: Size<Pixels>, window: Size<Pixels>) -> (Bounds<Pixels>, bool) {
    let margin = px(4.);
    let mut origin = at;
    if origin.x + size.width > window.width - margin {
        origin.x = match beside {
            Some(parent) => parent.left() - size.width + px(SUBMENU_OVERLAP),
            None => window.width - margin - size.width,
        };
        origin.x = origin.x.max(margin);
    }
    let room = window.height - margin - px(PANEL_TOP);
    let scrolls = size.height > room;
    let height = size.height.min(room);
    if origin.y + height > window.height - margin {
        origin.y = (window.height - margin - height).max(px(PANEL_TOP));
    }
    (Bounds::new(origin, gpui_kit::size(size.width, height)), scrolls)
}

pub struct MenuBar {
    pub focus: FocusHandle,
    /// Where the keys go back to when the bar lets go, and where an item's action is sent.
    return_focus: Option<FocusHandle>,
    /// The menus as they were when the bar was opened.
    menus: Vec<OwnedMenu>,
    /// The bar shows one ≡ button whose menu holds the others.
    collapsed: bool,
    /// The menu whose name is lit: the open one, or the one the keys are on.
    top: Option<usize>,
    /// Each open menu's lit row: the top menu's first, then each submenu opened from it.
    levels: Vec<Option<usize>>,
    /// Opened from the keys: the access keys show.
    keyboard: bool,
    /// Alt is held alone: the access keys show.
    alt_down: bool,
    /// Alt was held for a click or a drag: letting it go doesn't light the bar.
    alt_spoiled: bool,
}

impl MenuBar {
    pub fn new(cx: &mut App) -> MenuBar {
        MenuBar {
            focus: cx.focus_handle(),
            return_focus: None,
            menus: Vec::new(),
            collapsed: false,
            top: None,
            levels: Vec::new(),
            keyboard: false,
            alt_down: false,
            alt_spoiled: false,
        }
    }

    /// A menu is open or the keys are on the bar.
    pub fn active(&self) -> bool {
        self.top.is_some()
    }

    pub fn open(&self) -> bool {
        self.top.is_some() && !self.levels.is_empty()
    }

    /// The menu at `level`: the top one, then each submenu its lit rows opened.
    fn menu_at(&self, level: usize) -> Option<&OwnedMenu> {
        let mut menu = self.menus.get(self.top?)?;
        for row in self.levels.iter().take(level) {
            match menu.items.get((*row)?)? {
                OwnedMenuItem::Submenu(m) => menu = m,
                _ => return None,
            }
        }
        Some(menu)
    }

    fn items_at(&self, level: usize) -> &[OwnedMenuItem] {
        self.menu_at(level).map(|m| m.items.as_slice()).unwrap_or(&[])
    }

    fn deepest(&self) -> usize {
        self.levels.len().saturating_sub(1)
    }

    /// The lit menu and each open menu's lit row, for tests.
    #[cfg(all(test, not(target_os = "macos")))]
    pub fn shown(&self) -> (Option<usize>, Vec<Option<usize>>) {
        (self.top, self.levels.clone())
    }
}

/// The next row that isn't a separator, `step` rows on (wrapping).
fn step(items: &[OwnedMenuItem], from: Option<usize>, step: isize) -> Option<usize> {
    let n = items.len() as isize;
    if n == 0 {
        return None;
    }
    let mut i = match from {
        Some(i) => i as isize,
        None if step > 0 => -1,
        None => n,
    };
    for _ in 0..n {
        i = (i + step).rem_euclid(n);
        if !matches!(items[i as usize], OwnedMenuItem::Separator | OwnedMenuItem::SystemMenu(_)) {
            return Some(i as usize);
        }
    }
    None
}

fn label_of(item: &OwnedMenuItem) -> &str {
    match item {
        OwnedMenuItem::Action { name, .. } => name,
        OwnedMenuItem::Submenu(m) => &m.name,
        _ => "",
    }
}

impl Workspace {
    /// The menus for this window: its project's and its mode's, folded under ≡ when collapsed.
    fn menu_snapshot(&self, collapsed: bool, cx: &App) -> Vec<OwnedMenu> {
        let menus: Vec<OwnedMenu> =
            crate::actions::menus(&crate::files::recent(), crate::prefs::Prefs::get(cx), self.project_kind(), self.model.read(cx).layer).into_iter().map(Menu::owned).collect();
        if collapsed {
            vec![OwnedMenu { name: "".into(), items: menus.into_iter().map(OwnedMenuItem::Submenu).collect(), disabled: false }]
        } else {
            menus
        }
    }

    fn titles_collapsed(window: &Window) -> bool {
        crate::workspace::HeaderEdges::of(window).menus_collapsed()
    }

    /// Lights menu `top`, from the keys or the pointer; `open` drops its menu down with its
    /// first row lit (keys) or none (pointer).
    pub(crate) fn menu_bar_show(&mut self, top: usize, open: bool, keyboard: bool, window: &mut Window, cx: &mut Context<Self>) {
        let collapsed = Self::titles_collapsed(window);
        if !self.menu_bar.active() || self.menu_bar.collapsed != collapsed {
            self.menu_bar.menus = self.menu_snapshot(collapsed, cx);
            self.menu_bar.collapsed = collapsed;
        }
        if !self.menu_bar.focus.is_focused(window) {
            self.menu_bar.return_focus = window.focused(cx);
            window.focus(&self.menu_bar.focus, cx);
        }
        self.context_menu = None;
        self.menu_bar.top = Some(top.min(self.menu_bar.menus.len().saturating_sub(1)));
        self.menu_bar.keyboard = keyboard;
        self.menu_bar.levels = if open {
            let first = if keyboard { step(self.menu_bar.items_at(0), None, 1) } else { None };
            vec![first]
        } else {
            Vec::new()
        };
        cx.notify();
    }

    /// Closes every menu and lets go of the keys; `restore` gives them back to where they were.
    pub(crate) fn menu_bar_close(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.menu_bar.active() {
            return;
        }
        self.menu_bar.top = None;
        self.menu_bar.levels.clear();
        self.menu_bar.keyboard = false;
        let back = self.menu_bar.return_focus.take();
        if restore && self.menu_bar.focus.is_focused(window) {
            let to = back.unwrap_or_else(|| self.page.read(cx).focus.clone());
            window.focus(&to, cx);
        }
        cx.notify();
    }

    /// Alt or F10 alone: the keys go to the bar, or come back from it.
    pub(crate) fn show_menu_bar(&mut self, _: &ShowMenuBar, window: &mut Window, cx: &mut Context<Self>) {
        if cfg!(target_os = "macos") {
            return;
        }
        if std::mem::take(&mut self.menu_bar.alt_spoiled) {
            return;
        }
        if self.menu_bar.active() {
            self.menu_bar_close(true, window, cx);
        } else {
            self.menu_bar_show(0, false, true, window, cx);
        }
    }

    /// Access keys show while Alt is held alone; a click with it held is Alt's, not the bar's.
    pub(crate) fn menu_bar_modifiers(&mut self, e: &ModifiersChangedEvent, cx: &mut Context<Self>) {
        if cfg!(target_os = "macos") {
            return;
        }
        let m = e.modifiers;
        let alt_alone = m.alt && !m.control && !m.shift && !m.platform && !m.function;
        if alt_alone && !self.menu_bar.alt_down {
            self.menu_bar.alt_spoiled = false;
        }
        if alt_alone != self.menu_bar.alt_down {
            self.menu_bar.alt_down = alt_alone;
            cx.notify();
        }
    }

    /// Alt+letter opens the menu with that access key (no Alt binding uses those letters).
    pub(crate) fn menu_bar_key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if cfg!(target_os = "macos") {
            return;
        }
        let m = e.keystroke.modifiers;
        if !(m.alt && !m.control && !m.shift && !m.platform && !m.function) {
            return;
        }
        let Some(letter) = e.keystroke.key.chars().next().filter(|_| e.keystroke.key.chars().count() == 1) else { return };
        if let Some(i) = self.menu_bar_mnemonic(letter, window, cx) {
            cx.stop_propagation();
            self.menu_bar.alt_spoiled = true;
            self.menu_bar_show(i, true, true, window, cx);
        }
    }

    fn menu_bar_mnemonic(&self, letter: char, window: &Window, cx: &App) -> Option<usize> {
        let letter = letter.to_lowercase().next()?;
        let titles = crate::actions::menu_titles();
        if Self::titles_collapsed(window) {
            return None;
        }
        let taken = alt_letters(cx);
        let keys = mnemonics(&titles, |ch| taken.contains(&ch));
        titles.iter().zip(keys).position(|(t, k)| k.and_then(|k| t.chars().nth(k)).and_then(|ch| ch.to_lowercase().next()) == Some(letter))
    }

    /// A press outside the bar and its menus closes them, the keys going back first so the
    /// press lands as it would have.
    pub(crate) fn menu_bar_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.modifiers.alt {
            self.menu_bar.alt_spoiled = true;
        }
        if !self.menu_bar.active() {
            return;
        }
        let inside_bar = {
            let edges = crate::workspace::HeaderEdges::of(window);
            e.position.y < px(HEADER_H) && e.position.x >= px(BAR_LEFT) && e.position.x < px(edges.left - BAR_GAP)
        };
        let inside_panel = self.menu_panels(window, cx).iter().any(|p| p.bounds.contains(&e.position));
        if !inside_bar && !inside_panel {
            self.menu_bar_close(true, window, cx);
        }
    }

    /// Every open menu's box and rows.
    pub(crate) fn menu_panels(&self, window: &Window, cx: &App) -> Vec<Panel> {
        let bar = &self.menu_bar;
        let Some(top) = bar.top else { return Vec::new() };
        let target = bar.return_focus.clone().unwrap_or_else(|| self.page.read(cx).focus.clone());
        let titles = crate::actions::menu_titles();
        let viewport = window.viewport_size();
        let mut out: Vec<Panel> = Vec::new();
        for level in 0..bar.levels.len() {
            let rows: Vec<Row> = bar
                .items_at(level)
                .iter()
                .filter(|it| !matches!(it, OwnedMenuItem::SystemMenu(_)))
                .map(|it| match it {
                    OwnedMenuItem::Separator => Row { label: String::new(), keys: None, checked: false, enabled: false, submenu: false, separator: true },
                    OwnedMenuItem::Submenu(m) => Row { label: m.name.to_string(), keys: None, checked: false, enabled: !m.disabled, submenu: true, separator: false },
                    OwnedMenuItem::Action { name, action, checked, disabled, .. } => Row {
                        label: name.clone(),
                        keys: crate::context_menu::shortcut_label(action.as_ref(), cx),
                        checked: *checked,
                        enabled: !*disabled && (window.is_action_available_in(action.as_ref(), &target) || crate::is_app_action(action.as_ref())),
                        submenu: false,
                        separator: false,
                    },
                    OwnedMenuItem::SystemMenu(_) => unreachable!(),
                })
                .collect();
            let size = panel_size(&rows);
            let (at, beside) = match out.last() {
                None => {
                    let x = if bar.collapsed { BAR_LEFT } else { title_x(&titles, top) };
                    (point(px(x), px(PANEL_TOP)), None)
                }
                Some(parent) => {
                    let row = bar.levels[level - 1].unwrap_or(0);
                    let y = parent.bounds.top() + px(row_top(&parent.rows, row) - PANEL_PAD - BORDER);
                    (point(parent.bounds.right() - px(SUBMENU_OVERLAP), y), Some(parent.bounds))
                }
            };
            let (bounds, scrolls) = place(at, beside, size, viewport);
            out.push(Panel { bounds, rows, scrolls });
        }
        out
    }

    fn menu_left(&mut self, _: &MenuLeft, window: &mut Window, cx: &mut Context<Self>) {
        let bar = &mut self.menu_bar;
        if bar.levels.len() > 1 {
            bar.levels.pop();
            cx.notify();
            return;
        }
        let n = bar.menus.len().max(1);
        let to = (bar.top.unwrap_or(0) + n - 1) % n;
        let open = bar.open();
        self.menu_bar_show(to, open, true, window, cx);
    }

    fn menu_right(&mut self, _: &MenuRight, window: &mut Window, cx: &mut Context<Self>) {
        let bar = &mut self.menu_bar;
        if bar.open() {
            let level = bar.deepest();
            let row = bar.levels[level];
            if let Some(OwnedMenuItem::Submenu(m)) = row.and_then(|r| bar.items_at(level).get(r)) {
                if !m.disabled {
                    let first = step(&m.items, None, 1);
                    bar.levels.push(first);
                    cx.notify();
                    return;
                }
            }
        }
        let n = bar.menus.len().max(1);
        let to = (bar.top.unwrap_or(0) + 1) % n;
        let open = bar.open();
        self.menu_bar_show(to, open, true, window, cx);
    }

    fn menu_up(&mut self, _: &MenuUp, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_vertical(-1, window, cx);
    }

    fn menu_down(&mut self, _: &MenuDown, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_vertical(1, window, cx);
    }

    fn menu_vertical(&mut self, dir: isize, window: &mut Window, cx: &mut Context<Self>) {
        let bar = &mut self.menu_bar;
        let Some(top) = bar.top else { return };
        if !bar.open() {
            self.menu_bar_show(top, true, true, window, cx);
            if dir < 0 {
                let last = step(self.menu_bar.items_at(0), None, -1);
                self.menu_bar.levels = vec![last];
            }
            return;
        }
        bar.keyboard = true;
        let level = bar.deepest();
        let next = step(bar.items_at(level), bar.levels[level], dir);
        bar.levels[level] = next;
        cx.notify();
    }

    fn menu_confirm(&mut self, _: &MenuConfirm, window: &mut Window, cx: &mut Context<Self>) {
        let bar = &self.menu_bar;
        let Some(top) = bar.top else { return };
        if !bar.open() {
            self.menu_bar_show(top, true, true, window, cx);
            return;
        }
        let level = bar.deepest();
        if let Some(row) = bar.levels[level] {
            self.menu_bar_run(level, row, window, cx);
        }
    }

    fn menu_close(&mut self, _: &MenuClose, window: &mut Window, cx: &mut Context<Self>) {
        let bar = &mut self.menu_bar;
        if bar.levels.len() > 1 {
            bar.levels.pop();
        } else if bar.levels.len() == 1 {
            bar.levels.clear();
            bar.keyboard = true;
        } else {
            self.menu_bar_close(true, window, cx);
            return;
        }
        cx.notify();
    }

    /// A letter with the keys on the bar: the menu with that access key, or in an open menu
    /// the next row starting with it.
    fn menu_letter(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = e.keystroke.modifiers;
        if m.control || m.platform || m.function {
            return;
        }
        let key = e.keystroke.key_char.clone().unwrap_or_else(|| e.keystroke.key.clone());
        let mut chars = key.chars();
        let (Some(letter), None) = (chars.next(), chars.next()) else { return };
        if !letter.is_alphanumeric() {
            return;
        }
        cx.stop_propagation();
        let letter = letter.to_lowercase().next().unwrap_or(letter);
        if !self.menu_bar.open() {
            if let Some(i) = self.menu_bar_mnemonic(letter, window, cx) {
                self.menu_bar_show(i, true, true, window, cx);
            }
            return;
        }
        let bar = &mut self.menu_bar;
        let level = bar.deepest();
        let items = bar.items_at(level);
        let n = items.len();
        let from = bar.levels[level].map_or(0, |r| r + 1);
        let found = (0..n).map(|k| (from + k) % n).find(|&i| label_of(&items[i]).chars().next().and_then(|ch| ch.to_lowercase().next()) == Some(letter));
        if let Some(i) = found {
            bar.levels[level] = Some(i);
            bar.keyboard = true;
            cx.notify();
        }
    }

    /// Runs row `row` of the menu at `level`: an action goes to where the keys were, a submenu
    /// opens.
    fn menu_bar_run(&mut self, level: usize, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let panels = self.menu_panels(window, cx);
        let Some(r) = panels.get(level).and_then(|p| p.rows.get(row)) else { return };
        if !r.enabled || r.separator {
            return;
        }
        let item = self.menu_bar.items_at(level).get(row).cloned();
        match item {
            Some(OwnedMenuItem::Submenu(m)) => {
                self.menu_bar.levels.truncate(level + 1);
                self.menu_bar.levels[level] = Some(row);
                self.menu_bar.levels.push(step(&m.items, None, 1));
                cx.notify();
            }
            Some(OwnedMenuItem::Action { action, .. }) => {
                self.menu_bar_close(true, window, cx);
                // Opening a file looks at every window, this one too, which is mid-update here.
                window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
            }
            _ => {}
        }
    }

    fn menu_hover_row(&mut self, level: usize, row: usize, cx: &mut Context<Self>) {
        let bar = &mut self.menu_bar;
        if bar.levels.len() <= level || (bar.levels[level] == Some(row) && bar.levels.len() == level + 1 + usize::from(matches!(bar.items_at(level).get(row), Some(OwnedMenuItem::Submenu(_))))) {
            return;
        }
        bar.levels.truncate(level + 1);
        bar.levels[level] = Some(row);
        if let Some(OwnedMenuItem::Submenu(m)) = bar.items_at(level).get(row) {
            if !m.disabled {
                bar.levels.push(None);
            }
        }
        cx.notify();
    }

    fn menu_title_down(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu_bar.open() && self.menu_bar.top == Some(i) {
            self.menu_bar_close(true, window, cx);
        } else {
            self.menu_bar_show(i, true, false, window, cx);
        }
    }

    fn menu_title_hover(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu_bar.open() && self.menu_bar.top != Some(i) {
            let keyboard = self.menu_bar.keyboard;
            self.menu_bar_show(i, true, keyboard, window, cx);
        }
    }

    /// The bar in the header: each menu's name, or ≡.
    pub(crate) fn render_menu_bar(&mut self, collapsed: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // The keys left for somewhere else (a click on the page): the bar lets go.
        if self.menu_bar.active() && !self.menu_bar.focus.is_focused(window) {
            self.menu_bar.top = None;
            self.menu_bar.levels.clear();
            self.menu_bar.return_focus = None;
        }
        let titles = crate::actions::menu_titles();
        let bar = &self.menu_bar;
        let show_keys = bar.alt_down || (bar.active() && bar.keyboard);
        let keys = if show_keys && !collapsed {
            let taken = alt_letters(cx);
            mnemonics(&titles, |ch| taken.contains(&ch))
        } else {
            vec![None; titles.len()]
        };
        let lit = |i: usize| bar.top == Some(i);
        let open = bar.open();
        let title = |i: usize, label: AnyElement, w: f32| {
            let on = lit(i);
            div()
                .id(("menu-title", i))
                .role(Role::MenuItem)
                .occlude()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(w))
                .h(px(TITLE_H))
                .rounded(px(5.))
                .text_size(px(TEXT))
                .text_color(if on { c(theme::text_2()) } else { c(theme::muted()) })
                .when(on, |d| d.bg(c(theme::menu_hover())))
                .when(!on, |d| d.hover(|s| s.bg(ca(theme::wash(), 0.04)).text_color(c(theme::text_2()))))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.menu_title_down(i, window, cx);
                }))
                .on_mouse_move(cx.listener(move |this, _, window, cx| this.menu_title_hover(i, window, cx)))
                .child(label)
                .test_support()
        };
        let children: Vec<AnyElement> = if collapsed {
            vec![title(0, svg().path("chrome/menu.svg").size(px(16.)).text_color(if lit(0) { c(theme::text_2()) } else { c(theme::icon()) }).into_any_element(), COLLAPSED_W)
                .aria_label(tr!("Menu"))
                .aria_expanded(open)
                .into_any_element()]
        } else {
            titles
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let text: SharedString = t.to_string().into();
                    let label = match keys[i] {
                        Some(k) => {
                            let start = t.char_indices().nth(k).map_or(0, |(b, _)| b);
                            let end = t.char_indices().nth(k + 1).map_or(t.len(), |(b, _)| b);
                            let underline = HighlightStyle { underline: Some(UnderlineStyle { thickness: px(1.), ..Default::default() }), ..Default::default() };
                            StyledText::new(text.clone()).with_highlights([(start..end, underline)]).into_any_element()
                        }
                        None => StyledText::new(text.clone()).into_any_element(),
                    };
                    title(i, label, title_w(t)).aria_label(text).aria_expanded(open && lit(i)).into_any_element()
                })
                .collect()
        };
        div()
            .id("menu-bar")
            .key_context("MenuBar")
            .track_focus(&self.menu_bar.focus)
            .role(Role::MenuBar)
            .flex()
            .flex_none()
            .items_center()
            .h(px(HEADER_H))
            .ml(px(BAR_LEFT))
            .on_action(cx.listener(Self::menu_left))
            .on_action(cx.listener(Self::menu_right))
            .on_action(cx.listener(Self::menu_up))
            .on_action(cx.listener(Self::menu_down))
            .on_action(cx.listener(Self::menu_confirm))
            .on_action(cx.listener(Self::menu_close))
            .on_key_down(cx.listener(Self::menu_letter))
            .children(children)
            .test_support()
            .into_any_element()
    }

    /// The open menus, over everything else.
    pub(crate) fn render_menu_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.menu_bar.open() {
            return None;
        }
        let panels = self.menu_panels(window, cx);
        let levels = self.menu_bar.levels.clone();
        let boxes: Vec<AnyElement> = panels
            .into_iter()
            .enumerate()
            .map(|(level, panel)| {
                let lit = levels.get(level).copied().flatten();
                let rows: Vec<AnyElement> = panel
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        if r.separator {
                            return div().h(px(SEP_H)).flex().items_center().child(div().mx(px(8.)).flex_1().h(px(1.)).bg(c(theme::menu_sep()))).into_any_element();
                        }
                        let on = lit == Some(i);
                        let color = if r.enabled { c(theme::menu_text()) } else { c(theme::menu_disabled()) };
                        div()
                            .id(SharedString::from(format!("menu-row-{level}-{i}")))
                            .role(Role::MenuItem)
                            .aria_label(SharedString::from(r.label.clone()))
                            .when(r.checked, |d| d.aria_toggled(crate::workspace::toggled(true)))
                            .flex()
                            .items_center()
                            .h(px(ROW_H))
                            .pr(px(ROW_END))
                            .rounded(px(4.))
                            .text_color(color)
                            .when(on, |d| d.bg(c(theme::menu_hover())))
                            .when(r.enabled && !r.submenu, |d| d.cursor_pointer())
                            .on_mouse_move(cx.listener(move |this, _, _, cx| this.menu_hover_row(level, i, cx)))
                            .on_click(cx.listener(move |this, _, window, cx| this.menu_bar_run(level, i, window, cx)))
                            .child(div().w(px(CHECK_W)).flex_none().flex().justify_center().when(r.checked, |d| d.child(svg().path("chrome/check.svg").size(px(10.)).text_color(color))))
                            .child(div().flex_1().min_w_0().whitespace_nowrap().child(r.label.clone()))
                            .children(r.keys.clone().map(|k| div().flex_none().whitespace_nowrap().pl(px(KEYS_GAP)).text_color(if r.enabled { c(theme::menu_key()) } else { c(theme::menu_disabled()) }).child(k)))
                            .when(r.submenu, |d| d.child(div().w(px(ARROW_W)).flex_none().flex().justify_end().child(svg().path("chrome/chevron.svg").size(px(8.)).text_color(color))))
                            .test_support()
                            .into_any_element()
                    })
                    .collect();
                div()
                    .id(("menu-panel", level))
                    .role(Role::Menu)
                    .absolute()
                    .left(panel.bounds.left())
                    .top(panel.bounds.top())
                    .w(panel.bounds.size.width)
                    .h(panel.bounds.size.height)
                    .occlude()
                    .p(px(PANEL_PAD))
                    .rounded(px(7.))
                    .bg(c(theme::bg_raised()))
                    .border_1()
                    .border_color(c(theme::menu_border()))
                    .shadow(vec![BoxShadow {
                        color: ca(theme::shadow(), theme::palette().shadow_alpha),
                        offset: point(px(0.), px(12.)),
                        blur_radius: px(32.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .text_size(px(TEXT))
                    .line_height(px(16.))
                    .when(panel.scrolls, |d| d.overflow_y_scroll())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows)
                    .test_support()
                    .into_any_element()
            })
            .collect();
        Some(deferred(div().absolute().inset_0().children(boxes)).with_priority(2).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::{mnemonics, place, title_x, Row, BAR_LEFT, PANEL_TOP};
    use gpui_kit::{point, px, size, Bounds};

    #[test]
    fn access_keys_skip_letters_taken() {
        let titles = ["File", "Edit", "Alternatives", "View", "Help"];
        // Alt+A and Alt+L are the app's own keys off the Mac.
        let keys = mnemonics(&titles, |ch| matches!(ch, 'a' | 'l'));
        assert_eq!(keys, [Some(0), Some(0), Some(2), Some(0), Some(0)]);
        // Two names starting alike: the second takes its next letter.
        let keys = mnemonics(&["Datei", "Darstellung"], |_| false);
        assert_eq!(keys, [Some(0), Some(1)]);
        let keys = mnemonics(&["Файл", "Правка"], |_| false);
        assert_eq!(keys, [Some(0), Some(0)]);
    }

    #[test]
    fn titles_sit_side_by_side() {
        let titles = ["File", "Edit"];
        assert_eq!(title_x(&titles, 0), BAR_LEFT);
        assert!((title_x(&titles, 1) - (BAR_LEFT + 4. * 7.2 + 14.)).abs() < 0.01);
    }

    #[test]
    fn a_menu_stays_inside_the_window() {
        let window = size(px(800.), px(500.));
        let s = size(px(300.), px(200.));
        let (b, scrolls) = place(point(px(700.), px(PANEL_TOP)), None, s, window);
        assert!(b.right() <= px(796.) && !scrolls, "{b:?}");
        // A submenu with no room on the right opens to the left of its menu.
        let parent = Bounds::new(point(px(400.), px(PANEL_TOP)), size(px(300.), px(400.)));
        let (b, _) = place(point(parent.right(), px(100.)), Some(parent), s, window);
        assert!(b.right() <= parent.left() + px(3.), "{b:?}");
        // Taller than the window: it fits and scrolls.
        let (b, scrolls) = place(point(px(10.), px(PANEL_TOP)), None, size(px(300.), px(900.)), window);
        assert!(scrolls && b.bottom() <= px(496.), "{b:?}");
        // Near the bottom: it moves up.
        let (b, _) = place(point(px(10.), px(400.)), None, s, window);
        assert!(b.bottom() <= px(496.), "{b:?}");
    }

    #[test]
    fn rows_are_as_wide_as_their_words() {
        let r = |label: &str, keys: Option<&str>| Row { label: label.into(), keys: keys.map(Into::into), checked: false, enabled: true, submenu: false, separator: false };
        let short = super::panel_size(&[r("Save", Some("Ctrl+S"))]);
        let long = super::panel_size(&[r("Delete All Comments on This Page…", Some("Ctrl+Shift+Backspace"))]);
        assert_eq!(short.width, px(200. + 10.), "the narrowest menu");
        assert!(long.width > short.width);
    }
}
