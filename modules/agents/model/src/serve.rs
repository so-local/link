//! `lnk model serve`: a model runtime's API on a local port.
//!
//! Serves the OpenAI-compatible API of Ollama (by default at
//! `http://localhost:11434`), a llama.cpp server, LM Studio, vLLM or anything
//! else that serves `/v1/...`. Only that API is forwarded: a runtime's own
//! management endpoints (Ollama's `/api/pull`, `/api/delete`, ...) stay
//! private. Responses, including
//! streamed tokens (server-sent events), go back chunk by chunk as the
//! runtime produces them.
//!
//! With a pinned model (`--model`), every request uses that model whatever
//! the client asks for, and `/v1/models` lists only it.
//!
//! Without `--upstream`, [`resolve`] finds the runtime itself: the one
//! running on this machine, or the one serving the model asked for
//! (`lnk model serve --model deepseek-r1`). See [`discover`].

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;

use crate::{BoxError, discover};

/// Ollama's default address.
pub const DEFAULT_UPSTREAM: &str = "http://localhost:11434";

/// The largest request body read to pin the model: a chat with a few
/// base64 images fits. Without a pinned model, bodies stream through
/// unread.
pub const MAX_PINNED_BODY: usize = 16 * 1024 * 1024;

/// Bytes of pinned request bodies held at once, each body's whole length
/// (its `Content-Length`, or [`MAX_PINNED_BODY`] without one) taken when
/// it starts, all at once: the rest wait (their uploads held back by flow
/// control), so a burst of large requests can't hold 64 x
/// [`MAX_PINNED_BODY`] in memory, and bodies half sent never hold the room
/// each other needs to finish.
pub const PINNED_BYTES: usize = 4 * MAX_PINNED_BODY;

/// The largest pinned body rewritten on an async worker; larger ones are
/// rewritten on a thread of their own.
const PIN_ON_WORKER: usize = 64 * 1024;

/// How long a pinned request's body may go without sending anything, or
/// wait for room to hold it.
pub const PINNED_BODY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How long a pinned request's body may take in all.
pub const PINNED_BODY_TOTAL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// What a visitor sees when the runtime can't be reached. The details
/// (its address, the error) go to stderr, for the user, not to visitors.
const UNREACHABLE: &str = "could not reach the model runtime\n";

pub struct LlmService {
    /// The runtime's base URL, without a trailing slash.
    upstream: String,
    model: Option<String>,
    client: reqwest::Client,
    /// Room for [`PINNED_BYTES`] of pinned request bodies, a permit a byte.
    parses: std::sync::Arc<tokio::sync::Semaphore>,
    /// [`PINNED_BODY_TIMEOUT`], shorter in tests.
    body_timeout: std::time::Duration,
}

