use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use doc_model::Scope;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trc, trn};

use crate::actions::*;
use crate::context_menu::shortcut_label;
use crate::format;
use crate::markdown::{BlockKind, LineStyle, Span};
use crate::prefs::{Alignment, Appearance, Autosave, CommentLook, CutLook, Effects, HandFont, HiddenLook, ParagraphLook, Prefs, SpellCheck, StaysVisible, WheelSwitch, ZenFade, ZenFocus};
use crate::theme::{self, c, ca};
use crate::workspace::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefSection {
    General,
    Page,
    Writing,
    Editing,
    Ai,
    /// Only in a project window.
    Project,
}

pub enum Sheet {
    Source,
    Export(Box<crate::export_ui::ExportForm>),
    Shortcuts,
    Prefs { section: PrefSection },
    Detached,
    NewProject(Box<crate::project_sheets::NewProjectForm>),
    ProjectType(crate::project_sheets::FirstOpen),
}

impl Sheet {
    fn compact(&self) -> bool {
        matches!(self, Sheet::NewProject(_) | Sheet::ProjectType(_))
    }
}

/// Every source line is an element; what's cut off here still copies.
const SOURCE_LINES: usize = 2000;

type Setter = Rc<dyn Fn(&mut Prefs, f32)>;

pub(crate) fn set_prefs(cx: &mut App, f: impl FnOnce(&mut Prefs)) {
    let mut p = Prefs::get(cx).clone();
    f(&mut p);
    p.save();
    let old = Prefs::get(cx);
    let menus_changed = (old.appearance, old.zen_writing, old.typewriter, old.private_mode, old.alignment, old.hyphenation, old.paragraphs, old.ai.enabled, &old.language)
        != (p.appearance, p.zen_writing, p.typewriter, p.private_mode, p.alignment, p.hyphenation, p.paragraphs, p.ai.enabled, &p.language);
    if old.language != p.language {
        // Tests switch only their own thread, so tests running beside them stay in English.
        if cfg!(test) {
            l10n::set_for_thread(Some(p.ui_language()));
        } else {
            l10n::set(p.ui_language());
        }
    }
    let cuts_changed = crate::prefs::publish(&p);
    cx.set_global(p);
    crate::theme::apply(cx);
    if cuts_changed {
        crate::represent_cuts(cx);
    }
    if menus_changed {
        crate::refresh_menus(cx);
    }
    cx.refresh_windows();
}

pub(crate) fn segmented<T: std::marker::Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String)>,
    current: T,
    set: impl Fn(&mut Prefs, T) + 'static,
) -> impl IntoElement {
    segmented_with(id, options.into_iter().map(|(v, l)| (v, l, None, None)).collect(), current, set)
}

/// An unknown family shows as JetBrains Mono.
const PAGE_FONTS: [(&str, &str); 4] =
    [(theme::MONO, "JetBrains Mono"), (theme::PLEX, "IBM Plex Mono"), (theme::QUATTRO, "iA Writer Quattro"), (theme::LITERATA, "Literata")];

type Face = Option<(&'static str, Pixels)>;

fn segmented_with<T: std::marker::Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String, Option<AnyElement>, Face)>,
    current: T,
    set: impl Fn(&mut Prefs, T) + 'static,
) -> impl IntoElement {
    chips(id, options, current, set, false)
}

/// Chips that show only their pictures; the names are their tooltips.
fn swatch_chips<T: std::marker::Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String, AnyElement)>,
    current: T,
    set: impl Fn(&mut Prefs, T) + 'static,
) -> impl IntoElement {
    chips(id, options.into_iter().map(|(v, l, p)| (v, l, Some(p), None)).collect(), current, set, true)
}

fn chips<T: std::marker::Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String, Option<AnyElement>, Face)>,
    current: T,
    set: impl Fn(&mut Prefs, T) + 'static,
    pictures_only: bool,
) -> impl IntoElement {
    let set = Rc::new(set);
    div()
        .flex()
        // Too narrow a sheet for every option (the Font row at 520 px): the last ones wrap.
        .flex_wrap()
        .rounded_md()
        .border_1()
        .border_color(c(theme::line()))
        .overflow_hidden()
        .children(options.into_iter().enumerate().map(move |(i, (v, label, picture, face))| {
            let set = set.clone();
            let on = v == current;
            div()
                .id((id, i))
                .px_3()
                .py_1()
                .cursor_pointer()
                .when(i > 0, |d| d.border_l_1().border_color(c(theme::line())))
                .when(on, |d| d.bg(ca(theme::accent(), 0.22)).text_color(c(theme::text())))
                .when(!on, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text_2()))))
                .role(Role::RadioButton)
                .aria_label(label.clone())
                .aria_selected(on)
                .on_click(move |_, _, cx| set_prefs(cx, |p| set(p, v)))
                .flex()
                .items_center()
                .gap_2()
                .children(picture)
                .when(pictures_only, |d| d.min_h(px(22.)).tooltip(crate::about::tip(label.clone())))
                .when(!pictures_only, |d| {
                    d.child(match face {
                        Some((family, size)) => div().font_family(family).text_size(size).child(label).into_any_element(),
                        None => label.into_any_element(),
                    })
                })
                .test_support()
        }))
}

/// Choice 0 follows macOS; the others are `l10n::Lang::ALL`.
fn language_picker(p: &Prefs, open: bool) -> impl IntoElement {
    const BOX_HEIGHT: f32 = 30.;
    let mut options = vec![tr!("Match macOS").to_string()];
    options.extend(l10n::Lang::ALL.iter().map(|l| l.native_name().to_string()));
    let current = p.language.as_deref().and_then(l10n::Lang::from_code).and_then(|l| l10n::Lang::ALL.iter().position(|x| *x == l)).map_or(0, |i| i + 1);
    let toggle = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
        let a = cx.default_global::<crate::assist::Assist>();
        a.menu = if open { None } else { Some(crate::assist::PrefsMenu::Language) };
        cx.refresh_windows();
    };
    let picker = div()
        .id("language")
        .role(Role::ComboBox)
        .aria_label(options[current].clone())
        .aria_expanded(open)
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .w(px(240.))
        .h(px(BOX_HEIGHT))
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(if open { c(theme::accent()) } else { c(theme::line()) })
        .cursor_pointer()
        .on_click(toggle)
        .child(div().text_color(c(theme::text())).child(options[current].clone()))
        .child(div().text_color(c(theme::muted())).child("▾"))
        .test_support();
    // Below the box when the list fits, else just above it (switching sides drops the offset).
    let list = open.then(|| {
        deferred(
            anchored().offset(point(px(0.), px(BOX_HEIGHT))).child(
                div().py_1().child(
                div()
                    .id("language-menu")
                    .occlude()
                    .w(px(240.))
                    .p_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(c(theme::line()))
                    .bg(c(theme::bg_raised()))
                    .shadow_lg()
                    .children(options.into_iter().enumerate().map(move |(i, label)| {
                        let on = i == current;
                        div()
                            .id(("language", i))
                            .role(Role::RadioButton)
                            .aria_label(label.clone())
                            .aria_selected(on)
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(c(theme::text()))
                            .when(on, |d| d.bg(ca(theme::accent(), 0.22)))
                            .hover(|d| d.bg(ca(theme::accent(), 0.16)))
                            .on_click(move |_, _, cx| {
                                cx.default_global::<crate::assist::Assist>().menu = None;
                                set_prefs(cx, |p| p.language = i.checked_sub(1).map(|i| l10n::Lang::ALL[i].code().to_string()));
                            })
                            .child(label)
                            .test_support()
                    }))
                    .test_support(),
                ),
            ),
        )
        .with_priority(2)
    });
    div().relative().child(picker).children(list.map(|l| div().absolute().top_0().left_0().child(l)))
}

