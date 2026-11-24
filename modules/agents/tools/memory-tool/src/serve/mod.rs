//! An index served: its one writer ([`Writer`]), and sessions answered
//! through readers of their own ([`session`]), so a session's handshake
//! and calls never wait for indexing.

use std::io::{BufRead, Read, Write};
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::{Common, Index, Reader, mcp};

mod writer;

pub use writer::{Pool, Turn, Writer, Writes};

/// What a session is answered from: the index it reads, how it's served,
/// and whether it's still served.
pub trait Answers {
    /// The index its calls read, now: a reader of another is opened
    /// again.
    fn common(&self) -> Arc<Common>;
    /// How it's served.
    fn options(&self) -> &mcp::Options;
    /// Why it isn't served any more, once it isn't.
    fn ended(&self) -> Option<&'static str> {
        None
    }
}

/// An index served: its writer, what its calls share, and how it's served.
pub struct Served {
    writer: Writer<Index>,
    common: Arc<Common>,
    options: mcp::Options,
}

impl Served {
    /// `index` served as `options` say, its writer started: it's brought up
    /// to its folder in the background, never in a call.
    pub fn start(index: Index, options: mcp::Options) -> Self {
        Self::start_in(index, options, Pool::new())
    }

    /// [`Served::start`], its writer's turns taken from `pool`, which a
    /// service's indexes share.
    pub fn start_in(mut index: Index, options: mcp::Options, pool: Arc<Pool>) -> Self {
        let common = index.common();
        // Its folder watched, each change noted in its journal and its
        // writer woken for it.
        index.watch();
        let writer = Writer::start(index, pool);
        common.journal().wake_with(writer.waker());
        Self {
            writer,
            common,
            options,
        }
    }

    /// Its writer.
    pub fn writer(&self) -> &Writer<Index> {
        &self.writer
    }

    /// One session on `input` and `output`, until `input` ends.
    pub fn session(&self, input: impl BufRead, output: impl Write) -> Result<()> {
        session(self, input, output)
    }
}

impl Answers for Served {
    fn common(&self) -> Arc<Common> {
        self.common.clone()
    }

    fn options(&self) -> &mcp::Options {
        &self.options
    }
}

/// Answers each MCP message on `input` from what `answers` serves, on
/// `output`, until `input` ends: through a reader of its own, opened
/// before its first message, which reads nothing of the index.
pub fn session(
    answers: &dyn Answers,
    mut input: impl BufRead,
    mut output: impl Write,
) -> Result<()> {
    let most = crate::limits::limits().message_max_bytes;
    let mut reader = Reader::open(answers.common())?;
    loop {
        let mut read = Vec::new();
        (&mut input)
            .take(most + 1)
            .read_until(b'\n', &mut read)
            .context("reading a message")?;
        if read.is_empty() {
            break;
        }
        anyhow::ensure!(
            read.len() as u64 <= most,
            "a message past {most} bytes: the session is ended"
        );
        let line = String::from_utf8(read).context("a message that isn't text")?;
        let line = line.trim_end_matches(['\n', '\r']);
        if line.trim().is_empty() {
            continue;
        }
        if let Some(why) = answers.ended() {
            anyhow::bail!(why);
        }
        let common = answers.common();
        if !reader.reads(&common) {
            reader = Reader::open(common)?;
        }
        if let Some(reply) = mcp::answer_with(&reader, answers.options(), line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}