impl LlmService {
    /// A service forwarding to the runtime at `upstream` (e.g.
    /// `http://localhost:11434`), optionally pinned to one `model`.
    pub fn new(upstream: &str, model: Option<String>) -> Result<Self, BoxError> {
        let url = reqwest::Url::parse(upstream)
            .map_err(|err| format!("invalid upstream URL {upstream:?}: {err}"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(format!(
                "the upstream must be an http:// or https:// base URL, like {DEFAULT_UPSTREAM}"
            )
            .into());
        }
        if model.as_deref().is_some_and(|m| m.trim().is_empty()) {
            return Err("the model name is empty".into());
        }
        // The workspace's reqwest has TLS enabled, which needs a provider.
        link_fetch::install_crypto_provider();
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        let mut client = reqwest::Client::builder()
            // Pass redirects through to the public client unchanged.
            .redirect(reqwest::redirect::Policy::none());
        if local {
            // Never send localhost traffic through a configured HTTP proxy.
            client = client.no_proxy();
        }
        Ok(Self {
            upstream: url.as_str().trim_end_matches('/').to_string(),
            model,
            client: client.build().expect("static reqwest config is valid"),
            parses: std::sync::Arc::new(tokio::sync::Semaphore::new(PINNED_BYTES)),
            body_timeout: PINNED_BODY_TIMEOUT,
        })
    }

    /// A pinned request's body, `length` bytes as its `Content-Length`
    /// says (none without one), and the room it holds; or the status to
    /// answer with.
    async fn read_pinned(
        &self,
        body: Body,
        length: Option<usize>,
    ) -> Result<(Vec<u8>, tokio::sync::OwnedSemaphorePermit), (http::StatusCode, &'static str)>
    {
        use futures_util::StreamExt;
        let too_slow = (http::StatusCode::REQUEST_TIMEOUT, "request body too slow\n");
        let too_large = (
            http::StatusCode::PAYLOAD_TOO_LARGE,
            "request body too large\n",
        );
        let most = length.unwrap_or(MAX_PINNED_BODY);
        if most > MAX_PINNED_BODY {
            return Err(too_large);
        }
        // Its whole room before any of it is read: bodies each holding a
        // part of the room can't wait on each other for the rest.
        let room = self.parses.clone().acquire_many_owned(most as u32);
        let held = match tokio::time::timeout(self.body_timeout, room).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) | Err(_) => {
                return Err((
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    "busy with other large requests: try again\n",
                ));
            }
        };
        let deadline = tokio::time::Instant::now() + PINNED_BODY_TOTAL;
        let mut stream = body.into_data_stream();
        let mut bytes = Vec::new();
        loop {
            let wait = (tokio::time::Instant::now() + self.body_timeout).min(deadline);
            let chunk = match tokio::time::timeout_at(wait, stream.next()).await {
                Err(_) => return Err(too_slow),
                Ok(None) => return Ok((bytes, held)),
                Ok(Some(Err(_))) => {
                    return Err((http::StatusCode::BAD_REQUEST, "request body failed\n"));
                }
                Ok(Some(Ok(chunk))) => chunk,
            };
            if bytes.len() + chunk.len() > most {
                return Err(too_large);
            }
            bytes.extend_from_slice(&chunk);
        }
    }

    /// Answers one request: the API forwarded, or an error the client can
    /// read (502 when the runtime can't be reached).
    pub async fn handle(&self, req: Request) -> Response {
        match self.serve(req).await {
            Ok(response) => response,
            Err(err) => {
                link_format::aside(link_format::Report::warning(link_format::Sentence::adopt(
                    &format!("lnk model serve: {}", link_format::clean(&err.to_string())),
                )));
                text(http::StatusCode::BAD_GATEWAY, UNREACHABLE.into())
            }
        }
    }

