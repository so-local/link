//! A test's own folder, removed when the test ends, failed or not.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

#[cfg(unix)]
mod permissions;

/// A test's own folder, `<temp>/lnk-t<pid>-<n>-<name>`, made empty and
/// removed when dropped, so a test that fails or panics leaves nothing
/// behind. `n` counts the process's folders, so two tests that pick one
/// name, as a test program's modules can, never share a folder. `name`
/// says whose it is, and is short, as a socket's path under it has a
/// limit (about 100 bytes).
///
/// A test process that was killed never drops it: a test process's first
/// `TempDir` in a folder removes every `lnk-t<pid>-*` there whose process
/// is gone.
pub struct TempDir(PathBuf);

/// Whether `path` is in a folder the kit made for a test: one named
/// `lnk-t<pid>-…` ([`TempDir`]'s), or one in `LNK_TEST_STATE_DIR`.
pub fn in_test_folder(path: &Path) -> bool {
    let suite = std::env::var_os("LNK_TEST_STATE_DIR").map(PathBuf::from);
    suite.is_some_and(|suite| path.starts_with(suite))
        || path.components().any(|part| {
            part.as_os_str()
                .to_str()
                .and_then(|name| name.strip_prefix("lnk-t"))
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        })
}

/// Where [`TempDir::in_memory`] makes its folders, when it's there.
const MEMORY: &str = "/dev/shm";