fn swatch(a: Appearance) -> AnyElement {
    let (bg, ink) = match a {
        Appearance::Dark => (theme::DARK.bg, theme::DARK.text),
        Appearance::Light | Appearance::System => (theme::LIGHT.bg, theme::LIGHT.text),
    };
    let ink = if a == Appearance::System { theme::DARK.accent } else { ink };
    let line = move |w: f32, top: f32| div().absolute().left(px(3.)).top(px(top)).w(px(w)).h(px(1.5)).rounded(px(1.)).bg(c(ink));
    div()
        .relative()
        .w(px(18.))
        .h(px(12.))
        .rounded(px(2.5))
        .overflow_hidden()
        .border_1()
        .border_color(ca(theme::muted(), 0.5))
        .bg(c(bg))
        .when(a == Appearance::System, |d| d.child(div().absolute().right_0().top_0().w(px(9.)).h_full().bg(c(theme::DARK.bg))))
        .child(line(10., 2.5))
        .child(line(7., 6.))
        .into_any_element()
}

/// Private mode's looks, drawn as on the page: a few words' worth of each.
fn hidden_swatch(look: HiddenLook) -> AnyElement {
    let ink = theme::text();
    let shape = |w: f32, h: f32, r: f32| div().w(px(w)).h(px(h)).rounded(px(r)).bg(c(ink));
    let row = div().flex().items_center().justify_center().gap(px(3.)).min_w(px(30.)).h(px(14.));
    match look {
        HiddenLook::Dots => row.children((0..3).map(|_| shape(4., 4., 2.))),
        HiddenLook::Bars => row.children((0..3).map(|_| shape(8., 4., 1.))),
        HiddenLook::Solid => row.child(shape(26., 6., 1.)),
        // No blur in a div: soft shadows stand in for blurred letters.
        HiddenLook::Blur => row.children((0..3).map(|_| {
            div().w(px(8.)).h(px(5.)).rounded(px(2.5)).bg(ca(ink, 0.45)).shadow(vec![BoxShadow {
                color: ca(ink, 0.6),
                offset: point(px(0.), px(0.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            }])
        })),
    }
    .into_any_element()
}

fn toggle(id: &'static str, on: bool, set: impl Fn(&mut Prefs, bool) + 'static) -> impl IntoElement {
    toggle_with(id, on, move |cx, v| set_prefs(cx, |p| set(p, v)))
}

pub(crate) fn toggle_with(id: &'static str, on: bool, set: impl Fn(&mut App, bool) + 'static) -> impl IntoElement {
    div()
        .id(id)
        .w(px(38.))
        .h(px(22.))
        .rounded_full()
        .p(px(3.))
        .cursor_pointer()
        .bg(if on { c(theme::accent()) } else { c(theme::line()) })
        .flex()
        .when(on, |d| d.justify_end())
        .role(Role::Switch)
        .aria_toggled(crate::workspace::toggled(on))
        .on_click(move |_, _, cx| set(cx, !on))
        .child(div().size(px(16.)).rounded_full().bg(if on { c(theme::bg()) } else { c(theme::muted()) }))
        .test_support()
}

fn slider(id: &'static str, value: f32, min: f32, max: f32, w: Pixels, set: impl Fn(&mut Prefs, f32) + 'static) -> impl IntoElement {
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
    let set: Setter = Rc::new(set);
    let k = ((value - min) / (max - min)).clamp(0., 1.);
    let apply = {
        let bounds = bounds.clone();
        move |x: Pixels, cx: &mut App, set: &Setter| {
            let b = bounds.get();
            if b.size.width <= px(0.) {
                return;
            }
            let t = ((x - b.left()) / b.size.width).clamp(0., 1.);
            let v = min + (max - min) * t;
            set_prefs(cx, |p| set(p, v));
        }
    };
    let apply2 = apply.clone();
    let (set1, set2) = (set.clone(), set);
    div()
        .on_children_prepainted({
            let bounds = bounds.clone();
            move |b, _, _| {
                if let Some(b) = b.first() {
                    bounds.set(*b);
                }
            }
        })
        .id(id)
        .role(Role::Slider)
        .aria_numeric_value(value as f64)
        .aria_min_numeric_value(min as f64)
        .aria_max_numeric_value(max as f64)
        .w(w)
        .h(px(18.))
        .flex()
        .items_center()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |e, _, cx| apply(e.position.x, cx, &set1))
        .on_mouse_move(move |e, _, cx| {
            if e.pressed_button == Some(MouseButton::Left) {
                apply2(e.position.x, cx, &set2)
            }
        })
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(c(theme::line()))
                .child(div().absolute().left_0().top_0().h_full().w(relative(k)).rounded_full().bg(c(theme::accent())))
                .child(div().absolute().top(px(-5.)).left(relative(k)).ml(px(-7.)).size(px(14.)).rounded_full().bg(c(theme::text()))),
        )
        .test_support()
}

pub(crate) fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .border_1()
        .border_color(if primary { c(theme::accent()) } else { c(theme::line()) })
        .when(primary, |d| d.bg(ca(theme::accent(), 0.2)).text_color(c(theme::text())))
        .when(!primary, |d| d.text_color(c(theme::text_2())))
        .hover(|d| d.bg(ca(theme::accent(), 0.28)))
        .child(label.into())
}

pub(crate) fn heading(t: &'static str) -> AnyElement {
    div().pt_4().pb_1().text_size(px(10.5)).text_color(c(theme::faint())).child(t).into_any_element()
}

pub(crate) fn row(label: &'static str, hint: Option<&'static str>, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_4()
        .gap_y_2()
        .py(px(10.))
        .border_b_1()
        .border_color(c(theme::line()))
        .child(
            div()
                .w(px(240.))
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_color(c(theme::text())).child(label))
                .children(hint.map(|h| div().text_size(px(11.)).text_color(c(theme::muted())).child(h))),
        )
        .child(control)
}

impl Workspace {
    pub fn open_sheet(&mut self, sheet: Sheet, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet = Some(sheet);
        window.focus(&self.sheet_focus, cx);
        cx.notify();
    }