    async fn serve(&self, req: Request) -> Result<Response, BoxError> {
        let (mut parts, body) = req.into_parts();
        // Only this machine's own names: a web page that pointed its own
        // name here (DNS rebinding) would otherwise reach the model.
        if !this_machine(&parts.headers) {
            return reply(
                http::StatusCode::FORBIDDEN,
                "forbidden: only this machine's own address\n",
            );
        }
        if parts.uri.path() == "/" && parts.method == http::Method::GET {
            return Ok(text(http::StatusCode::OK, self.usage(&parts.headers)));
        }
        let Some(target) = api_target(&parts.method, &parts.uri) else {
            return reply(
                http::StatusCode::NOT_FOUND,
                "not found: only the OpenAI-compatible inference API under /v1/ is exposed\n",
            );
        };
        let target = target.to_string();
        let method = parts.method.clone();
        if !matches!(
            method,
            http::Method::GET | http::Method::POST | http::Method::OPTIONS
        ) {
            return reply(http::StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n");
        }
        remove_hop_by_hop(&mut parts.headers);
        parts.headers.remove(http::header::HOST);

        let mut body = Some(body);
        if let Some(model) = &self.model {
            let path = parts.uri.path();
            if method == http::Method::GET && path == "/v1/models" {
                return self.list_pinned(parts.headers, model).await;
            }
            if let Some(id) = path.strip_prefix("/v1/models/")
                && id != model
            {
                return reply(http::StatusCode::NOT_FOUND, "no such model\n");
            }
            if method == http::Method::POST {
                let length = parts
                    .headers
                    .get(http::header::CONTENT_LENGTH)
                    .and_then(|length| length.to_str().ok()?.parse().ok());
                let (bytes, permit) = match self.read_pinned(body.take().unwrap(), length).await {
                    Ok(read) => read,
                    Err((status, why)) => return reply(status, why),
                };
                // A large body is rewritten off the async workers (15 MiB
                // takes tens of milliseconds), so other requests' tokens
                // don't wait for it.
                let pinned = if bytes.len() > PIN_ON_WORKER {
                    let model = model.clone();
                    tokio::task::spawn_blocking(move || pin_model(&bytes, &model))
                        .await
                        .ok()
                        .flatten()
                } else {
                    let pinned = pin_model(&bytes, model);
                    drop(bytes);
                    pinned
                };
                let Some(pinned) = pinned else {
                    return reply(
                        http::StatusCode::BAD_REQUEST,
                        "this endpoint serves one model: send a JSON object body\n",
                    );
                };
                parts
                    .headers
                    .insert(http::header::CONTENT_LENGTH, pinned.len().into());
                // The rewritten copy stays counted, as the body it came
                // from, until it's written to the runtime, not while the
                // runtime works on it: a slow runtime can't make copies
                // pile up outside the budget, nor hold up other requests.
                let body = counted_body(pinned, Some(permit));
                return self
                    .forward(method, &target, parts.headers, Some(body))
                    .await;
            }
        }

        // The upload streams through as the client sends it, with its
        // Content-Length when it has one; without it the body goes chunked.
        let empty = parts
            .headers
            .get(http::header::CONTENT_LENGTH)
            .is_some_and(|length| length == "0");
        let body = if empty {
            parts.headers.remove(http::header::CONTENT_LENGTH);
            None
        } else {
            body.map(|body| reqwest::Body::wrap_stream(body.into_data_stream()))
        };
        self.forward(method, &target, parts.headers, body).await
    }

    async fn forward(
        &self,
        method: http::Method,
        target: &str,
        headers: http::HeaderMap,
        body: Option<reqwest::Body>,
    ) -> Result<Response, BoxError> {
        let request = self
            .client
            .request(method, format!("{}{target}", self.upstream))
            .headers(headers);
        let request = match body {
            Some(body) => request.body(body),
            None => request,
        };
        let upstream = request.send().await.map_err(|err| self.unreachable(err))?;

        let mut headers = upstream.headers().clone();
        remove_hop_by_hop(&mut headers);
        let mut response = http::Response::builder().status(upstream.status());
        if let Some(response_headers) = response.headers_mut() {
            *response_headers = headers;
        }
        Ok(response.body(Body::from_stream(upstream.bytes_stream()))?)
    }

    /// `GET /v1/models` with a pinned model: the runtime's list, keeping
    /// only that model.
    async fn list_pinned(
        &self,
        mut headers: http::HeaderMap,
        model: &str,
    ) -> Result<Response, BoxError> {
        // The list is rewritten, so it has to arrive uncompressed.
        headers.remove(http::header::ACCEPT_ENCODING);
        headers.remove(http::header::CONTENT_LENGTH);
        let upstream = self
            .client
            .get(format!("{}/v1/models", self.upstream))
            .headers(headers)
            .send()
            .await
            .map_err(|err| self.unreachable(err))?;
        let status = upstream.status();
        let bytes = upstream
            .bytes()
            .await
            .map_err(|err| self.unreachable(err))?;
        if !status.is_success() {
            return Ok(http::Response::builder()
                .status(status)
                .body(Body::from(bytes))?);
        }
        let mut list: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|err| format!("{} sent an invalid model list: {err}", self.upstream))?;
        if let Some(data) = list.get_mut("data").and_then(|d| d.as_array_mut()) {
            data.retain(|entry| entry.get("id").and_then(|id| id.as_str()) == Some(model));
        }
        Ok(http::Response::builder()
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&list)?))?)
    }

    /// The page at `/`: what this URL is and how to call it.
    fn usage(&self, headers: &http::HeaderMap) -> String {
        let header = |name| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                // Only a plain host name goes into the example.
                .filter(|v| {
                    !v.is_empty()
                        && v.chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
                })
        };
        let scheme = header("x-forwarded-proto").unwrap_or("https");
        let base = match header("x-forwarded-host") {
            Some(host) => format!("{scheme}://{host}/v1"),
            None => "<this URL>/v1".into(),
        };
        let model = self.model.as_deref().unwrap_or("<a model from /v1/models>");
        format!(
            "This is an OpenAI-compatible API, served with Link (lnk model serve).\n\
             \n\
             Base URL: {base}\n\
             \n\
             List models:\n\
             \x20 curl {base}/models\n\
             \n\
             Chat (add \"stream\":true to stream):\n\
             \x20 curl {base}/chat/completions \\\n\
             \x20   -H 'content-type: application/json' \\\n\
             \x20   -d '{{\"model\":\"{model}\",\"messages\":[{{\"role\":\"user\",\"content\":\"hi\"}}]}}'\n"
        )
    }

    fn unreachable(&self, err: reqwest::Error) -> BoxError {
        format!("could not reach {}: {err}", self.upstream).into()
    }
}

