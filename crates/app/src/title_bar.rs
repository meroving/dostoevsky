//! The window's own title bar off the Mac (dsky-fbso.10). The Mac draws the traffic lights over
//! the transparent title bar; Windows hides its caption for the app's header, so the header
//! draws the caption buttons and tells Windows where they are (`WindowControlArea`: its snap
//! layouts flyout, the drag and the double-click to maximize stay the system's own). On Linux the
//! window manager or compositor draws the frame when it can; where it can't (GNOME on Wayland),
//! the window has client-side decorations and the app draws the buttons and the resize edges.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trc};

use crate::theme::{self, c, ca};

/// What draws the window's frame and buttons.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Frame {
    /// The Mac's traffic lights.
    Mac,
    /// Windows: the app draws the caption buttons in its header.
    Windows,
    /// Linux with client-side decorations: the app draws the buttons and the edges.
    LinuxClient,
    /// Linux with the window manager's own title bar.
    LinuxServer,
}

pub fn frame(window: &Window) -> Frame {
    if cfg!(target_os = "macos") {
        Frame::Mac
    } else if cfg!(target_os = "windows") {
        Frame::Windows
    } else if matches!(window.window_decorations(), Decorations::Client { .. }) {
        Frame::LinuxClient
    } else {
        Frame::LinuxServer
    }
}

/// The narrowest window: off the Mac the header also holds the window's buttons and the menus.
pub const MIN_WINDOW_W: f32 = if cfg!(target_os = "macos") { 520. } else { 640. };

/// Windows' caption buttons are 46 px wide, the height of the title bar.
const WINDOWS_BUTTON_W: f32 = 46.;
/// GNOME's round buttons: 24 px with 8 px between them and 6 px to the window's edge.
const LINUX_BUTTON: f32 = 24.;
const LINUX_GAP: f32 = 8.;
const LINUX_EDGE: f32 = 6.;

/// The header's room the window's buttons take at its right end.
pub fn buttons_width(frame: Frame, full_screen: bool) -> f32 {
    if full_screen {
        return 0.;
    }
    match frame {
        Frame::Windows => 3. * WINDOWS_BUTTON_W,
        Frame::LinuxClient => 3. * LINUX_BUTTON + 3. * LINUX_GAP + LINUX_EDGE - LINUX_GAP,
        Frame::Mac | Frame::LinuxServer => 0.,
    }
}

/// The caption buttons at the header's right end: minimize, maximize or restore, close.
pub fn buttons(window: &Window, height: f32) -> Option<AnyElement> {
    let frame = frame(window);
    if window.is_fullscreen() {
        return None;
    }
    let maximized = window.is_maximized();
    let active = window.is_window_active();
    match frame {
        Frame::Windows => Some(windows_buttons(maximized, active, height).into_any_element()),
        Frame::LinuxClient => Some(linux_buttons(maximized, active, height).into_any_element()),
        Frame::Mac | Frame::LinuxServer => None,
    }
}

/// Windows 11 draws its caption glyphs from Segoe Fluent Icons; Windows 10 has the same glyphs in
/// Segoe MDL2 Assets.
fn windows_glyph_font() -> &'static str {
    static FONT: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let fluent = std::env::var_os("WINDIR")
            .map(|w| std::path::Path::new(&w).join("Fonts").join("SegoeIcons.ttf").exists())
            .unwrap_or(false);
        if fluent { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" }
    })
}

fn windows_buttons(maximized: bool, active: bool, height: f32) -> impl IntoElement {
    let glyph_color = if active { c(theme::text_2()) } else { c(theme::faint()) };
    let button = |id: &'static str, glyph: &'static str, label: &'static str, area: WindowControlArea| {
        let close = area == WindowControlArea::Close;
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            // Over the header's drag area: Windows asks the app what's under the pointer, and
            // this answers the button rather than the caption.
            .occlude()
            .window_control_area(area)
            .w(px(WINDOWS_BUTTON_W))
            .h(px(height))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .font_family(windows_glyph_font())
            .text_size(px(10.))
            .text_color(glyph_color)
            .map(|d| {
                if close {
                    // Windows' own red, the same in light and dark.
                    d.hover(|s| s.bg(rgb(0xc42b1c)).text_color(white())).active(|s| s.bg(rgb(0xc63d30)).text_color(ca(0xffffff, 0.8)))
                } else {
                    d.hover(|s| s.bg(ca(theme::wash(), 0.06))).active(|s| s.bg(ca(theme::wash(), 0.04)))
                }
            })
            .child(glyph)
            .test_support()
    };
    let (max_glyph, max_label) = if maximized { ("\u{e923}", trc!("window button", "Restore")) } else { ("\u{e922}", trc!("window button", "Maximize")) };
    div()
        .id("caption-buttons")
        .flex()
        .flex_none()
        .h(px(height))
        .child(button("caption-minimize", "\u{e921}", trc!("window button", "Minimize"), WindowControlArea::Min))
        .child(button("caption-maximize", max_glyph, max_label, WindowControlArea::Max))
        .child(button("caption-close", "\u{e8bb}", tr!("Close"), WindowControlArea::Close))
        .test_support()
}

