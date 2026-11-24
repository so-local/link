//! `[storage] conversations = "compact"`: a conversation's events as
//! small as they're kept, not for reading by hand. The same events as the
//! log, a line of JSON each, in zstd frames appended to [`FRAMES`], so a
//! write never rewrites what's there; the events since the last frame
//! are lines of their own in [`TAIL`], and once there are
//! [`FRAME_EVENTS`] of them, or [`FRAME_BYTES`], they become the next
//! frame and leave it. `lnk agent chat show <thread>` prints one as
//! text.
//!
//! Each frame is whole on its own: one cut short at the file's end (a
//! writer that stopped midway) isn't read, and the events it held are
//! still in the tail, which is emptied only once the frame is written.
//! An event in both, a writer having stopped between, is read once.
//!
//! The append that brings the tail to a frame's worth never folds it: it
//! hands the fold to a thread of its own ([`appended`]), which reads the
//! tail as a reader does, compresses it with no lock held, and takes the
//! tail's lock as a writer does only to write the frame, so neither the
//! turn that wrote the event nor a read waits while it's compressed. A
//! program waits for its folds before it ends, for at most
//! [`SETTLE_MAX`] ([`settle`]). One that ended first (killed, say) left
//! the tail whole, and its next append hands the fold on again; only a
//! tail [`FOLD_LOST`] frames' worth is folded by its append, its folds
//! having been lost.
//!
//! [`FRAMES`] starts with a frame zstd skips, [`named`]: the
//! conversation's thread, which its folder's name doesn't say whole
//! (`telegram-42` for `telegram:42`).

use std::ops::Range;

mod folding;
#[cfg(any(test, feature = "test-support"))]
pub use folding::testing;
pub use folding::{Folder, SETTLE_MAX, appended, last_framed, settle, settled_within};

use anyhow::{Context, bail};
use serde_json::Value;

/// A compact conversation's frames, in its folder: its marker too, made
/// with its first event, so a conversation started compact stays so.
pub const FRAMES: &str = "events.zst";

/// The events since the last frame, a line of JSON each: the log's name,
/// so `cat` and `jq` read them as a log's.
pub const TAIL: &str = "events.jsonl";

/// The events in the tail that make it a frame's worth: more compress
/// better, and cost more to read for the last of them. A frame holds
/// more when its fold ran late, the tail having grown meanwhile.
pub const FRAME_EVENTS: usize = 32;

/// The tail's bytes past which it becomes a frame however few its events.
pub const FRAME_BYTES: u64 = 1 << 20;

/// The most a frame holds once read: its events and a tail's bytes.
pub const FRAME_MAX: u64 = 64 << 20;

/// How many frames' worth the tail holds before the append that adds to
/// it folds it itself: each fold handed on was lost, its program having
/// ended first.
pub const FOLD_LOST: u64 = 4;

/// The most the tail is read to: [`FOLD_LOST`] frames' bytes, and the
/// event past them.
pub const TAIL_MAX: u64 = FRAME_BYTES * FOLD_LOST + super::EVENT_MAX + 1;

/// zstd's level: high, since a frame is written once per many events.
pub const LEVEL: i32 = 19;

/// Whether a tail of `events` lines and `bytes` is a frame's worth: due
/// to be folded.
pub fn due(events: usize, bytes: u64) -> bool {
    events >= FRAME_EVENTS || bytes >= FRAME_BYTES
}

/// Whether a tail of `events` lines and `bytes` is [`FOLD_LOST`] frames'
/// worth: its append folds it.
pub fn overdue(events: usize, bytes: u64) -> bool {
    events as u64 >= FRAME_EVENTS as u64 * FOLD_LOST || bytes >= FRAME_BYTES * FOLD_LOST
}

/// The magic number of the frame zstd skips that [`named`] writes.
const NAMED: u32 = 0x184D_2A50;

/// The frame [`FRAMES`] starts with, naming `thread`: a frame zstd
/// skips, holding nothing to read.
pub fn named(thread: &str) -> Vec<u8> {
    let mut frame = NAMED.to_le_bytes().to_vec();
    frame.extend((thread.len() as u32).to_le_bytes());
    frame.extend(thread.as_bytes());
    frame
}

/// The thread `frames` names at its start ([`named`]).
pub fn thread(frames: &[u8]) -> Option<String> {
    let rest = frames.strip_prefix(&NAMED.to_le_bytes())?;
    let size = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    String::from_utf8(rest.get(4..4usize.checked_add(size)?)?.to_vec()).ok()
}