/// What `lnk model serve` or `lnk model serve <model>` serves.
pub struct Target {
    pub service: LlmService,
    /// What usually runs there (`Ollama`, ...), when it was discovered.
    pub runtime: Option<String>,
    pub upstream: String,
    pub model: Option<String>,
}

/// Picks the runtime to expose. An `upstream` is used as given. Otherwise
/// the runtimes on this machine are discovered: with a `model`, the one
/// serving it (a name without a tag finds a tagged model); without one,
/// the only runtime running.
pub async fn resolve(upstream: Option<&str>, model: Option<&str>) -> Result<Target, BoxError> {
    if let Some(upstream) = upstream {
        let model = model.map(str::to_string);
        return Ok(Target {
            service: LlmService::new(upstream, model.clone())?,
            runtime: None,
            upstream: upstream.trim_end_matches('/').to_string(),
            model,
        });
    }
    choose(&discover::discover().await, model)
}

/// The runtime among `runtimes` to expose for `model` (see [`resolve`]).
pub fn choose(runtimes: &[discover::Runtime], model: Option<&str>) -> Result<Target, BoxError> {
    let (runtime, model) = match model {
        Some(model) => {
            let (runtime, id) = discover::find_model(runtimes, model)?;
            (runtime, Some(id))
        }
        None => match runtimes {
            [runtime] => (runtime, None),
            [] => return Err(discover::nothing_running().into()),
            several => {
                let found: Vec<_> = several
                    .iter()
                    .map(|r| format!("{} at {}", r.name, r.base_url))
                    .collect();
                return Err(format!(
                    "several model runtimes are running ({}): pick one with --upstream, \
                     or name a model (lnk model list lists them)",
                    found.join(", ")
                )
                .into());
            }
        },
    };
    Ok(Target {
        service: LlmService::new(&runtime.base_url, model.clone())?,
        runtime: Some(runtime.name.clone()),
        upstream: runtime.base_url.clone(),
        model,
    })
}

/// Serves `service` on `listener` until `until` finishes: then it stops
/// accepting, its port closed at once, answers each request it already
/// took and returns once they're answered.
pub async fn serve(
    listener: tokio::net::TcpListener,
    service: LlmService,
    until: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let service = std::sync::Arc::new(service);
    let app = axum::Router::new().fallback(move |req: Request| {
        let service = service.clone();
        async move { service.handle(req).await }
    });
    // Each streamed token goes out at once, not after the peer's delayed ACK.
    let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
        let _ = tcp.set_nodelay(true);
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(until)
        .await
}

/// The inference paths passed on: a runtime's own under `/v1` (Jan's
/// model pulls, LocalAI's file uploads, vLLM's adapters) are not.
const INFERENCE: &[&str] = &[
    "/v1/chat/completions",
    "/v1/completions",
    "/v1/embeddings",
    "/v1/responses",
    "/v1/audio/transcriptions",
    "/v1/models",
];

