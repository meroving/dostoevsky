use std::fs;
use std::path::{Path, PathBuf};

/// More than the recent list holds: documents that dropped off it can still have a crash
/// journal or be a project's last file.
const KEPT: usize = 64;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Entry {
    path: PathBuf,
    /// In hex.
    data: String,
    /// A folder the user allowed the app to write in: never dropped.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    kept: bool,
}

fn store_path() -> Option<PathBuf> {
    crate::files::support_dir().map(|d| d.join("bookmarks.json"))
}

fn load() -> Vec<Entry> {
    store_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(list: &[Entry]) {
    let Some(p) = store_path() else { return };
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    // Renamed into place, so it's never half written; no fsync (10 ms on every open): losing
    // the last change to a crash costs one bookmark, which the next open makes again.
    let Ok(s) = serde_json::to_string(list) else { return };
    let tmp = p.with_extension("json.tmp");
    if fs::write(&tmp, s).is_ok() && fs::rename(&tmp, &p).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok())).collect()
}

/// Outside the sandbox every path is reachable anyway, so launch pays nothing. Tests opt in
/// with `PRETEND_SANDBOX`: a bookmark asks for the process's code identity, which lists the
/// executable's folder, and a test binary's is target/debug/deps (300k+ files, ~30 s).
fn active() -> bool {
    sandboxed()
}

pub fn sandboxed() -> bool {
    #[cfg(test)]
    if PRETEND_SANDBOX.get() {
        return true;
    }
    std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}

