use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::tr;

use crate::actions::{Cancel, CloseWindow};
use crate::theme::{self, c};
use crate::workspace::Tip;

pub const SITE: &str = "https://dostoevsky.app";
pub const GITHUB: &str = "https://github.com/meroving/dostoevsky";
pub const SUPPORT: &str = "https://dostoevsky.app/support.html";
pub const PRIVACY: &str = "https://dostoevsky.app/privacy.html";

/// Both pages must fit, the links wrapping onto a second line in long languages.
pub const SIZE: (f32, f32) = (380., 520.);

const COPIED_FOR: Duration = Duration::from_millis(1500);

/// Name, maker; all under the SIL Open Font License 1.1.
const FONTS: [(&str, &str); 7] = [
    ("IBM Plex Mono", "IBM Corp."),
    ("JetBrains Mono", "JetBrains"),
    ("iA Writer Quattro", "Information Architects"),
    ("Literata", "TypeTogether"),
    ("Shantell Sans", "Arrow Type"),
    ("Caveat", "Impallari Type"),
    ("Bad Script", "Roman Shchyukin"),
];

const SOFTWARE: [(&str, &str, &str); 2] = [("GPUI", "Zed Industries", "Apache 2.0"), ("GPUI Kit", "Longbridge", "Apache 2.0")];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Page {
    About,
    Acknowledgements,
}

pub struct About {
    pub page: Page,
    pub copied: bool,
    copies: usize,
    focus: FocusHandle,
}

#[derive(Default)]
struct Open(Option<AnyWindowHandle>);

impl Global for Open {}

pub fn window(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<Open>().and_then(|o| o.0).filter(|h| cx.windows().contains(h))
}

pub fn show(cx: &mut App) {
    if let Some(h) = window(cx) {
        let _ = h.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let size = size(px(SIZE.0), px(SIZE.1));
    let mut bounds = Bounds::centered(None, size, cx);
    bounds.origin.x = bounds.origin.x.round();
    bounds.origin.y = bounds.origin.y.round();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(tr!("About Dostoevsky").into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(12.))),
        }),
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| About::new(window, cx))) {
        Ok((h, _)) => {
            cx.set_global(Open(Some(h)));
            cx.activate(true);
        }
        Err(e) => eprintln!("dostoevsky: could not open About: {e}"),
    }
}

impl About {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> About {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        About { page: Page::About, copied: false, copies: 0, focus }
    }

    pub fn copy_version(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(version_details()));
        self.copied = true;
        self.copies += 1;
        let n = self.copies;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |this, cx| {
                // A later click starts its own 1.5 s.
                if this.copies == n {
                    this.copied = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            Page::Acknowledgements => self.set_page(Page::About, cx),
            Page::About => window.remove_window(),
        }
    }

