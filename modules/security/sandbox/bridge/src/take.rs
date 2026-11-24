//! A bridge's side of one call: the challenge, the caller's [`Proven`]
//! checked within [`PROOF_WAIT`], then its action. [`crate::serve`] takes
//! every call this way; [`take`] and [`take_async`] are the same for a
//! program that takes its calls itself.

use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    CALL_MAX, CALL_WAIT, Challenge, Failure, Identity, Keys, PROOF_WAIT, PROVEN_MAX, Proven, Why,
    challenge, checked, parsed,
};

/// A bridge's side of one call on `stream`: sends a challenge, reads who's
/// calling within [`PROOF_WAIT`] and checks its proof against `keys`, then
/// reads its action, at the stream's own read timeout. Who called and what
/// they asked; an error, the call refused ([`Failure`], [`Why::Refused`]),
/// for an agent not bound here, a proof that isn't its, or none in time.
/// The bridge then writes its answer.
pub fn take<A: DeserializeOwned>(
    stream: &UnixStream,
    keys: &dyn Keys,
) -> anyhow::Result<(Identity, A)> {
    let challenge = challenge()?;
    let mut line = serde_json::to_vec(&Challenge::new(challenge.clone()))?;
    line.push(b'\n');
    (&mut &*stream).write_all(&line)?;
    let before = stream.read_timeout()?;
    let mut read = std::io::BufReader::new(Timed {
        stream,
        until: Some(Instant::now() + PROOF_WAIT),
    });
    let proven = line_of(&mut read, PROVEN_MAX, "who's calling", late)?;
    let proven: Proven = parsed(&proven, "who's calling")?;
    let who = checked(&challenge, &proven, keys)?;
    read.get_mut().until = None;
    stream.set_read_timeout(before)?;
    let action = line_of(&mut read, CALL_MAX, "the call's action", || {
        Failure::new(Why::Refused, "the call's action didn't come in time")
    })?;
    Ok((who, parsed(&action, "the call's action")?))
}

/// [`take`], on a connection of tokio's.
///
/// Its action comes within [`CALL_WAIT`], all of it. Its keys are looked
/// up off the task this runs on, where they may be read from files.
pub async fn take_async<A: DeserializeOwned>(
    stream: &mut tokio::net::UnixStream,
    keys: &(impl Keys + Clone + 'static),
) -> anyhow::Result<(Identity, A)> {
    let keys: std::sync::Arc<dyn Keys> = std::sync::Arc::new(keys.clone());
    let (read, mut write) = stream.split();
    let mut read = tokio::io::BufReader::new(read);
    let who = prove(&mut read, &mut write, &keys).await?;
    let action = tokio::time::timeout(
        CALL_WAIT,
        line_async(&mut read, CALL_MAX, "the call's action"),
    )
    .await
    .map_err(|_| Failure::new(Why::Refused, "the call's action didn't come in time"))??;
    Ok((who, parsed(&action, "the call's action")?))
}

/// Sends a challenge on `write` and checks who's calling, read from
/// `read`, against `keys`, all within [`PROOF_WAIT`]: who called, or the
/// call refused.
pub(crate) async fn prove<R, W>(
    read: &mut R,
    write: &mut W,
    keys: &std::sync::Arc<dyn Keys>,
) -> Result<Identity, Failure>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let exchange = async {
        let challenge = challenge().map_err(|err| Failure::new(Why::Failed, format!("{err:#}")))?;
        write_async(write, &Challenge::new(challenge.clone()))
            .await
            .map_err(|err| Failure::new(Why::Refused, format!("sending the challenge: {err}")))?;
        let line = line_async(read, PROVEN_MAX, "who's calling").await?;
        let proven = parsed(&line, "who's calling")?;
        // A key may be read from a file: looked up where that may block.
        let keys = keys.clone();
        tokio::task::spawn_blocking(move || checked(&challenge, &proven, keys.as_ref()))
            .await
            .map_err(|err| Failure::new(Why::Failed, format!("checking the proof: {err}")))?
    };
    tokio::time::timeout(PROOF_WAIT, exchange)
        .await
        .unwrap_or_else(|_| Err(late()))
}

/// The refusal of a call that didn't prove who's calling in time.
fn late() -> Failure {
    Failure::new(
        Why::Refused,
        format!(
            "no proof of who's calling within {} seconds: the call isn't taken",
            PROOF_WAIT.as_secs()
        ),
    )
}

/// One line from `read`, without its newline, at most `max` bytes: a
/// refusal for one longer, or none.
pub(crate) async fn line_async<R: AsyncBufRead + Unpin>(
    read: &mut R,
    max: u64,
    what: &str,
) -> Result<Vec<u8>, Failure> {
    let mut line = Vec::new();
    (&mut *read)
        .take(max)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|err| Failure::new(Why::Refused, format!("reading {what}: {err}")))?;
    ended(line, max, what)
}

/// One line from `read`, as [`line_async`]; `late` when it didn't come
/// in time.
fn line_of(
    read: &mut impl BufRead,
    max: u64,
    what: &str,
    late: impl FnOnce() -> Failure,
) -> Result<Vec<u8>, Failure> {
    use std::io::ErrorKind::{TimedOut, WouldBlock};
    let mut line = Vec::new();
    match (&mut *read).take(max).read_until(b'\n', &mut line) {
        Ok(_) => ended(line, max, what),
        Err(err) if matches!(err.kind(), TimedOut | WouldBlock) => Err(late()),
        Err(err) => Err(Failure::new(Why::Refused, format!("reading {what}: {err}"))),
    }
}

/// `line` as read, up to `max`: without its newline, or refused.
fn ended(mut line: Vec<u8>, max: u64, what: &str) -> Result<Vec<u8>, Failure> {
    match line.last() {
        Some(b'\n') => {
            line.pop();
        }
        _ if line.len() as u64 >= max => {
            return Err(Failure::new(
                Why::Refused,
                format!("{what} is longer than {max} bytes"),
            ));
        }
        _ => {}
    }
    if line.is_empty() {
        return Err(Failure::new(
            Why::Refused,
            format!("the connection ended before {what}"),
        ));
    }
    Ok(line)
}

/// `value` as one JSON line on `write`.
pub(crate) async fn write_async<W: AsyncWrite + Unpin>(
    write: &mut W,
    value: &impl serde::Serialize,
) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    write.write_all(&line).await?;
    write.flush().await
}

/// A connection read until a deadline, when there is one: each read
/// waits only what's left of it, so a caller sending a byte now and then
/// is cut off all the same.
struct Timed<'a> {
    stream: &'a UnixStream,
    until: Option<Instant>,
}

impl Read for Timed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(until) = self.until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            self.stream
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
        }
        (&mut &*self.stream).read(buf)
    }
}