    pub fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet = None;
        crate::assist::close_menu(cx);
        self.model_search = None;
        let f = self.page.read(cx).focus.clone();
        window.focus(&f, cx);
        cx.notify();
    }

    pub fn view_source(&mut self, _: &ViewSource, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.sheet, Some(Sheet::Source)) {
            self.close_sheet(window, cx);
        } else {
            self.open_sheet(Sheet::Source, window, cx);
        }
    }

    pub fn show_shortcuts(&mut self, _: &ShowShortcuts, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.sheet, Some(Sheet::Shortcuts)) {
            self.close_sheet(window, cx);
        } else {
            self.open_sheet(Sheet::Shortcuts, window, cx);
        }
    }

    pub fn show_preferences(&mut self, _: &ShowPreferences, window: &mut Window, cx: &mut Context<Self>) {
        self.open_sheet(Sheet::Prefs { section: PrefSection::General }, window, cx);
    }

    fn attach_detached(&mut self, i: usize, cx: &mut Context<Self>) {
        let sel = self.page.read(cx).selection();
        if sel.is_empty() {
            self.flash_status(tr!("Select the text this group belongs to, then Attach"), cx);
            return;
        }
        self.reattach_detached_at(i, sel, cx);
    }

    pub(crate) fn reattach_detached_at(&mut self, i: usize, range: std::ops::Range<usize>, cx: &mut Context<Self>) {
        let Some(g) = self.model.read(cx).kept.detached.get(i).cloned() else { return };
        let selected = self.page.read(cx).index().slice(range.clone()).to_string();
        let r = self.model.update(cx, |m, cx| {
            let id = m.create_group(cx, range, g.scope)?;
            for v in &g.variants {
                if v.text != selected {
                    let _ = m.edit(cx, |d| d.add_variant(id, &v.text, v.by));
                }
            }
            m.kept.detached.remove(i);
            Ok::<_, doc_model::EditError>(())
        });
        match r {
            Ok(()) => self.flash_status(tr!("Attached"), cx),
            Err(e) => self.flash_status(e.to_string(), cx),
        }
        cx.notify();
    }

    fn discard_detached(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.model.update(cx, |m, cx| m.discard_detached(cx, i)) {
            self.flash_status(tr!("Discarded. ⌘Z to undo"), cx);
        }
    }

    pub fn render_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.tidy_model_search(window, cx);
        let sheet = self.sheet.as_ref()?;
        let (title_bar, body): (AnyElement, AnyElement) = match sheet {
            Sheet::Source => self.render_source(cx),
            Sheet::Export(_) => self.render_export(window, cx),
            Sheet::Shortcuts => self.render_shortcuts(window, cx),
            Sheet::Prefs { section } => self.render_prefs(*section, window, cx),
            Sheet::Detached => self.render_detached(cx),
            Sheet::NewProject(_) => self.render_new_project(window, cx),
            Sheet::ProjectType(form) => {
                let form = form.clone();
                self.render_first_open(form, cx)
            }
        };
        let compact = self.sheet.as_ref().is_some_and(Sheet::compact);
        let width = window.viewport_size().width;
        let inset = if compact { ((width - px(720.)) / 2.).max(px(24.)) } else { sheet_inset(width) };
        let mut ctx = KeyContext::new_with_defaults();
        ctx.add("Sheet");
        // An open Preferences menu: a click anywhere else only closes it, and nothing under it hovers.
        let menu_catcher = crate::assist::menu_open(cx).then(|| {
            let size = window.viewport_size();
            let close = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                crate::assist::close_menu(cx);
                cx.stop_propagation();
            };
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("ai-menu-backdrop")
                        .w(size.width)
                        .h(size.height)
                        .occlude()
                        .on_mouse_down(MouseButton::Left, close)
                        .on_mouse_down(MouseButton::Right, close)
                        .test_support(),
                ),
            )
            .with_priority(1)
        });
        Some(
            div()
                .id("sheet-backdrop")
                .absolute()
                .inset_0()
                .bg(ca(theme::bg(), 0.72))
                .occlude()
                .key_context(ctx)
                .track_focus(&self.sheet_focus)
                .on_action(cx.listener(|this, _: &Cancel, w, cx| {
                    if !crate::assist::close_menu(cx) {
                        this.close_sheet(w, cx);
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, w, cx| this.close_sheet(w, cx)))
                .child(
                    div()
                        .id("sheet")
                        .absolute()
                        .top(px(if compact { 58. } else { 54. }))
                        .when(!compact, |d| d.bottom(px(46.)))
                        .left(inset)
                        .right(inset)
                        .flex()
                        .flex_col()
                        .rounded_xl()
                        .bg(c(theme::bg_panel()))
                        .border_1()
                        .border_color(c(theme::line()))
                        .shadow_lg()
                        .p(px(26.))
                        .gap_4()
                        .text_size(px(13.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(title_bar)
                        .child(div().when(!compact, |d| d.flex_1()).min_h_0().child(body))
                        .test_support(),
                )
                .children(menu_catcher)
                .into_any_element(),
        )
    }

    fn hand_tab(id: &'static str, label: &'static str, on: bool) -> Stateful<Div> {
        div()
            .id(id)
            .font_family(theme::hand())
            .text_size(theme::hand_px(25.))
            .cursor_pointer()
            .text_color(if on { c(theme::accent()) } else { c(theme::muted()) })
            .hover(|d| d.text_color(c(theme::text_2())))
            .child(label)
    }

    pub(crate) fn source_text(&self, cx: &App) -> String {
        format::serialize(self.model.read(cx))
    }

    fn render_source(&mut self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let text = self.source_text(cx);
        let meta = trn!("{count} byte", "{count} bytes", text.len(), count = group_thousands(text.len()));
        let copy_text = text.clone();
        let title = div()
            .flex()
            .items_center()
            .gap_5()
            .child(Self::hand_tab("as-saved", tr!("Markdown source"), true).test_support())
            .child(div().flex_1())
            .child(div().text_color(c(theme::muted())).text_size(px(12.)).child(meta))
            .child(
                Self::hand_tab("copy", tr!("Copy"), false)
                    .text_color(c(theme::text_2()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                        this.flash_status(tr!("Copied"), cx);
                    }))
                    .test_support(),
            )
            .into_any_element();
        let body = div()
            .id("source-text")
            .size_full()
            .overflow_y_scroll()
            .font_family(theme::UI)
            .text_size(px(12.5))
            .line_height(px(21.))
            .text_color(c(theme::text_2()))
            .children(text.split('\n').take(SOURCE_LINES).map(|l| div().min_h(px(21.)).child(l.to_string())).collect::<Vec<_>>())
            .when(text.lines().count() > SOURCE_LINES, |d| {
                let more = text.lines().count() - SOURCE_LINES;
                d.child(div().text_color(c(theme::muted())).child(trn!(
                    "… {count} more line. Copy takes it too.",
                    "… {count} more lines. Copy takes them all.",
                    more,
                    count = group_thousands(more)
                )))
            })
            .into_any_element();
        (title, body)
    }

    fn render_shortcuts(&mut self, _: &mut Window, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let title = div()
            .flex()
            .items_center()
            .gap_4()
            .child(Self::hand_tab("shortcuts", tr!("Shortcuts"), true).flex_none())
            .child(div().flex_1())
            .child(div().min_w_0().truncate().text_color(c(theme::faint())).text_size(px(11.)).child(tr!("from your keymap · {path}", path = "~/.config/dostoevsky/keymap.json")))
            .into_any_element();
        let mut groups = shortcut_groups();
        if !crate::assist::on(cx) {
            // By action: the titles are in the UI's language.
            groups.retain(|(_, items)| !items.iter().any(|(_, a)| a.partial_eq(&ToggleLab)));
            for (_, items) in &mut groups {
                items.retain(|(_, a)| !a.partial_eq(&crate::actions::AiAlternatives));
            }
        }
        let cols: Vec<AnyElement> = groups
            .into_iter()
            .map(|(name, items)| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w(px(300.))
                    .mb_5()
                    .child(div().text_color(c(theme::accent())).text_size(px(11.5)).mb_1().child(name.to_uppercase()))
                    .children(items.into_iter().map(|(label, action)| {
                        let keys = shortcut_label(action.as_ref(), cx).unwrap_or_else(|| "—".into());
                        let keys = match (action.partial_eq(&CycleNext), Prefs::get(cx).wheel) {
                            (false, _) => keys,
                            (true, WheelSwitch::Always) => tr!("↑ ↓ or scroll (hover)").to_string(),
                            (true, WheelSwitch::WithOption) => tr!("↑ ↓ or ⌥ scroll (hover)").to_string(),
                            (true, WheelSwitch::Off) => tr!("↑ ↓ (hover)").to_string(),
                        };
                        div()
                            .flex()
                            .justify_between()
                            .gap_3()
                            .py(px(3.))
                            .child(div().text_color(c(theme::text_2())).child(label))
                            .child(div().text_color(c(theme::muted())).child(keys))
                    }))
                    .into_any_element()
            })
            .collect();
        let body = div().id("shortcut-list").size_full().overflow_y_scroll().flex().flex_wrap().gap_x_10().children(cols).into_any_element();
        (title, body)
    }

    fn render_prefs(&mut self, section: PrefSection, window: &mut Window, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let p = Prefs::get(cx).clone();
        let in_project = self.project.is_some();
        let section = if section == PrefSection::Project && !in_project { PrefSection::General } else { section };
        let stacked = window.viewport_size().width < PREFS_STACKED_BELOW;
        let title = div()
            .flex()
            .items_center()
            .child(Self::hand_tab("prefs", tr!("Preferences"), true))
            .child(div().flex_1())
            .child(div().text_color(c(theme::faint())).child("⌘,"))
            .into_any_element();
        let nav_item = |id: &'static str, label: &'static str, s: PrefSection, badge: Option<&'static str>| {
            let on = s == section;
            div()
                .id(id)
                .flex()
                .gap_2()
                .px_3()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .when(on, |d| d.bg(c(theme::line())).text_color(c(theme::text())))
                .when(!on, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text_2()))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.sheet = Some(Sheet::Prefs { section: s });
                    if s == PrefSection::Ai {
                        crate::assist::on_prefs_open(cx);
                    }
                    cx.notify();
                }))
                .child(label)
                .children(badge.map(|b| div().px_1().rounded_sm().border_1().border_color(c(theme::faint())).text_size(px(9.)).child(b)))
                .test_support()
        };
        let nav = div()
            .flex()
            .gap_1()
            .border_color(c(theme::line()))
            .when(stacked, |d| d.flex_wrap().pb_3().border_b_1())
            .when(!stacked, |d| d.w(px(200.)).flex_none().flex_col().pr_4().border_r_1())
            .child(nav_item("nav-general", tr!("General"), PrefSection::General, None))
            .child(nav_item("nav-page", tr!("Page"), PrefSection::Page, None))
            .child(nav_item("nav-writing", tr!("Writing"), PrefSection::Writing, None))
            .child(nav_item("nav-editing", tr!("Editing"), PrefSection::Editing, None))
            .child(nav_item("nav-ai", tr!("AI"), PrefSection::Ai, None))
            .when(in_project, |d| {
                d.child(div().when(stacked, |d| d.w(px(1.)).mx_1()).when(!stacked, |d| d.h(px(1.)).my_2()).bg(c(theme::line())))
                    .child(nav_item("nav-project", tr!("Project"), PrefSection::Project, None))
            });
        let rows: Vec<AnyElement> = match section {
            PrefSection::General => self.general_rows(&p, crate::assist::get(cx).is_some_and(|a| a.menu == Some(crate::assist::PrefsMenu::Language)), cx),
            PrefSection::Page => self.page_rows(&p, cx),
            PrefSection::Writing => vec![
                heading(tr!("IN WRITING MODE")),
                row(tr!("Zen writing"), Some(tr!("dim everything else")), toggle("zen", p.zen_writing, |p, v| p.zen_writing = v)).into_any_element(),
                // Like Private mode's rows, these stay set with zen off.
                row(
                    tr!("Stays bright"),
                    None,
                    segmented(
                        "zen-focus",
                        [(ZenFocus::Sentence, tr!("Sentence")), (ZenFocus::Line, trc!("a row of text", "Line")), (ZenFocus::Paragraph, tr!("Paragraph"))]
                            .into_iter()
                            .map(|(v, l)| (v, l.to_string()))
                            .collect(),
                        p.zen_focus,
                        |p, v| p.zen_focus = v,
                    ),
                )
                .into_any_element(),
                row(
                    tr!("Distance fade"),
                    None,
                    segmented(
                        "zen-fade",
                        [(ZenFade::Off, tr!("Off")), (ZenFade::Near, trc!("distance fade", "Near")), (ZenFade::WholePage, tr!("Whole page"))]
                            .into_iter()
                            .map(|(v, l)| (v, l.to_string()))
                            .collect(),
                        p.zen_fade,
                        |p, v| p.zen_fade = v,
                    ),
                )
                .into_any_element(),
                row(tr!("Typewriter scrolling"), Some(tr!("keep the line centred")), toggle("typewriter", p.typewriter, |p, v| p.typewriter = v)).into_any_element(),
                row(tr!("Private mode"), Some(tr!("hide the page’s text")), toggle("private", p.private_mode, |p, v| p.private_mode = v)).into_any_element(),
                // Both choices stay set with the mode off: they say what it does when it goes on.
                row(
                    tr!("Hidden text looks"),
                    None,
                    swatch_chips("hidden-look", HiddenLook::ALL.iter().map(|&l| (l, l.name().to_string(), hidden_swatch(l))).collect(), p.hidden_look, |p, v| {
                        p.hidden_look = v
                    }),
                )
                .into_any_element(),
                row(
                    tr!("Stays visible"),
                    None,
                    segmented(
                        "stays-visible",
                        [(StaysVisible::Word, tr!("Live word")), (StaysVisible::Sentence, tr!("Live sentence")), (StaysVisible::Nothing, tr!("Nothing"))]
                            .into_iter()
                            .map(|(v, l)| (v, l.to_string()))
                            .collect(),
                        p.stays_visible,
                        |p, v| p.stays_visible = v,
                    ),
                )
                .into_any_element(),
                heading(tr!("AS YOU TYPE")),
                row(tr!("Smart quotes and dashes"), Some(tr!("“ ” and —")), toggle("smart", p.smart_punctuation, |p, v| p.smart_punctuation = v)).into_any_element(),
                row(
                    tr!("Check spelling"),
                    None,
                    segmented(
                        "spelling",
                        [(SpellCheck::Always, tr!("Always")), (SpellCheck::Editing, tr!("Editing only")), (SpellCheck::Off, tr!("Off"))]
                            .into_iter()
                            .map(|(v, l)| (v, l.to_string()))
                            .collect(),
                        p.spell_check,
                        |p, v| p.spell_check = v,
                    ),
                )
                .into_any_element(),
            ],
            PrefSection::Editing => self.editing_rows(&p, cx),
            PrefSection::Ai => self.render_ai_prefs(cx),
            PrefSection::Project => self.project_rows(cx),
        };
        let body = div()
            .flex()
            .when(stacked, |d| d.flex_col().gap_3())
            .when(!stacked, |d| d.gap_6())
            .size_full()
            .child(nav)
            .child(div().id("pref-rows").flex_1().min_w_0().min_h_0().overflow_y_scroll().flex().flex_col().children(rows))
            .into_any_element();
        (title, body)
    }

    /// General: everything that applies to the whole app — language, the window's look, files, keys.
    fn general_rows(&mut self, p: &Prefs, language_open: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let reduce_motion = crate::platform::reduce_motion();
        vec![
            row(tr!("Language"), None, language_picker(p, language_open)).into_any_element(),
            heading(tr!("WINDOW")),
            row(
                tr!("Appearance"),
                None,
                segmented_with(
                    "appearance",
                    [(Appearance::Dark, tr!("Dark")), (Appearance::Light, tr!("Light")), (Appearance::System, tr!("Match macOS"))]
                        .into_iter()
                        .map(|(a, l)| (a, l.to_string(), Some(swatch(a)), None))
                        .collect(),
                    p.appearance,
                    |p, v| p.appearance = v,
                ),
            )
            .into_any_element(),
            row(
                tr!("Background opacity"),
                None,
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(slider("opacity", p.background_opacity, theme::MIN_OPACITY, 1., px(180.), |p, v| p.background_opacity = (v * 100.).round() / 100.))
                    .child(format!("{}%", (p.background_opacity * 100.).round())),
            )
            .into_any_element(),
            row(
                tr!("Hand font"),
                Some(tr!("panel titles and tabs")),
                segmented_with(
                    "hand-font",
                    HandFont::ALL.iter().map(|&h| (h, h.name().to_string(), None, Some((h.family(), px(19. * h.scale()))))).collect(),
                    p.hand_font,
                    |p, v| p.hand_font = v,
                ),
            )
            .into_any_element(),
            row(
                tr!("Playful effects"),
                Some(tr!("bursts and flights")),
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(div().when(reduce_motion, |d| d.opacity(0.45)).child(segmented(
                        "effects",
                        vec![(Effects::On, tr!("On").into()), (Effects::Subtle, tr!("Subtle").into()), (Effects::Off, tr!("Off").into())],
                        p.effects,
                        |p, v| p.effects = v,
                    )))
                    .when(reduce_motion, |d| d.child(div().text_color(c(theme::muted())).text_size(px(11.)).child(tr!("Reduce Motion is on")))),
            )
            .into_any_element(),
            heading(tr!("FILES")),
            row(
                tr!("Autosave"),
                None,
                segmented(
                    "autosave",
                    vec![(Autosave::Idle, tr!("When I pause").into()), (Autosave::Blur, tr!("When I leave").into()), (Autosave::Off, tr!("Off").into())],
                    p.autosave,
                    |p, v| p.autosave = v,
                ),
            )
            .into_any_element(),
            row(
                tr!("Keep backups"),
                Some(tr!("last 20 per document")),
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(toggle("backups", p.backups, |p, v| p.backups = v))
                    .child(
                        div()
                            .id("reveal-backups")
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(c(theme::muted()))
                            .hover(|d| d.text_color(c(theme::text())))
                            .on_click(|_, _, cx| {
                                if let Some(d) = crate::files::support_dir() {
                                    let b = d.join("Backups");
                                    let _ = std::fs::create_dir_all(&b);
                                    cx.reveal_path(&b);
                                }
                            })
                            .child(tr!("Show in Finder"))
                            .test_support(),
                    ),
            )
            .into_any_element(),
            row(
                tr!("Default folder"),
                Some(tr!("for new documents")),
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(div().text_color(c(theme::text_2())).child(p.default_folder.as_deref().map(tilde).unwrap_or_else(|| "~/Documents".into())))
                    .child(
                        button("choose-folder", tr!("Choose…"), false)
                            .on_click(cx.listener(|_, _, w, cx| {
                                let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: None });
                                cx.spawn_in(w, async move |_, cx| {
                                    if let Ok(Ok(Some(mut paths))) = rx.await {
                                        let dir = paths.pop();
                                        let _ = cx.update(|_, cx| set_prefs(cx, |p| p.default_folder = dir));
                                    }
                                })
                                .detach();
                            }))
                            .test_support(),
                    ),
            )
            .into_any_element(),
            #[cfg(feature = "updates")]
            row(
                tr!("Check for updates"),
                Some(tr!("once a day")),
                toggle_with("check-updates", p.check_updates == Some(true), crate::updates::set_enabled),
            )
            .into_any_element(),
            heading(tr!("KEYBOARD")),
            row(tr!("Tap ⇧ to select"), Some(tr!("word, sentence, paragraph")), toggle("tap-shift", p.tap_shift, |p, v| p.tap_shift = v)).into_any_element(),
            row(
                tr!("Shortcuts"),
                None,
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        button("show-shortcuts", tr!("Show all shortcuts"), false)
                            .flex()
                            .gap_2()
                            .children(shortcut_label(&ShowShortcuts, cx).map(|k| div().text_color(c(theme::muted())).child(k)))
                            .on_click(|_, w, cx| w.dispatch_action(Box::new(ShowShortcuts), cx))
                            .test_support(),
                    )
                    .child(
                        button("open-keymap", tr!("Open keymap.json"), false)
                            .on_click(|_, w, cx| w.dispatch_action(Box::new(OpenKeymapFile), cx))
                            .test_support(),
                    ),
            )
            .into_any_element(),
        ]
    }

    /// Page: typography only, with the open document drawn above the rows so each change shows at once.
    fn page_rows(&mut self, p: &Prefs, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // A document too new or too short to show anything is stood in for by Dostoevsky.
        let doc = PageSample::of(&self.model.read(cx).doc.text()).filter(|s| s.para.text.chars().count() >= 80);
        let (sample, caption) = match doc {
            Some(s) => (s, tr!("THE OPEN DOCUMENT")),
            None => (PageSample::excerpt(l10n::current()), tr!("A PASSAGE FROM DOSTOEVSKY")),
        };
        let mut rows = Vec::new();
        rows.push(
            div()
                .id("page-sample")
                .relative()
                .mb(px(10.))
                .rounded_lg()
                .border_1()
                .border_color(c(theme::line()))
                .bg(c(theme::bg()))
                .pt(px(14.))
                .px(px(22.))
                .pb(px(16.))
                .child(
                    div()
                        .id("page-sample-caption")
                        .absolute()
                        .top(px(7.))
                        .right(px(10.))
                        .text_size(px(10.5))
                        .text_color(c(theme::faint()))
                        .aria_label(caption)
                        .child(caption)
                        .test_support(),
                )
                .child(sample)
                .test_support()
                .into_any_element(),
        );
        rows.extend([
            row(
                tr!("Font"),
                None,
                segmented_with(
                    "font",
                    PAGE_FONTS.iter().enumerate().map(|(i, &(family, name))| (i, name.to_string(), None, Some((family, px(12.5))))).collect(),
                    PAGE_FONTS.iter().position(|&(family, _)| family == p.font_family).unwrap_or(0),
                    |p, v| p.font_family = PAGE_FONTS[v].0.into(),
                ),
            )
            .into_any_element(),
            row(
                tr!("Headings"),
                None,
                segmented_with(
                    "hand-headings",
                    vec![(false, tr!("Page font").to_string(), None, None), (true, tr!("Hand font").to_string(), None, Some((p.hand_font.family(), px(19. * p.hand_font.scale()))))],
                    p.hand_headings,
                    |p, v| p.hand_headings = v,
                ),
            )
            .into_any_element(),
            row(tr!("Text size"), None, segmented("size", [15, 17, 19, 21].map(|s| (s, s.to_string())).to_vec(), p.font_size.round() as i32, |p, v| p.font_size = v as f32))
                .into_any_element(),
            row(
                tr!("Column width"),
                Some(tr!("characters per line")),
                segmented("column", [58, 66, 74, 82, 90].map(|s| (s, s.to_string())).to_vec(), p.column_chars, |p, v| p.column_chars = v),
            )
            .into_any_element(),
            row(
                tr!("Line spacing"),
                None,
                segmented(
                    "spacing",
                    vec![(180, tr!("Snug").into()), (235, tr!("Airy").into()), (280, tr!("Loose").into())],
                    (p.line_spacing * 100.).round() as i32,
                    |p, v| p.line_spacing = v as f32 / 100.,
                ),
            )
            .into_any_element(),
            row(
                tr!("Alignment"),
                None,
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(segmented(
                        "alignment",
                        vec![(Alignment::Left, trc!("alignment", "Left").into()), (Alignment::Justified, tr!("Justified").into())],
                        p.alignment,
                        |p, v| p.alignment = v,
                    ))
                    // Hyphenation changes the same edge as alignment, and also smooths a ragged one.
                    .child(toggle("hyphenation", p.hyphenation, |p, v| p.hyphenation = v))
                    .child(div().text_size(px(12.5)).text_color(c(theme::text_2())).child(tr!("Hyphenate"))),
            )
            .into_any_element(),
            row(
                tr!("Paragraphs"),
                None,
                segmented(
                    "paragraphs",
                    vec![(ParagraphLook::Spaced, tr!("Spaced").into()), (ParagraphLook::Indented, tr!("Indented").into())],
                    p.paragraphs,
                    |p, v| p.paragraphs = v,
                ),
            )
            .into_any_element(),
        ]);
        rows
    }

    fn editing_rows(&mut self, p: &Prefs, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let remembered = crate::memory::Memory::count(cx);
        let faded = p.cut_look == CutLook::Text;
        let cut_option = |i: usize, on: bool, label: &'static str| {
            div()
                .id(("cut-look", i))
                .px_3()
                .py_1()
                .cursor_pointer()
                .when(i > 0, |d| d.border_l_1().border_color(c(theme::line())))
                .when(on, |d| d.bg(ca(theme::accent(), 0.22)).text_color(c(theme::text())))
                .when(!on, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text_2()))))
                .role(Role::RadioButton)
                .aria_label(label)
                .aria_selected(on)
                .on_click(move |_, _, cx| set_prefs(cx, |p| p.cut_look = if i == 0 { CutLook::Text } else { CutLook::Mark }))
                .flex()
                .items_center()
                .gap_2()
                .child(label)
                // The opacity is the faded text's, so it lives in the choice; a mark has none.
                .when(on && i == 0, |d| {
                    d.child(slider("ghost", p.ghost_opacity, 0.05, 0.30, px(90.), |p, v| p.ghost_opacity = (v * 100.).round() / 100.))
                        .child(div().text_size(px(11.5)).text_color(c(theme::text_2())).child(format!("{}%", (p.ghost_opacity * 100.).round())))
                })
                .test_support()
        };
        vec![
            row(
                tr!("Buttons on a selection"),
                Some(tr!("Alternatives, Comment, Cut")),
                toggle("selection-buttons", p.selection_buttons, |p, v| p.selection_buttons = v),
            )
            .into_any_element(),
            heading(tr!("ALTERNATIVES")),
            row(tr!("Show “version 3 of 5” on hover"), None, toggle("hover-label", p.hover_label, |p, v| p.hover_label = v)).into_any_element(),
            row(
                tr!("Scroll to switch versions"),
                None,
                segmented(
                    "wheel",
                    [(WheelSwitch::Always, tr!("Always")), (WheelSwitch::WithOption, tr!("With ⌥ held")), (WheelSwitch::Off, tr!("Off"))]
                        .into_iter()
                        .map(|(v, l)| (v, l.to_string()))
                        .collect(),
                    p.wheel,
                    |p, v| p.wheel = v,
                ),
            )
            .into_any_element(),
            row(
                tr!("Suggest earlier words"),
                Some(tr!("for the same word")),
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(toggle("memory", p.remember_pairings, |p, v| p.remember_pairings = v))
                    .when(remembered > 0, |d| {
                        d.child(div().text_size(px(11.5)).text_color(c(theme::muted())).child(trn!("{count} remembered", "{count} remembered", remembered, count = group_thousands(remembered))))
                            .child(button("forget-memory", tr!("Forget…"), false).on_click(cx.listener(|this, _, w, cx| this.forget_memory(w, cx))).test_support())
                    }),
            )
            .into_any_element(),
            row(
                tr!("Sounds"),
                Some(tr!("a tone per switch")),
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(toggle("sounds", p.sounds, |p, v| p.sounds = v))
                    .child(div().when(!p.sounds, |d| d.opacity(0.4)).child(slider("volume", p.volume, 0., 1., px(180.), |p, v| p.volume = v)))
                    .child(div().text_size(px(11.5)).text_color(c(theme::muted())).child(tr!("volume"))),
            )
            .into_any_element(),
            heading(tr!("CUTS AND COMMENTS")),
            row(
                tr!("Show cuts as"),
                None,
                div().flex().rounded_md().border_1().border_color(c(theme::line())).overflow_hidden().child(cut_option(0, faded, tr!("Faded text"))).child(cut_option(1, !faded, tr!("A mark"))),
            )
            .into_any_element(),
            row(
                tr!("Show comments as"),
                None,
                segmented(
                    "comment-look",
                    [(CommentLook::Notes, trc!("comments", "Notes")), (CommentLook::Ticks, tr!("Ticks"))].into_iter().map(|(v, l)| (v, l.to_string())).collect(),
                    p.comment_look,
                    |p, v| p.comment_look = v,
                ),
            )
            .into_any_element(),
        ]
    }

    fn forget_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let n = crate::memory::Memory::count(cx);
        let words = trn!("Forget the one remembered word?", "Forget {count} remembered words?", n, count = group_thousands(n));
        let steers = crate::memory::Memory::steer_count(cx);
        let title = if steers == 0 { words } else { trn!("{words} And the one steer.", "{words} And {n} steers.", steers, words = words) };
        let rx = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(tr!("Alternatives you wrote stay in your documents. Only the suggestions go.")),
            &[tr!("Forget"), tr!("Cancel")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if rx.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    crate::memory::Memory::clear(cx);
                    this.flash_status(tr!("Remembered words forgotten"), cx);
                });
            }
        })
        .detach();
    }

    fn render_detached(&mut self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let title = div().child(Self::hand_tab("detached", tr!("Detached"), true)).into_any_element();
        let groups = self.model.read(cx).kept.detached.clone();
        let workspace = cx.entity();
        let rows: Vec<AnyElement> = groups
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let n = g.variants.len();
                let scope = match g.scope {
                    Scope::Word => trn!("word · {n} variant", "word · {n} variants", n),
                    Scope::Sentence => trn!("sentence · {n} variant", "sentence · {n} variants", n),
                    Scope::Paragraph => trn!("paragraph · {n} variant", "paragraph · {n} variants", n),
                };
                let all = g.variants.iter().map(|v| v.text.clone()).collect::<Vec<_>>().join("\n");
                let link = |id: (&'static str, usize), label: &'static str| {
                    div().id(id).cursor_pointer().text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::accent()))).child(label)
                };
                let preview = g.variants.get(g.active).or_else(|| g.variants.first()).map(|v| crate::drag::preview(&v.text)).unwrap_or_default();
                let drag = crate::drag::DetachedDrag { index: i, scope: g.scope, preview, variants: n };
                let workspace = workspace.clone();
                div()
                    .id(("detached", i))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(c(theme::line()))
                    .on_drag(drag, move |d, _, w, cx| {
                        workspace.update(cx, |this, cx| this.begin_detached_drag(w, cx));
                        cx.new(|_| d.clone())
                    })
                    .test_support()
                    .child(div().text_size(px(11.)).text_color(c(theme::para_bar())).child(scope))
                    .children(g.variants.iter().enumerate().map(|(j, v)| {
                        div()
                            .text_color(if j == g.active { c(theme::text()) } else { c(theme::muted()) })
                            .child(v.text.chars().take(160).collect::<String>())
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .mt_1()
                            .child(link(("attach", i), tr!("Attach to selection")).on_click(cx.listener(move |this, _, _, cx| this.attach_detached(i, cx))).test_support())
                            .child(link(("copy-detached", i), tr!("Copy")).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(all.clone()))).test_support())
                            .child(link(("discard", i), tr!("Discard")).on_click(cx.listener(move |this, _, _, cx| this.discard_detached(i, cx))).test_support()),
                    )
                    .into_any_element()
            })
            .collect();
        let body = div()
            .id("detached-list")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_color(c(theme::text_2())).child(tr!(
                "These alternatives couldn't find their text after the file changed outside Dostoevsky. Select the text they belong to and press Attach, or copy or discard them. They are kept in the file until you do."
            )))
            .when(rows.is_empty(), |d| d.child(div().text_color(c(theme::muted())).child(tr!("Nothing is detached."))))
            .children(rows)
            .into_any_element();
        (title, body)
    }
}