    fn close(&mut self, _: &CloseWindow, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    fn render_about(&self, cx: &mut Context<Self>) -> Div {
        let version = div()
            .id("about-version")
            .px(px(8.))
            .py(px(2.))
            .rounded(px(5.))
            .cursor_pointer()
            .when(self.copied, |d| d.bg(theme::ca(theme::accent(), 0.16)).text_color(c(theme::accent())).child(format!("✓ {}", tr!("Copied"))))
            .when(!self.copied, |d| {
                d.text_color(c(theme::text_2()))
                    .hover(|d| d.bg(theme::ca(theme::wash(), 0.06)))
                    .child(tr!("Version {version}", version = version_label()))
            })
            .tooltip(tip(tr!("Copy the version, for a bug report")))
            .on_click(cx.listener(|this, _, _, cx| this.copy_version(cx)))
            .test_support();
        let links = div()
            .flex()
            .flex_wrap()
            .justify_center()
            // No dots between them: a wrapped line would leave one hanging.
            .gap_x(px(18.))
            .gap_y(px(4.))
            .mt(px(18.))
            .child(link("about-site", "dostoevsky.app", SITE))
            .child(link("about-github", "GitHub", GITHUB))
            .child(link("about-support", tr!("Support"), SUPPORT))
            .child(link("about-privacy", tr!("Privacy"), PRIVACY));
        let thanks = crate::sheets::button("about-thanks", tr!("Acknowledgements"), false)
            .mt(px(18.))
            .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Acknowledgements, cx)))
            .test_support();
        div()
            .flex()
            .flex_col()
            .items_center()
            .px(px(24.))
            .child(img("icon.png").size(px(104.)).mb(px(6.)))
            .child(div().font_family(theme::hand()).text_size(theme::hand_px(38.)).line_height(px(40.)).mb(px(14.)).text_color(c(theme::text())).child("Dostoevsky"))
            .child(version)
            .child(links)
            .child(thanks)
            .child(
                div()
                    .id("about-foot")
                    .w_full()
                    .mt(px(20.))
                    .pt(px(14.))
                    .border_t_1()
                    .border_color(c(theme::line()))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_size(px(10.5))
                    .line_height(px(17.))
                    .text_color(c(if theme::is_light() { theme::muted() } else { theme::faint() }))
                    .child(tr!("© 2026 Dmitry Eibozhenko and contributors"))
                    .child(tr!("Free and open source, Apache License 2.0"))
                    .test_support(),
            )
    }

    fn render_acknowledgements(&self, cx: &mut Context<Self>) -> Div {
        let section = |t: &'static str| div().mt(px(14.)).mb(px(6.)).text_size(px(10.)).text_color(c(theme::faint())).child(t);
        let row = |name: &'static str, credit: String| {
            div()
                .flex()
                .justify_between()
                .gap_3()
                .line_height(px(22.))
                .border_b_1()
                .border_color(c(theme::line()))
                .child(div().text_color(c(theme::text_2())).child(name))
                .child(div().text_size(px(11.)).text_color(c(theme::muted())).child(credit))
        };
        let licences = licences_file();
        div()
            .flex()
            .flex_col()
            .px(px(26.))
            .child(
                div()
                    .id("about-back")
                    .cursor_pointer()
                    .text_color(c(theme::accent()))
                    .hover(|d| d.underline())
                    .child(tr!("‹ About"))
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::About, cx)))
                    .test_support(),
            )
            .child(div().mt(px(6.)).mb(px(4.)).font_family(theme::hand()).text_size(theme::hand_px(30.)).text_color(c(theme::text())).child(tr!("Acknowledgements")))
            .child(section(tr!("FONTS")))
            .children(FONTS.iter().map(|&(name, maker)| row(name, format!("{maker} · OFL 1.1"))))
            .child(section(tr!("SOFTWARE")))
            .children(SOFTWARE.iter().map(|&(name, maker, licence)| row(name, format!("{maker} · {licence}"))))
            .child(
                div()
                    .id("about-rest")
                    .mt(px(10.))
                    .mb(px(14.))
                    .text_size(px(11.5))
                    .line_height(px(18.))
                    .text_color(c(theme::muted()))
                    .child(tr!("And the Rust packages they are built on, under MIT, Apache 2.0, ISC, BSD and other open-source licences. Thank you to their authors."))
                    .test_support(),
            )
            .when_some(licences, |d, path| {
                d.child(
                    div().flex().justify_center().child(
                        crate::sheets::button("about-licences", tr!("Full licence texts…"), false)
                            .on_click(move |_, _, cx| cx.open_with_system(&path))
                            .test_support(),
                    ),
                )
            })
    }
}

impl Render for About {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.page {
            Page::About => self.render_about(cx),
            Page::Acknowledgements => self.render_acknowledgements(cx),
        };
        div()
            .id("about")
            .key_context("About")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::close))
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            // The title bar's strip, where the traffic lights sit.
            .pt(px(28.))
            .pb(px(12.))
            .bg(c(theme::bg_panel()))
            .font_family(theme::UI)
            .font_features(FontFeatures::disable_ligatures())
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(c(theme::text_2()))
            .child(body)
    }
}

fn link(id: &'static str, label: impl Into<SharedString>, url: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .cursor_pointer()
        .text_color(c(theme::accent()))
        .hover(|d| d.underline())
        .tooltip(tip(url.trim_start_matches("https://")))
        .on_click(move |_, _, cx| cx.open_url(url))
        .child(label.into())
        .test_support()
}

