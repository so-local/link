//! Summing up: a minute's summary ([`summarize`]) and each unit's last
//! day, an hour at a time ([`Days`]).

use super::*;

/// A minute's summary of `samples`, at `t`: per unit, each gauge's
/// largest, each total's last, each delta's sum, every each's values.
pub fn summarize(t: u64, samples: &[Line]) -> Line {
    let mut units: BTreeMap<String, Kept> = BTreeMap::new();
    for line in samples {
        for kept in &line.units {
            let unit = units.entry(kept.id.clone()).or_insert_with(|| Kept {
                id: kept.id.clone(),
                kind: kept.kind.clone(),
                parent: kept.parent.clone(),
                name: kept.name.clone(),
                owner: kept.owner.clone(),
                teams: kept.teams.clone(),
                ..Kept::default()
            });
            unit.s = kept.s;
            for (name, value) in &kept.v {
                let Some(m) = measure(name) else { continue };
                let entry = unit.v.entry(name.clone()).or_insert(match m.kind {
                    Kind::Delta => 0.0,
                    _ => *value,
                });
                *entry = match m.kind {
                    Kind::Gauge | Kind::Each => entry.max(*value),
                    Kind::Total => *value,
                    Kind::Delta => add(*entry, *value),
                };
            }
            for (name, values) in &kept.e {
                unit.e.entry(name.clone()).or_default().extend(values);
            }
        }
    }
    for unit in units.values_mut() {
        for values in unit.e.values_mut() {
            spread(values);
        }
    }
    Line {
        t,
        units: units.into_values().collect(),
    }
}

/// The most values of a [`Kind::Each`] measure a minute's summary keeps,
/// and an hour of [`Days`]: past it, a spread of them stands for all.
const EACH_KEPT: usize = 64;

/// `values`, when more than [`EACH_KEPT`]: that many, evenly spread from
/// the least to the most, so their median is near all of theirs.
fn spread(values: &mut Vec<f64>) {
    if values.len() <= EACH_KEPT {
        return;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    *values = (0..EACH_KEPT)
        .map(|i| values[i * (n - 1) / (EACH_KEPT - 1)])
        .collect();
}

/// Values of a [`Kind::Each`] measure, each with how many it stands for,
/// at most [`EACH_KEPT`] of them however many come: past it, the two
/// nearest are taken as one, so a median stays near.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Sample(Vec<(f64, u64)>);

impl Sample {
    fn add(&mut self, value: f64, weight: u64) {
        self.0.push((value, weight));
        if self.0.len() > 2 * EACH_KEPT {
            self.shrink();
        }
    }

    /// Down to [`EACH_KEPT`]: sorted, and each pair of neighbours taken
    /// as one, at their mean by what each stands for.
    fn shrink(&mut self) {
        self.0.sort_by(|a, b| a.0.total_cmp(&b.0));
        while self.0.len() > EACH_KEPT {
            self.0 = self
                .0
                .chunks(2)
                .map(|pair| match pair {
                    [a, b] => {
                        let weight = a.1 + b.1;
                        let mean = a.0 + (b.0 - a.0) * (b.1 as f64 / weight as f64);
                        (mean, weight)
                    }
                    [a] => *a,
                    _ => unreachable!(),
                })
                .collect();
        }
    }

    /// The value half of what it stands for is at or under.
    fn median(&self) -> Option<f64> {
        let mut values = self.0.clone();
        values.sort_by(|a, b| a.0.total_cmp(&b.0));
        let total: u64 = values.iter().map(|v| v.1).sum();
        let mut seen = 0;
        values.into_iter().find_map(|(value, weight)| {
            seen += weight;
            (seen > total / 2).then_some(value)
        })
    }
}

/// One unit's measures over an hour, as they're summed.
#[derive(Clone, Debug, Default)]
struct Sums {
    v: BTreeMap<&'static str, f64>,
    e: BTreeMap<&'static str, Sample>,
}

impl Sums {
    fn merge(&mut self, other: &Sums) {
        for (name, value) in &other.v {
            let Some(m) = find(name) else { continue };
            match self.v.get_mut(name) {
                None => {
                    self.v.insert(m.name, *value);
                }
                Some(sum) => {
                    *sum = match m.kind {
                        Kind::Gauge | Kind::Each => sum.max(*value),
                        Kind::Total | Kind::Delta => add(*sum, *value),
                    }
                }
            }
        }
        for (name, values) in &other.e {
            let sample = self.e.entry(name).or_default();
            for (value, weight) in &values.0 {
                sample.add(*value, *weight);
            }
        }
    }

    /// Each measure as its kind sums a span up: an each's median.
    fn summed(&self) -> BTreeMap<&'static str, f64> {
        let mut out = self.v.clone();
        for (name, values) in &self.e {
            if let Some(median) = values.median() {
                out.insert(name, median);
            }
        }
        out
    }
}

