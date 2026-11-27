//! Dostoevsky, a writing app that keeps every version of a word, sentence or paragraph in place.

mod about;
mod actions;
mod ai_prefs;
mod assist;
mod bookmarks;
mod clipboard;
mod comments;
mod context_menu;
mod count;
mod cuts;
mod cuts_panel;
mod drag;
mod dates;
mod drawer;
mod effects;
mod export_ui;
mod external;
mod files;
mod find;
mod format;
mod hunspell;
mod hyphen;
mod ideas;
#[cfg(not(target_os = "macos"))]
mod instance;
mod keymap;
mod lab;
mod list_panel;
mod markdown;
mod menu_bar;
mod memory;
mod model;
mod os_text;
mod overflow;
mod panel_foot;
mod panels;
mod pdf;
mod platform;
mod prefs;
mod project;
mod project_sheets;
mod project_ui;
#[cfg(windows)]
mod recycle_bin;
mod review;
mod review_text;
mod sound;
mod spell;
mod tagger;
#[cfg(windows)]
mod win_spell;
#[cfg(unix)]
mod trash;
mod text;
#[cfg(feature = "shots")]
mod shots;
mod sheets;
mod sentence_keys;
mod sidebar;
mod theme;
mod theme_import;
mod theme_ui;
mod title_bar;
#[cfg(test)]
mod ui_tests;
mod updates;
mod view;
mod welcome;
mod workspace;
mod wrap;

use std::path::{Path, PathBuf};

use gpui_kit::*;
use l10n::tr;

use actions::*;
use model::DocModel;
use prefs::Prefs;
use workspace::Workspace;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = app, no_json)]
pub struct OpenPath {
    pub path: String,
}

/// The Mac's bundle identifier; on Linux the windows' app id (Wayland) and class (X11), which
/// the desktop matches to `app.dostoevsky.Dostoevsky.desktop` for the name and icon.
pub const APP_ID: &str = "app.dostoevsky.Dostoevsky";

fn window_options(cx: &App) -> WindowOptions {
    let size = size(px(1180.), px(860.));
    #[cfg(feature = "shots")]
    let size = std::env::var("DOSTOEVSKY_WINDOW_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(gpui_kit::size(px(w.parse().ok()?), px(h.parse().ok()?)))
        })
        .unwrap_or(size);
    let n = cx.windows().len() as f32;
    let mut bounds = Bounds::centered(None, size, cx);
    // Whole points: a .5 origin makes macOS round both edges, and the window comes out 861 pt tall.
    bounds.origin.x = (bounds.origin.x + px(title_bar::CASCADE * n)).round();
    bounds.origin.y = (bounds.origin.y + px(title_bar::CASCADE * n)).round();
    // Off the Mac nothing moves a window back onto the screen: it opens inside the work area,
    // clear of the taskbar (dsky-fbso.10). A scripted run keeps the size it asks for.
    let sized_by_script = cfg!(feature = "shots") && std::env::var_os("DOSTOEVSKY_WINDOW_SIZE").is_some();
    if !cfg!(target_os = "macos") && !sized_by_script {
        if let Some(display) = cx.primary_display() {
            bounds = title_bar::fit_to_work_area(size, display.visible_bounds(), title_bar::CASCADE * n);
        }
    }
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Dostoevsky".into()),
            appears_transparent: true,
            // 14 px buttons, so 36 px high with this margin: centred on the header's line.
            traffic_light_position: Some(point(px(14.), px(11.))),
        }),
        window_min_size: Some(gpui_kit::size(px(title_bar::MIN_WINDOW_W), px(360.))),
        tabbing_identifier: Some("dostoevsky-document".into()),
        app_id: Some(APP_ID.into()),
        window_background: theme::window_background(Prefs::get(cx).background_opacity),
        // A scripted run stays in the background so typing elsewhere can't reach it.
        focus: !scripted(),
        ..Default::default()
    }
}

