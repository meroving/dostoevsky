#[cfg(target_os = "macos")]
pub fn reduce_motion() -> bool {
    use objc2_app_kit::NSWorkspace;
    if cfg!(test) {
        // Tests set motion through Prefs::effects, whatever the Mac says.
        return false;
    }
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// The desktop's own setting: GNOME's "Animations" off, or KDE's animation speed at instant;
/// on Windows, Animation effects off (Accessibility → Visual effects). Linux's is read on a
/// thread of its own every few seconds, so a change is seen without asking on every frame;
/// Windows' is one quick call. Tests set motion through Prefs::effects.
#[cfg(not(target_os = "macos"))]
pub fn reduce_motion() -> bool {
    #[cfg(test)]
    return false;
    #[cfg(all(windows, not(test)))]
    {
        use windows::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
        let mut animate = windows::core::BOOL(1);
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL where it's pointed.
        let read = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some((&raw mut animate).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
        read.is_ok() && !animate.as_bool()
    }
    #[cfg(not(any(test, windows)))]
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static REDUCE: AtomicBool = AtomicBool::new(false);
        static WATCH: std::sync::Once = std::sync::Once::new();
        WATCH.call_once(|| {
            let watch = move || {
                let mut gsettings = true;
                loop {
                    let gnome = gsettings.then(|| std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "enable-animations"]).output());
                    let gnome = match gnome {
                        Some(Ok(out)) if out.status.success() => gnome_reduces_motion(&String::from_utf8_lossy(&out.stdout)),
                        Some(Err(_)) => {
                            // No gsettings here: not a GNOME-family desktop.
                            gsettings = false;
                            None
                        }
                        _ => None,
                    };
                    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
                    let kde = config.and_then(|c| std::fs::read_to_string(c.join("kdeglobals")).ok()).and_then(|t| kde_reduces_motion(&t));
                    REDUCE.store(gnome.or(kde).unwrap_or(false), Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
            };
            let _ = std::thread::Builder::new().name("reduce-motion".into()).spawn(watch);
        });
        REDUCE.load(Ordering::Relaxed)
    }
}

/// `gsettings get org.gnome.desktop.interface enable-animations`: "false" reduces motion.
#[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
fn gnome_reduces_motion(output: &str) -> Option<bool> {
    match output.trim() {
        "false" => Some(true),
        "true" => Some(false),
        _ => None,
    }
}

/// KDE's `kdeglobals`: `AnimationDurationFactor=0` under `[KDE]` turns animations off.
#[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
fn kde_reduces_motion(kdeglobals: &str) -> Option<bool> {
    let mut in_kde = false;
    for line in kdeglobals.lines().map(str::trim) {
        if line.starts_with('[') {
            in_kde = line == "[KDE]";
        } else if let Some(v) = line.strip_prefix("AnimationDurationFactor=").filter(|_| in_kde) {
            return v.trim().parse::<f64>().ok().map(|f| f == 0.);
        }
    }
    None
}

/// Look Up (the Mac's dictionary panel) has no counterpart on Windows or Linux, so off the Mac the
/// page's menu leaves it out.
pub const HAS_LOOK_UP: bool = cfg!(target_os = "macos");

#[cfg(test)]
thread_local! {
    /// (text, window point) of each Look Up, for the UI tests.
    pub static LOOKED_UP: std::cell::RefCell<Vec<(String, gpui_kit::Point<gpui_kit::Pixels>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// `at` is the text's baseline in window coordinates, top-left origin.
#[cfg(all(target_os = "macos", not(test)))]
pub fn look_up(window: &gpui_kit::Window, text: &str, at: gpui_kit::Point<gpui_kit::Pixels>) {
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSAttributedString, NSPoint, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: AppKit handles carry the window's content view, alive while the window is.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let height = f32::from(window.viewport_size().height) as f64;
    let y = f32::from(at.y) as f64;
    // GPUI's view is not flipped: AppKit's y grows upwards.
    let point = NSPoint::new(f32::from(at.x) as f64, if view.isFlipped() { y } else { height - y });
    let s = NSAttributedString::from_nsstring(&NSString::from_str(text));
    view.showDefinitionForAttributedString_atPoint(Some(&s), point);
}

#[cfg(test)]
pub fn look_up(_: &gpui_kit::Window, text: &str, at: gpui_kit::Point<gpui_kit::Pixels>) {
    LOOKED_UP.with(|l| l.borrow_mut().push((text.to_string(), at)));
}

#[cfg(all(not(target_os = "macos"), not(test)))]
pub fn look_up(_: &gpui_kit::Window, _: &str, _: gpui_kit::Point<gpui_kit::Pixels>) {}

/// Scripted runs: the real pointer passes through, so it can't undo a scripted hover.
#[cfg(all(target_os = "macos", feature = "shots"))]
pub fn ignore_real_pointer(window: &gpui_kit::Window) {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: AppKit handles carry the window's content view, alive while the window is.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    if let Some(w) = view.window() {
        w.setIgnoresMouseEvents(true);
    }
}

/// The real home: under the App Sandbox `$HOME` is the app's container.
pub fn home() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?.into_string().ok()?;
    let real = home.split_once("/Library/Containers/").map_or(home.as_str(), |(h, _)| h);
    Some(std::path::PathBuf::from(real))
}

/// `None` when cancelled. Under the sandbox the folder chosen becomes writable.
#[cfg(all(target_os = "macos", not(test)))]
pub fn ask_for_folder(dir: &std::path::Path, message: &str, button: &str, _: &mut gpui_kit::App) -> smol::channel::Receiver<Option<std::path::PathBuf>> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSString, NSURL};

    let (tx, rx) = smol::channel::bounded(1);
    // GPUI calls in on the main thread.
    let Some(mtm) = MainThreadMarker::new() else {
        let _ = tx.try_send(None);
        return rx;
    };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    panel.setDirectoryURL(NSURL::from_directory_path(dir).as_deref());
    panel.setMessage(Some(&NSString::from_str(message)));
    panel.setPrompt(Some(&NSString::from_str(button)));
    let handler = RcBlock::new({
        let panel = panel.clone();
        move |response: NSModalResponse| {
            let chosen = (response == NSModalResponseOK).then(|| panel.URL().and_then(|u| u.to_file_path())).flatten();
            let _ = tx.try_send(chosen);
        }
    });
    panel.beginWithCompletionHandler(&handler);
    rx
}

