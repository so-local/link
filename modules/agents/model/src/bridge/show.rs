//! `GET /_link/model/show`, answered by the local model route itself: the
//! runtime is asked outside the sandbox in its own way, and its answer
//! mapped, field by field, to Link's one shape
//! ([`link_model_contract::show::Shown`]). Each runtime is one
//! mapping, tried in turn: Ollama's `POST /api/show` (with `/api/ps` for
//! the window it runs a model with), LM Studio's `GET
//! /api/v0/models/<id>`, llama.cpp's `GET /props`, then the
//! OpenAI-compatible list, `GET /v1/models`. An answer none maps is
//! refused, never passed on; so is one past [`ANSWER_MAX`], or a show
//! past [`SHOW_WITHIN`]. The list a sandbox asks for is the bridge's too
//! ([`list`]).

use std::time::Duration;

use anyhow::bail;
use link_model_contract::show::{self, Capability, Shown};
use serde_json::{Value, json};

/// The most of a runtime's answer read: past it, refused.
pub const ANSWER_MAX: usize = 1 << 20;

/// The longest a show takes, whatever it asks: within what Link Harness
/// waits for it (`limits::ASK_TIMEOUT`, three seconds).
pub const SHOW_WITHIN: Duration = Duration::from_millis(2_500);

/// `name`'s details from the runtime on this machine's `port`, in Link's
/// shape, within [`SHOW_WITHIN`].
pub async fn ask(port: u16, name: &str) -> anyhow::Result<Shown> {
    if !show::plain_name(name) {
        bail!("{name:?} isn't a model's name");
    }
    match tokio::time::timeout(SHOW_WITHIN, asked(port, name)).await {
        Ok(Some(shown)) => Ok(shown),
        Ok(None) => {
            bail!("the model runtime on port {port} says nothing of {name} that Link reads")
        }
        Err(_) => bail!("the model runtime on port {port} didn't say what {name} is in time"),
    }
}

async fn asked(port: u16, name: &str) -> Option<Shown> {
    let runtime = Runtime::new(port);
    if let Some(shown) = runtime.post("/api/show", json!({"model": name})).await {
        let running = runtime.get("/api/ps").await;
        if let Some(shown) = ollama(name, &shown, running.as_ref()) {
            return Some(shown);
        }
    }
    // LM Studio's ids are flat names.
    if !name.contains('/')
        && let Some(model) = runtime.get(&format!("/api/v0/models/{name}")).await
        && let Some(shown) = lm_studio(name, &model)
    {
        return Some(shown);
    }
    if let Some(props) = runtime.get("/props").await
        && let Some(shown) = llama_cpp(name, &props)
    {
        return Some(shown);
    }
    // vLLM serves the list alone, each model's window in its entry.
    let listed = runtime.get("/v1/models").await?;
    openai(name, &listed)
}

/// A runtime on this machine, asked as Link's bridge asks it.
struct Runtime {
    base: String,
    client: reqwest::Client,
}

impl Runtime {
    fn new(port: u16) -> Self {
        Self {
            base: format!("http://127.0.0.1:{port}"),
            client: client(),
        }
    }

    /// `GET path`'s answer, JSON, if it succeeded.
    async fn get(&self, path: &str) -> Option<Value> {
        read(self.client.get(format!("{}{path}", self.base))).await
    }

    /// `POST path` with `body`'s answer, JSON, if it succeeded.
    async fn post(&self, path: &str, body: Value) -> Option<Value> {
        let request = self
            .client
            .post(format!("{}{path}", self.base))
            .header("content-type", "application/json")
            .body(body.to_string());
        read(request).await
    }
}

/// `request`'s answer, JSON, if it succeeded and is at most
/// [`ANSWER_MAX`]: read a piece at a time, never whole first.
async fn read(request: reqwest::RequestBuilder) -> Option<Value> {
    let mut answer = request.send().await.ok()?;
    answer.status().is_success().then_some(())?;
    let mut body = Vec::new();
    while let Some(piece) = answer.chunk().await.ok()? {
        if body.len() + piece.len() > ANSWER_MAX {
            return None;
        }
        body.extend_from_slice(&piece);
    }
    serde_json::from_slice(&body).ok()
}

/// `GET /v1/models`, answered by the bridge: the runtime's list, each
/// model by its id alone and only an id that's a plain name, never the
/// runtime's own answer (llama.cpp's id can be its file's path, vLLM's
/// `root` is where its weights are).
pub async fn list(port: u16) -> anyhow::Result<Value> {
    let listed = tokio::time::timeout(SHOW_WITHIN, Runtime::new(port).get("/v1/models"))
        .await
        .ok()
        .flatten();
    let Some(listed) = listed else {
        bail!("the model runtime on port {port} listed no models Link reads");
    };
    Ok(listed_plainly(&listed))
}

/// Only this machine's runtimes are asked: never through a proxy, never
/// redirected, and soon given up on.
fn client() -> reqwest::Client {
    static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
        link_fetch::install_crypto_provider();
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            // Every destination is loopback HTTP. Reuse the transport,
            // including its connection pool, while reading fresh metadata.
            .tls_certs_only(std::iter::empty())
            .connect_timeout(Duration::from_millis(500))
            .timeout(SHOW_WITHIN)
            .build()
            .expect("static reqwest config is valid")
    });
    CLIENT.clone()
}

