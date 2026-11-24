//! The wire: a sandbox's one way out, its filtering proxy, seen from
//! outside. Every call on it is a line in its log (`lnk sandbox log show`), one
//! flag stops every wire at once (`lnk sandbox stop`), and one flag a
//! sandbox pauses its wire alone (`lnk sandbox stop <name>`), which its proxy
//! also sets itself when its calls look like no normal work ([`Watch`]).
//!
//! All live in Link's settings (`sandbox/wire.log`, `sandbox/stopped`,
//! `sandbox/paused/<sandbox>`), which no sandbox reads or writes. The log keeps who called, which verb,
//! where to, whether it was allowed, and how many bytes went each way;
//! never a request's path, headers or body. What it records comes from
//! inside a sandbox (a host name is whatever the program asked for), so
//! it's cut short when written and cleaned of control characters when
//! shown.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

mod flags;
mod log;
#[cfg(test)]
mod tests;
mod traffic;
mod watch;

pub use flags::*;
pub use log::*;
pub use traffic::*;
pub use watch::*;

/// The log's size before it's rotated (those before kept as `wire.log.1`
/// to `wire.log.<KEPT>`, the newest first).
const MAX_LOG: u64 = 8 * 1024 * 1024;
/// How many logs before the current one are kept.
const KEPT: usize = 4;
/// How often a wire logs a refusal while it's paused, stopped or over
/// its limit: once, then a count, so a sandbox can't push everyone's
/// history out of the log with refusals that cost it nothing.
const QUIET_EVERY: Duration = Duration::from_secs(60);
/// The most lines a wire logs of each outcome (allowed, ended, refused,
/// failed) within a minute; past it, what it leaves out is counted and
/// said on its next line of that outcome. So no sandbox pushes every
/// agent's history out of the shared log, whatever its calls end in,
/// and a flood of one outcome never hides the first lines of another.
const LINES_A_MINUTE: u32 = 120;
/// The longest target kept: a host name's longest, and some.
const MAX_TO: usize = 300;
/// How soon a call in progress ends once its wire is stopped or paused.
const WATCH: Duration = Duration::from_secs(1);
/// The window a wire's limit counts calls in.
const MINUTE: Duration = Duration::from_secs(60);

/// One sandbox's wire: its name, where its log and flags are, what it
/// watches its calls for, and how many it takes a minute.
#[derive(Clone, Debug)]
pub struct Wire {
    pub sandbox: String,
    pub dir: PathBuf,
    pub watch: Arc<std::sync::Mutex<Watch>>,
    /// The most calls it takes within a minute; None: no limit.
    pub per_minute: Option<u32>,
    /// When its latest calls came, within the last minute.
    calls: Arc<std::sync::Mutex<std::collections::VecDeque<std::time::Instant>>>,
    /// A pause it couldn't write yet (out of descriptors, say): it
    /// refuses meanwhile, and writes it at the next look.
    unwritten: Arc<std::sync::Mutex<Option<String>>>,
    /// When it last logged a refusal while paused, stopped or over its
    /// limit, and how many it has left out since.
    quiet: Arc<std::sync::Mutex<(Option<std::time::Instant>, u64)>>,
    /// The same for requests from outside the sandbox, a web page's
    /// ([`Call::outsider`]), apart, so they never hide the sandbox's own.
    quiet_outsiders: Arc<std::sync::Mutex<(Option<std::time::Instant>, u64)>>,
    /// Its lines of each outcome in the current minute ([`LINES_A_MINUTE`]).
    lines: Arc<std::sync::Mutex<[Lines; 4]>>,
    /// When its flags were last looked at, and whether it was stopped:
    /// shared by the calls in progress, which look every second.
    looked: Arc<std::sync::Mutex<Option<(std::time::Instant, bool)>>>,
}

impl Wire {
    /// Sandbox `sandbox`'s wire in `dir`.
    pub fn new(sandbox: &str, dir: PathBuf) -> Self {
        Self {
            sandbox: sandbox.into(),
            dir,
            watch: Default::default(),
            per_minute: None,
            calls: Default::default(),
            unwritten: Default::default(),
            quiet: Default::default(),
            quiet_outsiders: Default::default(),
            lines: Default::default(),
            looked: Default::default(),
        }
    }

