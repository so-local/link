//! The hosted model route: its key added outside the sandbox.
//!
//! A program behind a proxy reaches its provider at
//! `http://127.0.0.1:40781/_link/model` ([`link_model_contract::route::url`])
//! with a placeholder key. The proxy hands each call to this bridge,
//! which reads its request, passes it on only if it's inference (as for a
//! local model), puts the real key in, and speaks TLS to the provider
//! itself. The key is never in the sandbox: nothing there to find or send
//! elsewhere.
//!
//! A provider's own tools that run on its servers (web search, web fetch,
//! code execution, MCP servers, a URL it fetches for an image) are a
//! network of their own: through them a program allowed only its model
//! would reach the internet. Unless the policy allows them, they're
//! removed from each request, and a request asking the provider to fetch
//! a URL is refused.

use std::time::Duration;

use anyhow::bail;
use link_model_contract::route::ModelKey;
use link_sandbox_route::Verdict;
use link_sandbox_route::Verdicts;
use link_sandbox_route::http::RequestHead;
use link_sandbox_route::pool::Pool;
use link_secretive::Duplicate;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod relaying;
mod tools;

pub use relaying::relay_to;
pub use tools::strip_server_tools;
use tools::{fetched_url, preset, searching};

/// The largest request body passed on (a provider's own limit is about
/// this: images and documents ride in it).
const MAX_BODY: usize = 32 * 1024 * 1024;
/// How long a request's body may take to arrive.
const BODY_TIMEOUT: Duration = Duration::from_secs(120);

/// A provider's address, and what of its API is inference.
struct Provider {
    host: &'static str,
    /// Its API's path before `/v1` (OpenRouter's is `/api/v1`).
    root: &'static str,
    allowed: &'static [(&'static str, &'static str)],
}

fn provider(name: &str) -> Option<Provider> {
    Some(match name {
        "anthropic" => Provider {
            host: "api.anthropic.com",
            root: "",
            allowed: &[
                ("POST", "/v1/messages"),
                ("POST", "/v1/messages/count_tokens"),
                ("GET", "/v1/models"),
            ],
        },
        "openai" => Provider {
            host: "api.openai.com",
            root: "",
            allowed: &[
                ("POST", "/v1/chat/completions"),
                ("POST", "/v1/responses"),
                ("POST", "/v1/embeddings"),
                ("POST", "/v1/audio/transcriptions"),
                ("GET", "/v1/models"),
            ],
        },
        "openrouter" => Provider {
            host: "openrouter.ai",
            root: "/api",
            allowed: &[
                ("POST", "/v1/chat/completions"),
                ("POST", "/v1/completions"),
                ("POST", "/v1/responses"),
                ("POST", "/v1/embeddings"),
                ("GET", "/v1/models"),
            ],
        },
        _ => return None,
    })
}

/// Whether `method` on `path` (without its `?query`) is inference at
/// `provider`: messages, completions, embeddings, a voice note's
/// transcription, the models' list. Never
/// files, batches, fine-tuning or the account. Paths are matched as sent:
/// an encoded or relative one is refused.
pub fn allowed(provider_name: &str, method: &str, path: &str) -> bool {
    let Some(provider) = provider(provider_name) else {
        return false;
    };
    if provider.allowed.contains(&(method, path)) {
        return true;
    }
    method == "GET"
        && path.strip_prefix("/v1/models/").is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
                && !name.contains("..")
                && !name.contains("//")
        })
}

/// A request ready for the provider: its host, and the bytes to send.
#[derive(Debug)]
pub struct Prepared {
    pub host: String,
    pub bytes: Vec<u8>,
    /// What it asks, when it asks for a model's answer.
    pub asked: Option<crate::bridge::context::Asked>,
}

/// A request's head, parsed.
struct Head<'a> {
    method: &'a str,
    path: &'a str,
    query: Option<&'a str>,
    /// Headers passed on as they are.
    kept: Vec<&'a str>,
    length: usize,
    /// What the program sent as its key (`x-api-key`, or a bearer
    /// token): its placeholder, if it's the program.
    credentials: Vec<&'a str>,
}

