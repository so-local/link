//! A conversation kept compact: its events in zstd frames, `events.zst`,
//! and those since the last frame in the tail, `events.jsonl`. The tail's
//! lock is the conversation's. The append that fills the tail hands its
//! fold on ([`compact::appended`]).

use std::io::{Read, Seek, Write};

use serde_json::Value;

use super::super::Reply;
use super::super::compact::{self, FRAMES, TAIL, TAIL_MAX};
use super::super::under::read_open;
use super::{Dir, last_seq, lock_fd};

const FRAMES_MAX: u64 = 1 << 40;

fn frames(conv: &Dir) -> std::io::Result<Vec<u8>> {
    match conv.open(FRAMES, libc::O_RDONLY, 0) {
        Ok(file) => read_open(file, FRAMES_MAX),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

pub fn thread(conv: &Dir) -> Option<String> {
    let mut head = Vec::new();
    conv.open(FRAMES, libc::O_RDONLY, 0)
        .ok()?
        .take(8 + 128)
        .read_to_end(&mut head)
        .ok()?;
    compact::thread(&head)
}

pub fn read(conv: &Dir, first: u64) -> anyhow::Result<Vec<Value>> {
    let (framed, tail) = files(conv)?;
    compact::read_from(&framed, &tail, first)
}

pub fn last(conv: &Dir) -> anyhow::Result<u64> {
    let (framed, tail) = files(conv)?;
    compact::last(&framed, &tail)
}

/// The conversation's frames and its tail, the tail locked meanwhile.
fn files(conv: &Dir) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let tail = match conv.open(TAIL, libc::O_RDONLY, 0) {
        Ok(tail) => Some(tail),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };
    if let Some(tail) = &tail {
        lock_fd(std::os::fd::AsRawFd::as_raw_fd(tail), libc::LOCK_SH)?;
    }
    let framed = frames(conv)?;
    let tail = match tail {
        Some(tail) => read_open(tail, TAIL_MAX)?,
        None => Vec::new(),
    };
    Ok((framed, tail))
}

impl compact::Folder for Dir {
    fn open(&self, name: &str, flags: libc::c_int, mode: u32) -> std::io::Result<std::fs::File> {
        Dir::open(self, name, flags, mode)
    }

    fn duplicate(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
}

/// `line`, numbered `seq`, added to the tail if `seq` is the next; then
/// the tail folded once it's due ([`compact::appended`]).
pub fn append(conv: &Dir, seq: u64, line: &[u8]) -> anyhow::Result<Reply> {
    let mut tail = conv.open(TAIL, libc::O_RDWR | libc::O_CREAT, 0o600)?;
    lock_fd(std::os::fd::AsRawFd::as_raw_fd(&tail), libc::LOCK_EX)?;
    let last = match last_seq(&mut tail, true)? {
        Some(last) => last,
        None => compact::last_framed(conv)?,
    };
    if last.checked_add(1) != Some(seq) {
        return Ok(Reply::Taken { last });
    }
    tail.seek(std::io::SeekFrom::End(0))?;
    tail.write_all(line)?;
    compact::appended(conv, &mut tail);
    Ok(Reply::Done)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use link_test_kit::wait;

    use super::super::super::compact::{FRAME_EVENTS, testing};
    use super::*;

    /// A compact conversation in a folder of its own, its frames naming
    /// only its thread, and the bytes its test's events say, held by no
    /// other test.
    fn conversation(name: &str) -> (link_test_kit::TempDir, Dir, String) {
        let dir = link_test_kit::TempDir::new(name);
        std::fs::write(dir.join(FRAMES), compact::named("t")).unwrap();
        let conv = Dir::root(&dir).unwrap().unwrap();
        let marker = format!("{}", dir.display());
        (dir, conv, marker)
    }

    fn line(seq: u64, text: &str) -> Vec<u8> {
        format!("{{\"seq\":{seq},\"text\":\"{text}\"}}\n").into_bytes()
    }

    fn seqs(conv: &Dir) -> Vec<u64> {
        read(conv, 1)
            .unwrap()
            .iter()
            .map(|e| super::super::super::seq_of(e).unwrap())
            .collect()
    }

    fn settled() {
        assert!(
            compact::settled_within(wait::scaled(Duration::from_secs(30))),
            "a fold still runs"
        );
    }

    /// The tail filled, its fold handed on and held midway through
    /// compressing: an append and a read finish meanwhile, never waiting
    /// for it. Once it's let go, what was appended meanwhile is framed
    /// after it, and every event is read once.
    #[test]
    fn an_append_and_a_read_go_on_while_a_fold_compresses() {
        let (dir, conv, marker) = conversation("compact-fold-aside");
        let hold = testing::Hold::compressing(marker.as_bytes());
        let n = FRAME_EVENTS as u64;
        for seq in 1..=n {
            assert_eq!(
                append(&conv, seq, &line(seq, &marker)).unwrap(),
                Reply::Done
            );
        }
        wait::eventually("the fold to compress", Duration::from_secs(10), || {
            hold.entered()
        });
        let (said, done) = std::sync::mpsc::channel();
        let other = conv.try_clone().unwrap();
        std::thread::spawn(move || {
            let appended = append(&other, n + 1, &line(n + 1, "meanwhile")).ok();
            let read = read(&other, 1).ok().map(|events| events.len());
            let _ = said.send((appended, read));
        });
        let (appended, read) = wait::eventually(
            "an append and a read while the fold compresses",
            Duration::from_secs(10),
            || done.try_recv().ok(),
        );
        assert_eq!(appended, Some(Reply::Done));
        assert_eq!(read, Some(n as usize + 1));
        drop(hold);
        settled();
        assert_eq!(seqs(&conv), (1..=n + 1).collect::<Vec<_>>());
        assert_eq!(std::fs::read(dir.join(TAIL)).unwrap(), b"");
        let frames = std::fs::read(dir.join(FRAMES)).unwrap();
        // Its name, the fold's frame, and what came meanwhile.
        assert_eq!(compact::spans(&frames).len(), 3);
    }

    /// An append whose fold's thread can't start is done all the same:
    /// its event written, the fold said and left for the next append.
    #[test]
    fn a_fold_whose_thread_cant_start_leaves_its_append_done() {
        let (dir, conv, marker) = conversation("compact-fold-refused");
        let n = FRAME_EVENTS as u64;
        for seq in 1..n {
            append(&conv, seq, &line(seq, &marker)).unwrap();
        }
        let refusal = testing::Refusal::of(&dir.join(TAIL)).unwrap();
        assert_eq!(append(&conv, n, &line(n, &marker)).unwrap(), Reply::Done);
        drop(refusal);
        assert_eq!(seqs(&conv), (1..=n).collect::<Vec<_>>());
        append(&conv, n + 1, &line(n + 1, &marker)).unwrap();
        settled();
        assert_eq!(std::fs::read(dir.join(TAIL)).unwrap(), b"");
        assert_eq!(seqs(&conv), (1..=n + 1).collect::<Vec<_>>());
    }

    /// Waiting for the folds handed on ends by its deadline while one
    /// still runs, and is done once it's done.
    #[test]
    fn waiting_for_folds_ends_by_its_deadline() {
        let (_dir, conv, marker) = conversation("compact-fold-settle");
        let hold = testing::Hold::compressing(marker.as_bytes());
        for seq in 1..=FRAME_EVENTS as u64 {
            append(&conv, seq, &line(seq, &marker)).unwrap();
        }
        wait::eventually("the fold to compress", Duration::from_secs(10), || {
            hold.entered()
        });
        // Held until the hold drops: past any deadline.
        assert!(!compact::settled_within(Duration::from_millis(50)));
        drop(hold);
        settled();
    }
}
