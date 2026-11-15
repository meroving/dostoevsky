use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

const BACKUPS_KEPT: usize = 20;
const RECENT_KEPT: usize = 12;

/// Created on demand.
pub fn support_dir() -> Option<PathBuf> {
    // Each test thread has its own, so parallel tests stay apart.
    if cfg!(test) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        thread_local! {
            static DIR: PathBuf = std::env::temp_dir().join(format!(
                "dostoevsky-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        }
        return Some(DIR.with(Clone::clone));
    }
    if let Ok(dir) = std::env::var("DOSTOEVSKY_SUPPORT_DIR") {
        return Some(PathBuf::from(dir));
    }
    directories::BaseDirs::new().map(|b| b.data_dir().join("Dostoevsky"))
}

/// Keeps the original's permissions. On macOS the bytes go to a system temp folder on the same
/// volume and are swapped in with `replaceItemAtURL`: under the App Sandbox a temp file beside
/// a user-opened file is refused. A write of `path` queued by `write_later` lands first.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    wait_for_writes(Some(path));
    write_now(path, bytes)
}

fn write_now(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(swapped) = replace_from_temp_folder(path, bytes) {
        return match swapped {
            Ok(()) => Ok(()),
            Err(e) => write_beside(path, bytes).map_err(|_| e),
        };
    }
    write_beside(path, bytes)
}

/// `None` when the system has no temporary folder for `path`'s volume.
#[cfg(target_os = "macos")]
fn replace_from_temp_folder(path: &Path, bytes: &[u8]) -> Option<io::Result<()>> {
    use objc2_foundation::{
        NSFileManager, NSFileManagerItemReplacementOptions, NSSearchPathDirectory,
        NSSearchPathDomainMask, NSURL,
    };
    let name = path.file_name()?;
    let exists = path.exists();
    // The temp folder is picked per volume; a file that isn't there yet borrows its parent's.
    let near = if exists { path } else { path.parent().filter(|p| !p.as_os_str().is_empty())? };
    let near_url = NSURL::from_file_path(near)?;
    let fm = NSFileManager::defaultManager();
    let folder = fm
        .URLForDirectory_inDomain_appropriateForURL_create_error(
            NSSearchPathDirectory::ItemReplacementDirectory,
            NSSearchPathDomainMask::UserDomainMask,
            Some(&near_url),
            true,
        )
        .ok()?
        .to_file_path()?;
    let tmp = folder.join(name);
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if exists {
            let original = NSURL::from_file_path(path).ok_or_else(|| io::Error::other("no file URL"))?;
            let new = NSURL::from_file_path(&tmp).ok_or_else(|| io::Error::other("no file URL"))?;
            // Keeps the original's permissions, dates and extended attributes.
            fm.replaceItemAtURL_withItemAtURL_backupItemName_options_resultingItemURL_error(
                &original,
                &new,
                None,
                NSFileManagerItemReplacementOptions::empty(),
                None,
            )
            .map_err(|e| io::Error::other(e.localizedDescription().to_string()))?;
        } else {
            // Same volume, so a rename: the new file appears whole or not at all.
            fs::rename(&tmp, path)?;
        }
        if let Some(dir) = path.parent().and_then(|d| fs::File::open(d).ok()) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&folder);
    Some(result)
}

fn write_beside(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| io::Error::other("no file name"))?;
    let tmp = dir.join(format!("{}.tmp-{}", name.to_string_lossy(), std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, path)?;
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub(crate) fn path_key(path: &Path) -> String {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn now_stamp() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// One entry per queued write (a path may repeat).
static QUEUED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WRITTEN: Condvar = Condvar::new();

struct Job {
    path: PathBuf,
    bytes: Vec<u8>,
    backup_to: Option<PathBuf>,
    /// Removed once the write lands.
    journal: Option<PathBuf>,
    done: smol::channel::Sender<io::Result<()>>,
}

/// On the writer thread, in order. `journal` is removed here on success, so quitting straight
/// after leaves no stale recovery. Whatever reads, writes or moves the file waits for it first.
pub fn write_later(path: &Path, bytes: Vec<u8>, backup: bool, journal: Option<PathBuf>) -> smol::channel::Receiver<io::Result<()>> {
    static WRITER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    let (done, result) = smol::channel::bounded(1);
    // Tests give each thread its own support folder, so it's looked up here, not on the writer.
    let job = Job { path: path.to_path_buf(), bytes, backup_to: backup.then(support_dir).flatten(), journal, done };
    lock(&QUEUED).push(job.path.clone());
    let writer = WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new().name("dostoevsky saves".into()).spawn(move || {
            for job in rx {
                run(job);
            }
        });
        if let Err(e) = spawned {
            eprintln!("dostoevsky: no thread for saves: {e}");
        }
        Mutex::new(tx)
    });
    let unsent = lock(writer).send(job);
    if let Err(mpsc::SendError(job)) = unsent {
        run(job);
    }
    result
}

