//! Waiting for what a test needs, the one way: poll with backoff until a
//! deadline, then fail naming what was awaited, how long, and the last
//! value seen.
//!
//! Every deadline is scaled by one setting, `LNK_TEST_TIME_SCALE` (a
//! positive number, 1 when unset), so a loaded machine gives every wait
//! more time at once and no test picks its own slack.

use std::fmt::Debug;
use std::future::Future;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The setting every deadline is multiplied by.
pub const TIME_SCALE: &str = "LNK_TEST_TIME_SCALE";

/// The first pause between checks; each next one doubles, to [`MOST`].
const FIRST: Duration = Duration::from_millis(5);
/// The longest pause between checks.
const MOST: Duration = Duration::from_millis(200);

/// What a check returns: done with a value, or not yet with what it saw.
pub trait Check {
    type Value;
    /// The value when done, else what was seen, said for the failure.
    fn checked(self) -> Result<Self::Value, String>;
}

impl Check for bool {
    type Value = ();

    fn checked(self) -> Result<(), String> {
        if self { Ok(()) } else { Err("false".into()) }
    }
}

impl<T> Check for Option<T> {
    type Value = T;

    fn checked(self) -> Result<T, String> {
        self.ok_or_else(|| "None".into())
    }
}

impl<T, E: Debug> Check for Result<T, E> {
    type Value = T;

    fn checked(self) -> Result<T, String> {
        self.map_err(|err| format!("{err:?}"))
    }
}

/// A wait that reached its deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedOut {
    /// What was awaited.
    pub what: String,
    /// How long it was awaited, the scaled deadline or a moment past it.
    pub waited: Duration,
    /// What the last check saw.
    pub last: String,
}

impl std::fmt::Display for TimedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "waited {:?} for {}; the last check saw: {}",
            self.waited, self.what, self.last
        )
    }
}

impl std::error::Error for TimedOut {}

/// `deadline` times the machine's [`TIME_SCALE`].
pub fn scaled(deadline: Duration) -> Duration {
    deadline.mul_f64(scale())
}

fn scale() -> f64 {
    static SCALE: OnceLock<f64> = OnceLock::new();
    *SCALE.get_or_init(|| parse_scale(std::env::var(TIME_SCALE).ok().as_deref()))
}

fn parse_scale(value: Option<&str>) -> f64 {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return 1.0;
    };
    match value.parse::<f64>() {
        Ok(scale) if scale.is_finite() && scale > 0.0 => scale,
        _ => panic!("{TIME_SCALE} must be a positive number, not {value:?}"),
    }
}

/// Polls `check` until it's done, and returns its value; panics with
/// [`TimedOut`] past `deadline` (scaled).
#[track_caller]
pub fn eventually<C: Check>(what: &str, deadline: Duration, check: impl FnMut() -> C) -> C::Value {
    within(what, deadline, check).unwrap_or_else(|timed_out| panic!("{timed_out}"))
}

/// As [`eventually`], but the deadline passing is an error for the
/// caller, to say more or to expect it.
pub fn within<C: Check>(
    what: &str,
    deadline: Duration,
    check: impl FnMut() -> C,
) -> Result<C::Value, TimedOut> {
    within_scaled(what, scaled(deadline), check)
}

/// [`within`], its deadline scaled already.
#[expect(
    clippy::disallowed_methods,
    reason = "the kit's own wait, which every test's waiting goes through"
)]
pub(crate) fn within_scaled<C: Check>(
    what: &str,
    deadline: Duration,
    mut check: impl FnMut() -> C,
) -> Result<C::Value, TimedOut> {
    let mut poll = Poll {
        start: Instant::now(),
        deadline,
        pause: FIRST,
    };
    loop {
        let last = match check().checked() {
            Ok(value) => return Ok(value),
            Err(last) => last,
        };
        match poll.next() {
            Some(pause) => std::thread::sleep(pause),
            None => return Err(poll.timed_out(what, last)),
        }
    }
}

