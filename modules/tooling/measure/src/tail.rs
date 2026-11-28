//! What each agent's harness reported itself since the last look, and
//! what the harness module timed of its starts and stops: the lines
//! each appended to its file (`LNK_MEASURES`, and the agent's
//! [`link_measure_contract::client::TIMES`]), read from where the watcher left
//! off, in the file it moved aside too when it started a new one. Once
//! every line is read, to the file's end, the file is emptied, holding
//! its lock as its writer does to append ([`link_measure_contract::client::append`]):
//! it holds only what's unread. A harness's file is in its home, which
//! it writes: each is opened without following a link, read only if it's
//! a plain file, a bounded amount at a time, and only what the schema
//! has, and what its writer may say ([`By`]), is kept
//! ([`link_measure_contract::client::read_otlp`]).

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use link_measure_contract::client::{By, Kind, LINE_MAX, Point, read_otlp};
use serde::{Deserialize, Serialize};

/// The most read of a file at one look.
const READ_MAX: u64 = 4 * 1024 * 1024;

/// Where reading each file stopped: `offsets.json` in the measure
/// store's folder, naming its schema.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Offsets(pub BTreeMap<PathBuf, Offset>);

link_files::specification!(Offsets, include_str!("tail/offsets.schema.toml"));

/// The offsets read, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Offsets>()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Offset {
    pub inode: u64,
    pub at: u64,
    /// In a line longer than a look reads: what's left of it, up to
    /// the next newline, is dropped.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
}

impl Offsets {
    /// As kept in `path`; none when there's nothing there.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| link_files::saved::parse_json(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        link_files::write_private(path, &link_files::saved::print_json(self)?)?;
        Ok(())
    }

    /// The points appended to `path` since the last look of the
    /// measures `by` may say: a harness's own (a turn's tokens, its
    /// times), or the harness module's (a start's and a stop's times).
    pub fn read(&mut self, path: &Path, by: By) -> Vec<Point> {
        let mut text = Vec::new();
        let known = self.0.get(path).copied();
        // Written too, to empty it, when it can be; locked meanwhile, so
        // no line is appended between its read and its emptying.
        let (file, writable) = match open_writable(path) {
            Some(file) => (file, true),
            None => match open(path) {
                Some(file) => (file, false),
                None => return Vec::new(),
            },
        };
        if writable && link_measure_contract::client::lock(&file).is_err() {
            return Vec::new();
        }
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        let (mut at, mut skip) = match known {
            Some(o) if o.inode == meta.ino() && o.at <= meta.len() => (o.at, o.skip),
            // Moved aside since: the rest of the old one first.
            Some(o) => {
                let mut aside = path.as_os_str().to_os_string();
                aside.push(".1");
                if let Some(old) = open(Path::new(&aside))
                    && old.metadata().is_ok_and(|m| m.ino() == o.inode)
                {
                    rest(old, o.at, &mut { o.skip }, &mut text);
                }
                (0, false)
            }
            // A file never read: from its start.
            None => (0, false),
        };
        let emptied = file.try_clone().ok();
        at += rest(file, at, &mut skip, &mut text);
        // Read to its end, every line whole: emptied, so it holds only
        // what's unread.
        if writable
            && !skip
            && let Some(file) = emptied
            && file.metadata().is_ok_and(|m| m.len() == at)
            && at > 0
            && file.set_len(0).is_ok()
        {
            at = 0;
        }
        self.0.insert(
            path.to_path_buf(),
            Offset {
                inode: meta.ino(),
                at,
                skip,
            },
        );
        points(&text, by)
    }
}

