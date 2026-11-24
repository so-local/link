//! Incremental server-sent event payload framing.

/// The `data:` payloads of a server-sent event stream, as its bytes come.
#[derive(Default)]
pub struct SseLines {
    partial: Vec<u8>,
    /// How much of `partial` is known to hold no line break.
    scanned: usize,
}

impl SseLines {
    /// The payloads of the lines `bytes` completes. Each byte is looked
    /// at once, however many pieces a long line comes in.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.partial.extend_from_slice(bytes);
        let mut data = Vec::new();
        let mut start = 0;
        let mut from = self.scanned;
        while let Some(at) = self.partial[from..].iter().position(|b| *b == b'\n') {
            let end = from + at;
            let line = String::from_utf8_lossy(&self.partial[start..end]);
            if let Some(payload) = line.trim_end().strip_prefix("data:") {
                data.push(payload.trim_start().to_string());
            }
            start = end + 1;
            from = start;
        }
        self.partial.drain(..start);
        self.scanned = self.partial.len();
        data
    }

    /// How many bytes wait for their line's end.
    pub fn waiting(&self) -> usize {
        self.partial.len()
    }
}