/// [`eventually`], in an async test: the pauses await, so the runtime
/// goes on serving what the test waits for.
pub async fn eventually_async<C: Check>(
    what: &str,
    deadline: Duration,
    check: impl FnMut() -> C,
) -> C::Value {
    match within_async(what, deadline, check).await {
        Ok(value) => value,
        Err(timed_out) => panic!("{timed_out}"),
    }
}

/// [`within`], in an async test.
pub async fn within_async<C: Check>(
    what: &str,
    deadline: Duration,
    mut check: impl FnMut() -> C,
) -> Result<C::Value, TimedOut> {
    within_await(what, deadline, || std::future::ready(check())).await
}

/// [`eventually_async`], for a check that is itself awaited, such as a
/// request to a server: each check runs to its end before the next.
pub async fn eventually_await<C: Check, F: Future<Output = C>>(
    what: &str,
    deadline: Duration,
    check: impl FnMut() -> F,
) -> C::Value {
    match within_await(what, deadline, check).await {
        Ok(value) => value,
        Err(timed_out) => panic!("{timed_out}"),
    }
}

/// [`within_async`], for a check that is itself awaited.
#[expect(
    clippy::disallowed_methods,
    reason = "the kit's own wait, which every test's waiting goes through"
)]
pub async fn within_await<C: Check, F: Future<Output = C>>(
    what: &str,
    deadline: Duration,
    mut check: impl FnMut() -> F,
) -> Result<C::Value, TimedOut> {
    let mut poll = Poll::new(deadline);
    loop {
        let last = match check().await.checked() {
            Ok(value) => return Ok(value),
            Err(last) => last,
        };
        match poll.next() {
            Some(pause) => tokio::time::sleep(pause).await,
            None => return Err(poll.timed_out(what, last)),
        }
    }
}

/// Waits for `child` to exit, and returns how it did.
#[track_caller]
pub fn exited(what: &str, child: &mut Child, deadline: Duration) -> ExitStatus {
    eventually(what, deadline, || match child.try_wait() {
        Ok(Some(status)) => Ok(status),
        Ok(None) => Err("running".to_string()),
        Err(err) => Err(err.to_string()),
    })
}

/// Waits for `address` to accept a TCP connection.
#[track_caller]
pub fn accepting(what: &str, address: std::net::SocketAddr, deadline: Duration) {
    eventually(what, deadline, || {
        std::net::TcpStream::connect(address).map(drop)
    });
}

/// Waits for the Unix socket at `path` to accept a connection.
#[cfg(unix)]
#[track_caller]
pub fn accepting_unix(what: &str, path: &Path, deadline: Duration) {
    eventually(what, deadline, || {
        std::os::unix::net::UnixStream::connect(path).map(drop)
    });
}

/// Waits for `path` to exist.
#[track_caller]
pub fn appears(what: &str, path: &Path, deadline: Duration) {
    eventually(what, deadline, || {
        path.try_exists()
            .map_err(|err| err.to_string())
            .and_then(|there| there.then_some(()).ok_or("absent".to_string()))
    });
}

/// The pauses of one wait: doubling from [`FIRST`] to [`MOST`], and none
/// that ends past the deadline but the last, which ends at it.
struct Poll {
    start: Instant,
    deadline: Duration,
    pause: Duration,
}

impl Poll {
    fn new(deadline: Duration) -> Poll {
        Poll {
            start: Instant::now(),
            deadline: scaled(deadline),
            pause: FIRST,
        }
    }

    /// The next pause, or none when the deadline has passed.
    fn next(&mut self) -> Option<Duration> {
        let left = self.deadline.checked_sub(self.start.elapsed())?;
        if left.is_zero() {
            return None;
        }
        let pause = self.pause.min(left);
        self.pause = (self.pause * 2).min(MOST);
        Some(pause)
    }