fn run(job: Job) {
    /// Takes the write off the queue when dropped, even by a panic: nothing waits forever.
    struct Unqueue<'a>(&'a Path);
    impl Drop for Unqueue<'_> {
        fn drop(&mut self) {
            let mut queued = lock(&QUEUED);
            if let Some(i) = queued.iter().position(|p| p == self.0) {
                queued.remove(i);
            }
            drop(queued);
            WRITTEN.notify_all();
        }
    }
    let unqueue = Unqueue(&job.path);
    let result = write_now(&job.path, &job.bytes);
    if result.is_ok() {
        if let Some(base) = &job.backup_to {
            let _ = backup_in(base, &job.path, &job.bytes);
        }
        if let Some(j) = &job.journal {
            clear_journal(j);
        }
    }
    // Sent before unqueueing: whoever waited for the queue knows the outcome is on its way.
    let _ = job.done.try_send(result);
    drop(unqueue);
}

/// `None` waits for every file.
pub fn wait_for_writes(path: Option<&Path>) {
    let mut queued = lock(&QUEUED);
    while queued.iter().any(|q| path.is_none_or(|p| p == q)) {
        queued = WRITTEN.wait(queued).unwrap_or_else(PoisonError::into_inner);
    }
}

pub fn write_queued(path: &Path) -> bool {
    lock(&QUEUED).iter().any(|q| q == path)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn backup(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let Some(base) = support_dir() else { return Ok(()) };
    backup_in(&base, path, bytes)
}

fn backup_in(base: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = base.join("Backups").join(path_key(path));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("source.txt"), path.to_string_lossy().as_bytes())?;
    fs::write(dir.join(format!("{}.md", now_stamp())), bytes)?;
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    entries.sort();
    while entries.len() > BACKUPS_KEPT {
        let _ = fs::remove_file(entries.remove(0));
    }
    Ok(())
}

#[cfg(test)]
pub fn backup_count(path: &Path) -> usize {
    let Some(base) = support_dir() else { return 0 };
    let Ok(rd) = fs::read_dir(base.join("Backups").join(path_key(path))) else { return 0 };
    rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "md")).count()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    pub journal: PathBuf,
    pub original: Option<PathBuf>,
    pub contents: String,
}

fn recovery_dir() -> Option<PathBuf> {
    support_dir().map(|d| d.join("Recovery"))
}

pub fn journal_path(window_id: u64) -> Option<PathBuf> {
    recovery_dir().map(|d| d.join(format!("{}-{window_id}.md", std::process::id())))
}

/// First line: the original path (or empty); the rest: the contents.
pub fn write_journal(journal: &Path, original: Option<&Path>, contents: &str) -> io::Result<()> {
    if let Some(d) = journal.parent() {
        fs::create_dir_all(d)?;
    }
    let header = original.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    write_atomic(journal, format!("{header}\n{contents}").as_bytes())
}

pub fn clear_journal(journal: &Path) {
    let _ = fs::remove_file(journal);
}

pub fn pending_recoveries() -> Vec<Recovery> {
    let Some(dir) = recovery_dir() else { return Vec::new() };
    let mine = format!("{}-", std::process::id());
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&mine)))
        .filter_map(|p| {
            let s = fs::read_to_string(&p).ok()?;
            let (head, body) = s.split_once('\n')?;
            Some(Recovery {
                journal: p,
                original: (!head.is_empty()).then(|| PathBuf::from(head)),
                contents: body.to_string(),
            })
        })
        .collect()
}

/// Unfiltered: may include documents that are gone or out of reach.
fn recent_stored() -> Vec<PathBuf> {
    support_dir()
        .and_then(|d| fs::read_to_string(d.join("recent.json")).ok())
        .and_then(|s| serde_json::from_str::<Vec<PathBuf>>(&s).ok())
        .unwrap_or_default()
}

pub fn recent() -> Vec<PathBuf> {
    recent_stored().into_iter().filter(|p| p.exists()).collect()
}

/// The last document or project and crash journals' files; bookmarks are resolved first.
pub fn launch_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = recent_stored().into_iter().take(1).collect();
    paths.extend(pending_recoveries().into_iter().filter_map(|r| r.original));
    paths
}

