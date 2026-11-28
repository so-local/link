//! Where a stop sent to a Linux sandbox goes. Bubblewrap passes on no
//! signal: a stop sent to it ends it, and its sandbox with it, before
//! the program inside hears of it. So the stage in front of bubblewrap
//! asks it for its report ([`link_sandbox_contract::bubblewrap`]),
//! which names the sandbox's first process, and sends each stop to the
//! group that process leads instead.
//!
//! That process is bubblewrap's own init, the sandbox's process 1, and
//! with `--new-session` it leads the sandbox's session and process
//! group: `confine`, the proxy inside and the program are in it, and
//! each passes a stop on to what it runs. Process 1 itself, with no
//! handler for a stop, never gets one from outside its namespace.
//!
//! The group is named by a pidfd for that process, opened once it's
//! proved bubblewrap's own child (its parent, while it's still there),
//! so a number that a process outside the sandbox came to hold is never
//! sent a stop ([`link_secretive::process::pass_to_group_led_by`]).
//! Until it leads its group, or once the group has ended, a stop goes to
//! bubblewrap itself, as before: nothing inside runs yet, or still.
//!
//! The group is named only once a process besides process 1 runs in it:
//! process 1 alone would take the stop and ignore it, and what starts in
//! the sandbox after would never hear it. Until then a stop ends
//! bubblewrap, and the stage ends the process 1 it leaves
//! ([`link_secretive::process::end_left`]).

use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;

use anyhow::{Context, ensure};
use serde::Deserialize;

/// `session.toml`, as read.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    poll_ms: u64,
}

/// `session.toml`, read once.
fn settings() -> &'static Settings {
    static READ: std::sync::OnceLock<Settings> = std::sync::OnceLock::new();
    READ.get_or_init(|| toml::from_str(include_str!("session.toml")).expect("session.toml reads"))
}

/// Bubblewrap's report of its sandbox, asked for on a pipe of this
/// process: started, the stage reads it and passes stops to the group
/// it names ([`Self::started`]).
pub struct Asked {
    read: OwnedFd,
    write: OwnedFd,
    /// A descriptor its caller gave bubblewrap for its report (the
    /// proxy's, for its DNS): handed the report as it was read.
    theirs: Option<OwnedFd>,
    /// Whether its sandbox has a session of its own (`--new-session`):
    /// bubblewrap then has a process group of its own too, which no stop
    /// sent to the stages in front of it reaches.
    session: bool,
}

/// Asks bubblewrap, `argv[0]` with its options and command, for its
/// report on a pipe of this process. A report it was already asked for
/// (`--info-fd`) is taken over, that descriptor handed what it says.
pub fn ask(argv: &mut Vec<String>) -> anyhow::Result<Asked> {
    let options = super::pins::options(argv)?;
    let (read, write) = pipe()?;
    let session = options.iter().any(|&at| argv[at] == "--new-session");
    let ours = write.as_raw_fd().to_string();
    let theirs = match options.iter().find(|&&at| argv[at] == "--info-fd") {
        Some(&at) => {
            let fd: RawFd = argv[at + 1]
                .parse()
                .context("bubblewrap's report descriptor")?;
            argv[at + 1] = ours;
            Some(taken(fd)?)
        }
        None => {
            argv.splice(1..1, ["--info-fd".into(), ours]);
            None
        }
    };
    Ok(Asked {
        read,
        write,
        theirs,
        session,
    })
}

impl Asked {
    /// Bubblewrap's `command` gets the pipe's write end, and a process
    /// group of its own where its sandbox has a session of its own.
    pub fn pass_to(&self, command: &mut std::process::Command) {
        let fd = self.write.as_raw_fd();
        // SAFETY: fcntl is async-signal-safe, on a descriptor this
        // `Asked` keeps open until spawn returns.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        if self.session {
            command.process_group(0);
        }
    }

    /// Bubblewrap, started as `bwrap`: its report read on a thread of
    /// its own, handed on to its caller's descriptor, and each stop
    /// passed from then to the group of the process it names, once that
    /// is proved bubblewrap's child ([`leader`]).
    pub fn started(self, bwrap: u32) {
        let Self {
            read,
            write,
            theirs,
            ..
        } = self;
        drop(write);
        std::thread::spawn(move || {
            let text = link_sandbox_contract::bubblewrap::read_info(std::fs::File::from(read))
                .unwrap_or_default();
            // Handed on first: bubblewrap may wait (`--block-fd`) for
            // what its caller does with it.
            if let Some(theirs) = theirs {
                let _ = std::fs::File::from(theirs).write_all(text.as_bytes());
            }
            if let Some((pid, leader)) = link_sandbox_contract::bubblewrap::child_pid(&text)
                .and_then(|pid| Some((pid, leader(pid, bwrap).ok()?)))
                && inhabited(pid, &leader)
            {
                link_secretive::process::pass_to_group_led_by(leader);
            }
        });
    }
}

