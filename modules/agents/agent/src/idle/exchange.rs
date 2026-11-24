//! What one connection through an agent's front door carries, as
//! HTTP/1.1 frames it: whether an answer is still coming. The door reads
//! only how each message is framed (its head, then a body by
//! `Content-Length`, in chunks, or until the connection ends), never what
//! it says, and an agent answering is busy however long its answer
//! pauses. What it can't frame (a head too long, a length that isn't one,
//! a protocol the connection was upgraded or tunnelled to) is taken as
//! an answer still coming until the connection ends: never cut. Nor is
//! there a ceiling: an answer that never ends keeps its agent awake, as
//! a turn that never ends does.

use std::collections::VecDeque;

/// The longest head the door frames; past it, it frames nothing more.
const HEAD: usize = 64 * 1024;

/// The most asks a connection holds unanswered; past it, it frames
/// nothing more.
const WAITING: usize = 1024;

/// One connection's exchange: what its client asked, what came back.
#[derive(Default)]
pub struct Exchange {
    asks: Framing,
    answers: Framing,
    /// Each ask not yet answered, oldest first, by its method.
    waiting: VecDeque<Method>,
}

impl Exchange {
    /// Bytes its client sent.
    pub fn asked(&mut self, bytes: &[u8]) {
        let waiting = &mut self.waiting;
        self.asks.feed(bytes, |head| {
            let ask = Ask::of(head)?;
            if waiting.len() >= WAITING {
                return None;
            }
            waiting.push_back(ask.method);
            Some(ask.body)
        });
    }

    /// Bytes the agent sent back.
    pub fn answered(&mut self, bytes: &[u8]) {
        let waiting = &mut self.waiting;
        self.answers.feed(bytes, |head| {
            let status = status_of(head)?;
            match status {
                // Switching protocols: what follows isn't HTTP's.
                101 => None,
                // Interim: the answer itself is still to come.
                100..=199 => Some(Body::None),
                // An answer to no ask (one before its ask is whole, a 408
                // say): which ask a later answer is for is lost.
                _ => match waiting.pop_front()? {
                    // A tunnel opened: what follows isn't HTTP's.
                    Method::Connect if (200..300).contains(&status) => None,
                    Method::Head => Some(Body::None),
                    _ if status == 204 || status == 304 => Some(Body::None),
                    _ => Some(body_of(head).unwrap_or(Body::UntilEnd)),
                },
            }
        });
    }

    /// What it framed so far, for the log: where its asks and answers
    /// are, and how many asks wait.
    pub fn framed(&self) -> String {
        format!(
            "asks {}, answers {}, {} waiting",
            self.asks.at(),
            self.answers.at(),
            self.waiting.len()
        )
    }

    /// Whether an answer is still coming: an ask unanswered, an answer's
    /// body not all through, or what it couldn't frame.
    pub fn answering(&self) -> bool {
        !self.waiting.is_empty() || self.answers.within() || self.asks.lost() || self.answers.lost()
    }
}

/// A message's body, as its head frames it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Body {
    None,
    Length(u64),
    Chunked,
    /// Until the connection ends: an answer with no length.
    UntilEnd,
}

/// An ask's method, as its answer's framing needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Method {
    /// Its answer has no body.
    Head,
    /// Its answer, a success, opens a tunnel.
    Connect,
    Other,
}

/// An ask's head, as the door needs it.
struct Ask {
    method: Method,
    body: Body,
}

impl Ask {
    fn of(head: &[u8]) -> Option<Ask> {
        let text = std::str::from_utf8(head).ok()?;
        let method = text.split(' ').next()?;
        // An ask's body is framed or there is none (RFC 9112 6.3).
        let body = match body_of(head)? {
            Body::UntilEnd => Body::None,
            body => body,
        };
        let method = match method {
            "HEAD" => Method::Head,
            "CONNECT" => Method::Connect,
            _ => Method::Other,
        };
        Some(Ask { method, body })
    }
}

/// An answer's status, from its head's first line.
fn status_of(head: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(head).ok()?;
    let line = text.lines().next()?;
    let mut words = line.split(' ');
    words.next().filter(|v| v.starts_with("HTTP/1."))?;
    words.next()?.parse().ok()
}

/// A head's body: chunked, of a length, or neither (`UntilEnd`). None if
/// a length isn't one.
fn body_of(head: &[u8]) -> Option<Body> {
    let text = std::str::from_utf8(head).ok()?;
    let mut length = None;
    for line in text.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            if value
                .rsplit(',')
                .next()
                .is_some_and(|last| last.trim().eq_ignore_ascii_case("chunked"))
            {
                return Some(Body::Chunked);
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse::<u64>().ok()?);
        }
    }
    Some(length.map_or(Body::UntilEnd, Body::Length))
}

