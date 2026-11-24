//! A folder held open, and the writes made in it.

use super::When;
use super::*;

/// A folder held open ([`nofollow::Dir`]), and where it is, to say what's
/// read or written: each name in it opened from its handle, never through
/// a link, so a folder on the way swapped for one after it was opened (by
/// a `run` call) changes nothing read or written through it.
pub(super) struct Held {
    pub(super) dir: Dir,
    pub(super) path: PathBuf,
}

impl Held {
    /// The folder at `path`, of Link's own (a harness's home, an agent's
    /// folder, its Home): a link there is followed, once, here. None when
    /// it isn't there, unless `make`: then it's made first (700).
    pub(super) fn root(path: &Path, make: bool) -> std::io::Result<Option<Held>> {
        if make {
            std::os::unix::fs::DirBuilderExt::mode(
                std::fs::DirBuilder::new().recursive(true),
                0o700,
            )
            .create(path)?;
        }
        found(Dir::open(path)).map(|dir| {
            dir.map(|dir| Held {
                dir,
                path: path.to_path_buf(),
            })
        })
    }

    /// The folder at `path`, whose last part is never a link (refused,
    /// and said so), the folders above it followed as [`Held::root`]'s
    /// are: none when it isn't there, unless `make`: then made, 700.
    /// Opened for traversal, without listing this parent's entries on
    /// macOS: a sandbox may grant its children without granting the parent.
    pub(super) fn unlinked(path: &Path, make: bool) -> std::io::Result<Option<Held>> {
        let opened = match Dir::search_unlinked(path) {
            Err(err) if make && err.kind() == std::io::ErrorKind::NotFound => {
                std::os::unix::fs::DirBuilderExt::mode(
                    std::fs::DirBuilder::new().recursive(true),
                    0o700,
                )
                .create(path)?;
                Dir::search_unlinked(path)
            }
            opened => opened,
        }
        .map_err(|err| {
            let link = nofollow::is_link(&err)
                || std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
            refused_link(err, link, path)
        });
        found(opened).map(|dir| {
            dir.map(|dir| Held {
                dir,
                path: path.to_path_buf(),
            })
        })
    }

    /// The folder `name` in this one, never through a link (refused, and
    /// said so): none when it isn't there, unless `make`: then made, 700.
    pub(super) fn folder(&self, name: &str, make: bool) -> std::io::Result<Option<Held>> {
        let path = self.at(name);
        let opened = match make {
            true => self.dir.create_folder(OsStr::new(name), 0o700),
            false => self.dir.folder(OsStr::new(name)),
        }
        .map_err(|err| {
            let link = nofollow::is_link(&err)
                || self
                    .dir
                    .kind(OsStr::new(name))
                    .is_ok_and(|kind| kind == Kind::Link);
            refused_link(err, link, &path)
        });
        found(opened).map(|dir| dir.map(|dir| Held { dir, path }))
    }

    /// `name` in this folder, to say what's read or written.
    pub(super) fn at(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// The regular file `name` here, opened with `flags` (and `mode`,
    /// made with `O_CREAT`), as [`Dir::open_file`] opens one: a link
    /// there is refused, and said so.
    pub(super) fn open(
        &self,
        name: &str,
        flags: libc::c_int,
        mode: u32,
    ) -> std::io::Result<std::fs::File> {
        self.dir
            .open_file(OsStr::new(name), flags, mode)
            .map_err(|err| match nofollow::is_link(&err) {
                true => std::io::Error::other("a link: not following it"),
                false => err,
            })
    }

    /// The regular file `name` here, whole: an error when it holds more
    /// than `max` bytes, however it grows meanwhile.
    pub(super) fn read(&self, name: &str, max: u64) -> std::io::Result<Vec<u8>> {
        read_open(self.open(name, libc::O_RDONLY, 0)?, max)
    }

    /// Writes `name` with `bytes`, whole or not at all: written aside
    /// under a name of its own, then renamed in place, replacing whatever
    /// is there, a link included, never written through. Readable only by
    /// the user.
    pub(super) fn write(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        let aside = aside(name);
        let written = self
            .dir
            .create_new(OsStr::new(&aside), 0o600)
            .and_then(|mut file| file.write_all(bytes))
            .and_then(|()| self.dir.rename(OsStr::new(&aside), OsStr::new(name)));
        if written.is_err() {
            let _ = self.dir.remove_file(OsStr::new(&aside));
        }
        written
    }

    /// Writes `name` with `bytes` only if nothing is there: a taken number
    /// is refused, never overwritten. Readable only by the user; whole or
    /// not at all (written aside, then linked in place).
    pub(super) fn write_new(&self, name: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let aside = aside(name);
        let linked = self
            .dir
            .create_new(OsStr::new(&aside), 0o600)
            .and_then(|mut file| file.write_all(bytes))
            .and_then(|()| self.dir.link(OsStr::new(&aside), OsStr::new(name)));
        let _ = self.dir.remove_file(OsStr::new(&aside));
        match linked {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!(
                    "{} is written already: another turn came first",
                    self.at(name).display()
                )
            }
            Err(err) => Err(err).within(format!("writing {}", self.at(name).display())),
        }
    }

