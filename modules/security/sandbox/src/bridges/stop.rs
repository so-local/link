//! Ending a tool's server whole. Its input is closed first, by its owner,
//! and it gets the sandbox's `tool_stop` to end on its own and close what
//! it started, such as a browser. Then its process group is asked to stop
//! (SIGTERM), given that long again if it's still running, and killed
//! (SIGKILL), so nothing it started outlives it, on a Mac too, where no
//! child learns that its parent ended.

use std::time::{Duration, Instant};

/// Ends `pid`, a server leading a process group of its own, whose input
/// its owner has closed, as the module's page says; the caller reaps it
/// after. Until then its group's ID stays its own, so the signals reach
/// only what it started.
pub fn server(pid: u32, grace: Duration) {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return;
    };
    if pid <= 1 {
        return;
    }
    let ended = ended_within(pid, grace);
    // SAFETY: the group made for this server alone, never 0 or -1.
    unsafe { libc::kill(-pid, libc::SIGTERM) };
    if !ended {
        ended_within(pid, grace);
    }
    // SAFETY: as above.
    unsafe { libc::kill(-pid, libc::SIGKILL) };
}

/// Whether `pid` ends within `grace`, left unreaped to be signalled.
fn ended_within(pid: libc::pid_t, grace: Duration) -> bool {
    let start = Instant::now();
    loop {
        if ended(pid) {
            return true;
        }
        if start.elapsed() >= grace {
            return false;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "a server's owner polls for its end, no child's exit to wait on without reaping it"
        )]
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether `pid`, this process's child, has ended, without reaping it.
fn ended(pid: libc::pid_t) -> bool {
    // SAFETY: waitid on a child of this process, writing only into
    // `info`; WNOWAIT leaves it to be reaped by its owner.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let found = libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        );
        // Ended (its pid filled in), or no longer this process's child.
        found != 0 || info.si_pid() != 0
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt;
    use std::time::Duration;

    use link_test_kit::wait;

    /// A server that ends when its input closes ends then, on its own,
    /// and what it started that stays is ended with its group.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_server_and_what_it_started_end() {
        let dir = link_test_kit::TempDir::new("stop");
        let stays = dir.join("stays");
        link_test_kit::write_program(&stays, "#!/bin/sh\nwhile :; do sleep 1; done\n").unwrap();
        let script = format!("{} & read line; exit 0", stays.display());
        let mut child = link_test_kit::env::command("sh", &dir)
            .args(["-c", &script])
            .process_group(0)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        wait::eventually("what it started", Duration::from_secs(10), || {
            match link_test_kit::process::left_under(&dir).is_empty() {
                true => Err("not yet"),
                false => Ok(()),
            }
        });
        drop(child.stdin.take());
        super::server(child.id(), Duration::from_secs(5));
        // Ended on its own: its own exit, never a signal's.
        assert!(
            child.wait().unwrap().success(),
            "its own exit, not a signal"
        );
        wait::eventually("what it started ended", Duration::from_secs(10), || {
            match link_test_kit::process::left_under(&dir).as_slice() {
                [] => Ok(()),
                left => Err(left.join(", ")),
            }
        });
    }

    /// A server that stays when its input closes and ignores SIGTERM is
    /// killed.
    #[test]
    fn a_server_that_stays_is_ended() {
        let dir = link_test_kit::TempDir::new("stop");
        let mut child = link_test_kit::env::command("sh", &dir)
            .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
            .process_group(0)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdin.take());
        super::server(child.id(), Duration::from_millis(100));
        let status = wait::exited("the server", &mut child, Duration::from_secs(10));
        assert!(!status.success());
    }
}