/// Where a stream of messages one way is.
#[derive(Default)]
enum Framing {
    /// Reading a head: what of it came so far.
    #[default]
    Between,
    Head(Vec<u8>),
    /// A body's bytes still to come.
    Length(u64),
    /// A chunk's size line, so far.
    Size(Vec<u8>),
    /// A chunk's bytes still to come, its line's end among them.
    Chunk(u64),
    /// After the last chunk: its trailer's lines, the one so far.
    Trailer(Vec<u8>),
    UntilEnd,
    /// What it couldn't frame: nothing more is.
    Lost,
}

impl Framing {
    /// Reads `bytes` on, `head` saying each head's body (None: it can't).
    fn feed(&mut self, mut bytes: &[u8], mut head: impl FnMut(&[u8]) -> Option<Body>) {
        while !bytes.is_empty() {
            let state = std::mem::take(self);
            *self = match state {
                Framing::Between => Framing::Head(Vec::new()),
                Framing::Head(mut so_far) => {
                    let (line, rest, whole) = line_of(bytes);
                    so_far.extend_from_slice(line);
                    bytes = rest;
                    if so_far.len() > HEAD {
                        // Too long a head, of one line or of many.
                        Framing::Lost
                    } else if !whole {
                        Framing::Head(so_far)
                    } else if so_far == b"\r\n" || so_far == b"\n" {
                        // A blank line between messages.
                        Framing::Between
                    } else if so_far.ends_with(b"\r\n\r\n") || so_far.ends_with(b"\n\n") {
                        match head(&so_far) {
                            None => Framing::Lost,
                            Some(Body::None | Body::Length(0)) => Framing::Between,
                            Some(Body::Length(n)) => Framing::Length(n),
                            Some(Body::Chunked) => Framing::Size(Vec::new()),
                            Some(Body::UntilEnd) => Framing::UntilEnd,
                        }
                    } else {
                        Framing::Head(so_far)
                    }
                }
                Framing::Length(n) => {
                    let took = n.min(bytes.len() as u64);
                    bytes = &bytes[took as usize..];
                    match n - took {
                        0 => Framing::Between,
                        left => Framing::Length(left),
                    }
                }
                Framing::Size(mut so_far) => {
                    let (line, rest, whole) = line_of(bytes);
                    so_far.extend_from_slice(line);
                    bytes = rest;
                    match (whole, chunk_size(&so_far)) {
                        (false, _) if so_far.len() > HEAD => Framing::Lost,
                        (false, _) => Framing::Size(so_far),
                        (true, None) => Framing::Lost,
                        (true, Some(0)) => Framing::Trailer(Vec::new()),
                        // Its bytes, then its line's end.
                        (true, Some(n)) => Framing::Chunk(n.saturating_add(2)),
                    }
                }
                Framing::Chunk(n) => {
                    let took = n.min(bytes.len() as u64);
                    bytes = &bytes[took as usize..];
                    match n - took {
                        0 => Framing::Size(Vec::new()),
                        left => Framing::Chunk(left),
                    }
                }
                Framing::Trailer(mut so_far) => {
                    let (line, rest, whole) = line_of(bytes);
                    so_far.extend_from_slice(line);
                    bytes = rest;
                    match whole {
                        // The blank line that ends the message.
                        true if so_far == b"\r\n" || so_far == b"\n" => Framing::Between,
                        true => Framing::Trailer(Vec::new()),
                        false if so_far.len() > HEAD => Framing::Lost,
                        false => Framing::Trailer(so_far),
                    }
                }
                Framing::UntilEnd => {
                    bytes = &[];
                    Framing::UntilEnd
                }
                Framing::Lost => {
                    bytes = &[];
                    Framing::Lost
                }
            };
        }
    }

    /// Where it is, in a word or two.
    fn at(&self) -> String {
        match self {
            Framing::Between => "between".into(),
            Framing::Head(so_far) => format!("in a head ({} bytes)", so_far.len()),
            Framing::Length(left) => format!("in a body ({left} bytes left)"),
            Framing::Size(_) => "at a chunk's size".into(),
            Framing::Chunk(left) => format!("in a chunk ({left} bytes left)"),
            Framing::Trailer(_) => "in a trailer".into(),
            Framing::UntilEnd => "in a body until its end".into(),
            Framing::Lost => "lost".into(),
        }
    }

    /// Whether it's within a message: a body not all through.
    fn within(&self) -> bool {
        !matches!(self, Framing::Between | Framing::Head(_))
    }

    fn lost(&self) -> bool {
        matches!(self, Framing::Lost)
    }
}

