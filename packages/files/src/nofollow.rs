//! Folders another may write (a harness's, an agent's Home, a skill's
//! download), walked by `lnk` outside the sandbox without following a
//! link. Each name is opened relative to its folder's handle
//! with `O_NOFOLLOW`, so a link swapped in after a check, for a file or a
//! folder on the way, is refused rather than followed; and a file is read
//! or written through the handle it was opened as.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// The deepest [`Dir::remove_all`] and [`Dir::copy_into`] go: a folder
/// past it is refused.
pub const DEEPEST: usize = 32;

/// An open folder.
#[derive(Debug)]
pub struct Dir(File);

impl Dir {
    /// Reopen this same directory for listing and syncing, including when
    /// its existing handle was opened only for traversal. No path is resolved.
    pub fn readable(&self) -> io::Result<Dir> {
        // SAFETY: the held directory and literal current-directory name are valid.
        let fd = unsafe {
            libc::openat(
                self.fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        owned(fd).map(File::from).map(Dir)
    }
    /// Flush changes to this directory's entries after an atomic rename.
    pub fn sync_all(&self) -> io::Result<()> {
        self.0.sync_all()
    }

    /// The folder at `path`, a folder of Link's own (a harness's home, the
    /// files folder): a link there is followed, once, here.
    pub fn open(path: &Path) -> io::Result<Dir> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: a valid C string; the result is checked.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        owned(fd).map(File::from).map(Dir)
    }

    /// The folder at `path`, a folder of Link's own whose last part is
    /// never a link: one there is refused, never followed.
    pub fn open_unlinked(path: &Path) -> io::Result<Dir> {
        Self::unlinked_with(path, libc::O_RDONLY)
    }

    /// Opens a parent for operations relative to its handle, refusing a
    /// link at `path`. On macOS, `O_SEARCH` traverses the directory without
    /// asking Seatbelt for permission to read its entries. Open children
    /// separately when their entries must be listed.
    pub fn search_unlinked(path: &Path) -> io::Result<Dir> {
        #[cfg(target_os = "macos")]
        let access = libc::O_SEARCH;
        #[cfg(not(target_os = "macos"))]
        let access = libc::O_RDONLY;
        Self::unlinked_with(path, access)
    }

    fn unlinked_with(path: &Path, access: libc::c_int) -> io::Result<Dir> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: a valid C string; the result is checked.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                access | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        owned(fd).map(File::from).map(Dir)
    }