fn scripted() -> bool {
    cfg!(feature = "shots") && std::env::var_os("DOSTOEVSKY_SCRIPT").is_some()
}

#[derive(Default)]
struct Windows(Vec<(AnyWindowHandle, WeakEntity<Workspace>)>);

impl Global for Windows {}

pub fn open_model(model: DocModel, cx: &mut App) -> Option<AnyWindowHandle> {
    let opts = window_options(cx);
    let handle = gpui_kit::open_window(opts, cx, |window, cx| {
        cx.new(|cx| Workspace::new(model, window, cx))
    });
    match handle {
        Ok((h, ws)) => {
            cx.default_global::<Windows>().0.push((h, ws.downgrade()));
            #[cfg(feature = "shots")]
            if cx.windows().len() == 1 {
                shots::run(ws.downgrade(), h, cx);
            }
            if !scripted() {
                cx.activate(true);
            }
            Some(h)
        }
        Err(e) => {
            eprintln!("dostoevsky: could not open a window: {e}");
            None
        }
    }
}

/// What the front window shows, for the menus: its project's kind and its mode.
#[derive(Default, Clone, Copy, PartialEq)]
struct FrontWindow {
    kind: Option<project::Kind>,
    editing: bool,
}

impl Global for FrontWindow {}

pub fn refresh_menus(cx: &mut App) {
    let front = cx.try_global::<FrontWindow>().copied().unwrap_or_default();
    cx.set_menus(menus(&files::recent(), Prefs::get(cx), front.kind, front.editing));
}

fn set_front(change: impl FnOnce(&mut FrontWindow), cx: &mut App) {
    let was = cx.try_global::<FrontWindow>().copied();
    let mut front = was.unwrap_or_default();
    change(&mut front);
    if was != Some(front) {
        cx.set_global(front);
        refresh_menus(cx);
    }
}

pub fn set_front_project(kind: Option<project::Kind>, cx: &mut App) {
    set_front(|f| f.kind = kind, cx);
}

pub fn set_front_window(kind: Option<project::Kind>, editing: bool, cx: &mut App) {
    set_front(|f| *f = FrontWindow { kind, editing }, cx);
}

/// View checks Writing or Editing for the front window.
pub fn set_front_editing(editing: bool, cx: &mut App) {
    set_front(|f| f.editing = editing, cx);
}

pub(crate) fn represent_cuts(cx: &mut App) {
    for (_, ws) in workspaces(cx) {
        ws.update(cx, |ws, cx| ws.represent_cuts(cx));
    }
}

pub fn read_model(path: &Path) -> std::io::Result<DocModel> {
    read_model_as(path, None)
}

pub fn read_model_as(path: &Path, chapter: Option<(&str, &[doc_format::ExternalCut])>) -> std::io::Result<DocModel> {
    // A save still on its way would be read as the file's old words.
    files::wait_for_writes(Some(path));
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes).map_err(|_| std::io::Error::other(tr!("the file is not UTF-8 text")))?;
    let loaded = match chapter {
        Some((file, cuts)) => format::load_chapter(&text, file, cuts),
        None => format::load(&text),
    };
    let mut m = DocModel::new(loaded.doc);
    m.layer = loaded.layer;
    m.present();
    m.kept = loaded.kept;
    m.warnings = loaded.warnings;
    m.path = Some(path.to_path_buf());
    m.disk_text = Some(text);
    Ok(m)
}

fn workspaces(cx: &App) -> Vec<(AnyWindowHandle, Entity<Workspace>)> {
    let open = cx.windows();
    cx.try_global::<Windows>()
        .map(|w| {
            w.0.iter()
                .filter(|(h, _)| open.contains(h))
                .filter_map(|(h, ws)| Some((*h, ws.upgrade()?)))
                .collect()
        })
        .unwrap_or_default()
}

