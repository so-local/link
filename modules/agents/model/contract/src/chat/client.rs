//! Model protocol client and provider mappings shared by the bridge and standalone callers.
use super::*;
use anyhow::bail;
use link_format::WithinExt;
use serde_json::json;
use std::time::Instant;
use tokio::sync::mpsc;
mod bodies;
mod reply;
use super::lines::SseLines;
pub use bodies::{anthropic_body, anthropic_url, models_url, openai_base, openai_body, openai_url};
pub use reply::Reply;
pub mod canonical;
pub mod heard;
/// An endpoint supplied by the caller's grant, never by a model/chat request.
#[derive(Debug, Default)]
pub struct Model {
    pub provider: String,
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub api_key: link_secretive::Secret,
}
/// The model wasn't reached: its runtime refused the connection, or
/// answered nothing within the time it was given. Nothing of an answer
/// went out.
#[derive(Debug)]
pub struct Unreached(pub String);

impl std::fmt::Display for Unreached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "reaching the model: {}", self.0)
    }
}

impl std::error::Error for Unreached {}

/// `err` and what caused it, each after a colon: the HTTP client's own
/// words are only "error sending request", the same for a refused
/// connection, one closed before its answer, or a timeout.
pub fn with_causes(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut cause = err.source();
    while let Some(err) = cause {
        let more = err.to_string();
        if !text.ends_with(&more) {
            text = format!("{text}: {more}");
        }
        cause = err.source();
    }
    text
}

/// The model refused the prompt as longer than its window.
#[derive(Debug)]
pub struct TooLong(pub String);

impl std::fmt::Display for TooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TooLong {}

/// The model's provider answered busy or failing, HTTP 429 or a 5xx
/// (Anthropic's 529, overloaded, among them), before anything of an
/// answer went out.
#[derive(Debug)]
pub struct Overloaded(pub String);

impl std::fmt::Display for Overloaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Overloaded {}

