//! Measures on this machine: the schema and its OTLP are
//! `link-measure-contract`'s, re-exported here, and what follows is the local
//! side's alone: the files harnesses and the harness module append to,
//! and what this machine and its processes say of themselves.
//!
//! A harness reports its own (a turn's tokens, its time to the model) by
//! appending lines of OTLP to the file `LNK_MEASURES` names ([`ENV`],
//! [`append`]); the measure module reads them, as the agent's.

use std::io::Write;
use std::path::Path;

pub use crate::*;

pub mod asked;
pub mod group;
pub mod machine;
pub mod process;
pub mod record;
pub mod run;
pub mod tokens;
pub mod traffic;
use serde_json::Value;

/// The file a harness appends its own measures to, a line of OTLP JSON
/// each (`ExportMetricsServiceRequest`), set by `lnk agent start`.
pub const ENV: &str = "LNK_MEASURES";

/// Past this size, [`append`] moves the file aside (`<file>.1`, replacing
/// the one before) and starts a new one: the reader has long caught up.
pub const FILE_MAX: u64 = 1024 * 1024;

/// The longest line [`read_otlp`] takes: a turn's measures are a few kB.
pub const LINE_MAX: usize = 64 * 1024;

/// The file the harness module appends an agent's start and stop times
/// to, a line of OTLP JSON each, as a harness appends its own to
/// [`ENV`]'s: in the agent's settings (`~/.lnk/safe/agents/<name>/`),
/// which no harness writes. `lnk agent list --json` names it (`times`).
pub const TIMES: &str = "times.jsonl";

/// When this machine started, in nanoseconds since 1970.
pub fn booted() -> Option<u64> {
    static BOOTED: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *BOOTED.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            let mut time: libc::timeval = unsafe { std::mem::zeroed() };
            let mut size = std::mem::size_of::<libc::timeval>();
            // SAFETY: fills `time`, of the size it's told.
            let read = unsafe {
                libc::sysctlbyname(
                    c"kern.boottime".as_ptr(),
                    (&mut time as *mut libc::timeval).cast(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            let secs = u64::try_from(time.tv_sec).ok()?;
            let micros = u64::try_from(time.tv_usec).ok()?;
            (read == 0 && secs > 0).then(|| secs * 1_000_000_000 + micros * 1_000)
        }
        #[cfg(not(target_os = "macos"))]
        {
            std::fs::read_to_string("/proc/stat")
                .ok()?
                .lines()
                .find_map(|l| l.strip_prefix("btime ")?.trim().parse::<u64>().ok())
                .map(|secs| secs * 1_000_000_000)
        }
    })
}

/// What an agent's model requests counted ([`tokens`]), from the usage
/// each provider's answer gave: numbers only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tokens {
    /// Requests for a model's answer, answered (a 2xx).
    pub requests: u64,
    /// Of them, those whose answer gave no usage (a stream that didn't
    /// ask for it): their tokens aren't in the counts.
    pub unmetered: u64,
    /// Prompt tokens, cached or not.
    pub input: u64,
    pub output: u64,
    /// Of the prompt, read from the provider's cache.
    pub cached: u64,
    /// Of the prompt, written to the provider's cache (Anthropic's,
    /// charged above the prompt's price).
    #[serde(default)]
    pub written: u64,
}

impl Tokens {
    /// What `self` counted past `before`, an earlier count of the same
    /// proxy; all of it where a count went down (it isn't the same).
    pub fn since(&self, before: &Tokens) -> Tokens {
        let fields = [
            (self.requests, before.requests),
            (self.unmetered, before.unmetered),
            (self.input, before.input),
            (self.output, before.output),
            (self.cached, before.cached),
            (self.written, before.written),
        ];
        if fields.iter().any(|(now, then)| now < then) {
            return *self;
        }
        Tokens {
            requests: self.requests - before.requests,
            unmetered: self.unmetered - before.unmetered,
            input: self.input - before.input,
            output: self.output - before.output,
            cached: self.cached - before.cached,
            written: self.written - before.written,
        }
    }

    /// `self` and `other`, added, at most the largest count there is.
    pub fn plus(&self, other: &Tokens) -> Tokens {
        Tokens {
            requests: self.requests.saturating_add(other.requests),
            unmetered: self.unmetered.saturating_add(other.unmetered),
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cached: self.cached.saturating_add(other.cached),
            written: self.written.saturating_add(other.written),
        }
    }
}

