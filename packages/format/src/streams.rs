//! The program's own streams, for what is never a block, each only for
//! what it's for: a [`Protocol`] another program reads (an MCP server's
//! messages, a bridge's frames, a reply), [`Passed`] output another
//! program made, passed on as it came, stderr handed to a child
//! ([`child_stderr`]), and whether one is a terminal. None is a handle
//! for words a person reads: those are blocks on an [`Out`](crate::Out),
//! or an [`aside`](crate::aside). The docs checks' guard counts any other
//! way to the streams.

use std::fs::File;
use std::io::{self, IsTerminal, Write};
use std::os::fd::AsFd;

/// Which of the program's streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// Whether stdout is a terminal.
pub fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

/// Whether stderr is a terminal.
pub fn stderr_is_terminal() -> bool {
    io::stderr().is_terminal()
}

/// A stream's handle, for this module alone.
fn handle(stream: Stream) -> Box<dyn Write + Send> {
    match stream {
        Stream::Out => Box::new(io::stdout()),
        Stream::Err => Box::new(io::stderr()),
    }
}

/// A machine-readable protocol the program speaks on one of its
/// streams, for the program that runs it: MCP's messages, a bridge's or
/// a move's frames, a JSON reply, a line a caller reads by its mark.
/// Never words for a person.
pub struct Protocol(Box<dyn Write + Send>);

/// The protocol on stdout.
pub fn protocol() -> Protocol {
    Protocol(handle(Stream::Out))
}

/// The protocol on `stream`: stderr, where stdout carries another.
pub fn protocol_on(stream: Stream) -> Protocol {
    Protocol(handle(stream))
}

impl Protocol {
    /// Stdout as a file of its own, for an async writer of the protocol.
    pub fn file() -> io::Result<File> {
        Ok(File::from(io::stdout().as_fd().try_clone_to_owned()?))
    }
}

impl Write for Protocol {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// Output another program made (a child, a remote session, a service),
/// passed on to one of this program's streams as it came, or each line
/// after its label (`[build] `): never Link's own words.
pub struct Passed {
    to: Box<dyn Write + Send>,
    label: Option<String>,
    /// Whether the next byte opens a line.
    opening: bool,
}

/// `stream`, for another program's output.
pub fn passed(stream: Stream) -> Passed {
    Passed {
        to: handle(stream),
        label: None,
        opening: true,
    }
}

impl Passed {
    /// Each line after `name`'s label, as the look shows a source: `[name] `.
    pub fn labeled(mut self, name: &str) -> Self {
        self.label = Some(format!("[{}] ", crate::clean(name)));
        self
    }
}

impl Write for Passed {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(label) = &self.label else {
            return self.to.write(bytes);
        };
        let mut shown = Vec::with_capacity(bytes.len() + label.len());
        for chunk in bytes.split_inclusive(|b| *b == b'\n') {
            if self.opening {
                shown.extend_from_slice(label.as_bytes());
            }
            shown.extend_from_slice(chunk);
            self.opening = chunk.ends_with(b"\n");
        }
        self.to.write_all(&shown)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.to.flush()
    }
}

/// The terminal this program was started from (`/dev/tty`), held open
/// for a question asked on it from any thread while a command runs
/// ([`Question::ask_on_terminal`](crate::Question::ask_on_terminal)):
/// nothing else reads or writes it.
pub struct Terminal(pub(crate) File);

impl Terminal {
    /// The terminal, or none when the program has none.
    pub fn open() -> Option<Terminal> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()
            .map(Terminal)
    }
}

/// Stderr, for a child to write to in place of its stdout: its output is
/// about the run, and this program's stdout is its answer.
pub fn child_stderr() -> io::Result<std::process::Stdio> {
    Ok(std::process::Stdio::from(
        io::stderr().as_fd().try_clone_to_owned()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_labeled_pass_puts_the_label_before_each_line() {
        let kept = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        struct To(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for To {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut passed = Passed {
            to: Box::new(To(kept.clone())),
            label: None,
            opening: true,
        }
        .labeled("build");
        passed.write_all(b"one\ntw").unwrap();
        passed.write_all(b"o\nthree").unwrap();
        assert_eq!(
            String::from_utf8(kept.lock().unwrap().clone()).unwrap(),
            "[build] one\n[build] two\n[build] three"
        );
    }
}