/// Asks `model` to answer `prompt`, sending each piece of its text to
/// `pieces` as it comes, the first after a blank line when `separate`
/// (text went out before it this turn); the answer, as an event to keep
/// (its `seq` for the caller to set, and its calls' ids when the
/// provider gave none).
pub async fn answer(
    client: &reqwest::Client,
    model: &Model,
    prompt: &Prompt<'_>,
    pieces: &mpsc::Sender<String>,
    separate: bool,
    cancelled: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<Answered> {
    answer_within(client, model, prompt, pieces, separate, cancelled, None).await
}

/// [`answer`], [`Unreached`] when the model's runtime refuses the
/// connection or, with `first`, sends nothing back within it; [`TooLong`]
/// when it refuses the prompt as past its window; [`Overloaded`] when its
/// provider answers 429 or a 5xx.
pub async fn answer_within(
    client: &reqwest::Client,
    model: &Model,
    prompt: &Prompt<'_>,
    pieces: &mpsc::Sender<String>,
    mut separate: bool,
    mut cancelled: tokio::sync::watch::Receiver<bool>,
    first: Option<std::time::Duration>,
) -> anyhow::Result<Answered> {
    let Some(name) = model.name.clone() else {
        bail!(
            "no model named for {} (lnk agent start --model.default)",
            model.provider
        );
    };
    let anthropic = model.provider == "anthropic";
    let (url, body) = if anthropic {
        (anthropic_url(model), anthropic_body(&name, prompt))
    } else {
        (
            openai_url(model)?,
            openai_body(&name, &model.provider, prompt),
        )
    };
    let mut request = client.post(&url).header("content-type", "application/json");
    if anthropic && body.get("speed").is_some() {
        request = request.header("anthropic-beta", super::FAST_BETA);
    }
    let request = authorized(request.body(serde_json::to_vec(&body)?), model);
    let started = Instant::now();
    let sent = match first {
        Some(first) => tokio::time::timeout(first, request.send())
            .await
            .map_err(|_| Unreached(format!("nothing back within {}s", first.as_secs())))?,
        None => request.send().await,
    };
    let mut response = sent.map_err(|err| {
        let unreached = err.is_connect() || err.is_timeout();
        let why = with_causes(&err.without_url());
        match unreached {
            true => anyhow::Error::new(Unreached(why)),
            false => anyhow::anyhow!("reaching the model: {why}"),
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        let body = bounded(&mut response).await.unwrap_or_default();
        let why = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_default();
        if too_long(status.as_u16(), &why) {
            return Err(TooLong(format!(
                "the model refused the prompt as longer than its window ({}): the agent \
                 sends it {} tokens. Set a smaller [window] max in the agent's spec, or the \
                 model's window in ~/.lnk/safe/models/facts.toml",
                link_format::clean(&why),
                prompt.window
            ))
            .into());
        }
        let said = format!("the model answered {status} {}", link_format::clean(&why));
        if status.as_u16() == 429 || status.is_server_error() {
            return Err(Overloaded(said).into());
        }
        bail!(said);
    }
    let mut send = async |piece: String| {
        let piece = match std::mem::take(&mut separate) {
            true => format!("\n\n{piece}"),
            false => piece,
        };
        let _ = pieces.send(piece).await;
    };
    let mut reply = Reply::default();
    let mut interrupted = false;
    let streamed = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    if streamed {
        let mut lines = SseLines::default();
        let mut received = 0;
        loop {
            // Asked to end: what it wrote so far is kept.
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk.within("reading the model's answer")?,
                _ = asked_to_end(&mut cancelled) => {
                    interrupted = true;
                    break;
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            received += chunk.len();
            anyhow::ensure!(received <= 8 << 20, "model answer exceeds byte budget");
            for data in lines.push(&chunk) {
                if data == "[DONE]" {
                    continue;
                }
                let Ok(event) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                if let Some(piece) = reply.take(&event, anthropic)? {
                    send(piece).await;
                }
            }
        }
    } else {
        let body: Value = serde_json::from_slice(&bounded(&mut response).await?)
            .within("reading the model's answer")?;
        reply.whole(&body, anthropic);
        if !reply.text.is_empty() {
            send(reply.text.clone()).await;
        }
    }
    let calls = reply.calls();
    let thinking = reply.thinking.into_values().collect();
    // The provider's, else its tokens as the provider counted them at the
    // model's price.
    let cost = reply.cost.or_else(|| {
        prompt
            .price
            .map(|price| crate::price_at(price, prompt.above, reply.tokens.input))
            .map(|price| price.of(reply.tokens.input, reply.tokens.cached, reply.tokens.output))
    });
    Ok(Answered {
        text: reply.text,
        calls,
        model: Some(reply.model.unwrap_or(name)),
        response_id: reply.id,
        tokens: reply.tokens,
        latency_ms: started.elapsed().as_millis() as u64,
        cost,
        thinking,
        interrupted,
    })
}

/// Whether a provider's refusal, `status` and its message `why`, says
/// the prompt is longer than the model's window: Anthropic's "prompt is
/// too long", OpenAI's and OpenRouter's "context length", a runtime's
/// "context window".
/// The models `model`'s provider offers, by their ids as it names them:
/// what `/model` switches between ([`crate::commands`]).
pub async fn listed(client: &reqwest::Client, model: &Model) -> anyhow::Result<Vec<String>> {
    let request = authorized(
        client
            .get(models_url(model)?)
            .timeout(std::time::Duration::from_secs(10)),
        model,
    );
    let mut response = request
        .send()
        .await
        .map_err(|err| anyhow::anyhow!(with_causes(&err.without_url())))?;
    let status = response.status();
    anyhow::ensure!(status.is_success(), "it answered {status}");
    let listed: Value =
        serde_json::from_slice(&bounded(&mut response).await?).within("its list isn't JSON")?;
    let mut ids: Vec<String> = listed["data"]
        .as_array()
        .within("its list has no data")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// The client for a request that carries a real key, outside a sandbox's
/// proxy: over HTTPS alone, and never after a redirect, which would carry
/// a key in a header other than `Authorization` (Anthropic's `x-api-key`)
/// on to whatever host the answer names.
pub fn keyed() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// `request` with `model`'s key, as its provider takes it: Anthropic's
/// header, else a bearer's, where it has one.
pub fn authorized(request: reqwest::RequestBuilder, model: &Model) -> reqwest::RequestBuilder {
    if model.provider == "anthropic" {
        request
            .header("x-api-key", model.api_key.expose())
            .header("anthropic-version", "2023-06-01")
    } else if !model.api_key.is_empty() {
        request.bearer_auth(model.api_key.expose())
    } else {
        request
    }
}

pub fn too_long(status: u16, why: &str) -> bool {
    let why = why.to_ascii_lowercase();
    matches!(status, 400 | 413 | 422)
        && [
            "prompt is too long",
            "context length",
            "context_length",
            "context window",
            "maximum context",
            "too many tokens",
        ]
        .iter()
        .any(|said| why.contains(said))
}

/// Waits until the turn is asked to end; for ever if it never is.
pub async fn asked_to_end(cancelled: &mut tokio::sync::watch::Receiver<bool>) {
    if cancelled
        .wait_for(|asked| *asked)
        .await
        .map(|_| ())
        .is_err()
    {
        std::future::pending::<()>().await;
    }
}

/// Read a bounded provider answer before parsing it.
pub async fn bounded(response: &mut reqwest::Response) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(piece) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + piece.len() <= 8 << 20,
            "provider answer exceeds byte budget"
        );
        bytes.extend_from_slice(&piece);
    }
    Ok(bytes)
}

#[cfg(test)]
mod keyed_tests {
    /// A real key goes over HTTPS alone: a plain address is refused
    /// before anything is sent.
    #[tokio::test]
    async fn a_keyed_client_refuses_plain_http() {
        link_fetch::install_crypto_provider();
        let refused = super::keyed()
            .unwrap()
            .get("http://127.0.0.1:9/v1/models")
            .header("x-api-key", "sk-ant-not-sent")
            .send()
            .await
            .unwrap_err();
        assert!(refused.is_builder(), "{refused}");
    }
}