/// Each unit's measures, an hour at a time, summed up line by line
/// ([`Days::add`]), as their kinds say: what `lnk measure` shows of the
/// last day. The watcher keeps one up as it looks; reading the store
/// back builds the same.
#[derive(Default)]
pub struct Days {
    hours: BTreeMap<u64, HashMap<String, Sums>>,
    /// Each unit as last seen: what it's called, its totals, and when
    /// they began, to tell what they grew by.
    pub(super) last: HashMap<String, Kept>,
    /// The time of the latest line summed up.
    through: u64,
}

/// [`Days`] as the watcher keeps them ([`DAY`]): what a view or a watcher
/// starting reads, then only the lines after `through`, not the day's.
#[derive(Default, Serialize, Deserialize)]
struct KeptDays {
    through: u64,
    hours: BTreeMap<u64, BTreeMap<String, KeptSums>>,
    last: Vec<Kept>,
}

link_files::specification!(KeptDays, include_str!("daily/day.schema.toml"));

#[derive(Default, Serialize, Deserialize)]
struct KeptSums {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    v: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    e: BTreeMap<String, Sample>,
}

/// The watcher's [`Days`] as of its last minute, in its folder. Derived:
/// without it, or past it, the day is read from the store.
pub const DAY: &str = "day.json";

impl Days {
    /// The last day from `since`, of the lines before `before`: from
    /// what the watcher kept ([`DAY`]) and the lines after it, or, when
    /// it holds nothing from `since` on, from every line since.
    pub fn read(store: &Store, since: u64, before: u64) -> Days {
        let kept = std::fs::read(store.dir.join(DAY))
            .ok()
            .and_then(|bytes| link_files::saved::parse_json::<KeptDays>(&bytes).ok())
            .filter(|kept| kept.through >= since && kept.through < before);
        let mut days = match kept {
            Some(kept) => Days::from(kept),
            None => Days::default(),
        };
        days.prune(since);
        let after = days.through;
        let from = match after {
            0 => since,
            after => since.max(after + 1),
        };
        // Each unit as kept: what the lines after leave out, carried on.
        let seed = (after != 0).then(|| days.last.clone());
        store.each(from, seed.as_ref(), |line| {
            if line.t < before && (after == 0 || line.t > after) {
                days.add(&line);
            }
        });
        days
    }