    /// The same, taking at most `per_minute` calls within a minute.
    pub fn limited(self, per_minute: Option<u32>) -> Self {
        Self { per_minute, ..self }
    }

    /// Counts a call, and why it's refused if it's one more than the
    /// minute's limit allows: how long until the next one can go, and the
    /// refusal. A refused call isn't counted.
    pub fn over_limit(&self) -> Option<(u64, String)> {
        let limit = self.per_minute?;
        let now = std::time::Instant::now();
        let mut calls = self.calls.lock().ok()?;
        while calls
            .front()
            .is_some_and(|t| now.duration_since(*t) >= MINUTE)
        {
            calls.pop_front();
        }
        if calls.len() < limit as usize {
            calls.push_back(now);
            return None;
        }
        let wait = calls
            .front()
            .map_or(MINUTE, |t| MINUTE.saturating_sub(now.duration_since(*t)));
        Some((
            wait.as_secs().max(1),
            format!("this sandbox takes {limit} calls a minute"),
        ))
    }

    /// Sandbox `sandbox`'s wire, in Link's settings (`sandbox/`).
    pub fn here(sandbox: &str) -> anyhow::Result<Self> {
        Ok(Self::new(sandbox, dir()?))
    }

    /// Whether this wire refuses everything: every wire is stopped, or
    /// this one is paused.
    pub fn stopped(&self) -> bool {
        self.why_stopped().is_some()
    }

    /// Why this wire refuses everything, if it does, as a refusal says it.
    pub fn why_stopped(&self) -> Option<String> {
        if stop_file(&self.dir).exists() {
            return Some("every wire is stopped (lnk sandbox start opens them)".into());
        }
        let said = |reason: &str| {
            format!(
                "this sandbox's wire is paused, {} (lnk sandbox start {})",
                reason.trim(),
                self.sandbox
            )
        };
        // A pause it couldn't write: refused, and written now if it can.
        if let Ok(mut unwritten) = self.unwritten.lock()
            && let Some(reason) = unwritten.clone()
        {
            if pause(&self.dir, &self.sandbox, &reason).is_ok() {
                *unwritten = None;
            }
            return Some(said(&reason));
        }
        // Whether it's paused, by the file being there (a stat, which
        // needs no descriptor); why, if it can be read.
        let file = pause_file(&self.dir, &self.sandbox);
        if !file.exists() {
            return None;
        }
        let reason = std::fs::read_to_string(&file)
            .unwrap_or_else(|_| "for a reason it couldn't read".into());
        Some(said(&reason))
    }

    /// Whether this wire refuses everything, as its flags were at most
    /// half a [`WATCH`] ago: however many calls are open, the flags are
    /// read about twice a second.
    fn stopped_lately(&self) -> bool {
        let now = std::time::Instant::now();
        let Ok(mut looked) = self.looked.lock() else {
            return self.stopped();
        };
        match *looked {
            Some((when, stopped)) if now.duration_since(when) < WATCH / 2 => stopped,
            _ => {
                let stopped = self.stopped();
                *looked = Some((now, stopped));
                stopped
            }
        }
    }