fn linux_buttons(maximized: bool, active: bool, height: f32) -> impl IntoElement {
    let glyph_color = if active { c(theme::text_2()) } else { c(theme::muted()) };
    let button = |id: &'static str, path: &'static str, label: &'static str| {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .occlude()
            .size(px(LINUX_BUTTON))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(ca(theme::wash(), 0.07))
            .hover(|s| s.bg(ca(theme::wash(), 0.13)))
            .active(|s| s.bg(ca(theme::wash(), 0.2)))
            .cursor_pointer()
            // A press here mustn't start a window drag in the header under it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(svg().path(path).size(px(10.)).text_color(glyph_color))
            .test_support()
    };
    let (max_path, max_label) =
        if maximized { ("chrome/window-restore.svg", trc!("window button", "Restore")) } else { ("chrome/window-maximize.svg", trc!("window button", "Maximize")) };
    div()
        .id("caption-buttons")
        .flex()
        .flex_none()
        .items_center()
        .h(px(height))
        .gap(px(LINUX_GAP))
        .pr(px(LINUX_EDGE))
        .child(button("caption-minimize", "chrome/window-minimize.svg", trc!("window button", "Minimize")).on_click(|_, window, _| window.minimize_window()))
        .child(button("caption-maximize", max_path, max_label).on_click(|_, window, _| window.zoom_window()))
        // As ⌘W, Ctrl+W: an unsaved document asks first.
        .child(button("caption-close", "chrome/window-close.svg", tr!("Close")).on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::CloseWindow), cx)))
        .test_support()
}

/// A small window's title strip (About): the drag area and, off the Mac, a close button.
pub fn strip(window: &Window, height: f32) -> AnyElement {
    let close = match frame(window) {
        Frame::Windows => Some(
            div()
                .id("caption-close")
                .role(Role::Button)
                .aria_label(tr!("Close"))
                .occlude()
                .window_control_area(WindowControlArea::Close)
                .w(px(WINDOWS_BUTTON_W))
                .h(px(height))
                .flex()
                .items_center()
                .justify_center()
                .font_family(windows_glyph_font())
                .text_size(px(10.))
                .text_color(if window.is_window_active() { c(theme::text_2()) } else { c(theme::faint()) })
                .hover(|s| s.bg(rgb(0xc42b1c)).text_color(white()))
                .child("\u{e8bb}")
                .into_any_element(),
        ),
        Frame::LinuxClient => Some(
            div()
                .id("caption-close")
                .role(Role::Button)
                .aria_label(tr!("Close"))
                .occlude()
                .mr(px(LINUX_EDGE))
                .size(px(LINUX_BUTTON))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(ca(theme::wash(), 0.07))
                .hover(|s| s.bg(ca(theme::wash(), 0.13)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::CloseWindow), cx))
                .child(svg().path("chrome/window-close.svg").size(px(10.)).text_color(c(theme::text_2())))
                .into_any_element(),
        ),
        Frame::Mac | Frame::LinuxServer => None,
    };
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(height))
        .flex()
        .items_center()
        .justify_end()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |e, window, _| header_mouse_down(e, window))
        .children(close)
        .into_any_element()
}

/// A press on the title bar's empty part: Windows moves and maximizes the window itself (the
/// part is its caption), Linux and the Mac ask the window to. The right button opens Linux's
/// window menu.
pub fn header_mouse_down(e: &MouseDownEvent, window: &mut Window) {
    match frame(window) {
        Frame::Windows => {}
        _ if e.button == MouseButton::Right => {
            if frame(window) == Frame::LinuxClient {
                window.show_window_menu(e.position);
            }
        }
        _ if e.click_count == 2 => window.zoom_window(),
        _ => window.start_window_move(),
    }
}

/// How far in from the window's edge a press resizes it, with client-side decorations.
const EDGE: f32 = 6.;
const CORNER: f32 = 14.;

