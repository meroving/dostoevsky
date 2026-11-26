//! The freedesktop.org Trash, which Linux's file managers share (dsky-fbso.7): a file goes to
//! `files/` with a `.trashinfo` beside it in `info/` saying where it came from and when, so the
//! file manager can show it and put it back. The home trash first (`$XDG_DATA_HOME/Trash`); a
//! file on another disk goes to that disk's `.Trash-<uid>`, as the spec has it, since a move
//! between disks would be a copy. Undo moves it back and drops its `.trashinfo`.

// Built on the Mac for its tests only (the Mac's Trash is NSFileManager's); the UI tests trash
// into a folder of their own, not the writer's.
#![cfg_attr(any(target_os = "macos", test), allow(dead_code))]

use std::io;
use std::path::{Path, PathBuf};

/// Moves `path` to the trash; returns where it went.
#[cfg(unix)]
pub fn trash(path: &Path) -> io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    let home = home_trash().ok_or_else(|| io::Error::other("no home folder"))?;
    match trash_into(&path, &home) {
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            let top = mount_point(&path)?;
            // SAFETY: getuid has no preconditions and can't fail.
            let uid = unsafe { libc::getuid() };
            let dir = top.join(format!(".Trash-{uid}"));
            create_private(&dir)?;
            trash_into_relative(&path, &dir, &top)
        }
        other => other,
    }
}

/// Moves a trashed file back to `from`, and forgets it in the trash's `info/`.
pub fn restore(to: &Path, from: &Path) -> io::Result<()> {
    std::fs::rename(to, from)?;
    if let Some(info) = info_of(to) {
        let _ = std::fs::remove_file(info);
    }
    Ok(())
}

/// `$XDG_DATA_HOME/Trash`, by default `~/.local/share/Trash`.
fn home_trash() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    let data = data.or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("Trash"))
}

/// Into the home trash: `Path=` holds the absolute path.
fn trash_into(path: &Path, trash: &Path) -> io::Result<PathBuf> {
    trash_into_relative(path, trash, Path::new(""))
}

/// `Path=` is relative to `top` when that is set (a disk's own trash), else absolute.
fn trash_into_relative(path: &Path, trash: &Path, top: &Path) -> io::Result<PathBuf> {
    let (files, info) = (trash.join("files"), trash.join("info"));
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path.file_name().ok_or_else(|| io::Error::other("no file name"))?.to_string_lossy().into_owned();
    let shown = if top.as_os_str().is_empty() { path } else { path.strip_prefix(top).unwrap_or(path) };
    let record = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", encode(shown), chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"));
    for n in 1.. {
        let candidate = if n == 1 { name.clone() } else { numbered(&name, n) };
        let info_file = info.join(format!("{candidate}.trashinfo"));
        // The info file is made first and only if new: it claims the name, as the spec asks.
        let made = std::fs::OpenOptions::new().write(true).create_new(true).open(&info_file);
        let mut file = match made {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let to = files.join(&candidate);
        if to.symlink_metadata().is_ok() {
            let _ = std::fs::remove_file(&info_file);
            continue;
        }
        use std::io::Write;
        let moved = file.write_all(record.as_bytes()).and_then(|()| std::fs::rename(path, &to));
        return match moved {
            Ok(()) => Ok(to),
            Err(e) => {
                let _ = std::fs::remove_file(&info_file);
                Err(e)
            }
        };
    }
    unreachable!("a free name is always found")
}

/// "Chapter 2.md" for the second "Chapter.md", as file managers name them.
fn numbered(name: &str, n: usize) -> String {
    match name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) {
        Some((stem, ext)) => format!("{stem} {n}.{ext}"),
        None => format!("{name} {n}"),
    }
}

/// The `.trashinfo` of a file in a trash's `files/`.
fn info_of(trashed: &Path) -> Option<PathBuf> {
    let files = trashed.parent()?;
    if files.file_name()? != "files" {
        return None;
    }
    let info = files.parent()?.join("info").join(format!("{}.trashinfo", trashed.file_name()?.to_string_lossy()));
    info.is_file().then_some(info)
}

