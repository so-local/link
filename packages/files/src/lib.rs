//! Files Link keeps on this machine: its folder, and reading and writing
//! them so no other user can swap, read or change what's in them.

pub mod fs;
pub mod layout;
pub mod maintenance;
pub mod nofollow;
pub mod reading;
pub mod saved;
pub mod schema;
pub mod settings;
pub mod state;
mod system_dirs;
pub mod upgrade;

use std::path::PathBuf;

/// 32 random hex characters, for tokens and secrets Link makes (a
/// harness's local API key, a pairing code, a proxy's tag).
pub fn random_token() -> std::io::Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The user's home folder. Without an absolute `HOME` this is `.`: code
/// that deletes or trusts what's there uses [`try_home_dir`] instead.
pub fn home_dir() -> PathBuf {
    try_home_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// `HOME`, through no links, or an error when it is unset, empty or
/// relative. A home that is a link (`/tmp/me` to elsewhere) isn't there
/// inside a sandbox, which binds where it leads and has a `/tmp` of its
/// own, so every path under it is the one it leads to.
pub fn try_home_dir() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.canonicalize().unwrap_or(home))
        .ok_or_else(|| anyhow::anyhow!("HOME is not set to an absolute path"))
}

/// `~/.lnk`: this machine's folder. With no absolute `HOME`, this is a
/// relative `./.lnk`: code that deletes or trusts the folder uses
/// [`try_installation_dir`] instead.
pub fn installation_dir() -> PathBuf {
    try_installation_dir().unwrap_or_else(|_| PathBuf::from(&layout::LAYOUT.roots.hidden))
}

/// [`installation_dir`], or an error when it can't be an absolute path (no
/// `HOME`). [`INSTALLATION_DIR_VAR`], when absolute, puts it elsewhere for one
/// process and what it runs: a sandbox's session whose own settings
/// can't be in a folder it writes.
pub fn try_installation_dir() -> anyhow::Result<PathBuf> {
    match std::env::var_os(INSTALLATION_DIR_VAR)
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
    {
        Some(dir) => Ok(dir),
        None => Ok(try_home_dir()?.join(&layout::LAYOUT.roots.hidden)),
    }
}

/// The human-facing folder, found through the machine's bootstrap locator.
pub fn try_link_dir() -> anyhow::Result<PathBuf> {
    layout::link_in(&try_home_dir()?, &try_installation_dir()?)
}

/// The variable that puts the installation folder elsewhere ([`try_installation_dir`]).
pub const INSTALLATION_DIR_VAR: &str = "LNK_INSTALLATION_DIR";

/// Creates the folder `path` readable only by this user (0700 on Unix),
/// with any missing private parents (also 0700). Shared system roots
/// retain their catalogue mode and cannot themselves be made private.
/// If it already exists with
/// permissions for others, they are taken away. A folder, or a link to
/// one, that another user owns is refused: in a shared folder such as
/// `/tmp` they could have made it first, to swap what's put in it.
pub fn create_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    if system_dirs::shared(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a private system directory belongs below its shared root",
        ));
    }
    create_dirs_private(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Checked, and its permissions set, on one handle: a folder
        // swapped in after the check is never the one changed.
        let dir = nofollow::Dir::open(path)?;
        let meta = dir.metadata()?;
        check_own(path, &meta)?;
        if meta.permissions().mode() & 0o077 != 0 {
            dir.set_mode(0o700)?;
        }
    }
    Ok(())
}

/// Refuses folder `path` unless it's this user's, itself and not through
/// a link to another's.
#[cfg(unix)]
pub fn check_own_dir(path: &std::path::Path) -> std::io::Result<()> {
    check_own(path, &std::fs::metadata(path)?)
}

/// Refuses `path`, what it reaches read as `meta`, unless both it and
/// what it reaches are this user's.
#[cfg(unix)]
fn check_own(path: &std::path::Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid can't fail.
    let me = unsafe { libc::geteuid() };
    let link = std::fs::symlink_metadata(path)?;
    if link.uid() != me || meta.uid() != me {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} belongs to another user; not using it", path.display()),
        ));
    }
    Ok(())
}