    /// The folder `name` in this one; a link there is refused (`ELOOP`
    /// or `ENOTDIR`).
    pub fn folder(&self, name: &OsStr) -> io::Result<Dir> {
        self.open_at(
            name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
        .map(File::from)
        .map(Dir)
    }

    /// The folder `name` in this one, a link there followed: only for a
    /// link the caller has checked is the system's own (root's), which no
    /// one else can make.
    pub fn folder_through_link(&self, name: &OsStr) -> io::Result<Dir> {
        self.open_at(name, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .map(File::from)
            .map(Dir)
    }

    /// `/`, to pass through ([`Dir::passing`]) on the way to a folder.
    pub fn root_to_pass() -> io::Result<Dir> {
        Self::unlinked_with(Path::new("/"), PASS)
    }

    /// The folder `name` in this one, opened only to pass through to what
    /// is in it (`O_PATH` on Linux, `O_SEARCH` on macOS), made with `mode`
    /// first when `create` and it's missing; a link there is refused.
    /// Passing asks for no right to read the folder, which a sandbox may
    /// not grant above what it opens (Landlock, Seatbelt): its handle
    /// takes the `*at` calls, never a read, a lock or a sync.
    pub fn passing(&self, name: &OsStr, create: bool, mode: u32) -> io::Result<Dir> {
        if create {
            let c = c_name(name)?;
            // SAFETY: a valid folder handle and C string.
            let made = unsafe { libc::mkdirat(self.fd(), c.as_ptr(), mode as libc::mode_t) };
            if made != 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::EEXIST) {
                    return Err(err);
                }
            }
        }
        self.open_at(
            name,
            PASS | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
        .map(File::from)
        .map(Dir)
    }

    /// The folder `name` in this one, made with `mode` if it's missing; a
    /// link there is refused, never followed.
    pub fn create_folder(&self, name: &OsStr, mode: u32) -> io::Result<Dir> {
        let c = c_name(name)?;
        // SAFETY: a valid folder handle and C string.
        let made = unsafe { libc::mkdirat(self.fd(), c.as_ptr(), mode as libc::mode_t) };
        if made != 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EEXIST) {
                return Err(err);
            }
        }
        self.folder(name)
    }

    /// A new folder `name` in this one, made with `mode`: refused if
    /// anything, a link included, is there already.
    pub fn create_new_folder(&self, name: &OsStr, mode: u32) -> io::Result<Dir> {
        let c = c_name(name)?;
        // SAFETY: a valid folder handle and C string.
        check(unsafe { libc::mkdirat(self.fd(), c.as_ptr(), mode as libc::mode_t) })?;
        self.folder(name)
    }

    /// Whatever `name` is in this one, opened for reading, unless it's a
    /// link (`ELOOP`). Non-blocking, so a pipe can't hold the caller: what
    /// it is, the caller asks the handle (`File::metadata`).
    pub fn open_entry(&self, name: &OsStr) -> io::Result<File> {
        self.open_at(
            name,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
        .map(File::from)
    }

    /// The regular file `name` in this one, opened with `flags` (and
    /// `mode`, made with `O_CREAT`): never through a link, and a pipe or a
    /// device refused without waiting on it. Opened to write, a file with
    /// another name too (a hard link) is refused: what's written here
    /// would be written there. `O_TRUNC` is refused: it would empty
    /// that other name before the refusal (truncate with `set_len` after).
    pub fn open_file(&self, name: &OsStr, flags: libc::c_int, mode: u32) -> io::Result<File> {
        if flags & libc::O_TRUNC != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "O_TRUNC: truncated before it's checked; set_len after instead",
            ));
        }
        let c = c_name(name)?;
        // SAFETY: a valid folder handle and C string; the result is checked.
        let fd = unsafe {
            libc::openat(
                self.fd(),
                c.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                mode as libc::c_uint,
            )
        };
        let file = File::from(owned(fd)?);
        // Checked on the handle it's read from, so it can't change between.
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a regular file",
            ));
        }
        if flags & libc::O_ACCMODE != libc::O_RDONLY
            && std::os::unix::fs::MetadataExt::nlink(&meta) != 1
        {
            return Err(io::Error::other(
                "a file with another name too (a hard link): never written through",
            ));
        }
        Ok(file)
    }

    /// The regular file `name` in this one, whole: an error when it holds
    /// more than `max` bytes, however it grows meanwhile. Not a regular
    /// file, or too large, is an error of kind `InvalidData`.
    pub fn read(&self, name: &OsStr, max: u64) -> io::Result<Vec<u8>> {
        read_open(self.open_file(name, libc::O_RDONLY, 0)?, max)
    }

    /// A new file `name` in this one, with `mode`: refused if anything,
    /// a link included, is there.
    pub fn create_new(&self, name: &OsStr, mode: u32) -> io::Result<File> {
        let c = c_name(name)?;
        // SAFETY: a valid folder handle and C string; the result is checked.
        let fd = unsafe {
            libc::openat(
                self.fd(),
                c.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode as libc::c_uint,
            )
        };
        owned(fd).map(File::from)
    }

    /// Renames `from` to `to`, both in this folder: whatever is at `to`, a
    /// link included, is replaced, never followed.
    pub fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
        let (from, to) = (c_name(from)?, c_name(to)?);
        // SAFETY: a valid folder handle and C strings.
        let done = unsafe { libc::renameat(self.fd(), from.as_ptr(), self.fd(), to.as_ptr()) };
        check(done)
    }

    /// Moves `from` in this folder to `to` in the folder `into`: whatever
    /// is at `to`, a link included, is replaced, never followed.
    pub fn rename_to(&self, from: &OsStr, into: &Dir, to: &OsStr) -> io::Result<()> {
        let (from, to) = (c_name(from)?, c_name(to)?);
        // SAFETY: valid folder handles and C strings.
        let done = unsafe { libc::renameat(self.fd(), from.as_ptr(), into.fd(), to.as_ptr()) };
        check(done)
    }

    /// A second name `to` in this folder for the file `from`: refused
    /// (`AlreadyExists`) if anything is at `to`.
    pub fn link(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
        let (from, to) = (c_name(from)?, c_name(to)?);
        // SAFETY: a valid folder handle and C strings.
        check(unsafe { libc::linkat(self.fd(), from.as_ptr(), self.fd(), to.as_ptr(), 0) })
    }

    /// This folder locked, `LOCK_EX` or `LOCK_SH`, waited for, until the
    /// guard is dropped. Through a handle of the guard's own, opened for
    /// it: a lock is its open file's, which a duplicate of this handle
    /// would share, so this one's lock would last as long as this handle,
    /// and another thread holding it would never wait on it.
    pub fn lock(&self, how: libc::c_int) -> io::Result<Locked> {
        // SAFETY: a valid folder handle and a constant C string; the
        // result is checked.
        let fd = unsafe {
            libc::openat(
                self.fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        let held = Dir(owned(fd).map(File::from)?);
        lock_fd(held.fd(), how)?;
        Ok(Locked(held))
    }

    /// Removes the file or link `name` from this folder (never a link's
    /// target).
    pub fn remove_file(&self, name: &OsStr) -> io::Result<()> {
        let c = c_name(name)?;
        // SAFETY: a valid folder handle and C string.
        check(unsafe { libc::unlinkat(self.fd(), c.as_ptr(), 0) })
    }

    /// Removes the empty folder `name` from this one; a link there is
    /// refused (`ENOTDIR`), never followed.
    pub fn remove_folder(&self, name: &OsStr) -> io::Result<()> {
        let c = c_name(name)?;
        // SAFETY: a valid folder handle and C string.
        check(unsafe { libc::unlinkat(self.fd(), c.as_ptr(), libc::AT_REMOVEDIR) })
    }

    /// Removes `name` in this folder and all it holds: each name reached
    /// from its folder's handle, a link removed itself, never followed.
    /// Each folder of this user's that its owner may not write or search
    /// (a Go module cache's, `0555`) is given them first, on its handle,
    /// so its names can go. A tree past [`DEEPEST`] folders deep is
    /// refused there, never walked further (each level a handle). What
    /// can go goes, past a name that can't: the error names the first
    /// left, by its path from `name`, with the kind of its cause.
    pub fn remove_all(&self, name: &OsStr) -> io::Result<()> {
        self.remove_under(name, 1)
    }

    fn remove_under(&self, name: &OsStr, depth: usize) -> io::Result<()> {
        let at = |err: io::Error| left_at(name, err);
        if self.kind(name).map_err(at)? != Kind::Folder {
            return self.remove_file(name).map_err(at);
        }
        if depth > DEEPEST {
            return Err(at(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("folders more than {DEEPEST} deep: not removed past that"),
            )));
        }
        let inside = self.folder(name).map_err(at)?;
        inside.owner_may_empty().map_err(at)?;
        let mut first = None;
        for entry in inside.names().map_err(at)? {
            // A name gone meanwhile is as good as removed.
            if let Err(err) = inside.remove_under(&entry, depth + 1)
                && err.kind() != io::ErrorKind::NotFound
            {
                first.get_or_insert(io::Error::new(
                    err.kind(),
                    format!("{}/{err}", Path::new(name).display()),
                ));
            }
        }
        match first {
            Some(err) => Err(err),
            None => self.remove_folder(name).map_err(at),
        }
    }

    /// Gives this folder's owner the right to list, search and write it
    /// (`u+rwx`, on the handle), where this user owns it and lacks one:
    /// the folder is going, and its names with it.
    fn owner_may_empty(&self) -> io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let metadata = self.metadata()?;
        // SAFETY: geteuid has no failure.
        if metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o700 != 0o700 {
            self.set_mode((metadata.mode() | 0o700) & 0o7777)?;
        }
        Ok(())
    }

    /// Sets this folder's permissions, on the handle (`fchmod`).
    pub fn set_mode(&self, mode: u32) -> io::Result<()> {
        // SAFETY: a valid folder handle.
        check(unsafe { libc::fchmod(self.fd(), mode as libc::mode_t) })
    }

    /// Another handle on the same folder.
    pub fn try_clone(&self) -> io::Result<Dir> {
        self.0.try_clone().map(Dir)
    }

    /// What `name` in this folder is, not following a link.
    pub fn kind(&self, name: &OsStr) -> io::Result<Kind> {
        self.stat(name).map(|stat| stat.kind)
    }

    /// `name` in this folder as it is (`fstatat`), not following a link.
    pub fn stat(&self, name: &OsStr) -> io::Result<Stat> {
        let c = c_name(name)?;
        // SAFETY: a zeroed stat is valid to write into.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: a valid folder handle, C string and stat buffer.
        check(unsafe {
            libc::fstatat(self.fd(), c.as_ptr(), &mut stat, libc::AT_SYMLINK_NOFOLLOW)
        })?;
        #[allow(
            clippy::unnecessary_cast,
            reason = "stat's field types differ between Linux and macOS: a cast that's a no-op on one widens on the other"
        )]
        Ok(Stat {
            kind: match stat.st_mode & libc::S_IFMT {
                libc::S_IFDIR => Kind::Folder,
                libc::S_IFREG => Kind::File,
                libc::S_IFLNK => Kind::Link,
                _ => Kind::Other,
            },
            mode: stat.st_mode as u32,
            uid: stat.st_uid as u64,
            gid: stat.st_gid as u64,
            dev: stat.st_dev as u64,
            ino: stat.st_ino as u64,
            size: stat.st_size.max(0) as u64,
            mtime: stat.st_mtime.max(0) as u64,
            modified_ns: (stat.st_mtime as i64)
                .saturating_mul(1_000_000_000)
                .saturating_add(stat.st_mtime_nsec as i64),
        })
    }

    /// The target of the link `name` in this folder.
    pub fn read_link(&self, name: &OsStr) -> io::Result<PathBuf> {
        let c = c_name(name)?;
        let mut buf = vec![0u8; libc::PATH_MAX as usize + 1];
        // SAFETY: a valid folder handle, C string and buffer of that size.
        let len =
            unsafe { libc::readlinkat(self.fd(), c.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if len < 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(len as usize);
        Ok(PathBuf::from(OsString::from_vec(buf)))
    }

    /// The names in this folder, sorted, but `.` and `..`.
    pub fn names(&self) -> io::Result<Vec<OsString>> {
        // SAFETY: fdopendir takes ownership of the duplicate, closedir
        // closes it; readdir's entries are copied before the next call.
        let mut names = unsafe {
            let dup = libc::fcntl(self.fd(), libc::F_DUPFD_CLOEXEC, 0);
            if dup < 0 {
                return Err(io::Error::last_os_error());
            }
            let dir = libc::fdopendir(dup);
            if dir.is_null() {
                let err = io::Error::last_os_error();
                libc::close(dup);
                return Err(err);
            }
            // From the start, whoever read this handle before.
            libc::rewinddir(dir);
            let mut names = Vec::new();
            loop {
                let entry = libc::readdir(dir);
                if entry.is_null() {
                    break;
                }
                let name = CStr::from_ptr((*entry).d_name.as_ptr()).to_bytes();
                if name != b"." && name != b".." {
                    names.push(OsString::from_vec(name.to_vec()));
                }
            }
            libc::closedir(dir);
            names
        };
        names.sort();
        Ok(names)
    }

    /// A link `name` in this one, to `target`: refused if anything is
    /// there.
    pub fn symlink(&self, target: &Path, name: &OsStr) -> io::Result<()> {
        let (target, c) = (
            CString::new(target.as_os_str().as_bytes())
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
            c_name(name)?,
        );
        // SAFETY: a valid folder handle and C strings.
        check(unsafe { libc::symlinkat(target.as_ptr(), self.fd(), c.as_ptr()) })
    }

    /// What this folder holds, copied into folder `to`, as `copying`
    /// says: each name opened from its folder's handle and never through
    /// a link, on either side (one swapped in meanwhile is refused), and
    /// each file a copy of its own, never a hard link that a later write
    /// to either would change in both. Files and folders are copied,
    /// links as [`Copying::links`] says, anything else (a pipe, a device)
    /// left out. An error past [`Copying::limit`].
    pub fn copy_into(&self, to: &Dir, copying: &Copying) -> anyhow::Result<Copied> {
        let mut copied = Copied::default();
        self.copy_under(to, Path::new(""), copying, &mut copied)?;
        Ok(copied)
    }

    fn copy_under(
        &self,
        to: &Dir,
        under: &Path,
        copying: &Copying,
        copied: &mut Copied,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;
        let names = self
            .names()
            .with_context(|| format!("reading {}", under.display()))?;
        for name in names {
            let inside = under.join(&name);
            if (copying.skip)(&inside) {
                continue;
            }
            let kind = self
                .kind(&name)
                .with_context(|| format!("reading {}", inside.display()))?;
            match kind {
                Kind::Folder => {
                    copied.folders += 1;
                    // However it's limited, never deeper than a walk
                    // holds handles for.
                    anyhow::ensure!(
                        inside.components().count() <= DEEPEST,
                        "{} is more than {DEEPEST} folders deep: not copied",
                        inside.display()
                    );
                    if let Some(limit) = &copying.limit {
                        anyhow::ensure!(
                            copied.folders <= limit.folders,
                            "{} holds more than {} folders",
                            limit.what,
                            limit.folders
                        );
                        anyhow::ensure!(
                            inside.components().count() <= limit.depth,
                            "{} holds folders more than {} deep",
                            limit.what,
                            limit.depth
                        );
                    }
                    let source = self
                        .folder(&name)
                        .with_context(|| format!("opening {}", inside.display()))?;
                    let made = to
                        .create_new_folder(&name, copying.folder_mode())
                        .with_context(|| format!("creating {}", inside.display()))?;
                    source.copy_under(&made, &inside, copying, copied)?;
                }
                Kind::File => {
                    let mut source = self
                        .open_entry(&name)
                        .with_context(|| format!("opening {}", inside.display()))?;
                    let meta = source.metadata()?;
                    anyhow::ensure!(
                        meta.is_file(),
                        "{} changed as it was read",
                        inside.display()
                    );
                    copied.files += 1;
                    copied.bytes += meta.len();
                    if let Some(limit) = &copying.limit {
                        anyhow::ensure!(
                            copied.files <= limit.files,
                            "{} holds more than {} files",
                            limit.what,
                            limit.files
                        );
                        anyhow::ensure!(
                            copied.bytes <= limit.bytes,
                            "{} holds more than {} MB",
                            limit.what,
                            limit.bytes >> 20
                        );
                    }
                    let mut copy = to
                        .create_new(&name, copying.file_mode(meta.permissions().mode()))
                        .with_context(|| format!("writing {}", inside.display()))?;
                    let done = io::copy(&mut (&mut source).take(meta.len()), &mut copy)
                        .with_context(|| format!("copying {}", inside.display()))?;
                    anyhow::ensure!(
                        done == meta.len(),
                        "{} changed as it was read",
                        inside.display()
                    );
                }
                Kind::Link => match copying.links {
                    Links::Keep => {
                        let target = self
                            .read_link(&name)
                            .with_context(|| format!("reading {}", inside.display()))?;
                        to.symlink(&target, &name)
                            .with_context(|| format!("writing {}", inside.display()))?;
                    }
                    Links::LeaveOut => copied.left_out.push(inside),
                    Links::Refuse => anyhow::bail!("{} is a link", inside.display()),
                },
                Kind::Other => copied.left_out.push(inside),
            }
        }
        Ok(())
    }

    /// Its metadata (`fstat`).
    pub fn metadata(&self) -> io::Result<std::fs::Metadata> {
        self.0.metadata()
    }

    fn fd(&self) -> libc::c_int {
        self.0.as_raw_fd()
    }

    fn open_at(&self, name: &OsStr, flags: libc::c_int) -> io::Result<OwnedFd> {
        let c = c_name(name)?;
        // SAFETY: a valid folder handle and C string; the result is checked.
        owned(unsafe { libc::openat(self.fd(), c.as_ptr(), flags) })
    }
}

/// Its descriptor, for a call that takes the folder itself, such as a
/// child's `fchdir` into it, so no path is resolved again.
impl std::os::fd::AsFd for Dir {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.0)
    }
}

