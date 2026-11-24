//! The one HTTP parser: every head the proxy and the bridges behind it
//! read ([`head`]), the hosts requests name ([`host`]), and the answers
//! they write themselves.

mod head;
mod host;

pub use head::{AnswerHead, Header, RequestHead};
pub use host::{ends_in_number, host_port, url_ipv4};

use anyhow::ensure;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The largest request head the proxy reads.
pub const MAX_HEAD: usize = 16 * 1024;

/// How long the proxy waits for a request's head.
pub const HEAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn reply<S: tokio::io::AsyncWrite + Unpin>(client: &mut S, status: &str) {
    let _ = client
        .write_all(
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await;
}

/// The request head (through its blank line), and what came after it.
/// A head that isn't [`plain`] is refused.
pub async fn read_head<S: tokio::io::AsyncRead + Unpin>(
    client: &mut S,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            ensure!(
                plain(&buf),
                "refused: a request head with a bare line end or a control character"
            );
            return Ok((buf, rest));
        }
        ensure!(buf.len() <= MAX_HEAD, "request head too large");
        let n = client.read(&mut chunk).await?;
        ensure!(n > 0, "the connection closed before its request");
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// The largest answer head a route reads from where it sent a request.
pub const MAX_ANSWER_HEAD: usize = 64 * 1024;

/// Reads an answer head from `origin`, after `buf` (what was read
/// already): the head, and what came after it. Read as it comes, for
/// [`AnswerHead::parse`].
pub async fn read_answer_head<O: tokio::io::AsyncRead + Unpin>(
    origin: &mut O,
    mut buf: Vec<u8>,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok((buf, rest));
        }
        ensure!(buf.len() <= MAX_ANSWER_HEAD, "an answer head over 64 KiB");
        let n = origin.read(&mut chunk).await?;
        ensure!(n > 0, "the connection closed before its answer");
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Whether a request head means one thing to every server: each line
/// ended by `\r\n`, never a bare `\r` or `\n` (which some servers,
/// Go's and Node's, take as a line's end, so a header the proxy checked
/// could hide another, or a second request), and no other control
/// character but tab.
pub fn plain(head: &[u8]) -> bool {
    head.iter().enumerate().all(|(i, b)| match b {
        b'\r' => head.get(i + 1) == Some(&b'\n'),
        b'\n' => i > 0 && head[i - 1] == b'\r',
        b'\t' => true,
        b => *b >= 0x20 && *b != 0x7f,
    })
}