/// A path as a URL's path writes it: bytes other than letters, digits, `/` and `-_.~` escaped.
fn encode(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The top of the file system `path` is on: the last folder up with the same device.
#[cfg(unix)]
fn mount_point(path: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let device = std::fs::symlink_metadata(path)?.dev();
    let mut top = path.parent().unwrap_or(path).to_path_buf();
    while let Some(up) = top.parent() {
        if std::fs::metadata(up).map(|m| m.dev()).ok() != Some(device) {
            break;
        }
        top = up.to_path_buf();
    }
    Ok(top)
}

#[cfg(unix)]
fn create_private(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{encode, numbered, restore, trash_into, trash_into_relative};

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("dostoevsky-trash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    #[test]
    fn a_trashed_file_says_where_it_came_from_and_comes_back() {
        let d = dir("one");
        let (bin, docs) = (d.0.join("Trash"), d.0.join("Docs and notes"));
        std::fs::create_dir_all(&docs).unwrap();
        let file = docs.join("Глава 1.md");
        std::fs::write(&file, "words").unwrap();
        let to = trash_into(&file, &bin).unwrap();
        assert_eq!(to, bin.join("files/Глава 1.md"));
        assert!(!file.exists() && std::fs::read_to_string(&to).unwrap() == "words");
        let info = std::fs::read_to_string(bin.join("info/Глава 1.md.trashinfo")).unwrap();
        let mut lines = info.lines();
        assert_eq!(lines.next(), Some("[Trash Info]"));
        assert_eq!(lines.next().unwrap(), format!("Path={}/Docs%20and%20notes/%D0%93%D0%BB%D0%B0%D0%B2%D0%B0%201.md", encode(&d.0)));
        let date = lines.next().unwrap().strip_prefix("DeletionDate=").unwrap();
        assert!(chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S").is_ok(), "{date}");

        restore(&to, &file).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "words");
        assert!(!bin.join("info/Глава 1.md.trashinfo").exists(), "the trash forgets it");
    }

    #[test]
    fn a_second_file_of_the_same_name_gets_a_number() {
        let d = dir("two");
        let bin = d.0.join("Trash");
        for n in 0..3 {
            let file = d.0.join("Chapter.md");
            std::fs::write(&file, n.to_string()).unwrap();
            trash_into(&file, &bin).unwrap();
        }
        for (name, text) in [("Chapter.md", "0"), ("Chapter 2.md", "1"), ("Chapter 3.md", "2")] {
            assert_eq!(std::fs::read_to_string(bin.join("files").join(name)).unwrap(), text);
            assert!(bin.join("info").join(format!("{name}.trashinfo")).is_file(), "{name}");
        }
        assert_eq!(numbered(".hidden", 2), ".hidden 2");
        assert_eq!(numbered("README", 2), "README 2");
    }

    #[test]
    fn a_disks_own_trash_records_the_path_from_its_top() {
        let d = dir("top");
        let top = d.0.join("disk");
        std::fs::create_dir_all(top.join("books")).unwrap();
        let file = top.join("books/a.md");
        std::fs::write(&file, "x").unwrap();
        let bin = top.join(".Trash-1000");
        trash_into_relative(&file, &bin, &top).unwrap();
        assert!(std::fs::read_to_string(bin.join("info/a.md.trashinfo")).unwrap().contains("\nPath=books/a.md\n"));
    }

    #[test]
    fn a_file_the_trash_cant_take_stays_and_leaves_no_record() {
        let d = dir("missing");
        let bin = d.0.join("Trash");
        assert!(trash_into(&d.0.join("not there.md"), &bin).is_err());
        assert_eq!(std::fs::read_dir(bin.join("info")).unwrap().count(), 0);
        assert!(super::info_of(Path::new("/tmp/elsewhere/x.md")).is_none());
    }
}