/// With client-side decorations on Linux, the strips along the window's edges that resize it.
/// None where the window is tiled, maximized or full screen: those edges don't move.
pub fn resize_edges(window: &Window) -> Option<AnyElement> {
    let Decorations::Client { tiling } = window.window_decorations() else { return None };
    if !cfg!(target_os = "linux") || window.is_maximized() || window.is_fullscreen() {
        return None;
    }
    let edge = |id: &'static str, which: ResizeEdge, cursor: CursorStyle| {
        div()
            .id(id)
            .absolute()
            .occlude()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.start_window_resize(which);
            })
    };
    let (e, k) = (px(EDGE), px(CORNER));
    let mut strips: Vec<AnyElement> = Vec::new();
    if !tiling.top {
        strips.push(edge("resize-top", ResizeEdge::Top, CursorStyle::ResizeUpDown).top_0().left(k).right(k).h(e).into_any_element());
    }
    if !tiling.bottom {
        strips.push(edge("resize-bottom", ResizeEdge::Bottom, CursorStyle::ResizeUpDown).bottom_0().left(k).right(k).h(e).into_any_element());
    }
    if !tiling.left {
        strips.push(edge("resize-left", ResizeEdge::Left, CursorStyle::ResizeLeftRight).left_0().top(k).bottom(k).w(e).into_any_element());
    }
    if !tiling.right {
        strips.push(edge("resize-right", ResizeEdge::Right, CursorStyle::ResizeLeftRight).right_0().top(k).bottom(k).w(e).into_any_element());
    }
    if !tiling.top && !tiling.left {
        strips.push(edge("resize-top-left", ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight).top_0().left_0().size(k).into_any_element());
    }
    if !tiling.top && !tiling.right {
        strips.push(edge("resize-top-right", ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft).top_0().right_0().size(k).into_any_element());
    }
    if !tiling.bottom && !tiling.left {
        strips.push(edge("resize-bottom-left", ResizeEdge::BottomLeft, CursorStyle::ResizeUpRightDownLeft).bottom_0().left_0().size(k).into_any_element());
    }
    if !tiling.bottom && !tiling.right {
        strips.push(edge("resize-bottom-right", ResizeEdge::BottomRight, CursorStyle::ResizeUpLeftDownRight).bottom_0().right_0().size(k).into_any_element());
    }
    // A hairline round the window, as the system's frame would draw.
    Some(div().absolute().inset_0().border_1().border_color(c(theme::line())).children(strips).into_any_element())
}

/// Each further window opens this far down and right of the one before.
pub const CASCADE: f32 = 24.;

/// A window's first frame off the Mac: the size asked for, no bigger than the display's work
/// area (the screen less the taskbar or panels), and wholly inside it; `cascade` steps each
/// further window down and right, starting again in the middle where it would run off.
pub fn fit_to_work_area(size: Size<Pixels>, work: Bounds<Pixels>, cascade: f32) -> Bounds<Pixels> {
    const MARGIN: f32 = 16.;
    let w = size.width.min(work.size.width - px(2. * MARGIN)).max(px(1.));
    let h = size.height.min(work.size.height - px(2. * MARGIN)).max(px(1.));
    let centred = Bounds::centered_at(work.center(), gpui_kit::size(w, h));
    let room_x = f32::from(work.right() - w - centred.origin.x).max(0.);
    let room_y = f32::from(work.bottom() - h - centred.origin.y).max(0.);
    let steps = (room_x.min(room_y) / CASCADE).floor();
    let k = if steps < 1. { 0. } else { (cascade / CASCADE).round() % (steps + 1.) };
    let origin = point((centred.origin.x + px(k * CASCADE)).round(), (centred.origin.y + px(k * CASCADE)).round());
    Bounds::new(origin, gpui_kit::size(w, h))
}

#[cfg(test)]
mod tests {
    use super::fit_to_work_area;
    use gpui_kit::{point, px, size, Bounds};

    /// A 1536 × 864 screen at 125 %, its taskbar 48 px: the window fits under it.
    #[test]
    fn the_first_window_fits_the_work_area() {
        let work = Bounds::new(point(px(0.), px(0.)), size(px(1536.), px(816.)));
        for n in 0..12 {
            let b = fit_to_work_area(size(px(1180.), px(860.)), work, 24. * n as f32);
            assert!(b.left() >= work.left() && b.top() >= work.top(), "{n}: {b:?}");
            assert!(b.right() <= work.right() && b.bottom() <= work.bottom(), "{n}: {b:?}");
            assert_eq!(b.size.width, px(1180.), "{n}");
        }
        let first = fit_to_work_area(size(px(1180.), px(860.)), work, 0.);
        assert_eq!(first.size.height, px(816. - 32.));
        assert_eq!(first.origin, point(px(178.), px(16.)));
    }

    #[test]
    fn a_big_screen_keeps_the_size_and_cascades() {
        let work = Bounds::new(point(px(0.), px(25.)), size(px(2560.), px(1415.)));
        let a = fit_to_work_area(size(px(1180.), px(860.)), work, 0.);
        let b = fit_to_work_area(size(px(1180.), px(860.)), work, 24.);
        assert_eq!(a.size, size(px(1180.), px(860.)));
        assert_eq!(b.origin, a.origin + point(px(24.), px(24.)));
    }

    /// A small screen: the window shrinks to it rather than running off an edge.
    #[test]
    fn a_small_screen_shrinks_the_window() {
        let work = Bounds::new(point(px(-1280.), px(0.)), size(px(1280.), px(680.)));
        let b = fit_to_work_area(size(px(1180.), px(860.)), work, 48.);
        assert!(b.left() >= work.left() && b.right() <= work.right(), "{b:?}");
        assert!(b.top() >= work.top() && b.bottom() <= work.bottom(), "{b:?}");
    }
}
