//! A call being made ([`Request`]): its credential, headers and body, sent
//! and its answer read whole, capped and closed, or as a stream ([`Streamed`]).

use super::*;

/// A call being made: its credential, headers and body said, then sent
/// and its answer read ([`Request::answer`], [`Request::bytes`],
/// [`Request::text`], [`Request::send`]).
#[must_use = "a request does nothing until it's sent"]
pub struct Request<'a> {
    pub(super) http: &'a reqwest::Client,
    pub(super) rules: Rules,
    pub(super) method: Method,
    pub(super) url: Target<'a>,
    pub(super) route: Route,
    pub(super) credential: Option<(Scheme, &'a Secret)>,
    pub(super) headers: Vec<(String, Header<'a>)>,
    pub(super) query: Option<Result<String, String>>,
    pub(super) body: Option<Result<Body, String>>,
    pub(super) limit: u64,
    pub(super) timeout: Option<Duration>,
}

/// Where a call goes: a URL said in the clear, or one that holds a secret.
pub(super) enum Target<'a> {
    Plain(String),
    Secret(&'a Secret),
}

impl Target<'_> {
    pub(super) fn as_str(&self) -> &str {
        match self {
            Target::Plain(url) => url,
            Target::Secret(url) => url.expose(),
        }
    }
}

/// How a call's credential is sent.
pub(super) enum Scheme {
    Bearer,
    Basic,
}

/// A header's value: said in the clear, or a secret, sent marked
/// sensitive.
pub(super) enum Header<'a> {
    Plain(String),
    Secret(&'a Secret),
}

