//! What the sandbox asked of this machine, kept in Link's settings for
//! the sandbox so each run doesn't ask again: each wrap is a new process.
//! One file holds every answer ([`FILE`]), each by its name: whether
//! bubblewrap can refuse user namespaces inside, and the checks that
//! passed (that bubblewrap makes a sandbox here, that a wrap's
//! requirements hold).
//!
//! An answer holds only for the machine as it was asked ([`machine`]):
//! this boot, kernel and user, the user namespaces the kernel allows,
//! the bubblewrap and the `lnk-sandbox` that asked. Anything else, or a
//! file Link couldn't have written for this user ([`read_trusted`]),
//! and it asks again. Only a check that passed is kept: one that failed
//! asks every time, so nothing kept ever stops a sandbox, or opens one.

use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Where the answers are kept, in Link's settings for the sandbox: the
/// machine they hold for, then a line for each, its name, a tab and the
/// answer.
pub const FILE: &str = "bwrap-version";

/// What [`checked`] keeps for a check that passed.
const PASSED: &str = "passed";

/// The most a kept file holds: a longer one is none of Link's.
const MOST: u64 = 16 * 1024;

/// `check`, unless it passed before on this machine as it is now
/// ([`machine`], with bubblewrap at `bwrap`), kept as `probe`; kept once
/// it passes. Its error, as it was, whenever it runs and fails.
pub fn checked<E>(
    bwrap: &str,
    probe: &str,
    check: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    match (file(), machine(bwrap)) {
        (Some(file), Some(machine)) => checked_in(&file, &machine, probe, check),
        _ => check(),
    }
}

/// The answer kept as `name` on this machine as it is now, with
/// bubblewrap at `bwrap`.
pub fn kept(bwrap: &str, name: &str) -> Option<String> {
    kept_in(&file()?, &machine(bwrap)?, name)
}

/// Keeps `answer` as `name` on this machine as it is now. Best effort:
/// one not kept is asked again.
pub fn keep(bwrap: &str, name: &str, answer: &str) {
    if let (Some(file), Some(machine)) = (file(), machine(bwrap)) {
        keep_in(&file, &machine, name, answer);
    }
}

/// Forgets every answer kept: the next asks again (`lnk sandbox check --ready`).
pub fn forget() {
    if let Some(file) = file() {
        let _ = std::fs::remove_file(file);
    }
}

/// [`FILE`], in Link's settings for the sandbox.
fn file() -> Option<PathBuf> {
    link_files::layout::Hidden::Sandboxes
        .try_path()
        .ok()
        .map(|dir| dir.join(FILE))
}

/// [`checked`], kept in `file` for `machine`.
fn checked_in<E>(
    file: &Path,
    machine: &str,
    probe: &str,
    check: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    if kept_in(file, machine, probe).as_deref() == Some(PASSED) {
        return Ok(());
    }
    check()?;
    keep_in(file, machine, probe, PASSED);
    Ok(())
}

/// The answer `file` keeps as `name` for `machine`: its first line is
/// `machine`, and a line after it is `name`, a tab and the answer.
fn kept_in(file: &Path, machine: &str, name: &str) -> Option<String> {
    let text = read_trusted(file)?;
    let mut lines = text.lines();
    if lines.next() != Some(machine) {
        return None;
    }
    lines
        .filter_map(|line| line.split_once('\t'))
        .find(|(kept, _)| *kept == name)
        .map(|(_, answer)| answer.to_string())
}

/// Keeps `answer` as `name` for `machine` in `file`, beside the others
/// kept for it; those kept for another machine go.
fn keep_in(file: &Path, machine: &str, name: &str, answer: &str) {
    let plain = |text: &str| !text.contains(['\n', '\r']);
    if !plain(machine) || !plain(name) || !plain(answer) || name.contains('\t') {
        return;
    }
    let mut kept = vec![machine.to_string()];
    if let Some(text) = read_trusted(file) {
        let mut lines = text.lines();
        if lines.next() == Some(machine) {
            kept.extend(
                lines
                    .filter(|line| line.split_once('\t').is_some_and(|(n, _)| n != name))
                    .map(str::to_string),
            );
        }
    }
    kept.push(format!("{name}\t{answer}"));
    let text = kept.join("\n") + "\n";
    if text.len() as u64 > MOST {
        return;
    }
    if let Some(folder) = file.parent()
        && link_files::create_private_dir(folder).is_ok()
    {
        let _ = link_files::write_private(file, text.as_bytes());
    }
}

