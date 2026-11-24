//! What `lnk env here` starts: the specs it writes, the processes it
//! tracks and stops, and the pipes it hands them.

use super::*;

/// Where the sandbox specs a request holds as text are written while
/// they're used: in Link's settings, which no sandbox writes.
fn runs_dir() -> PathBuf {
    link_files::layout::Hidden::Envs.path().join("runs")
}

/// `text`, a sandbox spec, written to a file of its own in [`runs_dir`],
/// `<label>-<random>.toml`: made new (never one that's there), 600. Its
/// paths are `~/` or absolute ([`super::check_shipped`]), so none is
/// read from where it's written.
pub(super) fn write_spec(label: &str, text: &str) -> anyhow::Result<PathBuf> {
    write_spec_in(&runs_dir(), label, text)
}

/// [`write_spec`], in `dir`.
fn write_spec_in(dir: &Path, label: &str, text: &str) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    link_files::create_private_dir(dir)?;
    let path = dir.join(format!("{label}-{}.toml", crate::remote::random_hex(8)?));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .within(format!("writing {}", path.display()))?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}

/// The children this started, each its own process group, stopped when
/// its input closes.
static CHILDREN: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn track(child: &Child) {
    CHILDREN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(child.id());
}

pub(super) fn untrack(child: &Child) {
    CHILDREN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|pid| *pid != child.id());
}

/// Stops `pid`'s process group: asked, then made to after a moment.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a stopping group before killing it"
)]
pub(super) fn stop_group(pid: u32) {
    #[cfg(unix)]
    {
        let group = -(pid as i32);
        // SAFETY: kill sends a signal; a group that's gone is an error
        // it returns.
        unsafe { libc::kill(group, libc::SIGTERM) };
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            // SAFETY: as above; signal 0 only asks whether it's there.
            if unsafe { libc::kill(group, 0) } != 0 {
                return;
            }
        }
        // SAFETY: as above.
        unsafe { libc::kill(group, libc::SIGKILL) };
    }
}

/// Stops every child this started.
pub(super) fn stop_all() {
    let children = std::mem::take(&mut *CHILDREN.lock().unwrap_or_else(|e| e.into_inner()));
    for pid in children.into_iter().rev() {
        stop_group(pid);
    }
}

/// The `lnk` this machine runs, in full, for a background service.
pub(super) fn lnk_path() -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
    let lnk = exe.with_file_name(format!("lnk{}", std::env::consts::EXE_SUFFIX));
    Ok(match std::env::var_os(link_core::LNK_EXE) {
        Some(path) => PathBuf::from(path),
        None => lnk,
    }
    .display()
    .to_string())
}

/// `sudo`, or a stand-in for tests (`LNK_SUDO`), which a debug build
/// alone reads: what runs as root is never a program a variable names.
pub fn sudo() -> PathBuf {
    link_secretive::test_var("LNK_SUDO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("sudo"))
}

/// The variables [`kept`] keeps.
const KEPT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "TERM",
    link_core::release::RELEASES_VAR,
];

/// What a root command keeps of this one's environment: the releases
/// this machine's `lnk` follows (`LNK_RELEASES_URL`, a location, never a
/// secret) among them, so an `lnk` it runs installs from them too.
pub(super) fn kept() -> Vec<(String, String)> {
    KEPT.iter()
        .filter_map(|k| Some((k.to_string(), std::env::var(k).ok()?)))
        .collect()
}

/// `command` started from `cwd`, its own process group, its input
/// closed, tracked to be stopped.
pub(super) fn spawn(mut command: Command, cwd: &Path, capture: bool) -> anyhow::Result<Child> {
    command.current_dir(cwd).stdin(Stdio::null());
    if capture {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let child = command
        .spawn()
        .within(format!("running {:?}", command.get_program()))?;
    track(&child);
    Ok(child)
}

/// A pipe: its read end, its write end.
pub(super) fn pipe() -> anyhow::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    // SAFETY: pipe fills two descriptors, owned from here; close-on-exec,
    // so no other child inherits them.
    #[cfg(target_os = "linux")]
    let made = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let made = unsafe {
        let made = libc::pipe(fds.as_mut_ptr());
        if made == 0 {
            libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
        }
        made
    };
    ensure!(made == 0, "a pipe: {}", std::io::Error::last_os_error());
    // SAFETY: just made, each owned once.
    Ok(unsafe {
        (
            std::os::fd::OwnedFd::from_raw_fd(fds[0]),
            std::os::fd::OwnedFd::from_raw_fd(fds[1]),
        )
    })
}

/// Gives `command`'s program `fd` as its descriptor 3, open across
/// exec; this process's copy is the caller's to drop once it's spawned.
pub(super) fn pass_fd(command: &mut Command, fd: &std::os::fd::OwnedFd) {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let raw = fd.as_raw_fd();
    // SAFETY: dup2 and fcntl are async-signal-safe: descriptor 3 in the
    // child is the pipe's read end, without close-on-exec.
    unsafe {
        command.pre_exec(move || {
            if raw == 3 {
                let flags = libc::fcntl(3, libc::F_GETFD);
                if flags < 0 || libc::fcntl(3, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            } else if libc::dup2(raw, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    /// A root stage's `lnk` installs from the releases its machine follows.
    #[test]
    fn a_root_command_keeps_the_releases_followed() {
        assert!(super::KEPT.contains(&link_core::release::RELEASES_VAR));
    }

    use super::*;

    #[test]
    fn a_spec_shipped_is_written_new_and_only_for_its_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("env-runs");
        let first = write_spec_in(&dir, "job-a", "name = \"s\"\n").unwrap();
        let second = write_spec_in(&dir, "job-a", "name = \"s\"\n").unwrap();
        assert_ne!(first, second, "each made new");
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(first.starts_with(&*dir));
        // Link's settings, which no sandbox writes.
        assert!(runs_dir().starts_with(link_files::installation_dir()));
    }
}