/// Whether the request names this machine (`127.0.0.1`, `localhost`,
/// `[::1]`), or no host at all.
fn this_machine(headers: &http::HeaderMap) -> bool {
    let Some(host) = headers.get(http::header::HOST) else {
        return true;
    };
    let host = host.to_str().unwrap_or_default().to_ascii_lowercase();
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default().to_string(),
        None => host
            .rsplit_once(':')
            .map_or(host.as_str(), |(n, _)| n)
            .to_string(),
    };
    matches!(name.as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// The request target (`/v1/...?query`) if it is one of the inference
/// API's ([`INFERENCE`], and reading a model's `/v1/models/<id>`). Paths that could
/// resolve somewhere else (`..`, `.`, percent-encoding, `\`) are refused:
/// URL parsing would turn `/v1/../api/pull` into `/api/pull`.
fn api_target<'a>(method: &http::Method, uri: &'a http::Uri) -> Option<&'a str> {
    let path = uri.path();
    let plain = !path.contains(['%', '\\'])
        && !path
            .split('/')
            .any(|segment| segment == "." || segment == "..");
    let one_model = *method == http::Method::GET
        && path
            .strip_prefix("/v1/models/")
            .is_some_and(|id| !id.is_empty() && !id.contains('/'));
    let inference = INFERENCE.contains(&path) || one_model;
    (plain && inference).then(|| uri.path_and_query().map_or(path, |pq| pq.as_str()))
}

/// `bytes` as one chunk, `permit` held until it has been taken and the
/// body's end asked for: until it's written.
fn counted_body(
    bytes: Vec<u8>,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
) -> reqwest::Body {
    let chunks =
        futures_util::stream::unfold((Some(bytes), permit), |(bytes, permit)| async move {
            let bytes = bytes?;
            Some((Ok::<_, std::convert::Infallible>(bytes), (None, permit)))
        });
    reqwest::Body::wrap_stream(chunks)
}

/// The JSON object `body` with its `model` set to `model`, or `None` if
/// the body isn't a JSON object.
fn pin_model(body: &[u8], model: &str) -> Option<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value
        .as_object_mut()?
        .insert("model".into(), serde_json::Value::String(model.into()));
    serde_json::to_vec(&value).ok()
}

fn reply(status: http::StatusCode, message: &'static str) -> Result<Response, BoxError> {
    Ok(text(status, message.to_string()))
}

fn text(status: http::StatusCode, text: String) -> Response {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(text))
        .expect("static response parts are valid")
}