#[cfg(all(not(target_os = "macos"), not(test)))]
pub fn ask_for_folder(_: &std::path::Path, _: &str, button: &str, cx: &mut gpui_kit::App) -> smol::channel::Receiver<Option<std::path::PathBuf>> {
    let (tx, rx) = smol::channel::bounded(1);
    let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(button.to_string().into()) });
    cx.background_executor()
        .spawn(async move {
            let chosen = paths.await.ok().and_then(|r| r.ok()).flatten().and_then(|p| p.into_iter().next());
            let _ = tx.send(chosen).await;
        })
        .detach();
    rx
}

#[cfg(test)]
thread_local! {
    /// Each folder panel's starting folder.
    pub static FOLDER_PANELS: std::cell::RefCell<Vec<std::path::PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The next folder panel's answer; `None` is cancelled.
    pub static FOLDER_ANSWER: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn ask_for_folder(dir: &std::path::Path, _: &str, _: &str, _: &mut gpui_kit::App) -> smol::channel::Receiver<Option<std::path::PathBuf>> {
    FOLDER_PANELS.with(|p| p.borrow_mut().push(dir.to_path_buf()));
    let (tx, rx) = smol::channel::bounded(1);
    let _ = tx.try_send(FOLDER_ANSWER.with(|a| a.borrow_mut().take()));
    rx
}

/// Through NSWorkspace, not `open`, which the sandbox may not allow.
#[cfg(target_os = "macos")]
pub fn open_file(path: &std::path::Path) {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace().openURL(&url);
}

/// In the app the system gives the file to: xdg-open's choice, or Windows' (Explorer opens a
/// file with its associated app).
#[cfg(not(target_os = "macos"))]
pub fn open_file(path: &std::path::Path) {
    let opener = if cfg!(windows) { "explorer" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

/// Returns the path in the Trash, so ⌘Z can bring it back.
#[cfg(all(target_os = "macos", not(test)))]
pub fn trash(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let mut landed = None;
    let done = NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed));
    done.map_err(|e| std::io::Error::other(e.localizedDescription().to_string()))?;
    landed
        .and_then(|u| u.path())
        .map(|p| std::path::PathBuf::from(p.to_string()))
        .ok_or_else(|| std::io::Error::other(l10n::tr!("the Trash didn't say where the file went")))
}

/// Linux and the other Unixes: the freedesktop.org Trash the file managers share.
#[cfg(all(unix, not(target_os = "macos"), not(test)))]
pub fn trash(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    crate::trash::trash(path)
}

/// Windows: the Recycle Bin, as Explorer's Delete. A file it can't take (a drive without one, a
/// file too big for it, which Windows would delete for good) goes to the app's own folder.
#[cfg(all(windows, not(test)))]
pub fn trash(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    match crate::recycle_bin::recycle(path) {
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => own_trash(path),
        other => other,
    }
}

/// Tests: a Trash folder of the app's own.
#[cfg(test)]
pub fn trash(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    own_trash(path)
}

/// A Trash folder of the app's own, in its support folder.
#[cfg(any(test, windows))]
fn own_trash(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let base = crate::files::support_dir().ok_or_else(|| std::io::Error::other("no support folder"))?;
    let bin = base.join("Trash");
    std::fs::create_dir_all(&bin)?;
    let to = crate::project::unused(&bin.join(path.file_name().unwrap_or_default()));
    std::fs::rename(path, &to)?;
    Ok(to)
}

/// Brings back what [`trash`] moved to `to`, and has the trash forget it.
pub fn untrash(to: &std::path::Path, from: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    return crate::trash::restore(to, from);
    #[cfg(windows)]
    return crate::recycle_bin::restore(to, from);
}

/// Turns off macOS font smoothing, with which GPUI thickens light text on dark. GPUI reads it
/// once, at the first glyph, so this must run before any text is drawn.
#[cfg(target_os = "macos")]
pub fn thin_glyphs() {
    use objc2_foundation::{NSNumber, NSString, NSUserDefaults};
    let key = NSString::from_str("AppleFontSmoothing");
    let defaults = NSUserDefaults::standardUserDefaults();
    // SAFETY: an NSNumber is a property-list object.
    unsafe { defaults.setObject_forKey(Some(&NSNumber::new_i32(0)), &key) };
}

#[cfg(not(target_os = "macos"))]
pub fn thin_glyphs() {}

/// System or per-app (`AppleLanguages`) order, e.g. `ru-RU`, `en-US`.
#[cfg(target_os = "macos")]
pub fn preferred_languages() -> Vec<String> {
    use objc2_foundation::{NSString, NSUserDefaults};
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults
        .stringArrayForKey(&NSString::from_str("AppleLanguages"))
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// The locale's languages, as the Mac writes them (`ru-RU`): `LANGUAGE`'s list, then `LC_ALL`,
/// `LC_MESSAGES`, `LANG`; not `C` or `POSIX`.
#[cfg(all(not(target_os = "macos"), not(windows)))]
pub fn preferred_languages() -> Vec<String> {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    locale_languages(&var("LANGUAGE"), &[var("LC_ALL"), var("LC_MESSAGES"), var("LANG")])
}

/// Windows' display languages in the writer's order (Settings → Time & language), as the Mac
/// writes them: `ru-RU`, `en-US`.
#[cfg(windows)]
pub fn preferred_languages() -> Vec<String> {
    use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let (mut count, mut len) = (0u32, 0u32);
    // SAFETY: first the length the list needs, then the list into a buffer that long.
    let list = unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).is_err() || len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len).is_err() {
            return Vec::new();
        }
        buf
    };
    ui_language_list(&list)
}

