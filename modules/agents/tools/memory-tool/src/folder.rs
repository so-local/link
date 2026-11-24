//! The folder, read without following a link. Every name is opened
//! relative to its folder's handle with `O_NOFOLLOW`, so a link swapped
//! in after the walk, for a file or a folder on the way, is refused
//! rather than followed; and a file is read from the handle it was
//! checked on, so what it is and how large can't change in between.

use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::Path;

use link_files::nofollow;

/// One entry of a folder, as it is: a link is a link, never its target.
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    /// Nanoseconds since 1970.
    pub modified: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File,
    /// A link, a pipe, a device: never read.
    Other,
}

/// An open folder: a view of [`nofollow::Dir`], by names that are text.
pub struct Dir(nofollow::Dir);

impl Dir {
    /// The folder at `path`, which is where it's configured to be: a link
    /// there is followed, once, here.
    pub fn open(path: &Path) -> io::Result<Dir> {
        nofollow::Dir::open(path).map(Dir)
    }

    /// Traverse a parent whose children are granted separately, without
    /// asking to read the parent's directory entries.
    pub fn search(path: &Path) -> io::Result<Dir> {
        nofollow::Dir::search_unlinked(path).map(Dir)
    }

    /// The folder `name` in this one; a link there is refused.
    pub fn folder(&self, name: &str) -> io::Result<Dir> {
        self.0.folder(OsStr::new(name)).map(Dir)
    }

    /// Its entries that are text, as they are.
    pub fn entries(&self) -> io::Result<Vec<Entry>> {
        let mut entries = Vec::new();
        for name in self.0.names()? {
            let Ok(stat) = self.0.stat(&name) else {
                // Gone since it was listed.
                continue;
            };
            let Ok(name) = name.into_string() else {
                continue;
            };
            entries.push(Entry {
                name,
                kind: match stat.kind {
                    nofollow::Kind::Folder => Kind::Folder,
                    nofollow::Kind::File => Kind::File,
                    nofollow::Kind::Link | nofollow::Kind::Other => Kind::Other,
                },
                size: stat.size,
                modified: stat.modified_ns,
            });
        }
        Ok(entries)
    }