pub(crate) fn tip(label: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| {
        let label = label.clone();
        cx.new(|_| Tip::new(label, None)).into()
    }
}

fn bundle_contents() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?.to_path_buf();
    contents.join("Info.plist").is_file().then_some(contents)
}

/// The bundle's build number (the commit count), `None` in a cargo run.
fn build_number() -> Option<String> {
    let plist = std::fs::read_to_string(bundle_contents()?.join("Info.plist")).ok()?;
    plist_string(&plist, "CFBundleVersion")
}

fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = &plist[plist.find(&format!("<key>{key}</key>"))?..];
    let start = after.find("<string>")? + "<string>".len();
    let len = after[start..].find("</string>")?;
    Some(after[start..start + len].trim().to_string()).filter(|s| !s.is_empty())
}

pub fn version_label() -> String {
    format!("{} ({})", env!("CARGO_PKG_VERSION"), build_number().unwrap_or_else(|| "dev".into()))
}

fn channel() -> &'static str {
    if crate::bookmarks::sandboxed() {
        "App Store"
    } else if bundle_contents().is_some() {
        "direct download"
    } else {
        "dev"
    }
}

fn macos_version() -> Option<String> {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    plist_string(&plist, "ProductVersion")
}

/// The system and the chip: "macOS 15.1, Apple silicon", "Ubuntu 24.04 LTS, x86_64".
fn system_and_chip() -> String {
    if cfg!(target_os = "macos") {
        let chip = if cfg!(target_arch = "aarch64") { "Apple silicon" } else { "Intel" };
        let macos = macos_version().map(|v| format!("macOS {v}")).unwrap_or_else(|| "macOS".into());
        return format!("{macos}, {chip}");
    }
    let system = match std::env::consts::OS {
        "linux" => linux_name().unwrap_or_else(|| "Linux".into()),
        "windows" => "Windows".into(),
        other => other.into(),
    };
    format!("{system}, {}", std::env::consts::ARCH)
}

/// PRETTY_NAME from os-release, the one file every current distribution has.
fn linux_name() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").or_else(|_| std::fs::read_to_string("/usr/lib/os-release")).ok()?;
    os_release_name(&text)
}

fn os_release_name(text: &str) -> Option<String> {
    let value = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
    Some(value.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty())
}

/// In English, not translated: it is for whoever reads the bug report.
pub fn version_details() -> String {
    format!("Dostoevsky {}, {}, {}", version_label(), channel(), system_and_chip())
}

fn licences_file() -> Option<PathBuf> {
    Some(bundle_contents()?.join("Resources/THIRD-PARTY-LICENSES.txt")).filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::{os_release_name, plist_string, version_details, version_label};

    #[test]
    fn reads_a_string_from_a_plist() {
        let plist = "<dict>\n  <key>CFBundleShortVersionString</key><string>0.1.2</string>\n  <key>CFBundleVersion</key>\n  <string>178</string>\n</dict>";
        assert_eq!(plist_string(plist, "CFBundleVersion").as_deref(), Some("178"));
        assert_eq!(plist_string(plist, "CFBundleShortVersionString").as_deref(), Some("0.1.2"));
        assert_eq!(plist_string(plist, "CFBundleName"), None);
    }

    #[test]
    fn a_cargo_run_says_dev() {
        assert_eq!(version_label(), format!("{} (dev)", env!("CARGO_PKG_VERSION")));
        let d = version_details();
        let system = if cfg!(target_os = "macos") { "macOS " } else { "" };
        assert!(d.starts_with(&format!("Dostoevsky {} (dev), dev, {system}", env!("CARGO_PKG_VERSION"))), "{d}");
        assert!(cfg!(target_os = "macos") || !d.contains("macOS"), "{d}");
    }

    #[test]
    fn names_the_linux_distribution() {
        let text = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nID=ubuntu\n";
        assert_eq!(os_release_name(text).as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(os_release_name("PRETTY_NAME=Arch Linux"), Some("Arch Linux".into()));
        assert_eq!(os_release_name("ID=alpine"), None);
    }
}