fn reuse_empty_window(cx: &mut App) {
    for (w, ws) in workspaces(cx) {
        let ws = ws.read(cx);
        let m0 = ws.model.read(cx);
        if ws.project.is_none() && m0.path.is_none() && !m0.dirty() && m0.doc.is_empty() {
            let _ = w.update(cx, |_, window, _| window.remove_window());
            break;
        }
    }
}

pub(crate) fn open_project(root: PathBuf, cx: &mut App) {
    for (w, ws) in workspaces(cx) {
        if ws.read(cx).project_root() == Some(root.as_path()) {
            let _ = w.update(cx, |_, window, _| window.activate_window());
            return;
        }
    }
    let tree = project::open(&root);
    let memory = files::project_memory(&root);
    let first = tree.opening_file(memory.last.as_deref(), project::Day::today());
    let model = first
        .and_then(|p| if p.exists() { read_model(&p).ok() } else { project_ui::load_model(&p).ok() })
        .unwrap_or_else(|| DocModel::new(doc_model::Document::new()));
    reuse_empty_window(cx);
    let Some(handle) = open_model(model, cx) else { return };
    if let Some((_, ws)) = workspaces(cx).into_iter().find(|(w, _)| *w == handle) {
        let _ = handle.update(cx, |_, window, cx| ws.update(cx, |ws, cx| ws.attach_project(root.clone(), window, cx)));
    }
    files::add_recent(&root);
    refresh_menus(cx);
}

pub fn bring_forward_other_window(path: &Path, me: EntityId, cx: &mut App) -> bool {
    for (w, ws) in workspaces(cx) {
        if ws.entity_id() != me && ws.read(cx).model.read(cx).path.as_deref() == Some(path) {
            let _ = w.update(cx, |_, window, _| window.activate_window());
            return true;
        }
    }
    false
}

pub fn files_moved(renames: &[project::Rename], cx: &mut App) {
    for (_, ws) in workspaces(cx) {
        let model = ws.read(cx).model.clone();
        for r in renames {
            if model.read(cx).path.as_deref() == Some(r.from.as_path()) {
                model.update(cx, |m, _| m.path = Some(r.to.clone()));
            }
        }
    }
}

pub fn open_separately(path: PathBuf, cx: &mut App) {
    if let Ok(m) = read_model(&path) {
        open_model(m, cx);
    }
}

pub fn open_path(path: PathBuf, cx: &mut App) {
    let path = path.canonicalize().unwrap_or(path);
    if path.is_dir() {
        open_project(path, cx);
        return;
    }
    for (w, ws) in workspaces(cx) {
        if ws.read(cx).model.read(cx).path.as_deref() == Some(path.as_path()) {
            let _ = w.update(cx, |_, window, _| window.activate_window());
            return;
        }
    }
    for (w, ws) in workspaces(cx) {
        if ws.read(cx).holds(&path) {
            let _ = w.update(cx, |_, window, cx| {
                window.activate_window();
                ws.update(cx, |ws, cx| ws.switch_to(&path, window, cx));
            });
            return;
        }
    }
    match read_model(&path) {
        Ok(m) => {
            reuse_empty_window(cx);
            open_model(m, cx);
            files::add_recent(&path);
            refresh_menus(cx);
        }
        Err(e) => {
            eprintln!("dostoevsky: could not open {}: {e}", path.display());
        }
    }
}

/// A path from the command line: a file that isn't there yet opens empty, to be saved there.
fn open_launch_path(p: PathBuf, cx: &mut App) {
    if p.exists() {
        open_path(p, cx);
    } else {
        let mut m = DocModel::new(doc_model::Document::new());
        m.path = Some(p);
        open_model(m, cx);
    }
}

/// The window last in front, which another launch brings forward (off the Mac nothing says
/// which window is front while another app is).
#[cfg(not(target_os = "macos"))]
struct LastFront(AnyWindowHandle);

#[cfg(not(target_os = "macos"))]
impl Global for LastFront {}