/// What a call sends: its type, and the bytes, whole or as they come.
pub(super) enum Body {
    Whole(&'static str, Vec<u8>),
    Streamed(&'static str, Pieces),
}

/// A body's pieces, as they come.
type Pieces = std::pin::Pin<Box<dyn futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send>>;

/// How a body sent as it comes goes: when a piece of it last went, and
/// whether it all has.
struct Progress {
    last: std::sync::Mutex<tokio::time::Instant>,
    done: std::sync::atomic::AtomicBool,
}

impl<'a> Request<'a> {
    /// Sent with `credential` as `Authorization: Bearer`.
    pub fn bearer(mut self, credential: &'a Secret) -> Self {
        self.credential = Some((Scheme::Bearer, credential));
        self
    }

    /// Sent with `credential`, `user:password` (or a user alone), as
    /// `Authorization: Basic`.
    pub fn basic(mut self, credential: &'a Secret) -> Self {
        self.credential = Some((Scheme::Basic, credential));
        self
    }

    /// Sent with the header `name`: `value`, which is no secret (a
    /// version, an owner's id).
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_string(), Header::Plain(value.to_string())));
        self
    }

    /// Sent with the header `name` holding `secret` (an API's key, a
    /// signature), marked sensitive, so it's never printed.
    pub fn secret_header(mut self, name: &str, secret: &'a Secret) -> Self {
        self.headers
            .push((name.to_string(), Header::Secret(secret)));
        self
    }

    /// Sent with `query` as its query string.
    pub fn query(mut self, query: &impl Serialize) -> Self {
        self.query = Some(serde_urlencoded::to_string(query).map_err(|e| e.to_string()));
        self
    }

    /// Sent with `body` as JSON.
    pub fn json(mut self, body: &impl Serialize) -> Self {
        // What doesn't serialize is said when it's sent; its error names
        // the type's field, never a value.
        self.body = Some(
            serde_json::to_vec(body)
                .map(|bytes| Body::Whole("application/json", bytes))
                .map_err(|e| e.to_string()),
        );
        self
    }

    /// Sent with `body` as a form (`application/x-www-form-urlencoded`).
    pub fn form(mut self, body: &impl Serialize) -> Self {
        self.body = Some(
            serde_urlencoded::to_string(body)
                .map(|form| Body::Whole("application/x-www-form-urlencoded", form.into_bytes()))
                .map_err(|e| e.to_string()),
        );
        self
    }

    /// Sent with `bytes` as its body, of `content_type`.
    pub fn bytes_body(mut self, content_type: &'static str, bytes: Vec<u8>) -> Self {
        self.body = Some(Ok(Body::Whole(content_type, bytes)));
        self
    }

    /// Sent with what `body` gives as it comes, of `content_type`.
    pub(super) fn streamed_body(mut self, content_type: &'static str, body: Pieces) -> Self {
        self.body = Some(Ok(Body::Streamed(content_type, body)));
        self
    }

    /// This call stopped past `timeout`, not its client's: one within a
    /// budget of its caller's.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Its answer read up to `bytes`, not [`MAX_ANSWER`]: for an answer
    /// that may be long (a share's list of files, a release's archive).
    pub fn limit(mut self, bytes: u64) -> Self {
        self.limit = bytes;
        self
    }

    /// Sent, and its answer read as `T`: closed for a service of Link's,
    /// open for one [`Builder::outside`] it. A success with no body (`204
    /// No Content`) is read as JSON's `null`: `()`, or `None` of an
    /// `Option<T>`. Every other way it can go is an [`Error`].
    pub async fn answer<T: DeserializeOwned>(self) -> Result<T, Error> {
        Ok(self.answer_with_status().await?.1)
    }

    /// Sent, and the part of its answer `T` names read, the rest passed
    /// over: for a caller that reads some of a route's fields (a balance's
    /// level, of all a console shows), saying so where it calls. A value of
    /// the wrong type is still refused.
    pub async fn answer_part<T: DeserializeOwned>(mut self) -> Result<T, Error> {
        self.rules.outside = true;
        self.answer().await
    }

    /// [`Request::answer`], and the success's status (`200`, `202`): for
    /// a service whose successes differ (done, or still working).
    pub async fn answer_with_status<T: DeserializeOwned>(self) -> Result<(u16, T), Error> {
        let (route, limit, outside) = (self.route.clone(), self.limit, self.rules.outside);
        let stall = self.rules.stall;
        let response = self.call(Some("application/json")).await?;
        let status = response.status().as_u16();
        // Held as a secret, and so wiped once read: an answer may hold one.
        let body = Secret::new(read(&route, response, limit, stall).await?);
        let answer = match outside {
            true => parse_open(&route, body.expose())?,
            false => parse(&route, body.expose())?,
        };
        Ok((status, answer))
    }

    /// Sent, and its answer's bytes, up to its limit.
    pub async fn bytes(self) -> Result<Vec<u8>, Error> {
        let (route, limit, stall) = (self.route.clone(), self.limit, self.rules.stall);
        let response = self.call(None).await?;
        read(&route, response, limit, stall).await
    }

    /// Sent, and its answer as text (UTF-8), up to its limit.
    pub async fn text(self) -> Result<String, Error> {
        let route = self.route.clone();
        String::from_utf8(self.bytes().await?).map_err(|_| Error::Unexpected {
            route,
            why: "not UTF-8 text".into(),
        })
    }

    /// Sent, its answer a success; what it holds isn't read.
    pub async fn send(self) -> Result<(), Error> {
        self.call(None).await.map(drop)
    }

    /// Sent, and its answer as it comes ([`Streamed`]), up to its limit.
    pub async fn stream(self) -> Result<Streamed, Error> {
        let (route, limit, stall) = (self.route.clone(), self.limit, self.rules.stall);
        let response = self.call(None).await?;
        if response
            .content_length()
            .is_some_and(|length| length > limit)
        {
            return Err(Error::TooLarge { route, limit });
        }
        Ok(Streamed {
            response,
            route,
            limit,
            read: 0,
            stall,
        })
    }

    async fn call(self, accept: Option<&'static str>) -> Result<reqwest::Response, Error> {
        let route = self.route;
        let unsendable = |why: String| Error::Unsendable {
            route: route.clone(),
            why,
        };
        let url = self.url.as_str();
        let allowed = match self.rules.public {
            true => is_public_url(url),
            false => may_send(url, self.rules.plain),
        } && (!self.rules.https || url.starts_with("https://"));
        if !allowed {
            return Err(Error::Insecure {
                route,
                only: self.rules.only(),
            });
        }
        let carries = self.credential.is_some()
            || self
                .headers
                .iter()
                .any(|(_, value)| matches!(value, Header::Secret(_)));
        if carries && self.rules.redirects > 0 {
            return Err(unsendable(
                "a call that follows redirects carries no credential".into(),
            ));
        }
        // To the metadata server, in the clear on the machine's own link:
        // nothing of a secret, in its URL either.
        let in_the_clear = url.starts_with("http://") && !is_on_this_machine(url);
        let secret = carries || matches!(self.url, Target::Secret(_));
        if self.rules.plain == Plain::Metadata && in_the_clear && secret {
            return Err(unsendable(
                "a call to the metadata server carries no credential".into(),
            ));
        }
        // Whole, a secret in case it holds one, its query joined.
        let url = match self.url {
            Target::Plain(url) => Secret::new(url),
            Target::Secret(url) => url.duplicate(),
        };
        let url = match self.query {
            Some(query) => {
                let query = query.map_err(unsendable)?;
                url.expose_map(|url| {
                    let join = if url.contains('?') { '&' } else { '?' };
                    format!("{url}{join}{query}")
                })
            }
            None => url,
        };
        let mut request = self.http.request(self.method, url.expose());
        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }
        if let Some(accept) = accept {
            request = request.header(ACCEPT, accept);
        }
        // Both headers marked sensitive: never printed.
        request = match self.credential {
            Some((Scheme::Bearer, secret)) => {
                let value = secret.expose_map(|token| format!("Bearer {token}"));
                let mut value = HeaderValue::from_str(value.expose())
                    .map_err(|_| unsendable("its credential can't be a header's".into()))?;
                value.set_sensitive(true);
                request.header(reqwest::header::AUTHORIZATION, value)
            }
            Some((Scheme::Basic, secret)) => match secret.expose().split_once(':') {
                Some((user, password)) => request.basic_auth(user, Some(password)),
                None => request.basic_auth(secret.expose(), None::<&str>),
            },
            None => request,
        };
        for (name, value) in self.headers {
            let value = match value {
                Header::Plain(value) => HeaderValue::from_str(&value),
                Header::Secret(secret) => {
                    HeaderValue::from_str(secret.expose()).map(|mut value| {
                        value.set_sensitive(true);
                        value
                    })
                }
            }
            .map_err(|_| unsendable(format!("its {} header isn't one", printable(&name))))?;
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| unsendable(format!("{} isn't a header's name", printable(&name))))?;
            request = request.header(name, value);
        }
        let stall = self.rules.stall;
        let mut progress = None;
        request = match self.body.transpose().map_err(unsendable)? {
            Some(Body::Whole(kind, bytes)) => request.header(CONTENT_TYPE, kind).body(bytes),
            Some(Body::Streamed(kind, pieces)) => {
                let pieces = match stall {
                    Some(_) => {
                        let watched = Arc::new(Progress::new());
                        progress = Some(watched.clone());
                        watch(pieces, watched)
                    }
                    None => pieces,
                };
                request
                    .header(CONTENT_TYPE, kind)
                    .body(reqwest::Body::wrap_stream(pieces))
            }
            None => request,
        };
        let sending = request.send();
        let sent = match stall {
            Some(stall) => match until_stalled(sending, stall, progress.as_deref()).await {
                Some(sent) => sent,
                None => return Err(stalled(&route, stall)),
            },
            None => sending.await,
        };
        let response = sent.map_err(|e| Error::Unreachable {
            route: route.clone(),
            sent: !e.is_connect(),
            why: why_unreached(e),
        })?;
        let status = response.status().as_u16();
        match status {
            200..=299 => Ok(response),
            300..=399 => Err(Error::Redirected { route, status }),
            _ => {
                let after = retry_after(&response);
                let body = read_some(response, MAX_REFUSAL, stall).await;
                let refusal = Refusal {
                    route,
                    status,
                    body,
                    after,
                };
                Err(match status {
                    401 => Error::SignedOut(refusal),
                    429 => Error::Later(refusal),
                    500.. => Error::Failed(refusal),
                    _ => Error::Refused(refusal),
                })
            }
        }
    }
}

