use doc_model::GroupId;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use l10n::{tr, trc};

use crate::actions::*;
use crate::theme::{self, c, ca};
use crate::workspace::Workspace;

pub enum Command {
    Dispatch(Box<dyn Action>),
    CopyText(String),
    EditVariant(GroupId, usize),
    RemoveVariant(GroupId, usize),
    MakeOriginal(GroupId, usize),
    /// A version without the words, shown.
    AddCutVersion(GroupId),
    /// A spelling suggestion for the word at this page range.
    ReplaceText(std::ops::Range<usize>, String),
    OpenGroup(GroupId),
    Settle(GroupId),
    AiIdeas(GroupId),
    LearnSpelling(String),
    IgnoreSpelling(String),
    /// The dictionary popover for this text, at this window point.
    LookUp(String, Point<Pixels>),
    RenameRow(std::path::PathBuf),
    NewBelow(std::path::PathBuf),
    OpenInWindow(std::path::PathBuf),
    ShowInFinder(std::path::PathBuf),
    TrashRow(std::path::PathBuf),
    MoveToNotes(std::path::PathBuf),
    /// Into the book, the journal, or out of the notes folder.
    AddToList(std::path::PathBuf),
    OpenRecent(std::path::PathBuf),
    Cut(crate::cuts_panel::CutRef, crate::cuts_panel::CutOp),
    Trace(doc_model::CutId, crate::cuts::TraceOp),
    Comment(doc_model::CommentId, crate::comments::CommentOp),
    /// A theme tile's menu in Preferences, by theme id.
    Theme(String, crate::theme_ui::ThemeOp),
    /// A panel tab to the end of the other side's header.
    MoveTab(crate::panels::PanelTab, crate::panels::Side),
}

pub struct GroupTarget {
    pub id: GroupId,
    /// Its text on the page (the active variant).
    pub text: String,
    pub paragraph: bool,
}

pub struct Misspelling {
    pub range: std::ops::Range<usize>,
    pub word: String,
    pub guesses: Vec<String>,
}

fn plain(label: impl Into<SharedString>, command: Option<Command>) -> Option<Item> {
    Some(Item { label: label.into(), shortcut: None, keys: None, command, badge: None })
}

pub struct Item {
    pub label: SharedString,
    pub shortcut: Option<Box<dyn Action>>,
    /// A key shown when no action is bound to it (Remove's ⌘⌫ in the Alternatives panel).
    pub keys: Option<&'static str>,
    pub command: Option<Command>,
    pub badge: Option<&'static str>,
}

pub struct ContextMenu {
    pub at: Point<Pixels>,
    /// A state line above the items; `true` draws it as a warning.
    pub heading: Option<(SharedString, bool)>,
    pub items: Vec<Option<Item>>,
    /// Opens upward from `at`.
    pub up: bool,
}

fn item(label: impl Into<SharedString>, action: impl Action + Clone) -> Option<Item> {
    Some(Item {
        label: label.into(),
        shortcut: Some(action.boxed_clone()),
        keys: None,
        command: Some(Command::Dispatch(action.boxed_clone())),
        badge: None,
    })
}