/// Whether `frame` is one zstd skips: it holds no events.
fn skipped(frame: &[u8]) -> bool {
    frame.get(..4).is_some_and(|magic| {
        u32::from_le_bytes(magic.try_into().unwrap_or_default()) & !0xF == NAMED
    })
}

/// `lines` (whole lines of JSON) as one frame.
pub fn encode(lines: &[u8]) -> std::io::Result<Vec<u8>> {
    #[cfg(any(test, feature = "test-support"))]
    {
        ENCODED.with(|encoded| encoded.set(encoded.get() + 1));
        folding::testing::held(lines);
    }
    zstd::bulk::compress(lines, LEVEL)
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// The frames [`encode`] made on this thread: a test's proof of where
    /// its compressing happens.
    static ENCODED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The frames [`encode`] made on the calling thread.
#[cfg(any(test, feature = "test-support"))]
pub fn encoded_here() -> u64 {
    ENCODED.with(std::cell::Cell::get)
}

/// The whole frames of `bytes`, in order: each a frame's span. One cut
/// short, or that isn't one, ends them.
pub fn spans(bytes: &[u8]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match zstd::zstd_safe::find_frame_compressed_size(&bytes[at..]) {
            Ok(size) if size > 0 && at + size <= bytes.len() => {
                spans.push(at..at + size);
                at += size;
            }
            _ => break,
        }
    }
    spans
}

/// One frame's lines.
pub fn decode(frame: &[u8]) -> anyhow::Result<Vec<u8>> {
    if skipped(frame) {
        return Ok(Vec::new());
    }
    let size = match zstd::zstd_safe::get_frame_content_size(frame) {
        Ok(Some(size)) => size,
        _ => bail!("a frame that doesn't say its size"),
    };
    if size > FRAME_MAX {
        bail!("a frame of more than {FRAME_MAX} bytes");
    }
    let lines = zstd::bulk::decompress(frame, size as usize).context("reading a frame")?;
    if lines.last().is_some_and(|b| *b != b'\n') {
        bail!("a frame that doesn't end a line");
    }
    Ok(lines)
}

/// The events of a compact conversation whose frames are `frames` and
/// tail `tail` (the files' bytes), in order. A frame cut short at the
/// end, or a line at the tail's end, isn't read; an event in a frame and
/// the tail is read once. For a reader without the files' lock: what a
/// writer is midway through is left for the next read.
pub fn read(frames: &[u8], tail: &[u8]) -> anyhow::Result<Vec<Value>> {
    read_from(frames, tail, 0)
}

/// The events numbered `first` and on, as [`read`] reads them, but its
/// frames decoded newest first and only back to the one holding `first`:
/// a read of a conversation's end costs what it reads, however long the
/// conversation. An event before `first` is parsed only for its number.
pub fn read_from(frames: &[u8], tail: &[u8], first: u64) -> anyhow::Result<Vec<Value>> {
    let mut decoded = Vec::new();
    for span in spans(frames).into_iter().rev() {
        let lines = decode(&frames[span])?;
        let Some(head) = lines
            .split(|b| *b == b'\n')
            .next()
            .filter(|l| !l.is_empty())
        else {
            continue;
        };
        let head = seq_in(head)?;
        decoded.push(lines);
        if head <= first {
            break;
        }
    }
    let mut events: Vec<Value> = Vec::new();
    let mut last = 0;
    let mut take = |line: &[u8], last: &mut u64, tail: bool| -> anyhow::Result<()> {
        let seq = seq_in(line)?;
        // Written to a frame already, by a writer that stopped before it
        // emptied the tail.
        if tail && seq <= *last {
            return Ok(());
        }
        if *last != 0 && last.checked_add(1) != Some(seq) {
            bail!("event {seq} is out of order");
        }
        *last = seq;
        if seq >= first {
            events.push(serde_json::from_slice(line).context("reading an event")?);
        }
        Ok(())
    };
    for lines in decoded.iter().rev() {
        for line in lines.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
            take(line, &mut last, false)?;
        }
    }
    for line in whole_lines(tail)
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
    {
        take(line, &mut last, true)?;
    }
    Ok(events)
}

