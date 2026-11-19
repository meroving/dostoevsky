//! Themes in the app (TECH-NOTES §15): the Theme row in Preferences → General (one split tile
//! per pair, its right-click menu) and the New / Edit theme sheet with three colours a half.

use std::collections::BTreeMap;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::tr;

use crate::actions::*;
use crate::context_menu::{Command, ContextMenu, Item};
use crate::prefs::Prefs;
use crate::sheets::{button, set_prefs, Sheet};
use crate::theme::{self, c, ca, HexColor, Palette, Theme};
use crate::view::{Mode, TextView, ViewEvent};
use crate::workspace::Workspace;

const TILE_W: f32 = 74.;
const TILE_H: f32 = 56.;
const TILE_R: f32 = 7.;
/// Where the slant between the halves meets the top and the bottom edge, as parts of the width:
/// steep enough that every line of the little page crosses it.
const SLANT: (f32, f32) = (0.62, 0.22);
/// The editor warns under AA, for normal-size text.
const READABLE: f32 = 4.5;

/// What a theme tile's menu does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThemeOp {
    Edit,
    Duplicate,
    Delete,
}

/// The Theme row's control: one tile per theme, built-ins first, then "New".
pub(crate) fn tiles(p: &Prefs, cx: &mut Context<Workspace>) -> AnyElement {
    let current = p.theme.clone();
    div()
        .id("theme-tiles")
        // Shares the row with its label and wraps inside it; only a narrow sheet puts it below.
        .flex_1()
        .min_w(px(3. * (TILE_W + 15.)))
        .flex()
        .flex_wrap()
        .gap(px(8.))
        .children(theme::all().into_iter().map(|(id, t, builtin)| tile(&id, &t, id == current, builtin, cx)))
        .child(
            column("new-theme")
                .child(
                    div()
                        .w(px(TILE_W))
                        .h(px(TILE_H))
                        .m(px(3.5))
                        .rounded(px(TILE_R))
                        .border_1()
                        .border_dashed()
                        .border_color(c(theme::faint()))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(18.))
                        .text_color(c(theme::muted()))
                        .child("+"),
                )
                .child(name_label(tr!("New").into(), false))
                .role(Role::Button)
                .aria_label(tr!("New Theme…"))
                .on_click(cx.listener(|this, _, window, cx| this.create_theme_sheet(&CreateThemeSheet, window, cx)))
                .test_support(),
        )
        .test_support()
        .into_any_element()
}

fn column(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).w(px(TILE_W + 7.)).flex().flex_col().items_center().gap(px(5.)).cursor_pointer()
}

fn name_label(name: SharedString, on: bool) -> Div {
    div().max_w(px(TILE_W + 7.)).truncate().text_size(px(11.)).text_color(if on { c(theme::text()) } else { c(theme::muted()) }).child(name)
}

/// A little page: the light half left of a slant, the dark half right of it, and lines of ink and
/// accent that change colour as they cross. The ring marks the theme in use.
fn tile(id: &str, t: &Theme, on: bool, builtin: bool, cx: &mut Context<Workspace>) -> AnyElement {
    let (light, dark) = (t.light.palette(true), t.dark.palette(false));
    let page = div()
        .relative()
        .w(px(TILE_W))
        .h(px(TILE_H))
        .rounded(px(TILE_R))
        .bg(c(light.bg))
        .child(canvas(|_, _, _| (), move |bounds, _, window, _| paint_split(bounds, &light, &dark, window)).absolute().inset_0())
        .child(div().absolute().inset_0().rounded(px(TILE_R)).border_1().border_color(ca(theme::muted(), 0.35)));
    let ring = div()
        .p(px(2.))
        .rounded(px(TILE_R + 3.))
        .border(px(1.5))
        .border_color(if on { c(theme::accent()) } else { ca(theme::accent(), 0.) })
        .child(page);
    let edit_id = id.to_string();
    let menu_id = id.to_string();
    column(SharedString::from(format!("theme-{id}")))
        .child(ring)
        .child(name_label(t.name.clone().into(), on))
        .role(Role::RadioButton)
        .aria_label(SharedString::from(t.name.clone()))
        .aria_selected(on)
        .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
            if e.click_count() >= 2 && !builtin {
                this.theme_op(&edit_id, ThemeOp::Edit, window, cx);
            } else if Prefs::get(cx).theme != edit_id {
                let id = edit_id.clone();
                set_prefs(cx, |p| p.theme = id);
            }
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                this.context_menu = Some(theme_menu(&menu_id, builtin, e.position));
                cx.notify();
            }),
        )
        .test_support()
        .into_any_element()
}

