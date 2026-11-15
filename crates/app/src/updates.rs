//! The .dmg build's update check (dsky-a8n.11). The writer is asked once, on the second launch,
//! with a quiet line; after a yes the app reads dostoevsky.app/version.json at most once a day,
//! and a newer version shows as a quiet line at the foot of the window. Check for Updates… works
//! either way. No Sparkle: Download opens the release page.
//!
//! The rules here are always built (and tested); the request, the menu item, the setting and
//! the line only with the `updates` feature, which the App Store build leaves out (App Review
//! 2.4.5: updates come from the store).
#![cfg_attr(not(feature = "updates"), allow(dead_code))]

use serde::{Deserialize, Serialize};

/// What version.json says (written by the site's build from the newest `vX.Y.Z` tag).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub version: String,
    /// The release page.
    pub url: String,
    #[serde(default)]
    pub min_macos: Option<String>,
}

/// Kept in updates.json in the support folder; whether to check at all is a preference.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub launches: u32,
    pub asked: bool,
    /// Seconds since 1970 of the last check, answered or not.
    pub last_check: u64,
    pub skipped: Option<String>,
}

pub const DAY: u64 = 24 * 60 * 60;

/// `1.2.10` → [1, 2, 10]; anything after a `-` or `+` is dropped, a part that isn't a number is 0.
fn parts(v: &str) -> Vec<u64> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next().unwrap_or("");
    let mut p: Vec<u64> = core.split('.').map(|s| s.trim().parse().unwrap_or(0)).collect();
    while p.len() > 1 && p.last() == Some(&0) {
        p.pop();
    }
    p
}

pub fn newer(theirs: &str, ours: &str) -> bool {
    parts(theirs) > parts(ours)
}

/// Whether a release should show: newer than this build, runs on this macOS, and not skipped
/// (a check the writer asked for shows a skipped one too).
pub fn offer(r: &Release, ours: &str, macos: Option<&str>, skipped: Option<&str>, asked_for: bool) -> bool {
    let runs_here = match (r.min_macos.as_deref(), macos) {
        (Some(min), Some(os)) => !newer(min, os),
        _ => true,
    };
    newer(&r.version, ours) && runs_here && (asked_for || skipped != Some(r.version.as_str()))
}

/// Once a day; a clock set back counts as due.
pub fn due(last_check: u64, now: u64) -> bool {
    now < last_check || now - last_check >= DAY
}

/// The ask comes on the second launch and only once; a writer who already chose isn't asked.
pub fn ask_now(s: &Saved, chosen: Option<bool>) -> bool {
    chosen.is_none() && !s.asked && s.launches >= 2
}

#[cfg(feature = "updates")]
pub use check::*;

#[cfg(feature = "updates")]
mod check {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use gpui_kit::*;
    use l10n::tr;

    use super::{ask_now, due, offer, Release, Saved};
    use crate::prefs::Prefs;
    use crate::theme::{self, c};

    const URL: &str = "https://dostoevsky.app/version.json";

    #[derive(Default)]
    struct Updates {
        saved: Saved,
        /// The once-only question is showing.
        ask: bool,
        available: Option<Release>,
        checking: bool,
    }

    impl Global for Updates {}

    fn file() -> Option<PathBuf> {
        crate::files::support_dir().map(|d| d.join("updates.json"))
    }

    fn save(s: &Saved) {
        if let Some(f) = file() {
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(json) = serde_json::to_vec_pretty(s) {
                let _ = std::fs::write(f, json);
            }
        }
    }

