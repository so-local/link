//! A wrapper in front of its child: the stops sent to it passed on.

/// Which of the terminal's own signals (SIGINT, SIGQUIT) [`stand_in_front`]
/// passes on.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminal {
    /// The child is in the terminal's group, so they reach it directly:
    /// ignored here, so this process outlives it and sees it end.
    Reaches,
    /// Passed on like the rest: the child isn't in the terminal's group.
    Passed,
}

/// `child`, which this process stands in front of and waits for: SIGTERM
/// and SIGHUP sent here passed on to it, and the terminal's own as
/// `terminal` says. The one way a wrapper hands its stop on. Once per
/// process: a later call names a new child.
#[cfg(unix)]
pub fn stand_in_front(child: u32, terminal: Terminal) {
    stand_in_front_of(child as i32, terminal);
}

/// As [`stand_in_front`], passing stops to the child's whole process
/// group. The child must lead a group made for this run alone.
#[cfg(unix)]
pub fn stand_in_front_group(child: u32, terminal: Terminal) {
    assert!(child > 1 && child <= i32::MAX as u32);
    stand_in_front_of(-(child as i32), terminal);
}

/// The child [`stand_in_front`] passes stops to: a process, or a group
/// as a negative number; 0, none.
#[cfg(unix)]
static CHILD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// A pidfd for the process whose group [`pass_to_group_led_by`] passes
/// stops to; -1, none. Never closed once set: a stop being passed on on
/// another thread may hold its number still, which must never come to
/// name another descriptor.
#[cfg(target_os = "linux")]
static LEADER: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

#[cfg(unix)]
fn stand_in_front_of(child: i32, terminal: Terminal) {
    use std::sync::atomic::Ordering;
    extern "C" fn pass(signal: libc::c_int) {
        #[cfg(target_os = "linux")]
        {
            let leader = LEADER.load(Ordering::Relaxed);
            // SAFETY: a raw system call is async-signal-safe. The pidfd
            // holds its process's number, so the group it names is never
            // another's; with no member left (or a kernel before Linux 6.9,
            // which has no PIDFD_SIGNAL_PROCESS_GROUP) it fails, and the
            // child this process holds unreaped is sent the stop instead.
            if leader >= 0
                && unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        leader,
                        signal,
                        std::ptr::null::<libc::siginfo_t>(),
                        libc::PIDFD_SIGNAL_PROCESS_GROUP,
                    )
                } == 0
            {
                return;
            }
        }
        let child = CHILD.load(Ordering::Relaxed);
        if !(-1..=1).contains(&child) {
            // SAFETY: kill is async-signal-safe.
            unsafe { libc::kill(child, signal) };
        }
    }
    #[cfg(target_os = "linux")]
    LEADER.store(-1, Ordering::Relaxed);
    CHILD.store(child, Ordering::Relaxed);
    let theirs = match terminal {
        Terminal::Reaches => libc::SIG_IGN,
        Terminal::Passed => pass as *const () as libc::sighandler_t,
    };
    for (signal, handler) in [
        (libc::SIGTERM, pass as *const () as libc::sighandler_t),
        (libc::SIGHUP, pass as *const () as libc::sighandler_t),
        (libc::SIGINT, theirs),
        (libc::SIGQUIT, theirs),
    ] {
        // SAFETY: `pass` only makes async-signal-safe calls; SIG_IGN ignores.
        unsafe { libc::signal(signal, handler) };
    }
}

/// The stops [`stand_in_front`] passes on (each it passes, as its
/// `terminal` said) go to the process group `leader` leads, a pidfd, for
/// a child that starts the processes it runs in a session of their own
/// and passes on no stop itself (bubblewrap, a sandbox's first process
/// leading its session): to the child it stands in front of while that
/// group has no member, before its leader makes it or after it ends, or
/// on a kernel before Linux 6.9. The pidfd holds `leader`'s number,
/// so no other process's group is sent a stop. Until the next
/// [`stand_in_front`] or [`stand_aside`]; `leader` is kept open until
/// the process ends.
#[cfg(target_os = "linux")]
pub fn pass_to_group_led_by(leader: std::os::fd::OwnedFd) {
    use std::os::fd::IntoRawFd;
    LEADER.store(leader.into_raw_fd(), std::sync::atomic::Ordering::Relaxed);
}

/// Stops passing stops on, before the child [`stand_in_front`] named is
/// reaped: its number is free from then, and may name another process.
#[cfg(unix)]
pub fn stand_aside() {
    use std::sync::atomic::Ordering;
    #[cfg(target_os = "linux")]
    LEADER.store(-1, Ordering::Relaxed);
    CHILD.store(0, Ordering::Relaxed);
}

/// Waits for `child`, which this process stands in front of, and its
/// exit status: once it has ended, unreaped, this stands aside
/// ([`stand_aside`]), and only then reaps it, so no stop is passed to
/// a number it no longer holds.
#[cfg(unix)]
pub fn wait_in_front(child: &mut std::process::Child) -> std::io::Result<std::process::ExitStatus> {
    // SAFETY: siginfo_t is a C record with a valid all-zero representation.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: the writable record observes this direct child's end
        // without reaping it.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if waited == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            stand_aside();
            return Err(error);
        }
    }
    stand_aside();
    child.wait()
}