fn paint_split(b: Bounds<Pixels>, light: &Palette, dark: &Palette, window: &mut Window) {
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let at = |x: f32, y: f32| point(b.origin.x + px(x), b.origin.y + px(y));
    let (top, bottom) = (SLANT.0 * w, SLANT.1 * w);
    let r = TILE_R;
    let mut path = PathBuilder::fill();
    path.move_to(at(top, 0.));
    path.line_to(at(w - r, 0.));
    path.curve_to(at(w, r), at(w, 0.));
    path.line_to(at(w, h - r));
    path.curve_to(at(w - r, h), at(w, h));
    path.line_to(at(bottom, h));
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, c(dark.bg));
    }
    // Title, two lines of text, a word in the accent: as on the editor's sample page.
    let x0 = w * 11. / TILE_W;
    for (y, len, accent, alpha) in [(13., 34., false, 1.), (22., 50., false, 0.55), (30., 44., false, 0.55), (39., 26., true, 1.)] {
        let (y, x1) = (h * y / TILE_H, x0 + w * len / TILE_W);
        let split = top + (bottom - top) * (y + 1.) / h;
        for (from, to, p) in [(x0, x1.min(split), light), (x0.max(split), x1, dark)] {
            if to > from {
                let color = if accent { p.accent } else { p.text };
                window.paint_quad(fill(Bounds::new(at(from, y), size(px(to - from), px(2.))), ca(color, alpha)).corner_radii(px(1.)));
            }
        }
    }
}

fn theme_menu(id: &str, builtin: bool, at: Point<Pixels>) -> ContextMenu {
    let entry = |label: &'static str, op: ThemeOp| Some(Item { label: label.into(), shortcut: None, keys: None, command: Some(Command::Theme(id.to_string(), op)), badge: None });
    let items = if builtin {
        vec![entry(tr!("Edit a Copy…"), ThemeOp::Duplicate)]
    } else {
        let mut items = vec![entry(tr!("Edit…"), ThemeOp::Edit), entry(tr!("Duplicate"), ThemeOp::Duplicate)];
        if let Some(path) = theme::path(id) {
            items.push(Some(Item { label: tr!("Show in Finder").into(), shortcut: None, keys: None, command: Some(Command::ShowInFinder(path)), badge: None }));
        }
        items.extend([None, entry(tr!("Delete…"), ThemeOp::Delete)]);
        items
    };
    ContextMenu { at, heading: None, items, up: false }
}

/// Dostoevsky pins every token to keep its original look exactly, which would hide every seed
/// change in the editor: a copy of a built-in starts from its three seeds per half instead.
fn seeds_only(mut t: Theme) -> Theme {
    t.light.colors.clear();
    t.dark.colors.clear();
    t
}

fn field_box(id: impl Into<ElementId>, field: Entity<TextView>, width: Pixels) -> AnyElement {
    div()
        .id(id)
        .w(width)
        .px_2()
        .py_0p5()
        .rounded_md()
        .bg(c(theme::bg_raised()))
        .border_1()
        .border_color(c(theme::line()))
        .text_size(px(12.))
        .child(field)
        .test_support()
        .into_any_element()
}

