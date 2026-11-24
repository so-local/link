//! Connections a route's bridge keeps open between calls to the one host
//! a route reaches (a hosted model's provider, a secret's service: a
//! bot's API), so a model's turn or a bot's call doesn't pay for DNS, TCP
//! and TLS each time, and how an answer is read on one.
//!
//! Each is dialed as any other ([`super::dial::dial_out`]), and kept by
//! where it goes: its host, and the upstream it left through, so no two
//! sandboxes' ways out are mixed. One goes back to the pool only once its
//! answer was read to its end, with nothing after it; the program's own
//! connection still closes after each answer, as its head says.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::http::{AnswerHead, RequestHead, read_answer_head};
use anyhow::{bail, ensure};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// How long a connection waits in the pool, below what providers keep an
/// idle one open.
const IDLE_FOR: Duration = Duration::from_secs(30);
/// The most connections kept for one host through one upstream.
const MAX_IDLE: usize = 4;

/// The longest line of a chunked body (a chunk's size, a trailer).
const MAX_LINE: usize = 4096;
/// What an answer's body is read in.
const CHUNK: usize = 64 * 1024;

/// A connection kept: TLS to a route's host.
pub type Kept = tokio_rustls::client::TlsStream<super::dial::Dialed>;

/// Open connections, by where they go ([`Pool::key`]), newest last.
pub struct Pool<O> {
    idle: Mutex<Vec<(String, Instant, O)>>,
}

impl<O: AsyncRead + Unpin> Default for Pool<O> {
    fn default() -> Self {
        Self::new()
    }
}

impl<O: AsyncRead + Unpin> Pool<O> {
    pub const fn new() -> Self {
        Self {
            idle: Mutex::new(Vec::new()),
        }
    }

    /// Where a connection to `host` through `upstream` goes, as the pool
    /// keeps it.
    pub fn key(host: &str, upstream: &str) -> String {
        format!("{host} {upstream}")
    }

    /// An open connection kept as `key`, the newest, if one is still up.
    pub fn take(&self, key: &str) -> Option<O> {
        let mut idle = self.idle.lock().ok()?;
        let now = Instant::now();
        idle.retain(|(_, since, _)| now.duration_since(*since) < IDLE_FOR);
        while let Some(i) = idle.iter().rposition(|(k, _, _)| k == key) {
            let (_, _, mut conn) = idle.remove(i);
            if still_open(&mut conn) {
                return Some(conn);
            }
        }
        None
    }

    /// Keeps `conn`, as `key`, for the next call; the oldest of the
    /// key's goes when there are [`MAX_IDLE`].
    pub fn put(&self, key: String, conn: O) {
        let Ok(mut idle) = self.idle.lock() else {
            return;
        };
        if idle.iter().filter(|(k, _, _)| *k == key).count() >= MAX_IDLE
            && let Some(i) = idle.iter().position(|(k, _, _)| *k == key)
        {
            idle.remove(i);
        }
        idle.push((key, Instant::now(), conn));
    }
}

/// Whether an idle connection is still open: nothing to read, and not
/// closed by the other end.
fn still_open<O: AsyncRead + Unpin>(conn: &mut O) -> bool {
    let mut byte = [0u8; 1];
    let mut buf = tokio::io::ReadBuf::new(&mut byte);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    std::pin::Pin::new(conn)
        .poll_read(&mut cx, &mut buf)
        .is_pending()
}

/// A request's body length, from its head: Some when the head says it
/// once (a single `Content-Length`, or none and no `Transfer-Encoding`);
/// None when it doesn't, and the request is the last on its connection.
pub fn request_length(head: &[u8]) -> Option<u64> {
    let request = RequestHead::parse(head).ok()?;
    let mut length = None;
    for header in &request.headers {
        match header.name.as_str() {
            "transfer-encoding" => return None,
            "content-length" => {
                let value = header.value;
                if length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                length = Some(value.parse().ok()?);
            }
            _ => {}
        }
    }
    Some(length.unwrap_or(0))
}