/// `path` opened to read and write, if it's a plain file, never through a
/// link.
fn open_writable(path: &Path) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// `path` opened to read, if it's a plain file, never through a link.
fn open(path: &Path) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// Appends to `text` the whole lines of `file` from `from`, at most
/// [`READ_MAX`]; how many bytes it read on past. `skip`: in a line
/// longer than that, dropped up to its end, however many looks it
/// takes, so it never holds up the lines after it.
fn rest(mut file: std::fs::File, from: u64, skip: &mut bool, text: &mut Vec<u8>) -> u64 {
    if file.seek(SeekFrom::Start(from)).is_err() {
        return 0;
    }
    let mut read = Vec::new();
    if file.take(READ_MAX).read_to_end(&mut read).is_err() {
        return 0;
    }
    let mut start = 0;
    if *skip {
        match read.iter().position(|b| *b == b'\n') {
            Some(i) => (start, *skip) = (i + 1, false),
            None => return read.len() as u64,
        }
    }
    // Up to the last whole line: the rest is read next time, unless
    // the window holds no end of a line at all.
    let whole = match read[start..].iter().rposition(|b| *b == b'\n') {
        Some(i) => start + i + 1,
        None if read.len() as u64 == READ_MAX => {
            *skip = true;
            return read.len() as u64;
        }
        None => start,
    };
    text.extend_from_slice(&read[start..whole]);
    whole as u64
}

/// The most a harness may say one point of a measure is, by its unit:
/// past it, the point is dropped. A turn's tokens, cost, calls or time
/// never come near; a harness that says more says what isn't so.
fn most(unit: &str) -> f64 {
    match unit {
        "{token}" => 1e9,
        // A million dollars.
        "{microdollar}" => 1e12,
        // Eleven days of seconds; a million turns, calls or errors.
        _ => 1e6,
    }
}

/// The most values of one "each" measure (a turn's time) taken from a
/// harness in one look: a harness turns over far fewer in 10 s, and each
/// is kept one by one.
const EACH_MAX: usize = 64;