impl ContextMenu {
    /// `group` is the innermost group holding the selection; inside a word or sentence group the
    /// menu opens it rather than nesting a new one, inside a paragraph group it offers both.
    /// Writing (`editing` false) gets a short menu with one way into the alternatives.
    #[allow(clippy::too_many_arguments)]
    pub fn page(
        at: Point<Pixels>,
        word: Option<String>,
        group: Option<GroupTarget>,
        ghosted: bool,
        has_selection: bool,
        spelling: Option<Misspelling>,
        look_up: Option<String>,
        ai: bool,
        editing: bool,
    ) -> Self {
        let ai = ai && editing;
        let alt_label = match word {
            Some(w) => tr!("Alternatives for “{word}”", word = w),
            None => tr!("Alternatives for selection").into(),
        };
        let mut items = Vec::new();
        if let Some(s) = spelling {
            if s.guesses.is_empty() {
                items.push(plain(tr!("No Guesses Found"), None));
            }
            for g in s.guesses.into_iter().take(6) {
                items.push(plain(g.clone(), Some(Command::ReplaceText(s.range.clone(), g))));
            }
            items.push(None);
            items.push(plain(tr!("Learn Spelling"), Some(Command::LearnSpelling(s.word.clone()))));
            items.push(plain(tr!("Ignore Spelling"), Some(Command::IgnoreSpelling(s.word))));
            items.push(None);
        }
        let open = group.map(|g| {
            let name = if g.paragraph {
                None
            } else {
                let mut short = String::new();
                let mut cut = false;
                for w in g.text.split_whitespace() {
                    if !short.is_empty() && short.chars().count() + 1 + w.chars().count() > 24 {
                        cut = true;
                        break;
                    }
                    if !short.is_empty() {
                        short.push(' ');
                    }
                    short.push_str(w);
                }
                Some(format!("{short}{}", if cut { "…" } else { "" }))
            };
            let settle = Some(Item {
                label: tr!("Settle on this").into(),
                shortcut: Some(SettleOnThis.boxed_clone()),
                keys: None,
                command: Some(Command::Settle(g.id)),
                badge: None,
            });
            let (open_label, ai_label) = match &name {
                None => (tr!("Open alternatives for this paragraph").to_string(), tr!("AI alternatives for this paragraph").to_string()),
                Some(t) => (tr!("Open alternatives for “{text}”", text = t), tr!("AI alternatives for “{text}”", text = t)),
            };
            let ai_group = plain(ai_label, Some(Command::AiIdeas(g.id)));
            (plain(open_label, Some(Command::OpenGroup(g.id))), settle, ai_group, g.paragraph)
        });
        // Inside a group its items come first and are for that group, whatever is selected.
        let for_selection = match open {
            Some((open, _, _, paragraph)) if !editing => {
                if !paragraph {
                    items.push(open);
                }
                paragraph && has_selection
            }
            Some((open, settle, ai_group, paragraph)) => {
                items.extend([open, settle]);
                if ai {
                    items.push(ai_group);
                }
                paragraph
            }
            None => editing || has_selection,
        };
        if for_selection {
            items.push(item(alt_label, AlternativesForSelection));
            if ai {
                items.push(item(tr!("AI alternatives for selection"), AiAlternatives));
            }
        }
        if editing && ghosted {
            items.push(item(tr!("Put Back"), GhostToggle));
        }
        if items.last().is_some_and(|i| i.is_some()) {
            items.push(None);
        }
        items.push(item(tr!("Cut"), Cut));
        if editing && has_selection && !ghosted {
            items.push(item(tr!("Cut Without a Trace"), StashToOverflow));
        }
        items.extend([item(tr!("Copy"), Copy), item(tr!("Paste"), Paste)]);
        if let Some(t) = look_up {
            let short: String = t.chars().take(24).collect();
            let label = if short.len() < t.len() { tr!("Look Up “{text}”", text = format!("{short}…")) } else { tr!("Look Up “{text}”", text = t) };
            items.extend([None, plain(label, Some(Command::LookUp(t, at)))]);
        }
        ContextMenu { at, heading: None, items, up: false }
    }

    pub fn add_comment_items(&mut self, under: Option<doc_model::CommentId>, can_add: bool) {
        use crate::comments::CommentOp;
        let mut extra = Vec::new();
        match under {
            Some(id) => {
                let op = |label: String, keys: Option<&'static str>, o: CommentOp| Some(Item { label: label.into(), shortcut: None, keys, command: Some(Command::Comment(id, o)), badge: None });
                extra.extend([op(tr!("Edit Comment").to_string(), None, CommentOp::Edit), op(tr!("Delete Comment").to_string(), None, CommentOp::Delete)]);
            }
            None if can_add => extra.push(item(tr!("Add Comment"), AddComment)),
            None => {}
        }
        if extra.is_empty() {
            return;
        }
        // After the alternatives' items, before Cut.
        let at = self.items.iter().position(|i| i.as_ref().is_some_and(|i| i.label.as_ref() == tr!("Cut"))).unwrap_or(self.items.len());
        let mut insert: Vec<Option<Item>> = extra;
        if at > 0 && !self.items.get(at - 1).is_some_and(|i| i.is_none()) {
            insert.insert(0, None);
        }
        insert.push(None);
        self.items.splice(at..at, insert);
    }

