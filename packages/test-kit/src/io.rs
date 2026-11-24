//! Captured output with observable writes, so a test can synchronize on
//! what the caller displayed rather than when a producer sent it.

use std::io::{self, Write};

/// An in-memory writer that publishes its total bytes written.
pub struct ObservedOutput {
    bytes: Vec<u8>,
    written: tokio::sync::watch::Sender<usize>,
}

impl Default for ObservedOutput {
    fn default() -> Self {
        let (written, _) = tokio::sync::watch::channel(0);
        Self {
            bytes: Vec::new(),
            written,
        }
    }
}

impl ObservedOutput {
    /// The current total, including writes made before subscribing.
    pub fn written(&self) -> tokio::sync::watch::Receiver<usize> {
        self.written.subscribe()
    }

    /// All captured bytes, in write order.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for ObservedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        self.written.send_replace(self.bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_fragments_publish_their_total_after_the_write() {
        let mut output = ObservedOutput::default();
        let written = output.written();
        assert_eq!(*written.borrow(), 0);
        output.write_all(b"Thinking").unwrap();
        assert_eq!(*written.borrow(), 8);
        output.flush().unwrap();
        assert_eq!(*written.borrow(), 8);
        output.write_all(b"\n").unwrap();
        assert_eq!(*written.borrow(), 9);
        assert_eq!(output.into_bytes(), b"Thinking\n");
    }

    #[test]
    fn a_late_observer_sees_output_already_written() {
        let mut output = ObservedOutput::default();
        output.write_all(b"Ready").unwrap();
        assert_eq!(*output.written().borrow(), 5);
        assert_eq!(output.into_bytes(), b"Ready");
    }
}