    /// Keeps these days in `dir` ([`DAY`]), whole or not at all.
    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let kept = KeptDays {
            through: self.through,
            hours: self
                .hours
                .iter()
                .map(|(at, units)| {
                    let units = units
                        .iter()
                        .map(|(id, sums)| {
                            let sums = KeptSums {
                                v: sums.v.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
                                e: sums
                                    .e
                                    .iter()
                                    .map(|(k, v)| (k.to_string(), v.clone()))
                                    .collect(),
                            };
                            (id.clone(), sums)
                        })
                        .collect();
                    (*at, units)
                })
                .collect(),
            last: self.last.values().cloned().collect(),
        };
        link_files::write_private(&dir.join(DAY), &link_files::saved::print_json(&kept)?)?;
        Ok(())
    }

    /// Sums up every unit of `line`.
    pub fn add(&mut self, line: &Line) {
        self.through = self.through.max(line.t);
        let hour = line.t / HOUR * HOUR;
        for kept in &line.units {
            let before = self.last.get(&kept.id);
            let sums = self
                .hours
                .entry(hour)
                .or_default()
                .entry(kept.id.clone())
                .or_default();
            for (name, value) in &kept.v {
                let Some(m) = measure(name) else { continue };
                // A zero is left out as a line is written, but a total's:
                // the day is the same, read back or kept as it's seen.
                if !value.is_finite() || *value == 0.0 && m.kind != Kind::Total {
                    continue;
                }
                let value = match m.kind {
                    Kind::Total => {
                        let last = before.and_then(|b| Some((b.v.get(name)?, b.s)));
                        match last {
                            None => 0.0,
                            // Started again: its count from nothing.
                            Some((_, s)) if s.is_some() && kept.s.is_some() && s != kept.s => {
                                *value
                            }
                            // Fell without starting again (a process
                            // gone between two reads): nothing grew.
                            Some((last, _)) => (value - last).max(0.0),
                        }
                    }
                    _ => *value,
                };
                let sum = sums.v.entry(m.name).or_insert(match m.kind {
                    Kind::Gauge | Kind::Each => value,
                    Kind::Total | Kind::Delta => 0.0,
                });
                *sum = match m.kind {
                    Kind::Gauge | Kind::Each => sum.max(value),
                    Kind::Total | Kind::Delta => add(*sum, value),
                };
            }
            for (name, values) in &kept.e {
                if let Some(m) = measure(name) {
                    let sample = sums.e.entry(m.name).or_default();
                    for value in values.iter().filter(|v| v.is_finite()) {
                        sample.add(*value, 1);
                    }
                }
            }
            self.last.insert(kept.id.clone(), kept.clone());
        }
    }

    /// Forgets the hours before the one `since` is in, and the units
    /// seen in none of those left.
    pub fn prune(&mut self, since: u64) {
        self.hours = self.hours.split_off(&(since / HOUR * HOUR));
        let seen: std::collections::HashSet<&String> =
            self.hours.values().flat_map(|units| units.keys()).collect();
        self.last.retain(|id, _| seen.contains(id));
    }

    /// Unit `id`'s measures over the hours from the one `since` is in.
    pub fn day(&self, id: &str, since: u64) -> BTreeMap<&'static str, f64> {
        let mut sums = Sums::default();
        for units in self.hours.range(since / HOUR * HOUR..).map(|(_, u)| u) {
            if let Some(unit) = units.get(id) {
                sums.merge(unit);
            }
        }
        sums.summed()
    }

    /// Unit `id`'s measures each hour from the one `since` is in, those
    /// with anything kept: `(the hour's start, its measures)`.
    pub fn hours(&self, id: &str, since: u64) -> Vec<(u64, BTreeMap<&'static str, f64>)> {
        self.hours
            .range(since / HOUR * HOUR..)
            .filter_map(|(at, units)| Some((*at, units.get(id)?.summed())))
            .filter(|(_, summed)| !summed.is_empty())
            .collect()
    }

    /// Every unit seen, as last seen.
    pub fn units(&self) -> impl Iterator<Item = &Kept> {
        self.last.values()
    }
}

impl From<KeptDays> for Days {
    fn from(kept: KeptDays) -> Self {
        let named = |name: &str| find(name).map(|m| m.name);
        Days {
            hours: kept
                .hours
                .into_iter()
                .map(|(at, units)| {
                    let units = units
                        .into_iter()
                        .map(|(id, sums)| {
                            let sums = Sums {
                                v: sums
                                    .v
                                    .into_iter()
                                    .filter_map(|(k, v)| Some((named(&k)?, v)))
                                    .collect(),
                                e: sums
                                    .e
                                    .into_iter()
                                    .filter_map(|(k, v)| Some((named(&k)?, v)))
                                    .collect(),
                            };
                            (id, sums)
                        })
                        .collect();
                    (at, units)
                })
                .collect(),
            last: kept.last.into_iter().map(|k| (k.id.clone(), k)).collect(),
            through: kept.through,
        }
    }
}

