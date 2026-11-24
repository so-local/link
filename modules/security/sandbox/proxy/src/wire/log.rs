//! The log: a line for every call on a wire.

use super::*;

/// What happened to a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// It went through (logged as it does, so a call that stays open is
    /// in the log while it lasts).
    Allowed,
    /// An allowed call, over: the bytes it carried.
    Ended,
    /// The sandbox's rules (or a stopped or paused wire) refused it; or,
    /// for a `pause` line, the wire itself was paused.
    Refused,
    /// Allowed, but what it asked for couldn't be reached.
    Failed,
}

impl Outcome {
    /// How the log says it.
    pub(super) fn word(self) -> &'static str {
        match self {
            Outcome::Allowed => "allowed",
            Outcome::Ended => "ended",
            Outcome::Refused => "refused",
            Outcome::Failed => "failed",
        }
    }
}

/// A call on the wire, as the proxy learns it: logged once it's allowed
/// or refused, and again when an allowed call ends.
#[derive(Debug, Default)]
pub struct Call {
    pub(super) wire: Option<&'static Wire>,
    /// The verb: `connect`, `http`, `model`, `local-model`, `port`,
    /// `browser` for a web page's request, or `unknown` for what isn't a
    /// request. None: nothing was asked (the
    /// connection closed first), and nothing is logged.
    pub verb: Option<&'static str>,
    /// Where to: `host:port`, a provider, a port of this machine.
    pub to: String,
    pub(super) allowed: bool,
    /// Refused for being over the wire's calls a minute, which no pause
    /// counts.
    pub limited: bool,
    /// Refused as a web page's request, from outside the sandbox: never
    /// counted toward a pause, and logged once a minute at most.
    pub outsider: bool,
    /// Refused for resolving to a cloud's metadata service ([`Metadata`]),
    /// which pauses the wire at once.
    pub metadata: bool,
    /// Bytes from the sandbox, and to it.
    pub up: Arc<AtomicU64>,
    pub down: Arc<AtomicU64>,
    /// Its last line is logged.
    pub(super) over: bool,
    /// Why it failed, when its upstream socket was refused
    /// ([`link_sandbox_route::dial::UntrustedUpstream`]).
    pub(super) why: Option<String>,
}

impl Call {
    /// A call on `wire` (None: not logged).
    pub fn on(wire: Option<&'static Wire>) -> Self {
        Self {
            wire,
            ..Self::default()
        }
    }

    /// It reached what it asked for.
    pub fn allowed(&mut self) {
        if !self.allowed {
            self.allowed = true;
            self.log(Outcome::Allowed);
        }
    }

    /// It's over, with `result`: an allowed call's bytes, or why it
    /// didn't go through.
    pub fn end(&mut self, result: &anyhow::Result<()>) {
        let outcome = match result {
            _ if self.allowed => Outcome::Ended,
            // Allowed and over at once (a hosted model's request).
            Ok(()) => Outcome::Allowed,
            // The rules' own word, never a socket's "connection refused".
            Err(err) if format!("{err:#}").contains("refused:") => Outcome::Refused,
            Err(err) => {
                self.why = err.chain().find_map(|e| {
                    e.downcast_ref::<link_sandbox_route::dial::UntrustedUpstream>()
                        .map(ToString::to_string)
                });
                Outcome::Failed
            }
        };
        self.over = true;
        self.log(outcome);
    }

    fn log(&self, outcome: Outcome) {
        if let Some(wire) = self.wire {
            wire.record(self, outcome);
        }
    }
}

/// A [`Call`] that, dropped before [`Call::end`] (its proxy shut down as
/// the program exited), is logged as ended, with the bytes it carried.
#[derive(Debug)]
pub struct Logged(pub Call);

impl std::ops::Deref for Logged {
    type Target = Call;
    fn deref(&self) -> &Call {
        &self.0
    }
}

impl std::ops::DerefMut for Logged {
    fn deref_mut(&mut self) -> &mut Call {
        &mut self.0
    }
}