#[cfg(not(target_os = "macos"))]
pub fn note_front_window(window: AnyWindowHandle, cx: &mut App) {
    cx.set_global(LastFront(window));
}

/// Another launch's paths (dsky-fbso.20), opened as the command line's are; with none the app
/// comes forward, with a window if none is open.
#[cfg(not(target_os = "macos"))]
fn open_handed(paths: &[PathBuf], cx: &mut App) {
    for p in paths {
        open_launch_path(p.clone(), cx);
    }
    let all = workspaces(cx);
    if all.is_empty() {
        open_last_or_blank(cx);
        return;
    }
    let last = paths.last().map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
    let showing = |ws: &Entity<Workspace>, cx: &App| {
        last.as_deref().is_some_and(|p| ws.read(cx).model.read(cx).path.as_deref() == Some(p) || ws.read(cx).project_root() == Some(p))
    };
    let front = cx.try_global::<LastFront>().map(|f| f.0);
    let window = all
        .iter()
        .rev()
        .find(|(_, ws)| showing(ws, cx))
        .or_else(|| all.iter().find(|(h, _)| Some(*h) == front))
        .or(all.last())
        .map(|(h, _)| *h);
    if let Some(window) = window {
        let _ = window.update(cx, |_, window, _| window.activate_window());
    }
}

fn open_dialog(cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = rx.await {
            cx.update(|cx| {
                for p in paths {
                    open_path(p, cx);
                }
            });
        }
    })
    .detach();
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16);
    while i < b.len() {
        // Bytes, not a str slice: `%` before a multi-byte character mustn't panic.
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn file_paths(urls: &[String]) -> Vec<PathBuf> {
    urls.iter().filter_map(|u| u.strip_prefix("file://")).map(|p| PathBuf::from(percent_decode(p))).collect()
}

fn watch_keymap(cx: &mut App) {
    let Some(path) = keymap::path() else { return };
    let Some(dir) = path.parent().map(PathBuf::from) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let (tx, rx) = smol::channel::unbounded::<()>();
    let start = external::start_watcher(dir, notify::RecursiveMode::NonRecursive, move |e| {
        if e.paths.iter().any(|p| p.file_name() == path.file_name()) {
            let _ = tx.try_send(());
        }
    });
    cx.spawn(async move |cx| {
        let Some(_keep) = start.await else { return };
        while rx.recv().await.is_ok() {
            cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
            while rx.try_recv().is_ok() {}
            cx.update(|cx| {
                let errors = keymap::apply(cx);
                let msg = if errors.is_empty() { tr!("Key bindings reloaded").to_string() } else { format!("keymap.json: {}", errors.join("; ")) };
                for (_, ws) in workspaces(cx) {
                    ws.update(cx, |ws, cx| ws.flash_status(msg.clone(), cx));
                }
                refresh_menus(cx);
            });
        }
    })
    .detach();
}

/// The front window's workspace, or any, for actions the menu bar sends with nothing focused.
fn in_active_workspace(cx: &mut App, f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)) {
    let all = workspaces(cx);
    let active = cx.active_window();
    let Some((handle, ws)) = all.iter().find(|(h, _)| Some(*h) == active).or(all.first()).cloned() else { return };
    let _ = handle.update(cx, |_, window, cx| ws.update(cx, |ws, cx| f(ws, window, cx)));
}

thread_local! {
    /// The actions the app runs wherever the keys are (`register_actions`).
    static APP_ACTIONS: std::cell::RefCell<std::collections::HashSet<std::any::TypeId>> = Default::default();
}

/// An action the app runs with or without a window, which no menu greys out.
fn app_action<A: Action>(cx: &mut App, f: impl Fn(&A, &mut App) + 'static) {
    APP_ACTIONS.with(|a| a.borrow_mut().insert(std::any::TypeId::of::<A>()));
    cx.on_action(f);
}

pub fn is_app_action(action: &dyn Action) -> bool {
    APP_ACTIONS.with(|a| a.borrow().contains(&action.as_any().type_id()))
}