    /// Removes the file `name` here (a link itself, never what it names).
    pub(super) fn remove(&self, name: &str) -> std::io::Result<()> {
        self.dir.remove_file(OsStr::new(name))
    }

    /// Renames `from` to `to`, both here, in one step.
    pub(super) fn rename(&self, from: &str, to: &str) -> std::io::Result<()> {
        self.dir.rename(OsStr::new(from), OsStr::new(to))
    }

    /// What `name` here is, as it is: none when nothing is there.
    pub(super) fn kind(&self, name: &str) -> Option<Kind> {
        self.dir.kind(OsStr::new(name)).ok()
    }

    /// When `name` here was last written, as it is.
    pub(super) fn modified(&self, name: &str) -> Option<std::time::SystemTime> {
        let stat = self.dir.stat(OsStr::new(name)).ok()?;
        let ns = u64::try_from(stat.modified_ns).ok()?;
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(ns))
    }

    /// The names here that are text, sorted.
    pub(super) fn names(&self) -> std::io::Result<Vec<String>> {
        Ok(self
            .dir
            .names()?
            .into_iter()
            .filter_map(|name| name.into_string().ok())
            .collect())
    }

    /// Only the user's, as a folder this process made: another user's
    /// refused, and one others may read made 700.
    pub(super) fn private(&self) -> anyhow::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let meta = self.dir.metadata()?;
        // SAFETY: geteuid can't fail.
        if meta.uid() != unsafe { libc::geteuid() } {
            bail!(
                "{} belongs to another user; not using it",
                self.path.display()
            );
        }
        if meta.mode() & 0o077 != 0 {
            self.dir.set_mode(0o700)?;
        }
        Ok(())
    }
}

/// `err`, opening the folder at `path`, said as a link refused when
/// `link`.
fn refused_link(err: std::io::Error, link: bool, path: &Path) -> std::io::Error {
    match link {
        true => std::io::Error::other(format!("{} is a link: not following it", path.display())),
        false => err,
    }
}

/// What was there, `None` when nothing was.
fn found<T>(done: std::io::Result<T>) -> std::io::Result<Option<T>> {
    match done {
        Ok(it) => Ok(Some(it)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// A name of its own beside `name`, for a write made aside: two writers
/// of one process at once would otherwise take each other's.
fn aside(_name: &str) -> String {
    link_files::layout::temporary_name()
}

/// The folder `path` is in, and its name there.
pub(super) fn parted(path: &Path) -> anyhow::Result<(&Path, &str)> {
    match (path.parent(), path.file_name().and_then(OsStr::to_str)) {
        (Some(folder), Some(name)) => Ok((folder, name)),
        _ => bail!("{} isn't a file's path", path.display()),
    }
}

/// Writes the file at `path`, in a folder of Link's own (a harness's
/// home), made if it isn't there: whole, as [`Held::write`] does.
pub(super) fn write_in(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let writing = || format!("writing {}", path.display());
    let (folder, name) = parted(path)?;
    let folder = Held::root(folder, true)
        .within(When(&writing))?
        .within(When(&writing))?;
    folder.write(name, bytes).within(When(&writing))
}

/// Whether `err`, opening a file of the agent's, is one left out rather
/// than failing the turn: a link, a folder or a pipe where it goes.
pub(super) fn left_out(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::Other | std::io::ErrorKind::InvalidData
    ) || err.raw_os_error() == Some(libc::ENOTDIR)
}
