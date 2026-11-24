//! The log (`events.jsonl`).

use super::When;
use super::*;

/// An event as a driver kept it.
pub(super) fn event_of(value: serde_json::Value) -> anyhow::Result<Event> {
    let seq = link_store_contract::store::seq_of(&value).within("an event without its number")?;
    let mut event: Event =
        link_files::saved::parse_json(&serde_json::to_vec(&value)?).within("reading an event")?;
    event.seq = seq;
    Ok(event)
}

/// The number of the last event of the log in `folder`: 0 for none.
pub(super) fn log_last(folder: &Held) -> anyhow::Result<u64> {
    let reading = || format!("reading {}", folder.at(LOG).display());
    let last = match folder.open(LOG, libc::O_RDONLY, 0) {
        Ok(mut file) => {
            nofollow::lock_fd(file.as_raw_fd(), libc::LOCK_SH).within(When(&reading))?;
            last_seq(&mut file, false).within(When(&reading))?
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err).within(When(&reading)),
    };
    Ok(last.unwrap_or(0))
}

/// Whether the file `name` in `folder` ends with `tail`, or is only
/// `tail` when `only`.
pub(super) fn ends_with(
    folder: &Held,
    name: &str,
    tail: &[u8],
    only: bool,
) -> anyhow::Result<bool> {
    let mut file = folder
        .open(name, libc::O_RDONLY, 0)
        .within(format!("reading {}", folder.at(name).display()))?;
    let length = file.metadata()?.len();
    let want = tail.len() as u64;
    if length < want || (only && length != want) {
        return Ok(false);
    }
    file.seek(std::io::SeekFrom::Start(length - want))?;
    let mut end = Vec::with_capacity(tail.len());
    file.take(want).read_to_end(&mut end)?;
    Ok(end == tail)
}

/// An event's number, from its line ([`seq_in`]).
pub(super) fn seq_of_line(line: &[u8]) -> Option<u64> {
    seq_in(line)
}

/// An event's number, from its line: written first, so read without
/// reading the rest.
fn seq_in(line: &[u8]) -> Option<u64> {
    #[derive(serde::Deserialize)]
    struct Seq {
        seq: u64,
    }
    seq_at_head(line).or_else(|| serde_json::from_slice::<Seq>(line).ok().map(|s| s.seq))
}

/// The events of the log in `folder` numbered `first` and on. A line cut short at its end
/// (a writer stopped while writing it) isn't one yet. What this process
/// read of it before (`logs`) is used, and only the lines added since
/// are read.
pub(super) fn read_log(folder: &Held, first: u64, logs: &Logs) -> anyhow::Result<Vec<Event>> {
    Ok(read_with_tail(folder, first, Some(logs))?.0)
}

pub(super) fn snapshot(folder: &Held) -> anyhow::Result<Snapshot> {
    let (events, partial) = read_with_tail(folder, 0, None)?;
    let gaps = (partial != 0)
        .then_some(link_harness_wire::evidence::export::Gap::PartialLogTail { bytes: partial })
        .into_iter()
        .collect();
    Snapshot::captured(events, gaps)
}

/// The events numbered `first` and on, and the bytes of a line cut short
/// at the log's end.
fn read_with_tail(
    folder: &Held,
    first: u64,
    logs: Option<&Logs>,
) -> anyhow::Result<(Vec<Event>, u64)> {
    use std::io::BufRead;
    let path = folder.at(LOG);
    let mut file = match folder.open(LOG, libc::O_RDONLY, 0) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Some(logs) = logs {
                logs.forget(&path);
            }
            return Ok((Vec::new(), 0));
        }
        Err(err) => return Err(err.into()),
    };
    // No writer midway through a line meanwhile.
    nofollow::lock_fd(file.as_raw_fd(), libc::LOCK_SH)?;
    // What was read before, when it reaches back to `first`: read on from
    // its end.
    let seen = logs
        .and_then(|logs| logs.take(&path, &file))
        .filter(|seen| seen.events.first().is_some_and(|e| e.seq <= first.max(1)));
    let (mut start, mut at, mut events, mut starts) = match seen {
        Some(seen) => {
            let (start, end) = seen.span();
            (Some(start), end, seen.events, seen.starts)
        }
        // A turn reads from near the end: only the lines from `first` on
        // are read, found from the end back.
        None => {
            let found = match first {
                0 | 1 => None,
                first => line_of(&mut file, first)?,
            };
            (None, found.map_or(0, |(at, _)| at), Vec::new(), Vec::new())
        }
    };
    let mut last = events.last().map(|e: &Event| e.seq);
    file.seek(std::io::SeekFrom::Start(at))?;
    let mut reader = std::io::BufReader::new(&file);
    let (mut line, mut parsed) = (Vec::new(), 0);
    loop {
        line.clear();
        (&mut reader)
            .take(EVENT_MAX + 2)
            .read_until(b'\n', &mut line)?;
        if line.last() != Some(&b'\n') {
            if line.len() as u64 > EVENT_MAX + 1 {
                bail!("a line is larger than {EVENT_MAX} bytes");
            }
            break;
        }
        let begins = at;
        at += line.len() as u64;
        line.pop();
        let Some(seq) = seq_in(&line) else {
            bail!("a line isn't an event");
        };
        // One after another, from wherever it starts.
        if last.is_some_and(|last| last.checked_add(1) != Some(seq)) {
            bail!("event {seq} is out of order");
        }
        last = Some(seq);
        // Read on from what was read before, every line is kept.
        if seq >= first || start.is_some() {
            let mut event: Event = link_files::saved::parse_line(&line)?;
            event.seq = seq;
            events.push(event);
            starts.push(begins);
            parsed += line.len() as u64 + 1;
            start.get_or_insert(begins);
        }
    }
    drop(reader);
    let partial = line.len() as u64;
    let Some(logs) = logs else {
        return Ok((events, partial));
    };
    logs.parsed(parsed);
    let from = events
        .iter()
        .position(|e| e.seq >= first)
        .unwrap_or(events.len());
    let asked = events[from..].to_vec();
    if start.is_some() {
        logs.keep(path, &file, at, events, starts);
    }
    Ok((asked, partial))
}

