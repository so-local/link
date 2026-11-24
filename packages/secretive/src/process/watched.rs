//! A child sent a signal when this process ends, on every system, even
//! one that never looks for its parent itself, as a third-party server
//! doesn't. A watcher is forked in the child's place before its program
//! runs. It blocks on a pipe whose write end only this process holds, so
//! the pipe ends exactly when this process does, however it ends, and
//! then the watcher signals the child and its group.

use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::OnceLock;

/// The two ends of the pipe that tells watchers this process is alive:
/// the write end held here alone, made close-on-exec, so no program
/// started holds it.
pub(super) struct Alive {
    read: OwnedFd,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "held, never read: its closing, when this process ends, is what watchers see"
        )
    )]
    write: OwnedFd,
}

impl Alive {
    pub(super) fn new() -> std::io::Result<Self> {
        let (read, write) = std::io::pipe()?;
        Ok(Self {
            read: read.into(),
            write: write.into(),
        })
    }

    /// This process's own, made once and held until it ends.
    fn ours() -> std::io::Result<&'static Self> {
        static OURS: OnceLock<Alive> = OnceLock::new();
        if let Some(alive) = OURS.get() {
            return Ok(alive);
        }
        let made = Self::new()?;
        Ok(OURS.get_or_init(|| made))
    }
}

/// `command`'s child is sent `signal` when this process ends, however it
/// ends (killed too), on every system: its process group too, where it
/// leads one, so what it started that stayed in its group is reached.
/// The child needs nothing of its own: a watcher, forked in its place
/// before its program runs, does the watching, and it leaves without a
/// signal once the child ends first. The watcher is in the child's group,
/// so stopping that group stops it as well. Unlike Linux's parent-death
/// signal ([`super::ends_with_parent`]), it follows the process, not the
/// thread that started the child.
pub fn watched_by_parent(command: &mut Command, signal: i32) -> std::io::Result<&mut Command> {
    Ok(watched_through(command, signal, Alive::ours()?))
}

/// [`watched_by_parent`], told this process's end by `alive`.
pub(super) fn watched_through<'a>(
    command: &'a mut Command,
    signal: i32,
    alive: &Alive,
) -> &'a mut Command {
    let read = alive.read.as_raw_fd();
    // SAFETY: the closure runs in the child before its program, calling
    // only async-signal-safe functions (getpid, fork, and what `watch`
    // calls), allocating nothing.
    unsafe { command.pre_exec(move || watch(read, signal)) }
}

/// How long the watcher waits on the pipe before it looks again whether
/// its child is still there, in milliseconds.
const LOOK: libc::c_int = 1000;

/// The most descriptors a watcher closes, past what any process here holds.
const DESCRIPTORS_MOST: libc::c_long = 1 << 16;

/// The fewest descriptors a watcher closes, whatever the system's limit.
const DESCRIPTORS_LEAST: libc::c_long = 256;

