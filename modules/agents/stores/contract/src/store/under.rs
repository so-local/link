//! Files under a folder, never through a link: each part of a path
//! opened from the folder before it, and every read, write, rename and
//! removal made relative to that folder's handle ([`crate::nofollow`],
//! whose handle this is, by names that are text). A program outside every sandbox that keeps what a sandbox may
//! write (the `file` place's driver, [`super::file`]) reaches nothing
//! else, however the folder changes meanwhile: a link swapped in is
//! refused, never followed.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Write};

use link_files::nofollow;

pub use link_files::nofollow::{lock_fd, read_open};

/// A folder, held open: a view of [`nofollow::Dir`] by names that are
/// text, a name that isn't there `None`.
#[derive(Debug)]
pub struct Dir(nofollow::Dir);

/// What a name in a folder is, never following a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// A link, a pipe, a device: never read nor followed.
    Other,
}

/// An error unless `name` is one part of a path: not empty, `.` or `..`,
/// no `/` or NUL, at most 255 bytes.
pub fn check_part(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 255
        || name.contains('/')
        || name.contains('\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{:?} isn't a name in a folder", link_format::clean(name)),
        ));
    }
    Ok(())
}

/// `name`, checked to be one part of a path.
fn part(name: &str) -> io::Result<&OsStr> {
    check_part(name)?;
    Ok(OsStr::new(name))
}

/// `ELOOP` (a link where a folder or file was opened) as an error that
/// says so.
fn said(err: io::Error) -> io::Error {
    match err.raw_os_error() {
        Some(libc::ELOOP) | Some(libc::ENOTDIR) => {
            io::Error::other("a link or a file where a folder was: not following it")
        }
        _ => err,
    }
}