    pub fn project_row(path: std::path::PathBuf, kind: crate::project::Kind, place: crate::project_ui::Place, at: Point<Pixels>) -> Self {
        use crate::project::Kind;
        use crate::project_ui::Place;
        let mut items = Vec::new();
        match place {
            Place::Loose => {
                let add = match kind {
                    Kind::Book => tr!("Add to the Book"),
                    Kind::Journal => tr!("Add to the Journal"),
                    Kind::Folder => tr!("Add to the Folder"),
                };
                items.push(plain(add, Some(Command::AddToList(path.clone()))));
                items.push(plain(tr!("Move to Notes"), Some(Command::MoveToNotes(path.clone()))));
                items.push(None);
                items.push(plain(tr!("Rename"), Some(Command::RenameRow(path.clone()))));
            }
            Place::Note => {
                let back = match kind {
                    Kind::Folder => tr!("Move out of Notes"),
                    Kind::Book => tr!("Move to the Book"),
                    Kind::Journal => tr!("Move to the Journal"),
                };
                items.push(plain(back, Some(Command::AddToList(path.clone()))));
                items.push(None);
                items.push(plain(tr!("Rename"), Some(Command::RenameRow(path.clone()))));
            }
            Place::List => {
                items.push(plain(tr!("Rename"), Some(Command::RenameRow(path.clone()))));
                if kind != Kind::Journal {
                    items.push(Some(Item {
                        label: match kind {
                            Kind::Book => tr!("New Chapter Below"),
                            Kind::Journal => tr!("New Day Below"),
                            Kind::Folder => tr!("New File Below"),
                        }
                        .into(),
                        shortcut: Some(NewDocument.boxed_clone()),
                        keys: None,
                        command: Some(Command::NewBelow(path.clone())),
                        badge: None,
                    }));
                }
                items.push(plain(tr!("Move to Notes"), Some(Command::MoveToNotes(path.clone()))));
            }
        }
        items.extend([
            None,
            plain(tr!("Open in New Window"), Some(Command::OpenInWindow(path.clone()))),
            plain(tr!("Show in Finder"), Some(Command::ShowInFinder(path.clone()))),
            None,
            Some(Item {
                label: tr!("Move to Trash").into(),
                shortcut: Some(ListTrash.boxed_clone()),
                keys: None,
                command: Some(Command::TrashRow(path)),
                badge: None,
            }),
        ]);
        ContextMenu { at, heading: None, items, up: false }
    }

    /// `version`: a version without the words, which has no spot to forget.
    pub fn trace(id: doc_model::CutId, mark: bool, version: bool, pos: Point<Pixels>) -> Self {
        use crate::cuts::TraceOp;
        let op = |label: SharedString, shortcut: Option<Box<dyn Action>>, o: TraceOp| Some(Item { label, shortcut, keys: None, command: Some(Command::Trace(id, o)), badge: None });
        let checked = |on: bool, label: String| -> SharedString { if on { format!("✓ {label}").into() } else { format!("   {label}").into() } };
        let mut items = vec![
            op(tr!("Put Back").into(), Some(GhostToggle.boxed_clone()), TraceOp::PutBack),
            None,
            op(checked(!mark, tr!("Show the Text").to_string()), None, TraceOp::ShowText),
            op(checked(mark, tr!("Show as a Mark").to_string()), None, TraceOp::ShowMark),
            op(tr!("Show in Cuts").into(), Some(ToggleOverflow.boxed_clone()), TraceOp::ShowInCuts),
            None,
        ];
        if !version {
            items.push(op(tr!("Forget This Spot").into(), None, TraceOp::Forget));
        }
        items.push(op(tr!("Delete for Good").into(), None, TraceOp::Delete));
        ContextMenu { at: pos, heading: None, items, up: false }
    }

