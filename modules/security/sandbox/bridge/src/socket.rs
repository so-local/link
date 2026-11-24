//! A Unix socket only its user's (0600) from the moment it's at its path.
//!
//! A socket bound at its path is there as the system makes it (0777 less
//! the umask) until it's made private, and anyone who reaches the folder
//! can connect then, a connection that waits to be accepted after. So it
//! is bound in a folder of its own, made beside its path and only this
//! user's (0700), made private there, and moved in place. What's done to
//! it there is done through that folder and its path's folder, held open,
//! and never through a link: its path's folder may be one a sandbox
//! writes, which could swap a link in for either, to have a file of the
//! user's made anyone's, or removed.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// Whose a socket is besides this user's, and its mode: set before it's
/// at its path, so it's never there as anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Given {
    /// The user it's given to, if not this one (root, or `CAP_CHOWN`).
    pub owner: Option<u32>,
    /// The group it's given to, if not this user's.
    pub group: Option<u32>,
    pub mode: u32,
}

impl Given {
    /// Anyone's to connect to (0666): for a bridge whose every call is
    /// proved, or refused before its action is read.
    pub const OPEN: Given = Given {
        owner: None,
        group: None,
        mode: 0o666,
    };
}

impl Default for Given {
    /// Only this user's: 0600.
    fn default() -> Self {
        Self {
            owner: None,
            group: None,
            mode: 0o600,
        }
    }
}

/// Listens at `path`, the socket only this user's whenever it's there; a
/// socket left at `path` is replaced. Nothing of it is left on failure.
pub fn listen_private(path: &Path) -> anyhow::Result<UnixListener> {
    listen_given(path, Given::default())
}

/// [`listen_private`], the socket anyone's ([`Given::OPEN`]) instead.
pub fn listen_open(path: &Path) -> anyhow::Result<UnixListener> {
    listen_given(path, Given::OPEN)
}

/// [`listen_private`], the socket `given` instead whenever it's there.
pub fn listen_given(path: &Path, given: Given) -> anyhow::Result<UnixListener> {
    let at = || format!("listening at {}", path.display());
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .with_context(|| format!("{} names no socket", path.display()))?;
    // Reached at `path`; and, where it's bound at a path in its folder
    // (the folder's path, `/.XXXXXX/s`) rather than through the folder's
    // handle (Linux's `/proc/self/fd`, always short), at that too: each
    // within the limit.
    let bound = match cfg!(target_os = "linux") {
        true => 0,
        false => parent.as_os_str().len() + "/.XXXXXX/s".len(),
    };
    check_length(path, bound.max(path.as_os_str().len()))?;
    let held = Held::make(parent).with_context(at)?;
    let made = (|| -> std::io::Result<UnixListener> {
        let listener = UnixListener::bind(held.bind_path(parent))?;
        // Its mode first: given to another user, it's no longer this
        // process's to change (with `CAP_CHOWN` alone).
        held.chmod(given.mode)?;
        if given.owner.is_some() || given.group.is_some() {
            held.chown(given.owner, given.group)?;
        }
        // In one step: a socket left there is replaced, and one that
        // can't be is left as it was.
        held.rename_to(name)?;
        Ok(listener)
    })();
    held.remove();
    made.with_context(at)
}

/// [`listen_given`], as a listener of tokio's.
pub fn listen_async(path: &Path, given: Given) -> anyhow::Result<tokio::net::UnixListener> {
    let listener = listen_given(path, given)?;
    listener.set_nonblocking(true)?;
    Ok(tokio::net::UnixListener::from_std(listener)?)
}

/// The longest a socket's path may be here, its terminating NUL aside.
const MOST: usize = if cfg!(target_os = "macos") { 103 } else { 107 };

/// Fails if `path` is too long for a socket here, said with what to do
/// rather than as the system's limit: for one bound at its path itself.
pub fn check_path(path: &Path) -> anyhow::Result<()> {
    check_length(path, path.as_os_str().len())
}