/// Headers about one connection, which must not be passed on to the next.
fn remove_hop_by_hop(headers: &mut http::HeaderMap) {
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_v1_api_is_forwarded() {
        let uri = |uri: &'static str| http::Uri::from_static(uri);
        let target = |u: &'static str| api_target(&http::Method::POST, &uri(u)).map(str::to_string);
        let read = |u: &'static str| api_target(&http::Method::GET, &uri(u)).map(str::to_string);
        assert_eq!(
            target("/v1/chat/completions").as_deref(),
            Some("/v1/chat/completions")
        );
        assert_eq!(target("/v1/models?x=1").as_deref(), Some("/v1/models?x=1"));
        assert_eq!(
            read("/v1/models/llama3:8b").as_deref(),
            Some("/v1/models/llama3:8b")
        );
        for refused in [
            "/",
            "/v1",
            "/api/pull",
            "/api/delete",
            "/v1/../api/pull",
            "/v1/./x",
            "/v1/%2e%2e/api/pull",
            "/v1/..%2Fapi/pull",
            "/v1/\\..\\api",
            "*",
            // A runtime's own, under /v1.
            "/v1/models/pull",
            "/v1/models/x/start",
            "/v1/engines/llama-cpp/install",
            "/v1/files",
            "/v1/load_lora_adapter",
        ] {
            assert_eq!(target(refused), None, "{refused}");
        }
    }

    #[test]
    fn only_this_machines_names_are_answered() {
        let with = |host: &str| {
            let mut headers = http::HeaderMap::new();
            headers.insert(http::header::HOST, host.parse().unwrap());
            this_machine(&headers)
        };
        assert!(this_machine(&http::HeaderMap::new()));
        for fine in [
            "127.0.0.1:4000",
            "localhost:4000",
            "LOCALHOST",
            "[::1]:4000",
        ] {
            assert!(with(fine), "{fine}");
        }
        for rebound in [
            "evil.example:4000",
            "127.0.0.1.nip.io:4000",
            "localhost.evil.example",
        ] {
            assert!(!with(rebound), "{rebound}");
        }
    }

    #[test]
    fn pinning_replaces_the_model() {
        let pinned = pin_model(br#"{"model":"other","messages":[]}"#, "qwen3").unwrap();
        let value: serde_json::Value = serde_json::from_slice(&pinned).unwrap();
        assert_eq!(value["model"], "qwen3");
        assert_eq!(value["messages"], serde_json::json!([]));
        let added = pin_model(br#"{"prompt":"hi"}"#, "qwen3").unwrap();
        assert!(
            String::from_utf8(added)
                .unwrap()
                .contains(r#""model":"qwen3""#)
        );
        assert!(pin_model(b"[1]", "qwen3").is_none());
        assert!(pin_model(b"not json", "qwen3").is_none());
    }

    #[test]
    fn upstream_must_be_a_base_url() {
        let ok = LlmService::new("http://localhost:11434/", None).unwrap();
        assert_eq!(ok.upstream, "http://localhost:11434");
        let with_path = LlmService::new("https://example.com/runtime/", None).unwrap();
        assert_eq!(with_path.upstream, "https://example.com/runtime");
        for bad in ["localhost:11434", "ftp://x", "http://x/?a=b", "not a url"] {
            assert!(LlmService::new(bad, None).is_err(), "{bad}");
        }
        assert!(LlmService::new(DEFAULT_UPSTREAM, Some(" ".into())).is_err());
    }

    /// Uploads that stall give their room back, so they can't hold up
    /// every pinned request.
    #[tokio::test(start_paused = true)]
    async fn stalled_pinned_uploads_time_out() {
        let service = LlmService::new(DEFAULT_UPSTREAM, Some("qwen3".into())).unwrap();
        for _ in 0..6 {
            let stalled = futures_util::stream::pending::<Result<axum::body::Bytes, BoxError>>();
            let req = http::Request::post("/v1/chat/completions")
                .body(Body::from_stream(stalled))
                .unwrap();
            let res = service.handle(req).await;
            assert_eq!(res.status(), http::StatusCode::REQUEST_TIMEOUT);
        }
        assert_eq!(service.parses.available_permits(), PINNED_BYTES);
    }

    /// An upload that keeps sending isn't cut off, however long it
    /// takes in all: only one that goes quiet is.
    #[tokio::test(start_paused = true)]
    #[expect(
        clippy::disallowed_methods,
        reason = "advances only the upload's paused Tokio clock to prove that each chunk renews its idle deadline"
    )]
    async fn slow_pinned_uploads_that_keep_sending_are_read() {
        let service = LlmService::new(DEFAULT_UPSTREAM, Some("qwen3".into())).unwrap();
        let pieces = ["{\"model\"", ":\"x\",", "\"messages\"", ":[]}"];
        let slow =
            futures_util::StreamExt::then(futures_util::stream::iter(pieces), |piece| async move {
                tokio::time::sleep(PINNED_BODY_TIMEOUT / 2).await;
                Ok::<_, BoxError>(axum::body::Bytes::from(piece))
            });
        let (bytes, held) = service
            .read_pinned(Body::from_stream(slow), None)
            .await
            .unwrap();
        assert_eq!(bytes, pieces.concat().as_bytes());
        drop(held);
        assert_eq!(service.parses.available_permits(), PINNED_BYTES);
    }

    /// A pinned body's room comes back once it's written to the runtime,
    /// not when the runtime answers: a slow runtime doesn't hold up the
    /// other pinned requests.
    #[tokio::test]
    async fn a_pinned_body_is_counted_until_written() {
        use futures_util::StreamExt;
        let service = LlmService::new(DEFAULT_UPSTREAM, Some("qwen3".into())).unwrap();
        let input = br#"{"messages":[]}"#;
        for (length, finish) in [
            (Some(input.len()), true),
            (None, true),
            (Some(input.len()), false),
            (None, false),
        ] {
            let charged = length.unwrap_or(MAX_PINNED_BODY);
            let (bytes, held) = service
                .read_pinned(Body::from(input.as_slice()), length)
                .await
                .unwrap();
            let pinned = pin_model(&bytes, "qwen3").unwrap();
            let mut writing =
                Body::new(counted_body(pinned.clone(), Some(held))).into_data_stream();
            assert_eq!(service.parses.available_permits(), PINNED_BYTES - charged);
            assert_eq!(writing.next().await.unwrap().unwrap(), pinned);
            assert_eq!(service.parses.available_permits(), PINNED_BYTES - charged);
            if finish {
                assert!(writing.next().await.is_none());
                assert_eq!(service.parses.available_permits(), PINNED_BYTES);
            }
            // Cancellation also releases the body without waiting for an answer.
            drop(writing);
            assert_eq!(service.parses.available_permits(), PINNED_BYTES);
        }
    }

    /// Large pinned uploads at once, more than the room holds, each take
    /// their whole room when they start: those that fit finish, then the
    /// rest, never each holding a part and waiting on the others for good.
    #[tokio::test]
    async fn pinned_uploads_past_the_room_take_turns() {
        use futures_util::{StreamExt, poll};
        let mut service = LlmService::new(DEFAULT_UPSTREAM, Some("qwen3".into())).unwrap();
        let body = br#"{"messages":[]}"#;
        let length = body.len();
        // The admission contract depends on the room/body ratio, not MiB.
        // Four whole bodies fit; the fifth must wait without holding a part.
        let room = length * 4;
        service.parses = std::sync::Arc::new(tokio::sync::Semaphore::new(room));
        let mut senders = Vec::new();
        let mut uploads: Vec<_> = (0..5)
            .map(|_| {
                let (send, receive) = tokio::sync::oneshot::channel();
                senders.push(send);
                let chunks = futures_util::stream::once(async move {
                    Ok::<_, BoxError>(axum::body::Bytes::from(receive.await.unwrap()))
                });
                Box::pin(service.read_pinned(Body::from_stream(chunks), Some(length)))
            })
            .collect();
        for (i, upload) in uploads.iter_mut().enumerate() {
            assert!(poll!(upload.as_mut()).is_pending());
            assert_eq!(
                service.parses.available_permits(),
                room - length * (i + 1).min(4)
            );
        }
        for send in senders {
            send.send(body.to_vec()).unwrap();
        }
        let mut small = Box::pin(service.read_pinned(Body::from("{}"), Some(2)));
        assert!(poll!(small.as_mut()).is_pending());
        for upload in uploads.iter_mut().take(4) {
            let (bytes, held) = upload.await.unwrap();
            assert_eq!(bytes, body);
            // Room stays held until forwarding writes or drops the body.
            let mut writing = Body::new(counted_body(bytes, Some(held))).into_data_stream();
            assert_eq!(writing.next().await.unwrap().unwrap(), body.as_slice());
            assert!(writing.next().await.is_none());
        }
        let (bytes, held) = uploads.pop().unwrap().await.unwrap();
        assert_eq!(bytes, body);
        drop(held);
        let (bytes, held) = small.await.unwrap();
        assert_eq!(bytes, b"{}");
        drop(held);
        assert_eq!(service.parses.available_permits(), room);
        // One that says it's larger than a body may be is refused at once.
        let req = http::Request::post("/v1/chat/completions")
            .header(http::header::HOST, "127.0.0.1")
            .header(http::header::CONTENT_LENGTH, MAX_PINNED_BODY + 1)
            .body(Body::from_stream(futures_util::stream::pending::<
                Result<axum::body::Bytes, BoxError>,
            >()))
            .unwrap();
        let res = service.handle(req).await;
        assert_eq!(res.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_502_does_not_show_the_upstream() {
        // Nothing listens on the port, held so no other test takes it.
        let held = link_test_kit::HeldNumber::port();
        let port = held.port_number();
        let service = LlmService::new(&format!("http://127.0.0.1:{port}"), None).unwrap();
        let req = http::Request::get("/v1/models")
            .body(Body::empty())
            .unwrap();
        let res = service.handle(req).await;
        assert_eq!(res.status(), http::StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], UNREACHABLE.as_bytes());
        assert!(!String::from_utf8_lossy(&body).contains(&port.to_string()));
    }
}