    fn now() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    fn macos() -> Option<String> {
        let out = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// A cask keeps its receipt here; `brew upgrade` is how its writer updates.
    fn homebrew() -> bool {
        ["/opt/homebrew/Caskroom/dostoevsky", "/usr/local/Caskroom/dostoevsky"].iter().any(|p| Path::new(p).exists())
    }

    /// No cookies, no identifiers: a plain GET of a static file.
    fn fetch() -> Option<Release> {
        let url = std::env::var("DOSTOEVSKY_UPDATES_URL").unwrap_or_else(|_| URL.into());
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut res = agent.get(&url).call().ok()?;
        if res.status() != 200 {
            return None;
        }
        let body = res.body_mut().with_config().limit(64 * 1024).read_to_string().ok()?;
        serde_json::from_str(&body).ok()
    }

    /// At launch: counts it, asks on the second, checks if due, and looks again every hour.
    pub fn launched(cx: &mut App) {
        let mut saved: Saved = file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        saved.launches = saved.launches.saturating_add(1);
        let ask = ask_now(&saved, Prefs::get(cx).check_updates);
        saved.asked |= ask;
        save(&saved);
        cx.set_global(Updates { saved, ask, ..Default::default() });
        cx.spawn(async move |cx| loop {
            cx.update(check_if_due);
            cx.background_executor().timer(Duration::from_secs(60 * 60)).await;
        })
        .detach();
    }

    fn check_if_due(cx: &mut App) {
        let last = cx.try_global::<Updates>().map(|u| u.saved.last_check).unwrap_or(0);
        if Prefs::get(cx).check_updates == Some(true) && due(last, now()) {
            check(false, cx);
        }
    }

    /// Check for Updates…: reports either way, in the front window's status line.
    pub fn check_now(cx: &mut App) {
        check(true, cx);
    }

    fn check(asked_for: bool, cx: &mut App) {
        let u = cx.default_global::<Updates>();
        if u.checking {
            return;
        }
        u.checking = true;
        let task = cx.background_executor().spawn(async { (fetch(), macos()) });
        cx.spawn(async move |cx| {
            let (release, os) = task.await;
            cx.update(|cx| {
                let u = cx.default_global::<Updates>();
                u.checking = false;
                u.saved.last_check = now();
                save(&u.saved);
                let ours = env!("CARGO_PKG_VERSION");
                let report = match &release {
                    Some(r) if offer(r, ours, os.as_deref(), u.saved.skipped.as_deref(), asked_for) => {
                        u.available = Some(r.clone());
                        u.ask = false;
                        None
                    }
                    Some(r) if super::newer(&r.version, ours) => None,
                    Some(_) => Some(tr!("Dostoevsky {version} is the newest version.", version = ours)),
                    None => Some(tr!("Couldn't check for updates.").to_string()),
                };
                if asked_for {
                    if let Some(msg) = report {
                        status(msg, cx);
                    }
                }
                cx.refresh_windows();
            });
        })
        .detach();
    }

    fn status(msg: String, cx: &mut App) {
        let all = crate::workspaces(cx);
        let active = cx.active_window();
        if let Some((_, ws)) = all.iter().find(|(h, _)| Some(*h) == active).or(all.first()).cloned() {
            ws.update(cx, |ws, cx| ws.flash_status(msg, cx));
        }
    }

    fn change(cx: &mut App, f: impl FnOnce(&mut Updates)) {
        f(cx.default_global::<Updates>());
        cx.refresh_windows();
    }

    fn link(id: &'static str, label: &'static str, on_click: impl Fn(&mut App) + 'static) -> impl IntoElement {
        div()
            .id(id)
            .role(Role::Link)
            .aria_label(label)
            .cursor_pointer()
            .text_color(c(theme::accent()))
            .hover(|d| d.underline())
            .child(label)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| on_click(cx))
            .test_support()
    }

    /// The quiet line's contents, or nothing: the question, or a newer version.
    pub fn line(cx: &App) -> Option<AnyElement> {
        let u = cx.try_global::<Updates>()?;
        if let Some(r) = u.available.clone() {
            let page = r.url.clone();
            let version = r.version.clone();
            let brew = homebrew();
            let text = if brew {
                tr!("Dostoevsky {version} is available. Update with brew upgrade.", version = r.version)
            } else {
                tr!("Dostoevsky {version} is available", version = r.version)
            };
            return Some(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(text)
                    .child(link("update-download", if brew { tr!("What's New") } else { tr!("Download") }, move |cx| {
                        cx.open_url(&page);
                        change(cx, |u| u.available = None);
                    }))
                    .child(link("update-skip", tr!("Skip"), move |cx| {
                        let v = version.clone();
                        change(cx, |u| {
                            u.available = None;
                            u.saved.skipped = Some(v);
                            save(&u.saved);
                        });
                    }))
                    .into_any_element(),
            );
        }
        u.ask.then(|| {
            let answer = |yes: bool| {
                move |cx: &mut App| {
                    change(cx, |u| u.ask = false);
                    crate::sheets::set_prefs(cx, |p| p.check_updates = Some(yes));
                    if yes {
                        check(false, cx);
                    }
                }
            };
            div()
                .flex()
                .gap(px(12.))
                .child(tr!("Check for new versions once a day?"))
                .child(link("updates-yes", tr!("Yes"), answer(true)))
                .child(link("updates-no", tr!("No"), answer(false)))
                .into_any_element()
        })
    }

