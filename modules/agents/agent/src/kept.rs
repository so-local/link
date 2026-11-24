//! A program kept running for as long as it's wanted ([`Kept`]): started
//! again each time it ends, as its [`Restart`] says, and once stopped,
//! asked to end, waited for, and killed if it doesn't. The one way the
//! harness keeps a program beside an agent: its scheduler's runner, the
//! shared bridge's runners, and memory's model server.

use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long a stopped program has to end of itself, once asked, before
/// it's killed: what it was doing finished (a runner's firings, its mark
/// on its store let go), well within the time `lnk agent stop` waits for
/// an agent in a terminal.
pub const END_WITHIN: Duration = Duration::from_secs(10);

/// When a program that ended is started again: `after` its first end,
/// the wait doubling at each end after up to `at_most`, and back to
/// `after` once it ran longer than `at_most`. The same wait each time
/// when they're equal.
#[derive(Clone, Copy, Debug)]
pub struct Restart {
    pub after: Duration,
    pub at_most: Duration,
}

/// What a kept program's keeper is told: each child as it starts (to
/// take its output), and how each ended, saying whether to start it
/// again. Nothing, and always again, by default.
pub trait Watch: Send + 'static {
    fn started(&mut self, _child: &mut Child) {}
    fn ended(&mut self, _how: &std::io::Result<ExitStatus>) -> bool {
        true
    }
}

impl Watch for () {}