/// Every measure, in [`MEASURES`]' order, that `summed` has.
pub fn ordered(summed: &BTreeMap<&'static str, f64>) -> impl Iterator<Item = (&'static str, f64)> {
    MEASURES
        .iter()
        .filter_map(|m| Some((m.name, *summed.get(m.name)?)))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{kept, line};
    use super::*;

    fn day(lines: &[Line]) -> BTreeMap<&'static str, f64> {
        let mut days = Days::default();
        for line in lines {
            days.add(line);
        }
        days.day("a1", 0)
    }

    #[test]
    fn a_day_sums_each_measure_by_its_kind() {
        let restarted = Kept {
            s: Some(2),
            ..kept(&[("memory.used.sum", 6.0), ("cpu.used.sum", 1.0)], &[])
        };
        let lines = vec![
            line(
                10,
                kept(
                    &[
                        ("memory.used.sum", 5.0),
                        ("cpu.used.sum", 10.0),
                        ("tokens.input.sum", 3.0),
                    ],
                    &[("time.turn.own", &[1.0, 9.0])],
                ),
            ),
            line(
                20,
                kept(
                    &[
                        ("memory.used.sum", 7.0),
                        ("cpu.used.sum", 12.0),
                        ("tokens.input.sum", 4.0),
                    ],
                    &[("time.turn.own", &[2.0])],
                ),
            ),
            // Started again: its CPU counts from 0 again, all of it new.
            line(30, restarted),
        ];
        let span = day(&lines);
        assert_eq!(span["link.memory.used.sum"], 7.0);
        assert_eq!(span["link.cpu.used.sum"], 3.0);
        assert_eq!(span["link.tokens.input.sum"], 7.0);
        assert_eq!(span["link.time.turn.own"], 2.0);
        // A minute's summary sums up the same.
        let minute = summarize(60, &lines);
        assert_eq!(minute.units[0].v["memory.used.sum"], 7.0);
        assert_eq!(minute.units[0].v["cpu.used.sum"], 1.0);
        assert_eq!(minute.units[0].s, Some(2));
        assert_eq!(minute.units[0].v["tokens.input.sum"], 7.0);
        assert_eq!(minute.units[0].e["time.turn.own"], [1.0, 9.0, 2.0]);
    }

    /// The day read on from what the watcher kept is the day read from
    /// every line, and the day the watcher keeps as it looks: the same
    /// sums each hour, of every unit, totals that grew across it, a unit
    /// started again after it, and a sum of nothing, left out.
    #[test]
    fn a_day_read_on_from_the_kept_one_is_the_whole_days() {
        let dir = link_test_kit::TempDir::new("measure-kept");
        let mut store = Store::new(dir.to_path_buf());
        let start = 1_790_640_000;
        let unit = |id: &str, s: u64, n: f64| Kept {
            id: id.into(),
            kind: "agent".into(),
            s: Some(s),
            v: [
                ("memory.used.sum".to_string(), n),
                ("cpu.used.sum".to_string(), n * 2.0),
                ("tokens.input.sum".to_string(), 3.0),
                // Counted, as nothing: left out as it's written.
                ("requests.unmetered.sum".to_string(), 0.0),
            ]
            .into(),
            e: [("time.turn.own".to_string(), vec![n])].into(),
            ..Kept::default()
        };
        // Three hours of looks a minute apart; b starts again in the third.
        let lines: Vec<Line> = (0..180u64)
            .map(|i| Line {
                t: start + i * 60,
                units: vec![
                    unit("a", 1, i as f64),
                    unit("b", if i < 130 { 1 } else { 2 }, (i % 50) as f64),
                ],
            })
            .collect();
        for line in &lines {
            store.sample(line).unwrap();
        }
        let since = start + 600;
        let mut whole = Days::default();
        for line in lines.iter().filter(|l| l.t >= since) {
            whole.add(line);
        }
        let same = |days: &Days| {
            for id in ["a", "b"] {
                assert_eq!(days.day(id, since), whole.day(id, since), "{id}");
                assert_eq!(days.hours(id, since), whole.hours(id, since), "{id}");
            }
        };
        // Nothing kept: every line since.
        same(&Days::read(&store, since, u64::MAX));
        // Kept partway, as a watcher keeps it: that, and the lines after.
        let mut kept = Days::default();
        for line in lines
            .iter()
            .filter(|l| l.t >= since && l.t < start + 100 * 60)
        {
            kept.add(line);
        }
        kept.save(&dir).unwrap();
        same(&Days::read(&store, since, u64::MAX));
        // Only lines before `before`: the watcher's own look not twice.
        let before = start + 179 * 60;
        let mut short = Days::default();
        for line in lines.iter().filter(|l| l.t >= since && l.t < before) {
            short.add(line);
        }
        let read = Days::read(&store, since, before);
        assert_eq!(read.day("a", since), short.day("a", since));
        // Saved days before `since`: not used, the lines read instead.
        let mut stale = Days::default();
        stale.add(&lines[0]);
        stale.save(&dir).unwrap();
        same(&Days::read(&store, since, u64::MAX));
    }

    /// A total that falls while its unit runs on (a process gone between
    /// two reads, a device away) adds nothing, never its whole count.
    #[test]
    fn a_total_that_falls_without_a_restart_adds_nothing() {
        let lines = vec![
            line(10, kept(&[("cpu.used.sum", 100.0)], &[])),
            line(20, kept(&[("cpu.used.sum", 99.0)], &[])),
            line(30, kept(&[("cpu.used.sum", 104.0)], &[])),
        ];
        assert_eq!(day(&lines)["link.cpu.used.sum"], 5.0);
    }

    /// Two harness lines of 1e308 tokens or spend never make a sum no
    /// one can read back.
    #[test]
    fn sums_of_huge_values_stay_numbers_and_read_back() {
        let dir = link_test_kit::TempDir::new("measure-huge");
        let mut store = Store::new(dir.to_path_buf());
        let huge = |t| {
            line(
                t,
                kept(&[("cost.sum", 1e308), ("cpu.used.sum", 1e308)], &[]),
            )
        };
        let minute = summarize(0, &[huge(0), huge(10)]);
        assert_eq!(minute.units[0].v["cost.sum"], f64::MAX);
        store.minute(&minute).unwrap();
        store.sample(&huge(120)).unwrap();
        assert_eq!(store.since(0).len(), 2);
        let mut days = Days::default();
        store.each(0, None, |l| days.add(&l));
        days.add(&huge(130));
        days.add(&line(20, kept(&[("cpu.used.sum", f64::MAX)], &[])));
        let span = days.day("a1", 0);
        assert!(span.values().all(|v| v.is_finite()), "{span:?}");
    }

    #[test]
    fn a_day_is_kept_an_hour_at_a_time_and_pruned() {
        let mut days = Days::default();
        days.add(&line(10, kept(&[("tokens.input.sum", 1.0)], &[])));
        days.add(&line(HOUR + 10, kept(&[("tokens.input.sum", 2.0)], &[])));
        let hours = days.hours("a1", 0);
        assert_eq!(hours.len(), 2);
        assert_eq!(
            hours[1],
            (HOUR, BTreeMap::from([("link.tokens.input.sum", 2.0)]))
        );
        assert_eq!(days.day("a1", 0)["link.tokens.input.sum"], 3.0);
        days.prune(HOUR + 20);
        assert_eq!(days.day("a1", 0)["link.tokens.input.sum"], 2.0);
        days.prune(3 * HOUR);
        assert!(days.units().next().is_none());
    }

    /// However many values a harness's turns give an "each" measure, a
    /// minute keeps at most 64 and an hour of the day as many, and the
    /// median read is near all of theirs.
    #[test]
    fn each_values_are_kept_bounded() {
        let looks: Vec<Line> = (0..6u64)
            .map(|i| {
                let values: Vec<f64> = (0..64).map(|j| (i * 64 + j) as f64).collect();
                line(i * 10, kept(&[], &[("time.turn.own", &values)]))
            })
            .collect();
        let minute = summarize(60, &looks);
        assert_eq!(minute.units[0].e["time.turn.own"].len(), EACH_KEPT);
        let mut days = Days::default();
        for i in 0..1_000u64 {
            let values: Vec<f64> = (0..64).map(|j| ((i * 64 + j) % 1_000) as f64).collect();
            days.add(&line(i, kept(&[], &[("time.turn.own", &values)])));
        }
        let sample = &days.hours[&0]["a1"].e["link.time.turn.own"];
        assert!(sample.0.len() <= 2 * EACH_KEPT, "{}", sample.0.len());
        assert_eq!(sample.0.iter().map(|v| v.1).sum::<u64>(), 64_000);
        let median = days.day("a1", 0)["link.time.turn.own"];
        assert!((median - 500.0).abs() < 50.0, "{median}");
    }
}