/// What was there, `None` when nothing was.
fn found<T>(done: io::Result<T>) -> io::Result<Option<T>> {
    match done {
        Ok(it) => Ok(Some(it)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(said(err)),
    }
}

impl Dir {
    /// The folder at `path`, which the caller trusts: its last part
    /// never a link. None when it isn't there.
    pub fn root(path: &std::path::Path) -> io::Result<Option<Dir>> {
        found(nofollow::Dir::open_unlinked(path)).map(|dir| dir.map(Dir))
    }

    /// This folder, held a second time: for a thread of its own.
    pub fn try_clone(&self) -> io::Result<Dir> {
        Ok(Dir(self.0.try_clone()?))
    }

    /// The folder `name` in this one: made (700) when `make`, else none
    /// when it isn't there. A link there is refused.
    pub fn sub(&self, name: &str, make: bool) -> io::Result<Option<Dir>> {
        let name = part(name)?;
        let opened = match make {
            true => self.0.create_folder(name, 0o700),
            false => self.0.folder(name),
        };
        found(opened).map(|dir| dir.map(Dir))
    }

    /// The folder `path` (parts between `/`) under this one, each part
    /// opened from the one before: made when `make`, else none when one
    /// isn't there.
    pub fn walk(&self, path: &str, make: bool) -> io::Result<Option<Dir>> {
        let mut at = Dir(self.0.try_clone()?);
        for part in path.split('/').filter(|p| !p.is_empty()) {
            match at.sub(part, make)? {
                Some(next) => at = next,
                None => return Ok(None),
            }
        }
        Ok(Some(at))
    }

    /// What `name` is here, never following a link: none when there's
    /// nothing.
    pub fn kind(&self, name: &str) -> io::Result<Option<Kind>> {
        Ok(found(self.0.kind(part(name)?))?.map(|kind| match kind {
            nofollow::Kind::File => Kind::File,
            nofollow::Kind::Folder => Kind::Dir,
            nofollow::Kind::Link | nofollow::Kind::Other => Kind::Other,
        }))
    }

    /// The regular file `name` here, opened with `flags` (and `mode`, made
    /// with `O_CREAT`): never through a link, a pipe or a device refused
    /// without waiting on it, and one with another name too (a hard link)
    /// never opened to write ([`nofollow::Dir::open_file`]).
    pub fn open(&self, name: &str, flags: libc::c_int, mode: u32) -> io::Result<File> {
        self.0.open_file(part(name)?, flags, mode).map_err(said)
    }

    /// The regular file `name`, whole: none when it isn't there; an error
    /// when it holds more than `max` bytes, however it grows meanwhile.
    pub fn read(&self, name: &str, max: u64) -> io::Result<Option<Vec<u8>>> {
        found(self.0.read(part(name)?, max))
    }

    /// Writes `name` with `bytes`, whole or not at all: written aside
    /// under a name of its own, then renamed in place. Only the user's.
    pub fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let aside = self.aside(name)?;
        let written = self
            .open(&aside, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)
            .and_then(|mut file| file.write_all(bytes))
            .and_then(|()| self.rename(&aside, name));
        if written.is_err() {
            let _ = self.remove(&aside);
        }
        written
    }

    /// Atomically write and flush both the file and its directory entry.
    /// Revision reservations use this before changing a data file.
    pub fn write_synced(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.write(name, bytes)?;
        self.open(name, libc::O_RDONLY, 0)?.sync_all()?;
        self.sync_all()
    }

    /// Flush entries changed through this held directory.
    pub fn sync_all(&self) -> io::Result<()> {
        self.0.readable()?.sync_all()
    }

    /// Writes `name` with `bytes` only if there's none: an error of kind
    /// `AlreadyExists` when there is. Whole or not at all (written aside,
    /// then linked in place).
    pub fn write_new(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let aside = self.aside(name)?;
        let written = self
            .open(&aside, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)
            .and_then(|mut file| file.write_all(bytes))
            .and_then(|()| self.0.link(part(&aside)?, part(name)?).map_err(said));
        let _ = self.remove(&aside);
        written
    }

    /// A name of its own beside `name`, for a write made aside.
    fn aside(&self, name: &str) -> io::Result<String> {
        check_part(name)?;
        Ok(link_files::layout::temporary_name())
    }

    /// Renames `from` to `to`, both here, in one step.
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.0.rename(part(from)?, part(to)?).map_err(said)
    }

    /// Moves `from`, here, to `to` in folder `into`, in one step: an
    /// error of kind `NotFound` when `from` isn't there.
    pub fn rename_into(&self, from: &str, into: &Dir, to: &str) -> io::Result<()> {
        self.0
            .rename_to(part(from)?, &into.0, part(to)?)
            .map_err(said)
    }

    /// Removes the file `name`: whether it was there.
    pub fn remove(&self, name: &str) -> io::Result<bool> {
        found(self.0.remove_file(part(name)?)).map(|done| done.is_some())
    }

    /// Removes the empty folder `name`: whether it was there.
    pub fn remove_dir(&self, name: &str) -> io::Result<bool> {
        found(self.0.remove_folder(part(name)?)).map(|done| done.is_some())
    }

    /// The names in this folder that are text, but `.` and `..`.
    pub fn names(&self) -> io::Result<Vec<String>> {
        Ok(self
            .0
            .names()?
            .into_iter()
            .filter_map(|name| name.into_string().ok())
            .collect())
    }

    /// This folder locked: `LOCK_EX` or `LOCK_SH`, waited for, held until
    /// the guard is dropped.
    pub fn lock(&self, how: libc::c_int) -> io::Result<nofollow::Locked> {
        self.0.lock(how)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_anywhere_on_the_way_is_never_followed() {
        let dir = link_test_kit::TempDir::new("under-links");
        let outside = dir.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret"), "theirs").unwrap();
        let root = dir.join("root");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("a/out")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), root.join("a/file")).unwrap();
        let at = Dir::root(&root).unwrap().unwrap();
        let a = at.sub("a", false).unwrap().unwrap();
        assert!(a.sub("out", false).is_err());
        assert!(at.walk("a/out", true).is_err());
        assert!(a.read("file", 100).is_err());
        assert!(a.write("file", b"mine").is_ok());
        // Replaced by a file of its own, never written through.
        assert_eq!(std::fs::read(outside.join("secret")).unwrap(), b"theirs");
        assert_eq!(a.kind("out").unwrap(), Some(Kind::Other));
        for bad in ["..", ".", "", "a/b", "x\0y"] {
            assert!(at.sub(bad, true).is_err(), "{bad:?}");
            assert!(at.read(bad, 10).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_new_file_is_written_once() {
        let dir = link_test_kit::TempDir::new("under-new");
        let at = Dir::root(&dir).unwrap().unwrap();
        at.write_new("x", b"first").unwrap();
        let again = at.write_new("x", b"second").unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(at.read("x", 10).unwrap().unwrap(), b"first");
        let mut names = at.names().unwrap();
        names.sort();
        assert_eq!(names, ["x"]);
        assert!(at.remove("x").unwrap());
        assert!(!at.remove("x").unwrap());
    }
}