/// An answer read as it comes ([`Request::stream`]), never past its
/// limit.
pub struct Streamed {
    response: reqwest::Response,
    route: Route,
    limit: u64,
    read: u64,
    stall: Option<Duration>,
}

/// Its route, never its URL, which may hold a secret
/// ([`Client::secret_url`]).
impl fmt::Debug for Streamed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Streamed")
            .field("route", &self.route)
            .field("limit", &self.limit)
            .field("read", &self.read)
            .finish_non_exhaustive()
    }
}

impl Streamed {
    /// Its next bytes, none once it's all been read.
    pub async fn chunk(&mut self) -> Result<Option<bytes::Bytes>, Error> {
        let chunk = next_chunk(&self.route, &mut self.response, self.stall).await?;
        if let Some(chunk) = &chunk {
            self.read += chunk.len() as u64;
            if self.read > self.limit {
                return Err(Error::TooLarge {
                    route: self.route.clone(),
                    limit: self.limit,
                });
            }
        }
        Ok(chunk)
    }

    /// How long it says it is, when it says.
    pub fn length(&self) -> Option<u64> {
        self.response.content_length()
    }

    /// Its header `name`, when it has one in text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.response.headers().get(name)?.to_str().ok()
    }
}

impl Progress {
    fn new() -> Self {
        Self {
            last: std::sync::Mutex::new(tokio::time::Instant::now()),
            done: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Since when nothing of the body has gone; none once it all has.
    fn since(&self) -> Option<tokio::time::Instant> {
        match self.done.load(std::sync::atomic::Ordering::SeqCst) {
            true => None,
            false => Some(*self.last.lock().unwrap_or_else(|e| e.into_inner())),
        }
    }
}

/// `pieces`, each one taken telling `progress`, and its end.
fn watch(mut pieces: Pieces, progress: Arc<Progress>) -> Pieces {
    use std::task::Poll;
    Box::pin(futures_util::stream::poll_fn(move |cx| {
        let next = pieces.as_mut().poll_next(cx);
        match &next {
            Poll::Ready(Some(_)) => {
                *progress.last.lock().unwrap_or_else(|e| e.into_inner()) =
                    tokio::time::Instant::now();
            }
            Poll::Ready(None) => progress
                .done
                .store(true, std::sync::atomic::Ordering::SeqCst),
            Poll::Pending => {}
        }
        next
    }))
}

/// `sending`'s outcome, or none once nothing moved for `stall`: while a
/// body goes, none of it went for that long ([`Progress`]); with none to
/// send, no answer started within it. Once a body has all gone, its
/// answer is waited for as long as it takes, the service at work on it.
async fn until_stalled<F: std::future::Future>(
    sending: F,
    stall: Duration,
    progress: Option<&Progress>,
) -> Option<F::Output> {
    tokio::pin!(sending);
    let start = tokio::time::Instant::now();
    loop {
        let since = match progress {
            Some(progress) => progress.since(),
            None => Some(start),
        };
        let Some(since) = since else {
            return Some(sending.await);
        };
        if since.elapsed() >= stall {
            return None;
        }
        tokio::select! {
            sent = &mut sending => return Some(sent),
            () = tokio::time::sleep_until(since + stall) => {}
        }
    }
}

/// A call given up for nothing moving for `stall`.
fn stalled(route: &Route, stall: Duration) -> Error {
    Error::Unreachable {
        route: route.clone(),
        why: format!("nothing moved in {}s", stall.as_secs()),
        sent: true,
    }
}

/// An answer's next bytes, given up past `stall` when there's one.
async fn next_chunk(
    route: &Route,
    response: &mut reqwest::Response,
    stall: Option<Duration>,
) -> Result<Option<bytes::Bytes>, Error> {
    let chunk = match stall {
        Some(stall) => tokio::time::timeout(stall, response.chunk())
            .await
            .map_err(|_| stalled(route, stall))?,
        None => response.chunk().await,
    };
    chunk.map_err(|e| Error::Unreachable {
        route: route.clone(),
        why: why_unreached(e),
        sent: true,
    })
}

/// Why a call wasn't answered, without its URL (the route says it) or
/// anything the URL held.
fn why_unreached(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "no answer in time".into()
    } else {
        let e = e.without_url();
        let mut why = e.to_string();
        let mut source = std::error::Error::source(&e);
        while let Some(cause) = source {
            why = format!("{why}: {cause}");
            source = cause.source();
        }
        printable(&why)
    }
}

/// How long an answer asks to wait before another try, when it says in
/// seconds (`Retry-After`).
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let secs = response.headers().get(RETRY_AFTER)?.to_str().ok()?;
    secs.trim().parse().ok().map(Duration::from_secs)
}

