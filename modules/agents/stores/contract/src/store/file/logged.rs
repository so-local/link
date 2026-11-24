//! A conversation's events as kept: the log, a line each.

use super::*;

/// `event` as a line of the log, its number first so a reader finds it
/// without reading the rest.
pub(super) fn line_of(event: &Value) -> anyhow::Result<Vec<u8>> {
    let object = event.as_object().context("an event that isn't an object")?;
    let seq = seq_of(event).context("an event without its number")?;
    let mut line = format!("{{\"seq\":{seq}");
    for (key, value) in object {
        if key == "seq" {
            continue;
        }
        line.push(',');
        line.push_str(&serde_json::to_string(key)?);
        line.push(':');
        line.push_str(&serde_json::to_string(value)?);
    }
    line.push_str("}\n");
    anyhow::ensure!(
        line.len() as u64 <= EVENT_MAX + 1,
        "the event is larger than {EVENT_MAX} bytes"
    );
    Ok(line.into_bytes())
}

/// An event's number, from the head of its line, as [`seq_in`] reads it
/// quickly: none when it isn't there.
pub(super) fn seq_at_head(head: &[u8]) -> Option<u64> {
    let rest = head.strip_prefix(br#"{"seq":"#)?;
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    (digits > 0 && matches!(rest.get(digits), Some(b',' | b'}')))
        .then(|| std::str::from_utf8(&rest[..digits]).ok()?.parse().ok())
        .flatten()
}

/// An event's number, from its line.
pub(super) fn seq_in(line: &[u8]) -> Option<u64> {
    #[derive(serde::Deserialize)]
    struct Seq {
        seq: u64,
    }
    seq_at_head(line).or_else(|| serde_json::from_slice::<Seq>(line).ok().map(|s| s.seq))
}

/// The number of the log's last whole event, `file` locked: none when it
/// holds none. With `cut` (locked for writing), a line cut short at its
/// end, by a writer that stopped, is taken off first.
pub(super) fn last_seq(file: &mut std::fs::File, cut: bool) -> std::io::Result<Option<u64>> {
    let length = file.metadata()?.len();
    let too_large = || std::io::Error::other(format!("a line is larger than {EVENT_MAX} bytes"));
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

/// Where the log's line numbered `seq` starts, or the last before it, and
/// its number: found from the end back, by each line's head. None when
/// every line is numbered after `seq`, or one isn't numbered at its head.
pub(super) fn line_at(file: &mut std::fs::File, seq: u64) -> std::io::Result<Option<(u64, u64)>> {
    const HEAD: usize = 32;
    let length = file.metadata()?.len();
    let mut chunk = vec![0; (64 << 10) + HEAD];
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

/// The events of the log in `conv` numbered `first` and on. A line cut
/// short at its end isn't one yet.
pub(super) fn read_log(conv: &Dir, first: u64) -> anyhow::Result<Vec<Value>> {
    use std::io::BufRead;
    let mut file = match conv.open(EVENTS, libc::O_RDONLY, 0) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    lock_fd(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_SH)?;
    let found = match first {
        0 | 1 => None,
        first => line_at(&mut file, first)?,
    };
    file.seek(std::io::SeekFrom::Start(found.map_or(0, |(at, _)| at)))?;
    let mut reader = std::io::BufReader::new(&file);
    let (mut events, mut last) = (Vec::new(), None::<u64>);
    let mut line = Vec::new();
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
        line.pop();
        let Some(seq) = seq_in(&line) else {
            bail!("a line isn't an event");
        };
        if last.is_some_and(|last| last.checked_add(1) != Some(seq)) {
            bail!("event {seq} is out of order");
        }
        last = Some(seq);
        if seq >= first {
            events.push(serde_json::from_slice(&line).context("reading an event")?);
        }
    }
    Ok(events)
}

/// Whether the file `name` in `conv` ends with `tail`, or is only `tail`
/// when `only`.
pub(super) fn ends_with(conv: &Dir, name: &str, tail: &[u8], only: bool) -> anyhow::Result<bool> {
    let mut file = conv.open(name, libc::O_RDONLY, 0)?;
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