/// Copies exactly `length` bytes of a request's body to `origin`: `rest`
/// (what came with its head) and then what `client` sends. Anything
/// after them is never sent.
pub async fn send_body<C, O>(
    client: &mut C,
    origin: &mut O,
    rest: &[u8],
    length: u64,
) -> anyhow::Result<()>
where
    C: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
{
    let first = rest
        .len()
        .min(usize::try_from(length).unwrap_or(usize::MAX));
    origin.write_all(&rest[..first]).await?;
    let mut left = length - first as u64;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        let n = client.read(&mut buf[..want]).await?;
        ensure!(n > 0, "the connection closed mid-body");
        origin.write_all(&buf[..n]).await?;
        left -= n as u64;
    }
    origin.flush().await?;
    Ok(())
}

/// How an answer's body ends.
#[derive(Debug, PartialEq)]
enum Body {
    None,
    Length(u64),
    Chunked,
    /// When the connection closes.
    Close,
}

/// An answer's head, read.
#[derive(Debug)]
struct Head {
    /// A 1xx, which another head follows.
    informational: bool,
    body: Body,
    /// Whether the connection may carry another request after it.
    keep_alive: bool,
    /// The head for the program: its connection closes after the answer.
    for_client: Vec<u8>,
}

fn parse_head(head: &[u8]) -> anyhow::Result<Head> {
    let answer = AnswerHead::parse(head).map_err(anyhow::Error::msg)?;
    let (status, version, code) = (answer.status, answer.version, answer.code);
    if code == 101 {
        bail!("an answer switching protocols, never asked for");
    }
    let informational = answer.informational();
    let mut keep_alive = version == "HTTP/1.1";
    let (mut length, mut chunked, mut encoded) = (None, false, false);
    let mut for_client = format!("{status}\r\n");
    for header in &answer.headers {
        let value = header.value;
        match header.name.as_str() {
            "connection" => {
                if value
                    .split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("close"))
                {
                    keep_alive = false;
                }
                continue;
            }
            "keep-alive" => continue,
            "content-length" => match (length, value.parse::<u64>()) {
                (None, Ok(n)) => length = Some(n),
                (Some(seen), Ok(n)) if seen == n => {}
                // Two lengths, or none that reads: to the connection's end.
                _ => encoded = true,
            },
            "transfer-encoding" => {
                encoded = true;
                chunked = value
                    .rsplit(',')
                    .next()
                    .is_some_and(|t| t.trim().eq_ignore_ascii_case("chunked"));
            }
            _ => {}
        }
        for_client.push_str(header.line);
        for_client.push_str("\r\n");
    }
    let body = if informational || code == 204 || code == 304 {
        Body::None
    } else if chunked {
        Body::Chunked
    } else if let (Some(n), false) = (length, encoded) {
        Body::Length(n)
    } else {
        Body::Close
    };
    // A length beside an encoding, or a body to the connection's end:
    // nothing after it can be told apart.
    if encoded && !(chunked && length.is_none()) || body == Body::Close {
        keep_alive = false;
    }
    let for_client = match informational {
        true => head.to_vec(),
        false => format!("{for_client}Connection: close\r\n\r\n").into_bytes(),
    };
    Ok(Head {
        informational,
        body,
        keep_alive,
        for_client,
    })
}