#[cfg(test)]
thread_local! {
    pub static PRETEND_SANDBOX: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Call it while the app has access: right after the user opened, dropped or saved `path`.
pub fn remember(path: &Path) {
    add(path, false);
}

pub fn remember_place(path: &Path) {
    add(path, true);
}

pub fn last_place() -> Option<PathBuf> {
    if !active() {
        return None;
    }
    load().into_iter().find(|e| e.kept).map(|e| e.path)
}

fn add(path: &Path, kept: bool) {
    if !active() {
        return;
    }
    let Some(data) = platform::bookmark(path) else { return };
    let mut list = load();
    let kept = kept || list.iter().any(|e| e.path == path && e.kept);
    list.retain(|e| e.path != path);
    list.insert(0, Entry { path: path.to_path_buf(), data: hex(&data), kept });
    let mut others = 0;
    list.retain(|e| {
        others += usize::from(!e.kept);
        e.kept || others <= KEPT
    });
    save(&list);
}

pub fn moved(from: &Path, to: &Path) {
    if !active() {
        return;
    }
    let mut list = load();
    let Some(i) = list.iter().position(|e| e.path == from) else { return };
    match platform::bookmark(to) {
        Some(data) => list[i] = Entry { path: to.to_path_buf(), data: hex(&data), kept: list[i].kept },
        None => list[i].path = to.to_path_buf(),
    }
    save(&list);
}

/// Applied on the main thread, which owns the files.
pub struct Fix {
    from: PathBuf,
    to: PathBuf,
    data: Option<String>,
}

/// Any thread. A bookmark that doesn't resolve stays in the store for when it's back.
fn resolve(entries: Vec<Entry>) -> (Vec<PathBuf>, Vec<Fix>) {
    let mut reachable = Vec::new();
    let mut fixes = Vec::new();
    for e in entries {
        let Some(r) = unhex(&e.data).and_then(|b| platform::resolve(&b)) else { continue };
        let same = r.path == e.path || fs::canonicalize(&r.path).ok() == fs::canonicalize(&e.path).ok();
        let to = if same { e.path.clone() } else { r.path };
        let data = r.fresh.map(|d| hex(&d));
        reachable.push(to.clone());
        if !same || data.is_some() {
            fixes.push(Fix { from: e.path, to, data });
        }
    }
    (reachable, fixes)
}

/// Before anything opens (~3 ms a bookmark); returns what was resolved, for [`restore_rest`].
pub fn restore_now(wanted: impl FnOnce() -> Vec<PathBuf>) -> Vec<PathBuf> {
    if !active() {
        return Vec::new();
    }
    let wanted = wanted();
    let urgent: Vec<Entry> = load().into_iter().filter(|e| wanted.iter().any(|w| w.starts_with(&e.path))).collect();
    let mut done: Vec<PathBuf> = urgent.iter().map(|e| e.path.clone()).collect();
    let (reachable, fixes) = resolve(urgent);
    apply(fixes);
    for p in reachable {
        if !done.contains(&p) {
            done.push(p);
        }
    }
    done
}

/// On a thread so launch doesn't wait (64 take about 200 ms); the fixes are for [`apply`] on
/// the main thread.
pub fn restore_rest(done: &[PathBuf]) -> Option<smol::channel::Receiver<Vec<Fix>>> {
    if !active() {
        return None;
    }
    let rest: Vec<Entry> = load().into_iter().filter(|e| !done.contains(&e.path)).collect();
    if rest.is_empty() {
        return None;
    }
    let (tx, rx) = smol::channel::bounded(1);
    std::thread::Builder::new()
        .name("bookmarks".into())
        .spawn(move || {
            let _ = tx.send_blocking(resolve(rest).1);
        })
        .ok()?;
    Some(rx)
}

pub fn apply(fixes: Vec<Fix>) {
    if fixes.is_empty() {
        return;
    }
    let mut list = load();
    for f in fixes {
        let Some(e) = list.iter_mut().find(|e| e.path == f.from) else { continue };
        if f.to != f.from {
            crate::files::recent_moved(&f.from, &f.to);
            e.path = f.to;
        }
        if let Some(d) = f.data {
            e.data = d;
        }
    }
    save(&list);
}

#[cfg(test)]
fn restore_all() -> Vec<PathBuf> {
    let (reachable, fixes) = resolve(load());
    apply(fixes);
    reachable
}

struct Resolved {
    path: PathBuf,
    /// New bytes for a stale bookmark.
    fresh: Option<Vec<u8>>,
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Resolved;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSData, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions};
    use std::path::Path;

    pub(super) fn bookmark(path: &Path) -> Option<Vec<u8>> {
        let url = NSURL::from_file_path(path)?;
        url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::WithSecurityScope,
            None,
            None,
        )
        .ok()
        .map(|d| d.to_vec())
    }

    pub(super) fn resolve(bytes: &[u8]) -> Option<Resolved> {
        let data = NSData::with_bytes(bytes);
        let mut stale = Bool::NO;
        // SAFETY: `stale` is a valid pointer for the call.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope
                    | NSURLBookmarkResolutionOptions::WithoutUI
                    // A server that isn't there mustn't hold up launch.
                    | NSURLBookmarkResolutionOptions::WithoutMounting,
                None,
                &mut stale,
            )
        }
        .ok()?;
        // Access for the rest of the session: never stopped, the system ends it at quit.
        // Outside the sandbox there's nothing to start and this says no, which is fine.
        // SAFETY: a plain message to a live NSURL.
        let _ = unsafe { url.startAccessingSecurityScopedResource() };
        let path = url.to_file_path()?;
        let fresh = if stale.as_bool() { bookmark(&path) } else { None };
        Some(Resolved { path, fresh })
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Resolved;
    use std::path::Path;

    pub(super) fn bookmark(_: &Path) -> Option<Vec<u8>> {
        None
    }

    pub(super) fn resolve(_: &[u8]) -> Option<Resolved> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{hex, last_place, load, moved, remember, remember_place, restore_all, restore_now, restore_rest, save, unhex, Entry, KEPT};
    use std::fs;

    #[test]
    fn hex_round_trips() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(unhex(&hex(&bytes)).unwrap(), bytes);
        assert_eq!(unhex("zz"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn remembered_paths_come_back_after_a_relaunch() {
        super::PRETEND_SANDBOX.set(true);
        let dir = std::env::temp_dir().join(format!("dsky-bookmarks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("book")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let doc = dir.join("Мой черновик.md");
        fs::write(&doc, "text").unwrap();
        remember(&doc);
        remember(&dir.join("book"));
        assert_eq!(load().len(), 2);
        assert!(restore_all().contains(&doc));

        let renamed = dir.join("Renamed.md");
        fs::rename(&doc, &renamed).unwrap();
        let back = restore_all();
        assert!(back.contains(&renamed), "{back:?}");
        assert!(load().iter().any(|e| e.path == renamed));

        let reordered = dir.join("01 Renamed.md");
        fs::rename(&renamed, &reordered).unwrap();
        moved(&renamed, &reordered);
        let paths: Vec<_> = load().into_iter().map(|e| e.path).collect();
        assert!(paths.contains(&reordered) && !paths.contains(&renamed), "{paths:?}");
        assert!(restore_all().contains(&reordered));

        let full: Vec<Entry> = (0..KEPT).map(|i| Entry { path: dir.join(format!("{i}.md")), data: "00".into(), kept: false }).collect();
        save(&full);
        let last = dir.join("last.md");
        fs::write(&last, "").unwrap();
        remember(&last);
        let list = load();
        assert_eq!((list.len(), &list[0].path), (KEPT, &last));

        let done = restore_now(|| vec![last.clone()]);
        assert_eq!(done, vec![last]);
        let rest = restore_rest(&done).unwrap().recv_blocking().unwrap();
        assert!(rest.is_empty(), "nothing moved or went stale");
        assert_eq!(load().len(), KEPT);

        let place = dir.join("book");
        assert_eq!(last_place(), None);
        remember_place(&place);
        for i in 0..KEPT {
            let f = dir.join(format!("more-{i}.md"));
            fs::write(&f, "").unwrap();
            remember(&f);
        }
        assert_eq!(load().len(), KEPT + 1);
        assert_eq!(last_place(), Some(place.clone()));
        remember(&place);
        assert_eq!(last_place(), Some(place), "opening it as a project keeps it kept");
        fs::remove_dir_all(&dir).unwrap();
    }
}
