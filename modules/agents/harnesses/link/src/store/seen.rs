//! What this process last read of each conversation's log, so the next
//! read parses only the lines added since. A turn reads its
//! conversation more than once, and every turn reads it again; the log
//! only grows, so what was read stays as it was. Derived: a process
//! without it reads the file, and one that finds the file changed under
//! it reads it again.

use std::collections::BTreeMap;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::event::Event;

/// The most logs a process remembers: past it, the one read longest ago
/// is forgotten. A conversation's turns read it again and again; one
/// left alone costs a whole read when it's next taken up.
const LOGS_MAX: usize = 64;

/// How many bytes of a log's first and last lines read are compared
/// with the file before what was read is used: a log put in its place
/// (a conversation moved, or deleted and begun again) differs in them.
const ENDS: u64 = 256;

/// The logs a process has read, by path.
#[derive(Default)]
pub(super) struct Logs {
    held: Mutex<(u64, BTreeMap<PathBuf, Seen>)>,
    /// The bytes of lines parsed into events, all told.
    parsed: AtomicU64,
}

/// What was read of one log: its events from the `start`th byte to the
/// `end`th, each line whole, from the file `dev` and `ino` name.
pub(super) struct Seen {
    dev: u64,
    ino: u64,
    start: u64,
    end: u64,
    /// The bytes from `start`, and those before `end`, as read.
    head: Vec<u8>,
    tail: Vec<u8>,
    pub(super) events: Vec<Event>,
    /// Where each of `events` starts in the file.
    pub(super) starts: Vec<u64>,
    used: u64,
}

impl Logs {
    /// What was read of the log at `path`, taken out while it's read
    /// again: none when it wasn't, or not of `file` as it is now (put in
    /// its place, or cut shorter than was read).
    pub(super) fn take(&self, path: &Path, file: &std::fs::File) -> Option<Seen> {
        let seen = self
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(path)?;
        let meta = file.metadata().ok()?;
        let same = |at: u64, bytes: &[u8]| {
            let mut read = vec![0; bytes.len()];
            file.read_exact_at(&mut read, at).is_ok() && read == bytes
        };
        (meta.dev() == seen.dev
            && meta.ino() == seen.ino
            && meta.len() >= seen.end
            && same(seen.start, &seen.head)
            && same(seen.end - seen.tail.len() as u64, &seen.tail))
        .then_some(seen)
    }

    /// Keeps `events`, read from `file` at `path`, each starting where
    /// `starts` says and the last ending at `end`, for the next read: only
    /// those from the first a turn reads by the latest window mark among
    /// them, so what a process keeps follows its window, not the log.
    pub(super) fn keep(
        &self,
        path: PathBuf,
        file: &std::fs::File,
        end: u64,
        mut events: Vec<Event>,
        mut starts: Vec<u64>,
    ) {
        let before = before_window(&events);
        events.drain(..before);
        starts.drain(..before);
        let Some(&start) = starts.first() else {
            return;
        };
        let Ok(meta) = file.metadata() else {
            return;
        };
        let read = |at: u64, n: u64| {
            let mut bytes = vec![0; n as usize];
            file.read_exact_at(&mut bytes, at).ok().map(|()| bytes)
        };
        let n = ENDS.min(end - start);
        let (Some(head), Some(tail)) = (read(start, n), read(end - n, n)) else {
            return;
        };
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let (tick, logs) = &mut *held;
        *tick += 1;
        logs.insert(
            path,
            Seen {
                dev: meta.dev(),
                ino: meta.ino(),
                start,
                end,
                head,
                tail,
                events,
                starts,
                used: *tick,
            },
        );
        while logs.len() > LOGS_MAX {
            let Some(oldest) = logs
                .iter()
                .min_by_key(|(_, seen)| seen.used)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            logs.remove(&oldest);
        }
    }

    /// Counts `bytes` more parsed into events.
    pub(super) fn parsed(&self, bytes: u64) {
        self.parsed.fetch_add(bytes, Ordering::Relaxed);
    }

    /// The bytes of lines parsed into events, all told.
    pub(super) fn total(&self) -> u64 {
        self.parsed.load(Ordering::Relaxed)
    }

    /// How many events it holds, of every log.
    #[cfg(test)]
    pub(super) fn held(&self) -> usize {
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        held.1.values().map(|seen| seen.events.len()).sum()
    }

    /// Forgets the log at `path`: it's gone.
    pub(super) fn forget(&self, path: &Path) {
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(path);
    }
}

/// How many of `events` come before the first a turn reads by the latest
/// window mark among them, and before the last [`super::MARK_EVENTS`],
/// which a turn reads to find that mark: none when there's no mark.
pub(super) fn before_window(events: &[Event]) -> usize {
    events
        .iter()
        .rev()
        .find_map(|e| e.window.as_ref())
        .map_or(0, |mark| events.partition_point(|e| e.seq < mark.first()))
        .min(events.len().saturating_sub(super::MARK_EVENTS as usize))
}

impl Seen {
    /// Where its first event's line starts, and where its last one ends.
    pub(super) fn span(&self) -> (u64, u64) {
        (self.start, self.end)
    }
}
