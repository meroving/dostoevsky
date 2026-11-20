use std::path::{Path, PathBuf};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::actions::*;
use crate::theme::{self, c, ca};
use crate::workspace::{self, Tip, Workspace};

pub const FOOT_H: f32 = 46.;
pub const PANEL_PB: f32 = FOOT_H + 12.;
const RECENT_SHOWN: usize = 5;

/// (path, name, kind: "book" or "file").
pub type Recent = (PathBuf, String, &'static str);

/// Its lists are read when it opens, not on every frame.
pub struct FilePanel {
    pub recent: Vec<Recent>,
}

pub fn recent_entries(skip: &[&Path]) -> Vec<Recent> {
    // The same file can be spelled two ways (`/tmp`, `/private/tmp`; a symlinked folder).
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let skip: Vec<PathBuf> = skip.iter().map(|p| real(p)).collect();
    crate::files::recent()
        .into_iter()
        .filter(|p| !skip.contains(&real(p)))
        .take(RECENT_SHOWN)
        .map(|p| {
            let kind = if p.is_dir() {
                crate::project::read_settings(&p).map_or(tr!("folder"), |s| match s.kind {
                    crate::project::Kind::Book => tr!("book"),
                    crate::project::Kind::Journal => tr!("journal"),
                    crate::project::Kind::Folder => tr!("folder"),
                })
            } else {
                tr!("file")
            };
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
            (p, name, kind)
        })
        .collect()
}

fn tilde(path: &Path) -> String {
    match crate::platform::home() {
        Some(home) if path.starts_with(&home) => format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display()).trim_end_matches('/').to_string(),
        _ => path.display().to_string(),
    }
}

fn plain_tip(label: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Tip::new(label, None)).into()
}

fn foot_button(id: &'static str, icon: &'static str, label: &'static str, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .group(id)
        .role(Role::Button)
        .aria_label(label)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(32.))
        .h(px(28.))
        .rounded(px(5.))
        .child(
            svg()
                .path(icon)
                .size(px(17.))
                .flex_none()
                .text_color(if enabled { c(theme::icon()) } else { c(theme::faint()) })
                .when(enabled, |s| s.group_hover(id, |s| s.text_color(c(theme::accent())))),
        )
        .when(enabled, |d| d.cursor_pointer().hover(|d| d.bg(ca(theme::line(), 0.8))))
}

