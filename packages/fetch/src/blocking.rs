//! The same client, for a program that doesn't run on a runtime of its
//! own (a command, a sync service): every call made as [`crate::Client`]
//! makes it, on a runtime this client keeps, and waited for. The same
//! rules hold, the same errors come back. Like reqwest's own, it can't be
//! called from a task on another runtime.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use link_secretive::Secret;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Error;

/// A [`crate::Client`] whose calls are waited for. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Client {
    inner: crate::Client,
    runtime: Arc<Runtime>,
}

/// The runtime a client's calls run on, shut down without waiting when
/// the last client goes, wherever that is.
#[derive(Debug)]
struct Runtime(Option<tokio::runtime::Runtime>);

impl Runtime {
    fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        match &self.0 {
            Some(runtime) => runtime.block_on(future),
            None => unreachable!("a client's runtime lasts as long as it does"),
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

impl Client {
    /// A client by Link's rules ([`crate::Client::new`]).
    pub fn new() -> Result<Self, Error> {
        Self::builder().build()
    }

    /// A client by Link's rules, its calls stopped past `timeout`.
    pub fn with_timeout(timeout: Duration) -> Result<Self, Error> {
        Self::builder().timeout(timeout).build()
    }

    /// A client by Link's rules but for what its caller names
    /// ([`crate::Builder`]).
    pub fn builder() -> Builder {
        Builder(crate::Builder::default())
    }

    /// A `GET` of `url`.
    pub fn get(&self, url: &str) -> Request<'_> {
        self.wrap(self.inner.get(url))
    }

    /// A `POST` to `url`.
    pub fn post(&self, url: &str) -> Request<'_> {
        self.wrap(self.inner.post(url))
    }

    /// A `PUT` to `url`.
    pub fn put(&self, url: &str) -> Request<'_> {
        self.wrap(self.inner.put(url))
    }

    /// A `PATCH` of `url`.
    pub fn patch(&self, url: &str) -> Request<'_> {
        self.wrap(self.inner.patch(url))
    }

    /// A `DELETE` of `url`.
    pub fn delete(&self, url: &str) -> Request<'_> {
        self.wrap(self.inner.delete(url))
    }

    /// A call of `method` to `url`.
    pub fn request(&self, method: crate::Method, url: &str) -> Request<'_> {
        self.wrap(self.inner.request(method, url))
    }

    /// [`crate::Client::secret_url`].
    pub fn secret_url<'a>(
        &'a self,
        method: crate::Method,
        url: &'a Secret,
        shown: &str,
    ) -> Request<'a> {
        self.wrap(self.inner.secret_url(method, url, shown))
    }

    fn wrap<'a>(&'a self, inner: crate::Request<'a>) -> Request<'a> {
        Request {
            inner,
            runtime: &self.runtime,
        }
    }
}

/// [`crate::Builder`], for a blocking client.
#[derive(Debug)]
#[must_use = "a builder does nothing until it's built"]
pub struct Builder(crate::Builder);

impl Builder {
    /// [`crate::Builder::timeout`].
    pub fn timeout(self, timeout: Duration) -> Self {
        Self(self.0.timeout(timeout))
    }

    /// [`crate::Builder::streaming`].
    pub fn streaming(self, stall: Duration) -> Self {
        Self(self.0.streaming(stall))
    }

    /// [`crate::Builder::connect_timeout`].
    pub fn connect_timeout(self, connect: Duration) -> Self {
        Self(self.0.connect_timeout(connect))
    }

    /// [`crate::Builder::outside`].
    pub fn outside(self) -> Self {
        Self(self.0.outside())
    }

    /// [`crate::Builder::trusting`].
    pub fn trusting(self, pem: &[u8]) -> Self {
        Self(self.0.trusting(pem))
    }

    /// [`crate::Builder::direct`].
    pub fn direct(self) -> Self {
        Self(self.0.direct())
    }

    /// [`crate::Builder::https_only`].
    pub fn https_only(self) -> Self {
        Self(self.0.https_only())
    }

    /// [`crate::Builder::plain_http`].
    pub fn plain_http(self) -> Self {
        Self(self.0.plain_http())
    }

    /// [`crate::Builder::metadata_server`].
    pub fn metadata_server(self) -> Self {
        Self(self.0.metadata_server())
    }

    /// [`crate::Builder::follow_redirects`].
    pub fn follow_redirects(self, hops: usize) -> Self {
        Self(self.0.follow_redirects(hops))
    }

    /// The client, and the runtime its calls run on: one thread.
    pub fn build(self) -> Result<Client, Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("link-fetch")
            .enable_all()
            .build()
            .map_err(|e| Error::Setup(format!("its runtime: {e}")))?;
        Ok(Client {
            inner: self.0.build()?,
            runtime: Arc::new(Runtime(Some(runtime))),
        })
    }
}

/// [`crate::Request`], waited for.
#[must_use = "a request does nothing until it's sent"]
pub struct Request<'a> {
    inner: crate::Request<'a>,
    runtime: &'a Runtime,
}