impl Drop for Logged {
    fn drop(&mut self) {
        if self.0.allowed && !self.0.over {
            self.0.log(Outcome::Ended);
        }
    }
}

/// One line of the log, naming its schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since 1970, UTC.
    pub time: u64,
    pub sandbox: String,
    pub verb: String,
    pub to: String,
    pub outcome: Outcome,
    pub up: u64,
    pub down: u64,
    /// Why a failed call failed, when Link knows ([`Call::end`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

link_files::specification!(Entry, include_str!("log/entry.schema.toml"));

impl Entry {
    /// The columns `lnk sandbox log show` shows, as [`Entry::cells`] fills them.
    pub const HEADER: [&str; 6] = ["TIME", "SANDBOX", "VERB", "TO", "OUTCOME", "CARRIED"];

    /// The entry as `lnk sandbox log show` shows it, a cell per column of
    /// [`Entry::HEADER`], cleaned of anything a terminal would act on.
    pub fn cells(&self) -> [String; 6] {
        let outcome = match &self.why {
            Some(why) => format!("{}: {why}", self.outcome.word()),
            None => self.outcome.word().to_string(),
        };
        let carried = match self.up + self.down {
            0 => String::new(),
            _ => format!("{} up, {} down", bytes(self.up), bytes(self.down)),
        };
        [
            utc(self.time),
            link_format::clean(&self.sandbox),
            link_format::clean(&self.verb),
            link_format::clean(&self.to),
            link_format::clean(&outcome),
            carried,
        ]
    }
}

/// Appends `entry` to the log in `dir`, rotating it when it's grown past
/// [`MAX_LOG`]. One write per line: proxies of several sandboxes share the
/// file.
pub(super) fn append(dir: &Path, entry: &Entry) -> anyhow::Result<()> {
    let path = log_file(dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG) {
        rotate(&path, MAX_LOG);
    }
    let line = link_files::saved::line(entry)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&path)?.write_all(&line)?;
    Ok(())
}

/// Moves `log` to `wire.log.1`, and each kept one a place on, when it's
/// longer than `max`: one proxy at a time, holding `wire.log.lock`, and
/// only a log still too long once it holds it, so two proxies finding it
/// long at once never move a fresh log over the one just kept. Best
/// effort: with no lock to hold, nothing moves.
pub(super) fn rotate(log: &Path, max: u64) {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let Ok(lock) = options.open(log.with_extension("log.lock")) else {
        return;
    };
    if link_files::fs::lock(&lock).is_err() {
        return;
    }
    if std::fs::metadata(log).is_ok_and(|m| m.len() > max) {
        for n in (1..KEPT).rev() {
            let _ = std::fs::rename(kept(log, n), kept(log, n + 1));
        }
        let _ = std::fs::rename(log, kept(log, 1));
    }
}

/// The `n`th log kept before `log`: `wire.log.<n>`.
pub(super) fn kept(log: &Path, n: usize) -> PathBuf {
    log.with_extension(format!("log.{n}"))
}

/// The log in `dir`, oldest first: the rotated files, then the current
/// one. Lines that aren't entries are skipped.
pub fn read(dir: &Path) -> Vec<Entry> {
    let path = log_file(dir);
    let mut files: Vec<PathBuf> = (1..=KEPT).rev().map(|n| kept(&path, n)).collect();
    files.push(path);
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| link_files::saved::parse_line(line.as_bytes()).ok())
                .collect::<Vec<Entry>>()
        })
        .collect()
}

/// `2026-09-27 14:03:09`, UTC.
pub(super) fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970 to a date (Howard Hinnant's civil_from_days).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// `512 B`, `1.2 kB`, `3.4 MB`.
pub fn bytes(n: u64) -> String {
    match n {
        0..1_000 => format!("{n} B"),
        1_000..1_000_000 => format!("{:.1} kB", n as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} MB", n as f64 / 1e6),
        _ => format!("{:.1} GB", n as f64 / 1e9),
    }
}
