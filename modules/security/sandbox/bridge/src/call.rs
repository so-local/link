//! A caller's side of one call: the bridge's challenge, the caller's
//! proof and its action, then the bridge's answer: its own, or the error
//! envelope ([`Failure`]), which reads as an error that is that failure.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::Context;
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::AsyncReadExt;

use crate::{Binding, CHALLENGE_WAIT, Challenge, Failure, Why};

/// A caller's side of one call on `stream`: reads the bridge's challenge,
/// and writes `binding`'s proof, then `action`. The caller then reads the
/// bridge's answer ([`read_answer`]).
pub fn call<A: Serialize>(
    stream: &UnixStream,
    binding: &Binding,
    action: &A,
) -> anyhow::Result<()> {
    // A bridge sends its challenge at once: one that doesn't isn't one.
    let before = stream.read_timeout()?;
    stream.set_read_timeout(Some(CHALLENGE_WAIT))?;
    // A byte at a time: what the bridge says next (closed busy for a
    // newer connection) is read as the answer, never taken with this.
    let challenge: Challenge =
        answer_of(&line_of(stream, 4096)?).context("reading the bridge's challenge")?;
    stream.set_read_timeout(before)?;
    crate::PROTOCOL.accept("the bridge", challenge.protocol)?;
    // A call refused, or closed busy for a newer one, is answered before
    // the rest is read, and the connection closed: its answer, read next,
    // says why.
    let _ = write_line(stream, &binding.prove(&challenge.challenge));
    let _ = write_line(stream, action);
    Ok(())
}

/// Connects to the bridge at `socket`, calls `action` as `binding`'s, and
/// reads its answer, one JSON line of at most `max` bytes: an error that
/// is the [`Failure`] the bridge answered, if it did. A call answered
/// busy is made again ([`retrying`]).
pub fn call_at<A: Serialize, R: DeserializeOwned>(
    socket: &Path,
    binding: &Binding,
    action: &A,
    max: u64,
) -> anyhow::Result<R> {
    retrying(|| {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("reaching the bridge at {}", socket.display()))?;
        call(&stream, binding, action)?;
        read_answer(&stream, max)
    })
}

/// How many times [`retrying`] makes a call answered busy again, a
/// little longer apart each time: about 5 seconds in all.
pub const BUSY_TRIES: u32 = 20;

/// `call`, made again while the bridge answers it busy ([`Why::Busy`]:
/// nothing was done), [`BUSY_TRIES`] times at most: a bridge is busy for
/// as long as the calls it holds take, and a caller's own calls made at
/// once, past its peer's places for proving, close its oldest busy.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off while the bridge is busy"
)]
pub fn retrying<R>(mut call: impl FnMut() -> anyhow::Result<R>) -> anyhow::Result<R> {
    let mut tries = 0;
    loop {
        match call() {
            Err(err)
                if tries < BUSY_TRIES
                    && err
                        .downcast_ref::<Failure>()
                        .is_some_and(|failure| failure.why == Why::Busy) =>
            {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(25 * u64::from(tries)));
            }
            done => return done,
        }
    }
}

/// The bridge's answer on `stream`, one JSON line of at most `max` bytes
/// ([`answer_of`]).
pub fn read_answer<R: DeserializeOwned>(stream: &UnixStream, max: u64) -> anyhow::Result<R> {
    // A bridge sends nothing after a line it waits on the caller for (its
    // challenge), or after its answer: nothing more is taken with it.
    let mut line = Vec::new();
    BufReader::new(Read::take(stream, max))
        .read_until(b'\n', &mut line)
        .context("reading the bridge's answer")?;
    answer_of(&line)
}

/// A bridge's answer, `line`: an error that is the [`Failure`] it is,
/// when it's the error envelope; else `R`.
pub fn answer_of<R: DeserializeOwned>(line: &[u8]) -> anyhow::Result<R> {
    anyhow::ensure!(
        !line.iter().all(u8::is_ascii_whitespace),
        "the bridge closed the connection without answering"
    );
    let value: serde_json::Value =
        serde_json::from_slice(line).context("the bridge's answer isn't JSON")?;
    if value.get("error").is_some_and(serde_json::Value::is_string) {
        let failure: Failure =
            serde_json::from_value(value).context("reading the bridge's error")?;
        return Err(failure.into());
    }
    serde_json::from_value(value).context("the bridge's answer isn't one it gives")
}

/// [`call`], on a connection of tokio's.
pub async fn call_async<A: Serialize>(
    stream: &mut tokio::net::UnixStream,
    binding: &Binding,
    action: &A,
) -> anyhow::Result<()> {
    let challenge: Challenge =
        tokio::time::timeout(CHALLENGE_WAIT, read_answer_async(stream, 4096))
            .await
            .map_err(|_| anyhow::anyhow!("the bridge sent no challenge"))?
            .context("reading the bridge's challenge")?;
    crate::PROTOCOL.accept("the bridge", challenge.protocol)?;
    // As in [`call`]: a refusal is read next.
    let _ = crate::take::write_async(stream, &binding.prove(&challenge.challenge)).await;
    let _ = crate::take::write_async(stream, action).await;
    Ok(())
}

/// [`read_answer`], on a connection of tokio's: read a byte at a time, so
/// nothing after the line is taken from `stream`.
pub async fn read_answer_async<R: DeserializeOwned>(
    stream: &mut tokio::net::UnixStream,
    max: u64,
) -> anyhow::Result<R> {
    answer_of(
        &line_async(stream, max)
            .await
            .context("reading the bridge's answer")?,
    )
}

/// One line of JSON from `stream`, at most `max` bytes, as it is (an error
/// envelope too), read a byte at a time so nothing after it is taken
/// from the stream.
pub async fn read_line_async<T: DeserializeOwned>(
    stream: &mut tokio::net::UnixStream,
    max: u64,
    what: &str,
) -> anyhow::Result<T> {
    let line = line_async(stream, max)
        .await
        .with_context(|| format!("reading {what}"))?;
    anyhow::ensure!(!line.is_empty(), "the connection ended before {what}");
    serde_json::from_slice(&line).with_context(|| format!("reading {what}"))
}

/// `value` as one JSON line on `stream`.
pub async fn write_line_async(
    stream: &mut tokio::net::UnixStream,
    value: &impl Serialize,
) -> anyhow::Result<()> {
    crate::take::write_async(stream, value).await?;
    Ok(())
}

/// A line from `stream`, a byte at a time, at most `max` bytes: what
/// came before its end.
async fn line_async(stream: &mut tokio::net::UnixStream, max: u64) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut byte = [0u8];
    while (line.len() as u64) < max {
        if stream.read(&mut byte).await? == 0 || byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
    }
    Ok(line)
}

/// A line from `stream`, a byte at a time, at most `max` bytes: what
/// came before its end.
fn line_of(mut stream: &UnixStream, max: u64) -> anyhow::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut byte = [0u8];
    while (line.len() as u64) < max {
        // A signal (a child exiting, which a runtime watches) interrupts a
        // read that's waiting: it's tried again, never an error.
        let read = match stream.read(&mut byte) {
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            read => read.context("reading the bridge's challenge")?,
        };
        if read == 0 || byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
    }
    Ok(line)
}

/// `value` as one JSON line on `stream`.
fn write_line(mut stream: &UnixStream, value: &impl Serialize) -> anyhow::Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    Ok(())
}