/// Each end said in the log, as `<what> stopped (<status>)`, and always
/// started again.
pub struct Said(pub &'static str);

impl Watch for Said {
    fn ended(&mut self, how: &std::io::Result<ExitStatus>) -> bool {
        match how {
            Ok(status) => link_format::daemon::info!("{} stopped ({status})", self.0),
            Err(err) => link_format::daemon::warn!("{}: {err}", self.0),
        }
        true
    }
}

/// A program kept running ([`Kept::start`]) until [`Kept::stop`], or
/// until it's dropped. Its child is given a pipe for stdin, held open
/// while it runs, and ends with this process however it ends
/// ([`link_secretive::process::ends_with_parent`]). A stop asks it
/// to end both ways Link's programs listen for, its stdin closed and a
/// SIGTERM, then waits [`END_WITHIN`] before killing it.
pub struct Kept {
    shared: Arc<Shared>,
    keeper: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    state: Mutex<State>,
    /// Told when it's stopped.
    changed: Condvar,
}

#[derive(Default)]
struct State {
    child: Option<Child>,
    stopped: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Kept {
    /// Starts the program `command` makes, and keeps it running: started
    /// again as `restart` says each time it ends, while `watch` says to.
    pub fn start(
        mut command: impl FnMut() -> Command + Send + 'static,
        restart: Restart,
        mut watch: impl Watch,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        let keeping = shared.clone();
        // The child's parent, for its end with this process: this thread,
        // which lives while it does.
        let keeper = std::thread::spawn(move || {
            let mut wait = restart.after;
            loop {
                let mut next = command();
                next.stdin(Stdio::piped());
                link_secretive::process::ends_with_parent(&mut next, libc::SIGTERM);
                let begun = Instant::now();
                let how = {
                    let mut state = keeping.lock();
                    if state.stopped {
                        return;
                    }
                    match next.spawn() {
                        Ok(mut child) => {
                            watch.started(&mut child);
                            state.child = Some(child);
                            drop(state);
                            match keeping.until_ended(begun) {
                                Some(how) => how,
                                // Stopped: the stop ends it.
                                None => return,
                            }
                        }
                        Err(err) => Err(err),
                    }
                };
                if !watch.ended(&how) {
                    return;
                }
                if begun.elapsed() > restart.at_most {
                    wait = restart.after;
                }
                let state = keeping.lock();
                let (state, _) = keeping
                    .changed
                    .wait_timeout_while(state, wait, |state| !state.stopped)
                    .unwrap_or_else(|e| e.into_inner());
                if state.stopped {
                    return;
                }
                wait = (wait * 2).min(restart.at_most);
            }
        });
        Self {
            shared,
            keeper: Mutex::new(Some(keeper)),
        }
    }

    /// Its child's process ID, while it runs.
    pub fn pid(&self) -> Option<u32> {
        self.shared.lock().child.as_ref().map(Child::id)
    }

    /// Whether it's given up: its watch said not to start it again, or
    /// it was stopped.
    pub fn ended(&self) -> bool {
        self.keeper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
    }

    /// Stops keeping it: never started again, and the one running asked
    /// to end (its stdin closed, a SIGTERM), killed if it hasn't within
    /// [`END_WITHIN`], and waited for. Done when this returns; again,
    /// nothing.
    pub fn stop(&self) {
        let child = {
            let mut state = self.shared.lock();
            state.stopped = true;
            state.child.take()
        };
        self.shared.changed.notify_all();
        if let Some(child) = child {
            end(child);
        }
        let keeper = self.keeper.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(keeper) = keeper {
            let _ = keeper.join();
        }
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    /// Until the child running ends, how; none once it's stopped. Looked
    /// at often while it's new, so one that ends at once is known at once.
    fn until_ended(&self, begun: Instant) -> Option<std::io::Result<ExitStatus>> {
        loop {
            let every = match begun.elapsed() < Duration::from_secs(2) {
                true => Duration::from_millis(20),
                false => Duration::from_secs(1),
            };
            let state = self.lock();
            let (mut state, _) = self
                .changed
                .wait_timeout(state, every)
                .unwrap_or_else(|e| e.into_inner());
            if state.stopped {
                return None;
            }
            let child = state.child.as_mut()?;
            match child.try_wait() {
                Ok(None) => continue,
                Ok(Some(status)) => {
                    state.child = None;
                    return Some(Ok(status));
                }
                Err(err) => {
                    state.child = None;
                    return Some(Err(err));
                }
            }
        }
    }
}

/// Asks `child` to end, its stdin closed and a SIGTERM; kills it if it
/// hasn't within [`END_WITHIN`]; waits for it.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a stopping child until it ends"
)]
fn end(mut child: Child) {
    drop(child.stdin.take());
    // SAFETY: a signal to the one process, ours and not yet waited for,
    // so its ID is still its own.
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    let asked = Instant::now();
    while matches!(child.try_wait(), Ok(None)) {
        if asked.elapsed() > END_WITHIN {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    const SOON: Restart = Restart {
        after: Duration::from_millis(50),
        at_most: Duration::from_millis(50),
    };

    /// One that ends is started again, while its watch says so.
    #[test]
    fn it_is_started_again_until_its_watch_says_not() {
        struct Thrice(Arc<Mutex<u32>>);
        impl Watch for Thrice {
            fn ended(&mut self, _: &std::io::Result<ExitStatus>) -> bool {
                let mut ends = self.0.lock().unwrap();
                *ends += 1;
                *ends < 3
            }
        }
        let ends = Arc::new(Mutex::new(0));
        let kept = Kept::start(|| sh("exit 0"), SOON, Thrice(ends.clone()));
        link_test_kit::eventually("it to end for good", Duration::from_secs(10), || {
            kept.ended()
        });
        assert_eq!(*ends.lock().unwrap(), 3);
    }

    /// A stop ends the one running and reaps it, and it's never started
    /// again: one that ends when asked at once, and one that ignores
    /// being asked, killed.
    #[test]
    fn a_stop_ends_it_and_waits_for_it() {
        for (script, within) in [
            ("sleep 30", Duration::from_secs(5)),
            ("trap '' TERM; cat >/dev/null; sleep 30", END_WITHIN * 2),
        ] {
            let kept = Kept::start(move || sh(script), SOON, ());
            link_test_kit::eventually("it to start", Duration::from_secs(5), || {
                kept.pid().is_some()
            });
            let pid = kept.pid().unwrap();
            let start = Instant::now();
            kept.stop();
            assert!(start.elapsed() < within, "{script}: {:?}", start.elapsed());
            assert!(kept.ended() && kept.pid().is_none());
            // Reaped: no process, not even a zombie.
            assert!(
                !std::path::Path::new(&format!("/proc/{pid}")).exists()
                    || cfg!(not(target_os = "linux")),
                "{script}"
            );
        }
    }

    /// Its child's stdin is a pipe held open while it runs: closing it
    /// is one way it's asked to end.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn its_child_reads_a_pipe_held_open() {
        let kept = Kept::start(|| sh("cat; exit 3"), SOON, ());
        link_test_kit::eventually("it to start", Duration::from_secs(5), || {
            kept.pid().is_some()
        });
        std::thread::sleep(Duration::from_millis(200));
        // Still running: its stdin wasn't closed under it.
        assert!(kept.pid().is_some());
    }
}