fn register_actions(cx: &mut App) {
    app_action(cx, |_: &OpenKeymapFile, _| {
        if let Some(p) = keymap::ensure_file() {
            platform::open_file(&p);
        }
    });
    // Runs only when nothing in a window has focus, so Settings… in the menu bar never greys out.
    app_action(cx, |a: &ShowPreferences, cx| in_active_workspace(cx, |ws, window, cx| ws.show_preferences(a, window, cx)));
    app_action(cx, |a: &CreateThemeSheet, cx| in_active_workspace(cx, |ws, window, cx| ws.create_theme_sheet(a, window, cx)));
    app_action(cx, |_: &About, cx| about::show(cx));
    app_action(cx, |_: &ShowWelcome, cx| welcome::open(cx));
    app_action(cx, |_: &ShowSupport, cx| cx.open_url(about::SUPPORT));
    #[cfg(feature = "updates")]
    app_action(cx, |_: &CheckForUpdates, cx| updates::check_now(cx));
    app_action(cx, |_: &Quit, cx| quit(cx));
    app_action(cx, |_: &Hide, cx| cx.hide());
    app_action(cx, |_: &HideOthers, cx| cx.hide_other_apps());
    app_action(cx, |_: &ShowAll, cx| cx.unhide_other_apps());
    app_action(cx, |_: &ToggleZenWriting, cx| crate::sheets::set_prefs(cx, |p| p.zen_writing = !p.zen_writing));
    app_action(cx, |_: &ToggleTypewriter, cx| crate::sheets::set_prefs(cx, |p| p.typewriter = !p.typewriter));
    app_action(cx, |_: &TogglePrivate, cx| crate::sheets::set_prefs(cx, |p| p.private_mode = !p.private_mode));
    app_action(cx, |_: &AlignLeft, cx| crate::sheets::set_prefs(cx, |p| p.alignment = prefs::Alignment::Left));
    app_action(cx, |_: &AlignJustified, cx| crate::sheets::set_prefs(cx, |p| p.alignment = prefs::Alignment::Justified));
    app_action(cx, |_: &ToggleHyphenation, cx| crate::sheets::set_prefs(cx, |p| p.hyphenation = !p.hyphenation));
    app_action(cx, |_: &ParagraphsSpaced, cx| crate::sheets::set_prefs(cx, |p| p.paragraphs = prefs::ParagraphLook::Spaced));
    app_action(cx, |_: &ParagraphsIndented, cx| crate::sheets::set_prefs(cx, |p| p.paragraphs = prefs::ParagraphLook::Indented));
    app_action(cx, |_: &AppearanceDark, cx| crate::sheets::set_prefs(cx, |p| p.appearance = prefs::Appearance::Dark));
    app_action(cx, |_: &AppearanceLight, cx| crate::sheets::set_prefs(cx, |p| p.appearance = prefs::Appearance::Light));
    app_action(cx, |_: &AppearanceSystem, cx| crate::sheets::set_prefs(cx, |p| p.appearance = prefs::Appearance::System));
    app_action(cx, |_: &ToggleAppearance, cx| {
        let to = if theme::is_light() { prefs::Appearance::Dark } else { prefs::Appearance::Light };
        crate::sheets::set_prefs(cx, |p| p.appearance = to);
    });
    // The same four sizes Preferences → Page offers (dsky-nke1.1); ⌘0 is the default, 19.
    app_action(cx, |_: &TextBigger, cx| crate::sheets::set_prefs(cx, |p| {
        p.step_text_size(1);
    }));
    app_action(cx, |_: &TextSmaller, cx| crate::sheets::set_prefs(cx, |p| {
        p.step_text_size(-1);
    }));
    app_action(cx, |_: &TextDefaultSize, cx| crate::sheets::set_prefs(cx, |p| {
        p.font_size = Prefs::TEXT_SIZES[2];
    }));
    app_action(cx, |_: &NewDocument, cx| {
        open_model(DocModel::new(doc_model::Document::new()), cx);
    });
    // In a project ⌘N adds to it (the window takes it first); this one's always a loose page.
    app_action(cx, |_: &NewLooseDocument, cx| {
        open_model(DocModel::new(doc_model::Document::new()), cx);
    });
    // A window takes it first; with none, New Project… opens one.
    app_action(cx, |_: &NewProject, cx| {
        let Some(handle) = open_model(DocModel::new(doc_model::Document::new()), cx) else { return };
        if let Some((_, ws)) = workspaces(cx).into_iter().find(|(w, _)| *w == handle) {
            let _ = handle.update(cx, |_, window, cx| ws.update(cx, |ws, cx| ws.new_project(&NewProject, window, cx)));
        }
    });
    app_action(cx, |_: &Open, cx| open_dialog(cx));
    app_action(cx, |a: &OpenPath, cx| open_path(PathBuf::from(&a.path), cx));
    app_action(cx, |_: &OpenRecent, cx| open_most_recent(cx));
}