/// The number of the last event of a compact conversation whose frames
/// are `frames` and tail `tail`, as [`read`] reads them: 0 for none. Only
/// the newest frame holding one is decoded.
pub fn last(frames: &[u8], tail: &[u8]) -> anyhow::Result<u64> {
    if let Some(line) = whole_lines(tail)
        .split(|b| *b == b'\n')
        .rfind(|l| !l.is_empty())
    {
        return seq_in(line);
    }
    for span in spans(frames).into_iter().rev() {
        let lines = decode(&frames[span])?;
        if let Some(line) = lines.split(|b| *b == b'\n').rfind(|l| !l.is_empty()) {
            return seq_in(line);
        }
    }
    Ok(0)
}

/// `tail`'s whole lines: a line at its end not yet ended isn't one.
fn whole_lines(tail: &[u8]) -> &[u8] {
    let whole = tail
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |end| end + 1);
    &tail[..whole]
}

/// The number of the event on `line`: read from its head when it starts
/// with it, as Link's writers write it, else from the whole line.
fn seq_in(line: &[u8]) -> anyhow::Result<u64> {
    if let Some(rest) = line.strip_prefix(br#"{"seq":"#) {
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0
            && matches!(rest.get(digits), Some(b',' | b'}'))
            && let Some(seq) = std::str::from_utf8(&rest[..digits])
                .ok()
                .and_then(|d| d.parse().ok())
        {
            return Ok(seq);
        }
    }
    let event: Value = serde_json::from_slice(line).context("reading an event")?;
    super::seq_of(&event).context("an event without its number")
}

/// The newest events of a compact conversation, as [`read`] reads them,
/// but its frames decoded newest first and only while their lines come
/// to at most `most` bytes, by the size each frame says before it's
/// decoded: what's older is left out, and whether any was is said. For a
/// reader that wants only the end of a conversation an agent wrote, so
/// one file never costs more than `most` and the frames' own bytes.
pub fn read_newest(frames: &[u8], tail: &[u8], most: u64) -> anyhow::Result<(Vec<Value>, bool)> {
    let spans = spans(frames);
    let mut decoded = Vec::new();
    let mut taken = 0u64;
    let mut left_out = false;
    for span in spans.iter().rev() {
        let frame = &frames[span.clone()];
        if skipped(frame) {
            continue;
        }
        let size = match zstd::zstd_safe::get_frame_content_size(frame) {
            Ok(Some(size)) => size,
            _ => bail!("a frame that doesn't say its size"),
        };
        if taken.saturating_add(size) > most {
            left_out = true;
            break;
        }
        taken += size;
        decoded.push(decode(frame)?);
    }
    let mut events: Vec<Value> = Vec::new();
    let mut last = 0;
    for lines in decoded.iter().rev() {
        for line in lines.split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            let event: Value = serde_json::from_slice(line).context("reading an event")?;
            last = number(&event, last)?;
            events.push(event);
        }
    }
    let whole = tail
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |end| end + 1);
    for line in tail[..whole].split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let event: Value = serde_json::from_slice(line).context("reading an event")?;
        let seq = super::seq_of(&event).context("an event without its number")?;
        if seq <= last {
            continue;
        }
        last = number(&event, last)?;
        events.push(event);
    }
    Ok((events, left_out))
}

