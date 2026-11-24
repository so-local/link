//! How long an agent's starts and stops take, as the harness module
//! times them. It starts and stops each harness, so it reports these as
//! a harness reports its own measures: a line of OTLP each, appended to
//! a file of the agent's settings (`link_measure_contract::client::TIMES`), where
//! no harness writes, which the measure module reads.
//!
//! A start is timed from its harness's service started until its health
//! check first answers; it's from cold when nothing of the agent was
//! timed since the machine started, else warm. A stop is timed from when
//! it's asked until none of the main processes it had is left. A start in
//! a terminal (`--foreground`) is timed the same way, until its health
//! check answers, and its stop by `lnk agent stop` until the terminal lets
//! go of its lock; Ctrl-C in that terminal, and the system's own start
//! of a service at boot or login, aren't: nothing waits for them here.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use link_measure_contract::client::{self as measure, Point, Report, Unit};

use crate::settings::{self, Settings};

/// How long a stop is waited for once its service says it's stopped:
/// past it, the stop isn't timed.
const STOP_WAIT: Duration = Duration::from_secs(10);

/// Agent `name`'s file of times.
pub fn path_of(name: &str) -> PathBuf {
    settings::config_of(name).join(measure::TIMES)
}

/// Whether a start now is from cold, by the file of times at `path`:
/// nothing of the agent timed since the machine started at `booted`
/// (nanoseconds since 1970), or ever.
pub fn cold(path: &Path, booted: Option<u64>) -> bool {
    let written = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64);
    match (written, booted) {
        (None, _) => true,
        (Some(written), Some(booted)) => written < booted,
        (Some(_), None) => false,
    }
}

/// Appends `seconds` of measure `name` (one the harness module says) to
/// agent `agent`'s file at `path`. Best effort: a start or stop never
/// fails for it.
fn keep(path: &Path, agent: &str, id: Option<&str>, name: &str, seconds: f64) {
    let unit = Unit {
        id: id.unwrap_or(agent).to_string(),
        kind: "agent".into(),
        name: Some(agent.to_string()),
        ..Unit::default()
    };
    let points = vec![Point::new(name, seconds, measure::now())];
    let request = measure::otlp(&[Report { unit, points }], "lnk-agent");
    let _ = measure::append(path, &request);
}

/// A start being timed, from its harness's service started.
pub struct Starting {
    at: Instant,
    cold: bool,
}

impl Starting {
    /// Its harness's service is about to start.
    pub fn begin(agent: &Settings) -> Self {
        Self {
            at: Instant::now(),
            cold: cold(&path_of(&agent.name), measure::booted()),
        }
    }

    /// It answered: how long it took, kept.
    pub fn answered(self, agent: &Settings) {
        let name = match self.cold {
            true => "link.time.start_cold.own",
            false => "link.time.start_warm.own",
        };
        let path = path_of(&agent.name);
        keep(
            &path,
            &agent.name,
            agent.agent_id(),
            name,
            self.at.elapsed().as_secs_f64(),
        );
    }
}

/// A stop being timed, from when it's asked.
pub struct Stopping {
    at: Instant,
    pids: Vec<u32>,
}

impl Stopping {
    /// Agent `agent`'s service is about to be stopped, its main processes
    /// `pids` (its own service's, its connections'): none, nothing to time.
    pub fn begin(pids: Vec<u32>) -> Self {
        Self {
            at: Instant::now(),
            pids,
        }
    }

    /// Agent `agent`'s service says it stopped: once none of its
    /// processes is left, how long that took, kept.
    pub fn stopped(self, agent: &str) {
        if self.pids.is_empty() {
            return;
        }
        let id = Settings::load(agent)
            .ok()
            .and_then(|s| s.agent_id().map(str::to_string));
        self.stopped_at(&path_of(agent), agent, id.as_deref());
    }

    /// Agent `agent` has stopped, as its caller saw: how long that took,
    /// kept now. For a terminal's agent, whose lock lets go as it ends.
    pub fn ended(self, agent: &str) {
        let id = Settings::load(agent)
            .ok()
            .and_then(|s| s.agent_id().map(str::to_string));
        keep(
            &path_of(agent),
            agent,
            id.as_deref(),
            "link.time.stop.own",
            self.at.elapsed().as_secs_f64(),
        );
    }

    /// [`Stopping::stopped`], kept in the file at `path`.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls stopping processes until they end"
    )]
    fn stopped_at(self, path: &Path, agent: &str, id: Option<&str>) {
        if self.pids.is_empty() {
            return;
        }
        let waited = Instant::now();
        while self.pids.iter().any(|pid| alive(*pid)) {
            if waited.elapsed() >= STOP_WAIT {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        keep(
            path,
            agent,
            id,
            "link.time.stop.own",
            self.at.elapsed().as_secs_f64(),
        );
    }
}

/// Whether process `pid` runs.
fn alive(pid: u32) -> bool {
    let Ok(id) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process is there.
    id > 0
        && (unsafe { libc::kill(id, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> link_test_kit::TempDir {
        link_test_kit::TempDir::new(&format!("times-{name}"))
    }

    /// The points kept in the file at `path`, as the measure module reads
    /// them: each measure's name and value.
    fn kept(path: &Path) -> Vec<(&'static str, f64)> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .flat_map(|l| measure::read_otlp(&serde_json::from_str(l).unwrap()))
            .flat_map(|r| r.points)
            .map(|p| (p.measure.name, p.value))
            .collect()
    }

    /// From cold while nothing of the agent was timed since the machine
    /// started; warm once something was.
    #[test]
    fn a_start_is_cold_until_the_agent_was_timed_since_the_machine_started() {
        let dir = dir("cold");
        let path = dir.join(measure::TIMES);
        assert!(cold(&path, measure::booted()));
        keep(&path, "main", Some("a1"), "link.time.start_cold.own", 1.5);
        assert!(!cold(&path, measure::booted()));
        // Written before the machine started: cold again.
        assert!(cold(&path, Some(measure::now() + 60_000_000_000)));
        assert_eq!(kept(&path), [("link.time.start_cold.own", 1.5)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A stop is kept once its processes are gone, and not at all with
    /// none to wait for.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_stop_is_timed_until_its_processes_are_gone() {
        let dir = dir("stop");
        let path = dir.join(measure::TIMES);
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let stopping = Stopping::begin(vec![child.id()]);
        child.kill().unwrap();
        child.wait().unwrap();
        stopping.stopped_at(&path, "main", Some("a1"));
        let times = kept(&path);
        assert_eq!(times.len(), 1);
        assert_eq!(times[0].0, "link.time.stop.own");
        assert!(times[0].1 >= 0.0 && times[0].1 < 5.0, "{times:?}");
        Stopping::begin(Vec::new()).stopped_at(&path, "main", Some("a1"));
        assert_eq!(kept(&path).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