    pub fn cut(at: crate::cuts_panel::CutRef, traced: bool, pos: Point<Pixels>) -> Self {
        use crate::cuts_panel::{CutOp, CutRef};
        let mut items = Vec::new();
        if matches!(at, CutRef::Page(_) | CutRef::Drawer(_)) {
            items.push(plain(tr!("Edit Text"), Some(Command::Cut(at.clone(), CutOp::Edit))));
        }
        if traced {
            items.push(Some(Item { label: tr!("Put Back").into(), shortcut: Some(GhostToggle.boxed_clone()), keys: None, command: Some(Command::Cut(at.clone(), CutOp::PutBack)), badge: None }));
        }
        items.extend([
            plain(tr!("Go to Its Place"), Some(Command::Cut(at.clone(), CutOp::GoTo))),
            plain(tr!("Copy"), Some(Command::Cut(at.clone(), CutOp::Copy))),
            None,
            plain(tr!("Delete for Good"), Some(Command::Cut(at, CutOp::Delete))),
        ]);
        ContextMenu { at: pos, heading: None, items, up: false }
    }

    /// `row` adds the project row's actions (Rename, Move to Trash).
    pub fn document(at: Point<Pixels>, heading: (SharedString, bool), path: Option<std::path::PathBuf>, row: bool) -> Self {
        let mut items = vec![item(tr!("Save"), Save), item(tr!("Save As…"), SaveAs), item(tr!("Open…"), Open)];
        if let (true, Some(p)) = (row, &path) {
            items.extend([
                None,
                plain(tr!("Rename…"), Some(Command::RenameRow(p.clone()))),
                Some(Item { label: tr!("Move to Trash").into(), shortcut: Some(ListTrash.boxed_clone()), keys: None, command: Some(Command::TrashRow(p.clone())), badge: None }),
            ]);
        }
        items.extend([None, item(tr!("Copy Final Text"), CopyFinalMarkdown), item(tr!("Export…"), Export), item(tr!("Print…"), Print)]);
        if let Some(p) = path {
            items.extend([None, plain(tr!("Show in Finder"), Some(Command::ShowInFinder(p)))]);
        }
        items.extend([None, item(tr!("Settings…"), ShowPreferences), item(tr!("Keyboard Shortcuts"), ShowShortcuts)]);
        ContextMenu { at, heading: Some(heading), items, up: false }
    }

    /// A panel tab's label: the way to move it without dragging (VoiceOver's too).
    pub fn panel_tab(tab: crate::panels::PanelTab, to: crate::panels::Side, at: Point<Pixels>) -> Self {
        let label = match to {
            crate::panels::Side::Left => tr!("Move to Left Panel"),
            crate::panels::Side::Right => tr!("Move to Right Panel"),
        };
        let items = vec![plain(label, Some(Command::MoveTab(tab, to))), None, item(tr!("Reset Panel Layout"), ResetPanelLayout)];
        ContextMenu { at, heading: None, items, up: false }
    }

    pub fn open_menu(at: Point<Pixels>, recent: Vec<(std::path::PathBuf, String, &'static str)>, enclosing: bool) -> Self {
        let mut items = vec![item(tr!("Open…"), Open)];
        if !recent.is_empty() {
            items.push(None);
            for (path, name, kind) in recent {
                items.push(Some(Item { label: name.into(), shortcut: None, keys: Some(kind), command: Some(Command::OpenRecent(path)), badge: None }));
            }
        }
        items.extend([None, item(tr!("New Project…"), NewProject)]);
        if enclosing {
            items.push(item(tr!("Open Enclosing Folder"), OpenEnclosingFolder));
        }
        ContextMenu { at, heading: None, items, up: true }
    }

    /// `active`: the row is the version showing, which ⌘⌫ in the empty field removes.
    /// `cut`: a version without the words, which can't be edited or made the original.
    /// `without`: the original's row offers a version without it (a word group that has none).
    pub fn variant(id: GroupId, index: usize, active: bool, cut: bool, without: bool, text: String, at: Point<Pixels>) -> Self {
        let mut items = vec![
            Some(Item { label: tr!("Copy text").into(), shortcut: None, keys: None, command: Some(Command::CopyText(text)), badge: None }),
            Some(Item { label: tr!("Remove").into(), shortcut: None, keys: active.then_some("⌘⌫"), command: Some(Command::RemoveVariant(id, index)), badge: None }),
        ];
        if cut {
            return ContextMenu { at, heading: None, items, up: false };
        }
        if index > 0 {
            items.insert(0, Some(Item { label: tr!("Make original").into(), shortcut: None, keys: None, command: Some(Command::MakeOriginal(id, index)), badge: None }));
        }
        items.insert(0, Some(Item { label: trc!("variant", "Edit").into(), shortcut: None, keys: None, command: Some(Command::EditVariant(id, index)), badge: None }));
        if without {
            items.push(None);
            items.push(Some(Item { label: tr!("Add a version without it").into(), shortcut: None, keys: None, command: Some(Command::AddCutVersion(id)), badge: None }));
        }
        ContextMenu { at, heading: None, items, up: false }
    }
}

/// "cmd-shift-a" → "⌘⇧A". Searches the whole keymap (bindings are scoped to `Page`/`TextView`
/// and menus are drawn outside them); the last binding wins, so keymap.json beats the defaults.
pub fn shortcut_label(action: &dyn Action, cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).next_back()?;
    Some(binding.keystrokes().iter().map(keystroke_label).collect::<Vec<_>>().join(" "))
}

