//! The watcher (`lnk measure watch`, the background service `lnk measure
//! start` runs): every so often (`every`) it looks at the machine and
//! each agent from outside ([`crate::sample`]), reads what each agent's
//! harness reported itself ([`crate::tail`]), keeps it ([`crate::store`])
//! and, when you named one, sends it to your OTLP endpoint
//! ([`crate::export`]). One runs at a time.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use link_format::{Done, Note, Out, Report as Failed, WithinExt, sentence};
use link_measure_contract::client::{self as measure, By, Kind, Point, Report};

use crate::export::Sender;
use crate::sample::{self, Agent, Measured, Sampler};
use crate::settings::{self, Settings};
use crate::store::{self, Days, Line, Store};
use crate::tail::Offsets;
use crate::view;

/// How long the list of agents is kept before it's asked for again: a
/// new agent is watched within it, one that stopped or started again
/// at the next look.
const AGENTS_EVERY: Duration = Duration::from_secs(60);

pub struct Watcher {
    sampler: Sampler,
    store: Store,
    offsets: Offsets,
    offsets_path: PathBuf,
    /// Where what proxies had counted is kept ([`Sampler::counted`]),
    /// so a restarted watcher counts none of it twice.
    counted_path: PathBuf,
    agents: Vec<Agent>,
    listed: Option<Instant>,
    /// The minute being sampled, and its samples so far.
    minute: (u64, Vec<Line>),
    /// Each unit's measures over the last day, as kept, summed up as
    /// it looks: what `lnk measure` shows ([`view::NOW`]).
    days: Option<Days>,
    /// When each unit's running totals began, by unit: OTLP's start
    /// time.
    starts: HashMap<String, (Option<u64>, u64)>,
    /// The most memory each unit used while watched.
    peaks: HashMap<String, f64>,
    /// When the last look was, in nanoseconds since 1970: where what a
    /// look counted (a proxy's tokens) starts.
    last: Option<u64>,
    sender: Option<Sender>,
}

impl Watcher {
    pub fn new(dir: PathBuf) -> Self {
        let offsets_path = dir.join("offsets.json");
        let counted_path = dir.join("counted.json");
        let mut sampler = Sampler::default();
        sampler.counted = std::fs::read(&counted_path)
            .ok()
            .and_then(|b| link_files::saved::parse_json(&b).ok())
            .unwrap_or_default();
        Self {
            sampler,
            store: Store::new(dir),
            offsets: Offsets::load(&offsets_path),
            offsets_path,
            counted_path,
            agents: Vec::new(),
            listed: None,
            minute: (0, Vec::new()),
            days: None,
            starts: HashMap::new(),
            peaks: HashMap::new(),
            last: None,
            sender: None,
        }
    }

    /// One look: sampled, kept, and sent where `settings` say.
    pub fn tick(&mut self, settings: &Settings) -> anyhow::Result<()> {
        let at = measure::now();
        let secs = at / 1_000_000_000;
        if self.listed.is_none_or(|l| l.elapsed() >= AGENTS_EVERY) || sample::changed(&self.agents)
        {
            self.agents = self.sampler.agents();
            self.listed = Some(Instant::now());
        }
        let machine =
            link_machine::installation::installation().within("reading this machine's ID")?;
        let counted = self.sampler.counted.clone();
        let mut units = self.sampler.sample(&machine, &self.agents, settings);
        if self.sampler.counted != counted
            && let Err(err) = link_files::saved::print_json(&self.sampler.counted)
                .and_then(|json| Ok(link_files::write_private(&self.counted_path, &json)?))
        {
            crate::problem("keeping what proxies counted", &err);
        }
        // What was looked at, before what harnesses reported is added.
        let mut reports = match settings.export {
            Some(_) => self.reports(&units, at),
            None => Vec::new(),
        };
        self.last = Some(at);
        reports.extend(self.harnesses(&mut units));
        let line = Line::of(secs, &units, settings.processes);
        self.store.sample(&line)?;
        self.keep_up(&units, &line, secs);
        if self.minute.0 != secs / 60 {
            if !self.minute.1.is_empty() {
                self.store
                    .minute(&store::summarize(self.minute.0 * 60, &self.minute.1))?;
                self.store.prune(
                    secs,
                    settings.keep_fine.as_secs(),
                    settings.keep_minutes.as_secs(),
                );
                // The day so far, for the views and the next watcher to
                // read on from rather than read the day back.
                if let Some(days) = &self.days
                    && let Err(err) = days.save(&self.store.dir)
                {
                    crate::problem("keeping the day", &err);
                }
            }
            self.minute = (secs / 60, Vec::new());
        }
        self.minute.1.push(line);
        if let Some(export) = &settings.export {
            if self.sender.is_none() {
                self.sender = Some(Sender::new()?);
            }
            if let Some(sender) = &self.sender
                && let Err(err) = sender.send(export, measure::otlp(&reports, "lnk-measure"))
            {
                crate::problem("sending a look on", &err);
            }
        }
        Ok(())
    }