impl Workspace {
    pub(crate) fn create_theme_sheet(&mut self, _: &CreateThemeSheet, window: &mut Window, cx: &mut Context<Self>) {
        let current = Prefs::get(cx).theme.clone();
        let mut draft = theme::find(&current).unwrap_or_default();
        if theme::is_builtin(&current) {
            draft = seeds_only(draft);
        }
        draft.name.clear();
        self.open_theme_editor(draft, None, window, cx);
    }

    pub(crate) fn select_theme(&mut self, action: &SelectTheme, _: &mut Window, cx: &mut Context<Self>) {
        if theme::find(&action.id).is_some() {
            let id = action.id.clone();
            set_prefs(cx, |p| p.theme = id);
        }
    }

    pub(crate) fn theme_op(&mut self, id: &str, op: ThemeOp, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = theme::find(id) else { return };
        match op {
            ThemeOp::Edit if !theme::is_builtin(id) => self.open_theme_editor(t, Some(id.to_string()), window, cx),
            ThemeOp::Edit | ThemeOp::Duplicate => {
                let mut copy = if theme::is_builtin(id) { seeds_only(t.clone()) } else { t.clone() };
                copy.name = tr!("{name} copy", name = t.name).to_string();
                self.open_theme_editor(copy, None, window, cx);
            }
            ThemeOp::Delete => self.confirm_delete_theme(id.to_string(), window, cx),
        }
    }

    fn confirm_delete_theme(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(theme) = theme::find(&id) else { return };
        let name = theme.name;
        let rx = window.prompt(
            PromptLevel::Warning,
            &tr!("Delete “{name}”?", name = name).to_string(),
            Some(tr!("Only the theme file goes; your writing is untouched.")),
            &[tr!("Delete"), tr!("Keep")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if rx.await == Ok(0) {
                let _ = this.update(cx, |_, cx| {
                    theme::delete(&id);
                    if Prefs::get(cx).theme == id {
                        set_prefs(cx, |p| p.theme = "dostoevsky".into());
                    } else {
                        set_prefs(cx, |_| {});
                    }
                    crate::refresh_menus(cx);
                });
            }
        })
        .detach();
    }

    /// `id` is the theme file being overwritten; `None` creates a new one.
    pub(crate) fn open_theme_editor(&mut self, draft: Theme, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let name = cx.new(|cx| {
            let mut field = TextView::new(Mode::Field, model.clone(), cx);
            field.set_field_text(&draft.name, cx);
            field.placeholder = tr!("e.g. Aurora").into();
            field
        });
        let mut subs = vec![cx.subscribe_in(&name, window, move |this, field, event, _, cx| {
            if matches!(event, ViewEvent::Edited) {
                let text = field.read(cx).field_text().to_string();
                if let Some(Sheet::ThemeEditor(editor)) = &mut this.sheet {
                    editor.draft.name = text;
                    editor.error = None;
                }
                cx.notify();
            }
        })];
        let mut seeds = BTreeMap::new();
        for (light, variant) in [(true, &draft.light), (false, &draft.dark)] {
            for (i, hex) in [variant.paper, variant.ink, variant.accent].into_iter().enumerate() {
                let (field, sub) = Self::seed_field(model.clone(), hex, light, i as u8, window, cx);
                seeds.insert((light, i as u8), field);
                subs.push(sub);
            }
        }
        self.open_sheet(
            Sheet::ThemeEditor(Box::new(ThemeEditor { id, draft, half: theme::is_light(), name, seeds, _subs: subs, error: None, picking: None })),
            window,
            cx,
        );
    }

