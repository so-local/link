//! Reading an answer's body as it passes: its framing and chunks, and
//! each stream event or whole answer's usage taken to its fields.

use super::*;

impl Body {
    /// The body's next bytes, its events collected; whether it ended.
    pub(super) fn feed(&mut self, bytes: &[u8], events: &mut Vec<Vec<u8>>) -> bool {
        match &mut self.framing {
            Framing::Plain => {
                self.reader.feed(bytes, events);
                false
            }
            Framing::Chunked(chunk) => {
                let mut i = 0;
                while i < bytes.len() {
                    match chunk {
                        Chunk::Data(left) => {
                            let take = (*left).min((bytes.len() - i) as u64) as usize;
                            self.reader.feed(&bytes[i..i + take], events);
                            i += take;
                            *left -= take as u64;
                            if *left == 0 {
                                *chunk = Chunk::DataEnd(Vec::new());
                            }
                        }
                        Chunk::DataEnd(line) => {
                            let byte = bytes[i];
                            i += 1;
                            if byte == b'\n' {
                                *chunk = Chunk::Size(Vec::new());
                            } else {
                                line.push(byte);
                                if line.len() > MAX_LINE {
                                    return true;
                                }
                            }
                        }
                        Chunk::Size(line) => {
                            let byte = bytes[i];
                            i += 1;
                            if byte != b'\n' {
                                line.push(byte);
                                if line.len() > MAX_LINE {
                                    return true;
                                }
                                continue;
                            }
                            let text = String::from_utf8_lossy(line);
                            let size = text.split(';').next().unwrap_or_default().trim();
                            match u64::from_str_radix(size, 16) {
                                Ok(0) => {
                                    *chunk = Chunk::Trailers;
                                    return true;
                                }
                                Ok(n) => *chunk = Chunk::Data(n),
                                Err(_) => return true,
                            }
                        }
                        Chunk::Trailers => return true,
                    }
                }
                false
            }
        }
    }
}

impl Reader {
    fn feed(&mut self, bytes: &[u8], events: &mut Vec<Vec<u8>>) {
        match self {
            Reader::Whole(scan) => scan.feed(bytes),
            Reader::Events { line, data, over } => {
                for piece in bytes.split_inclusive(|b| *b == b'\n') {
                    if !*over {
                        line.extend_from_slice(piece);
                        if line.len() + data.len() > MAX_EVENT {
                            *over = true;
                            line.clear();
                            data.clear();
                        }
                    }
                    if piece.last() != Some(&b'\n') {
                        continue;
                    }
                    let text = std::mem::take(line);
                    let text = text.strip_suffix(b"\n").unwrap_or(&text);
                    let text = text.strip_suffix(b"\r").unwrap_or(text);
                    if text.is_empty() {
                        // The event's end.
                        if !*over && names_usage(data) {
                            events.push(std::mem::take(data));
                        }
                        data.clear();
                        *over = false;
                    } else if !*over && let Some(payload) = text.strip_prefix(b"data:") {
                        if !data.is_empty() {
                            data.push(b'\n');
                        }
                        data.extend_from_slice(payload.strip_prefix(b" ").unwrap_or(payload));
                    }
                }
            }
            Reader::Lines { line, over } => {
                for piece in bytes.split_inclusive(|b| *b == b'\n') {
                    if !*over {
                        line.extend_from_slice(piece);
                        if line.len() > MAX_EVENT {
                            *over = true;
                            line.clear();
                        }
                    }
                    if piece.last() == Some(&b'\n') {
                        if !*over && names_usage(line) {
                            events.push(std::mem::take(line));
                        }
                        line.clear();
                        *over = false;
                    }
                }
            }
        }
    }

    /// The body ended: what's left of it, as an event if it may hold
    /// usage.
    pub(super) fn end(&mut self, events: &mut Vec<Vec<u8>>) {
        match self {
            Reader::Whole(scan) => events.extend(scan.usage()),
            Reader::Events { line, data, over } => {
                if !*over {
                    if let Some(payload) = line.strip_prefix(b"data:") {
                        if !data.is_empty() {
                            data.push(b'\n');
                        }
                        data.extend_from_slice(payload.strip_prefix(b" ").unwrap_or(payload));
                    }
                    if names_usage(data) {
                        events.push(std::mem::take(data));
                    }
                }
            }
            Reader::Lines { line, over } => {
                if !*over && names_usage(line) {
                    events.push(std::mem::take(line));
                }
            }
        }
    }
}

/// Whether an event may hold usage, before it's parsed: most of a
/// stream's are a piece of the answer, and don't.
fn names_usage(data: &[u8]) -> bool {
    const KEYS: [&[u8]; 2] = [b"\"usage\"", b"\"eval_count\""];
    KEYS.iter()
        .any(|key| data.windows(key.len()).any(|w| w == *key))
}