impl<'a> Request<'a> {
    fn map(self, f: impl FnOnce(crate::Request<'a>) -> crate::Request<'a>) -> Self {
        Self {
            inner: f(self.inner),
            runtime: self.runtime,
        }
    }

    /// [`crate::Request::bearer`].
    pub fn bearer(self, credential: &'a Secret) -> Self {
        self.map(|r| r.bearer(credential))
    }

    /// [`crate::Request::basic`].
    pub fn basic(self, credential: &'a Secret) -> Self {
        self.map(|r| r.basic(credential))
    }

    /// [`crate::Request::header`].
    pub fn header(self, name: &str, value: &str) -> Self {
        self.map(|r| r.header(name, value))
    }

    /// [`crate::Request::secret_header`].
    pub fn secret_header(self, name: &str, secret: &'a Secret) -> Self {
        self.map(|r| r.secret_header(name, secret))
    }

    /// [`crate::Request::query`].
    pub fn query(self, query: &impl Serialize) -> Self {
        self.map(|r| r.query(query))
    }

    /// [`crate::Request::json`].
    pub fn json(self, body: &impl Serialize) -> Self {
        self.map(|r| r.json(body))
    }

    /// [`crate::Request::form`].
    pub fn form(self, body: &impl Serialize) -> Self {
        self.map(|r| r.form(body))
    }

    /// [`crate::Request::bytes_body`].
    pub fn bytes_body(self, content_type: &'static str, bytes: Vec<u8>) -> Self {
        self.map(|r| r.bytes_body(content_type, bytes))
    }

    /// Sent with what `reader` gives as it comes, of `content_type`: read
    /// on a thread of its own, a piece at a time. On a client
    /// [`crate::Builder::streaming`], given up when none of it goes for
    /// that long, however long it all takes.
    pub fn reader_body(
        self,
        content_type: &'static str,
        reader: impl Read + Send + 'static,
    ) -> Self {
        let (pieces, taken) = tokio::sync::mpsc::channel::<std::io::Result<Vec<u8>>>(4);
        std::thread::spawn(move || {
            let mut reader = reader;
            loop {
                let mut piece = vec![0; 64 << 10];
                let read = match reader.read(&mut piece) {
                    Ok(0) => return,
                    Ok(n) => {
                        piece.truncate(n);
                        Ok(piece)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => Err(e),
                };
                let failed = read.is_err();
                if pieces.blocking_send(read).is_err() || failed {
                    return;
                }
            }
        });
        let stream = futures_util::stream::unfold(taken, |mut taken| async move {
            taken.recv().await.map(|piece| (piece, taken))
        });
        self.map(|r| r.streamed_body(content_type, Box::pin(stream)))
    }

    /// [`crate::Request::timeout`].
    pub fn timeout(self, timeout: Duration) -> Self {
        self.map(|r| r.timeout(timeout))
    }

    /// [`crate::Request::limit`].
    pub fn limit(self, bytes: u64) -> Self {
        self.map(|r| r.limit(bytes))
    }

    /// [`crate::Request::answer`], waited for.
    pub fn answer<T: DeserializeOwned>(self) -> Result<T, Error> {
        self.runtime.block_on(self.inner.answer())
    }

    /// [`crate::Request::answer_part`], waited for.
    pub fn answer_part<T: DeserializeOwned>(self) -> Result<T, Error> {
        self.runtime.block_on(self.inner.answer_part())
    }

    /// [`crate::Request::answer_with_status`], waited for.
    pub fn answer_with_status<T: DeserializeOwned>(self) -> Result<(u16, T), Error> {
        self.runtime.block_on(self.inner.answer_with_status())
    }

    /// [`crate::Request::bytes`], waited for.
    pub fn bytes(self) -> Result<Vec<u8>, Error> {
        self.runtime.block_on(self.inner.bytes())
    }

    /// [`crate::Request::text`], waited for.
    pub fn text(self) -> Result<String, Error> {
        self.runtime.block_on(self.inner.text())
    }

    /// [`crate::Request::send`], waited for.
    pub fn send(self) -> Result<(), Error> {
        self.runtime.block_on(self.inner.send())
    }

    /// [`crate::Request::stream`], read as a [`Read`].
    pub fn stream(self) -> Result<Streamed<'a>, Error> {
        let streamed = self.runtime.block_on(self.inner.stream())?;
        Ok(Streamed {
            streamed,
            runtime: self.runtime,
            piece: bytes::Bytes::new(),
        })
    }
}

/// An answer read as it comes, as a [`Read`], never past its limit.
pub struct Streamed<'a> {
    streamed: crate::Streamed,
    runtime: &'a Runtime,
    piece: bytes::Bytes,
}

impl Streamed<'_> {
    /// How long it says it is, when it says.
    pub fn length(&self) -> Option<u64> {
        self.streamed.length()
    }

    /// Its header `name`, when it has one in text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.streamed.header(name)
    }
}

impl Read for Streamed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.piece.is_empty() {
            match self.runtime.block_on(self.streamed.chunk()) {
                Ok(Some(piece)) => self.piece = piece,
                Ok(None) => return Ok(0),
                Err(e) => return Err(std::io::Error::other(e)),
            }
        }
        let n = buf.len().min(self.piece.len());
        buf[..n].copy_from_slice(&self.piece.split_to(n));
        Ok(n)
    }
}