/// A folder opened as a file ([`Dir::open_entry`]) that is one.
impl From<File> for Dir {
    fn from(file: File) -> Dir {
        Dir(file)
    }
}

/// A name in a folder, as it is.
#[derive(Clone, Copy, Debug)]
pub struct Stat {
    pub kind: Kind,
    /// Its type and permissions (`st_mode`).
    pub mode: u32,
    pub uid: u64,
    pub gid: u64,
    /// The device it's on (`st_dev`) and its number there (`st_ino`):
    /// together, the file whatever name reaches it.
    pub dev: u64,
    pub ino: u64,
    /// In bytes.
    pub size: u64,
    /// Seconds since 1970.
    pub mtime: u64,
    /// Nanoseconds since 1970.
    pub modified_ns: i64,
}

/// A folder's lock ([`Dir::lock`]), let go when dropped.
#[derive(Debug)]
pub struct Locked(
    #[expect(dead_code, reason = "held only so the lock lasts until it's dropped")] Dir,
);

/// Takes `how` of the file or folder `fd`, waiting for it.
pub fn lock_fd(fd: libc::c_int, how: libc::c_int) -> io::Result<()> {
    loop {
        // SAFETY: a descriptor the caller holds open.
        if unsafe { libc::flock(fd, how) } == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

/// `file`, opened non-blocking ([`Dir::open_entry`]), set back to
/// blocking, as a reader that waits on it expects.
pub fn set_blocking(file: &File) -> io::Result<()> {
    // SAFETY: an open file's flags, read and set.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above.
    check(unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) })
}

