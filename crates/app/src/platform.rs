#[cfg(target_os = "macos")]
pub fn reduce_motion() -> bool {
    use objc2_app_kit::NSWorkspace;
    if cfg!(test) {
        // Tests set motion through Prefs::effects, whatever the Mac says.
        return false;
    }
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
pub fn reduce_motion() -> bool {
    false
}

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

#[cfg(not(target_os = "macos"))]
pub fn open_file(path: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
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

/// Tests use a folder next to the test's data, not the real Trash.
#[cfg(any(test, not(target_os = "macos")))]
pub fn trash(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let base = crate::files::support_dir().ok_or_else(|| std::io::Error::other("no support folder"))?;
    let bin = base.join("Trash");
    std::fs::create_dir_all(&bin)?;
    let to = crate::project::unused(&bin.join(path.file_name().unwrap_or_default()));
    std::fs::rename(path, &to)?;
    Ok(to)
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

#[cfg(not(target_os = "macos"))]
pub fn preferred_languages() -> Vec<String> {
    std::env::var("LANG").map(|l| vec![l.split('.').next().unwrap_or("").to_string()]).unwrap_or_default()
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