/// `event`'s number, the one after `last` (or any, when `last` is 0).
fn number(event: &Value, last: u64) -> anyhow::Result<u64> {
    let seq = super::seq_of(event).context("an event without its number")?;
    if last != 0 && last.checked_add(1) != Some(seq) {
        bail!("event {seq} is out of order");
    }
    Ok(seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(from: u64, to: u64) -> Vec<u8> {
        let mut out = Vec::new();
        for seq in from..=to {
            out.extend(serde_json::to_vec(&serde_json::json!({"seq": seq, "text": "ok"})).unwrap());
            out.push(b'\n');
        }
        out
    }

    fn seqs(events: &[Value]) -> Vec<u64> {
        events
            .iter()
            .map(|e| super::super::seq_of(e).unwrap())
            .collect()
    }

    #[test]
    fn frames_then_the_tail_in_order_each_once() {
        let mut frames = encode(&lines(1, 3)).unwrap();
        frames.extend(encode(&lines(4, 6)).unwrap());
        assert_eq!(spans(&frames).len(), 2);
        let events = read(&frames, &lines(7, 8)).unwrap();
        assert_eq!(seqs(&events), [1, 2, 3, 4, 5, 6, 7, 8]);
        // The tail not emptied after its frame was written: read once.
        let events = read(&frames, &lines(5, 8)).unwrap();
        assert_eq!(seqs(&events), [1, 2, 3, 4, 5, 6, 7, 8]);
        // Out of order is refused.
        assert!(read(&frames, &lines(8, 9)).is_err());
    }

    #[test]
    fn a_frame_or_line_cut_short_isnt_read() {
        let mut frames = encode(&lines(1, 3)).unwrap();
        let whole = frames.len();
        frames.extend(&encode(&lines(4, 6)).unwrap()[..10]);
        assert_eq!(spans(&frames), std::slice::from_ref(&(0..whole)));
        let mut tail = lines(4, 6);
        tail.extend(br#"{"seq":7,"te"#);
        assert_eq!(seqs(&read(&frames, &tail).unwrap()), [1, 2, 3, 4, 5, 6]);
    }

    /// Read from a number, only the frames back to the one holding it are
    /// decoded: an older frame that isn't events at all is never read.
    #[test]
    fn a_read_from_a_number_decodes_only_the_frames_it_needs() {
        let mut frames = named("telegram:42");
        frames.extend(encode(b"not an event\n").unwrap());
        frames.extend(encode(&lines(4, 6)).unwrap());
        frames.extend(encode(&lines(7, 9)).unwrap());
        let tail = lines(10, 11);
        assert!(read(&frames, &tail).is_err());
        assert_eq!(
            seqs(&read_from(&frames, &tail, 5).unwrap()),
            [5, 6, 7, 8, 9, 10, 11]
        );
        assert_eq!(seqs(&read_from(&frames, &tail, 8).unwrap()), [8, 9, 10, 11]);
        assert_eq!(seqs(&read_from(&frames, &tail, 11).unwrap()), [11]);
        assert!(read_from(&frames, &tail, 12).unwrap().is_empty());
        // Its last is the tail's, or the newest frame's with none there.
        assert_eq!(last(&frames, &tail).unwrap(), 11);
        assert_eq!(last(&frames, b"").unwrap(), 9);
        assert_eq!(last(&named("t"), b"").unwrap(), 0);
        // The tail not emptied after its frame was written: read once.
        assert_eq!(
            seqs(&read_from(&frames, &lines(8, 10), 8).unwrap()),
            [8, 9, 10]
        );
        // A line that doesn't start with its number is read whole.
        let other = b"{\"text\":\"ok\",\"seq\":10}\n";
        assert_eq!(seqs(&read_from(&frames, other, 9).unwrap()), [9, 10]);
    }

    /// The frame naming the thread is skipped as events, and read as the
    /// thread.
    #[test]
    fn the_first_frame_names_the_thread() {
        let mut frames = named("telegram:42");
        frames.extend(encode(&lines(1, 3)).unwrap());
        assert_eq!(spans(&frames).len(), 2);
        assert_eq!(thread(&frames).as_deref(), Some("telegram:42"));
        assert_eq!(seqs(&read(&frames, &lines(4, 4)).unwrap()), [1, 2, 3, 4]);
        assert_eq!(thread(&encode(&lines(1, 1)).unwrap()), None);
        // Named, nothing written yet.
        assert!(read(&named("t"), b"").unwrap().is_empty());
    }

    /// Read newest first within a size, only the newest frames are
    /// decoded, by the size each says, and the older are said left out.
    #[test]
    fn only_the_newest_frames_are_read_within_a_size() {
        let mut frames = named("t");
        for (from, to) in [(1, 3), (4, 6), (7, 9)] {
            frames.extend(encode(&lines(from, to)).unwrap());
        }
        let tail = lines(10, 11);
        let frame = lines(1, 3).len() as u64;
        let (events, left_out) = read_newest(&frames, &tail, u64::MAX).unwrap();
        assert_eq!(seqs(&events), (1..=11).collect::<Vec<_>>());
        assert!(!left_out);
        let (events, left_out) = read_newest(&frames, &tail, 2 * frame).unwrap();
        assert_eq!(seqs(&events), (4..=11).collect::<Vec<_>>());
        assert!(left_out);
        // None fits: the tail alone.
        let (events, left_out) = read_newest(&frames, &tail, 1).unwrap();
        assert_eq!(seqs(&events), [10, 11]);
        assert!(left_out);
    }

    #[test]
    fn many_events_are_smaller_framed() {
        let text = lines(1, 64);
        assert!(encode(&text).unwrap().len() * 4 < text.len());
    }
}