/// Where the log's line numbered `seq` starts, or the last before it
/// when none is numbered that far, and its number: found from the end
/// back, by each line's head. None when every line is numbered after
/// `seq`, or one isn't numbered at its head: the log is read from its
/// start.
fn line_of(file: &mut std::fs::File, seq: u64) -> std::io::Result<Option<(u64, u64)>> {
    // `{"seq":`, a number's digits, and what follows them.
    const HEAD: usize = 32;
    let length = file.metadata()?.len();
    let mut chunk = vec![0; (64 << 10) + HEAD];
    // The head of the chunk read before, which follows this one.
    let mut after = 0;
    let mut at = length;
    while at > 0 {
        let n = (chunk.len() - HEAD).min(at as usize);
        at -= n as u64;
        chunk.copy_within(..after, n);
        file.seek(std::io::SeekFrom::Start(at))?;
        file.read_exact(&mut chunk[..n])?;
        let read = &chunk[..n + after];
        let starts = (0..n).rev().filter(|&i| read[i] == b'\n').map(|i| i + 1);
        for i in starts.chain((at == 0).then_some(0)) {
            // Past the last line end: nothing yet.
            if at + i as u64 == length {
                continue;
            }
            match seq_at_head(&read[i..read.len().min(i + HEAD)]) {
                Some(found) if found <= seq => return Ok(Some((at + i as u64, found))),
                Some(_) => {}
                None => return Ok(None),
            }
        }
        after = (n + after).min(HEAD);
    }
    Ok(None)
}

/// An event's number, from the head of its line, as [`seq_in`] reads it
/// quickly: none when it isn't there.
fn seq_at_head(head: &[u8]) -> Option<u64> {
    let rest = head.strip_prefix(br#"{"seq":"#)?;
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    (digits > 0 && matches!(rest.get(digits), Some(b',' | b'}')))
        .then(|| std::str::from_utf8(&rest[..digits]).ok()?.parse().ok())
        .flatten()
}

/// The number of the log's last whole event, `file` locked: none when it
/// holds none. With `cut` (locked for writing), a line cut short at its
/// end, by a writer that stopped, is taken off first.
pub(super) fn last_seq(file: &mut std::fs::File, cut: bool) -> std::io::Result<Option<u64>> {
    let length = file.metadata()?.len();
    let too_large = || std::io::Error::other(format!("a line is larger than {EVENT_MAX} bytes"));
    // The last two line ends, from the end back.
    let mut ends: Vec<u64> = Vec::new();
    let mut chunk = vec![0; 64 << 10];
    let mut at = length;
    while at > 0 && ends.len() < 2 {
        if length - at > 2 * (EVENT_MAX + 1) {
            return Err(too_large());
        }
        let n = chunk.len().min(at as usize);
        at -= n as u64;
        file.seek(std::io::SeekFrom::Start(at))?;
        file.read_exact(&mut chunk[..n])?;
        for i in (0..n).rev() {
            if chunk[i] == b'\n' {
                ends.push(at + i as u64);
                if ends.len() == 2 {
                    break;
                }
            }
        }
    }
    let whole = ends.first().map_or(0, |end| end + 1);
    // More than a line's most isn't a line cut short: not taken off.
    if length - whole > EVENT_MAX + 1 {
        return Err(too_large());
    }
    if cut && whole < length {
        file.set_len(whole)?;
    }
    let Some(&end) = ends.first() else {
        return Ok(None);
    };
    let start = ends.get(1).map_or(0, |before| before + 1);
    if end - start > EVENT_MAX {
        return Err(too_large());
    }
    let mut line = vec![0; (end - start) as usize];
    file.seek(std::io::SeekFrom::Start(start))?;
    file.read_exact(&mut line)?;
    seq_in(&line)
        .map(Some)
        .ok_or_else(|| std::io::Error::other("its last line isn't an event"))
}

/// The answer the file `name` in `folder` (a `turn.recovered`) names, in
/// the conversation whose folder is named `named`: only a thread whose
/// folder that is.
pub(super) fn recovered_in(folder: &Held, name: &str, named: &str) -> Option<(Thread, u64)> {
    let kept: serde_json::Value =
        serde_json::from_slice(&folder.read(name, MARK_MAX).ok()?).ok()?;
    let thread = kept["thread"]
        .as_str()
        .and_then(|name| Thread::new(name).ok())
        .filter(|thread| thread.folder() == named)?;
    Some((thread, kept["seq"].as_u64()?))
}