/// `bytes` up to and with its first line's end, the rest, and whether
/// that line ended.
fn line_of(bytes: &[u8]) -> (&[u8], &[u8], bool) {
    match bytes.iter().position(|b| *b == b'\n') {
        Some(at) => (&bytes[..=at], &bytes[at + 1..], true),
        None => (bytes, &[], false),
    }
}

/// A chunk's size, from its line: hexadecimal, before any extension.
fn chunk_size(line: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(line).ok()?;
    let size = text.trim_end().split(';').next()?.trim();
    u64::from_str_radix(size, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASK: &[u8] =
        b"POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nContent-Length: 4\r\n\r\nabcd";

    /// An answer in chunks is coming until its last chunk and the line
    /// after it, however its bytes are split; then it's answered.
    #[test]
    fn a_chunked_answer_is_coming_until_its_last_chunk() {
        let answer = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nI see \r\n19\r\n2 messages; the last: Hi.\r\n0\r\n\r\n";
        for split in [1, 2, 3, 7, answer.len()] {
            let mut exchange = Exchange::default();
            assert!(!exchange.answering());
            exchange.asked(ASK);
            assert!(exchange.answering(), "asked");
            for (i, piece) in answer.chunks(split).enumerate() {
                assert!(exchange.answering(), "split {split}, before piece {i}");
                exchange.answered(piece);
            }
            assert!(!exchange.answering(), "split {split}: all through");
        }
    }

    /// An ask half sent isn't one yet; an answer of a length is coming
    /// until its last byte; the next ask on the connection starts again.
    #[test]
    fn asks_and_answers_on_one_connection() {
        let mut exchange = Exchange::default();
        exchange.asked(&ASK[..10]);
        assert!(!exchange.answering());
        exchange.asked(&ASK[10..]);
        assert!(exchange.answering());
        exchange.answered(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhel");
        assert!(exchange.answering());
        exchange.answered(b"lo");
        assert!(!exchange.answering());
        exchange.asked(b"GET / HTTP/1.1\r\n\r\n");
        assert!(exchange.answering());
        exchange.answered(b"HTTP/1.1 204 No Content\r\n\r\n");
        assert!(!exchange.answering());
    }

    /// A HEAD's answer has no body, whatever its length says; an interim
    /// answer isn't the answer.
    #[test]
    fn heads_and_interim_answers_have_no_body() {
        let mut exchange = Exchange::default();
        exchange.asked(b"HEAD / HTTP/1.1\r\n\r\n");
        exchange.answered(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n");
        assert!(!exchange.answering());
        exchange.asked(ASK);
        exchange.answered(b"HTTP/1.1 100 Continue\r\n\r\n");
        assert!(exchange.answering());
        exchange.answered(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        assert!(!exchange.answering());
    }

    /// What it can't frame is an answer coming until the connection ends.
    #[test]
    fn what_it_cant_frame_is_never_cut() {
        let mut tunnel = Exchange::default();
        tunnel.asked(b"CONNECT example.com:443 HTTP/1.1\r\n\r\n");
        tunnel.answered(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        assert!(tunnel.answering());
        let mut upgraded = Exchange::default();
        upgraded.asked(b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\n\r\n");
        upgraded.answered(b"HTTP/1.1 101 Switching Protocols\r\n\r\n");
        assert!(upgraded.answering());
        let mut unlengthed = Exchange::default();
        unlengthed.asked(ASK);
        unlengthed.answered(b"HTTP/1.1 200 OK\r\n\r\nall of it, until it closes");
        assert!(unlengthed.answering());
        let mut garbled = Exchange::default();
        garbled.asked(b"POST / HTTP/1.1\r\nContent-Length: lots\r\n\r\n");
        assert!(garbled.answering());
        let mut endless = Exchange::default();
        endless.asked(&vec![b'a'; HEAD + 1]);
        assert!(endless.answering());
        // A head of short lines that never ends: framed no further once
        // past the longest, holding no more of it.
        let mut lines = Exchange::default();
        lines.asked(ASK);
        lines.answered(b"HTTP/1.1 200 OK\r\n");
        for _ in 0..(HEAD / 6 + 1) {
            lines.answered(b"a: b\r\n");
        }
        assert!(matches!(lines.answers, Framing::Lost));
        lines.answered(b"\r\n");
        assert!(lines.answering());
        // An answer before any ask, then an ask: lost, never stranded.
        let mut early = Exchange::default();
        early.answered(b"HTTP/1.1 408 Request Timeout\r\nContent-Length: 0\r\n\r\n");
        early.asked(ASK);
        assert!(early.answering());
        // Asks past the most held: lost.
        let mut many = Exchange::default();
        for _ in 0..=WAITING {
            many.asked(b"GET / HTTP/1.1\r\n\r\n");
        }
        assert!(matches!(many.asks, Framing::Lost));
    }
}