    /// Sums `line` up into the day, read back from the store at the
    /// first look, and writes what `lnk measure` shows ([`view::NOW`]).
    fn keep_up(&mut self, units: &[Measured], line: &Line, secs: u64) {
        let since = view::day_start(secs);
        let days = self
            .days
            .get_or_insert_with(|| Days::read(&self.store, since, line.t));
        days.add(line);
        days.prune(since);
        let path = self.store.dir.join(view::NOW);
        let written = view::NOW_FORMAT.print_json(&view::json(units, days, secs));
        if let Err(err) = written.and_then(|text| Ok(link_files::write_private(&path, &text)?)) {
            crate::problem("writing its last look", &err);
        }
    }

    /// What each agent's harness reported since the last look, and the
    /// harness module timed of its starts and stops, added to its unit
    /// in `units`; as reports of the agent, to send on.
    fn harnesses(&mut self, units: &mut [Measured]) -> Vec<Report> {
        let mut reports = Vec::new();
        let before = self.offsets.clone();
        for agent in &self.agents {
            let mut points = Vec::new();
            if let Some(path) = &agent.measures {
                points.extend(self.offsets.read(path, By::Harness));
            }
            if let Some(path) = &agent.times {
                points.extend(self.offsets.read(path, By::Module));
            }
            let Some(unit) = units.iter_mut().find(|u| u.unit.id == agent.id) else {
                continue;
            };
            // Its proxy counts its tokens: not its harness too.
            if unit.metered {
                points.retain(|p| !p.measure.name.starts_with("link.tokens."));
            }
            if points.is_empty() {
                continue;
            }
            for point in &points {
                match point.measure.kind {
                    Kind::Each => unit
                        .each
                        .entry(point.measure.name)
                        .or_default()
                        .push(point.value),
                    _ => unit.add(point.measure.name, point.value),
                }
            }
            reports.push(Report {
                unit: unit.unit.clone(),
                points,
            });
        }
        if self.offsets != before
            && let Err(err) = self.offsets.save(&self.offsets_path)
        {
            crate::problem("keeping where it read", &err);
        }
        reports
    }

    /// `units` as OTLP reports, at `at`: each gauge now, each running
    /// total since it began, what was counted since the last look (a
    /// proxy's tokens), and each unit's peak while watched. What a
    /// harness counted itself goes as it reported it.
    fn reports(&mut self, units: &[Measured], at: u64) -> Vec<Report> {
        let reports = units
            .iter()
            .map(|m| {
                let mut points = Vec::new();
                for (name, value) in &m.values {
                    let Some(measure) = measure::find(name) else {
                        continue;
                    };
                    match measure.kind {
                        Kind::Gauge => points.push(Point::new(name, *value, at)),
                        Kind::Total => {
                            // From when the unit started, as it says;
                            // else from the look it was first seen.
                            let start = match self.starts.get(&m.unit.id) {
                                Some((started, start)) if *started == m.started => *start,
                                _ => m.started.unwrap_or(at),
                            };
                            self.starts.insert(m.unit.id.clone(), (m.started, start));
                            points.push(Point::new(name, *value, at).since(start));
                        }
                        // Counted from outside since the last look.
                        Kind::Delta if *value > 0.0 => {
                            points.push(Point::new(name, *value, at).since(self.last.unwrap_or(at)))
                        }
                        Kind::Delta | Kind::Each => {}
                    }
                }
                if let Some(used) = m.values.get("link.memory.used.sum") {
                    let peak = self.peaks.entry(m.unit.id.clone()).or_insert(*used);
                    *peak = peak.max(*used);
                    points.push(Point::new("link.memory.peak.max", *peak, at));
                }
                Report {
                    unit: m.unit.clone(),
                    points,
                }
            })
            .collect();
        // Nothing kept of units gone.
        let ids: std::collections::HashSet<&str> =
            units.iter().map(|m| m.unit.id.as_str()).collect();
        self.starts.retain(|id, _| ids.contains(id.as_str()));
        self.peaks.retain(|id, _| ids.contains(id.as_str()));
        reports
    }
}