    fn timed_out(&self, what: &str, last: String) -> TimedOut {
        TimedOut {
            what: what.into(),
            waited: self.start.elapsed(),
            last,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_done_check_returns_its_value() {
        let mut calls = 0;
        let value = eventually("the third call", Duration::from_secs(10), || {
            calls += 1;
            (calls == 3).then_some(calls)
        });
        assert_eq!(value, 3);
    }

    #[test]
    fn a_timeout_names_what_how_long_and_the_last_seen() {
        let mut n = 0;
        let timed_out = within("a number past ten", Duration::from_millis(50), || {
            n += 1;
            if n > 1_000_000 {
                Ok(n)
            } else {
                Err(format!("only {n}"))
            }
        })
        .unwrap_err();
        assert_eq!(timed_out.what, "a number past ten");
        assert!(timed_out.waited >= scaled(Duration::from_millis(50)));
        assert_eq!(timed_out.last, format!("\"only {n}\""));
        let said = timed_out.to_string();
        assert!(said.starts_with("waited "), "{said}");
        assert!(said.contains("for a number past ten"), "{said}");
    }

    #[test]
    #[should_panic(expected = "for never; the last check saw: false")]
    fn eventually_fails_on_timeout() {
        eventually("never", Duration::from_millis(10), || false);
    }

    #[test]
    fn pauses_double_and_end_at_the_deadline() {
        let mut poll = Poll {
            start: Instant::now(),
            deadline: Duration::from_secs(3600),
            pause: FIRST,
        };
        let pauses: Vec<_> = (0..8).map(|_| poll.next().unwrap()).collect();
        assert_eq!(pauses[0], FIRST);
        assert_eq!(pauses[1], FIRST * 2);
        assert_eq!(*pauses.last().unwrap(), MOST);
        let mut over = Poll {
            start: Instant::now() - Duration::from_secs(2),
            deadline: Duration::from_secs(1),
            pause: FIRST,
        };
        assert_eq!(over.next(), None);
    }

    #[test]
    fn the_scale_is_a_positive_number_or_one() {
        assert_eq!(parse_scale(None), 1.0);
        assert_eq!(parse_scale(Some("")), 1.0);
        assert_eq!(parse_scale(Some("2.5")), 2.5);
        for bad in ["0", "-1", "inf", "NaN", "slow"] {
            assert!(
                std::panic::catch_unwind(|| parse_scale(Some(bad))).is_err(),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn an_async_wait_lets_the_runtime_serve() {
        let (send, mut got) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move { send.send(7).unwrap() });
        let value = eventually_async("the task's value", Duration::from_secs(10), || {
            got.try_recv()
        })
        .await;
        assert_eq!(value, 7);
    }

    #[tokio::test]
    async fn an_awaited_check_runs_to_its_end_each_time() {
        let mut calls = 0;
        let value = eventually_await("the third call", Duration::from_secs(10), || {
            calls += 1;
            let call = calls;
            async move {
                tokio::task::yield_now().await;
                (call == 3).then_some(call)
            }
        })
        .await;
        assert_eq!(value, 3);
        let timed_out = within_await("never", Duration::from_millis(10), || async { false })
            .await
            .unwrap_err();
        assert_eq!(timed_out.last, "false");
    }

    #[test]
    fn a_file_a_socket_and_an_exit_are_awaited() {
        let dir = crate::TempDir::new("wait-built-on");
        let path = dir.join("later");
        let writer = {
            let path = path.clone();
            std::thread::spawn(move || std::fs::write(path, "x").unwrap())
        };
        appears("the file", &path, Duration::from_secs(10));
        writer.join().unwrap();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        accepting(
            "the listener",
            listener.local_addr().unwrap(),
            Duration::from_secs(10),
        );

        #[cfg(unix)]
        {
            let socket = dir.join("s.sock");
            let _held = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            accepting_unix("the socket", &socket, Duration::from_secs(10));
            let mut child = link_secretive::process::command("true").spawn().unwrap();
            assert!(exited("true", &mut child, Duration::from_secs(10)).success());
        }
    }
}