/// What one proxy has carried ([`asked::Traffic`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Carried {
    /// Bytes up from its sandbox, and down to it.
    pub up: u64,
    pub down: u64,
}

/// When process `pid` started: every service's measure of a process.
pub use link_service::started;

/// Appends `request` (OTLP JSON) to `path` as a line, the file only the
/// user's, holding its lock (`flock`, `LOCK_EX`): the measure module
/// takes the lock too, to empty the file once it has read every line;
/// past [`FILE_MAX`] the file is moved aside first. For a harness, at
/// `LNK_MEASURES`.
pub fn append(path: &Path, request: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > FILE_MAX) {
        let mut aside = path.as_os_str().to_os_string();
        aside.push(".1");
        std::fs::rename(path, aside)?;
    }
    append_line(path, request)
}

/// Retain a finite execution's numeric observations without harness-stream
/// rotation. The caller owns its artifact's retention, beside its results.
pub fn append_retained(path: &Path, reports: &[Report], service: &str) -> std::io::Result<()> {
    if reports.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if reports.iter().any(|report| !report.unit.valid()) {
        return Err(std::io::Error::other("invalid measurement unit"));
    }
    for batch in reports.chunks(32) {
        append_line(path, &otlp(batch, service))?;
    }
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)?
        .sync_data()
}

fn append_line(path: &Path, request: &Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(request)?;
    line.push(b'\n');
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    #[cfg(unix)]
    lock(&file)?;
    // One write: a reader never sees half a line, but at the end.
    file.write_all(&line)
}

pub use link_files::fs::lock;

/// Every whole line of the file at `path`, as [`append`] writes them,
/// read holding its lock: a line still being written isn't read. None
/// of a file that isn't there.
pub fn written(path: &Path) -> std::io::Result<Vec<Value>> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    #[cfg(unix)]
    lock(&file)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    text.split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
        .map(|line| serde_json::from_str(line).map_err(std::io::Error::other))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn retained_runs_keep_the_first_and_last_point_beyond_stream_rotation_size() {
        let dir = link_test_kit::TempDir::new("measure-retained");
        let path = dir.join("run.jsonl");
        let reports: Vec<_> = (0..32768)
            .map(|at| Report {
                unit: Unit {
                    id: "process".into(),
                    kind: "process".into(),
                    ..Unit::default()
                },
                points: vec![Point::new("link.cpu.used.sum", at as f64, at + 1)],
            })
            .collect();
        append_retained(&path, &reports, "fixture").unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > FILE_MAX);
        assert!(!dir.join("run.jsonl.1").exists());
        let requests = written(&path).unwrap();
        let points: Vec<_> = requests
            .iter()
            .flat_map(crate::read_otlp)
            .flat_map(|r| r.points)
            .collect();
        assert_eq!(points.len(), reports.len());
        assert_eq!(points.first().unwrap().value, 0.0);
        assert_eq!(points.last().unwrap().value, 32767.0);
        assert!(
            std::fs::read(&path)
                .unwrap()
                .split(|b| *b == b'\n')
                .all(|line| line.len() < LINE_MAX)
        );
        append_retained(&dir.join("empty"), &[], "fixture").unwrap();
        assert!(!dir.join("empty").exists());
    }

    #[test]
    fn this_machine_started_before_now() {
        assert!(booted().is_some_and(|b| b > 0 && b <= now()));
    }

    #[test]
    fn a_full_file_is_moved_aside() {
        let dir = link_test_kit::TempDir::new("measure-append");
        let path = dir.join("measures.jsonl");
        let big = json!({"pad": "x".repeat(FILE_MAX as usize)});
        append(&path, &big).unwrap();
        append(&path, &json!({"n": 2})).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"n\":2}\n");
        assert!(dir.join("measures.jsonl.1").exists());
    }

    /// A line still being written, the end of a file a reader caught
    /// between two pages of one write, isn't read; a file not yet there
    /// holds none.
    #[test]
    fn only_whole_lines_are_read() {
        let dir = link_test_kit::TempDir::new("measure-written");
        let path = dir.join("measures.jsonl");
        assert!(written(&path).unwrap().is_empty());
        append(&path, &json!({"n": 1})).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{\"resourceMetrics\":[").unwrap();
        assert_eq!(written(&path).unwrap(), [json!({"n": 1})]);
    }
}