/// `file`, whole: an error when it holds more than `max` bytes.
pub fn read_open(file: File, max: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("larger than {max} bytes"),
        )
    };
    if file.metadata()?.len() > max {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(too_large());
    }
    Ok(bytes)
}

/// What's done with a link in what's copied ([`Dir::copy_into`]) or
/// unpacked: made again as the same link, left out, or refused, nothing
/// of it written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Links {
    Keep,
    LeaveOut,
    Refuse,
}

/// How [`Dir::copy_into`] copies.
pub struct Copying<'a> {
    /// Whether a path in what's copied is left out, unsaid (a skill's
    /// `.git`).
    pub skip: &'a dyn Fn(&Path) -> bool,
    pub links: Links,
    /// At most so many files, bytes and folders, and folders so deep,
    /// else an error; none, no limit but [`DEEPEST`] folders deep.
    pub limit: Option<Limit<'a>>,
    /// Folders made `0o700` and files `0o600`, the user's alone; else
    /// folders `0o755`, and files their permissions, but anyone's but
    /// the user's write, and the user's read and write.
    pub private: bool,
}

impl Copying<'_> {
    /// Every file and folder, the user's alone, links as `links` says.
    pub fn private(links: Links) -> Copying<'static> {
        Copying {
            skip: &|_| false,
            links,
            limit: None,
            private: true,
        }
    }

    fn folder_mode(&self) -> u32 {
        match self.private {
            true => 0o700,
            false => 0o755,
        }
    }

    fn file_mode(&self, mode: u32) -> u32 {
        match self.private {
            true => 0o600,
            false => (mode & 0o755) | 0o600,
        }
    }
}