fn parse(head: &[u8], at: bool) -> Result<Head<'_>, String> {
    use link_model_contract::route::{HOSTED, HOSTED_AT, HOSTED_AT_FALLBACK};
    let request = RequestHead::parse(head)?;
    // Its route's prefix: the provider's, or an address of its own's.
    let prefixes: &[&str] = match at {
        false => &[HOSTED],
        true => &[HOSTED_AT_FALLBACK, HOSTED_AT],
    };
    let Some(rest) = prefixes.iter().find_map(|prefix| {
        request
            .target
            .strip_prefix(prefix)
            .filter(|rest| rest.is_empty() || rest.starts_with(['/', '?']))
    }) else {
        return Err(format!("{} isn't on the wire", request.target));
    };
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let mut head = Head {
        method: request.method,
        path,
        query,
        kept: Vec::new(),
        length: 0,
        credentials: Vec::new(),
    };
    for header in &request.headers {
        let value = header.value;
        match header.name.as_str() {
            "upgrade" => return Err("no switching protocols".into()),
            "transfer-encoding" => return Err("a body without a length (chunked)".into()),
            "content-length" => {
                head.length = value.parse().map_err(|_| "a bad content-length")?;
            }
            "x-api-key" => head.credentials.push(value),
            "authorization" => head.credentials.push(
                value
                    .split_once(' ')
                    .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
                    .map_or("", |(_, token)| token.trim()),
            ),
            // Link's to set: where it goes, the key, one request.
            "host" | "expect" | "connection" | "keep-alive" | "te" | "trailer" => {}
            other if other.starts_with("proxy-") => {}
            _ => head.kept.push(header.line),
        }
    }
    if head.length > MAX_BODY {
        return Err(format!("a body over {} MB", MAX_BODY / 1024 / 1024));
    }
    Ok(head)
}