/// Watches until stopped, one look every `every` of the settings, read
/// again at each look. Returns at once when another watcher runs.
#[expect(clippy::disallowed_methods, reason = "the watch's own period")]
pub fn watch(out: &mut Out) -> Result<Done, Failed> {
    let dir = settings::dir()?;
    link_files::create_private_dir(&dir).within("making its folder")?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(crate::view::WATCH_LOCK))
        .within("opening the watcher's lock")?;
    if lock.try_lock().is_err() {
        out.block(Note(sentence!("Another watcher runs here already.")));
        return Ok(Done::shown());
    }
    link_format::daemon::logs();
    let mut watcher = Watcher::new(dir);
    let mut settings = settings::load()?;
    loop {
        let started = Instant::now();
        match settings::load() {
            Ok(now) => settings = now,
            Err(err) => crate::problem("reading your settings, which are kept as they were", &err),
        }
        if let Err(err) = watcher.tick(&settings) {
            crate::problem("taking a look", &err);
        }
        std::thread::sleep(settings.every.saturating_sub(started.elapsed()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_measure_contract::client::{Unit, append, otlp};

    /// An agent whose proxy counts its tokens takes none from its
    /// harness, which still gives its turns and spend; an agent without
    /// one takes its harness's: never both.
    #[test]
    fn an_agents_tokens_come_from_one_place() {
        let dir = link_test_kit::TempDir::new("measure-one");
        let file = dir.join("measures.jsonl");
        let turn = otlp(
            &[Report {
                unit: Unit {
                    id: "a1".into(),
                    kind: "agent".into(),
                    ..Unit::default()
                },
                points: vec![
                    Point::new("link.tokens.input.sum", 1200.0, 1),
                    Point::new("link.tokens.output.sum", 80.0, 1),
                    Point::new("link.turns.sum", 1.0, 1),
                    Point::new("link.cost.sum", 4500.0, 1),
                ],
            }],
            "test",
        );
        let mut watcher = Watcher::new(dir.to_path_buf());
        watcher.agents = vec![Agent {
            id: "a1".into(),
            name: "main".into(),
            measures: Some(file.clone()),
            ..Agent::default()
        }];
        for metered in [true, false] {
            append(&file, &turn).unwrap();
            let mut unit = Measured {
                unit: Unit {
                    id: "a1".into(),
                    kind: "agent".into(),
                    ..Unit::default()
                },
                metered,
                ..Measured::default()
            };
            // What its proxy counted, as the sampler adds it.
            if metered {
                unit.add("link.tokens.input.sum", 300.0);
            }
            let mut units = vec![unit];
            let reports = watcher.harnesses(&mut units);
            let values = &units[0].values;
            let (input, output) = match metered {
                true => (300.0, None),
                false => (1200.0, Some(&80.0)),
            };
            assert_eq!(values["link.tokens.input.sum"], input, "metered: {metered}");
            assert_eq!(values.get("link.tokens.output.sum"), output);
            assert_eq!(values["link.turns.sum"], 1.0);
            assert_eq!(values["link.cost.sum"], 4500.0);
            let sent_tokens = reports
                .iter()
                .flat_map(|r| &r.points)
                .any(|p| p.measure.name.starts_with("link.tokens."));
            assert_eq!(sent_tokens, !metered);
        }
    }

    /// What the harness module timed of an agent's starts and stops goes
    /// onto the agent, each as it was; what its file says that isn't
    /// the module's to time is dropped.
    #[test]
    fn an_agents_starts_and_stops_come_from_its_times_file() {
        let dir = link_test_kit::TempDir::new("measure-times");
        let file = dir.join("times.jsonl");
        let timed = |points| {
            otlp(
                &[Report {
                    unit: Unit {
                        id: "a1".into(),
                        kind: "agent".into(),
                        ..Unit::default()
                    },
                    points,
                }],
                "test",
            )
        };
        append(
            &file,
            &timed(vec![
                Point::new("link.time.start_cold.own", 4.5, 1),
                Point::new("link.turns.sum", 1.0, 1),
            ]),
        )
        .unwrap();
        append(
            &file,
            &timed(vec![Point::new("link.time.stop.own", 0.3, 2)]),
        )
        .unwrap();
        append(
            &file,
            &timed(vec![Point::new("link.time.start_warm.own", 1.25, 3)]),
        )
        .unwrap();
        let mut watcher = Watcher::new(dir.to_path_buf());
        watcher.agents = vec![Agent {
            id: "a1".into(),
            name: "main".into(),
            times: Some(file.clone()),
            ..Agent::default()
        }];
        let agent = || Measured {
            unit: Unit {
                id: "a1".into(),
                kind: "agent".into(),
                ..Unit::default()
            },
            ..Measured::default()
        };
        let mut units = vec![agent()];
        let reports = watcher.harnesses(&mut units);
        let each = &units[0].each;
        assert_eq!(each["link.time.start_cold.own"], [4.5]);
        assert_eq!(each["link.time.start_warm.own"], [1.25]);
        assert_eq!(each["link.time.stop.own"], [0.3]);
        assert!(!units[0].values.contains_key("link.turns.sum"));
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].points.len(), 3);
        // Read once.
        let mut units = vec![agent()];
        assert!(watcher.harnesses(&mut units).is_empty());
        assert!(units[0].each.is_empty());
    }
}