/// Fails if a socket asked for at `path` takes a path of `long` bytes,
/// past [`MOST`].
fn check_length(path: &Path, long: usize) -> anyhow::Result<()> {
    if long > MOST {
        anyhow::bail!(
            "listening at {}: the socket's path is {long} bytes as it's made or reached, past \
             the {MOST} a socket's path may be here: Link makes its sockets under your home \
             (~/.lnk), so a home with a shorter path runs it",
            path.display()
        );
    }
    Ok(())
}

/// The folder a socket is made in, a new one of this user's alone (0700)
/// in its path's folder, both held open: everything done there (made,
/// given, moved in place, removed) is done through the two handles, never
/// through a path a sandbox writing that folder could swap a link into.
/// On Linux it's bound through the folder's handle too
/// (`/proc/self/fd/<folder>/s`); elsewhere, with neither `/proc` nor
/// `bindat`, it's bound at its path, where a link swapped in makes at most
/// a socket elsewhere, a new file named `s` in a folder the user writes,
/// which nothing here then touches: the one thing left there, said in the
/// sandbox's known gaps. (Never `fchdir`, which is the whole process's.)
struct Held {
    parent: OwnedFd,
    name: CString,
    folder: OwnedFd,
}

impl Held {
    /// A new folder in `parent`.
    fn make(parent: &Path) -> std::io::Result<Held> {
        let path = c_path(parent)?;
        // Only reached through, never read: on Linux it needs no right to
        // read it (`O_PATH`).
        #[cfg(target_os = "linux")]
        let access = libc::O_PATH;
        #[cfg(not(target_os = "linux"))]
        let access = libc::O_RDONLY;
        // SAFETY: a NUL-terminated path, opened as a folder.
        let parent = owned(unsafe {
            libc::open(path.as_ptr(), access | libc::O_DIRECTORY | libc::O_CLOEXEC)
        })?;
        for _ in 0..8 {
            let name = CString::new(link_files::random_token()?[..6].to_string())
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
            // SAFETY: a held folder and a NUL-terminated name in it.
            match ok(unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) }) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
            // SAFETY: a held folder and a NUL-terminated name in it: the
            // folder just made, or nothing, never a link swapped in.
            let folder = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            return match owned(folder) {
                // Another folder can be swapped in between its making and
                // its opening, by anyone writing `parent`: only this
                // user's alone, and empty, is the one just made, or as
                // good as it.
                Ok(folder) if !made_here(&folder)? => Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "the folder made for a socket was swapped as it was made",
                )),
                Ok(folder) => Ok(Held {
                    parent,
                    name,
                    folder,
                }),
                Err(err) => {
                    // SAFETY: as above; a folder of that name, removed if empty.
                    unsafe {
                        libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR)
                    };
                    Err(err)
                }
            };
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    /// Where the socket is bound: through the folder's handle where the
    /// system has one to bind through, else at its path in `parent`.
    fn bind_path(&self, parent: &Path) -> PathBuf {
        if cfg!(target_os = "linux") {
            PathBuf::from(format!("/proc/self/fd/{}/s", self.folder.as_raw_fd()))
        } else {
            use std::os::unix::ffi::OsStrExt;
            parent
                .join(std::ffi::OsStr::from_bytes(self.name.as_bytes()))
                .join("s")
        }
    }

    /// The socket just bound, `s`, checked as it: a socket of this
    /// user's, its one name. Between the bind and a change of its mode or
    /// owner, a sandbox of this user's writing its path's folder could
    /// put a hard link to a file of the user's in its place, to have that
    /// made anyone's. On Linux the one checked is the one changed (held
    /// by `O_PATH`); elsewhere it's checked by its name just before.
    fn socket(&self) -> std::io::Result<Socket> {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: a held folder and a NUL-terminated name in it.
            let held = owned(unsafe {
                libc::openat(
                    self.folder.as_raw_fd(),
                    c"s".as_ptr(),
                    libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            })?;
            // SAFETY: an open descriptor and room for its status.
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            ok(unsafe { libc::fstat(held.as_raw_fd(), &mut stat) })?;
            is_ours(&stat)?;
            Ok(Socket(held))
        }
        #[cfg(not(target_os = "linux"))]
        {
            // SAFETY: a held folder, a NUL-terminated name in it and room
            // for its status.
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            ok(unsafe {
                libc::fstatat(
                    self.folder.as_raw_fd(),
                    c"s".as_ptr(),
                    &mut stat,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            })?;
            is_ours(&stat)?;
            Ok(Socket(self.folder.as_raw_fd()))
        }
    }

    fn chmod(&self, mode: u32) -> std::io::Result<()> {
        let socket = self.socket()?;
        #[cfg(target_os = "linux")]
        {
            // The file held, through its handle: `fchmod` refuses an
            // `O_PATH` one, and Linux has no `AT_EMPTY_PATH` for a mode.
            let path = CString::new(format!("/proc/self/fd/{}", socket.0.as_raw_fd()))
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
            // SAFETY: a NUL-terminated path.
            ok(unsafe { libc::chmod(path.as_ptr(), mode as libc::mode_t) })
        }
        #[cfg(not(target_os = "linux"))]
        // SAFETY: a held folder and a NUL-terminated name in it.
        ok(unsafe {
            libc::fchmodat(
                socket.0,
                c"s".as_ptr(),
                mode as libc::mode_t,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        })
    }

    fn chown(&self, owner: Option<u32>, group: Option<u32>) -> std::io::Result<()> {
        // -1 leaves it as it is.
        let id = |id: Option<u32>| id.unwrap_or(u32::MAX);
        let socket = self.socket()?;
        #[cfg(target_os = "linux")]
        // SAFETY: a held file, changed itself (`AT_EMPTY_PATH`).
        return ok(unsafe {
            libc::fchownat(
                socket.0.as_raw_fd(),
                c"".as_ptr(),
                id(owner),
                id(group),
                libc::AT_EMPTY_PATH,
            )
        });
        #[cfg(not(target_os = "linux"))]
        // SAFETY: a held folder and a NUL-terminated name in it.
        ok(unsafe {
            libc::fchownat(
                socket.0,
                c"s".as_ptr(),
                id(owner),
                id(group),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        })
    }

    /// Moves the socket to `name` in the parent.
    fn rename_to(&self, name: &std::ffi::OsStr) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let to = CString::new(name.as_bytes())
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        // SAFETY: held folders and NUL-terminated names in them.
        ok(unsafe {
            libc::renameat(
                self.folder.as_raw_fd(),
                c"s".as_ptr(),
                self.parent.as_raw_fd(),
                to.as_ptr(),
            )
        })
    }

    /// What's left: the socket if it's still there, then the folder.
    fn remove(&self) {
        // SAFETY: held folders and NUL-terminated names in them.
        unsafe {
            libc::unlinkat(self.folder.as_raw_fd(), c"s".as_ptr(), 0);
            libc::unlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::AT_REMOVEDIR,
            );
        }
    }
}