/// A journal goes only once its window has its own, so nothing is ever only in memory.
fn restore_recoveries(cx: &mut App) -> usize {
    let mut n = 0;
    for r in files::pending_recoveries() {
        let loaded = format::load(&r.contents);
        let mut m = DocModel::new(loaded.doc);
        m.layer = loaded.layer;
        m.present();
        m.kept = loaded.kept;
        m.path = r.original.clone();
        // The file as the crash left it: what the recovered words are unsaved against, not a
        // change from outside for the watcher to ask about (and hold every save back on).
        m.disk_text = r.original.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
        m.version += 1; // unsaved
        m.warnings.push(tr!("Recovered unsaved changes from the last session.").into());
        if open_model(m, cx).is_some() {
            files::clear_journal(&r.journal);
            n += 1;
        }
    }
    n
}

fn open_last_or_blank(cx: &mut App) {
    if let Some(p) = files::recent().into_iter().next() {
        open_path(p, cx);
    }
    if workspaces(cx).is_empty() {
        if welcome::due(cx) {
            welcome::open_first(cx);
        } else {
            open_model(DocModel::new(doc_model::Document::new()), cx);
        }
    }
}

fn open_most_recent(cx: &mut App) {
    let mut open: Vec<PathBuf> = workspaces(cx).iter().filter_map(|(_, ws)| ws.read(cx).model.read(cx).path.clone()).collect();
    open.extend(workspaces(cx).iter().filter_map(|(_, ws)| ws.read(cx).project_root().map(Path::to_path_buf)));
    if let Some(p) = files::recent().into_iter().find(|p| p.exists() && !open.contains(p)) {
        open_path(p, cx);
    }
}

fn quit(cx: &mut App) {
    let mut all_closed = true;
    for (w, ws) in workspaces(cx) {
        let ok = w
            .update(cx, |_, window, cx| ws.update(cx, |ws, cx| ws.confirm_close(window, cx)))
            .unwrap_or(true);
        all_closed &= ok;
    }
    if all_closed {
        cx.quit();
    }
}

