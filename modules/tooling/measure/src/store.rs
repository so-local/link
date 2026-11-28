//! What the watcher keeps: every sample for a while (`fine/`, a file an
//! hour), then a summary of each minute for longer (`minutes/`, a file a
//! day), as the settings say ([`crate::settings`]). Each line is one
//! look at the machine: its time and each unit with its values. Old
//! files are deleted whole.
//!
//! A file is written short ([`Writer`]): its first line names its
//! measures (`{"measures": [...]}`, `link.` left out), and each unit's
//! values are in that order (`"v": [...]`), with `null` for one left
//! out. Left out are a zero, a running total that hasn't changed since
//! the unit's last line in the file, and the unit's description (its
//! kind, parent, name, owner, teams and start) when it hasn't either;
//! the reader ([`Reader`]) carries each forward. A file is compressed
//! (zstd, `.jsonl.zst`) once its span is over.
//!
//! A summary of a span, a day or an hour, is each measure's as its kind
//! says: the largest of a [`Kind::Gauge`] (memory in use), what a
//! [`Kind::Total`] grew by (CPU used; it starts over only when its unit
//! does), the sum of a [`Kind::Delta`] (tokens), and the median of a
//! [`Kind::Each`] (a turn's time). Every sum is a number, at most the
//! largest there is ([`link_measure_contract::client::add`]).

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use link_measure_contract::client::{Kind, MEASURES, Measure, Unit, add, find};
use serde::{Deserialize, Serialize};

use crate::sample::Measured;

mod daily;
mod disk;
pub use daily::{DAY, Days, ordered, saved as saved_days, summarize};
use disk::{OPEN, Reader, Writer, close, file_time, files, first, lines};

const HOUR: u64 = 3_600;

/// One unit in a line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<String>,
    /// When its running totals began, in nanoseconds since 1970
    /// ([`Measured::started`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<u64>,
    /// Each measure's value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub v: BTreeMap<String, f64>,
    /// Each [`Kind::Each`] measure's values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub e: BTreeMap<String, Vec<f64>>,
}

impl Kept {
    /// The unit it is.
    pub fn unit(&self) -> Unit {
        Unit {
            id: self.id.clone(),
            kind: self.kind.clone(),
            parent: self.parent.clone(),
            name: self.name.clone(),
            owner: self.owner.clone(),
            teams: self.teams.clone(),
            machine: None,
        }
    }
}

/// One line: a sample, or a minute's summary.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// Seconds since 1970.
    pub t: u64,
    pub units: Vec<Kept>,
}

fn short(name: &str) -> String {
    name.strip_prefix("link.").unwrap_or(name).to_string()
}

/// The measure a kept name is.
pub fn measure(short: &str) -> Option<&'static Measure> {
    static SHORT: std::sync::OnceLock<HashMap<&'static str, &'static Measure>> =
        std::sync::OnceLock::new();
    SHORT
        .get_or_init(|| {
            MEASURES
                .iter()
                .filter_map(|m| Some((m.name.strip_prefix("link.")?, m)))
                .collect()
        })
        .get(short)
        .copied()
}

impl Line {
    /// The units of a sample, at `t`: the machine and its agents, and
    /// each agent's processes when `processes` (the watcher's setting),
    /// else theirs are only in their agent's.
    pub fn of(t: u64, units: &[Measured], processes: bool) -> Self {
        Self {
            t,
            units: units
                .iter()
                .filter(|m| m.unit.kind == "machine" || m.unit.kind == "agent" || processes)
                .map(|m| Kept {
                    id: m.unit.id.clone(),
                    kind: m.unit.kind.clone(),
                    parent: m.unit.parent.clone(),
                    name: m.unit.name.clone(),
                    owner: m.unit.owner.clone(),
                    teams: m.unit.teams.clone(),
                    s: m.started,
                    v: m.values
                        .iter()
                        .filter(|(_, v)| v.is_finite())
                        .map(|(k, v)| (short(k), *v))
                        .collect(),
                    e: m.each.iter().map(|(k, v)| (short(k), v.clone())).collect(),
                })
                .collect(),
        }
    }
}

/// `secs` since 1970, UTC, as (year, month, day, hour).
fn civil(secs: u64) -> (i64, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let hour = (secs % 86_400 / 3_600) as u32;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day, hour)
}

/// Where the store is, its two folders under it, and what's being
/// written to each.
#[derive(Debug)]
pub struct Store {
    pub dir: PathBuf,
    fine: Writer,
    minutes: Writer,
}