pub fn keystroke_label(k: &impl KeystrokeLike) -> String {
    k.label()
}

pub trait KeystrokeLike {
    fn label(&self) -> String;
}

impl KeystrokeLike for KeybindingKeystroke {
    fn label(&self) -> String {
        format_keystroke(self.modifiers(), self.key())
    }
}

pub fn format_keystroke(m: &Modifiers, key: &str) -> String {
    let mut s = String::new();
    if m.control {
        s.push('⌃');
    }
    if m.alt {
        s.push('⌥');
    }
    if m.shift {
        s.push('⇧');
    }
    if m.platform {
        s.push('⌘');
    }
    let k = match key {
        "up" => "↑".to_string(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "enter" => "↩".into(),
        "backspace" => "⌫".into(),
        "delete" => "⌦".into(),
        "escape" => "esc".into(),
        "tab" => "⇥".into(),
        "space" => "space".into(),
        "pageup" => "⇞".into(),
        "pagedown" => "⇟".into(),
        "-" => "−".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    };
    s.push_str(&k);
    s
}

impl Workspace {
    /// A button on the selection bar: the bar goes, then the action runs as its key would.
    fn selection_button(&mut self, action: Box<dyn Action>, window: &mut Window, cx: &mut Context<Self>) {
        self.page.update(cx, |p, cx| p.hide_selection_buttons(cx));
        window.dispatch_action(action, cx);
    }

    pub fn render_selection_buttons(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.context_menu.is_some() || self.sheet.is_some() {
            return None;
        }
        let page = self.page.read(cx);
        let sel = page.selection_buttons(cx)?;
        let layout = page.layout.clone()?;
        let rects = layout.rects(sel, false);
        let (first, last) = (*rects.first()?, *rects.last()?);
        const H: f32 = 34.;
        let room = layout.bounds.top().max(px(crate::workspace::UNDER_HEADER));
        let at = if first.top() - px(H + 6.) >= room {
            point(first.left() - px(8.), first.top() - px(H + 6.))
        } else if last.bottom() + px(H + 6.) <= layout.bounds.bottom() {
            point(first.left() - px(8.), last.bottom() + px(6.))
        } else {
            return None;
        };
        let in_cut = self.selection_in_cut(cx).is_some();
        let button = |id: &'static str, icon: AnyElement, label: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(7.))
                .h(px(26.))
                .px(px(10.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|d| d.bg(c(theme::menu_hover())))
                .child(icon)
                .child(label)
        };
        // The labels are the menu's text; the icons match, save Comment's own colour (mockup selection-bar, A).
        let icon = |path: &'static str, color: u32| svg().path(path).size(px(14.)).flex_none().text_color(c(color)).into_any_element();
        let sep = || div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(c(theme::menu_sep())).into_any_element();
        let cut = |id: &'static str, label: &'static str| {
            button(id, icon("chrome/axe.svg", theme::text_2()), label)
                .tooltip(crate::workspace::tip(tr!("Cut or Put Back"), Box::new(GhostToggle)))
                .on_click(cx.listener(|this, _, window, cx| this.selection_button(Box::new(GhostToggle), window, cx)))
                .test_support()
                .into_any_element()
        };
        // On cut words only Put Back applies: they aren't in the book.
        let buttons: Vec<AnyElement> = if in_cut {
            vec![cut("sel-put-back", tr!("Put Back"))]
        } else {
            let alternatives = button("sel-alternatives", icon("chrome/layer.svg", theme::text_2()), tr!("Alternatives"))
                .tooltip(crate::workspace::tip(tr!("Alternatives"), Box::new(AlternativesForSelection)))
                .on_click(cx.listener(|this, _, window, cx| this.selection_button(Box::new(AlternativesForSelection), window, cx)))
                .test_support()
                .into_any_element();
            let comment = button("sel-comment", icon("chrome/comment.svg", theme::comment()), tr!("Comment"))
                .tooltip(crate::workspace::tip(tr!("Add Comment"), Box::new(AddComment)))
                .on_click(cx.listener(|this, _, window, cx| this.selection_button(Box::new(AddComment), window, cx)))
                .test_support()
                .into_any_element();
            vec![alternatives, sep(), comment, cut("sel-cut", tr!("Cut"))]
        };
        Some(
            deferred(
                anchored().position(at).snap_to_window().child(
                    div()
                        .id("selection-buttons")
                        .occlude()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .h(px(H))
                        .px(px(4.))
                        .rounded(px(9.))
                        .bg(c(theme::bg_raised()))
                        .border_1()
                        .border_color(c(theme::menu_border()))
                        .shadow(vec![BoxShadow {
                            color: ca(theme::shadow(), theme::palette().shadow_alpha * 0.6),
                            offset: point(px(0.), px(8.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .text_size(px(12.5))
                        .text_color(c(theme::menu_text()))
                        .children(buttons)
                        .test_support(),
                ),
            )
            .into_any_element(),
        )
    }

    pub fn render_context_menu(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let at = menu.at;
        let corner = if menu.up { Anchor::BottomLeft } else { Anchor::TopLeft };
        let heading = menu.heading.clone();
        let rows: Vec<AnyElement> = menu
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| match it {
                None => div().h(px(1.)).my(px(5.)).mx(px(6.)).bg(c(theme::menu_sep())).into_any_element(),
                Some(it) => {
                    let enabled = it.command.is_some();
                    let keys = it.shortcut.as_ref().and_then(|a| shortcut_label(a.as_ref(), cx)).or(it.keys.map(String::from));
                    div()
                        .id(("menu", i))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(30.))
                        .px(px(11.))
                        .py(px(7.))
                        .line_height(px(17.))
                        .rounded(px(5.))
                        .text_color(if enabled { c(theme::menu_text()) } else { c(theme::menu_disabled()) })
                        .when(enabled, |d| d.cursor_pointer().hover(|d| d.bg(c(theme::menu_hover()))))
                        .on_click(cx.listener(move |this, _, window, cx| this.run_menu_item(i, window, cx)))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(it.label.clone())
                                .children(it.badge.map(|b| {
                                    div().px(px(4.)).rounded(px(3.)).border_1().border_color(c(theme::menu_badge_line())).text_color(c(theme::menu_badge_text())).text_size(px(9.)).child(b)
                                })),
                        )
                        .children(keys.map(|k| div().text_color(c(theme::menu_key())).child(k)))
                        .into_any_element()
                }
            })
            .collect();
        Some(
            div()
                .id("context-menu-backdrop")
                .absolute()
                .inset_0()
                // An open menu takes the pointer: nothing under it hovers; a click outside only closes it.
                .occlude()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.context_menu = None;
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| {
                    this.context_menu = None;
                    cx.notify();
                }))
                .child(
                    deferred(
                        anchored().anchor(corner).position(at).snap_to_window().child(
                            div()
                                .id("context-menu")
                                .occlude()
                                .min_w(px(320.))
                                .p(px(5.))
                                .rounded(px(8.))
                                .bg(c(theme::bg_raised()))
                                .border_1()
                                .border_color(c(theme::menu_border()))
                                .shadow(vec![BoxShadow {
                                    color: ca(theme::shadow(), theme::palette().shadow_alpha),
                                    offset: point(px(0.), px(18.)),
                                    blur_radius: px(50.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }])
                                .text_size(px(12.5))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .children(heading.map(|(text, warn)| {
                                    div()
                                        .id("menu-heading")
                                        .px(px(11.))
                                        .pt(px(5.))
                                        .pb(px(6.))
                                        .text_size(px(11.))
                                        .text_color(if warn { c(theme::origin_hover_text()) } else { c(theme::menu_disabled()) })
                                        .child(text)
                                        .test_support()
                                }))
                                .children(rows),
                        ),
                    ),
                )
                .into_any_element(),
        )
    }

    #[cfg(test)]
    pub fn run_menu_label(&mut self, label: &str, window: &mut Window, cx: &mut Context<Self>) {
        let menu = self.context_menu.as_ref().expect("no context menu is open");
        let i = menu
            .items
            .iter()
            .position(|it| it.as_ref().is_some_and(|it| it.label.as_ref() == label))
            .unwrap_or_else(|| panic!("no menu item {label:?}"));
        self.run_menu_item(i, window, cx);
    }

    fn run_menu_item(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.context_menu.take() else { return };
        let Some(Some(item)) = menu.items.into_iter().nth(i) else { return };
        cx.notify();
        match item.command {
            Some(Command::Dispatch(a)) => {
                // Run it against the page, where the selection lives.
                let f = self.page.read(cx).focus.clone();
                window.focus(&f, cx);
                window.dispatch_action(a, cx);
            }
            Some(Command::CopyText(t)) => cx.write_to_clipboard(ClipboardItem::new_string(t)),
            Some(Command::EditVariant(id, idx)) => self.edit_variant_in_sidebar(id, idx, window, cx),
            Some(Command::RemoveVariant(id, idx)) => self.remove_variant_at(id, idx, window, cx),
            Some(Command::MakeOriginal(id, idx)) => {
                let ok = self.model.update(cx, |m, cx| m.edit(cx, |d| d.make_original(id, idx)).is_ok());
                if !ok {
                    self.flash_status(tr!("Couldn't make that the original"), cx);
                }
            }
            Some(Command::AddCutVersion(id)) => self.add_cut_version(id, cx),
            Some(Command::OpenGroup(id)) => self.open_group(id, window, cx),
            Some(Command::Settle(id)) => self.settle_group(id, window, cx),
            Some(Command::AiIdeas(id)) => self.ai_alternatives_for_group(id, window, cx),
            Some(Command::ReplaceText(range, text)) => {
                self.page.update(cx, |p, cx| {
                    p.replace(range.clone(), &text, cx);
                    let end = range.start + text.chars().count();
                    p.select(end..end, cx);
                });
            }
            Some(Command::LearnSpelling(w)) => crate::spell::Spelling::learn(cx, &w),
            Some(Command::IgnoreSpelling(w)) => crate::spell::Spelling::ignore(cx, &w),
            Some(Command::LookUp(text, at)) => crate::platform::look_up(window, &text, at),
            Some(Command::RenameRow(path)) => {
                let label = self.project.as_ref().and_then(|p| p.tree.entry(&path)).map(|e| e.label.clone()).unwrap_or_default();
                self.begin_row_edit(crate::project_ui::EditTarget::Rename(path), &label, window, cx);
            }
            Some(Command::NewBelow(path)) => {
                let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                self.begin_row_edit(crate::project_ui::EditTarget::New { dir, after: Some(path), line: false }, "", window, cx);
            }
            Some(Command::OpenInWindow(path)) => self.open_row_in_window(&path, cx),
            Some(Command::ShowInFinder(path)) => cx.reveal_path(&path),
            Some(Command::TrashRow(path)) => self.trash_file(&path, window, cx),
            Some(Command::MoveToNotes(path)) => self.move_to_notes(&path, window, cx),
            Some(Command::AddToList(path)) => self.add_to_list(&path, window, cx),
            // Opening looks at every window, this one too, which is mid-update here: wait.
            Some(Command::OpenRecent(path)) => cx.defer(move |cx| crate::open_path(path, cx)),
            Some(Command::Cut(at, op)) => self.cut_op(at, op, window, cx),
            Some(Command::Trace(id, op)) => self.trace_op(id, op, window, cx),
            Some(Command::Comment(id, op)) => self.comment_op(id, op, window, cx),
            Some(Command::Theme(id, op)) => self.theme_op(&id, op, window, cx),
            Some(Command::MoveTab(tab, side)) => self.move_tab(tab, side, None, window, cx),
            None => {}
        }
    }
}