/// Preferences → Page's live sample: the open document's first heading and paragraph,
/// shaped by the page's own wrapping, so every row below shows at once without closing
/// the sheet (mockup preferences-2, B).
struct PageSample {
    heading: Option<SampleBlock>,
    para: SampleBlock,
    out: Rc<RefCell<Option<Rc<SampleLayout>>>>,
}

#[derive(Clone)]
struct SampleBlock {
    text: String,
    /// 1–3 for a heading; the hand font or the page font follows the pref.
    level: Option<u8>,
}

struct SampleLine {
    wrapped: WrappedLine,
    rows: usize,
    /// (row, x within it, the shaped `-`).
    hyphens: Vec<(usize, Pixels, gpui_kit::ShapedLine)>,
    y: Pixels,
    line_height: Pixels,
    /// The paragraph's first-row indent.
    indent: Pixels,
}

struct SampleLayout {
    lines: Vec<SampleLine>,
}

/// The text as the page shows it: markers (`# `, `**`, `> `) never paint.
fn show_line(line: &str, style: Option<&LineStyle>) -> String {
    let mut out = String::new();
    let mut at = 0;
    if let Some(style) = style {
        for (r, span) in &style.spans {
            if *span == Span::Marker && r.start >= at {
                out.push_str(&line[at..r.start]);
                at = r.end;
            }
        }
    }
    out.push_str(&line[at..]);
    out.trim().to_string()
}