/// The request to send `key`'s provider for `head` and `body`, at its
/// provider's address or the one of its own it's at, or why not:
/// inference only, its server-side tools removed unless allowed, the
/// real key in place of whatever the program sent. `listed`: the
/// sandbox's policy lists the hosts it may reach, so a model that
/// searches the web by nature is refused too.
pub fn prepare(key: &ModelKey, head: &[u8], body: &[u8], listed: bool) -> Result<Prepared, String> {
    let Some(provider) = provider(&key.provider) else {
        return Err(format!(
            "{} isn't a provider Link adds keys for",
            key.provider
        ));
    };
    let parsed = parse(head, key.at.is_some())?;
    // At an address of its own, its host and root: the provider's API,
    // inference alone, there.
    let (host, root) = match &key.at {
        Some(at) => (at.host.clone(), at.root.as_str()),
        None => (provider.host.to_string(), provider.root),
    };
    // Only the program given the placeholder gets the key added: on
    // macOS anyone on this machine can reach the proxy's port.
    let placeholder = key.placeholder.expose();
    let shown = |c: &&str| link_secretive::equal(c.as_bytes(), placeholder.as_bytes());
    if parsed.credentials.is_empty() || !parsed.credentials.iter().all(shown) {
        return Err("no key, or not the placeholder this sandbox was given".into());
    }
    if !allowed(&key.provider, parsed.method, parsed.path) {
        return Err(format!(
            "{} {} isn't inference (messages, completions, embeddings, the models' list)",
            parsed.method, parsed.path
        ));
    }
    let mut checked = None;
    let api =
        link_model_contract::context::Api::of(parsed.path).filter(|_| parsed.method == "POST");
    // Read as JSON to be checked, and to take what a model's request
    // asks: one that isn't JSON is refused only where it's checked.
    let json = match body.is_empty() {
        true => None,
        false if !key.server_tools => {
            Some(serde_json::from_slice(body).map_err(|_| "a request body that isn't JSON")?)
        }
        false => api.and_then(|_| serde_json::from_slice(body).ok()),
    };
    let mut asked = None;
    if let Some(mut json) = json {
        if !key.server_tools {
            strip_server_tools(&key.provider, &mut json);
            if let Some(model) = preset(&key.provider, &json) {
                return Err(format!(
                    "{model} is a preset, which can search the web for the sandbox (server-side \
                     tools are off)"
                ));
            }
            if let Some(model) = searching(&key.provider, &json).filter(|_| listed) {
                return Err(format!(
                    "{model} searches the web by nature, past this sandbox's list of hosts \
                     (server-side tools are off)"
                ));
            }
            if let Some(url) = fetched_url(&json) {
                return Err(format!(
                    "the provider would fetch {url} for the sandbox (server-side tools are off)"
                ));
            }
            // Always what was checked, written again, even when nothing was
            // removed: the program's own bytes could mean something else to
            // the provider's parser (a key given twice, say).
            checked = Some(serde_json::to_vec(&json).map_err(|e| e.to_string())?);
        }
        asked = api.and_then(|api| crate::bridge::context::Asked::of(api, &json, None));
    }
    let body = checked.as_deref().unwrap_or(body);
    let target = match parsed.query {
        Some(query) => format!("{root}{}?{query}", parsed.path),
        None => format!("{root}{}", parsed.path),
    };
    let mut out = format!("{} {target} HTTP/1.1\r\nHost: {host}\r\n", parsed.method);
    // A model's answer comes uncompressed, for its usage to be read
    // ([`crate::usage`]).
    let metered = crate::bridge::usage::Format::of(parsed.method, parsed.path).is_some();
    for line in parsed.kept {
        if metered && line.to_ascii_lowercase().starts_with("accept-encoding:") {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    if metered {
        out.push_str("Accept-Encoding: identity\r\n");
    }
    // Written into the request as it goes, with no copy of its own.
    let header = link_model_contract::route::KeyHeader::of(&key.provider, &key.key);
    out.push_str(header.name());
    out.push_str(": ");
    out.push_str(header.prefix());
    out.push_str(key.key.expose());
    out.push_str("\r\n");
    if !body.is_empty() || parsed.method == "POST" {
        out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    out.push_str("\r\n");
    let mut bytes = Vec::with_capacity(out.len() + body.len());
    bytes.extend_from_slice(out.as_bytes());
    bytes.extend_from_slice(body);
    Ok(Prepared { host, bytes, asked })
}

/// An error unless the proxy can add `key`: a provider it knows, and a
/// placeholder Link made for it.
pub fn check_key(key: &ModelKey) -> anyhow::Result<()> {
    if provider(&key.provider).is_none() {
        bail!("{} isn't a provider Link adds keys for", key.provider);
    }
    key.check_placeholder()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The placeholder `key()`'s program is given for its run.
    const PLACEHOLDER: &str = "sk-link-wire-00112233445566778899aabbccddeeff";

    pub(super) fn key(provider: &str) -> ModelKey {
        ModelKey {
            provider: provider.into(),
            key: "sk-real-key".to_string().into(),
            placeholder: PLACEHOLDER.to_string().into(),
            server_tools: false,
            at: None,
        }
    }

    /// `head` with the placeholder the program is given for `key()`'s
    /// key, in place of any other it names, or added.
    pub(super) fn wired(head: &[u8]) -> Vec<u8> {
        let placeholder = PLACEHOLDER;
        let text = String::from_utf8_lossy(head)
            .replace("sk-ant-api03-link-wire", placeholder)
            .replace("sk-or-v1-link-wire", placeholder);
        if text.to_ascii_lowercase().contains("x-api-key:")
            || text.to_ascii_lowercase().contains("authorization:")
        {
            return text.into_bytes();
        }
        let (line, rest) = text.split_once("\r\n").unwrap_or((&text, ""));
        format!("{line}\r\nx-api-key: {placeholder}\r\n{rest}").into_bytes()
    }

    /// A key comes with its program's placeholder, one Link made.
    #[test]
    fn a_key_without_a_placeholder_link_made_is_refused() {
        check_key(&key("anthropic")).unwrap();
        for placeholder in ["", "sk-link-wire-", "sk-real-key"] {
            let unplaced = ModelKey {
                placeholder: placeholder.to_string().into(),
                ..key("anthropic")
            };
            assert!(check_key(&unplaced).is_err(), "{placeholder:?}");
        }
    }

    /// Only the program given the placeholder gets the key added: none,
    /// another placeholder, or the real key itself is refused.
    #[test]
    fn only_the_placeholder_gets_the_key() {
        let placeholder = PLACEHOLDER;
        // Made for the same key, by another run or holder.
        let another = link_model_contract::route::placeholder(
            &link_sandbox_route::Placeholders::new().unwrap(),
            &key("anthropic").key,
        );
        for credential in [
            String::new(),
            format!("x-api-key: {}\r\n", another.expose()),
            "x-api-key: sk-ant-api03-link-wire\r\n".into(),
            "x-api-key: sk-real-key\r\n".into(),
            format!("x-api-key: {placeholder}x\r\n"),
            format!("x-api-key: {placeholder}\r\nAuthorization: Bearer other\r\n"),
            format!("Authorization: Basic {placeholder}\r\n"),
        ] {
            let head = format!(
                "POST /_link/model/v1/messages HTTP/1.1\r\n{credential}Content-Length: 2\r\n\r\n"
            );
            let err = prepare(&key("anthropic"), head.as_bytes(), b"{}", false).unwrap_err();
            assert!(err.contains("placeholder"), "{credential:?}: {err}");
        }
        for credential in [
            format!("x-api-key: {placeholder}"),
            format!("Authorization: Bearer {placeholder}"),
            format!("authorization: bearer  {placeholder} "),
        ] {
            let head = format!(
                "POST /_link/model/v1/messages HTTP/1.1\r\n{credential}\r\nContent-Length: 2\r\n\r\n"
            );
            assert!(
                prepare(&key("anthropic"), head.as_bytes(), b"{}", false).is_ok(),
                "{credential}"
            );
        }
    }

    pub(super) fn sent(prepared: &Prepared) -> String {
        String::from_utf8(prepared.bytes.clone()).unwrap()
    }

    /// A model's request is taken part by part before it's sent, as it's
    /// checked or passed as it is; anything else isn't.
    #[test]
    fn a_models_request_is_taken_part_by_part() {
        let head = b"POST /_link/model/v1/messages HTTP/1.1\r\nContent-Length: 1\r\n\r\n";
        let body = serde_json::to_vec(&serde_json::json!({
            "model": "claude-opus-5-5",
            "system": "Be brief.",
            "tools": [{"name": "read", "input_schema": {}}],
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        for server_tools in [false, true] {
            let key = ModelKey {
                server_tools,
                ..key("anthropic")
            };
            let asked = prepare(&key, &wired(head), &body, false)
                .unwrap()
                .asked
                .expect("asked");
            assert_eq!(asked.model.as_deref(), Some("claude-opus-5-5"));
            assert!(asked.chat.starts_with("opening-"));
            let paths: Vec<&str> = asked.parts.iter().map(|p| p.path.as_str()).collect();
            assert_eq!(paths, ["tools/read", "instructions", "conversation/user"]);
        }
        let models = b"GET /_link/model/v1/models HTTP/1.1\r\n\r\n";
        assert!(
            prepare(&key("anthropic"), &wired(models), &[], false)
                .unwrap()
                .asked
                .is_none()
        );
        // Passed as it is, a body that isn't JSON is passed, not taken.
        let allowing = ModelKey {
            server_tools: true,
            ..key("anthropic")
        };
        let prepared = prepare(&allowing, &wired(head), b"not json", false).unwrap();
        assert!(prepared.asked.is_none());
    }

    /// A model at an address of its own (a provider a module adds, a `base_url`) is
    /// sent to its host under its root, inference alone, its provider's
    /// server tools removed, as a provider's own is: never its files or
    /// its account, and never on the provider's route.
    #[test]
    fn a_model_at_its_own_address_passes_inference_only() {
        let at =
            link_model_contract::route::At::new("openai", "https://gw.example/openai/v1").unwrap();
        let key = ModelKey {
            at: Some(at),
            ..key("openai")
        };
        let body = serde_json::to_vec(&serde_json::json!({
            "model": "gpt-6.1-sol",
            "tools": [{"type": "web_search"}, {"type": "function", "name": "read"}],
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        let head = |route: &str, path: &str| {
            wired(format!("POST {route}{path} HTTP/1.1\r\nContent-Length: 1\r\n\r\n").as_bytes())
        };
        for route in ["/_link/model-at", "/_link/model-at-fallback"] {
            let prepared = prepare(&key, &head(route, "/v1/chat/completions"), &body, false)
                .unwrap_or_else(|why| panic!("{route}: {why}"));
            let sent = sent(&prepared);
            assert_eq!(prepared.host, "gw.example");
            assert!(
                sent.starts_with(
                    "POST /openai/v1/chat/completions HTTP/1.1\r\nHost: gw.example\r\n"
                ),
                "{sent}"
            );
            assert!(sent.contains("Authorization: Bearer sk-real-key"), "{sent}");
            assert!(!sent.contains("web_search"), "{sent}");
            assert!(sent.contains("\"read\""), "{sent}");
        }
        // Not inference, or not on its own route: refused.
        for (route, path) in [
            ("/_link/model-at", "/v1/files"),
            ("/_link/model-at", "/v1/fine_tuning/jobs"),
            ("/_link/model-at", "/v1/organization/costs"),
            ("/_link/model", "/v1/chat/completions"),
            ("/_link/model-attic", "/v1/chat/completions"),
        ] {
            assert!(
                prepare(&key, &head(route, path), &body, false).is_err(),
                "{route}{path}"
            );
        }
    }

    #[test]
    fn only_inference_passes() {
        for (provider, method, path) in [
            ("anthropic", "POST", "/v1/messages"),
            ("anthropic", "POST", "/v1/messages/count_tokens"),
            ("anthropic", "GET", "/v1/models"),
            ("anthropic", "GET", "/v1/models/claude-opus-5-5"),
            ("openai", "POST", "/v1/responses"),
            ("openai", "POST", "/v1/chat/completions"),
            ("openai", "POST", "/v1/audio/transcriptions"),
            ("openrouter", "POST", "/v1/chat/completions"),
            ("openrouter", "GET", "/v1/models/anthropic/claude-opus-5.5"),
        ] {
            assert!(
                allowed(provider, method, path),
                "{provider} {method} {path}"
            );
        }
        for (provider, method, path) in [
            ("anthropic", "POST", "/v1/messages/batches"),
            ("anthropic", "POST", "/v1/files"),
            ("anthropic", "GET", "/v1/messages"),
            ("anthropic", "POST", "/v1/messages/../files"),
            ("anthropic", "GET", "/v1/models/../files"),
            ("anthropic", "GET", "/v1/models/"),
            ("openai", "POST", "/v1/files"),
            ("openai", "POST", "/v1/audio/speech"),
            ("openrouter", "POST", "/v1/audio/transcriptions"),
            ("openai", "POST", "/v1/fine_tuning/jobs"),
            ("openai", "DELETE", "/v1/models/ft:x"),
            ("openrouter", "GET", "/v1/credits"),
            ("openrouter", "POST", "/v1/keys"),
            ("ollama", "POST", "/v1/chat/completions"),
        ] {
            assert!(
                !allowed(provider, method, path),
                "{provider} {method} {path}"
            );
        }
    }

    #[test]
    fn the_real_key_replaces_the_placeholder() {
        let head = b"POST /_link/model/v1/messages?beta=true HTTP/1.1\r\n\
            Host: 127.0.0.1:40781\r\nx-api-key: sk-ant-api03-link-wire\r\n\
            anthropic-version: 2023-06-01\r\nConnection: keep-alive\r\n\
            Content-Length: 2\r\n\r\n";
        let prepared = prepare(&key("anthropic"), &wired(head), b"{}", false).unwrap();
        assert_eq!(prepared.host, "api.anthropic.com");
        assert_eq!(
            sent(&prepared),
            "POST /v1/messages?beta=true HTTP/1.1\r\nHost: api.anthropic.com\r\n\
             anthropic-version: 2023-06-01\r\nAccept-Encoding: identity\r\n\
             x-api-key: sk-real-key\r\nContent-Length: 2\r\n\r\n{}"
        );
        let head = b"GET /_link/model/v1/models HTTP/1.1\r\n\
            Authorization: Bearer sk-or-v1-link-wire\r\n\r\n";
        let prepared = prepare(&key("openrouter"), &wired(head), b"", false).unwrap();
        assert_eq!(prepared.host, "openrouter.ai");
        assert_eq!(
            sent(&prepared),
            "GET /api/v1/models HTTP/1.1\r\nHost: openrouter.ai\r\n\
             Authorization: Bearer sk-real-key\r\n\r\n"
        );
    }

    #[test]
    fn requests_that_arent_inference_are_refused() {
        for head in [
            &b"POST /_link/model/v1/files HTTP/1.1\r\nContent-Length: 2\r\n\r\n"[..],
            b"POST /v1/messages HTTP/1.1\r\nContent-Length: 2\r\n\r\n",
            b"POST /_link/model/v1/messages HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"GET /_link/model/v1/models HTTP/1.1\r\nUpgrade: websocket\r\n\r\n",
            b"POST /_link/model/v1/messages HTTP/2\r\n\r\n",
            b"POST /_link/model/v1/messages HTTP/1.1\r\nA: b\r\n x-api-key: k\r\n\r\n",
            // A length hidden where a server that ends a line at a bare \n
            // would read it.
            b"POST /_link/model/v1/messages HTTP/1.1\r\nA: b\nContent-Length: 0\r\n\r\n",
            b"POST /_link/model/v1/messages HTTP/1.1\r\nA: b\0\r\n\r\n",
        ] {
            assert!(
                prepare(&key("anthropic"), &wired(head), b"{}", false).is_err(),
                "{}",
                String::from_utf8_lossy(head)
            );
        }
        let head = b"POST /_link/model/v1/messages HTTP/1.1\r\nContent-Length: 3\r\n\r\n";
        assert!(
            prepare(&key("anthropic"), &wired(head), b"a=b", false).is_err(),
            "not JSON"
        );
    }
}
