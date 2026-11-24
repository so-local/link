//! Bounded observations of an explicitly owned process. Callers launch and
//! reap it; Measure supplies the readers, units and OTLP recording format.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{Point, Report, Unit};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    sample_milliseconds: u64,
    maximum_samples: usize,
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, Write},
        process::Stdio,
    };

    #[test]
    fn final_cpu_is_observed_after_exit_before_the_owner_reaps_the_process() {
        let dir = link_test_kit::TempDir::new("measure-owned-process");
        let mut child=link_test_kit::process::ChildGroup::spawn(
            link_test_kit::env::command("sh",&dir)
                .args(["-c","printf 'ready\\n'; read release; i=0; while [ \"$i\" -lt 50000 ]; do i=$((i+1)); done"])
                .stdin(Stdio::piped()).stdout(Stdio::piped())
        ).unwrap();
        let mut ready = String::new();
        std::io::BufReader::new(child.take_stdout().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let mut capture = Capture::new(
            Unit {
                id: "owned-process".into(),
                kind: "process".into(),
                ..Unit::default()
            },
            child.id(),
        );
        capture.observe(false);
        child.take_stdin().unwrap().write_all(b"go\n").unwrap();
        link_test_kit::wait::eventually(
            "the owned process exits without reaping",
            Duration::from_secs(30),
            || super::super::process::exited_without_reaping(child.id()).unwrap(),
        );
        capture.observe(true);
        let (reports, limited) = capture.finish();
        assert!(!limited);
        let points: Vec<_> = reports.iter().flat_map(|r| &r.points).collect();
        assert!(
            points
                .iter()
                .any(|p| p.measure.name == "link.cpu.used.sum" && p.value > 0.0)
        );
        assert!(
            points
                .iter()
                .any(|p| p.measure.name == "link.memory.used.sum" && p.value > 0.0)
        );
        assert!(points.iter().all(|p| p.value.is_finite() && p.value >= 0.0));
        child.stop();
    }
}

fn settings() -> &'static Settings {
    static SETTINGS: std::sync::OnceLock<Settings> = std::sync::OnceLock::new();
    SETTINGS.get_or_init(|| {
        let value: Settings =
            toml::from_str(include_str!("run.toml")).expect("Measure run settings");
        assert!(value.sample_milliseconds > 0 && value.maximum_samples > 0);
        value
    })
}

pub struct Capture {
    unit: Unit,
    #[cfg(target_os = "linux")]
    pid: u32,
    #[cfg(target_os = "linux")]
    identity: Option<u64>,
    started: u64,
    clock: Instant,
    next: Duration,
    records: Vec<Report>,
    samples: usize,
    pub limit_reached: bool,
    peak: Option<u64>,
}

impl Capture {
    pub fn new(unit: Unit, pid: u32) -> Self {
        assert!(unit.valid(), "invalid Measure process unit");
        #[cfg(not(target_os = "linux"))]
        let _ = pid;
        Self {
            unit,
            #[cfg(target_os = "linux")]
            pid,
            #[cfg(target_os = "linux")]
            identity: None,
            started: crate::now(),
            clock: Instant::now(),
            next: Duration::ZERO,
            records: Vec::new(),
            samples: 0,
            limit_reached: false,
            peak: None,
        }
    }

    /// Sampling never waits or starts a process. A last observation before
    /// reaping may read its final CPU counters even after it becomes a zombie.
    pub fn observe(&mut self, final_observation: bool) {
        if !final_observation && self.clock.elapsed() < self.next {
            return;
        }
        self.next = self.clock.elapsed() + Duration::from_millis(settings().sample_milliseconds);
        if self.samples >= settings().maximum_samples && !final_observation {
            self.limit_reached = true;
            return;
        }
        self.sample();
    }

    /// One sample of the process tree's CPU, memory and disk, read from
    /// `/proc`.
    #[cfg(target_os = "linux")]
    fn sample(&mut self) {
        use super::process;
        let Some(root) = process::process(self.pid, false) else {
            return;
        };
        if self.identity.is_some() && root.started != self.identity {
            return;
        }
        self.identity = root.started;
        let at = crate::now();
        let mut points = vec![Point::new("link.cpu.used.sum", root.cpu, at).since(self.started)];
        // Root CPU includes waited children. Summing a live tree would
        // count those children twice when their parent reaps them.
        let tree = process::live_tree(&[self.pid]);
        let memory: Option<Vec<_>> = tree
            .into_iter()
            .filter(|pid| process::state(*pid).is_some_and(|state| state != 'Z'))
            .map(|pid| {
                process::process(pid, true)?
                    .memory
                    .filter(|value| *value > 0)
            })
            .collect();
        if let Some(memory) = memory.filter(|values| !values.is_empty()) {
            let total = memory.into_iter().fold(0u64, u64::saturating_add);
            self.peak = Some(self.peak.map_or(total, |old| old.max(total)));
            points.push(Point::new("link.memory.used.sum", total as f64, at));
        }
        // An unreadable I/O file is unknown, rather than a zero counter.
        if let Ok(io) = std::fs::read_to_string(format!("/proc/{}/io", self.pid)) {
            for (field, measure) in [
                ("read_bytes:", "link.disk.read.sum"),
                ("write_bytes:", "link.disk.written.sum"),
            ] {
                if let Some(number) = io
                    .lines()
                    .find_map(|line| line.strip_prefix(field)?.trim().parse::<u64>().ok())
                {
                    points.push(Point::new(measure, number as f64, at).since(self.started));
                }
            }
        }
        self.samples += 1;
        self.records.push(Report {
            unit: self.unit.clone(),
            points,
        });
    }

    /// Nothing to sample: only Linux shows a process tree's counters here.
    #[cfg(not(target_os = "linux"))]
    fn sample(&mut self) {}

    pub fn finish(mut self) -> (Vec<Report>, bool) {
        let at = crate::now();
        let mut points = vec![
            Point::new("link.time.run.own", self.clock.elapsed().as_secs_f64(), at)
                .since(self.started),
        ];
        if let Some(peak) = self.peak {
            points.push(Point::new("link.memory.peak.max", peak as f64, at));
        }
        self.records.push(Report {
            unit: self.unit,
            points,
        });
        (self.records, self.limit_reached)
    }
}
