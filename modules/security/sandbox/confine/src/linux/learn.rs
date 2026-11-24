//! `lnk sandbox learn` on Linux: the exec watch inside the sandbox
//! ([`super::execs`]) writes each program started to a pipe this end
//! reads.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::ExitStatus;

use anyhow::{Context, ensure};

/// Runs `argv` (wrapped, `env` its whole environment) with the pipe's
/// write end open across exec into the sandbox, where `sandbox confine`
/// takes it: its exit status, and every program it started, a line each.
pub fn learn(argv: &[String], env: &[(String, String)]) -> anyhow::Result<(ExitStatus, String)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 fills two descriptors, which we own from here.
    ensure!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
        "a pipe: {}",
        std::io::Error::last_os_error()
    );
    let (from, to) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    let to_fd = to.as_raw_fd();
    // Only what it keeps: none of this process's secrets.
    let mut child = link_secretive::process::command(&argv[0]);
    child
        .args(&argv[1..])
        .env_clear()
        .envs(env.iter().cloned())
        .env(super::execs::LEARN_FD, to_fd.to_string());
    // SAFETY: fcntl is async-signal-safe: the write end, open across exec
    // into the sandbox, where `sandbox confine` takes it.
    unsafe {
        child.pre_exec(move || {
            libc::fcntl(to_fd, libc::F_SETFD, 0);
            Ok(())
        });
    }
    let started = child
        .spawn()
        .with_context(|| format!("running {}", argv[0]));
    drop(to);
    // Read as it runs, so a full pipe never holds a program up; it ends
    // when everything in the sandbox has, with the command.
    let reading = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut std::fs::File::from(from), &mut text);
        text
    });
    let status = started.and_then(|mut child| Ok(child.wait()?))?;
    Ok((status, reading.join().unwrap_or_default()))
}
