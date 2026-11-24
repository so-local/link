//! Where each log's first message of each delivery and sender is, so a
//! message repeating one ([`Repeat`]) is found without reading the
//! conversation. Derived, and grown only by the lines added since: a
//! process without it reads the log's lines once, parsing only those that
//! name a delivery or a sender.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Read, Seek};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::{EVENT_MAX, Held, LOG, Repeat, nofollow};
use crate::event::Event;

/// The most logs remembered: past it, the one asked of longest ago is
/// forgotten, and read again when it's next asked of.
const LOGS_MAX: usize = 64;

/// How many bytes before where a log was read to are compared with the
/// file before what was found is used: a log put in its place differs.
const TAIL: u64 = 256;

/// What each log's lines name, by path.
#[derive(Default)]
pub(super) struct Firsts {
    held: Mutex<(u64, BTreeMap<PathBuf, Found>)>,
    /// The bytes of lines read, all told.
    read: std::sync::atomic::AtomicU64,
}

/// One log's: where the first line naming each key starts, read to `end`
/// of the file `dev` and `ino` name, whose bytes before `end` are `tail`.
#[derive(Default)]
struct Found {
    dev: u64,
    ino: u64,
    end: u64,
    tail: Vec<u8>,
    firsts: HashMap<String, u64>,
    used: u64,
}

impl Firsts {
    /// The first message of the log in `folder` that `repeat` names: none
    /// when there's none.
    pub(super) fn first(&self, folder: &Held, repeat: &Repeat) -> anyhow::Result<Option<Event>> {
        let path = folder.at(LOG);
        let mut file = match folder.open(LOG, libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        nofollow::lock_fd(file.as_raw_fd(), libc::LOCK_SH)?;
        let meta = file.metadata()?;
        let mut found = self.take(&path, &file).unwrap_or_default();
        found.dev = meta.dev();
        found.ino = meta.ino();
        // Only the lines added since are read.
        file.seek(std::io::SeekFrom::Start(found.end))?;
        let mut reader = std::io::BufReader::new(&file);
        let mut line = Vec::new();
        loop {
            line.clear();
            (&mut reader)
                .take(EVENT_MAX + 2)
                .read_until(b'\n', &mut line)?;
            if line.last() != Some(&b'\n') {
                break;
            }
            let begins = found.end;
            found.end += line.len() as u64;
            self.read
                .fetch_add(line.len() as u64, std::sync::atomic::Ordering::Relaxed);
            if !(holds(&line, br#""delivery":"#) || holds(&line, br#""from":"#)) {
                continue;
            }
            let event: Event = link_files::saved::parse_line(&line[..line.len() - 1])?;
            for key in Repeat::keys(&event) {
                found.firsts.entry(key).or_insert(begins);
            }
        }
        drop(reader);
        let n = TAIL.min(found.end);
        found.tail = vec![0; n as usize];
        file.read_exact_at(&mut found.tail, found.end - n)?;
        let at = found.firsts.get(&repeat.key()).copied();
        self.keep(path, found);
        let Some(at) = at else {
            return Ok(None);
        };
        file.seek(std::io::SeekFrom::Start(at))?;
        let mut reader = std::io::BufReader::new(&file);
        line.clear();
        (&mut reader)
            .take(EVENT_MAX + 2)
            .read_until(b'\n', &mut line)?;
        let line = line.strip_suffix(b"\n").unwrap_or(&line);
        let mut event: Event = link_files::saved::parse_line(line)?;
        event.seq = super::log::seq_of_line(line).unwrap_or(event.seq);
        Ok(Some(event))
    }

    /// The bytes of lines it has read, all told.
    #[cfg(test)]
    pub(super) fn read(&self) -> u64 {
        self.read.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn take(&self, path: &Path, file: &std::fs::File) -> Option<Found> {
        let found = self
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(path)?;
        let meta = file.metadata().ok()?;
        let mut tail = vec![0; found.tail.len()];
        (meta.dev() == found.dev
            && meta.ino() == found.ino
            && meta.len() >= found.end
            && file
                .read_exact_at(&mut tail, found.end - found.tail.len() as u64)
                .is_ok()
            && tail == found.tail)
            .then_some(found)
    }

    fn keep(&self, path: PathBuf, mut found: Found) {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let (tick, logs) = &mut *held;
        *tick += 1;
        found.used = *tick;
        logs.insert(path, found);
        while logs.len() > LOGS_MAX {
            let Some(oldest) = logs
                .iter()
                .min_by_key(|(_, found)| found.used)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            logs.remove(&oldest);
        }
    }
}

/// Whether `line` holds `bytes`.
fn holds(line: &[u8], bytes: &[u8]) -> bool {
    line.windows(bytes.len()).any(|window| window == bytes)
}
