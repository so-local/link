//! One JSON message between two of Link's programs: a request a child
//! reads on its standard input and the reply it writes to its standard
//! output, each bounded by [`MAX_MESSAGE_BYTES`], and the child stopped,
//! with everything it started, when the exchange returns.

use link_secretive::process::OwnedChild;
use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// The largest message either side sends: 1 MiB.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub fn read<T: serde::de::DeserializeOwned>(reader: impl Read) -> io::Result<T> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_MESSAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io::Error::other)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::other("protocol message exceeds 1 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
pub fn write<T: serde::Serialize>(mut writer: impl Write, value: &T) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() >= MAX_MESSAGE_BYTES {
        return Err(io::Error::other("protocol message exceeds 1 MiB"));
    }
    bytes.push(b'\n');
    writer.write_all(&bytes).map_err(io::Error::other)
}
/// Copies raw bytes with an 8 KiB buffer, flushing each batch before reading again.
/// EOF also flushes the writer. Interrupted reads retry; other I/O errors propagate.
/// The caller owns stream deadlines and cancellation.
pub fn copy_flushed(mut reader: impl Read, mut writer: impl Write) -> io::Result<u64> {
    let mut bytes = [0u8; 8192];
    let mut copied = 0u64;
    loop {
        let count = match reader.read(&mut bytes) {
            Ok(0) => {
                writer.flush()?;
                return Ok(copied);
            }
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        writer.write_all(&bytes[..count])?;
        writer.flush()?;
        copied = copied
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("raw stream byte count overflow"))?;
    }
}
/// Reads exactly one bounded line, leaving following stream bytes untouched.
pub fn read_line<T: serde::de::DeserializeOwned>(
    reader: &mut impl std::io::BufRead,
) -> io::Result<T> {
    let mut bytes = Vec::new();
    loop {
        let buf = reader.fill_buf().map_err(io::Error::other)?;
        if buf.is_empty() {
            return Err(io::Error::other("protocol header ended before newline"));
        }
        let count = buf
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| n + 1)
            .unwrap_or(buf.len());
        if bytes.len() + count > MAX_MESSAGE_BYTES {
            return Err(io::Error::other("protocol header exceeds 1 MiB"));
        }
        bytes.extend_from_slice(&buf[..count]);
        reader.consume(count);
        if bytes.last() == Some(&b'\n') {
            return serde_json::from_slice(&bytes).map_err(io::Error::other);
        }
    }
}
/// The framework command is supplied so tests own its executable and environment.
pub fn exchange<I: serde::Serialize, O: serde::de::DeserializeOwned>(
    command: Command,
    input: &I,
    timeout: Duration,
) -> io::Result<O> {
    let deadline = Instant::now() + timeout;
    exchange_until(command, input, || Instant::now() >= deadline)
}
fn exchange_until<I: serde::Serialize, O: serde::de::DeserializeOwned>(
    mut command: Command,
    input: &I,
    expired: impl Fn() -> bool,
) -> io::Result<O> {
    let mut bytes = serde_json::to_vec(input).map_err(io::Error::other)?;
    if bytes.len() >= MAX_MESSAGE_BYTES {
        return Err(io::Error::other("request exceeds 1 MiB"));
    }
    bytes.push(b'\n');
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = OwnedChild::spawn(&mut command)?;
    let stdin = child
        .child()
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("missing child stdin"))?;
    let mut stdout = child
        .child()
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing child stdout"))?;
    let result = (|| {
        nonblocking(&stdin)?;
        nonblocking(&stdout)?;
        let mut stdin = Some(stdin);
        let mut written = 0;
        let mut bytes_out = Vec::new();
        let mut eof = false;
        loop {
            if let Some(input) = stdin.as_mut() {
                match input.write(&bytes[written..]) {
                    Ok(0) => return Err(io::Error::other("request pipe closed early")),
                    Ok(count) => written += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
                if written == bytes.len() {
                    stdin = None;
                }
            }
            let mut buffer = [0u8; 8192];
            match stdout.read(&mut buffer) {
                Ok(0) => eof = true,
                Ok(count) => bytes_out.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            if bytes_out.len() > MAX_MESSAGE_BYTES {
                return Err(io::Error::other("reply exceeds 1 MiB"));
            }
            if eof && child.exited()? {
                return Ok(bytes_out);
            }
            if expired() {
                return Err(io::Error::other("subprocess deadline expired"));
            }
            std::thread::park_timeout(Duration::from_millis(2));
        }
    })();
    let stopped = child.finish();
    let bytes = result?;
    let status = stopped?;
    if !status.success() {
        return Err(io::Error::other(format!("subprocess exited with {status}")));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
fn nonblocking(file: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    // SAFETY: both calls operate on the live owned pipe, preserving existing flags.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
