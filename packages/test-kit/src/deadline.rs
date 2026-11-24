//! A whole test's deadline: one time, set as the test starts, that every
//! wait of the test and every program it runs to its end is bounded by,
//! so a test that hangs anywhere still ends by then, failing with what it
//! was waiting for and what the program said so far.
//!
//! A panic past it unwinds as any other: what the test owns (its folders,
//! its process groups) is cleaned up by its drops. A program run past it
//! is stopped with its process group first.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::wait::{Check, TimedOut, scaled, within_scaled};

/// A test's deadline, cheap to clone into its threads.
#[derive(Clone, Debug)]
pub struct Deadline {
    what: String,
    until: Instant,
}

impl Deadline {
    /// `what`'s deadline, `total` (scaled, as every wait's) from now.
    pub fn new(what: &str, total: Duration) -> Self {
        Deadline {
            what: what.into(),
            until: Instant::now() + scaled(total),
        }
    }

    /// What's left of it.
    pub fn left(&self) -> Duration {
        self.until.saturating_duration_since(Instant::now())
    }

    /// [`crate::wait::within`], ending by this deadline if it comes first.
    pub fn within<C: Check>(
        &self,
        what: &str,
        deadline: Duration,
        check: impl FnMut() -> C,
    ) -> Result<C::Value, TimedOut> {
        within_scaled(what, scaled(deadline).min(self.left()), check)
    }

    /// [`crate::wait::eventually`], ending by this deadline if it comes
    /// first.
    #[track_caller]
    pub fn eventually<C: Check>(
        &self,
        what: &str,
        deadline: Duration,
        check: impl FnMut() -> C,
    ) -> C::Value {
        self.within(what, deadline, check)
            .unwrap_or_else(|timed_out| panic!("{timed_out}"))
    }

    /// `command` run to its end, its streams read, as `output()` runs it;
    /// past the deadline, stopped with its process group (it runs in one
    /// of its own) and a panic naming it and what it said.
    #[track_caller]
    pub fn output(&self, command: &mut Command) -> Output {
        use std::os::unix::process::CommandExt;
        let mut child = command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|err| panic!("running {command:?}: {err}"));
        let read = |stream: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut stream) = stream {
                    let _ = stream.read_to_end(&mut bytes);
                }
                bytes
            })
        };
        let stdout = read(
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        );
        let stderr = read(
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        );
        let status = within_scaled(&format!("{command:?}"), self.left(), || {
            child
                .try_wait()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "running".to_string())
        });
        let status = match status {
            Ok(status) => status,
            Err(timed_out) => {
                crate::process::kill_group(&mut child);
                let said = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
                panic!(
                    "past the deadline of {}: {timed_out}\nit said: {}{}",
                    self.what,
                    said(stdout.join().unwrap_or_default()),
                    said(stderr.join().unwrap_or_default())
                );
            }
        };
        Output {
            status,
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_program_ending_in_time_is_run_as_output_runs_it() {
        let home = crate::TempDir::new("deadline");
        let deadline = Deadline::new("the test", Duration::from_secs(30));
        let output = deadline.output(crate::env::command("sh", &home).args(["-c", "echo said"]));
        assert!(output.status.success());
        assert_eq!(output.stdout, b"said\n");
    }

    #[test]
    #[should_panic(expected = "past the deadline of the test")]
    fn a_program_past_the_deadline_is_stopped() {
        let home = crate::TempDir::new("deadline");
        let deadline = Deadline::new("the test", Duration::from_millis(200));
        deadline.output(crate::env::command("sh", &home).args(["-c", "echo said; sleep 30"]));
    }

    #[test]
    fn a_wait_ends_by_the_deadline() {
        let deadline = Deadline::new("the test", Duration::from_millis(100));
        let start = Instant::now();
        let waited = deadline.within("never", Duration::from_secs(30), || false);
        assert!(waited.is_err());
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}