impl PageSample {
    /// The first heading (when the text opens with one) and the first paragraph.
    fn of(text: &str) -> Option<PageSample> {
        let lines: Vec<&str> = text.lines().take(8).collect();
        let styled = crate::markdown::style_lines_cached(lines.iter().copied(), &mut HashMap::new());
        let kind = |i: usize| styled.get(i).map_or(BlockKind::Paragraph, |s| s.kind);
        let shown = |i: usize| show_line(lines[i], styled.get(i));
        let first = (0..lines.len()).find(|&i| !lines[i].trim().is_empty());
        let (heading, from) = match first.map(|i| (i, kind(i))) {
            Some((i, BlockKind::Heading(n))) => (Some(SampleBlock { text: shown(i), level: Some(n.min(3)) }), i + 1),
            _ => (None, 0),
        };
        let para = (from..lines.len())
            .find(|&i| kind(i) == BlockKind::Paragraph && !shown(i).is_empty())
            .map(|i| SampleBlock { text: shown(i), level: None })?;
        Some(PageSample { heading, para, out: Rc::new(RefCell::new(None)) })
    }

    /// The opening of Crime and Punishment — the passage the welcome page tours with —
    /// in the UI's language, for a document with too little text to show.
    fn excerpt(lang: l10n::Lang) -> PageSample {
        let text = match lang {
            l10n::Lang::En => include_str!("../assets/excerpt/en.md"),
            l10n::Lang::De => include_str!("../assets/excerpt/de.md"),
            l10n::Lang::Ru => include_str!("../assets/excerpt/ru.md"),
            l10n::Lang::Fr => include_str!("../assets/excerpt/fr.md"),
            l10n::Lang::Es => include_str!("../assets/excerpt/es.md"),
            l10n::Lang::It => include_str!("../assets/excerpt/it.md"),
            l10n::Lang::PtBr => include_str!("../assets/excerpt/pt-BR.md"),
        };
        PageSample::of(text).expect("each excerpt is a heading and a long paragraph")
    }