    /// The setting: switching it on checks at once if a day has passed.
    pub fn set_enabled(cx: &mut App, on: bool) {
        change(cx, |u| u.ask = false);
        crate::sheets::set_prefs(cx, |p| p.check_updates = Some(on));
        if on {
            check_if_due(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ask_now, due, newer, offer, Release, Saved, DAY};

    fn release(v: &str, min: Option<&str>) -> Release {
        Release { version: v.into(), url: "https://example.org".into(), min_macos: min.map(Into::into) }
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("1.0.1", "1.0.0"));
        assert!(newer("1.10.0", "1.9.3"));
        assert!(newer("v2", "1.99.99"));
        assert!(!newer("1.0", "1.0.0"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("0.9.9", "1.0.0"));
        assert!(!newer("1.0.0-beta", "1.0.0"));
        assert!(!newer("garbage", "1.0.0"));
    }

    #[test]
    fn a_release_shows_when_newer_runs_here_and_isnt_skipped() {
        let r = release("1.1.0", Some("14.0"));
        assert!(offer(&r, "1.0.0", Some("15.2"), None, false));
        assert!(offer(&r, "1.0.0", Some("14.0"), None, false));
        assert!(!offer(&r, "1.0.0", Some("13.6.1"), None, false), "needs a newer macOS");
        assert!(offer(&r, "1.0.0", None, None, false), "an unknown macOS doesn't hide it");
        assert!(!offer(&r, "1.1.0", Some("15.2"), None, false));
        assert!(!offer(&r, "1.0.0", Some("15.2"), Some("1.1.0"), false), "skipped");
        assert!(offer(&r, "1.0.0", Some("15.2"), Some("1.1.0"), true), "asked for, it shows anyway");
        assert!(offer(&r, "1.0.0", Some("15.2"), Some("1.0.5"), false), "an older skip doesn't hide a newer one");
    }

    #[test]
    fn once_a_day() {
        assert!(due(0, 1_000_000));
        assert!(!due(1_000_000, 1_000_000 + DAY - 1));
        assert!(due(1_000_000, 1_000_000 + DAY));
        assert!(due(1_000_000, 999_000), "a clock set back");
    }

    #[test]
    fn asked_once_on_the_second_launch() {
        let at = |launches, asked| Saved { launches, asked, ..Saved::default() };
        assert!(!ask_now(&at(1, false), None));
        assert!(ask_now(&at(2, false), None));
        assert!(ask_now(&at(5, false), None), "an update from a build without the question");
        assert!(!ask_now(&at(3, true), None));
        assert!(!ask_now(&at(2, false), Some(false)));
        assert!(!ask_now(&at(2, false), Some(true)));
    }

    #[test]
    fn version_json_from_the_site() {
        let json = r#"{"version":"1.0.1","url":"https://github.com/meroving/dostoevsky/releases/tag/v1.0.1","dmg":"https://github.com/meroving/dostoevsky/releases/download/v1.0.1/Dostoevsky-1.0.1.dmg","min_macos":"13.0"}"#;
        let r: Release = serde_json::from_str(json).unwrap();
        assert_eq!(r.version, "1.0.1");
        assert_eq!(r.min_macos.as_deref(), Some("13.0"));
        let bare: Release = serde_json::from_str(r#"{"version":"2.0.0","url":"u"}"#).unwrap();
        assert_eq!(bare.min_macos, None);
    }
}