/// Brings the window to the front, restored if minimized. GPUI's `activate_window` presses Alt
/// to be allowed to, and the window, once in front, takes that Alt as its own: the menu bar shows
/// its keys and swallows the next one. This asks Windows directly, which works when the app is in
/// front or was given the right (a later launch gives it, `instance.rs`); otherwise Windows
/// flashes its taskbar button.
#[cfg(windows)]
pub fn bring_to_front(window: &gpui_kit::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow};

    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    let hwnd = HWND(h.hwnd.get() as *mut _);
    // SAFETY: the window's own handle, alive while it is; on its thread.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// A double-null-terminated list of names, UTF-16.
#[cfg(any(windows, test))]
fn ui_language_list(list: &[u16]) -> Vec<String> {
    list.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
}

#[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
fn locale_languages(language: &str, locales: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for locale in language.split(':').chain(locales.iter().map(String::as_str)) {
        // `ru_RU.UTF-8@euro` → `ru-RU`.
        let tag = locale.split(['.', '@']).next().unwrap_or("").replace('_', "-");
        if !tag.is_empty() && tag != "C" && tag != "POSIX" && !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

/// Opens the system colour panel on `hex` (sRGB, no alpha). Its choice is read back with
/// [`colour_panel_colour`] while it stays open.
#[cfg(all(target_os = "macos", not(test)))]
pub fn show_colour_panel(hex: u32) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSColor, NSColorPanel};
    let Some(mtm) = MainThreadMarker::new() else { return };
    let channel = |shift: u32| ((hex >> shift) & 0xff) as f64 / 255.;
    let panel = NSColorPanel::sharedColorPanel(mtm);
    panel.setShowsAlpha(false);
    panel.setColor(&NSColor::colorWithSRGBRed_green_blue_alpha(channel(16), channel(8), channel(0), 1.));
    panel.orderFront(None);
}