/// The longest path [`TempDir::short`] makes, in bytes.
pub const SHORT: usize = 44;

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        match std::env::var_os("LNK_TEST_STATE_DIR").map(PathBuf::from) {
            Some(root) => Self::in_suite(&root),
            None => TempDir::under(&real_temp(), name),
        }
    }

    fn in_suite(root: &Path) -> TempDir {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::fs::create_dir_all(root).unwrap();
        loop {
            let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = root.join(format!("t{:x}-{next:x}", std::process::id()));
            match Self::try_create(dir) {
                Ok(dir) => return dir,
                // Refuse an existing folder even if a process ID was reused.
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("making a test's suite folder: {err}"),
            }
        }
    }

    /// One under `/tmp` on Unix, whatever the temp folder: a Mac's is long
    /// enough that a socket's path under it passes the limit. Its path is
    /// at most [`SHORT`] bytes, so a socket's under it has 60 of the 104
    /// a socket's path may have (`sun_path`, macOS's, Linux's 108): a
    /// `name` that would make it longer fails the test.
    pub fn short(name: &str) -> TempDir {
        Self::short_with_limit(name, SHORT)
    }

    /// A short folder with a smaller path budget, such as an SSH home
    /// whose control socket adds its own directories and temporary suffix.
    /// A suite folder that exceeds that budget falls back to `/tmp`.
    pub fn short_with_limit(name: &str, limit: usize) -> TempDir {
        let suite = std::env::var_os("LNK_TEST_STATE_DIR")
            .map(PathBuf::from)
            .filter(|root| root.as_os_str().len() < 32)
            .map(|root| Self::in_suite(&root))
            .filter(|dir| dir.as_os_str().len() <= limit);
        let dir = match suite {
            Some(dir) => dir,
            _ if cfg!(unix) => TempDir::under(Path::new("/tmp"), name),
            _ => TempDir::new(name),
        };
        assert!(
            dir.0.as_os_str().len() <= limit,
            "{} is longer than a short folder's {limit} bytes: name it more briefly than {name:?}",
            dir.0.display()
        );
        dir
    }

    /// A short one out of the temp folders, for a fixture that a sandbox
    /// reads and that holds sockets ([`out_of_tmp`]): in a short
    /// `LNK_TEST_STATE_DIR`, else in this user's cache folder
    /// (`XDG_CACHE_HOME`, else the account's own `~/.cache`, whatever
    /// home the program was [given](crate::home)), which a checkout's
    /// `CARGO_TARGET_TMPDIR` can be too long to stand in for. Where
    /// neither is short, the one [`TempDir::short`] makes, which
    /// [`out_of_tmp`] then says is in `/tmp`.
    ///
    /// [`out_of_tmp`]: crate::gate::out_of_tmp
    pub fn short_out_of_tmp(name: &str) -> TempDir {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| crate::home::account().map(|home| home.join(".cache")));
        match std::env::var_os("LNK_TEST_STATE_DIR") {
            Some(_) => Self::short(name),
            None => match short_parent(cache, name) {
                Some(cache) => TempDir::under(&cache, name),
                None => Self::short(name),
            },
        }
    }

    /// One in memory where the machine has a memory file system
    /// (Linux's `/dev/shm`), for a test that makes thousands of durable
    /// writes: there a sync returns at once rather than waiting on a
    /// disk. Elsewhere, or with `LNK_TEST_STATE_DIR` set, the one
    /// [`TempDir::new`] makes.
    pub fn in_memory(name: &str) -> TempDir {
        let memory = Path::new(MEMORY);
        if std::env::var_os("LNK_TEST_STATE_DIR").is_none() && memory.is_dir() {
            TempDir::under(memory, name)
        } else {
            TempDir::new(name)
        }
    }

    /// One in `parent`, such as `CARGO_TARGET_TMPDIR` for a test kept out
    /// of `/tmp` ([`out_of_tmp`]).
    pub fn under(parent: &Path, name: &str) -> TempDir {
        static SWEPT: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
        let mut swept = SWEPT.lock().unwrap_or_else(|e| e.into_inner());
        if !swept.iter().any(|done| done == parent) {
            remove_the_gone(parent);
            swept.push(parent.to_path_buf());
        }
        drop(swept);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = parent.join(format!("lnk-t{}-{n:x}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(parent);
        Self::create(dir)
    }

    fn create(dir: PathBuf) -> TempDir {
        // Made new, and only this user's: a folder another user put at
        // this name first, as one in a shared `/tmp` could, is refused.
        Self::try_create(dir.clone())
            .unwrap_or_else(|err| panic!("making a test's temp folder, {}: {err}", dir.display()))
    }

    fn try_create(dir: PathBuf) -> std::io::Result<TempDir> {
        let mut made = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut made, 0o700);
        made.create(&dir)?;
        Ok(TempDir(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    #[expect(
        clippy::disallowed_methods,
        reason = "the kit's own: a folder still written to is removed again a moment later"
    )]
    fn drop(&mut self) {
        // Again, a moment later, while what the test started still writes
        // in it (a turn that ends after its test): the last file written
        // keeps the folder from going.
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<OsStr> for TempDir {
    fn as_ref(&self) -> &OsStr {
        self.0.as_os_str()
    }
}

/// The temp folder by its real path. A Mac's goes through `/var`, a link
/// to `/private/var`: SQLite refuses a path through a link, and a path
/// that code resolves stops matching the one a test kept.
fn real_temp() -> PathBuf {
    let temp = std::env::temp_dir();
    temp.canonicalize().unwrap_or(temp)
}

/// `parent`, when absolute and short enough that `name`'s folder in it
/// ([`TempDir::under`]'s `lnk-t<pid>-<n>-<name>`) is a short folder.
fn short_parent(parent: Option<PathBuf>, name: &str) -> Option<PathBuf> {
    // `lnk-t`, a pid's 7 digits at most, a count and two dashes.
    const NAMING: usize = 20;
    parent.filter(|p| p.is_absolute() && p.as_os_str().len() + NAMING + name.len() <= SHORT)
}

/// Removes the folders of test processes that are gone: killed, or the
/// machine restarted under them.
fn remove_the_gone(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name
            .to_str()
            .and_then(|name| name.strip_prefix("lnk-t")?.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<u32>().ok());
        if pid.is_some_and(|pid| pid != std::process::id() && gone(pid)) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Whether no process `pid` runs. Signal 0 sends nothing; it only asks.
#[cfg(unix)]
fn gone(pid: u32) -> bool {
    // Only an id that names one process: never 0, 1 or a group (negative).
    let Ok(id) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if id <= 1 {
        return false;
    }
    // SAFETY: signal 0 to one process, which only checks it exists.
    let refused = unsafe { libc::kill(id, 0) } != 0;
    refused && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(windows)]
compile_error!(
    "link-test-kit: no Windows implementation of temp::gone (whether a test process is gone), which EO1 adds"
);

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_fixture_directory_owns_its_permission_modes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("directory-mode");
        dir.set_mode(0o777).unwrap();
        assert_eq!(dir.metadata().unwrap().permissions().mode() & 0o777, 0o777);
        dir.set_mode(0o700).unwrap();
        assert_eq!(dir.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    }

    /// A test's folder goes through no link, so what it opens and what
    /// code resolves name one path.
    #[test]
    fn a_folder_in_the_temp_folder_has_its_real_path() {
        let dir = TempDir::under(&real_temp(), "real");
        assert_eq!(dir.canonicalize().unwrap(), dir.to_path_buf());
    }

    /// A short folder leaves a socket's path room, and a long name is
    /// refused rather than left to fail where a socket is bound.
    #[test]
    fn a_short_folder_is_short() {
        let dir = TempDir::short("file-bridged-pages");
        assert!(dir.as_os_str().len() <= SHORT, "{}", dir.display());
        let long = std::panic::catch_unwind(|| TempDir::short(&"x".repeat(40)));
        // Suite state uses a compact process ID and counter, not the label.
        // A long label is valid there; outside it, it must still be refused.
        let suite = std::env::var_os("LNK_TEST_STATE_DIR").is_some_and(|root| root.len() < 32);
        assert_eq!(long.is_ok(), suite);
    }

    #[test]
    fn a_socket_home_obeys_its_smaller_path_budget() {
        let dir = TempDir::short_with_limit("", 23);
        assert!(dir.join(".lnk/safe/boxes/mux").as_os_str().len() + 1 + 40 + 17 <= 100);
    }

    /// A folder out of `/tmp` is made in the cache folder only where a
    /// socket's path under it still has room; a long or relative one is
    /// passed over, never used to make a folder too long.
    #[test]
    fn a_short_folder_out_of_tmp_is_made_only_where_it_fits() {
        let cache = || Some(PathBuf::from("/home/me/.cache"));
        assert_eq!(short_parent(cache(), "runtime"), cache());
        let long = Some(PathBuf::from(format!("/home/{}/.cache", "x".repeat(20))));
        assert_eq!(short_parent(long, "runtime"), None);
        assert_eq!(short_parent(Some(".cache".into()), "runtime"), None);
        assert_eq!(short_parent(None, "runtime"), None);
        // What it makes there is a short folder, at the longest pid.
        let made = format!("/home/me/.cache/lnk-t{}-ff-runtime", "9".repeat(7));
        assert!(made.len() <= SHORT, "{made}");
    }

    #[test]
    fn a_temp_folder_goes_when_dropped_or_its_process_is_gone() {
        let dir = TempDir::new("guard");
        assert!(dir.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::write(dir.join("file"), "x").unwrap();
        let path = dir.to_path_buf();
        drop(dir);
        assert!(!path.exists());

        // A process that's gone, as a killed test's, and one that runs.
        let mut finished = link_secretive::process::command("true").spawn().unwrap();
        finished.wait().unwrap();
        let mut running = link_secretive::process::command("sleep")
            .arg("300")
            .spawn()
            .unwrap();
        let of = |pid: u32| std::env::temp_dir().join(format!("lnk-t{pid}-left"));
        for pid in [finished.id(), running.id()] {
            std::fs::create_dir_all(of(pid).join("home")).unwrap();
        }
        remove_the_gone(&std::env::temp_dir());
        let (gone, kept) = (of(finished.id()).exists(), of(running.id()).exists());
        let _ = running.kill();
        let _ = running.wait();
        let _ = std::fs::remove_dir_all(of(running.id()));
        assert!(!gone, "a gone process's folder is removed");
        assert!(kept, "a running process's folder stays");
    }
}

#[cfg(test)]
mod suite_paths {
    use super::TempDir;

    #[cfg(unix)]
    #[test]
    fn different_processes_do_not_reuse_retired_suite_folders() {
        if let Some(report) = std::env::var_os("LNK_TEST_SUITE_PATH_REPORT") {
            let dir = TempDir::new("child");
            std::fs::write(report, dir.to_str().unwrap()).unwrap();
            return;
        }
        let root = TempDir::short("suite-pids");
        let mut paths = Vec::new();
        for name in ["first", "second"] {
            let report = root.join(name);
            let mut command = crate::env::command(std::env::current_exe().unwrap(), &root);
            command
                .args([
                    "--exact",
                    "temp::suite_paths::different_processes_do_not_reuse_retired_suite_folders",
                ])
                .env("LNK_TEST_STATE_DIR", root.join("suite"))
                .env("LNK_TEST_SUITE_PATH_REPORT", &report);
            let child = crate::process::ChildGroup::spawn(&mut command).unwrap();
            let status = crate::wait::eventually(
                "the suite folder's child to finish",
                std::time::Duration::from_secs(30),
                || child.try_status().unwrap(),
            );
            assert!(status.success(), "{status}");
            let path = std::fs::read_to_string(report).unwrap();
            assert!(!std::path::Path::new(&path).exists());
            paths.push(path);
        }
        assert_ne!(
            paths[0], paths[1],
            "a later process reused a retired folder"
        );
    }

    #[test]
    fn suite_state_is_short_private_and_independent() {
        let root = TempDir::under(std::path::Path::new("/tmp"), "s");
        let first = TempDir::in_suite(&root);
        let second = TempDir::in_suite(&root);
        assert_ne!(first.path(), second.path());
        assert!(first.path().starts_with(root.path()));
        assert!(first.path().as_os_str().len() <= super::SHORT);
        assert!(TempDir::try_create(first.path().to_path_buf()).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(first.path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
}