/// Reads `origin`'s answer and passes it to `client` as it comes, its head
/// saying the connection closes after it. Whether `origin` can carry
/// another request: the answer read to its end, and nothing after it.
/// When no answer comes, `client` gets a 502 saying so.
pub async fn answer<O, C>(origin: &mut O, client: &mut C, host: &str) -> anyhow::Result<bool>
where
    O: AsyncRead + Unpin,
    C: AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    let mut first = true;
    loop {
        let (head, rest) = match read_answer_head(origin, buf).await {
            Ok(read) => read,
            Err(err) => {
                if first {
                    let body = format!("Link's sandbox: {host} didn't answer: {err:#}\n");
                    let _ = client
                        .write_all(
                            format!(
                                "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\n\
                                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                }
                return Err(err);
            }
        };
        first = false;
        let head = parse_head(&head)?;
        client.write_all(&head.for_client).await?;
        if head.informational {
            buf = rest;
            continue;
        }
        let whole = copy_body(origin, client, rest, head.body).await?;
        client.flush().await?;
        return Ok(whole && head.keep_alive);
    }
}

/// Copies an answer's body, `rest` of it read already, to `client`.
/// Whether it ended where its head said, with nothing after it.
async fn copy_body<O, C>(
    origin: &mut O,
    client: &mut C,
    rest: Vec<u8>,
    body: Body,
) -> anyhow::Result<bool>
where
    O: AsyncRead + Unpin,
    C: AsyncWrite + Unpin,
{
    match body {
        Body::None => Ok(rest.is_empty()),
        Body::Close => {
            client.write_all(&rest).await?;
            tokio::io::copy(origin, client).await?;
            Ok(false)
        }
        Body::Length(length) => {
            let first = rest
                .len()
                .min(usize::try_from(length).unwrap_or(usize::MAX));
            client.write_all(&rest[..first]).await?;
            let mut left = length - first as u64;
            let mut buf = vec![0u8; CHUNK];
            while left > 0 {
                let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let n = origin.read(&mut buf[..want]).await?;
                ensure!(n > 0, "the answer ended early");
                client.write_all(&buf[..n]).await?;
                left -= n as u64;
            }
            Ok(first == rest.len())
        }
        Body::Chunked => {
            let mut chunks = Chunks::default();
            if let Some(end) = chunks.feed(&rest)? {
                client.write_all(&rest[..end]).await?;
                return Ok(end == rest.len());
            }
            client.write_all(&rest).await?;
            // One buffer for every read: a streamed answer is many small
            // reads, and none clears 64 KiB first.
            let mut buf = vec![0u8; CHUNK];
            loop {
                let n = origin.read(&mut buf).await?;
                ensure!(n > 0, "the answer ended early");
                let data = &buf[..n];
                if let Some(end) = chunks.feed(data)? {
                    client.write_all(&data[..end]).await?;
                    return Ok(end == n);
                }
                client.write_all(data).await?;
            }
        }
    }
}

/// Where a chunked body is: in a line (a chunk's size, the end of its
/// data, a trailer), or in a chunk's data.
#[derive(Debug, Default)]
enum Part {
    #[default]
    Size,
    Data(u64),
    DataEnd,
    Trailers,
}

/// A chunked body, followed as it's read, to find its end.
#[derive(Debug, Default)]
struct Chunks {
    part: Part,
    line: Vec<u8>,
}

impl Chunks {
    /// Follows `data`, the body's next bytes: where in it the body ends,
    /// if it does.
    fn feed(&mut self, data: &[u8]) -> anyhow::Result<Option<usize>> {
        let mut i = 0;
        while i < data.len() {
            if let Part::Data(left) = &mut self.part {
                let take = (*left).min((data.len() - i) as u64);
                i += take as usize;
                *left -= take;
                if *left == 0 {
                    self.part = Part::DataEnd;
                }
                continue;
            }
            let byte = data[i];
            i += 1;
            if byte != b'\n' {
                self.line.push(byte);
                ensure!(
                    self.line.len() <= MAX_LINE,
                    "a chunked body's line over 4 KiB"
                );
                continue;
            }
            let mut line = std::mem::take(&mut self.line);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.part = match self.part {
                Part::Size => {
                    let size = line.split(|b| *b == b';').next().unwrap_or_default();
                    let size = std::str::from_utf8(size).unwrap_or_default().trim();
                    ensure!(
                        !size.is_empty()
                            && size.len() <= 15
                            && size.bytes().all(|b| b.is_ascii_hexdigit()),
                        "a chunk without a size"
                    );
                    match u64::from_str_radix(size, 16)? {
                        0 => Part::Trailers,
                        n => Part::Data(n),
                    }
                }
                Part::DataEnd => {
                    ensure!(line.is_empty(), "a chunk longer than it said");
                    Part::Size
                }
                Part::Trailers if line.is_empty() => return Ok(Some(i)),
                Part::Trailers => Part::Trailers,
                Part::Data(_) => unreachable!("data is read above"),
            };
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunked_body_ends_where_it_says_however_it_arrives() {
        let body =
            b"4\r\nWiki\r\n6;ext=1\r\npedia \r\nE\r\nin \r\n\r\nchunks.\r\n0\r\nX-T: 1\r\n\r\n";
        for split in 0..body.len() {
            let mut chunks = Chunks::default();
            let (a, b) = body.split_at(split);
            let mut after = b.to_vec();
            after.extend_from_slice(b"HTTP/1.1");
            let end = match chunks.feed(a).unwrap() {
                Some(end) => end,
                None => split + chunks.feed(&after).unwrap().expect("its end"),
            };
            assert_eq!(end, body.len(), "split at {split}");
        }
        for bad in [&b"zz\r\n"[..], b"4\r\nWikiX\r\n", b"\r\n"] {
            assert!(Chunks::default().feed(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_answer_head_says_how_its_body_ends() {
        let head = |h: &str| parse_head(h.as_bytes()).unwrap();
        let got = head("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\n");
        assert_eq!((got.body, got.keep_alive), (Body::Length(5), true));
        assert_eq!(
            got.for_client,
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n"
        );
        let got = head("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert_eq!((got.body, got.keep_alive), (Body::Chunked, true));
        let got = head("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n");
        assert!(!got.keep_alive);
        let got = head("HTTP/1.1 200 OK\r\n\r\n");
        assert_eq!((got.body, got.keep_alive), (Body::Close, false));
        let got =
            head("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert_eq!((got.body, got.keep_alive), (Body::Chunked, false));
        let got = head("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\n");
        assert_eq!((got.body, got.keep_alive), (Body::Close, false));
        let got = head("HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\n");
        assert!(!got.keep_alive);
        let got = head("HTTP/1.1 204 No Content\r\n\r\n");
        assert_eq!((got.body, got.keep_alive), (Body::None, true));
        assert!(head("HTTP/1.1 100 Continue\r\n\r\n").informational);
        assert!(parse_head(b"HTTP/1.1 101 Switching\r\n\r\n").is_err());
    }

    #[test]
    fn a_request_says_its_length_once_or_is_the_last() {
        assert_eq!(request_length(b"GET / HTTP/1.1\r\nX: y\r\n\r\n"), Some(0));
        assert_eq!(
            request_length(b"POST / HTTP/1.1\r\ncontent-length: 12\r\n\r\n"),
            Some(12)
        );
        for last in [
            &b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"[..],
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: +1\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n",
        ] {
            assert_eq!(
                request_length(last),
                None,
                "{:?}",
                String::from_utf8_lossy(last)
            );
        }
    }

    #[tokio::test]
    async fn an_answer_read_to_its_end_leaves_its_connection_for_the_next() {
        let pool = Pool::new();
        let (mut origin, mut server) = tokio::io::duplex(64 * 1024);
        server
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n0\r\n\r\n")
            .await
            .unwrap();
        let mut client = Vec::new();
        assert!(answer(&mut origin, &mut client, "h").await.unwrap());
        let client = String::from_utf8(client).unwrap();
        assert!(client.contains("Connection: close\r\n"), "{client}");
        assert!(client.ends_with("2\r\nhi\r\n0\r\n\r\n"), "{client}");
        pool.put("h".into(), origin);
        let origin = pool.take("h").expect("still open");
        assert!(pool.take("h").is_none());
        // Closed by the other end while it waited: never handed out.
        pool.put("h".into(), origin);
        drop(server);
        assert!(pool.take("h").is_none());
        // More than it said: not kept.
        let (mut origin, mut server) = tokio::io::duplex(64 * 1024);
        server
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhiHTTP/1.1 200 OK")
            .await
            .unwrap();
        let mut client = Vec::new();
        assert!(!answer(&mut origin, &mut client, "h").await.unwrap());
        // Nothing at all: a 502 for the program.
        let (mut origin, server) = tokio::io::duplex(64);
        drop(server);
        let mut client = Vec::new();
        assert!(answer(&mut origin, &mut client, "h").await.is_err());
        assert!(client.starts_with(b"HTTP/1.1 502"));
    }
}