/// A pidfd for `pid`, proved the child of `bwrap`, a child of this
/// process it hasn't reaped. Its parent is read once the pidfd holds it,
/// and it's still there after: a process can't come back, so the number
/// named it all along. Bubblewrap starts one child, the sandbox's first
/// process; a number no child of bubblewrap holds is refused.
pub fn leader(pid: u32, bwrap: u32) -> anyhow::Result<OwnedFd> {
    let leader = pidfd(pid).with_context(|| format!("process {pid}"))?;
    let parent = parent(pid)?;
    ensure!(
        parent == bwrap,
        "process {pid} is not bubblewrap's: its parent is {parent}"
    );
    ensure!(!exited(&leader, 0)?, "process {pid} has ended");
    Ok(leader)
}

/// Whether the sandbox whose process 1 is `pid` (held by `leader`) comes
/// to run a process besides it, which then hears a stop sent to its group:
/// false once process 1 has ended.
fn inhabited(pid: u32, leader: &OwnedFd) -> bool {
    let poll = i32::try_from(settings().poll_ms).unwrap_or(i32::MAX);
    loop {
        let children =
            std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap_or_default();
        if !children.trim().is_empty() {
            return true;
        }
        if exited(leader, poll).unwrap_or(true) {
            return false;
        }
    }
}

/// Process `pid`'s parent, as `/proc` gives it.
fn parent(pid: u32) -> anyhow::Result<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .with_context(|| format!("process {pid}"))?;
    // `pid (name) state ppid ...`: the name may hold spaces and parentheses.
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(1)?.parse().ok())
        .with_context(|| format!("process {pid}'s parent"))
}

/// A pidfd for process `pid`: it holds that process, never another
/// given its number later.
pub(super) fn pidfd(pid: u32) -> std::io::Result<OwnedFd> {
    // SAFETY: pidfd_open returns a new close-on-exec descriptor.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

/// Whether the process `fd` holds has ended, waiting `timeout`
/// milliseconds at most.
pub(super) fn exited(fd: &OwnedFd, timeout: i32) -> std::io::Result<bool> {
    let mut event = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll reads and fills this single initialized event.
    if unsafe { libc::poll(&mut event, 1, timeout) } < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error);
    }
    Ok(event.revents != 0)
}

/// A pipe, both ends closed on exec.
fn pipe() -> anyhow::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 fills two descriptors, which are owned from here.
    ensure!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
        "a pipe: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Descriptor `fd`, which this process was started with for bubblewrap,
/// owned from here and closed on exec: bubblewrap writes to ours.
fn taken(fd: RawFd) -> anyhow::Result<OwnedFd> {
    // SAFETY: a flag on a descriptor; one not open fails.
    ensure!(
        fd > 2 && unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
        "bubblewrap's report descriptor {fd} isn't open"
    );
    // SAFETY: open, inherited for bubblewrap, and owned by nothing else here.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A report naming a process bubblewrap didn't start, or one that's
    /// gone, gives no group to stop: the process named gets nothing.
    #[test]
    fn a_process_the_sandbox_didnt_start_is_never_its_leader() {
        let home = link_test_kit::TempDir::new("session-leader");
        let mut sleep = link_test_kit::env::command("sleep", &home);
        sleep.arg("300");
        let bwrap = link_test_kit::ChildGroup::spawn(&mut sleep).unwrap();
        let unrelated = link_test_kit::ChildGroup::spawn(&mut sleep).unwrap();
        let err = leader(unrelated.id(), bwrap.id()).unwrap_err();
        assert!(format!("{err:#}").contains("not bubblewrap's"), "{err:#}");
        // A pid of this process's own, whose parent is the test runner.
        assert!(leader(std::process::id(), bwrap.id()).is_err());
        assert!(unrelated.try_status().unwrap().is_none());
        // The child of the stand-in, as bubblewrap's sandbox is: its leader.
        let mut shell = link_test_kit::env::command("sh", &home);
        shell.args(["-c", "sleep 300 & echo $!; wait"]);
        shell.stdout(std::process::Stdio::piped());
        let mut parent = link_test_kit::ChildGroup::spawn(&mut shell).unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(parent.take_stdout().unwrap()),
            &mut line,
        )
        .unwrap();
        let child: u32 = line.trim().parse().unwrap();
        assert!(leader(child, parent.id()).is_ok());
        assert!(leader(child, bwrap.id()).is_err());
    }

    /// A report already asked for (the proxy's) is taken over, and one
    /// that wasn't is asked for after bubblewrap's program.
    #[test]
    fn the_report_is_asked_for_once() {
        let words = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        let mut argv = words("bwrap --new-session --unshare-pid -- sh");
        let asked = ask(&mut argv).unwrap();
        assert!(asked.session);
        assert_eq!(argv[1], "--info-fd");
        assert_eq!(argv[2], asked.write.as_raw_fd().to_string());
        assert_eq!(argv[3..], words("--new-session --unshare-pid -- sh"));
        let mut argv = words("bwrap --unshare-pid -- sh");
        assert!(!ask(&mut argv).unwrap().session);
        assert!(ask(&mut words("bwrap --unknown -- sh")).is_err());
    }
}
