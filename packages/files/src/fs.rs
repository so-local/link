//! Reading a file whose writer matters: once, where it really is, and no
//! more of it than a limit ([`read_trusted`]).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

mod strict;

/// Who a file [`read_trusted`] reads must have been written by.
#[derive(Clone, Copy, Debug, Default)]
pub struct Trust<'a> {
    /// The users who may own it, and with [`Trust::ancestors`] each folder
    /// on its way; empty, anyone.
    pub owners: &'a [u32],
    /// The mode bits none of them may have: `0o022`, written by no one but
    /// its owner.
    pub deny_mode: u32,
    /// Whether every folder on its way is held to the same, a folder its
    /// others write let by only with its sticky bit, as `/tmp`.
    pub ancestors: bool,
    /// Refuse a link at the file. With `ancestors`, use checked handles
    /// throughout and refuse hard links and unapproved linked ancestors.
    pub no_link: bool,
}

impl<'a> Trust<'a> {
    /// Control is owned by these users, never redirected through data.
    pub fn control(owners: &'a [u32]) -> Self {
        Self {
            owners,
            deny_mode: 0o022,
            ancestors: true,
            no_link: true,
        }
    }
    /// Written by `owners` alone, read and written by no one else
    /// (`0o077` denied), and read where it is, never through a link: a
    /// file of a service's own, such as one holding a password.
    pub fn alone(owners: &'a [u32]) -> Trust<'a> {
        Trust {
            owners,
            deny_mode: 0o077,
            ancestors: false,
            no_link: true,
        }
    }
}

/// Read control belonging to this user or root through checked handles.
pub fn read_control(path: &Path, max: u64, what: &str) -> anyhow::Result<Trusted> {
    // SAFETY: geteuid has no failure and touches no memory.
    let owners = [0, unsafe { libc::geteuid() }];
    read_trusted(path, Trust::control(&owners), max, what)
}

/// Missing control is accepted only beneath the same checked ancestors.
pub fn read_control_optional(path: &Path, max: u64, what: &str) -> anyhow::Result<Option<Trusted>> {
    // SAFETY: geteuid has no failure and touches no memory.
    let owners = [0, unsafe { libc::geteuid() }];
    let trust = Trust::control(&owners);
    match read_trusted(path, trust, max, what) {
        Ok(read) => Ok(Some(read)),
        Err(error) if missing(&error) => {
            check_folders(path, trust)?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Pin a checked parent for subsequent operations relative to its handle.
pub fn checked_parent(
    path: &Path,
    trust: Trust<'_>,
) -> anyhow::Result<(crate::nofollow::Dir, PathBuf)> {
    anyhow::ensure!(
        trust.ancestors && trust.no_link,
        "a checked parent refuses linked ancestors"
    );
    strict::parent(path, trust, false)?.context("a checked parent exists")
}

/// A folder on the way to a file [`read_trusted`] refused, with why: what
/// a caller tells a person, naming the folder rather than the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untrusted {
    /// The file it was reading.
    pub file: PathBuf,
    /// The folder above it that failed.
    pub folder: PathBuf,
    /// Who owns that folder, by user ID.
    pub owner: u32,
    pub why: Distrust,
}

/// Which of [`Trust`]'s rules a folder broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distrust {
    /// Owned by none of its trusted owners (root or you).
    Owner,
    /// Written by others than its owner, with no sticky bit.
    Written,
}

impl std::fmt::Display for Untrusted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let why = match self.why {
            Distrust::Owner => format!("is owned by user {}, neither root nor you", self.owner),
            Distrust::Written => "is written by others than its owner".to_string(),
        };
        write!(
            f,
            "{} isn't trusted: {}, a folder on its way, {why}",
            self.file.display(),
            self.folder.display()
        )
    }
}

impl std::error::Error for Untrusted {}

/// A file read whole.
#[derive(Debug)]
pub struct Trusted {
    /// Its real path.
    pub path: PathBuf,
    pub text: String,
}

