//! One app process off the Mac (dsky-fbso.20). On the Mac LaunchServices hands a second file to
//! the running app; on Windows and Linux every launch (a double-click, Open with, the .desktop
//! entry's %F, the .msi's open verb) is a new process. The first one listens (a per-user named
//! pipe on Windows, a Unix socket in XDG_RUNTIME_DIR on Linux) and a later launch passes it its
//! paths, made whole against its own working folder, then exits; with no paths it asks the app
//! to come forward. The name is keyed to the support folder, so a run with its own
//! DOSTOEVSKY_SUPPORT_DIR is its own app. A dead owner leaves nothing behind (Windows) or a
//! socket nobody answers on (Linux, replaced); one that doesn't take the paths in time is left
//! alone and the launch runs by itself.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

/// A message: the magic and version, when the launch stops waiting (ms since 1970, u64 LE), a
/// count, then each path's length and bytes (u32 LE).
const MAGIC: &[u8; 5] = b"DSKY\x01";
const MAX_PATHS: u32 = 4096;
const MAX_PATH_BYTES: u32 = 1 << 16;
/// The running app's reply once it has taken the paths.
const DONE: u8 = 1;
/// How long the running app has to take the paths before the launch opens them itself.
const WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq)]
pub struct Message {
    /// After this the launch opens the paths itself (a suspended app that wakes up late
    /// mustn't open them too).
    pub deadline_ms: u64,
    pub paths: Vec<PathBuf>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Message {
    fn left(&self) -> Duration {
        Duration::from_millis(self.deadline_ms.saturating_sub(now_ms()))
    }
}

pub fn encode(m: &Message) -> Vec<u8> {
    let paths = &m.paths;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&m.deadline_ms.to_le_bytes());
    out.extend_from_slice(&(paths.len() as u32).to_le_bytes());
    for p in paths {
        let b = path_bytes(p);
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&b);
    }
    out
}

pub fn decode(r: &mut impl Read) -> io::Result<Message> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
    let mut magic = [0; 5];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(bad("not a Dostoevsky message"));
    }
    let mut deadline = [0; 8];
    r.read_exact(&mut deadline)?;
    let n = read_u32(r)?;
    if n > MAX_PATHS {
        return Err(bad("too many paths"));
    }
    let mut paths = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let len = read_u32(r)?;
        if len > MAX_PATH_BYTES {
            return Err(bad("path too long"));
        }
        let mut b = vec![0; len as usize];
        r.read_exact(&mut b)?;
        paths.push(path_from_bytes(b).ok_or_else(|| bad("malformed path"))?);
    }
    Ok(Message { deadline_ms: u64::from_le_bytes(deadline), paths })
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

#[cfg(unix)]
fn path_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn path_from_bytes(b: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(OsString::from_vec(b)))
}

/// UTF-16 units as Windows keeps them, unpaired surrogates and all.
#[cfg(windows)]
fn path_bytes(p: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(windows)]
fn path_from_bytes(b: Vec<u8>) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let (pairs, rest) = b.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let wide: Vec<u16> = pairs.iter().map(|&c| u16::from_le_bytes(c)).collect();
    Some(PathBuf::from(OsString::from_wide(&wide)))
}