/// The points in `text`'s lines that `by` may report: what it counts
/// itself (a harness's turn's tokens and cost, its times; the harness
/// module's starts and stops), never what's measured from outside. Of
/// each "each" measure, the first [`EACH_MAX`].
fn points(text: &[u8], by: By) -> Vec<Point> {
    let mut points = Vec::new();
    let mut each: std::collections::HashMap<&'static str, usize> = Default::default();
    for line in text.split(|b| *b == b'\n') {
        if line.is_empty() || line.len() > LINE_MAX {
            continue;
        }
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        for report in read_otlp(&json) {
            points.extend(report.points.into_iter().filter(|p| {
                let taken = p.measure.by == by && p.value <= most(p.measure.unit);
                if !taken || p.measure.kind != Kind::Each {
                    return taken;
                }
                let seen = each.entry(p.measure.name).or_default();
                *seen += 1;
                *seen <= EACH_MAX
            }));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_measure_contract::client::{Report, Unit, append, otlp};

    fn turn(tokens: f64) -> serde_json::Value {
        otlp(
            &[Report {
                unit: Unit::default(),
                points: vec![
                    Point::new("link.tokens.input.sum", tokens, 1),
                    // Not the harness's to say.
                    Point::new("link.memory.used.sum", 1.0, 1),
                ],
            }],
            "test",
        )
    }

    #[test]
    fn each_line_is_read_once_across_a_move_aside() {
        let dir = link_test_kit::TempDir::new("measure-tail");
        let path = dir.join("measures.jsonl");
        let mut offsets = Offsets::default();
        assert!(offsets.read(&path, By::Harness).is_empty());
        append(&path, &turn(1.0)).unwrap();
        append(&path, &turn(2.0)).unwrap();
        let read: Vec<f64> = offsets
            .read(&path, By::Harness)
            .iter()
            .map(|p| p.value)
            .collect();
        assert_eq!(read, [1.0, 2.0]);
        // Read to its end: it holds nothing now.
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        assert!(offsets.read(&path, By::Harness).is_empty());
        // One more, then the harness moves the file aside and starts anew.
        append(&path, &turn(3.0)).unwrap();
        std::fs::rename(&path, dir.join("measures.jsonl.1")).unwrap();
        append(&path, &turn(4.0)).unwrap();
        // Half a line: left for the next look.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, b"{\"resourceMe"))
            .unwrap();
        let read: Vec<f64> = offsets
            .read(&path, By::Harness)
            .iter()
            .map(|p| p.value)
            .collect();
        assert_eq!(read, [3.0, 4.0]);
        // Half a line left: kept, and the next whole one read with it.
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        // Kept across a restart of the watcher.
        offsets.save(&dir.join("offsets.json")).unwrap();
        assert_eq!(Offsets::load(&dir.join("offsets.json")), offsets);
    }

    /// Past what a turn could be, a point is dropped: what's summed of
    /// a harness's stays a number.
    #[test]
    fn a_harness_says_no_more_than_a_turn_could() {
        let line = |name: &str, value: f64| {
            let mut json = otlp(
                &[Report {
                    unit: Unit::default(),
                    points: vec![Point::new(name, 1.0, 1)],
                }],
                "test",
            );
            json["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"][0]["asInt"] =
                serde_json::Value::Null;
            json["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"][0]["asDouble"] =
                value.into();
            let mut text = json.to_string().into_bytes();
            text.push(b'\n');
            text
        };
        let mut text = line("link.cost.sum", 1e308);
        text.extend(line("link.cost.sum", 1e308));
        text.extend(line("link.tokens.input.sum", 1e10));
        text.extend(line("link.cost.sum", 4500.0));
        let read: Vec<f64> = points(&text, By::Harness).iter().map(|p| p.value).collect();
        assert_eq!(read, [4500.0]);
    }

    /// Of one "each" measure, a look takes the first 64 values a harness
    /// says, however many it writes.
    #[test]
    fn a_harness_says_only_so_many_values() {
        let mut points_written: Vec<Point> = (0..1000)
            .map(|n| Point::new("link.time.to_model.own", f64::from(n), 1))
            .collect();
        points_written.push(Point::new("link.tokens.input.sum", 7.0, 1));
        let mut text = otlp(
            &[Report {
                unit: Unit::default(),
                points: points_written,
            }],
            "test",
        )
        .to_string()
        .into_bytes();
        text.push(b'\n');
        let read = points(&text, By::Harness);
        let of = |name: &str| read.iter().filter(|p| p.measure.name == name).count();
        assert_eq!(of("link.time.to_model.own"), EACH_MAX);
        assert_eq!(of("link.tokens.input.sum"), 1);
    }

    /// Each file gives only what its writer may say: a harness can't
    /// time its own start, nor the harness module a turn, nor either
    /// what only a proxy counts.
    #[test]
    fn each_file_gives_only_what_its_writer_says() {
        let mut text = otlp(
            &[Report {
                unit: Unit::default(),
                points: vec![
                    Point::new("link.time.start_warm.own", 0.8, 1),
                    Point::new("link.time.stop.own", 0.2, 1),
                    Point::new("link.turns.sum", 1.0, 1),
                    Point::new("link.gpu.used.sum", 9.0, 1),
                    Point::new("link.requests.model.sum", 3.0, 1),
                    Point::new("link.tokens.cache_written.sum", 5.0, 1),
                ],
            }],
            "test",
        )
        .to_string()
        .into_bytes();
        text.push(b'\n');
        let names =
            |by| -> Vec<&str> { points(&text, by).iter().map(|p| p.measure.name).collect() };
        assert_eq!(names(By::Harness), ["link.turns.sum"]);
        assert_eq!(
            names(By::Module),
            ["link.time.start_warm.own", "link.time.stop.own"]
        );
    }

    /// A line longer than a look reads is dropped, and the lines after
    /// it read: it never stalls the file.
    #[test]
    fn a_line_too_long_to_read_is_skipped() {
        let dir = link_test_kit::TempDir::new("measure-long");
        let path = dir.join("measures.jsonl");
        // Written as is: `append` would move a file this big aside.
        let mut text = vec![b'x'; READ_MAX as usize + 10];
        text.push(b'\n');
        text.extend(turn(7.0).to_string().into_bytes());
        text.push(b'\n');
        std::fs::write(&path, &text).unwrap();
        let mut offsets = Offsets::default();
        let mut read = Vec::new();
        for _ in 0..3 {
            read.extend(offsets.read(&path, By::Harness).iter().map(|p| p.value));
        }
        assert_eq!(read, [7.0]);
    }

    #[test]
    fn a_link_is_never_followed() {
        let dir = link_test_kit::TempDir::new("measure-link");
        let target = dir.join("elsewhere.jsonl");
        append(&target, &turn(1.0)).unwrap();
        std::os::unix::fs::symlink(&target, dir.join("measures.jsonl")).unwrap();
        assert!(
            Offsets::default()
                .read(&dir.join("measures.jsonl"), By::Harness)
                .is_empty()
        );
    }
}
