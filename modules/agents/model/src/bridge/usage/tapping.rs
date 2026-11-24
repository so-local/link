//! What one call's answers counted, and the connection that counts them
//! as they pass.

use super::*;

/// What one call's model answers counted, for the measure module
/// ([`link_measure_contract::client::tokens`]), and what it asked, for
/// its record ([`crate::bridge::context`]).
#[derive(Debug, Default)]
pub struct Counter {
    counts: [AtomicU64; 6],
    asked: std::sync::Mutex<Option<crate::bridge::context::Asked>>,
}

impl Counter {
    pub const fn new() -> Self {
        Self {
            counts: [const { AtomicU64::new(0) }; 6],
            asked: std::sync::Mutex::new(None),
        }
    }

    /// What the call asked, taken from its request.
    pub fn asked(&self, asked: crate::bridge::context::Asked) {
        *self.asked.lock().unwrap_or_else(|e| e.into_inner()) = Some(asked);
    }

    /// What the call asked, once.
    pub fn take_asked(&self) -> Option<crate::bridge::context::Asked> {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    pub fn add(&self, t: Tokens) {
        let counts = [
            t.requests,
            t.unmetered,
            t.input,
            t.output,
            t.cached,
            t.written,
        ];
        for (slot, n) in self.counts.iter().zip(counts) {
            slot.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub fn now(&self) -> Tokens {
        let [requests, unmetered, input, output, cached, written] =
            std::array::from_fn(|i| self.counts[i].load(Ordering::Relaxed));
        Tokens {
            requests,
            unmetered,
            input,
            output,
            cached,
            written,
        }
    }
}

/// A connection to the program whose answer is metered: each byte
/// written to it is fed to its [`Meter`], and what it counts is added to
/// its counter once the connection is dropped.
pub struct Tapped<'a, S> {
    inner: S,
    meter: Option<Meter>,
    counter: &'a Counter,
}

impl<'a, S> Tapped<'a, S> {
    /// `inner`, metered if `format` is some.
    pub fn new(inner: S, format: Option<Format>, counter: &'a Counter) -> Self {
        Self {
            inner,
            meter: format.map(Meter::new),
            counter,
        }
    }
}

impl<S> Drop for Tapped<'_, S> {
    fn drop(&mut self) {
        if let Some(meter) = self.meter.take() {
            self.counter.add(meter.finish());
        }
    }
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Tapped<'_, S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Tapped<'_, S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let poll = std::pin::Pin::new(&mut self.inner).poll_write(cx, buf);
        if let std::task::Poll::Ready(Ok(n)) = &poll
            && let Some(meter) = &mut self.meter
        {
            meter.feed(&buf[..*n]);
        }
        poll
    }

    // What `inner` writes in one go, it's passed in one go; the meter is
    // fed the bytes taken, in order.
    fn poll_write_vectored(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let poll = std::pin::Pin::new(&mut self.inner).poll_write_vectored(cx, bufs);
        if let std::task::Poll::Ready(Ok(n)) = &poll
            && let Some(meter) = &mut self.meter
        {
            let mut left = *n;
            for buf in bufs {
                if left == 0 {
                    break;
                }
                let taken = left.min(buf.len());
                meter.feed(&buf[..taken]);
                left -= taken;
            }
        }
        poll
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{PHRASE, events, streamed, tokens};
    use super::*;
    use serde_json::json;

    /// Through a connection: each piece written as it comes, the count
    /// added once it's dropped, and the answer passed on unchanged.
    #[tokio::test]
    async fn a_tapped_connection_counts_what_passes_and_changes_none_of_it() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let counter = Counter::new();
        let body = events(&[
            (
                "message_start",
                json!({"type": "message_start", "message": {
                "usage": {"input_tokens": 7, "output_tokens": 1}}}),
            ),
            (
                "content_block_delta",
                json!({"type": "content_block_delta",
                "delta": {"type": "text_delta", "text": PHRASE}}),
            ),
            (
                "message_delta",
                json!({"type": "message_delta", "usage": {"output_tokens": 9}}),
            ),
        ]);
        let answer = streamed("text/event-stream", &body);
        let (program, mut reading) = tokio::io::duplex(1 << 20);
        let mut tapped = Tapped::new(program, Some(Format::Anthropic), &counter);
        for piece in answer.chunks(17) {
            tapped.write_all(piece).await.unwrap();
        }
        tapped.shutdown().await.unwrap();
        assert_eq!(counter.now(), Tokens::default(), "counted before it ended");
        drop(tapped);
        let mut got = Vec::new();
        reading.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, answer);
        assert_eq!(counter.now(), tokens(7, 9, 0));
        // Nothing of what passed is kept: a count is five numbers.
        let kept = serde_json::to_string(&counter.now()).unwrap();
        assert!(!kept.contains("purple"), "{kept}");
    }

    /// Written in vectored pieces, a part of one taken at a time, the
    /// answer is counted the same.
    #[tokio::test]
    async fn a_tapped_connection_counts_vectored_writes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let counter = Counter::new();
        let body = events(&[
            (
                "message_start",
                json!({"type": "message_start", "message": {
                "usage": {"input_tokens": 4, "output_tokens": 1}}}),
            ),
            (
                "message_delta",
                json!({"type": "message_delta", "usage": {"output_tokens": 6}}),
            ),
        ]);
        let answer = streamed("text/event-stream", &body);
        let (program, mut reading) = tokio::io::duplex(1 << 20);
        let mut tapped = Tapped::new(program, Some(Format::Anthropic), &counter);
        let mut slices: Vec<std::io::IoSlice> =
            answer.chunks(17).map(std::io::IoSlice::new).collect();
        let mut slices = &mut slices[..];
        while !slices.is_empty() {
            let n = tapped.write_vectored(slices).await.unwrap();
            std::io::IoSlice::advance_slices(&mut slices, n);
        }
        tapped.shutdown().await.unwrap();
        drop(tapped);
        let mut got = Vec::new();
        reading.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, answer);
        assert_eq!(counter.now(), tokens(4, 6, 0));
    }
}