/// The whole of a success's body, at most `limit` bytes: more is an error,
/// found before it's read whole.
async fn read(
    route: &Route,
    mut response: reqwest::Response,
    limit: u64,
    stall: Option<Duration>,
) -> Result<Vec<u8>, Error> {
    let too_large = || Error::TooLarge {
        route: route.clone(),
        limit,
    };
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(route, &mut response, stall).await? {
        if (body.len() + chunk.len()) as u64 > limit {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// At most `limit` bytes of a refusal's body, whatever else it held, as
/// much as came before it stalled.
async fn read_some(
    mut response: reqwest::Response,
    limit: usize,
    stall: Option<Duration>,
) -> Vec<u8> {
    let mut body = Vec::new();
    let route = Route(String::new());
    while body.len() < limit {
        match next_chunk(&route, &mut response, stall).await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            _ => break,
        }
    }
    body.truncate(limit);
    body
}

/// `body` as `T`, closed: a field `T` doesn't name is refused. An empty
/// body is JSON's `null`.
pub(super) fn parse<T: DeserializeOwned>(route: &Route, body: &[u8]) -> Result<T, Error> {
    let unexpected = |why: String| Error::Unexpected {
        route: route.clone(),
        why,
    };
    let body: &[u8] = if body.iter().all(u8::is_ascii_whitespace) {
        b"null"
    } else {
        body
    };
    let mut unknown = None;
    let mut json = serde_json::Deserializer::from_slice(body);
    let value = serde_ignored::deserialize(&mut json, |path| {
        unknown.get_or_insert_with(|| path.to_string());
    })
    .and_then(|value| json.end().map(|()| value))
    .map_err(|e| unexpected(where_wrong(&e)))?;
    match unknown {
        Some(path) => Err(unexpected(format!(
            "a field its type doesn't name: {}",
            printable(cut(&path, MAX_PATH))
        ))),
        None => Ok(value),
    }
}

/// `body` as `T`, open: a field `T` doesn't name is passed over, for a
/// service that isn't Link's ([`Builder::outside`]). An empty body is
/// JSON's `null`.
fn parse_open<T: DeserializeOwned>(route: &Route, body: &[u8]) -> Result<T, Error> {
    let body: &[u8] = if body.iter().all(u8::is_ascii_whitespace) {
        b"null"
    } else {
        body
    };
    serde_json::from_slice(body).map_err(|e| Error::Unexpected {
        route: route.clone(),
        why: where_wrong(&e),
    })
}

/// What's wrong with an answer and where, never the value it held:
/// serde's own message quotes it, and it may be a secret.
fn where_wrong(e: &serde_json::Error) -> String {
    use serde_json::error::Category;
    let what = match e.classify() {
        Category::Io | Category::Syntax => "not JSON",
        Category::Data => "not of its type",
        Category::Eof => "cut short",
    };
    format!("{what}, at line {} column {}", e.line(), e.column())
}

/// At most `max` bytes of `text`, cut where a character ends.
fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_unknown_field_is_cut() {
        let route = Route::new(&Method::GET, "https://x.example/");
        let field = "é".repeat(500);
        let body = format!(r#"{{"{field}": 1}}"#);
        #[derive(serde::Deserialize, Debug)]
        struct Empty {}
        let said = parse::<Empty>(&route, body.as_bytes())
            .unwrap_err()
            .to_string();
        let path = said.rsplit(": ").next().unwrap();
        assert!(path.len() <= MAX_PATH, "{}", path.len());
        assert!(path.starts_with('é'));
    }

    /// A type closed or not: a field it doesn't name is refused, at any
    /// depth.
    #[test]
    fn every_answer_is_closed() {
        #[derive(serde::Deserialize, Debug)]
        #[expect(
            dead_code,
            reason = "only deserialized, to show an unknown field is refused"
        )]
        struct Machine {
            id: String,
        }
        #[derive(serde::Deserialize, Debug)]
        #[expect(
            dead_code,
            reason = "only deserialized, to show an unknown field is refused"
        )]
        struct Account {
            username: String,
            machine: Option<Machine>,
        }
        let route = Route::new(&Method::GET, "https://x.example/");
        assert!(parse::<Account>(&route, br#"{"username":"ada"}"#).is_ok());
        let err = parse::<Account>(&route, br#"{"username":"ada","admin":1}"#).unwrap_err();
        assert!(err.to_string().contains("admin"), "{err}");
        let err = parse::<Account>(&route, br#"{"username":"ada","machine":{"id":"m","x":1}}"#)
            .unwrap_err();
        assert!(err.to_string().ends_with(".x"), "{err}");
    }

    #[test]
    fn a_wrong_value_isnt_quoted() {
        #[derive(serde::Deserialize, Debug)]
        struct Answer {
            #[expect(
                dead_code,
                reason = "only deserialized, to show a wrong value isn't quoted"
            )]
            count: u64,
        }
        let route = Route::new(&Method::GET, "https://x.example/");
        let err = parse::<Answer>(&route, br#"{"count": "s3cret"}"#).unwrap_err();
        assert!(!err.to_string().contains("s3cret"), "{err}");
        assert!(err.to_string().contains("GET https://x.example/"), "{err}");
    }
}
