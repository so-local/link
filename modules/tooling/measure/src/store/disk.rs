//! A file on disk: its name and span, written short ([`Writer`]) and read
//! back whole ([`Reader`]), and compressed once its span is over.

use super::*;

/// The end of a file's name while it's written to.
pub(super) const OPEN: &str = ".jsonl";
/// The end of a file's name once its span is over: compressed.
const CLOSED: &str = ".jsonl.zst";
/// zstd's level for a closed file: written once, read a while.
const LEVEL: i32 = 9;

/// The start of the span a file's name says, `2026-10-06-14.jsonl` (an
/// hour) or `2026-10-06.jsonl.zst` (a day), in seconds since 1970, and
/// whether it's still open (not compressed).
pub(super) fn file_time(name: &str) -> Option<(u64, bool)> {
    let (stem, open) = match name.strip_suffix(CLOSED) {
        Some(stem) => (stem, false),
        None => (name.strip_suffix(OPEN)?, true),
    };
    let parts: Vec<u64> = stem
        .split('-')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let (y, m, d) = (parts.first()?, parts.get(1)?, parts.get(2)?);
    let hour = parts.get(3).copied().unwrap_or(0);
    // Days from civil (Howard Hinnant's).
    let y = *y as i64 - i64::from(*m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (*m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + *d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days)
        .ok()
        .map(|d| (d * 86_400 + hour * 3_600, open))
}

/// Compresses the open file `path` into its closed one beside it, then
/// removes it: added to the end of the closed one, as a frame of its
/// own, when that's there already (a clock set back).
pub(super) fn close(path: &Path) -> anyhow::Result<()> {
    let text = std::fs::read(path)?;
    let frame = zstd::bulk::compress(&text, LEVEL)?;
    let name = path.to_string_lossy();
    let closed = PathBuf::from(format!(
        "{}{CLOSED}",
        name.strip_suffix(OPEN).unwrap_or(&name)
    ));
    if closed.exists() {
        let mut options = std::fs::OpenOptions::new();
        options.append(true);
        options.open(&closed)?.write_all(&frame)?;
    } else {
        link_files::write_private(&closed, &frame)?;
    }
    std::fs::remove_file(path)?;
    Ok(())
}

/// What's written to one of the store's folders: the file being
/// written, and each unit as a [`Reader`] of it would have it, to leave
/// out what that reader carries forward.
#[derive(Debug, Default)]
pub(super) struct Writer {
    path: Option<PathBuf>,
    read: Reader,
}

impl Writer {
    pub(super) fn append(&mut self, path: &Path, line: &Line) -> anyhow::Result<()> {
        let mut text = Vec::new();
        // A file new to this writer: its measures named first, every
        // unit's line whole after.
        let new = (self.path.as_deref() != Some(path)).then(|| {
            let header = Header {
                measures: MEASURES.iter().map(|m| short(m.name)).collect(),
            };
            Reader {
                order: header.measures,
                ..Reader::default()
            }
        });
        if let Some(new) = &new {
            text.extend(link_files::saved::line(&Header {
                measures: new.order.clone(),
            })?);
        }
        let reader = new.as_ref().unwrap_or(&self.read);
        let written = Written {
            t: line.t,
            units: line.units.iter().map(|k| reader.write(k)).collect(),
        };
        text.extend(link_files::saved::line(&written)?);
        if let Some(dir) = path.parent() {
            link_files::create_private_dir(dir)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(path)?.write_all(&text)?;
        // On disk: what it left out is now its reader's to carry.
        if let Some(new) = new {
            self.read = new;
            self.path = Some(path.to_path_buf());
        }
        for unit in written.units {
            self.read.read(unit);
        }
        Ok(())
    }
}

/// A file's first line: the names of the measures its values are in
/// the order of. Like every line, it names its schema.
#[derive(Serialize, Deserialize)]
struct Header {
    measures: Vec<String>,
}

link_files::specification!(Header, include_str!("disk/header.schema.toml"));

/// A line as written, naming its schema.
#[derive(Serialize, Deserialize)]
struct Written {
    t: u64,
    units: Vec<WrittenUnit>,
}

link_files::specification!(Written, include_str!("disk/line.schema.toml"));

/// A unit as written: its ID, its description when it's new or changed
/// (`kind` there says so), its values in the file's order, and each
/// [`Kind::Each`]'s values by its place in that order.
#[derive(Clone, Default, Serialize, Deserialize)]
struct WrittenUnit {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    teams: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s: Option<u64>,
    #[serde(default, skip_serializing_if = "Values::is_empty")]
    v: Values,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    e: BTreeMap<String, Vec<Number>>,
}

/// A unit's values, in the file's order, `null` for one left out.
#[derive(Clone, Default)]
enum Values {
    #[default]
    None,
    Ordered(Vec<Option<Number>>),
}

impl Values {
    fn is_empty(&self) -> bool {
        match self {
            Values::None => true,
            Values::Ordered(v) => v.is_empty(),
        }
    }
}

impl Serialize for Values {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Values::None => s.serialize_none(),
            Values::Ordered(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Values {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visit;
        impl<'de> serde::de::Visitor<'de> for Visit {
            type Value = Values;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a list of values")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Values, A::Error> {
                let mut v = Vec::with_capacity(a.size_hint().unwrap_or(0));
                while let Some(x) = a.next_element::<Option<f64>>()? {
                    v.push(x.map(Number));
                }
                Ok(Values::Ordered(v))
            }
            fn visit_unit<E>(self) -> Result<Values, E> {
                Ok(Values::None)
            }
        }
        d.deserialize_any(Visit)
    }
}

/// A value, written as an integer when it is one: `1024`, not `1024.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Number(f64);

impl Serialize for Number {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Exact below 2^53, as every f64 that's an integer there is.
        if self.0.fract() == 0.0 && self.0.abs() < 9_007_199_254_740_992.0 {
            s.serialize_i64(self.0 as i64)
        } else {
            s.serialize_f64(self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Number {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        f64::deserialize(d).map(Number)
    }
}

/// Reads a file's lines in order: its measures' order, and each unit as
/// its last line had it, to carry forward what a line leaves out.
#[derive(Debug, Default)]
pub(super) struct Reader {
    order: Vec<String>,
    pub(super) last: HashMap<String, Kept>,
}

impl Reader {
    /// The unit `kept`, as written after what's read so far (read it once
    /// it's on disk).
    fn write(&self, kept: &Kept) -> WrittenUnit {
        let before = self.last.get(&kept.id);
        let mut unit = WrittenUnit {
            id: kept.id.clone(),
            ..WrittenUnit::default()
        };
        let described = |b: &Kept| {
            (&b.kind, &b.parent, &b.name, &b.owner, &b.teams, b.s)
                == (
                    &kept.kind,
                    &kept.parent,
                    &kept.name,
                    &kept.owner,
                    &kept.teams,
                    kept.s,
                )
        };
        if !before.is_some_and(described) {
            unit.kind = Some(kept.kind.clone());
            unit.parent = kept.parent.clone();
            unit.name = kept.name.clone();
            unit.owner = kept.owner.clone();
            unit.teams = kept.teams.clone();
            unit.s = kept.s;
        }
        let mut values: Vec<Option<Number>> = self
            .order
            .iter()
            .map(|name| {
                let value = *kept.v.get(name)?;
                let kind = measure(name).map(|m| m.kind);
                let carried = before.and_then(|b| b.v.get(name));
                let left_out = match kind {
                    Some(Kind::Total) => carried == Some(&value),
                    _ => value == 0.0,
                };
                (!left_out).then_some(Number(value))
            })
            .collect();
        while values.last() == Some(&None) {
            values.pop();
        }
        unit.v = Values::Ordered(values);
        for (at, name) in self.order.iter().enumerate() {
            if let Some(each) = kept.e.get(name).filter(|e| !e.is_empty()) {
                unit.e
                    .insert(at.to_string(), each.iter().copied().map(Number).collect());
            }
        }
        unit
    }

    /// The unit `unit` as written, whole: what it left out carried
    /// forward from its last line.
    fn read(&mut self, unit: WrittenUnit) -> Kept {
        let before = self.last.get(&unit.id);
        let mut kept = match (&unit.kind, before) {
            (None, Some(before)) => Kept {
                v: BTreeMap::new(),
                e: BTreeMap::new(),
                ..before.clone()
            },
            _ => Kept {
                id: unit.id.clone(),
                kind: unit.kind.clone().unwrap_or_default(),
                parent: unit.parent,
                name: unit.name,
                owner: unit.owner,
                teams: unit.teams,
                s: unit.s,
                ..Kept::default()
            },
        };
        // Its running totals as last read, unless the line says.
        if let Some(before) = before {
            for (name, value) in &before.v {
                if measure(name).is_some_and(|m| m.kind == Kind::Total) {
                    kept.v.insert(name.clone(), *value);
                }
            }
        }
        match unit.v {
            Values::None => {}
            Values::Ordered(values) => {
                for (name, value) in self.order.iter().zip(values) {
                    if let Some(Number(value)) = value {
                        kept.v.insert(name.clone(), value);
                    }
                }
            }
        }
        for (at, values) in unit.e {
            let Some(name) = at.parse::<usize>().ok().and_then(|at| self.order.get(at)) else {
                continue;
            };
            let name = name.clone();
            kept.e
                .insert(name, values.into_iter().map(|n| n.0).collect());
        }
        self.last.insert(kept.id.clone(), kept.clone());
        kept
    }

    /// A line of a file, read: its header (none back), or a line.
    fn line(&mut self, text: &str) -> Option<Line> {
        let Ok(written) = link_files::saved::parse_line::<Written>(text.as_bytes()) else {
            if let Ok(header) = link_files::saved::parse_line::<Header>(text.as_bytes()) {
                self.order = header.measures;
            }
            return None;
        };
        Some(Line {
            t: written.t,
            units: written.units.into_iter().map(|u| self.read(u)).collect(),
        })
    }
}

/// `folder`'s files that may hold times from `since` on (each `span`
/// seconds long), oldest first: of one span, the closed one first.
pub(super) fn files(folder: &Path, since: u64, span: u64) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<(u64, bool, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let (start, open) = file_time(&name)?;
            Some((start, open, e.path()))
        })
        .filter(|(start, _, _)| start + span > since)
        .collect();
    files.sort();
    files.into_iter().map(|(_, _, path)| path).collect()
}

/// `path`'s text, read a line at a time: uncompressed if it's closed.
pub(super) fn open(path: &Path) -> Option<Box<dyn BufRead>> {
    let file = std::fs::File::open(path).ok()?;
    if path.to_string_lossy().ends_with(CLOSED) {
        let decoder = zstd::stream::read::Decoder::new(file).ok()?;
        return Some(Box::new(std::io::BufReader::new(decoder)));
    }
    Some(Box::new(std::io::BufReader::new(file)))
}

/// Each line of `path` from `since` on that reads as one, in order,
/// read by `reader`. One before it is passed over by its time, at its
/// head, unread when `seeded` (the reader has each unit as last read
/// before `since`), else read for what the lines after leave out, and
/// not passed on.
pub(super) fn lines(
    path: &Path,
    since: u64,
    reader: &mut Reader,
    seeded: bool,
    mut f: impl FnMut(Line),
) {
    let Some(file) = open(path) else {
        return;
    };
    for text in file.lines().map_while(Result::ok) {
        let before = time_at_head(&text).is_some_and(|t| t < since);
        if before && seeded {
            continue;
        }
        if let Some(line) = reader.line(&text)
            && !before
        {
            f(line);
        }
    }
}

/// A line's time, from its head (`{"t":`, written first): none when it
/// isn't there.
fn time_at_head(line: &str) -> Option<u64> {
    let rest = line.strip_prefix(r#"{"t":"#)?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    rest.get(..digits)?.parse().ok()
}

/// The time of the first line from `since` on in `files`.
pub(super) fn first(files: &[PathBuf], since: u64) -> Option<u64> {
    files.iter().find_map(|path| {
        open(path)?
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| time_at_head(&l))
            .find(|t| *t >= since)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_named_by_their_hour_and_read_back() {
        assert_eq!(civil(1_790_690_587), (2026, 9, 29, 14));
        assert_eq!(
            file_time("2026-09-29-14.jsonl"),
            Some((1_790_690_587 / 3600 * 3600, true))
        );
        assert_eq!(
            file_time("2026-09-29.jsonl.zst"),
            Some((1_790_690_587 / 86_400 * 86_400, false))
        );
        assert_eq!(file_time("notes"), None);
        assert_eq!(file_time("2026-09-29"), None);
    }

    /// An agent much as the watcher sees it, at look `i`: memory that
    /// moves, CPU that grows, folders and network that mostly don't.
    fn agent(n: usize, i: u64) -> Kept {
        let id = format!("{n:032x}");
        let v = [
            ("memory.used.sum", 180_000_000.0 + (i % 7) as f64 * 4096.0),
            ("memory.peak.max", 210_000_000.0),
            ("cpu.used.sum", 12.5 + i as f64 * 0.25),
            ("disk.used.sum", 98_765_432.0),
            ("disk.files.sum", 1_234.0),
            ("disk.read.sum", 1_000_000.0 + (i / 30) as f64 * 4096.0),
            ("disk.written.sum", 2_000_000.0 + (i / 30) as f64 * 8192.0),
            ("network.in.sum", 5_000_000.0 + (i / 6) as f64 * 1500.0),
            ("network.out.sum", 900_000.0 + (i / 6) as f64 * 700.0),
            ("processes.sum", 6.0),
            ("ports.sum", 1.0),
            ("gpu.used.sum", 0.0),
            ("tokens.input.sum", 0.0),
            ("tokens.output.sum", 0.0),
            ("requests.model.sum", 0.0),
        ];
        Kept {
            id,
            kind: "agent".into(),
            parent: Some("f".repeat(32)),
            name: Some(format!("agent-{n}")),
            owner: Some("local".into()),
            s: Some(1_790_000_000_000_000_000),
            v: v.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            ..Kept::default()
        }
    }

    /// What a line reads back as: zeros left out, nothing else changed.
    fn as_read(line: &Line) -> Line {
        let mut line = line.clone();
        for unit in &mut line.units {
            unit.v.retain(|name, v| {
                *v != 0.0 || measure(name).is_some_and(|m| m.kind == Kind::Total)
            });
        }
        line
    }

    /// Every line written reads back as it was, totals and descriptions
    /// carried forward where they were left out, an agent that started
    /// again and one renamed included, and from a file closed too.
    #[test]
    fn lines_read_back_as_written_and_once_closed() {
        let dir = link_test_kit::TempDir::new("measure-short");
        let mut store = Store::new(dir.to_path_buf());
        let start = 1_790_640_000;
        let lines: Vec<Line> = (0..400u64)
            .map(|i| {
                let mut b = agent(1, i);
                if i >= 200 {
                    b.s = Some(2);
                    b.v.insert("cpu.used.sum".into(), (i - 200) as f64 * 0.25);
                }
                if i >= 300 {
                    b.name = Some("renamed".into());
                }
                let mut a = agent(0, i);
                if i % 50 == 0 {
                    a.e.insert("time.turn.own".into(), vec![1.5, 2.0]);
                    a.v.insert("tokens.input.sum".into(), 1_000.0);
                }
                Line {
                    t: start + i * 10,
                    units: vec![a, b],
                }
            })
            .collect();
        for line in &lines {
            store.sample(line).unwrap();
        }
        let expected: Vec<Line> = lines.iter().map(as_read).collect();
        assert_eq!(store.since(start), expected);
        // Read on from partway, as a view does from what the watcher
        // kept: what's left out comes from the units as last read.
        let mut days = Days::default();
        for line in &lines[..250] {
            days.add(line);
        }
        let mut read = Vec::new();
        store.each(lines[250].t, Some(&days.last), |l| read.push(l));
        assert_eq!(read, expected[250..]);
        // Closed once its hour is over: compressed, and the same.
        let open = |dir: &Path| {
            std::fs::read_dir(dir.join("fine"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(OPEN))
                .count()
        };
        assert_eq!(open(&dir), 2);
        store.prune(start + 7_200, 86_400, 86_400);
        assert_eq!(open(&dir), 0);
        assert_eq!(store.since(start), expected);
        let mut read = Vec::new();
        store.each(lines[250].t, Some(&days.last), |l| read.push(l));
        assert_eq!(read, expected[250..]);
    }

    /// What a look at 300 agents takes on disk, kept as before (every
    /// value by name, every line) and as now, and once its hour is
    /// closed: `cargo test -p link-measure -- --ignored --nocapture
    /// store_size`.
    #[test]
    #[ignore = "a measurement, run by hand"]
    fn store_size() {
        let dir = link_test_kit::TempDir::new("measure-size");
        let mut store = Store::new(dir.to_path_buf());
        let start = 1_790_640_000;
        let mut by_name = 0;
        for i in 0..360u64 {
            let line = Line {
                t: start + i * 10,
                units: (0..300).map(|n| agent(n, i)).collect(),
            };
            by_name += serde_json::to_vec(&line).unwrap().len() + 1;
            store.sample(&line).unwrap();
        }
        let path = store.fine_file(start);
        let short = std::fs::metadata(&path).unwrap().len();
        store.prune(start + 3_600, 86_400, 86_400);
        let closed = std::fs::metadata(format!("{}.zst", path.display()))
            .unwrap()
            .len();
        println!(
            "an hour at 300 agents: {by_name} bytes by name ({} a look), {short} short ({} a look), {closed} closed",
            by_name / 360,
            short / 360
        );
    }

    /// Read from partway through a file with nothing kept to start
    /// from, each unit is still whole: what the lines before left out is
    /// read from them, though they aren't passed on.
    #[test]
    fn a_read_from_partway_without_a_seed_is_whole() {
        let dir = link_test_kit::TempDir::new("measure-partway");
        let mut store = Store::new(dir.to_path_buf());
        let start = 1_790_640_000;
        let lines: Vec<Line> = (0..100u64)
            .map(|i| Line {
                t: start + i * 10,
                units: vec![agent(0, i)],
            })
            .collect();
        for line in &lines {
            store.sample(line).unwrap();
        }
        let expected: Vec<Line> = lines.iter().map(as_read).collect();
        let read = store.since(lines[50].t);
        assert_eq!(read, expected[50..]);
        assert_eq!(read[0].units[0].kind, "agent");
        assert!(read[0].units[0].v.contains_key("disk.used.sum"));
    }

    /// A line that couldn't be written leaves the writer as it was: the
    /// next line says everything the last one on disk didn't.
    #[test]
    fn a_line_not_written_leaves_out_nothing_after() {
        let dir = link_test_kit::TempDir::new("measure-unwritten");
        let mut store = Store::new(dir.to_path_buf());
        let start = 1_790_640_000;
        let at = |i: u64| Line {
            t: start + i * 10,
            units: vec![agent(0, i * 30)],
        };
        store.sample(&at(0)).unwrap();
        // The disk full for the next: its file is /dev/full a moment.
        let path = store.fine_file(start);
        let kept = path.with_extension("kept");
        std::fs::rename(&path, &kept).unwrap();
        std::os::unix::fs::symlink("/dev/full", &path).unwrap();
        assert!(store.sample(&at(1)).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&kept, &path).unwrap();
        // The same totals as the line that wasn't written, new to the file.
        let again = Line {
            t: start + 20,
            ..at(1)
        };
        store.sample(&again).unwrap();
        assert_eq!(store.since(start), [as_read(&at(0)), as_read(&again)]);
    }
}