/// An OpenAI-style list rebuilt from plain ids alone.
fn listed_plainly(listed: &Value) -> Value {
    let data: Vec<Value> = listed["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str())
        .filter(|id| show::plain_name(id))
        .map(|id| json!({"id": id, "object": "model"}))
        .collect();
    json!({"object": "list", "data": data})
}

/// Ollama's `/api/show` answer, with `/api/ps`'s when it gave one: its
/// details, the window it runs the model with (loaded, else its settings'
/// `num_ctx`) and the window it was trained for.
fn ollama(name: &str, shown: &Value, running: Option<&Value>) -> Option<Shown> {
    let details = shown
        .get("details")
        .filter(|d| d.as_object().is_some_and(|d| !d.is_empty()))?;
    let loaded = running
        .and_then(|ps| ps["models"].as_array())
        .and_then(|models| {
            models
                .iter()
                .find(|m| m["name"] == name || m["model"] == name)
        })
        .and_then(|m| show::tokens(&m["context_length"]));
    let set = shown["parameters"].as_str().and_then(|parameters| {
        parameters.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("num_ctx")).then(|| words.next()?.parse::<u32>().ok())?
        })
    });
    let trained = shown["model_info"].as_object().and_then(|info| {
        info.iter()
            .find(|(key, _)| key.ends_with(".context_length"))
            .and_then(|(_, value)| show::tokens(value))
    });
    Some(Shown {
        facts: None,
        name: name.into(),
        family: show::family(&details["family"]),
        parameters: show::parameters(&details["parameter_size"]),
        quantization: show::quantization(&details["quantization_level"]),
        context: loaded.or(set),
        trained,
        capabilities: capabilities(shown["capabilities"].as_array()),
        endpoint: None,
    })
}

/// LM Studio's `/api/v0/models/<id>` answer, for the model asked.
fn lm_studio(name: &str, model: &Value) -> Option<Shown> {
    if model["id"] != name || model["max_context_length"].is_null() {
        return None;
    }
    let mut capabilities = capabilities(model["capabilities"].as_array());
    capabilities.extend(model["type"].as_str().and_then(Capability::of));
    capabilities.sort();
    capabilities.dedup();
    Some(Shown {
        facts: None,
        name: name.into(),
        family: show::family(&model["arch"]),
        parameters: None,
        quantization: show::quantization(&model["quantization"]),
        context: show::tokens(&model["loaded_context_length"]),
        trained: show::tokens(&model["max_context_length"]),
        capabilities,
        endpoint: None,
    })
}

/// llama.cpp's `/props` answer: the window it runs its one model with.
fn llama_cpp(name: &str, props: &Value) -> Option<Shown> {
    // Its one model, by its alias, else its file's name without `.gguf`.
    let file = props["model_path"]
        .as_str()
        .and_then(|path| path.rsplit('/').next())
        .map(|file| file.trim_end_matches(".gguf"));
    if props["model_alias"].as_str() != Some(name) && file != Some(name) {
        return None;
    }
    let context = show::tokens(&props["default_generation_settings"]["n_ctx"])
        .or_else(|| show::tokens(&props["n_ctx"]))?;
    Some(Shown {
        facts: None,
        name: name.into(),
        context: Some(context),
        ..Shown::default()
    })
}

/// An OpenAI-compatible `/v1/models` list's entry for the model asked:
/// vLLM's window (`max_model_len`) when it says.
fn openai(name: &str, listed: &Value) -> Option<Shown> {
    let model = listed["data"]
        .as_array()?
        .iter()
        .find(|model| model["object"] == "model" && model["id"] == name)?;
    Some(Shown {
        facts: None,
        name: name.into(),
        context: show::tokens(&model["max_model_len"]),
        ..Shown::default()
    })
}

/// The capabilities a runtime lists that Link knows, each once.
fn capabilities(listed: Option<&Vec<Value>>) -> Vec<Capability> {
    let mut known: Vec<Capability> = listed
        .into_iter()
        .flatten()
        .filter_map(|word| Capability::of(word.as_str()?))
        .collect();
    known.sort();
    known.dedup();
    known
}

/// Minimal native read compatibility for outside harnesses. No runtime metadata is relayed.
pub async fn native(port: u16, path: &str) -> anyhow::Result<Value> {
    use anyhow::ensure;
    use link_format::WithinExt;
    let runtime = Runtime::new(port);
    match path {
        "/api/tags" => {
            let listed = list(port).await?;
            let models: Vec<_> = listed["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry["id"].as_str())
                .map(|name| json!({"name":name,"model":name}))
                .collect();
            Ok(json!({"models":models}))
        }
        "/api/ps" => {
            let listed = runtime.get(path).await.within("runtime is unavailable")?;
            let models: Vec<_> = listed["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry["name"].as_str())
                .filter(|name| show::plain_name(name))
                .map(|name| json!({"name":name,"model":name}))
                .collect();
            Ok(json!({"models":models}))
        }
        "/api/version" => {
            let listed = runtime.get(path).await.within("runtime is unavailable")?;
            let version = listed["version"].as_str().within("no runtime version")?;
            ensure!(
                version.len() <= 32
                    && version.split('.').count() == 3
                    && version
                        .split('.')
                        .all(|part| !part.is_empty()
                            && part.bytes().all(|byte| byte.is_ascii_digit())),
                "unknown runtime version"
            );
            Ok(json!({"version":version}))
        }
        "/" | "/health" => {
            list(port).await?;
            Ok(json!({"ready":true}))
        }
        _ => anyhow::bail!("unknown native read"),
    }
}

#[cfg(test)]
mod tests;