/// In the child, before its program runs: forks the watcher, which
/// keeps only `read`, and waits there until this process ends (the pipe
/// closes) or the child does (the watcher's parent changes).
fn watch(read: RawFd, signal: i32) -> std::io::Result<()> {
    // SAFETY: async-signal-safe calls only, in a child of a fork; the
    // watcher never returns, ending with _exit.
    unsafe {
        let child = libc::getpid();
        match libc::fork() {
            -1 => Err(std::io::Error::last_os_error()),
            0 => {
                // Nothing of the child's or this process's stays open in
                // the watcher, which never runs a program to close them:
                // a pipe another program waits to see closed, the write
                // end of `read`'s own pipe among them.
                // No limit known (-1): as many as it may close at most.
                let most = match libc::sysconf(libc::_SC_OPEN_MAX) {
                    n if n > 0 => n.clamp(DESCRIPTORS_LEAST, DESCRIPTORS_MOST),
                    _ => DESCRIPTORS_MOST,
                };
                for fd in 0..most as RawFd {
                    if fd != read {
                        libc::close(fd);
                    }
                }
                let mut waiting = libc::pollfd {
                    fd: read,
                    events: libc::POLLIN,
                    revents: 0,
                };
                loop {
                    if libc::getppid() != child {
                        // The child ended first: there's nothing to stop.
                        libc::_exit(0);
                    }
                    if libc::poll(&mut waiting, 1, LOOK) > 0 {
                        break;
                    }
                }
                // Its parent is still the child, so the child's ID, and
                // the group's it leads, are still its own. The child first:
                // the watcher is in the child's group, and ends with it.
                libc::kill(child, signal);
                libc::kill(-child, signal);
                libc::_exit(0)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::time::Duration;

    use super::*;

    fn sleeper(home: &std::path::Path, alive: &Alive, signal: i32) -> link_test_kit::ChildGroup {
        let mut command = link_test_kit::env::command("sh", home);
        command.args(["-c", "sleep 30 & wait"]);
        watched_through(&mut command, signal, alive);
        link_test_kit::ChildGroup::spawn(&mut command).unwrap()
    }

    /// When the process that started a child ends (its pipe's write end
    /// closes), the child and what it started in its group get the
    /// signal, though neither looks for its parent.
    #[test]
    fn a_child_and_its_group_end_with_their_parent() {
        let home = link_test_kit::TempDir::new("watched-ends");
        let alive = Alive::new().unwrap();
        let child = sleeper(home.path(), &alive, libc::SIGTERM);
        let group = child.id() as libc::pid_t;
        // While its parent lives, it runs on.
        assert!(child.try_status().unwrap().is_none());
        drop(alive.write);
        let status = link_test_kit::wait::eventually(
            "the child to end with its parent",
            Duration::from_secs(10),
            || child.try_status().unwrap(),
        );
        assert_eq!(status.signal(), Some(libc::SIGTERM));
        // The sleep it started, in its group, ended with it.
        #[cfg(target_os = "linux")]
        link_test_kit::wait::eventually("its group to empty", Duration::from_secs(10), || {
            only_leader(group).then_some(())
        });
        let _ = group;
    }

    /// A child that ends on its own takes its watcher with it, and the
    /// watcher holds none of the child's pipes, so its output ends with it.
    #[test]
    fn a_watcher_leaves_with_its_child() {
        use std::io::Read;
        let home = link_test_kit::TempDir::new("watched-leaves");
        let alive = Alive::new().unwrap();
        let mut command = link_test_kit::env::command("sh", home.path());
        command
            .args(["-c", "echo done"])
            .stdout(std::process::Stdio::piped());
        watched_through(&mut command, libc::SIGKILL, &alive);
        let mut child = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
        let group = child.id() as libc::pid_t;
        let mut said = String::new();
        child
            .take_stdout()
            .unwrap()
            .read_to_string(&mut said)
            .unwrap();
        assert_eq!(said, "done\n");
        let status =
            link_test_kit::wait::eventually("the child to end", Duration::from_secs(10), || {
                child.try_status().unwrap()
            });
        assert!(status.success());
        #[cfg(target_os = "linux")]
        link_test_kit::wait::eventually("its watcher to leave", Duration::from_secs(10), || {
            only_leader(group).then_some(())
        });
        let _ = group;
    }

    /// A child that left the group it led is still signalled: the watcher,
    /// still in that group, signals the child before the group it ends
    /// with.
    #[test]
    fn a_child_that_left_its_group_still_ends_with_its_parent() {
        if !link_test_kit::gate::program("python3", &["-c", ""]) {
            return;
        }
        let home = link_test_kit::TempDir::new("watched-left-group");
        let alive = Alive::new().unwrap();
        let left = home.join("left");
        // It leads a group of its own, then joins this process's.
        let leave = format!(
            "import os, time\nos.setpgid(0, {})\nopen({:?}, 'w').close()\ntime.sleep(30)",
            // SAFETY: getpgrp reads this process's group.
            unsafe { libc::getpgrp() },
            left.display().to_string(),
        );
        let mut command = link_test_kit::env::command("python3", home.path());
        command.args(["-c", &leave]);
        watched_through(&mut command, libc::SIGTERM, &alive);
        let child = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
        link_test_kit::wait::appears(
            "the child to leave its group",
            &left,
            Duration::from_secs(10),
        );
        drop(alive.write);
        let status = link_test_kit::wait::eventually(
            "the child to end with its parent",
            Duration::from_secs(10),
            || child.try_status().unwrap(),
        );
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }

    /// Whether every process in `group` but its leader, ended and
    /// unreaped, has left it.
    #[cfg(target_os = "linux")]
    fn only_leader(group: libc::pid_t) -> bool {
        let found = std::fs::read_dir("/proc").unwrap();
        !found.flatten().any(|entry| {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<i32>().ok())
            else {
                return false;
            };
            // SAFETY: getpgid reads another process's group.
            pid != group && unsafe { libc::getpgid(pid) } == group
        })
    }
}
