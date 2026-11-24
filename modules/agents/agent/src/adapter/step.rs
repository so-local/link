//! An adapter's command as it runs ([`Step`]): wrapped in its sandbox,
//! with its environment, in its home.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

/// An adapter's command as it runs: wrapped in its sandbox, with its
/// environment, in its home.
pub struct Step {
    pub(super) argv: Vec<String>,
    pub(super) env: BTreeMap<String, String>,
    pub(super) dir: PathBuf,
}

impl Step {
    pub(super) fn command(&self) -> Command {
        let mut command = link_secretive::process::command(&self.argv[0]);
        command
            .args(&self.argv[1..])
            .env_clear()
            .envs(&self.env)
            .current_dir(&self.dir);
        command
    }

    /// Whether it succeeds, its output dropped, within the time one run
    /// of a health check has ([`crate::waits::HarnessWaits::health`]): one
    /// still running then, its sandbox's start stuck, say, is ended with
    /// what it started, and hasn't succeeded.
    pub fn succeeds(&self) -> bool {
        self.succeeds_within(crate::waits::HarnessWaits::get().health())
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "polls a health check's process, which nothing announces, until its deadline"
    )]
    fn succeeds_within(&self, wait: Duration) -> bool {
        let mut command = self.command();
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let Ok(child) = link_secretive::process::OwnedChild::spawn(&mut command) else {
            return false;
        };
        let deadline = std::time::Instant::now() + wait;
        loop {
            match child.exited() {
                Ok(true) => return child.finish().is_ok_and(|s| s.success()),
                Ok(false) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                // Out of time, or no longer seen: ended, with its group.
                _ => return false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `script` run by `sh` in `dir`, which its environment names, so
    /// whatever it starts is seen as the test's ([`link_test_kit::process::left_under`]).
    fn step(script: &str, dir: &std::path::Path) -> Step {
        Step {
            argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
            env: BTreeMap::from([("STEP_DIR".into(), dir.display().to_string())]),
            dir: dir.to_path_buf(),
        }
    }

    /// A health check that succeeds, or fails, says so.
    #[test]
    fn a_health_check_says_how_it_ended() {
        let dir = link_test_kit::TempDir::new("step");
        assert!(step("exit 0", dir.path()).succeeds_within(Duration::from_secs(30)));
        assert!(!step("exit 3", dir.path()).succeeds_within(Duration::from_secs(30)));
    }

    /// One still running at its deadline, a sandbox's start stuck, say,
    /// hasn't succeeded, and is ended with what it started, even what
    /// ignores being asked to: it never holds up `lnk agent show`.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_health_check_past_its_time_is_ended() {
        let dir = link_test_kit::TempDir::new("step");
        let stuck = "trap '' TERM; sleep 600 & wait";
        // Returning at all is the proof: the test kit's deadline fails a hang.
        assert!(!step(stuck, dir.path()).succeeds_within(Duration::from_millis(300)));
        link_test_kit::wait::eventually(
            "the stuck health check's processes ended",
            Duration::from_secs(10),
            || link_test_kit::process::left_under(dir.path()).is_empty(),
        );
    }
}