    /// A `#rrggbb` field for one seed; what parses goes into the draft as it is typed.
    /// The subscription must live as long as the field, or edits go nowhere.
    fn seed_field(model: Entity<crate::model::DocModel>, hex: HexColor, light: bool, seed: u8, window: &mut Window, cx: &mut Context<Self>) -> (Entity<TextView>, Subscription) {
        let field = cx.new(|cx| {
            let mut field = TextView::new(Mode::Field, model, cx);
            field.set_field_text(&hex.to_string(), cx);
            field
        });
        let sub = cx.subscribe_in(&field, window, move |this, field, event, _, cx| {
            if !matches!(event, ViewEvent::Edited) {
                return;
            }
            let Some(hex) = HexColor::parse(field.read(cx).field_text().trim()) else { return };
            let Some(Sheet::ThemeEditor(editor)) = &mut this.sheet else { return };
            editor.set_seed(light, seed, hex);
            editor.error = None;
            cx.notify();
        });
        (field, sub)
    }

    /// The colour well: the system colour panel edits the seed until it closes or another well
    /// takes it. The panel has no callback here, so the sheet asks it while it is open.
    fn pick_colour(&mut self, seed: u8, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::ThemeEditor(editor)) = &mut self.sheet else { return };
        let half = editor.half;
        let Some(hex) = editor.seed(half, seed) else { return };
        let polling = editor.picking.is_some();
        editor.picking = Some((half, seed, hex.0));
        crate::platform::show_colour_panel(hex.0);
        if polling {
            return;
        }
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(120)).await;
            let more = this
                .update(cx, |this, cx| {
                    let Some(Sheet::ThemeEditor(editor)) = &mut this.sheet else {
                        crate::platform::close_colour_panel();
                        return false;
                    };
                    let Some((light, seed, seen)) = editor.picking else { return false };
                    let Some(colour) = crate::platform::colour_panel_colour() else {
                        editor.picking = None;
                        return false;
                    };
                    if colour != seen {
                        editor.picking = Some((light, seed, colour));
                        editor.set_seed(light, seed, HexColor(colour));
                        editor.error = None;
                        if let Some(field) = editor.seeds.get(&(light, seed)).cloned() {
                            field.update(cx, |f, cx| f.set_field_text(&HexColor(colour).to_string(), cx));
                        }
                        cx.notify();
                    }
                    true
                })
                .unwrap_or(false);
            if !more {
                break;
            }
        })
        .detach();
    }

    fn save_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::ThemeEditor(editor)) = &mut self.sheet else { return };
        let name = editor.name.read(cx).field_text().trim().to_string();
        if name.is_empty() {
            editor.error = Some(tr!("Give the theme a name.").to_string());
            cx.notify();
            return;
        }
        if editor.seeds.values().any(|field| HexColor::parse(field.read(cx).field_text().trim()).is_none()) {
            editor.error = Some(tr!("Colours use #rrggbb.").to_string());
            cx.notify();
            return;
        }
        editor.draft.name = name;
        let id = editor.id.clone().unwrap_or_else(|| theme::unique_id(&editor.draft.name));
        let draft = editor.draft.clone();
        match theme::save(&id, &draft) {
            Ok(()) => {
                set_prefs(cx, |p| p.theme = id);
                crate::refresh_menus(cx);
                self.close_sheet(window, cx);
            }
            Err(error) => {
                let Some(Sheet::ThemeEditor(editor)) = &mut self.sheet else { return };
                editor.error = Some(tr!("Couldn't save the theme: {error}", error = error).to_string());
                cx.notify();
            }
        }
    }

    /// A Dostoevsky `.json` someone shared, or one VS Code colour theme: saved and taken.
    fn import_theme_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(tr!("Import").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let job = crate::assist::off_thread(move || {
                paths
                    .into_iter()
                    .map(|path| {
                        let fallback = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Theme".into());
                        let text = std::fs::read_to_string(&path).map_err(|error| tr!("Couldn't read {path}: {error}", path = path.display(), error = error).to_string())?;
                        crate::theme_import::from_json(&text, &fallback)
                    })
                    .collect::<Vec<_>>()
            });
            let Some(results) = job.await else { return };
            let _ = this.update_in(cx, |this, window, cx| this.take_imported(results, window, cx));
        })
        .detach();
    }

    pub(crate) fn take_imported(&mut self, results: Vec<Result<Theme, String>>, window: &mut Window, cx: &mut Context<Self>) {
        let mut selected = None;
        let mut errors = Vec::new();
        for result in results {
            match result.and_then(|theme| {
                let id = theme::unique_id(&theme.name);
                theme::save(&id, &theme).map(|()| id).map_err(|e| e.to_string())
            }) {
                Ok(id) => selected = Some(id),
                Err(error) => errors.push(error),
            }
        }
        if let Some(id) = selected {
            set_prefs(cx, |p| p.theme = id);
            crate::refresh_menus(cx);
        }
        if errors.is_empty() {
            self.close_sheet(window, cx);
        } else {
            if let Some(Sheet::ThemeEditor(editor)) = &mut self.sheet {
                editor.error = Some(errors.join("; "));
            }
            cx.notify();
        }
    }

    pub(crate) fn render_theme_editor(&mut self, _: &mut Window, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let Some(Sheet::ThemeEditor(editor)) = &self.sheet else { return (Empty.into_any_element(), Empty.into_any_element()) };
        let editing = editor.id.is_some();
        let (half, error) = (editor.half, editor.error.clone());
        let variant = if half { &editor.draft.light } else { &editor.draft.dark };
        let palette = variant.palette(half);
        let title = div()
            .font_family(theme::hand())
            .text_size(theme::hand_px(25.))
            .text_color(c(theme::accent()))
            .child(if editing { tr!("Edit theme") } else { tr!("New theme") })
            .into_any_element();

        let p = Prefs::get(cx);
        let sample = div()
            .id("theme-sample")
            .flex_1()
            .min_w(px(260.))
            .rounded_lg()
            .border_1()
            .border_color(c(palette.line))
            .bg(c(palette.bg))
            .px_6()
            .py_5()
            .font_family(SharedString::from(p.font_family.clone()))
            .text_size(px(13.))
            .line_height(px(23.))
            .text_color(c(palette.text))
            .child(div().text_size(px(16.)).pb_2().child(tr!("Chapter One")))
            .child(div().child(tr!("Every word stays where it was written, while earlier versions wait underneath.")))
            .child(div().pt_2().text_color(c(palette.accent)).child(tr!("The right word, in its own colour.")))
            .child(div().pt_3().text_size(px(12.)).italic().text_color(c(palette.comment)).child(tr!("a note in the margin")))
            .test_support()
            .into_any_element();

        let segment = |light: bool, label: &'static str| {
            let on = light == half;
            div()
                .id(if light { "theme-half-light" } else { "theme-half-dark" })
                .px_3()
                .py_1()
                .cursor_pointer()
                .when(!light, |d| d.border_l_1().border_color(c(theme::line())))
                .when(on, |d| d.bg(ca(theme::accent(), 0.22)).text_color(c(theme::text())))
                .when(!on, |d| d.text_color(c(theme::muted())).hover(|d| d.text_color(c(theme::text_2()))))
                .role(Role::RadioButton)
                .aria_label(label)
                .aria_selected(on)
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(Sheet::ThemeEditor(editor)) = &mut this.sheet {
                        editor.half = light;
                    }
                    cx.notify();
                }))
                .test_support()
        };
        let halves = div().flex().rounded_md().border_1().border_color(c(theme::line())).overflow_hidden().child(segment(true, tr!("Light"))).child(segment(false, tr!("Dark")));

        let label = |text: &'static str| div().w(px(64.)).flex_none().text_color(c(theme::text())).child(text);
        let line = || div().flex().items_center().gap_3().py(px(9.)).border_b_1().border_color(c(theme::line()));
        let wells = [(tr!("Paper"), variant.paper.0, None), (tr!("Ink"), variant.ink.0, Some(palette.text)), (tr!("Accent"), variant.accent.0, Some(palette.accent))];
        let rows = wells.into_iter().enumerate().filter_map(|(seed, (name, hex, reads))| {
            let field = editor.seeds.get(&(half, seed as u8)).cloned()?;
            let hard = reads.is_some_and(|colour| theme::contrast_ratio(colour, palette.bg) < READABLE);
            Some(
                line()
                    .child(label(name))
                    .child(
                        div()
                            .id(SharedString::from(format!("theme-well-{seed}")))
                            .size(px(22.))
                            .flex_none()
                            .rounded_full()
                            .border_1()
                            .border_color(ca(theme::muted(), 0.45))
                            .bg(c(hex))
                            .cursor_pointer()
                            .role(Role::Button)
                            .aria_label(name)
                            .on_click(cx.listener(move |this, _, window, cx| this.pick_colour(seed as u8, window, cx)))
                            .test_support(),
                    )
                    .child(field_box(SharedString::from(format!("theme-seed-{seed}")), field, px(92.)))
                    .when(hard, |d| {
                        d.child(div().id(SharedString::from(format!("theme-hard-{seed}"))).text_size(px(11.)).text_color(c(theme::origin_hover_text())).child(tr!("hard to read")).test_support())
                    }),
            )
        });
        let controls = div()
            .w(px(300.))
            .flex_none()
            .flex()
            .flex_col()
            .child(line().child(label(tr!("Name"))).child(field_box("theme-name", editor.name.clone(), px(200.))))
            .child(line().child(div().w(px(64.)).flex_none()).child(halves))
            .children(rows);

        let footer = div()
            .flex()
            .items_center()
            .gap_3()
            .pt_3()
            .border_t_1()
            .border_color(c(theme::line()))
            .child(
                div()
                    .id("import-theme")
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(c(theme::muted()))
                    .hover(|d| d.text_color(c(theme::text())))
                    .child(tr!("Import a theme file…"))
                    .on_click(cx.listener(|this, _, window, cx| this.import_theme_files(window, cx)))
                    .test_support(),
            )
            .child(div().flex_1().min_w_0().text_size(px(11.)).text_color(c(theme::danger())).children(error))
            .child(button("theme-cancel", tr!("Cancel"), false).on_click(cx.listener(|this, _, window, cx| this.close_sheet(window, cx))).test_support())
            .child(button("theme-save", if editing { tr!("Save") } else { tr!("Create") }, true).on_click(cx.listener(|this, _, window, cx| this.save_theme(window, cx))).test_support());
        let body = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().id("theme-editor-body").flex_1().min_h_0().overflow_y_scroll().child(div().flex().flex_wrap().gap_6().child(sample).child(controls)))
            .child(footer)
            .into_any_element();
        (title, body)
    }
}

/// The New / Edit theme sheet's state. Text fields keep their own text; `draft` holds what parsed.
pub struct ThemeEditor {
    pub id: Option<String>,
    pub draft: Theme,
    /// The half being edited: `true` is the light one.
    pub half: bool,
    pub name: Entity<TextView>,
    /// (light?, seed 0 paper / 1 ink / 2 accent)
    pub seeds: BTreeMap<(bool, u8), Entity<TextView>>,
    _subs: Vec<Subscription>,
    error: Option<String>,
    /// The seed the colour panel edits, and the colour it last showed.
    picking: Option<(bool, u8, u32)>,
}

impl ThemeEditor {
    fn seed(&self, light: bool, seed: u8) -> Option<HexColor> {
        let v = if light { &self.draft.light } else { &self.draft.dark };
        [v.paper, v.ink, v.accent].get(seed as usize).copied()
    }

    fn set_seed(&mut self, light: bool, seed: u8, hex: HexColor) {
        let v = if light { &mut self.draft.light } else { &mut self.draft.dark };
        match seed {
            0 => v.paper = hex,
            1 => v.ink = hex,
            _ => v.accent = hex,
        }
    }
}