/// The most [`Dir::copy_into`] copies, and what's said of it past that
/// (`the skill`).
pub struct Limit<'a> {
    pub files: usize,
    pub bytes: u64,
    /// Folders in all, and how deep in it one may be: a tree very wide or
    /// very deep, of folders alone, costs a walk and handles all the same.
    pub folders: usize,
    pub depth: usize,
    pub what: &'a str,
}

/// What [`Dir::copy_into`] copied: so many files, bytes and folders, and what it
/// left out by its path in the copy (a link, a pipe).
#[derive(Debug, Default)]
pub struct Copied {
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
    pub left_out: Vec<PathBuf>,
}

/// What a name in a folder is, as it is: a link is a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File,
    Link,
    /// A pipe, a socket, a device.
    Other,
}

/// Whether `err` is a link refused where a file or folder was
/// (`O_NOFOLLOW`: `ELOOP`, or `ENOTDIR` with `O_DIRECTORY` on some
/// systems).
pub fn is_link(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(libc::ELOOP | libc::EMLINK))
}

/// `name` for the system, refused unless it's one name in a folder: not
/// empty, `.`, `..`, nor with a `/`.
/// How [`Dir::passing`] opens a folder: to pass through, never to read.
#[cfg(target_os = "linux")]
const PASS: libc::c_int = libc::O_PATH;
#[cfg(target_os = "macos")]
const PASS: libc::c_int = libc::O_SEARCH;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const PASS: libc::c_int = libc::O_RDONLY;