impl Workspace {
    pub(crate) fn render_panel_foot(&self, cx: &mut Context<Self>) -> AnyElement {
        let path = self.model.read(cx).path.clone();
        let root = self.project_root().map(Path::to_path_buf);
        let (state, warn) = self.save_state(cx);

        let open = foot_button("foot-open", "chrome/open.svg", tr!("Open"), true)
            .tooltip(workspace::tip(tr!("Open… · recent"), Box::new(Open)))
            .on_click(cx.listener(|this, _, w, cx| this.open_open_menu(w, cx)))
            .test_support();
        let save = foot_button("foot-save", "chrome/save.svg", tr!("Save"), true)
            // Only unsaved with autosave off (or never saved): with autosave on, the header says it.
            .when(warn, |d| d.child(div().id("foot-unsaved").absolute().right(px(5.)).top(px(4.)).size(px(6.)).rounded_full().bg(c(theme::accent())).test_support()))
            .tooltip(move |_, cx| {
                let keys = crate::context_menu::shortcut_label(&Save, cx);
                let label = tr!("Save · {state}", state = state);
                cx.new(|_| Tip::new(label, keys)).into()
            })
            .on_click(|_, w, cx| w.dispatch_action(Box::new(Save), cx))
            .test_support();
        let finder = foot_button("foot-finder", "chrome/finder.svg", tr!("Show in Finder"), path.is_some())
            .when_some(path, move |d, p| {
                d.tooltip(plain_tip(tr!("Show in Finder"))).on_click(move |e: &ClickEvent, _, cx| {
                    let target = if e.modifiers().alt { root.clone().unwrap_or_else(|| p.clone()) } else { p.clone() };
                    cx.reveal_path(&target);
                })
            })
            .test_support();
        // What Export opens on is read from disk when its tooltip shows, not on every frame.
        let (in_book, export_key) = self.export_tip_source(cx);
        let export = foot_button("foot-export", "chrome/export.svg", tr!("Export"), true)
            .tooltip(move |_, cx| {
                let keys = crate::context_menu::shortcut_label(&Export, cx);
                let label = crate::export_ui::export_tip(in_book, export_key.as_deref());
                cx.new(|_| Tip::new(label, keys)).into()
            })
            .on_click(cx.listener(|this, _, w, cx| this.open_export(true, w, cx)))
            .test_support();
        let settings = foot_button("foot-settings", "chrome/sliders.svg", tr!("Settings"), true)
            .tooltip(workspace::tip(tr!("Settings…"), Box::new(ShowPreferences)))
            .on_click(cx.listener(|this, _, w, cx| this.show_preferences(&ShowPreferences, w, cx)))
            .test_support();
        div()
            .id("panel-foot")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(FOOT_H))
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(14.))
            .pb(px(3.))
            .border_t_1()
            .border_color(c(theme::line()))
            .child(open)
            .child(save)
            .child(finder)
            .child(export)
            .child(div().flex_1())
            .child(settings)
            .test_support()
            .into_any_element()
    }

    pub(crate) fn open_open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.model.read(cx).path.clone();
        let skip: Vec<&Path> = path.as_deref().into_iter().chain(self.project_root()).collect();
        let recent = recent_entries(&skip);
        let enclosing = self.project.is_none() && path.is_some();
        let side = crate::panels::side_of(crate::panels::PanelTab::Project, cx);
        let x = self.side_x(side, f32::from(window.viewport_size().width), cx) + 14.;
        let at = point(px(x), window.viewport_size().height - px(FOOT_H - 4.));
        self.count_open = false;
        self.context_menu = Some(crate::context_menu::ContextMenu::open_menu(at, recent, enclosing));
        cx.notify();
    }

    pub(crate) fn open_file_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.make_room_for(crate::panels::PanelTab::Project, window, cx);
        let path = self.model.read(cx).path.clone();
        let skip: Vec<&Path> = path.as_deref().into_iter().collect();
        self.file_panel = Some(FilePanel { recent: recent_entries(&skip) });
        cx.notify();
    }

    pub fn render_file_panel(&mut self, side: crate::panels::Side, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.project.is_some() {
            return None;
        }
        let recent = self.file_panel.as_ref()?.recent.clone();
        let m = self.model.read(cx);
        let path = m.path.clone();
        let words = m.counts(None).words;
        let title = path.as_deref().and_then(Path::file_stem).map(|s| crate::project::spoken(&s.to_string_lossy())).unwrap_or_else(|| tr!("untitled").into());
        let subtitle = if path.is_some() { tr!("a single file · not in a project") } else { tr!("not saved yet") };
        let folder = path.as_deref().and_then(Path::parent).map(Path::to_path_buf);

        let row = |id: SharedString, label: String, detail: Option<&'static str>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(24.))
                .flex_none()
                .px(px(4.))
                .rounded_md()
                .text_size(px(13.))
                .line_height(px(19.))
                .cursor_pointer()
                .text_color(c(theme::muted()))
                .hover(|d| d.bg(ca(theme::line(), 0.8)))
                .role(Role::Button)
                .aria_label(label.clone())
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(detail.map(|k| div().flex_none().text_size(px(10.5)).text_color(c(theme::faint())).child(k)))
        };
        let add = |id: &'static str, label: String| {
            div()
                .id(id)
                .flex()
                .items_center()
                .h(px(24.))
                .flex_none()
                .px(px(4.))
                .rounded_md()
                .text_size(px(12.5))
                .line_height(px(19.))
                .cursor_pointer()
                .text_color(c(theme::faint()))
                .hover(|d| d.bg(ca(theme::line(), 0.8)).text_color(c(theme::accent())))
                .role(Role::Button)
                .aria_label(label.clone())
                .child(div().min_w_0().truncate().child(label))
        };

        let mut list = div().id("file-panel-list").flex().flex_col().flex_1().min_h(px(0.)).pt(px(14.)).overflow_y_scroll();
        if let Some(dir) = folder {
            let name = crate::project::folder_name(&dir);
            list = list
                .child(div().h(px(24.)).flex_none().flex().items_center().px(px(4.)).text_size(px(12.5)).italic().text_color(c(theme::muted())).child(div().min_w_0().truncate().child(tr!("in {folder}", folder = tilde(&dir)))))
                .child(
                    add("folder-as-project", tr!("+ “{name}” as a project", name = name))
                        .tooltip(workspace::tip(tr!("The files in this folder become the list"), Box::new(OpenEnclosingFolder)))
                        .on_click(cx.listener(|this, _, w, cx| this.open_enclosing_folder(&OpenEnclosingFolder, w, cx)))
                        .test_support(),
                );
        }
        list = list.child(div().h(px(15.)).mt(px(18.)).mb(px(3.)).flex_none().px(px(4.)).text_size(px(10.5)).text_color(c(theme::muted())).child(tr!("recent")));
        if recent.is_empty() {
            list = list.child(div().id("recent-empty").px(px(4.)).h(px(24.)).flex().items_center().text_size(px(12.5)).text_color(c(theme::faint())).child(tr!("nothing opened yet")).test_support());
        }
        for (i, (p, name, kind)) in recent.into_iter().enumerate() {
            list = list.child(
                row(SharedString::from(format!("recent-{i}")), name, Some(kind))
                    .on_click(move |_, _, cx| crate::open_path(p.clone(), cx))
                    .test_support(),
            );
        }
        list = list.child(div().h(px(6.)).flex_none()).child(
            add("new-project", tr!("+ new project…").into())
                .on_click(|_, w, cx| w.dispatch_action(Box::new(NewProject), cx))
                .test_support(),
        );

        let summary = trn!("{count} word", "{count} words", words, count = crate::sheets::group_thousands(words));
        let strip = crate::panels::strip_at_top(self.tab_strip(side, cx));
        let foot = self.render_panel_foot(cx);
        let el = crate::panels::frame(div().id("file-panel").test_support(), side, cx)
            .flex()
            .flex_col()
            .pt(px(160. - workspace::HEADER_H))
            .px(px(20.))
            .pb(px(PANEL_PB))
            .gap_3()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(workspace::resize_handle(side.edge()))
            .child(strip)
            .child(div().id("file-panel-title").absolute().top(px(76.)).left(px(24.)).right(px(18.)).truncate().font_family(theme::hand()).text_size(theme::hand_px(23.)).text_color(c(theme::text_2())).child(title).test_support())
            .child(div().absolute().top(px(106.)).left(px(24.)).right(px(18.)).truncate().text_size(px(10.5)).text_color(c(theme::faint())).child(subtitle))
            .child(list)
            .child(div().id("file-panel-summary").flex_none().mt_auto().px(px(4.)).text_size(px(11.)).text_color(c(theme::text_2())).child(summary).test_support())
            .child(foot);
        Some(el.into_any_element())
    }
}