/// Also keeps a bookmark, so the sandboxed build can reopen it after a relaunch.
pub fn add_recent(path: &Path) -> Vec<PathBuf> {
    crate::bookmarks::remember(path);
    let mut list = recent();
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(RECENT_KEPT);
    save_recent(&list);
    list
}

fn save_recent(list: &[PathBuf]) {
    if let Some(d) = support_dir() {
        let _ = fs::create_dir_all(&d);
        if let Ok(s) = serde_json::to_string_pretty(list) {
            let _ = fs::write(d.join("recent.json"), s);
        }
    }
}

/// Moves its backups, recent-list place and bookmark along.
pub fn file_moved(from: &Path, to: &Path) {
    recent_moved(from, to);
    crate::bookmarks::moved(from, to);
}

/// Without the bookmark: the bookmark store calls this for a file moved behind the app's back.
pub fn recent_moved(from: &Path, to: &Path) {
    let Some(base) = support_dir() else { return };
    let backups = base.join("Backups");
    let (old, new) = (backups.join(path_key(from)), backups.join(path_key(to)));
    if old.exists() && !new.exists() && fs::rename(&old, &new).is_ok() {
        let _ = fs::write(new.join("source.txt"), to.to_string_lossy().as_bytes());
    }
    let mut list = recent();
    if let Some(slot) = list.iter_mut().find(|p| p.as_path() == from) {
        *slot = to.to_path_buf();
        save_recent(&list);
    }
}

/// Kept in Application Support, not the folder; a book's or journal's type is in `.dostoevsky`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ProjectMemory {
    pub last: Option<PathBuf>,
    pub collapsed: Vec<PathBuf>,
    pub panel_closed: bool,
    /// Only for a plain folder, once the user chose a type.
    pub kind: Option<String>,
    pub notes: Option<String>,
}

fn project_memory_path(root: &Path) -> Option<PathBuf> {
    support_dir().map(|d| d.join("Projects").join(format!("{}.json", path_key(root))))
}

pub fn project_memory(root: &Path) -> ProjectMemory {
    project_memory_path(root)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_project_memory(root: &Path, memory: &ProjectMemory) {
    let Some(p) = project_memory_path(root) else { return };
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(memory) {
        let _ = fs::write(p, s);
    }
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt" | "mdown"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FILE-3
    #[test]
    fn atomic_write_replaces_and_keeps_permissions() {
        let dir = std::env::temp_dir().join(format!("dsky-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.md");
        fs::write(&p, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        }
        write_atomic(&p, b"new").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(leftovers.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_creates_new_files_and_keeps_xattrs() {
        let dir = std::env::temp_dir().join(format!("dsky-files-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("Новый файл.md");
        write_atomic(&p, "first".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "first");
        #[cfg(target_os = "macos")]
        {
            let tag = std::process::Command::new("xattr")
                .args(["-w", "app.dostoevsky.test", "kept"])
                .arg(&p)
                .status()
                .unwrap();
            assert!(tag.success());
            write_atomic(&p, "second".as_bytes()).unwrap();
            assert_eq!(fs::read_to_string(&p).unwrap(), "second");
            let out = std::process::Command::new("xattr").args(["-p", "app.dostoevsky.test"]).arg(&p).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "kept");
        }
        let names: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("Новый файл.md")]);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// NFR-1
    #[test]
    fn writes_later_land_in_order() {
        let dir = std::env::temp_dir().join(format!("dsky-files-later-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.md");
        let journal = dir.join("journal.md");
        fs::write(&journal, "\nwords").unwrap();
        let first = write_later(&p, b"first".to_vec(), false, Some(journal.clone()));
        let second = write_later(&p, b"second".to_vec(), false, None);
        assert!(smol::block_on(first.recv()).unwrap().is_ok());
        assert!(smol::block_on(second.recv()).unwrap().is_ok());
        assert_eq!(fs::read_to_string(&p).unwrap(), "second");
        assert!(!journal.exists(), "the words are on disk, so their journal goes");
        assert!(!write_queued(&p));

        let _later = write_later(&p, b"later".to_vec(), false, None);
        write_atomic(&p, b"now").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "now", "the queued write went first");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_write_later_keeps_the_journal() {
        let dir = std::env::temp_dir().join(format!("dsky-files-fail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("journal.md");
        fs::write(&journal, "\nwords").unwrap();
        let p = dir.join("no such folder/a.md");
        let done = write_later(&p, b"words".to_vec(), false, Some(journal.clone()));
        assert!(smol::block_on(done.recv()).unwrap().is_err());
        wait_for_writes(Some(&p));
        assert!(journal.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
