//! A conversation kept compact (`[storage] conversations = "compact"`):
//! its events in zstd frames appended to `events.zst`, and those since
//! the last frame lines of their own in `events.jsonl`, the tail
//! ([`link_store_contract::store::compact`]). The tail's lock is the
//! conversation's: a writer holds it to append, and to write a frame and
//! empty the tail; a reader holds it shared, so never sees a frame
//! written and its events still in the tail. The append that fills the
//! tail hands its fold on to a thread of its own ([`compact::appended`]),
//! which compresses with no lock held, so the turn's appends and reads go
//! on meanwhile.

use super::When;
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;

use anyhow::bail;
use link_format::WithinExt;
use link_store_contract::store::compact::{self, FRAMES, TAIL, TAIL_MAX};

use super::{Held, Snapshot, last_seq};
use crate::event::Event;
use link_files::nofollow::{lock_fd, read_open};
use link_harness_wire::evidence::export::Gap;

/// The most `events.zst` is read to: far past any conversation.
const FRAMES_MAX: u64 = 1 << 40;

/// Starts a compact conversation in `folder`, named `thread`: its frames,
/// holding only its name ([`compact::named`]), and an empty tail. Its
/// frames say it's compact from now on.
pub(super) fn start(folder: &Held, thread: &str) -> anyhow::Result<()> {
    let path = folder.at(FRAMES);
    let made = folder.open(FRAMES, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600);
    match made {
        Ok(mut file) => file
            .write_all(&compact::named(thread))
            .within(format!("writing {}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err).within(format!("writing {}", path.display())),
    }
    folder
        .open(TAIL, libc::O_WRONLY | libc::O_CREAT, 0o600)
        .within(format!("writing {}", folder.at(TAIL).display()))?;
    Ok(())
}

/// The thread the conversation in `folder` was started for.
pub(super) fn thread(folder: &Held) -> Option<String> {
    let mut head = Vec::new();
    folder
        .open(FRAMES, libc::O_RDONLY, 0)
        .ok()?
        .take(8 + 128)
        .read_to_end(&mut head)
        .ok()?;
    compact::thread(&head)
}

/// The events of the compact conversation in `folder` numbered `first`
/// and on, in order.
pub(super) fn read(folder: &Held, first: u64) -> anyhow::Result<Vec<Event>> {
    Ok(read_snapshot(folder, first)?.events)
}

pub(super) fn snapshot(folder: &Held) -> anyhow::Result<Snapshot> {
    read_snapshot(folder, 0)
}

fn read_snapshot(folder: &Held, first: u64) -> anyhow::Result<Snapshot> {
    let reading = || format!("reading {}", folder.path.display());
    let tail = match folder.open(TAIL, libc::O_RDONLY, 0) {
        Ok(tail) => Some(tail),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err).within(When(&reading)),
    };
    // No fold midway meanwhile.
    if let Some(tail) = &tail {
        lock_fd(tail.as_raw_fd(), libc::LOCK_SH).within(When(&reading))?;
    }
    let frames = read_frames(folder).within(When(&reading))?;
    let tail = match tail {
        Some(tail) => read_open(tail, TAIL_MAX).within(When(&reading))?,
        None => Vec::new(),
    };
    let mut events = Vec::new();
    // Only the frames back to the one holding `first` are decoded.
    for value in compact::read_from(&frames, &tail, first).within(When(&reading))? {
        let seq = link_store_contract::store::seq_of(&value).unwrap_or_default();
        let mut event = super::event_of(value).within(When(&reading))?;
        event.seq = seq;
        events.push(event);
    }
    let mut gaps = Vec::new();
    let framed = compact::spans(&frames).last().map_or(0, |span| span.end);
    if framed < frames.len() {
        gaps.push(Gap::PartialCompactFrames {
            bytes: (frames.len() - framed) as u64,
        });
    }
    let whole_tail = tail
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |end| end + 1);
    if whole_tail < tail.len() {
        gaps.push(Gap::PartialLogTail {
            bytes: (tail.len() - whole_tail) as u64,
        });
    }
    Snapshot::captured(events, gaps)
}