/// The launch's paths (as `main` reads them: options left out), whole against this process's
/// working folder, which the running app doesn't share.
pub fn launch_paths(args: impl IntoIterator<Item = OsString>) -> Vec<PathBuf> {
    args.into_iter()
        .filter(|a| !a.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect()
}

/// Names the app by its support folder: FNV-1a, the same in every build.
fn key(support: &Path) -> String {
    let whole = std::path::absolute(support).unwrap_or_else(|_| support.to_path_buf());
    let mut s = whole.to_string_lossy().into_owned();
    if cfg!(windows) {
        s = s.to_lowercase();
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub enum Claim {
    /// The running app took the paths: this process is done.
    Handed,
    /// This is the app; later launches come here.
    Owner(sys::Listener),
    /// No way to reach or become the app: run by itself.
    Alone,
}

pub fn claim(paths: &[PathBuf]) -> Claim {
    let Some(support) = crate::files::support_dir() else { return Claim::Alone };
    claim_at(&key(&support), paths)
}

fn claim_at(key: &str, paths: &[PathBuf]) -> Claim {
    let message = encode(&Message { deadline_ms: now_ms() + WAIT.as_millis() as u64, paths: paths.to_vec() });
    // Two launches at once: the one that loses the race to listen talks to the other.
    for _ in 0..20 {
        match sys::connect_or_listen(key) {
            Ok(sys::Reached::Running(stream)) => {
                return match send(stream, message) {
                    Ok(()) => Claim::Handed,
                    Err(e) => {
                        eprintln!("dostoevsky: the running app didn't answer ({e}); opening here");
                        Claim::Alone
                    }
                };
            }
            Ok(sys::Reached::Listening(l)) => return Claim::Owner(l),
            Ok(sys::Reached::Retry) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                eprintln!("dostoevsky: can't listen for other launches: {e}");
                return Claim::Alone;
            }
        }
    }
    Claim::Alone
}

/// Writes the message and waits for the app's reply on a thread of its own, so a hung app can't
/// keep this launch waiting.
fn send(mut stream: sys::Stream, message: Vec<u8>) -> io::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = (|| {
            stream.write_all(&message)?;
            stream.flush()?;
            let mut reply = [0];
            stream.read_exact(&mut reply)?;
            if reply[0] == DONE { Ok(()) } else { Err(io::Error::other("unexpected reply")) }
        })();
        let _ = tx.send(r);
    });
    rx.recv_timeout(WAIT + Duration::from_secs(3)).unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))
}

/// Paths from another launch, for the app to open once and say so.
pub struct Handed {
    pub paths: Vec<PathBuf>,
    deadline_ms: u64,
    state: Arc<AtomicU8>,
    done: std::sync::mpsc::Sender<()>,
}

const PENDING: u8 = 0;
const TAKEN: u8 = 1;
const GIVEN_UP: u8 = 2;

impl Handed {
    /// False when the launch has stopped waiting and opens them itself.
    pub fn take(&self) -> bool {
        now_ms() <= self.deadline_ms && self.state.compare_exchange(PENDING, TAKEN, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    pub fn done(self) {
        let _ = self.done.send(());
    }
}

/// Listens on a thread of its own; each launch's paths arrive on the channel.
pub fn serve(mut listener: sys::Listener) -> smol::channel::Receiver<Handed> {
    let (tx, rx) = smol::channel::unbounded();
    std::thread::Builder::new()
        .name("other launches".into())
        .spawn(move || loop {
            match listener.accept() {
                Ok(stream) => {
                    let tx = tx.clone();
                    std::thread::spawn(move || answer(stream, tx));
                }
                Err(e) => {
                    eprintln!("dostoevsky: stopped listening for other launches: {e}");
                    return;
                }
            }
        })
        .ok();
    rx
}

fn answer(mut stream: sys::Stream, tx: smol::channel::Sender<Handed>) {
    let Ok(m) = decode(&mut stream) else { return };
    let left = m.left();
    if left.is_zero() {
        return;
    }
    let state = Arc::new(AtomicU8::new(PENDING));
    let (done, finished) = std::sync::mpsc::channel();
    if tx.try_send(Handed { paths: m.paths, deadline_ms: m.deadline_ms, state: state.clone(), done }).is_err() {
        return;
    }
    if finished.recv_timeout(left).is_err() {
        // Too late to say no once the app has started opening them.
        if state.compare_exchange(PENDING, GIVEN_UP, Ordering::AcqRel, Ordering::Acquire).is_ok() || finished.recv().is_err() {
            return;
        }
    }
    let _ = stream.write_all(&[DONE]);
    let _ = stream.flush();
}

#[cfg(windows)]
pub mod sys {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::FromRawHandle;
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE};
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW};
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    use windows::core::HSTRING;

    pub type Stream = File;

    pub enum Reached {
        Running(Stream),
        Listening(Listener),
        Retry,
    }

    fn name(key: &str) -> String {
        format!(r"\\.\pipe\{}-{key}", crate::APP_ID)
    }