/// The text of the file at `path`, at most `max` bytes (`what` names that
/// limit's owner in the error, `a spec's`), read where it really is: a
/// link is followed once, here, and the file opened with no link left
/// and checked to be the one its real path names, so a folder swapped
/// on the way opens nothing else; with [`Trust::no_link`], a link at
/// `path` is refused instead. It's opened without waiting, so what isn't
/// a file (a pipe put there) is refused, never waited on. Refused unless
/// it, and as `trust` says the folders on its way, are written only as
/// `trust` lets. A file that's missing is an error whose root cause is
/// that `io::Error`.
pub fn read_trusted(path: &Path, trust: Trust, max: u64, what: &str) -> anyhow::Result<Trusted> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let strict = trust.ancestors && trust.no_link;
    let (file, real) = if strict {
        strict::open(path, trust).with_context(|| format!("reading {}", path.display()))?
    } else {
        let real =
            std::fs::canonicalize(path).with_context(|| format!("reading {}", path.display()))?;
        if trust.no_link {
            let folder = match path.parent() {
                Some(folder) if !folder.as_os_str().is_empty() => folder,
                _ => Path::new("."),
            };
            let here = std::fs::canonicalize(folder)
                .with_context(|| format!("reading {}", path.display()))?
                .join(path.file_name().unwrap_or_default());
            if real != here {
                bail!(
                    "{} is a link: it's read where it is, never through one",
                    path.display()
                );
            }
        }
        if trust.ancestors {
            trusted_folders(path, &real, trust)?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&real)
            .with_context(|| format!("reading {}", real.display()))?;
        (file, real)
    };
    let owned = |uid: u32| trust.owners.is_empty() || trust.owners.contains(&uid);
    let meta = file
        .metadata()
        .with_context(|| format!("reading {}", real.display()))?;
    if !strict {
        let named =
            std::fs::metadata(&real).with_context(|| format!("reading {}", real.display()))?;
        if (named.dev(), named.ino()) != (meta.dev(), meta.ino()) {
            bail!("{} changed as it was read: read it again", real.display());
        }
    }
    if !meta.is_file() || !owned(meta.uid()) || meta.mode() & trust.deny_mode != 0 {
        bail!(
            "{} isn't a file written by its owners alone",
            path.display()
        );
    }
    if meta.len() > max {
        bail!(
            "{} is {} bytes, more than {what} {max}",
            real.display(),
            meta.len()
        );
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(file, max + 1), &mut text)
        .with_context(|| format!("reading {}", real.display()))?;
    if text.len() as u64 > max {
        bail!(
            "{} grew past {what} {max} bytes as it was read",
            real.display()
        );
    }
    Ok(Trusted { path: real, text })
}

/// `text`, the file at `path`, as TOML: an error says only where it's
/// wrong (the path and line), never what's there, which may be a secret
/// (a password in an address).
pub fn from_toml<T: serde::de::DeserializeOwned>(text: &str, path: &Path) -> anyhow::Result<T> {
    toml::from_str(text).map_err(|err| {
        let line = err
            .span()
            .map(|span| text[..span.start.min(text.len())].lines().count().max(1));
        match line {
            Some(line) => {
                anyhow::anyhow!("{} isn't as it should be at line {line}", path.display())
            }
            None => anyhow::anyhow!("{} isn't as it should be", path.display()),
        }
    })
}

/// An error unless every folder on the way to `path`, which needn't be
/// there, is written only as `trust` lets ([`Trust::ancestors`]), those
/// that are there: so a file missing there is missing because its owners
/// left it out, not because another removed it.
pub fn check_folders(path: &Path, trust: Trust) -> anyhow::Result<()> {
    if trust.ancestors && trust.no_link {
        return strict::folders(path, trust);
    }
    let there = path
        .ancestors()
        .skip(1)
        .find(|folder| folder.exists())
        .with_context(|| format!("no folder of {} is there", path.display()))?;
    let real =
        std::fs::canonicalize(there).with_context(|| format!("reading {}", there.display()))?;
    // As a file in it would be: its folder and every one above.
    trusted_folders(path, &real.join("file"), trust)
}

/// [`check_folders`] of `real`, the real path of `path` (or of a file in
/// its folder).
fn trusted_folders(path: &Path, real: &Path, trust: Trust) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let owned = |uid: u32| trust.owners.is_empty() || trust.owners.contains(&uid);
    for folder in real.ancestors().skip(1) {
        let meta = std::fs::symlink_metadata(folder)
            .with_context(|| format!("reading {}", folder.display()))?;
        let sticky = meta.mode() & 0o1000 != 0;
        if !owned(meta.uid()) || (meta.mode() & trust.deny_mode != 0 && !sticky) {
            bail!(
                "{} is in {}, which others than its owners write",
                path.display(),
                folder.display()
            );
        }
    }
    Ok(())
}

/// Whether `err`, from [`read_trusted`], is that its file is missing.
pub fn missing(err: &anyhow::Error) -> bool {
    err.root_cause()
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}

/// Takes `file`'s lock for writing (`flock`, `LOCK_EX`), waiting for it:
/// let go when it's closed.
#[cfg(unix)]
pub fn lock(file: &std::fs::File) -> std::io::Result<()> {
    lock_with(file, libc::LOCK_EX)
}