    /// The file at `rel` (names joined by `/`) under this folder, at most
    /// `max` bytes: `None` when a name on the way is a link, it isn't a
    /// regular file, or it's larger.
    pub fn read(&self, rel: &str, max: u64) -> io::Result<Option<Vec<u8>>> {
        let Some((dir, last)) = self.parent_of(rel)? else {
            return Ok(None);
        };
        match dir.as_ref().unwrap_or(&self.0).read(OsStr::new(last), max) {
            Ok(bytes) => Ok(Some(bytes)),
            // Not a regular file, or larger than `max`.
            Err(err) if refused(&err) || err.kind() == io::ErrorKind::InvalidData => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// The last `max` bytes of the file at `rel`, read on the handle it
    /// was opened as, and how many came before them: `None` as
    /// [`Dir::read`] gives it, but for its size.
    pub fn read_tail(&self, rel: &str, max: u64) -> io::Result<Option<(Vec<u8>, u64)>> {
        use std::io::{Read, Seek, SeekFrom};
        let Some((dir, last)) = self.parent_of(rel)? else {
            return Ok(None);
        };
        let mut file =
            match dir
                .as_ref()
                .unwrap_or(&self.0)
                .open_file(OsStr::new(last), libc::O_RDONLY, 0)
            {
                Ok(file) => file,
                Err(err) if refused(&err) || err.kind() == io::ErrorKind::InvalidData => {
                    return Ok(None);
                }
                Err(err) => return Err(err),
            };
        let size = file.metadata()?.len();
        let skipped = size.saturating_sub(max);
        file.seek(SeekFrom::Start(skipped))?;
        let mut bytes = Vec::new();
        // Grown meanwhile: no more than `max`, the rest read next time.
        file.take(max).read_to_end(&mut bytes)?;
        Ok(Some((bytes, skipped)))
    }

    /// Bytes `start` to `end` of the file at `rel`, fewer where it ends
    /// sooner, read on the handle it was opened as: `None` as
    /// [`Dir::read`] gives it, but for its size.
    pub fn read_range(&self, rel: &str, start: u64, end: u64) -> io::Result<Option<Vec<u8>>> {
        use std::io::{Read, Seek, SeekFrom};
        let Some((dir, last)) = self.parent_of(rel)? else {
            return Ok(None);
        };
        let mut file =
            match dir
                .as_ref()
                .unwrap_or(&self.0)
                .open_file(OsStr::new(last), libc::O_RDONLY, 0)
            {
                Ok(file) => file,
                Err(err) if refused(&err) || err.kind() == io::ErrorKind::InvalidData => {
                    return Ok(None);
                }
                Err(err) => return Err(err),
            };
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(end.saturating_sub(start))
            .read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    /// The folder `rel`'s last name is in (none: this one), opened a name
    /// at a time, and that name: `None` when a name is empty, `.` or
    /// `..`, or one on the way is a link or gone.
    /// The entry at `rel` (names joined by `/`) under this folder, as it
    /// is: none when it's gone, or a name on the way is a link or gone.
    pub fn entry(&self, rel: &str) -> io::Result<Option<Entry>> {
        let parent = match self.parent_of(rel) {
            Ok(Some(parent)) => parent,
            Ok(None) => return Ok(None),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let (dir, last) = parent;
        let stat = match dir.as_ref().unwrap_or(&self.0).stat(OsStr::new(last)) {
            Ok(stat) => stat,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        Ok(Some(Entry {
            name: last.to_string(),
            kind: match stat.kind {
                nofollow::Kind::Folder => Kind::Folder,
                nofollow::Kind::File => Kind::File,
                nofollow::Kind::Link | nofollow::Kind::Other => Kind::Other,
            },
            size: stat.size,
            modified: stat.modified_ns,
        }))
    }

    fn parent_of<'r>(&self, rel: &'r str) -> io::Result<Option<(Option<nofollow::Dir>, &'r str)>> {
        let names: Vec<&str> = rel.split('/').collect();
        let (last, folders) = names.split_last().expect("split gives one");
        if names
            .iter()
            .any(|n| n.is_empty() || *n == "." || *n == "..")
        {
            return Ok(None);
        }
        let mut dir: Option<nofollow::Dir> = None;
        for name in folders {
            let next = match dir.as_ref().unwrap_or(&self.0).folder(OsStr::new(name)) {
                Ok(next) => next,
                Err(err) if refused(&err) => return Ok(None),
                Err(err) => return Err(err),
            };
            dir = Some(next);
        }
        Ok(Some((dir, last)))
    }

    /// Replaces file `name` in this folder with `bytes`, whole, in one
    /// step: written to a file of its own beside it, then renamed over
    /// it, so a reader sees the old or the new, never part, and the
    /// folder synced, so the rename outlasts a crash. A link at `name` is
    /// replaced, never followed. The file is the user's alone (0600):
    /// what an agent noted, kept by a service other users' agents may
    /// share a machine with.
    pub fn replace(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let temp = link_files::layout::temporary_name();
        let temp = OsStr::new(&temp);
        // Drawn at random and made new: another writer's file is never
        // removed or written through.
        let renamed = self
            .0
            .create_new(temp, 0o600)
            .and_then(|mut file| {
                // Made 0600 whatever the umask.
                file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
                file.write_all(bytes)?;
                file.sync_all()
            })
            .and_then(|()| self.0.rename(temp, OsStr::new(name)))
            .and_then(|()| self.0.sync_all());
        if renamed.is_err() {
            let _ = self.0.remove_file(temp);
        }
        renamed
    }

    /// Makes file `name` in this folder the user's alone (0600), as
    /// [`Dir::replace`] writes it, when it's there, a regular file and its
    /// only name: one kept before, as the umask left it, is private from
    /// the first time it's opened, not the first time it's written. A link
    /// or anything else there is left as it is.
    pub fn make_private(&self, name: &str) -> io::Result<()> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let file = match self.0.open_file(OsStr::new(name), libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(err) if refused(&err) || err.kind() == io::ErrorKind::InvalidData => {
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        let meta = file.metadata()?;
        if meta.nlink() == 1 && meta.mode() & 0o777 != 0o600 {
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Holds this folder for this process alone until `f` returns: two
    /// servers over one folder take turns.
    pub fn locked<T>(&self, f: impl FnOnce() -> T) -> io::Result<T> {
        let _held = self.0.lock(libc::LOCK_EX)?;
        Ok(f())
    }
}

/// A link where a file or folder was (`O_NOFOLLOW`), something that isn't
/// a folder on the way, or a name gone since the walk.
fn refused(err: &io::Error) -> bool {
    nofollow::is_link(err)
        || matches!(
            err.raw_os_error(),
            Some(libc::ENOTDIR | libc::ENOENT | libc::ELOOP)
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn reads_only_regular_files_no_larger_than_asked() {
        let root = link_test_kit::TempDir::new("memory-folder");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/a.md"), "hello").unwrap();
        std::fs::write(root.join("big.md"), "x".repeat(11)).unwrap();
        std::os::unix::fs::symlink("/dev/zero", root.join("zero.md")).unwrap();
        std::os::unix::fs::symlink(root.join("notes"), root.join("linked")).unwrap();
        let fifo = CString::new(root.join("pipe.md").as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid C string.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);

        let dir = Dir::open(&root).unwrap();
        assert_eq!(dir.read("notes/a.md", 10).unwrap().unwrap(), b"hello");
        // Too large, a link to a device, a link on the way, a pipe (which
        // would block a read), a way out: none read, none waited on.
        for rel in [
            "big.md",
            "zero.md",
            "linked/a.md",
            "pipe.md",
            "../x",
            "/etc/passwd",
        ] {
            assert!(dir.read(rel, 10).unwrap().is_none(), "{rel}");
        }
        let kinds: Vec<(String, bool)> = dir
            .entries()
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.kind == Kind::File))
            .collect();
        assert!(kinds.contains(&("zero.md".into(), false)));
        assert!(kinds.contains(&("big.md".into(), true)));
        assert!(dir.folder("linked").is_err() && dir.folder("notes").is_ok());
        // Replaced whole, a link there replaced rather than followed.
        let notes = dir.folder("notes").unwrap();
        notes.replace("a.md", b"bye").unwrap();
        assert_eq!(std::fs::read(root.join("notes/a.md")).unwrap(), b"bye");
        std::os::unix::fs::symlink(root.join("big.md"), root.join("notes/b.md")).unwrap();
        notes
            .locked(|| notes.replace("b.md", b"new"))
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(root.join("big.md")).unwrap(), b"x".repeat(11));
        assert_eq!(std::fs::read(root.join("notes/b.md")).unwrap(), b"new");
        let left: Vec<String> = notes
            .entries()
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
    }
}