    /// The pipe goes when its last handle does, so a killed app leaves nothing to clean up.
    pub fn connect_or_listen(key: &str) -> io::Result<Reached> {
        let name = name(key);
        match std::fs::OpenOptions::new().read(true).write(true).open(&name) {
            Ok(f) => {
                // The app may take the foreground: this launch has the right to give it.
                let mut pid = 0;
                // SAFETY: a pipe handle this process owns; `pid` outlives the call.
                if unsafe { GetNamedPipeServerProcessId(HANDLE(f.as_raw_handle()), &mut pid) }.is_ok() {
                    // SAFETY: no pointers.
                    let _ = unsafe { AllowSetForegroundWindow(pid) };
                }
                Ok(Reached::Running(f))
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                // Every instance is talking to another launch: wait for the next one.
                // SAFETY: no pointers beyond the name, alive for the call.
                let _ = unsafe { WaitNamedPipeW(&HSTRING::from(name.as_str()), 1000) };
                Ok(Reached::Retry)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => match create(&name, true) {
                Ok(h) => Ok(Reached::Listening(Listener { name, next: Some(h) })),
                // Another launch made it first.
                Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED.0 as i32) => Ok(Reached::Retry),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    fn create(name: &str, first: bool) -> io::Result<HANDLE> {
        let mut mode = PIPE_ACCESS_DUPLEX;
        if first {
            mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        // SAFETY: the name lives for the call; the default security lets only this user (and
        // the system and administrators) write to it.
        let h = unsafe { CreateNamedPipeW(&HSTRING::from(name), mode, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, PIPE_UNLIMITED_INSTANCES, 4096, 4096, 0, None) };
        if h.is_invalid() { Err(io::Error::last_os_error()) } else { Ok(h) }
    }

    pub struct Listener {
        name: String,
        next: Option<HANDLE>,
    }

    // SAFETY: a pipe handle can be used from any thread.
    unsafe impl Send for Listener {}

    impl Listener {
        /// The next instance is made before this one is handed out, so the name never lapses
        /// and a launch never takes it for a dead app.
        pub fn accept(&mut self) -> io::Result<Stream> {
            let h = match self.next.take() {
                Some(h) => h,
                None => create(&self.name, false)?,
            };
            // SAFETY: a pipe instance this process made; no overlapped I/O.
            match unsafe { ConnectNamedPipe(h, None) } {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
                Err(e) => {
                    // SAFETY: ours, not used again.
                    let _ = unsafe { CloseHandle(h) };
                    return Err(e.into());
                }
            }
            self.next = Some(create(&self.name, false)?);
            // SAFETY: the handle is ours and the File now owns it.
            Ok(unsafe { File::from_raw_handle(h.0) })
        }
    }
}

#[cfg(unix)]
pub mod sys {
    use std::fs::{self, File};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::Duration;

    pub type Stream = UnixStream;

    pub enum Reached {
        Running(Stream),
        Listening(Listener),
        #[allow(dead_code)]
        Retry,
    }

    /// XDG_RUNTIME_DIR is the user's own; without it a folder of the app's in /tmp, made only
    /// the user's.
    fn folder() -> io::Result<PathBuf> {
        if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute() && d.is_dir()) {
            return Ok(d);
        }
        // SAFETY: no arguments, can't fail.
        let uid = unsafe { libc::getuid() };
        let d = std::env::temp_dir().join(format!("dostoevsky-{uid}"));
        match fs::DirBuilder::new().mode(0o700).create(&d) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let m = fs::symlink_metadata(&d)?;
        if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
            return Err(io::Error::other(format!("{} isn't this user's own folder", d.display())));
        }
        Ok(d)
    }

    /// The socket and its lock file.
    pub fn files(key: &str) -> io::Result<(PathBuf, PathBuf)> {
        let dir = folder()?;
        let base = format!("{}-{key}", crate::APP_ID);
        Ok((dir.join(format!("{base}.sock")), dir.join(format!("{base}.lock"))))
    }

    /// Under a lock, so of two launches at once one listens and the other reaches it. A socket
    /// nobody answers on is a dead app's, and goes.
    pub fn connect_or_listen(key: &str) -> io::Result<Reached> {
        let (socket, lock) = files(key)?;
        let lock = File::options().create(true).truncate(false).write(true).open(lock)?;
        lock.lock()?;
        match UnixStream::connect(&socket) {
            Ok(s) => return Ok(Reached::Running(s)),
            Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {}
            Err(e) => return Err(e),
        }
        let _ = fs::remove_file(&socket);
        let l = UnixListener::bind(&socket)?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
        Ok(Reached::Listening(Listener(l)))
    }

    pub struct Listener(UnixListener);

    impl Listener {
        pub fn accept(&mut self) -> io::Result<Stream> {
            let (s, _) = self.0.accept()?;
            // A launch that connects and says nothing doesn't hold a thread for good.
            s.set_read_timeout(Some(Duration::from_secs(5)))?;
            Ok(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::Cursor;
    use std::path::PathBuf;

    use super::{Message, decode, encode, key, launch_paths};

    /// Through the wire and back: the paths.
    fn round_trip(paths: &[PathBuf]) -> Vec<PathBuf> {
        let m = Message { deadline_ms: 1_764_000_000_123, paths: paths.to_vec() };
        let back = decode(&mut Cursor::new(encode(&m))).unwrap();
        assert_eq!(back.deadline_ms, m.deadline_ms);
        back.paths
    }

    /// The start of a message with no paths yet: magic, deadline.
    fn head() -> Vec<u8> {
        let mut m = super::MAGIC.to_vec();
        m.extend_from_slice(&0u64.to_le_bytes());
        m
    }

    #[test]
    fn paths_come_through_as_they_went() {
        let paths = vec![
            PathBuf::from("/home/me/Черновик.md"),
            PathBuf::from(r"C:\Users\me\My Essay.md"),
            PathBuf::from("/tmp/a\nb.md"),
            PathBuf::from(""),
        ];
        assert_eq!(round_trip(&paths), paths);
        assert_eq!(round_trip(&[]), Vec::<PathBuf>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_isnt_utf8_comes_through() {
        use std::os::unix::ffi::OsStringExt;
        let p = PathBuf::from(OsString::from_vec(b"/tmp/\xff\xfe.md".to_vec()));
        assert_eq!(round_trip(std::slice::from_ref(&p)), [p]);
    }

    #[cfg(windows)]
    #[test]
    fn a_path_that_isnt_utf16_comes_through() {
        use std::os::windows::ffi::OsStringExt;
        // An unpaired surrogate, which Windows allows in a file name.
        let p = PathBuf::from(OsString::from_wide(&[b'C' as u16, b':' as u16, b'\\' as u16, 0xD800, b'x' as u16]));
        assert_eq!(round_trip(std::slice::from_ref(&p)), [p]);
    }

    #[test]
    fn a_broken_message_is_refused() {
        let good = encode(&Message { deadline_ms: 0, paths: vec![PathBuf::from("/a/b.md")] });
        for cut in 0..good.len() {
            assert!(decode(&mut Cursor::new(&good[..cut])).is_err(), "cut at {cut}");
        }
        let mut other = good.clone();
        other[0] = b'X';
        assert!(decode(&mut Cursor::new(other)).is_err());
        let mut newer = good.clone();
        newer[4] = 2;
        assert!(decode(&mut Cursor::new(newer)).is_err());
        // A count or a length too big to be real isn't allocated for.
        let mut many = head();
        many.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&mut Cursor::new(many)).is_err());
        let mut long = head();
        long.extend_from_slice(&1u32.to_le_bytes());
        long.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&mut Cursor::new(long)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn an_odd_number_of_bytes_isnt_a_windows_path() {
        let mut m = head();
        m.extend_from_slice(&1u32.to_le_bytes());
        m.extend_from_slice(&3u32.to_le_bytes());
        m.extend_from_slice(b"abc");
        assert!(decode(&mut Cursor::new(m)).is_err());
    }

    #[test]
    fn launch_paths_are_whole_against_the_working_folder() {
        let cwd = std::env::current_dir().unwrap();
        let elsewhere = if cfg!(windows) { PathBuf::from(r"D:\Notes\b.md") } else { PathBuf::from("/notes/b.md") };
        let args: Vec<OsString> = vec!["a.md".into(), "--flag".into(), "-psn_0_1".into(), elsewhere.clone().into(), "sub/c.md".into()];
        let got = launch_paths(args);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], cwd.join("a.md"));
        assert_eq!(got[1], elsewhere);
        assert!(got[2].is_absolute() && got[2].starts_with(&cwd) && got[2].ends_with("c.md"), "{:?}", got[2]);
        assert!(launch_paths(Vec::<OsString>::new()).is_empty());
    }

    #[test]
    fn each_support_folder_is_its_own_app() {
        let a = key(&std::env::temp_dir().join("one"));
        assert_eq!(a, key(&std::env::temp_dir().join("one")));
        assert_ne!(a, key(&std::env::temp_dir().join("two")));
        assert_eq!(a.len(), 16);
        if cfg!(windows) {
            assert_eq!(key(&PathBuf::from(r"C:\Users\Me\AppData")), key(&PathBuf::from(r"c:\users\me\appdata")));
        }
    }

    /// The socket and lock file a test made (Linux keeps them in XDG_RUNTIME_DIR).
    fn forget(_k: &str) {
        #[cfg(unix)]
        if let Ok((socket, lock)) = super::sys::files(_k) {
            let _ = std::fs::remove_file(socket);
            let _ = std::fs::remove_file(lock);
        }
    }

    /// A real pipe or socket: a second launch's paths reach the first, which says it took them;
    /// with no app there, the launch becomes it.
    #[test]
    fn a_second_launch_hands_its_paths_to_the_first() {
        use super::{Claim, claim_at, serve};
        let k = format!("test-{}-{}", std::process::id(), line!());
        let Claim::Owner(listener) = claim_at(&k, &[]) else { panic!("the first launch should listen") };
        let rx = serve(listener);
        let app = std::thread::spawn(move || {
            for _ in 0..2 {
                let h = smol::block_on(rx.recv()).unwrap();
                assert!(h.take());
                let paths = h.paths.clone();
                h.done();
                if !paths.is_empty() {
                    return paths;
                }
            }
            unreachable!()
        });
        // With no paths, the app is asked to come forward.
        assert!(matches!(claim_at(&k, &[]), Claim::Handed));
        let p = vec![std::env::temp_dir().join("handed.md")];
        assert!(matches!(claim_at(&k, &p), Claim::Handed));
        assert_eq!(app.join().unwrap(), p);
        forget(&k);
    }

    /// An app that doesn't take the paths in time: the launch runs by itself, and the app,
    /// when it gets to them, leaves them alone.
    #[test]
    fn a_hung_app_is_left_alone() {
        use super::{Claim, claim_at, serve};
        let k = format!("test-{}-{}", std::process::id(), line!());
        let Claim::Owner(listener) = claim_at(&k, &[]) else { panic!("the first launch should listen") };
        let rx = serve(listener);
        let started = std::time::Instant::now();
        assert!(matches!(claim_at(&k, &[PathBuf::from("/x.md")]), Claim::Alone));
        // The deadline travels as whole milliseconds of the wall clock, so the wait is WAIT less a
        // millisecond or so of rounding; this clock is another.
        assert!(started.elapsed() >= super::WAIT - std::time::Duration::from_millis(100));
        let h = smol::block_on(rx.recv()).unwrap();
        assert!(!h.take());
        forget(&k);
    }

    /// A suspended app that wakes after the launch stopped waiting doesn't open the paths too.
    #[test]
    fn a_message_past_its_deadline_is_left_alone() {
        use std::io::{Read, Write};

        use super::{Claim, claim_at, now_ms, serve, sys};
        let k = format!("test-{}-{}", std::process::id(), line!());
        let Claim::Owner(listener) = claim_at(&k, &[]) else { panic!("the first launch should listen") };
        let rx = serve(listener);
        let Ok(sys::Reached::Running(mut s)) = sys::connect_or_listen(&k) else { panic!("the app should be there") };
        s.write_all(&encode(&Message { deadline_ms: now_ms() - 1, paths: vec![PathBuf::from("/late.md")] })).unwrap();
        let mut reply = Vec::new();
        let _ = s.read_to_end(&mut reply);
        assert!(reply.is_empty());
        assert!(rx.try_recv().is_err());
        forget(&k);
    }

    #[cfg(unix)]
    #[test]
    fn a_dead_apps_socket_is_replaced() {
        use super::{Claim, claim_at};
        let k = format!("test-{}-{}", std::process::id(), line!());
        let (socket, lock) = super::sys::files(&k).unwrap();
        // The socket file stays when its listener goes, as after a kill -9.
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        assert!(socket.exists());
        assert!(matches!(claim_at(&k, &[]), Claim::Owner(_)));
        let _ = std::fs::remove_file(socket);
        let _ = std::fs::remove_file(lock);
    }
}