/// `err`, a removal's, named by its path from `name` down.
fn left_at(name: &OsStr, err: io::Error) -> io::Error {
    io::Error::new(err.kind(), format!("{}: {err}", Path::new(name).display()))
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    let bytes = name.as_bytes();
    if matches!(bytes, b"" | b"." | b"..") || bytes.contains(&b'/') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    CString::new(bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new, open descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn check(done: libc::c_int) -> io::Result<()> {
    match done {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

/// `path` opened with `O_PATH` and `flags`: a handle on it, read by no
/// one. Not through `OpenOptions::custom_flags`, which masks out
/// `O_ACCMODE`, and musl's (the Linux release's) includes `O_PATH`: the
/// file would be opened for reading instead, and a socket not at all.
#[cfg(target_os = "linux")]
pub fn open_path(path: &Path, flags: libc::c_int) -> io::Result<File> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: a NUL-terminated path, and flags that need no mode.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC | flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a file descriptor just opened, owned by nothing else.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// A unix socket at `path`, checked the one safe way for a path a sandbox
/// or another agent may write: never through a link (`O_PATH |
/// O_NOFOLLOW`), and only a socket whose owner `owned` takes, so a
/// program that swapped it for a link or another user's socket can't
/// redirect a connection. Connect to [`Socket::at`] while it's held: on
/// Linux that's the opened handle (`/proc/self/fd/<n>`), so nothing
/// swapped between the check and the connect is reached; elsewhere,
/// where a socket has no `O_PATH`, the path it named. Every connect to
/// such a path, blocking or async, is checked here.
pub fn socket(path: &Path, owned: impl Fn(u32) -> bool) -> io::Result<Socket> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let not_its = || {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} isn't a unix socket of an owner it may be",
                path.display()
            ),
        )
    };
    #[cfg(target_os = "linux")]
    {
        let file = open_path(path, libc::O_NOFOLLOW)?;
        let meta = file.metadata()?;
        if !meta.file_type().is_socket() || !owned(meta.uid()) {
            return Err(not_its());
        }
        let at = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
        Ok(Socket { _held: file, at })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let meta = std::fs::symlink_metadata(path)?;
        if !meta.file_type().is_socket() || !owned(meta.uid()) {
            return Err(not_its());
        }
        Ok(Socket {
            at: path.to_path_buf(),
        })
    }
}

