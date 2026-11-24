//! Small observation records shared by controlled Rust fixtures. They
//! never launch targets or a tracer, and never record arguments or content.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    pub started: Option<u64>,
    #[serde(default)]
    pub state: Option<char>,
    /// Only a known program label, never its arguments or arbitrary name.
    pub program: Option<String>,
    /// Cumulative core seconds, including children this process waited
    /// for. Do not sum a tree: waited-child CPU can overlap descendants.
    pub cpu_with_waited_children_seconds: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Phase {
    pub name: String,
    pub unix_ns: u64,
    pub elapsed_ms: f64,
    pub roots: Vec<u32>,
    /// A best-effort live tree. Detached descendants require explicit
    /// roots; missing processes/counters are not proof of zero use.
    pub processes: Vec<Process>,
    #[serde(default)]
    pub resident_pids: Option<usize>,
}

impl Phase {
    #[cfg(target_os = "linux")]
    pub fn linux(name: &str, roots: &[u32], elapsed: std::time::Duration) -> Self {
        use super::process;
        let processes: Vec<Process> = process::tree(roots, process::alive, process::children)
            .into_iter()
            .filter_map(|pid| process::process(pid, false))
            .map(|p| Process {
                pid: p.pid,
                parent: p.parent,
                started: p.started,
                state: process::state(p.pid),
                program: crate::program(&p.name).map(str::to_string),
                cpu_with_waited_children_seconds: p.cpu,
            })
            .collect();
        let resident_pids = processes
            .iter()
            .all(|p| p.state.is_some())
            .then(|| processes.iter().filter(|p| p.state != Some('Z')).count());
        Self {
            name: name.into(),
            unix_ns: super::now(),
            elapsed_ms: elapsed.as_secs_f64() * 1000.0,
            roots: roots.to_vec(),
            processes,
            resident_pids,
        }
    }
}

/// Successful program images in an explicitly traced time window.
/// Executing PIDs aren't a count of every fork or resident processes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Executions {
    pub executing_pids: usize,
    pub execs: usize,
}

impl Executions {
    /// Read `strace -f -ttt -e trace=process` from a controlled run.
    /// Include resumed execs, exclude explicit observer identities and
    /// failed execs. A caller keeps tracing separate from timed runs.
    pub fn in_window(trace: &str, from_ns: u64, to_ns: u64, observers: &[u32]) -> Option<Self> {
        if from_ns > to_ns {
            return None;
        }
        let (mut pids, mut execs, mut seen) = (BTreeSet::new(), 0, false);
        for line in trace.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (at, call) = rest.trim_start().split_once(char::is_whitespace)?;
            let (Ok(pid), Some(at)) = (pid.parse::<u32>(), timestamp(at)) else {
                return None;
            };
            seen = true;
            let call = call.trim_start();
            if at < from_ns || at > to_ns || observers.contains(&pid) {
                continue;
            }
            let exec = call.starts_with("execve(")
                || call.starts_with("execveat(")
                || call.starts_with("<... execve resumed>")
                || call.starts_with("<... execveat resumed>");
            if exec
                && call
                    .rsplit_once("= ")
                    .is_some_and(|(_, result)| result.trim() == "0")
            {
                pids.insert(pid);
                execs += 1;
            }
        }
        seen.then_some(Self {
            executing_pids: pids.len(),
            execs,
        })
    }
}

fn timestamp(text: &str) -> Option<u64> {
    let (seconds, fraction) = text.split_once('.')?;
    if fraction.len() > 9 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let nanos = fraction.parse::<u64>().ok()? * 10u64.pow(9 - fraction.len() as u32);
    seconds
        .parse::<u64>()
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_resumed_execs_without_counting_failed_images_or_observers() {
        let trace = " 7  100.000001 execve(\"lnk\", [], []) = 0\n\
7 100.000002 execve(\"module\", [], [] <unfinished ...>\n\
7 100.000003 <... execve resumed>) = 0\n\
8 100.000004 execveat(3, \"\", [], [], 0) = 0\n\
9 100.000005 execve(\"missing\", [], []) = -1 ENOENT\n\
10 100.000006 execve(\"observer\", [], []) = 0\n";
        assert_eq!(
            Executions::in_window(trace, 100_000_001_000, 100_000_009_000, &[10]),
            Some(Executions {
                executing_pids: 2,
                execs: 3
            })
        );
        assert_eq!(
            Executions::in_window(trace, 101_000_000_000, 102_000_000_000, &[]),
            Some(Executions::default())
        );
        assert_eq!(Executions::in_window("", 0, 1, &[]), None);
        assert_eq!(Executions::in_window("malformed", 0, 1, &[]), None);
    }
}