/// What tells this machine, as a sandbox finds it, from another or from
/// itself changed: the boot, the kernel, who asks (the user, in its
/// namespace), what the kernel lets an unprivileged user do with user
/// namespaces, bubblewrap at `bwrap` and this program. None where any
/// can't be read: then nothing is kept, and every check runs.
pub fn machine(bwrap: &str) -> Option<String> {
    let read = |path: &str| {
        std::fs::read_to_string(path)
            .ok()
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    // Absent where the kernel has no such setting: that, too, is kept.
    let setting = |path: &str| read(path).unwrap_or_else(|| "-".into());
    let exe = std::env::current_exe().ok()?;
    // SAFETY: geteuid can't fail.
    let me = unsafe { libc::geteuid() };
    let key = [
        read("/proc/sys/kernel/random/boot_id")?,
        read("/proc/sys/kernel/osrelease")?,
        read("/proc/sys/kernel/version")?,
        me.to_string(),
        read("/proc/self/uid_map")?,
        setting("/proc/sys/user/max_user_namespaces"),
        setting("/proc/sys/kernel/unprivileged_userns_clone"),
        setting("/proc/sys/kernel/apparmor_restrict_unprivileged_userns"),
        identity(Path::new(bwrap))?,
        identity(&exe)?,
    ]
    .join(" | ");
    (!key.contains('\n')).then_some(key)
}

/// What tells one copy of the file at `path` from another: its path,
/// and its file's identity and time.
pub fn identity(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    Some(format!(
        "{:?} {} {} {} {}.{}",
        path,
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec()
    ))
}

/// `file`'s text, when it's one Link could have written for this user:
/// a plain file, never through a link at its end, in a folder of this
/// user's, the file this user's and no one else's to write, and short.
/// None otherwise, and what reads it asks again.
pub fn read_trusted(file: &Path) -> Option<String> {
    // SAFETY: geteuid can't fail.
    read_as(file, unsafe { libc::geteuid() })
}

/// [`read_trusted`], for user `me`.
fn read_as(file: &Path, me: u32) -> Option<String> {
    let folder: PathBuf = file.parent()?.to_path_buf();
    let folder = std::fs::metadata(&folder).ok()?;
    if !folder.is_dir() || folder.uid() != me {
        return None;
    }
    // Never blocks: a FIFO there opens at once, and is refused below.
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(file)
        .ok()?;
    let meta = opened.metadata().ok()?;
    if !meta.file_type().is_file()
        || meta.uid() != me
        || meta.mode() & 0o022 != 0
        || meta.len() > MOST
    {
        return None;
    }
    let mut text = String::new();
    opened.take(MOST + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MOST).then_some(text)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> link_test_kit::TempDir {
        link_test_kit::TempDir::new(&format!("kept-{name}"))
    }

    fn me() -> u32 {
        // SAFETY: geteuid can't fail.
        unsafe { libc::geteuid() }
    }

    fn passed(file: &Path, machine: &str, probe: &str) -> bool {
        kept_in(file, machine, probe).as_deref() == Some(PASSED)
    }

    /// The check's runs, counted, passing or not as `ok` says.
    fn check(runs: &std::cell::Cell<u32>, ok: bool) -> impl FnOnce() -> Result<(), String> {
        move || {
            runs.set(runs.get() + 1);
            ok.then_some(()).ok_or_else(|| "refused".to_string())
        }
    }

    #[test]
    fn a_check_that_passed_runs_once_per_machine() {
        let dir = temp("once");
        let file = dir.join("sandbox").join(FILE);
        let runs = std::cell::Cell::new(0);
        for _ in 0..3 {
            checked_in(&file, "boot-a", "works", check(&runs, true)).unwrap();
        }
        assert_eq!(runs.get(), 1);
        // Another check is asked of its own.
        checked_in(&file, "boot-a", "requires", check(&runs, true)).unwrap();
        checked_in(&file, "boot-a", "works", check(&runs, true)).unwrap();
        assert_eq!(runs.get(), 2);
        // Another boot, kernel or bubblewrap: every check again, and what
        // was kept for the old one goes.
        checked_in(&file, "boot-b", "works", check(&runs, true)).unwrap();
        assert_eq!(runs.get(), 3);
        assert!(!passed(&file, "boot-b", "requires"));
        assert!(!passed(&file, "boot-a", "works"));
    }

    #[test]
    fn each_answer_is_kept_by_its_name() {
        let dir = temp("named");
        let file = dir.join("sandbox").join(FILE);
        keep_in(&file, "boot", "disables-userns", "true");
        keep_in(&file, "boot", "works", PASSED);
        keep_in(&file, "boot", "disables-userns", "false");
        assert_eq!(
            kept_in(&file, "boot", "disables-userns").as_deref(),
            Some("false")
        );
        assert!(passed(&file, "boot", "works"));
        assert_eq!(kept_in(&file, "boot", "requires"), None);
        // A name or answer that would make another line is never kept.
        keep_in(&file, "boot", "a\tb", "x");
        keep_in(&file, "boot", "c", "x\nworks\tpassed");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "boot\nworks\tpassed\ndisables-userns\tfalse\n"
        );
        // One in the format before answers had names: asked again.
        std::fs::write(&file, "/usr/bin/bwrap 1 2 3 4.5\ntrue\n").unwrap();
        assert_eq!(kept_in(&file, "boot", "disables-userns"), None);
    }

    #[test]
    fn a_check_that_failed_is_never_kept() {
        let dir = temp("failed");
        let file = dir.join("sandbox").join(FILE);
        let runs = std::cell::Cell::new(0);
        for _ in 0..2 {
            let err = checked_in(&file, "boot", "works", check(&runs, false)).unwrap_err();
            assert_eq!(err, "refused");
        }
        assert_eq!(runs.get(), 2);
        assert!(!file.exists());
    }

    #[test]
    fn a_file_link_could_not_have_written_is_refused() {
        let dir = temp("refused");
        let folder = dir.join("sandbox");
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(FILE);
        let good = "boot\nworks\tpassed\n";
        let runs = std::cell::Cell::new(0);

        // Corrupt: not text, or longer than Link writes.
        std::fs::write(&file, b"boot\nworks\tpassed\xff\xfe\n").unwrap();
        assert!(!passed(&file, "boot", "works"));
        std::fs::write(&file, format!("{good}{}", "x".repeat(MOST as usize))).unwrap();
        assert!(!passed(&file, "boot", "works"));
        // Asked again, and written whole.
        checked_in(&file, "boot", "works", check(&runs, true)).unwrap();
        assert_eq!(
            (runs.get(), std::fs::read_to_string(&file).unwrap().as_str()),
            (1, good)
        );

        // Another user's, or one anyone else may write.
        assert!(read_as(&file, me() + 1).is_none());
        assert!(read_as(&file, me()).is_some());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o622)).unwrap();
        assert!(!passed(&file, "boot", "works"));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        if me() == 0 {
            // Root can give it away: the real thing, besides `read_as`'s.
            std::os::unix::fs::chown(&file, Some(65534), Some(65534)).unwrap();
            assert!(!passed(&file, "boot", "works"));
            std::os::unix::fs::chown(&file, Some(0), Some(0)).unwrap();
        }
        assert!(passed(&file, "boot", "works"));

        // A link to a file that says it passed, or a FIFO, which never
        // blocks the read.
        let elsewhere = dir.join("elsewhere");
        std::fs::write(&elsewhere, good).unwrap();
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        assert!(!passed(&file, "boot", "works"));
        std::fs::remove_file(&file).unwrap();
        let fifo = std::ffi::CString::new(file.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a path we own, as a C string.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(!passed(&file, "boot", "works"));
        std::fs::remove_file(&file).unwrap();

        // In a folder that isn't this user's.
        std::fs::write(&file, good).unwrap();
        assert!(read_as(&file, me()).is_some());
        if me() == 0 {
            std::os::unix::fs::chown(&folder, Some(65534), Some(65534)).unwrap();
            assert!(!passed(&file, "boot", "works"));
            std::os::unix::fs::chown(&folder, Some(0), Some(0)).unwrap();
        }
    }

    #[test]
    fn the_machine_changes_with_its_bubblewrap_and_holds_its_boot() {
        let dir = temp("machine");
        let bwrap = dir.join("bwrap");
        link_test_kit::write_program(&bwrap, "#!/bin/sh\n").unwrap();
        let bwrap = bwrap.display().to_string();
        let before = machine(&bwrap).unwrap();
        assert_eq!(machine(&bwrap).as_deref(), Some(before.as_str()));
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
        let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
        assert!(before.contains(boot.trim()) && before.contains(release.trim()));
        // Replaced, it's another bubblewrap.
        std::fs::remove_file(&bwrap).unwrap();
        link_test_kit::write_program(Path::new(&bwrap), "#!/bin/sh\nexit 0\n").unwrap();
        assert_ne!(machine(&bwrap).unwrap(), before);
        // None to ask of: nothing kept.
        assert!(machine(&dir.join("none").display().to_string()).is_none());
    }
}