/// The socket made, as [`Held::socket`] checked it: on Linux the file
/// itself, held; elsewhere its folder, where it's `s`.
#[cfg(target_os = "linux")]
struct Socket(OwnedFd);
#[cfg(not(target_os = "linux"))]
struct Socket(libc::c_int);

/// An error unless `stat` is of a socket of this user's with one name.
fn is_ours(stat: &libc::stat) -> std::io::Result<()> {
    // SAFETY: geteuid has no failure and touches no memory.
    let me = unsafe { libc::geteuid() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFSOCK || stat.st_uid != me || stat.st_nlink != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the socket made was swapped as it was made",
        ));
    }
    Ok(())
}

/// Whether `folder`, just opened, is this user's alone (none of the
/// group's or others' bits) and empty: as one made here is.
fn made_here(folder: &OwnedFd) -> std::io::Result<bool> {
    // SAFETY: an open descriptor and room for its status.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    ok(unsafe { libc::fstat(folder.as_raw_fd(), &mut stat) })?;
    // SAFETY: geteuid has no failure and touches no memory.
    let me = unsafe { libc::geteuid() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR
        || stat.st_uid != me
        || stat.st_mode & 0o077 != 0
    {
        return Ok(false);
    }
    // Read from a copy of its handle, which `closedir` closes.
    // SAFETY: an open descriptor, copied.
    let copy = unsafe { libc::fcntl(folder.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    let copy = owned(copy)?;
    // SAFETY: a folder's open descriptor, given to the stream that then
    // owns it.
    let dir = unsafe { libc::fdopendir(copy.as_raw_fd()) };
    if dir.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    std::mem::forget(copy);
    let mut empty = true;
    loop {
        // SAFETY: an open stream.
        let entry = unsafe { libc::readdir(dir) };
        if entry.is_null() {
            break;
        }
        // SAFETY: an entry readdir just gave, NUL-terminated.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name != c"." && name != c".." {
            empty = false;
            break;
        }
    }
    // SAFETY: the stream opened above, closed once.
    unsafe { libc::closedir(dir) };
    Ok(empty)
}

fn c_path(path: &Path) -> std::io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
}

/// A descriptor just opened, or its error.
fn owned(fd: libc::c_int) -> std::io::Result<OwnedFd> {
    match fd {
        // SAFETY: the descriptor just opened, this one's alone.
        0.. => Ok(unsafe { OwnedFd::from_raw_fd(fd) }),
        _ => Err(std::io::Error::last_os_error()),
    }
}

fn ok(done: libc::c_int) -> std::io::Result<()> {
    match done {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Set in the process [`a_private_socket_is_never_seen_open`] runs
    /// itself in, under a loose umask of its own.
    const UMASK_CHILD: &str = "LNK_TEST_SOCKET_UMASK";

    /// Made again and again under a loose umask (002), with a look at
    /// its path and at every folder beside it as it's made: the socket
    /// is never seen anywhere others could connect, and nothing is left
    /// but it. Run in a process of its own, whose umask no other test
    /// shares.
    #[test]
    fn a_private_socket_is_never_seen_open() {
        if std::env::var_os(UMASK_CHILD).is_none() {
            let output = link_secretive::process::command(std::env::current_exe().unwrap())
                .args([
                    "socket::tests::a_private_socket_is_never_seen_open",
                    "--exact",
                    "--test-threads=1",
                ])
                .env(UMASK_CHILD, "1")
                .output()
                .unwrap();
            let said = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success(), "{said}");
            assert!(said.contains("1 passed"), "{said}");
            return;
        }
        let dir = link_test_kit::TempDir::new("socket-private");
        let path = dir.join("control");
        // SAFETY: this process's umask, which runs this test alone.
        let before = unsafe { libc::umask(0o002) };
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let seen = std::thread::spawn({
            let (dir, path, done) = (dir.to_path_buf(), path.clone(), done.clone());
            move || {
                let mut open = std::collections::BTreeSet::new();
                let mode = |p: &Path| {
                    std::fs::symlink_metadata(p)
                        .ok()
                        .map(|m| m.permissions().mode() & 0o777)
                };
                while !done.load(Ordering::SeqCst) {
                    if let Some(m) = mode(&path).filter(|m| *m != 0o600) {
                        open.insert(format!("control {m:o}"));
                    }
                    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                        let name = entry.file_name();
                        if name == "control" {
                            continue;
                        }
                        if let Some(m) = mode(&entry.path()).filter(|m| *m != 0o700) {
                            open.insert(format!("{} {m:o}", name.to_string_lossy()));
                        }
                    }
                }
                open
            }
        });
        for _ in 0..300 {
            drop(listen_private(&path).unwrap());
        }
        done.store(true, Ordering::SeqCst);
        // SAFETY: the umask as it was.
        unsafe { libc::umask(before) };
        let seen = seen.join().unwrap();
        assert!(seen.is_empty(), "seen open: {seen:?}");
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["control"]);
    }

    /// One that can't be made says the path asked for, and leaves
    /// nothing.
    #[test]
    fn a_socket_that_cant_be_made_names_its_path() {
        let dir = link_test_kit::TempDir::new("socket-refused");
        let path = dir.join("nowhere/control");
        let err = format!("{:#}", listen_private(&path).unwrap_err());
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    /// At a path as long as a socket's may be (and, where it's bound at
    /// a path in its folder, with room for that), made; one byte longer,
    /// or a name that makes the path it's reached at so, refused naming
    /// the path asked for, and leaving nothing.
    #[test]
    fn a_socket_at_the_length_limit_is_made_and_past_it_refused() {
        let dir = link_test_kit::TempDir::new("socket-long");
        let folder_of = |len: usize| {
            let pad = len - dir.as_os_str().len() - 1;
            let folder = dir.join("f".repeat(pad));
            std::fs::create_dir_all(&folder).unwrap();
            folder
        };
        // On Linux the path it's reached at, `/c`: MOST bytes; elsewhere
        // its folder's path, `/.XXXXXX/s`.
        let room = match cfg!(target_os = "linux") {
            true => "/c".len(),
            false => "/.XXXXXX/s".len(),
        };
        let fits = folder_of(MOST - room).join("c");
        drop(listen_private(&fits).unwrap());
        assert!(fits.exists());
        let past = folder_of(MOST - room + 1).join("c");
        let err = format!("{:#}", listen_private(&past).unwrap_err());
        assert!(err.contains(&past.display().to_string()), "{err}");
        assert!(err.contains("past the"), "{err}");
        assert_eq!(
            std::fs::read_dir(past.parent().unwrap()).unwrap().count(),
            0
        );
        // A name long enough that the path it's reached at is past the
        // limit, though the one it's made at isn't: refused too.
        let folder = fits.parent().unwrap();
        let named = folder.join("n".repeat(MOST - folder.as_os_str().len()));
        assert_eq!(named.as_os_str().len(), MOST + 1);
        let err = format!("{:#}", listen_private(&named).unwrap_err());
        assert!(err.contains(&named.display().to_string()), "{err}");
        assert!(err.contains("past the"), "{err}");
        assert!(!named.exists());
    }

    /// Given to a group with its mode, it's so the moment it's at its
    /// path.
    #[test]
    fn a_socket_given_is_so_when_its_there() {
        use std::os::unix::fs::MetadataExt;
        let dir = link_test_kit::TempDir::new("socket-given");
        let path = dir.join("control");
        // This user's own group: one it may give to without a capability.
        let group = std::fs::metadata(&*dir).unwrap().gid();
        let given = Given {
            group: Some(group),
            mode: 0o660,
            ..Given::default()
        };
        drop(listen_given(&path, given).unwrap());
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o660);
        assert_eq!(meta.gid(), group);
    }

    /// A link put in the socket's place in its folder, as a sandbox that
    /// writes the folder's folder could: its mode is never set through
    /// it, and what's moved in place is the link, never what it names.
    #[test]
    fn a_link_in_the_sockets_place_is_never_followed() {
        use std::os::unix::ffi::OsStrExt;
        let dir = link_test_kit::TempDir::new("socket-link");
        let yours = dir.join("yours");
        std::fs::write(&yours, "keep").unwrap();
        std::fs::set_permissions(&yours, std::fs::Permissions::from_mode(0o644)).unwrap();
        let held = Held::make(&dir).unwrap();
        let folder = dir.join(std::ffi::OsStr::from_bytes(held.name.as_bytes()));
        std::os::unix::fs::symlink(&yours, folder.join("s")).unwrap();
        assert!(held.chmod(0o666).is_err());
        let me = std::fs::metadata(&yours).unwrap();
        assert_eq!(me.permissions().mode() & 0o777, 0o644);
        held.rename_to(std::ffi::OsStr::new("control")).unwrap();
        assert!(
            std::fs::symlink_metadata(dir.join("control"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&yours).unwrap(), "keep");
        held.remove();
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        left.sort();
        assert_eq!(left, ["control", "yours"]);
    }

    /// A hard link to a file of the user's put in the socket's place
    /// between its bind and its mode: refused, the file as it was.
    #[test]
    fn a_hard_link_in_the_sockets_place_is_never_changed() {
        use std::os::unix::ffi::OsStrExt;
        let dir = link_test_kit::TempDir::new("socket-hard");
        let yours = dir.join("yours");
        std::fs::write(&yours, "keep").unwrap();
        std::fs::set_permissions(&yours, std::fs::Permissions::from_mode(0o600)).unwrap();
        let held = Held::make(&dir).unwrap();
        let _listener = UnixListener::bind(held.bind_path(&dir)).unwrap();
        let s = dir
            .join(std::ffi::OsStr::from_bytes(held.name.as_bytes()))
            .join("s");
        std::fs::remove_file(&s).unwrap();
        std::fs::hard_link(&yours, &s).unwrap();
        assert!(held.chmod(0o666).is_err());
        assert!(held.chown(None, None).is_err());
        let mode = std::fs::metadata(&yours).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        held.remove();
        assert_eq!(std::fs::read_to_string(&yours).unwrap(), "keep");
    }

    /// Its folder swapped for a link elsewhere again and again while
    /// sockets are made, as a sandbox writing its path's folder could:
    /// nothing is made, changed or removed there. (A path was: a file
    /// named `s` elsewhere was removed.)
    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_swapped_as_its_made_reaches_nothing_elsewhere() {
        let dir = link_test_kit::TempDir::new("socket-swap");
        let elsewhere = link_test_kit::TempDir::new("socket-swap-elsewhere");
        std::fs::write(elsewhere.join("s"), "keep").unwrap();
        let path = dir.join("control");
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let swapping = std::thread::spawn({
            let (dir, elsewhere, done) = (dir.to_path_buf(), elsewhere.to_path_buf(), done.clone());
            move || {
                let mut n = 0;
                while !done.load(Ordering::SeqCst) {
                    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                        let name = entry.file_name();
                        if name.as_encoded_bytes().first() != Some(&b'.') {
                            continue;
                        }
                        n += 1;
                        if std::fs::rename(entry.path(), dir.join(format!("moved-{n}"))).is_ok() {
                            let _ = std::os::unix::fs::symlink(&elsewhere, entry.path());
                        }
                    }
                }
            }
        });
        let mut made = 0;
        for _ in 0..300 {
            made += usize::from(listen_private(&path).is_ok());
            // The links it left, for the next.
            for entry in std::fs::read_dir(&*dir).unwrap().flatten() {
                if entry.file_type().unwrap().is_symlink() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        done.store(true, Ordering::SeqCst);
        swapping.join().unwrap();
        // Made where it wasn't swapped: the test reached what it tests.
        assert!(made > 0, "no socket was made in 300 tries");
        let there: Vec<_> = std::fs::read_dir(&*elsewhere)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(there, ["s"]);
        assert_eq!(
            std::fs::read_to_string(elsewhere.join("s")).unwrap(),
            "keep"
        );
    }

    /// Anyone's ([`Given::OPEN`]) the moment it's at its path, whatever
    /// the umask.
    #[test]
    fn an_open_socket_is_anyones() {
        let dir = link_test_kit::TempDir::new("socket-open");
        let path = dir.join("bridge.sock");
        drop(listen_open(&path).unwrap());
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o666);
    }
}