    fn shape(block: &SampleBlock, ts: &WindowTextSystem, p: &Prefs, wrap: Pixels, body: bool, lang: Option<prose::Lang>, indent: Pixels) -> SampleLine {
        let lh = px((p.font_size * p.line_spacing).round());
        let (line_size, f) = match block.level {
            Some(level) if p.hand_headings => (crate::view::heading_size(px(p.font_size), lh, level), font(theme::hand())),
            _ => (px(p.font_size), Font { features: FontFeatures::disable_ligatures(), ..font(p.font_family.clone()) }),
        };
        let run = TextRun { len: block.text.len(), font: f.clone(), color: c(theme::text()), background_color: None, underline: None, strikethrough: None };
        let hyphen_w = ts.advance(ts.resolve_font(&f), line_size, '-').map(|s| s.width).unwrap_or(px(p.font_size * 0.3));
        let mut wrapped = ts
            .shape_text(block.text.clone().into(), line_size, std::slice::from_ref(&run), Some(wrap), None)
            .ok()
            .and_then(|mut v| v.pop())
            .unwrap_or_default();
        let breaks = move |w: &str| lang.map_or_else(|| Rc::from(Vec::<usize>::new()), |l| crate::hyphen::breaks(w, l));
        let hyphens = body.then_some(crate::wrap::Hyphens { breaks: &breaks, width: hyphen_w, keep: None });
        let hyphenated = crate::wrap::rewrap(&mut wrapped, wrap, hyphens.as_ref(), indent);
        if body && p.alignment == Alignment::Justified {
            crate::wrap::justify(&mut wrapped, wrap, &hyphenated, hyphen_w, 0, indent);
        }
        let ul = &wrapped.unwrapped_layout;
        let starts = std::iter::once(0)
            .chain(wrapped.wrap_boundaries().iter().map(|b| ul.runs[b.run_ix].glyphs[b.glyph_ix].index))
            .collect::<Vec<_>>();
        let rows = starts.len();
        let hyphens = hyphenated
            .iter()
            .enumerate()
            .filter(|(_, &h)| h)
            .map(|(r, _)| {
                let line = ts.shape_line("-".into(), line_size, std::slice::from_ref(&TextRun { len: 1, ..run.clone() }), None);
                (r, ul.x_for_index(starts.get(r + 1).copied().unwrap_or(block.text.len())) - ul.x_for_index(starts[r]), line)
            })
            .collect();
        SampleLine { wrapped, rows, hyphens, y: px(0.), line_height: lh, indent }
    }
}