/// Holds the lock file `path` until the returned file is dropped: the
/// next to ask waits. Its folder is made private ([`create_private_dir`],
/// another user's refused), and the file is opened in place, never
/// replaced and never through a link (`O_NOFOLLOW`), readable by this
/// user alone (0600), so every holder locks the same file.
pub fn lock_file(path: &std::path::Path) -> anyhow::Result<std::fs::File> {
    use anyhow::Context;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        create_private_dir(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    let file = options
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    fs::lock(&file).with_context(|| format!("locking {}", path.display()))?;
    Ok(file)
}

/// A new folder in the system's temporary folder, `lnk-<name>-` and a
/// random suffix, that only this user can use (0700), made here and now
/// (as `mkdtemp`): never one another user made first.
pub fn private_temp_dir(name: &str) -> std::io::Result<PathBuf> {
    loop {
        let dir = std::env::temp_dir().join(format!("lnk-{name}-{}", random_token()?));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
}

/// `create_dir_all`, where the folders it creates are 0700 on Unix.
fn create_dirs_private(path: &std::path::Path) -> std::io::Result<()> {
    system_dirs::prepare(path)?;
    #[cfg(unix)]
    layout::guard_runtime(path)?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// Writes `contents` to `path` readable only by this user (it may hold a
/// secret), atomically, creating the folder (0700) if it is missing. An
/// existing installation folder ([`installation_dir`], `~/.lnk`) is made private too.
/// Two writes at once each leave the file whole, the last one's; a
/// caller that reads, changes and writes it back holds a lock of its own.
/// Success follows syncing the staged file, renaming and syncing its parent.
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        if let Ok(hidden) = try_installation_dir()
            && dir.starts_with(&hidden)
        {
            create_private_dir(&hidden)?;
        }
        create_dirs_private(dir)?;
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let dir = nofollow::Dir::open(parent)?;
    let tmp = std::ffi::OsString::from(layout::temporary_name());
    let written = (|| {
        let mut staged = dir.create_new(&tmp, 0o600)?;
        // Lock this transaction, independently of any caller's directory lock.
        fs::lock(&staged)?;
        std::io::Write::write_all(&mut staged, contents)?;
        staged.sync_all()?;
        dir.rename(&tmp, name)?;
        dir.sync_all()
    })();
    if written.is_err() {
        let _ = dir.remove_file(&tmp);
    }
    written
}

/// Writes `contents` to `path` as a program (mode 755), never open for
/// writing in this process. A program this process wrote itself could
/// fail to run ("Text file busy", ETXTBSY) while any child forked by
/// another thread in the meantime still held the write open, before its
/// own exec closed it; so a short-lived `install` writes it, from a copy
/// this process wrote and never runs.
pub fn write_program(path: &std::path::Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    let staged = path.with_file_name(layout::temporary_name());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = std::io::Write::write_all(&mut options.open(&staged)?, contents.as_ref());
    if let Err(err) = written {
        let _ = std::fs::remove_file(&staged);
        return Err(err);
    }
    let installed = copy_program(&staged, path);
    let _ = std::fs::remove_file(&staged);
    installed
}

/// Copies the program `from` to `to` (mode 755), never open for writing in
/// this process ([`write_program`]).
pub fn copy_program(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let status = link_secretive::process::command("install")
        .args(["-m", "755"])
        .arg(from)
        .arg(to)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "install {} {}: {status}",
            from.display(),
            to.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod lock_tests {
    use super::*;

    /// One lock file, held by one at a time, never through a link.
    #[test]
    fn a_lock_file_is_held_by_one_and_never_followed() {
        let dir = private_temp_dir("lock").unwrap();
        let path = dir.join("a").join("x.lock");
        let held = lock_file(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        // Another waits while it's held.
        let (tx, rx) = std::sync::mpsc::channel();
        let other = path.clone();
        let waiter = std::thread::spawn(move || {
            let _l = lock_file(&other).unwrap();
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200))
                .is_err()
        );
        drop(held);
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        waiter.join().unwrap();
        // A link in its place is refused.
        let link = dir.join("a").join("y.lock");
        std::os::unix::fs::symlink(dir.join("elsewhere"), &link).unwrap();
        assert!(lock_file(&link).is_err());
        assert!(!dir.join("elsewhere").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

pub mod pattern;
pub mod relative;
pub use relative::Relative;

/// bubblewrap, which makes the sandbox on Linux, where the system's
/// package puts it: never one found on `PATH`, which another program may
/// have put first. Every check of it and every sandbox run the same one.
pub fn bubblewrap() -> Option<&'static str> {
    ["/usr/bin/bwrap", "/bin/bwrap"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
}

/// Program `name` on `path`, a PATH value: the first that is a file, in
/// its absolute folders only.
pub fn find_program(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    path_dirs(path)
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// The folders in a PATH value, without empty or relative entries: those
/// mean the current folder, where anyone could have put an `lnk-<module>`.
pub fn path_dirs(path: &std::ffi::OsStr) -> Vec<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .collect()
}

/// `path`, a PATH value, without its empty and relative entries, which
/// mean the current folder: `lnk` runs with this, so neither it nor a
/// module runs a `tar` or `id` from wherever the user happens to be.
pub fn absolute_path(path: &std::ffi::OsStr) -> std::ffi::OsString {
    std::env::join_paths(path_dirs(path)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two `lnk` commands saving one file at once (two concurrent commands
    /// pinning host keys): each write lands whole, and none fails.
    #[test]
    fn writes_of_one_file_at_once_all_land() {
        let dir = link_test_kit::TempDir::new("writes");
        let file = dir.join("known_hosts");
        std::thread::scope(|scope| {
            for writer in 0..8 {
                let file = &file;
                scope.spawn(move || {
                    for n in 0..50 {
                        let text = format!("writer {writer}, write {n}\n");
                        write_private(file, text.as_bytes()).unwrap();
                    }
                });
            }
        });
        let last = std::fs::read_to_string(&file).unwrap();
        assert!(
            last.starts_with("writer ") && last.ends_with('\n'),
            "{last:?}"
        );
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "temporary files left: {left:?}");
    }

    /// A folder another user made first, in a shared folder such as
    /// `/tmp`, or a link they made to one: refused, never used.
    #[cfg(unix)]
    #[test]
    fn a_private_dir_another_user_owns_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let root = link_test_kit::TempDir::new("theirs");
        // SAFETY: geteuid can't fail.
        let theirs = match unsafe { libc::geteuid() } {
            // As root, a folder of nobody's; else one of root's.
            0 => {
                let dir = root.join("theirs");
                std::fs::create_dir(&dir).unwrap();
                std::os::unix::fs::chown(&dir, Some(65534), Some(65534)).unwrap();
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
                dir
            }
            _ => PathBuf::from("/"),
        };
        let err = create_private_dir(&theirs).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        let link = root.join("link");
        std::os::unix::fs::symlink(&theirs, &link).unwrap();
        assert!(create_private_dir(&link).is_err());
    }

    /// Each a new folder, 0700, never one already there.
    #[cfg(unix)]
    #[test]
    fn private_temp_dirs_are_new() {
        use std::os::unix::fs::PermissionsExt;
        let (a, b) = (
            private_temp_dir("t").unwrap(),
            private_temp_dir("t").unwrap(),
        );
        assert_ne!(a, b);
        for dir in [&a, &b] {
            let meta = std::fs::symlink_metadata(dir).unwrap();
            assert!(meta.is_dir());
            assert_eq!(meta.permissions().mode() & 0o777, 0o700);
            std::fs::remove_dir(dir).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn private_dirs_are_0700() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let root = link_test_kit::TempDir::new("private");
        let dir = root.join("a/b");
        create_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&root.join("a")), 0o700);
        // An existing, looser one is tightened.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        // write_private creates its missing folder private.
        write_private(&root.join("c/secret"), b"x").unwrap();
        assert_eq!(mode(&root.join("c")), 0o700);
        assert_eq!(mode(&root.join("c/secret")), 0o600);
    }

    /// A program written while other threads fork runs at once: never
    /// "Text file busy" from a child that inherited it open for writing.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_program_written_while_others_fork_runs_at_once() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = link_test_kit::TempDir::new("write-program");
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let forking: Vec<_> = (0..8)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = std::process::Command::new("true").status();
                    }
                })
            })
            .collect();
        let ran: Vec<std::io::Result<std::process::ExitStatus>> = (0..300)
            .map(|i| {
                let program = dir.join(format!("p{i}"));
                write_program(&program, "#!/bin/sh\nexit 0\n")?;
                std::process::Command::new(&program).status()
            })
            .collect();
        stop.store(true, Ordering::Relaxed);
        for thread in forking {
            thread.join().unwrap();
        }
        for (i, ran) in ran.into_iter().enumerate() {
            assert!(ran.as_ref().is_ok_and(|s| s.success()), "run {i}: {ran:?}");
        }
    }
}

#[cfg(test)]
link_test_kit::program!(system);
