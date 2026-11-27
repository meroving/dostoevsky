//! Windows' Recycle Bin (dsky-fbso.7): a file goes where Explorer's Delete sends it, through the
//! shell's own file operation (`IFileOperation`, recycle on delete, no dialogs), which says where
//! the file landed: `C:\$Recycle.Bin\<user>\$R….md`, beside the `$I….md` that records where it
//! came from. Undo moves the `$R` file back and drops its `$I` record, so the Recycle Bin forgets
//! it, as the freedesktop.org Trash's undo drops its `.trashinfo`. A file the Recycle Bin can't
//! take (a drive without one, a file too big for it) would be deleted for good; that one is
//! refused instead, and the caller keeps it in a folder of its own.

use std::cell::{Cell, RefCell};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
    IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH, TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::{HRESULT, HSTRING, PCWSTR, Ref, implement};
use windows::Win32::Foundation::E_ABORT;

#[derive(Default)]
struct Outcome {
    /// The recycled file, once the shell says where it went.
    landed: RefCell<Option<PathBuf>>,
    /// The shell would have deleted it for good: the operation was stopped.
    not_recyclable: Cell<bool>,
}

#[implement(IFileOperationProgressSink)]
struct Sink(Rc<Outcome>);

/// Moves `path` to the Recycle Bin and returns where it went. An `Unsupported` error when the
/// Recycle Bin can't take it: the file is untouched then.
pub fn recycle(path: &Path) -> io::Result<PathBuf> {
    let path = shell_path(&std::path::absolute(path)?);
    let outcome = Rc::new(Outcome::default());
    // SAFETY: COM on this thread (the main thread, already initialised by GPUI; a second
    // initialisation only counts up), objects used only here.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(other)?;
        op.SetOperationFlags(FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOFX_EARLYFAILURE | FOFX_RECYCLEONDELETE).map_err(other)?;
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).map_err(other)?;
        let sink: IFileOperationProgressSink = Sink(outcome.clone()).into();
        op.DeleteItem(&item, &sink).map_err(other)?;
        let done = op.PerformOperations();
        if outcome.not_recyclable.get() {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "the Recycle Bin can't take this file"));
        }
        done.map_err(other)?;
        if op.GetAnyOperationsAborted().is_ok_and(|a| a.as_bool()) {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
    }
    let landed = outcome.landed.borrow_mut().take();
    landed.ok_or_else(|| io::Error::other("the Recycle Bin didn't say where the file went"))
}

/// Moves a recycled file back to `from` and drops its `$I` record. Refuses to replace a file
/// that has since taken its place.
pub fn restore(to: &Path, from: &Path) -> io::Result<()> {
    if from.symlink_metadata().is_ok() {
        // ERROR_FILE_EXISTS: Windows says it in the writer's language.
        return Err(io::Error::from_raw_os_error(80));
    }
    std::fs::rename(to, from)?;
    if let Some(record) = record_of(to) {
        let _ = std::fs::remove_file(record);
    }
    Ok(())
}

/// The shell parses ordinary paths only: `\\?\C:\…` (what `canonicalize` gives) as `C:\…`, and
/// `\\?\UNC\server\…` as `\\server\…`.
fn shell_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match (s.strip_prefix(r"\\?\UNC\"), s.strip_prefix(r"\\?\")) {
        (Some(unc), _) => PathBuf::from(format!(r"\\{unc}")),
        (None, Some(local)) => PathBuf::from(local),
        _ => path.to_path_buf(),
    }
}

/// `$I…` beside a recycled `$R…`.
fn record_of(recycled: &Path) -> Option<PathBuf> {
    let name = recycled.file_name()?.to_str()?;
    let rest = name.strip_prefix("$R")?;
    Some(recycled.with_file_name(format!("$I{rest}")))
}

fn other(e: windows::core::Error) -> io::Error {
    io::Error::other(e.message())
}

impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn PreDeleteItem(&self, flags: u32, _: Ref<IShellItem>) -> windows::core::Result<()> {
        // Without this flag the shell deletes the file for good.
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            self.0.not_recyclable.set(true);
            return Err(E_ABORT.into());
        }
        Ok(())
    }

    fn PostDeleteItem(&self, _: u32, _: Ref<IShellItem>, result: HRESULT, created: Ref<IShellItem>) -> windows::core::Result<()> {
        if result.is_err() {
            return Ok(());
        }
        let Some(created) = created.as_ref() else {
            // Deleted, not recycled.
            self.0.not_recyclable.set(true);
            return Ok(());
        };
        // SAFETY: the shell's item; the name is COM-allocated and freed here.
        unsafe {
            if let Ok(name) = created.GetDisplayName(SIGDN_FILESYSPATH) {
                *self.0.landed.borrow_mut() = name.to_string().ok().map(PathBuf::from);
                CoTaskMemFree(Some(name.0 as *const _));
            }
        }
        Ok(())
    }

    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR, _: HRESULT, _: Ref<IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(&self, _: u32, _: Ref<IShellItem>, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(&self, _: u32, _: Ref<IShellItem>, _: Ref<IShellItem>, _: &PCWSTR, _: HRESULT, _: Ref<IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(&self, _: u32, _: Ref<IShellItem>, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(&self, _: u32, _: Ref<IShellItem>, _: Ref<IShellItem>, _: &PCWSTR, _: HRESULT, _: Ref<IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR, _: &PCWSTR, _: u32, _: HRESULT, _: Ref<IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{record_of, recycle, restore, shell_path};

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_shell_gets_an_ordinary_path() {
        assert_eq!(shell_path(&PathBuf::from(r"\\?\C:\Book\02 Two.md")), PathBuf::from(r"C:\Book\02 Two.md"));
        assert_eq!(shell_path(&PathBuf::from(r"\\?\UNC\nas\share\a.md")), PathBuf::from(r"\\nas\share\a.md"));
        assert_eq!(shell_path(&PathBuf::from(r"C:\a.md")), PathBuf::from(r"C:\a.md"));
    }

    #[test]
    fn a_record_sits_beside_its_file() {
        let r = PathBuf::from(r"C:\$Recycle.Bin\S-1-5-21\$RAB12CD.md");
        assert_eq!(record_of(&r), Some(PathBuf::from(r"C:\$Recycle.Bin\S-1-5-21\$IAB12CD.md")));
        assert_eq!(record_of(&PathBuf::from(r"C:\Trash\a.md")), None);
    }

    /// The real Recycle Bin: the file goes there with its record, and comes back.
    #[test]
    fn a_recycled_file_comes_back_and_the_bin_forgets_it() {
        let d = Dir(std::env::temp_dir().join(format!("dostoevsky-recycle-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&d.0);
        std::fs::create_dir_all(&d.0).unwrap();
        // As the app opens a project: canonicalised, `\\?\C:\…`.
        let file = d.0.canonicalize().unwrap().join("Глава 1.md");
        std::fs::write(&file, "words").unwrap();
        let to = recycle(&file).unwrap();
        assert!(!file.exists());
        assert!(to.to_string_lossy().to_lowercase().contains(r"\$recycle.bin\"), "{to:?}");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "words");
        let record = record_of(&to).expect("a $R name");
        assert!(record.is_file(), "the bin records where it came from");

        std::fs::write(&file, "new").unwrap();
        assert!(restore(&to, &file).is_err(), "a file that took its place stays");
        std::fs::remove_file(&file).unwrap();

        restore(&to, &file).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "words");
        assert!(!to.exists() && !record.exists(), "the bin forgets it");
    }
}