/// Try to hold a file exclusively without waiting for another transaction.
pub fn try_lock(file: &std::fs::File) -> std::io::Result<bool> {
    match lock_with(file, libc::LOCK_EX | libc::LOCK_NB) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(error),
    }
}

fn lock_with(file: &std::fs::File, how: libc::c_int) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    loop {
        // SAFETY: a file descriptor this process holds open.
        if unsafe { libc::flock(file.as_raw_fd(), how) } == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What it reads is the file's, whole, under the limit: a missing file
    /// says so, a longer one is refused, and one written by others is
    /// refused where the trust says no one else writes.
    #[test]
    fn a_trusted_read_is_the_files_own_under_its_limit() {
        use std::os::unix::fs::PermissionsExt;
        let temp = link_test_kit::TempDir::new("read-trusted");
        let path = temp.join("a.toml");
        let err = read_trusted(&path, Trust::default(), 8, "a test's").unwrap_err();
        assert!(missing(&err));
        std::fs::write(&path, "x = 1\n").unwrap();
        let read = read_trusted(&path, Trust::default(), 8, "a test's").unwrap();
        assert_eq!(read.text, "x = 1\n");
        assert_eq!(read.path, std::fs::canonicalize(&path).unwrap());
        let err = read_trusted(&path, Trust::default(), 4, "a test's").unwrap_err();
        assert!(
            format!("{err:#}").contains("more than a test's 4"),
            "{err:#}"
        );
        assert!(!missing(&err));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        let strict = Trust {
            deny_mode: 0o022,
            ..Trust::default()
        };
        assert!(read_trusted(&path, strict, 8, "a test's").is_err());
        assert!(read_trusted(&path, Trust::default(), 8, "a test's").is_ok());
    }

    /// With `no_link`, a link at its path is refused, and the file it
    /// names read where it is; a pipe put where a file is read is refused,
    /// never waited on.
    #[test]
    fn a_link_is_refused_where_asked_and_a_pipe_never_waited_on() {
        let temp = link_test_kit::TempDir::new("read-trusted-link");
        let path = temp.join("a.toml");
        std::fs::write(&path, "x = 1\n").unwrap();
        let link = temp.join("b.toml");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let own = Trust {
            no_link: true,
            ..Trust::default()
        };
        assert!(read_trusted(&link, Trust::default(), 8, "a test's").is_ok());
        let err = read_trusted(&link, own, 8, "a test's").unwrap_err();
        assert!(format!("{err:#}").contains("is a link"), "{err:#}");
        assert_eq!(
            read_trusted(&path, own, 8, "a test's").unwrap().text,
            "x = 1\n"
        );
        let pipe = temp.join("pipe.toml");
        let at = std::ffi::CString::new(pipe.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(at.as_ptr(), 0o600) }, 0);
        assert!(read_trusted(&pipe, Trust::default(), 8, "a test's").is_err());
    }

    /// A folder on the way to a control file that others write is named
    /// in the refusal, with its owner and why, typed for the caller to say.
    #[test]
    fn an_untrusted_folder_is_named_with_why() {
        use std::os::unix::fs::PermissionsExt;
        let temp = link_test_kit::TempDir::new("read-control-untrusted");
        let open = temp.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o577)).unwrap();
        let path = open.join("map.toml");
        let err = read_control_optional(&path, 64, "a test's").unwrap_err();
        let refused = err
            .chain()
            .find_map(|e| e.downcast_ref::<Untrusted>())
            .unwrap_or_else(|| panic!("not typed: {err:#}"));
        assert_eq!(refused.why, Distrust::Written);
        assert_eq!(refused.folder, std::fs::canonicalize(&open).unwrap());
        assert_eq!(refused.file, path);
        assert!(format!("{refused}").contains("is written by others than its owner"));
    }

    /// TOML that isn't as it should be is said by its path and line,
    /// never by what's there.
    #[test]
    fn toml_wrong_says_where_never_what() {
        #[derive(serde::Deserialize, Debug)]
        #[expect(
            dead_code,
            reason = "only deserialized, to show a wrong file's error says where, never what"
        )]
        struct Settings {
            database: String,
        }
        let text = "database = \"postgres://link_store:hunter2@db\"\nport = [\n";
        let err = from_toml::<Settings>(text, Path::new("/run/store.toml")).unwrap_err();
        let said = format!("{err:#}");
        assert!(!said.contains("hunter2"), "{said}");
        assert!(
            said.contains("/run/store.toml") && said.contains("line"),
            "{said}"
        );
        let typo = "database = 42\n";
        let said = format!(
            "{:#}",
            from_toml::<Settings>(typo, Path::new("s")).unwrap_err()
        );
        assert_eq!(said, "s isn't as it should be at line 1");
    }
}