impl Element for PageSample {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, ()) {
        let out = self.out.clone();
        let heading = self.heading.clone();
        let para = self.para.clone();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(w) => w,
                _ => px(300.),
            });
            let p = Prefs::get(cx).clone();
            let ts = window.text_system().clone();
            let regular = Font { features: FontFeatures::disable_ligatures(), ..font(p.font_family.clone()) };
            let char_w = crate::view::average_letter(|ch| ts.advance(ts.resolve_font(&regular), px(p.font_size), ch).ok().map(|s| s.width)).unwrap_or(px(p.font_size * 0.6));
            let column = (char_w * p.column_chars as f32).min(width);
            let lh = px((p.font_size * p.line_spacing).round());
            let lang = prose::detect(&para.text);
            // The page sets the first paragraph in only in a Russian text (ED-13).
            let indent = if p.paragraphs == ParagraphLook::Indented && lang == Some(prose::Lang::Ru) { (px(p.font_size) * 1.5).round() } else { px(0.) };
            let mut lines = Vec::new();
            let mut y = px(0.);
            if let Some(h) = &heading {
                let mut l = Self::shape(h, &ts, &p, column, false, None, px(0.));
                l.y = y;
                y += lh * l.rows as f32 + lh; // the blank line after the heading, as on the page
                lines.push(l);
            }
            let mut l = Self::shape(&para, &ts, &p, column, true, lang.filter(|_| p.hyphenation), indent);
            l.y = y;
            y += lh * l.rows as f32;
            lines.push(l);
            *out.borrow_mut() = Some(Rc::new(SampleLayout { lines }));
            size(width, y)
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(layout) = self.out.borrow().clone() else { return };
        for l in &layout.lines {
            let origin = point(bounds.origin.x, bounds.origin.y + l.y);
            if l.indent > px(0.) && l.rows > 1 {
                // Only the first row is set in: paint it shifted, then the rest from the column edge.
                let first = Bounds::from_corners(point(px(-1e5), px(-1e5)), point(px(1e5), origin.y + l.line_height));
                let rest = Bounds::from_corners(point(px(-1e5), origin.y + l.line_height), point(px(1e5), px(1e7)));
                window.with_content_mask(Some(ContentMask { bounds: first }), |window| {
                    let _ = l.wrapped.paint(point(origin.x + l.indent, origin.y), l.line_height, TextAlign::Left, None, window, cx);
                });
                window.with_content_mask(Some(ContentMask { bounds: rest }), |window| {
                    let _ = l.wrapped.paint(origin, l.line_height, TextAlign::Left, None, window, cx);
                });
            } else {
                let at = if l.indent > px(0.) { point(origin.x + l.indent, origin.y) } else { origin };
                let _ = l.wrapped.paint(at, l.line_height, TextAlign::Left, None, window, cx);
            }
            for (r, x, hyphen) in &l.hyphens {
                let at = point(origin.x + if *r == 0 { l.indent } else { px(0.) } + *x, origin.y + l.line_height * *r as f32);
                let _ = hyphen.paint(at, l.line_height, TextAlign::Left, None, window, cx);
            }
        }
    }
}

impl IntoElement for PageSample {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

const PREFS_STACKED_BELOW: Pixels = px(820.);

pub(crate) fn sheet_inset(window_width: Pixels) -> Pixels {
    ((window_width - px(900.)) / 2.).clamp(px(24.), px(140.))
}

fn tilde(path: &std::path::Path) -> String {
    match crate::platform::home() {
        Some(home) if path.starts_with(&home) => format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display()).trim_end_matches('/').to_string(),
        _ => path.display().to_string(),
    }
}

pub(crate) fn group_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{2009}');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::group_thousands;

    #[test]
    fn thousands() {
        assert_eq!(group_thousands(3778), "3\u{2009}778");
        assert_eq!(group_thousands(12), "12");
        assert_eq!(group_thousands(1_234_567), "1\u{2009}234\u{2009}567");
    }
}