/// The panel's colour in sRGB; `None` once the writer has closed it.
#[cfg(all(target_os = "macos", not(test)))]
pub fn colour_panel_colour() -> Option<u32> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSColorPanel, NSColorSpace};
    let mtm = MainThreadMarker::new()?;
    if !NSColorPanel::sharedColorPanelExists(mtm) {
        return None;
    }
    let panel = NSColorPanel::sharedColorPanel(mtm);
    if !panel.isVisible() {
        return None;
    }
    let colour = panel.color().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let channel = |v: f64| ((v.clamp(0., 1.) * 255.).round() as u32) & 0xff;
    Some(channel(colour.redComponent()) << 16 | channel(colour.greenComponent()) << 8 | channel(colour.blueComponent()))
}

#[cfg(all(target_os = "macos", not(test)))]
pub fn close_colour_panel() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSColorPanel;
    if let Some(mtm) = MainThreadMarker::new() {
        if NSColorPanel::sharedColorPanelExists(mtm) {
            NSColorPanel::sharedColorPanel(mtm).close();
        }
    }
}

#[cfg(all(not(target_os = "macos"), not(test)))]
pub fn show_colour_panel(_: u32) {}

#[cfg(all(not(target_os = "macos"), not(test)))]
pub fn colour_panel_colour() -> Option<u32> {
    None
}

#[cfg(all(not(target_os = "macos"), not(test)))]
pub fn close_colour_panel() {}

#[cfg(test)]
thread_local! {
    /// Tests stand in for the writer at the colour panel: `Some` while it is open.
    pub static TEST_COLOUR_PANEL: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub fn show_colour_panel(hex: u32) {
    TEST_COLOUR_PANEL.with(|p| p.set(Some(hex)));
}

#[cfg(test)]
pub fn colour_panel_colour() -> Option<u32> {
    TEST_COLOUR_PANEL.with(|p| p.get())
}

#[cfg(test)]
pub fn close_colour_panel() {
    TEST_COLOUR_PANEL.with(|p| p.set(None));
}

#[cfg(test)]
mod tests {
    use super::locale_languages;

    #[test]
    fn a_locale_names_its_languages_as_the_mac_does() {
        let locales = ["".to_string(), "".to_string(), "ru_RU.UTF-8".to_string()];
        assert_eq!(locale_languages("", &locales), ["ru-RU"]);
        assert_eq!(locale_languages("de_AT:de:en", &locales), ["de-AT", "de", "en", "ru-RU"]);
        assert_eq!(locale_languages("", &["C.UTF-8".into(), "POSIX".into(), "sr_RS@latin".into()]), ["sr-RS"]);
    }

    #[test]
    fn windows_lists_its_languages_between_nulls() {
        let list: Vec<u16> = "ru-RU\0en-US\0\0".encode_utf16().collect();
        assert_eq!(super::ui_language_list(&list), ["ru-RU", "en-US"]);
        assert!(super::ui_language_list(&[0, 0]).is_empty());
    }

    /// The real list: never empty, every name a language tag.
    #[cfg(windows)]
    #[test]
    fn windows_has_display_languages() {
        let langs = super::preferred_languages();
        assert!(!langs.is_empty() && langs.iter().all(|l| l.len() >= 2 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')), "{langs:?}");
    }

    #[test]
    fn the_desktop_says_whether_to_reduce_motion() {
        use super::{gnome_reduces_motion, kde_reduces_motion};
        assert_eq!(gnome_reduces_motion("false\n"), Some(true));
        assert_eq!(gnome_reduces_motion("true\n"), Some(false));
        assert_eq!(gnome_reduces_motion("No such schema"), None);
        assert_eq!(kde_reduces_motion("[General]\nAnimationDurationFactor=0\n[KDE]\nAnimationDurationFactor=0\n"), Some(true));
        assert_eq!(kde_reduces_motion("[KDE]\nAnimationDurationFactor=0.5\n"), Some(false));
        assert_eq!(kde_reduces_motion("[General]\nAnimationDurationFactor=0\n"), None, "only under [KDE]");
    }
}