fn main() {
    // Off the Mac every launch is a process: a later one hands its paths to the first and goes
    // (dsky-fbso.20). A scripted run is always its own.
    #[cfg(not(target_os = "macos"))]
    let handed = if scripted() {
        None
    } else {
        match instance::claim(&instance::launch_paths(std::env::args_os().skip(1))) {
            instance::Claim::Handed => return,
            instance::Claim::Owner(listener) => Some(instance::serve(listener)),
            instance::Claim::Alone => None,
        }
    };
    platform::thin_glyphs();
    let app = gpui_kit::application().with_assets(theme::Assets);
    let (url_tx, url_rx) = smol::channel::unbounded::<Vec<String>>();
    app.on_open_urls(move |urls| {
        let _ = url_tx.try_send(urls);
    });
    // The Dock icon with no document open (the About window doesn't count).
    app.on_reopen(|cx| {
        if workspaces(cx).is_empty() {
            open_last_or_blank(cx);
        }
    });
    app.run(move |cx: &mut App| {
        // Sandbox: what launch opens is reachable now, the rest on a thread; the recent menu catches up.
        let done = bookmarks::restore_now(files::launch_paths);
        if let Some(rest) = bookmarks::restore_rest(&done) {
            cx.spawn(async move |cx| {
                if let Ok(fixes) = rest.recv().await {
                    cx.update(|cx| {
                        bookmarks::apply(fixes);
                        refresh_menus(cx);
                    });
                }
            })
            .detach();
        }
        gpui_kit::init(cx);
        theme::load_fonts(cx);
        let prefs = Prefs::load();
        crate::prefs::publish(&prefs);
        cx.set_global(prefs);
        l10n::set(Prefs::get(cx).ui_language());
        theme::reload();
        theme::apply(cx);
        sound::Sounds::init(cx);
        // Saves still on the writer thread finish before the process goes.
        cx.on_app_quit(|_| async { files::wait_for_writes(None) }).detach();

        memory::Memory::load(cx);
        let keymap_errors = keymap::apply(cx);
        for e in &keymap_errors {
            eprintln!("dostoevsky: {e}");
        }
        watch_keymap(cx);
        register_actions(cx);
        refresh_menus(cx);
        // Files Finder opened the app with arrive before launch finishes.
        let mut finder: Vec<PathBuf> = Vec::new();
        while let Ok(urls) = url_rx.try_recv() {
            finder.extend(file_paths(&urls));
        }
        cx.spawn(async move |cx| {
            while let Ok(urls) = url_rx.recv().await {
                cx.update(|cx| {
                    for p in file_paths(&urls) {
                        open_path(p, cx);
                    }
                });
            }
        })
        .detach();
        // Other launches' paths, after this one's own.
        #[cfg(not(target_os = "macos"))]
        if let Some(rx) = handed {
            cx.spawn(async move |cx| {
                while let Ok(h) = rx.recv().await {
                    if h.take() {
                        cx.update(|cx| open_handed(&h.paths, cx));
                        h.done();
                    }
                }
            })
            .detach();
        }
        let recovered = restore_recoveries(cx);

        let args: Vec<PathBuf> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).map(PathBuf::from).collect();
        for p in &finder {
            open_path(p.clone(), cx);
        }
        if args.is_empty() && finder.is_empty() && recovered == 0 {
            open_last_or_blank(cx);
        } else {
            for p in args {
                open_launch_path(p, cx);
            }
        }
        if !scripted() {
            cx.activate(true);
        }
        spell::Spelling::init_after_launch(cx);
        #[cfg(feature = "updates")]
        if !scripted() {
            updates::launched(cx);
        }
    });
}

#[cfg(test)]
mod url_tests {
    use super::{file_paths, percent_decode};
    use std::path::PathBuf;

    #[test]
    fn file_urls_from_finder() {
        let urls = [
            "file:///Users/me/My%20Essay.md".to_string(),
            "file:///Users/me/%D0%A7%D0%B5%D1%80%D0%BD%D0%BE%D0%B2%D0%B8%D0%BA.md".to_string(),
            "file:///Users/me/100%25%20done/".to_string(),
            "https://example.com/not-a-file".to_string(),
        ];
        assert_eq!(
            file_paths(&urls),
            [
                PathBuf::from("/Users/me/My Essay.md"),
                PathBuf::from("/Users/me/Черновик.md"),
                PathBuf::from("/Users/me/100% done/"),
            ]
        );
    }

    #[test]
    fn malformed_escapes_stay_as_they_are() {
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("end%2"), "end%2");
        assert_eq!(percent_decode("%aé"), "%aé");
        assert_eq!(percent_decode("%é"), "%é");
    }
}