impl Clone for Store {
    /// The same store, written from scratch: its next line of each file
    /// says everything again.
    fn clone(&self) -> Self {
        Self::new(self.dir.clone())
    }
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            fine: Writer::default(),
            minutes: Writer::default(),
        }
    }

    fn fine_file(&self, t: u64) -> PathBuf {
        let (y, m, d, h) = civil(t);
        self.dir
            .join("fine")
            .join(format!("{y:04}-{m:02}-{d:02}-{h:02}{OPEN}"))
    }

    fn minutes_file(&self, t: u64) -> PathBuf {
        let (y, m, d, _) = civil(t);
        self.dir
            .join("minutes")
            .join(format!("{y:04}-{m:02}-{d:02}{OPEN}"))
    }

    /// Keeps a sample.
    pub fn sample(&mut self, line: &Line) -> anyhow::Result<()> {
        let path = self.fine_file(line.t);
        self.fine.append(&path, line)
    }

    /// Keeps a minute's summary.
    pub fn minute(&mut self, line: &Line) -> anyhow::Result<()> {
        let path = self.minutes_file(line.t);
        self.minutes.append(&path, line)
    }

    /// Deletes the samples past `fine` seconds, and the minutes
    /// past `minutes`, a whole file at a time, by its name's time,
    /// and compresses each file left whose span is over.
    pub fn prune(&self, now: u64, fine: u64, minutes: u64) {
        for (folder, keep, span) in [("fine", fine, 3_600), ("minutes", minutes, 86_400)] {
            let Ok(entries) = std::fs::read_dir(self.dir.join(folder)) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some((start, open)) = file_time(&name) else {
                    continue;
                };
                if start + span + keep < now {
                    let _ = std::fs::remove_file(entry.path());
                } else if open
                    && start + span <= now
                    && let Err(err) = close(&entry.path())
                {
                    crate::problem(&format!("compressing {name}"), &err);
                }
            }
        }
    }

    /// Each line from `since` on, oldest first, read one at a time: the
    /// samples, and before the first of them, the minutes' whole.
    /// `seed` is each unit as last read before `since`, when known, so
    /// what a line left out is carried forward from it.
    pub fn each(&self, since: u64, seed: Option<&HashMap<String, Kept>>, mut f: impl FnMut(Line)) {
        let fine = files(&self.dir.join("fine"), since, 3_600);
        let first = first(&fine, since).unwrap_or(u64::MAX);
        let reader = || {
            let mut reader = Reader::default();
            if let Some(seed) = seed {
                reader.last = seed.clone();
            }
            reader
        };
        let mut minutes = reader();
        for path in files(&self.dir.join("minutes"), since, 86_400) {
            lines(&path, since, &mut minutes, seed.is_some(), |l| {
                if l.t >= since && l.t + 60 <= first {
                    f(l);
                }
            });
        }
        let mut samples = reader();
        for path in fine {
            lines(&path, since, &mut samples, seed.is_some(), |l| {
                if l.t >= since {
                    f(l);
                }
            });
        }
    }

    /// The lines from `since` on ([`Store::each`]).
    pub fn since(&self, since: u64) -> Vec<Line> {
        let mut lines = Vec::new();
        self.each(since, None, |l| lines.push(l));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn kept(v: &[(&str, f64)], e: &[(&str, &[f64])]) -> Kept {
        Kept {
            id: "a1".into(),
            kind: "agent".into(),
            s: Some(1),
            v: v.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            e: e.iter().map(|(k, v)| (k.to_string(), v.to_vec())).collect(),
            ..Kept::default()
        }
    }

    pub(super) fn line(t: u64, k: Kept) -> Line {
        Line { t, units: vec![k] }
    }

    #[test]
    fn samples_then_minutes_before_them_and_pruned() {
        let dir = link_test_kit::TempDir::new("measure-store");
        let mut store = Store::new(dir.to_path_buf());
        let day = 1_790_640_000;
        store
            .minute(&line(day + 60, kept(&[("memory.used.sum", 1.0)], &[])))
            .unwrap();
        // The minute the samples start in: they say it themselves.
        store
            .minute(&line(day + 7_200, kept(&[("memory.used.sum", 2.0)], &[])))
            .unwrap();
        store
            .sample(&line(day + 7_200, kept(&[("memory.used.sum", 3.0)], &[])))
            .unwrap();
        store
            .sample(&line(day + 7_210, kept(&[("memory.used.sum", 4.0)], &[])))
            .unwrap();
        let times: Vec<u64> = store.since(day).iter().map(|l| l.t).collect();
        assert_eq!(times, [day + 60, day + 7_200, day + 7_210]);
        assert_eq!(store.since(day)[1].units[0].v["memory.used.sum"], 3.0);
        // An hour of samples kept, a day of minutes: two days on, all gone.
        store.prune(day + 3 * 86_400, 3_600, 86_400);
        assert!(store.since(0).is_empty());
    }

    /// What's kept of a look: the machine and agents, and their
    /// processes only when the settings say.
    #[test]
    fn processes_are_kept_only_when_asked() {
        let unit = |id: &str, kind: &str| Measured {
            unit: Unit {
                id: id.into(),
                kind: kind.into(),
                ..Unit::default()
            },
            ..Measured::default()
        };
        let units = [
            unit("m1", "machine"),
            unit("a1", "agent"),
            unit("a1/12", "process"),
            unit("a1/13", "sandbox"),
        ];
        let kinds = |processes| -> Vec<String> {
            Line::of(1, &units, processes)
                .units
                .into_iter()
                .map(|k| k.kind)
                .collect()
        };
        assert_eq!(kinds(false), ["machine", "agent"]);
        assert_eq!(kinds(true), ["machine", "agent", "process", "sandbox"]);
    }

    /// An agent's teams are kept with it, in a sample and a minute's
    /// summary, and are its teams read back.
    #[test]
    fn an_agents_teams_are_kept() {
        let agent = Measured {
            unit: Unit {
                id: "a1".into(),
                kind: "agent".into(),
                teams: vec!["t1".into(), "t2".into()],
                ..Unit::default()
            },
            ..Measured::default()
        };
        let line = Line::of(10, &[agent], false);
        let text = serde_json::to_string(&line).unwrap();
        assert!(text.contains(r#""teams":["t1","t2"]"#), "{text}");
        let read: Line = serde_json::from_str(&text).unwrap();
        assert_eq!(summarize(60, &[read]).units[0].unit().teams, ["t1", "t2"]);
        // None: none written.
        let alone = Line::of(10, &[Measured::default()], false);
        assert!(!serde_json::to_string(&alone).unwrap().contains("teams"));
    }
}