/// The number of the compact conversation's last event in `folder`: 0
/// for none. Only its newest frame holding one is decoded.
pub(super) fn last(folder: &Held) -> anyhow::Result<u64> {
    let reading = || format!("reading {}", folder.path.display());
    let tail = match folder.open(TAIL, libc::O_RDONLY, 0) {
        Ok(tail) => Some(tail),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err).within(When(&reading)),
    };
    // No fold midway meanwhile.
    if let Some(tail) = &tail {
        lock_fd(tail.as_raw_fd(), libc::LOCK_SH).within(When(&reading))?;
    }
    let frames = read_frames(folder).within(When(&reading))?;
    let tail = match tail {
        Some(tail) => read_open(tail, TAIL_MAX).within(When(&reading))?,
        None => Vec::new(),
    };
    compact::last(&frames, &tail).within(When(&reading))
}

/// `events.zst` in `folder`, whole: none when it isn't there.
fn read_frames(folder: &Held) -> std::io::Result<Vec<u8>> {
    match folder.open(FRAMES, libc::O_RDONLY, 0) {
        Ok(file) => read_open(file, FRAMES_MAX),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

impl compact::Folder for Held {
    fn open(&self, name: &str, flags: libc::c_int, mode: u32) -> std::io::Result<std::fs::File> {
        Held::open(self, name, flags, mode)
    }

    fn duplicate(&self) -> std::io::Result<Self> {
        Ok(Held {
            dir: self.dir.try_clone()?,
            path: self.path.clone(),
        })
    }
}

/// Adds `line` (an event's JSON and its line end), numbered `seq`, to the
/// compact conversation in `folder`, if `seq` is the next; then folds the
/// tail once it's due ([`compact::appended`]).
pub(super) fn append(folder: &Held, seq: u64, line: &[u8]) -> anyhow::Result<()> {
    let path = folder.at(TAIL);
    let writing = || format!("writing {}", path.display());
    let mut tail = folder
        .open(TAIL, libc::O_RDWR | libc::O_CREAT, 0o600)
        .within(When(&writing))?;
    // One writer at a time, of any process.
    lock_fd(tail.as_raw_fd(), libc::LOCK_EX).within(When(&writing))?;
    let last = match last_seq(&mut tail, true).within(When(&writing))? {
        Some(last) => last,
        // An empty tail: the last frame's last.
        None => compact::last_framed(folder)?,
    };
    if seq <= last {
        bail!(
            "{} holds number {seq} already: another turn came first",
            folder.path.display()
        );
    }
    if seq != last + 1 {
        bail!(
            "{} is the next number in {}, not {seq}",
            last + 1,
            folder.path.display()
        );
    }
    tail.seek(std::io::SeekFrom::End(0))
        .within(When(&writing))?;
    tail.write_all(line).within(When(&writing))?;
    compact::appended(folder, &mut tail);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_store_contract::store::compact::FRAME_EVENTS;

    fn folder(name: &str) -> link_test_kit::TempDir {
        link_test_kit::TempDir::new(&format!("frames-{name}"))
    }

    fn held(dir: &std::path::Path) -> Held {
        Held::root(dir, false).unwrap().unwrap()
    }

    fn line(seq: u64) -> Vec<u8> {
        let event = Event {
            seq,
            ..Event::user(&format!("m{seq}"))
        };
        link_files::saved::line(&event).unwrap()
    }

    fn seqs(events: &[Event]) -> Vec<u64> {
        events.iter().map(|e| e.seq).collect()
    }

    /// The folds handed on, done within the testing kit's deadline.
    fn settled() {
        let within = link_test_kit::wait::scaled(std::time::Duration::from_secs(30));
        assert!(compact::settled_within(within), "a fold still runs");
    }

    /// Events go to the tail, and each frame's worth into a frame, folded
    /// on a thread of its own: never by the append, the frames made
    /// meanwhile. Read back in order, from any number, whatever's framed.
    #[test]
    fn the_tail_folds_into_frames_never_in_an_append() {
        let dir = folder("fold");
        let at = held(&dir);
        start(&at, "telegram:42").unwrap();
        assert_eq!(thread(&at).as_deref(), Some("telegram:42"));
        // Short of a tail whose folds were lost.
        let n = FRAME_EVENTS as u64 * 3 + 5;
        assert!(n < FRAME_EVENTS as u64 * compact::FOLD_LOST);
        let before = compact::encoded_here();
        for seq in 1..=n {
            append(&at, seq, &line(seq)).unwrap();
        }
        assert_eq!(compact::encoded_here(), before, "an append compressed");
        settled();
        let frames = std::fs::read(dir.join(FRAMES)).unwrap();
        assert!(compact::spans(&frames).len() > 1);
        let tail = std::fs::read_to_string(dir.join(TAIL)).unwrap();
        assert!(tail.lines().count() < FRAME_EVENTS);
        assert_eq!(seqs(&read(&at, 1).unwrap()), (1..=n).collect::<Vec<_>>());
        assert_eq!(seqs(&read(&at, n - 1).unwrap()), [n - 1, n]);
        // Taken, past the next, refused.
        assert!(append(&at, 3, &line(3)).is_err());
        assert!(append(&at, n + 2, &line(n + 2)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A writer that stopped between writing a frame and emptying the
    /// tail: the events are read once, and framed once.
    #[test]
    fn an_event_framed_already_is_framed_once() {
        let dir = folder("crash");
        let at = held(&dir);
        start(&at, "t").unwrap();
        let n = FRAME_EVENTS as u64;
        for seq in 1..n {
            append(&at, seq, &line(seq)).unwrap();
        }
        let tail = std::fs::read(dir.join(TAIL)).unwrap();
        append(&at, n, &line(n)).unwrap();
        settled();
        // The tail as it was before it was emptied.
        let mut stale = tail.clone();
        stale.extend(line(n));
        std::fs::write(dir.join(TAIL), &stale).unwrap();
        assert_eq!(seqs(&read(&at, 1).unwrap()), (1..=n).collect::<Vec<_>>());
        for seq in n + 1..=2 * n {
            append(&at, seq, &line(seq)).unwrap();
        }
        assert_eq!(
            seqs(&read(&at, 1).unwrap()),
            (1..=2 * n).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A frame cut short at the end isn't read, and the next fold cuts it
    /// off; a line cut short in the tail is taken off by the next writer.
    #[test]
    fn what_was_cut_short_is_cut_off() {
        let dir = folder("cut");
        let at = held(&dir);
        start(&at, "t").unwrap();
        let n = FRAME_EVENTS as u64;
        for seq in 1..=n {
            append(&at, seq, &line(seq)).unwrap();
        }
        settled();
        let mut frames = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(FRAMES))
            .unwrap();
        frames
            .write_all(&compact::encode(&line(n + 1)).unwrap()[..9])
            .unwrap();
        let mut tail = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(TAIL))
            .unwrap();
        tail.write_all(br#"{"seq":33,"ro"#).unwrap();
        assert_eq!(read(&at, 1).unwrap().len(), n as usize);
        for seq in n + 1..=2 * n {
            append(&at, seq, &line(seq)).unwrap();
        }
        assert_eq!(
            seqs(&read(&at, 1).unwrap()),
            (1..=2 * n).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Events added to the tail as appends add them, whose folds were
    /// lost (their program ended first): each is read once, the next
    /// append hands the fold on again, and a tail [`compact::FOLD_LOST`]
    /// frames' worth is folded by its append.
    #[test]
    fn a_lost_fold_leaves_every_event_read_once() {
        let dir = folder("lost");
        let at = held(&dir);
        start(&at, "t").unwrap();
        let frame = FRAME_EVENTS as u64;
        let all = |last: u64| (1..=last).collect::<Vec<_>>();
        let lost = |from: u64, to: u64| {
            let mut tail = std::fs::OpenOptions::new()
                .append(true)
                .open(dir.join(TAIL))
                .unwrap();
            for seq in from..=to {
                tail.write_all(&line(seq)).unwrap();
            }
        };
        lost(1, 2 * frame + 3);
        assert_eq!(seqs(&read(&at, 1).unwrap()), all(2 * frame + 3));
        let next = 2 * frame + 4;
        append(&at, next, &line(next)).unwrap();
        settled();
        assert_eq!(std::fs::read(dir.join(TAIL)).unwrap(), b"");
        assert_eq!(seqs(&read(&at, 1).unwrap()), all(next));
        // Lost again and again: the append that makes it overdue folds.
        let overdue = next + frame * compact::FOLD_LOST;
        lost(next + 1, overdue - 1);
        let before = compact::encoded_here();
        append(&at, overdue, &line(overdue)).unwrap();
        assert_eq!(compact::encoded_here(), before + 1);
        assert_eq!(std::fs::read(dir.join(TAIL)).unwrap(), b"");
        assert_eq!(seqs(&read(&at, 1).unwrap()), all(overdue));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