    /// Returns once this wire refuses everything, within a second.
    #[expect(
        clippy::disallowed_methods,
        reason = "the wire's own period of looking"
    )]
    pub async fn until_stopped(&self) {
        loop {
            tokio::time::sleep(WATCH / 2).await;
            if self.stopped_lately() {
                return;
            }
        }
    }

    /// Adds `call` to the log, as `outcome`. A log that can't be written
    /// doesn't stop the call.
    fn record(&self, call: &Call, outcome: Outcome) {
        let Some(verb) = call.verb else {
            return;
        };
        let mut to = call.to.clone();
        // Refused while paused, stopped or over its limit, or from outside
        // the sandbox: logged once a minute, with how many were left out
        // since.
        let quiet = match call.outsider {
            true => &self.quiet_outsiders,
            false => &self.quiet,
        };
        if outcome == Outcome::Refused
            && (call.outsider || call.limited || self.stopped())
            && let Ok(mut quiet) = quiet.lock()
        {
            if quiet.0.is_some_and(|last| last.elapsed() < QUIET_EVERY) {
                quiet.1 += 1;
                return;
            }
            if quiet.1 > 0 {
                to = format!(
                    "{} (and {} more refused since)",
                    to.chars().take(MAX_TO - 40).collect::<String>(),
                    quiet.1
                );
            }
            *quiet = (Some(std::time::Instant::now()), 0);
        }
        // At most so many lines of each outcome a minute; the watch below
        // still sees every call.
        let written = match self.lines.lock() {
            Ok(mut lines) => lines[outcome as usize].take(),
            Err(_) => Some(0),
        };
        if let Some(left_out) = written.filter(|n| *n > 0) {
            to = format!(
                "{} (and {left_out} more {} since)",
                to.chars().take(MAX_TO - 40).collect::<String>(),
                outcome.word()
            );
        }
        let entry = Entry {
            time: now(),
            sandbox: self.sandbox.clone(),
            verb: verb.into(),
            to: to.chars().take(MAX_TO).collect(),
            outcome,
            up: call.up.load(Ordering::Relaxed),
            down: call.down.load(Ordering::Relaxed),
            why: call.why.clone(),
        };
        if written.is_some() {
            let _ = append(&self.dir, &entry);
        }
        // Its own limit refusing it is no pattern: the policy at work. A
        // stopped wire refuses everything, and has nothing to pause. What
        // came from outside the sandbox says nothing of it.
        if call.outsider || call.limited || self.stopped_lately() {
            return;
        }
        let pattern = match call.metadata {
            true => Some(format!(
                "it asked for a cloud's metadata service ({})",
                entry.to
            )),
            false => self
                .watch
                .lock()
                .ok()
                .and_then(|mut watch| watch.saw(&entry)),
        };
        if let Some(reason) = pattern
            && !self.stopped()
        {
            self.pause_for(&reason);
        }
    }

    /// Pauses this wire for `reason`: logged, and said on this computer.
    fn pause_for(&self, reason: &str) {
        if pause(&self.dir, &self.sandbox, reason).is_err() {
            // Refused all the same, until it's written.
            if let Ok(mut unwritten) = self.unwritten.lock() {
                *unwritten = Some(reason.to_string());
            }
            return;
        }
        if let Ok(mut looked) = self.looked.lock() {
            *looked = Some((std::time::Instant::now(), true));
        }
        let _ = append(
            &self.dir,
            &Entry {
                time: now(),
                sandbox: self.sandbox.clone(),
                verb: "pause".into(),
                to: reason.chars().take(MAX_TO).collect(),
                outcome: Outcome::Refused,
                up: 0,
                down: 0,
                why: None,
            },
        );
        link_sandbox_route::desktop::notify(
            "Link paused a sandbox's wire",
            &format!(
                "{} {reason}. lnk sandbox log show shows its calls; lnk sandbox start {} opens it.",
                self.sandbox, self.sandbox
            ),
        );
    }
}

/// A wire's lines of one outcome in the current minute.
#[derive(Debug, Default)]
struct Lines {
    /// When the minute began.
    since: Option<std::time::Instant>,
    /// Lines written in it.
    written: u32,
    /// Lines left out, since the last one written.
    left_out: u64,
}

impl Lines {
    /// Whether one more line may be written (Some: how many were left
    /// out before it, to say so), or it's left out (None, counted).
    fn take(&mut self) -> Option<u64> {
        let now = std::time::Instant::now();
        if self
            .since
            .is_none_or(|since| now.duration_since(since) >= MINUTE)
        {
            self.since = Some(now);
            self.written = 0;
        }
        if self.written >= LINES_A_MINUTE {
            self.left_out += 1;
            return None;
        }
        self.written += 1;
        Some(std::mem::take(&mut self.left_out))
    }
}