/// A socket [`socket`] checked: what to connect to while it's held.
pub struct Socket {
    #[cfg(target_os = "linux")]
    _held: File,
    /// Where to connect.
    pub at: PathBuf,
}

/// A connection to the unix socket at `path`, a socket this user owns,
/// reached as [`socket`] checks it. A raw
/// [`std::os::unix::net::UnixStream::connect`] of such a path would follow
/// a planted link.
pub fn connect(path: &Path) -> io::Result<std::os::unix::net::UnixStream> {
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    let socket = socket(path, |uid| uid == me)?;
    std::os::unix::net::UnixStream::connect(&socket.at)
}

/// A private file unlinked immediately after opening. It disappears on close.
/// Large transfers use it to hash and validate bytes before committing them.
pub fn temporary(spool: &std::path::Path) -> std::io::Result<std::fs::File> {
    crate::create_private_dir(spool).map_err(std::io::Error::other)?;
    let dir = Dir::open_unlinked(spool)?;
    let name = format!(
        "spool-{}",
        crate::random_token().map_err(std::io::Error::other)?
    );
    let file = dir.open_file(
        std::ffi::OsStr::new(&name),
        libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    dir.remove_file(std::ffi::OsStr::new(&name))?;
    Ok(file)
}

#[cfg(test)]
mod tests;
