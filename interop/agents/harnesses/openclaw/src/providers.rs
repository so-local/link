//! The model's providers: the one the model is on, the ones Link set
//! and unsets, and Ollama's and a hosted model's entries.

use super::*;

/// Each provider entry Link manages, and what it is for `model`: a
/// local Ollama model registered with its address, so OpenClaw knows it
/// without live discovery (doctor checks its tools then); a hosted model
/// with an address (Link's proxy, which adds its key) pointed there,
/// speaking its own API. None: removed.
pub(super) fn providers(model: &harness::Model) -> Vec<(&'static str, Option<serde_json::Value>)> {
    ["ollama", "anthropic", "openai", "openrouter"]
        .into_iter()
        .map(|name| {
            let entry = match name {
                _ if model.provider != name => None,
                "ollama" => ollama_provider(model),
                _ => hosted_provider(model),
            };
            (name, entry)
        })
        .collect()
}

/// Where `configure` keeps the providers whose entries it set, beside
/// OpenClaw's state, to unset only those when they're no longer the
/// model's.
pub(super) const PROVIDERS_SET: &str = "link-providers.json";

/// The providers whose entries Link set, in [`PROVIDERS_SET`].
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Set {
    providers: Vec<String>,
}

link_files::specification!(Set, include_str!("providers.schema.toml"));

/// The providers set, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Set>()
}

/// Of `providers` without an entry now, those whose entries Link may
/// have set before, as `record` lists them: without a record (an install
/// configured before there was one), all of them. Until
/// [`record_providers`] says what this configure left, `record` lists
/// those and the ones with an entry now, so a configure that stops
/// halfway forgets none.
pub(super) fn providers_gone(
    record: &Path,
    providers: &[(&'static str, Option<serde_json::Value>)],
) -> anyhow::Result<Vec<&'static str>> {
    let before: Option<Vec<String>> = std::fs::read(record)
        .ok()
        .and_then(|bytes| link_files::saved::parse_json::<Set>(&bytes).ok())
        .map(|set| set.providers);
    let gone: Vec<&'static str> = providers
        .iter()
        .filter(|(_, entry)| entry.is_none())
        .map(|(name, _)| *name)
        .filter(|name| {
            before
                .as_ref()
                .is_none_or(|before| before.iter().any(|b| b == name))
        })
        .collect();
    record_providers(record, providers, &gone)?;
    Ok(gone)
}

/// Records the providers with an entry now, and `kept`, those still to
/// unset.
pub(super) fn record_providers(
    record: &Path,
    providers: &[(&'static str, Option<serde_json::Value>)],
    kept: &[&str],
) -> anyhow::Result<()> {
    let set = Set {
        providers: providers
            .iter()
            .filter(|(name, entry)| entry.is_some() || kept.contains(name))
            .map(|(name, _)| name.to_string())
            .collect(),
    };
    Ok(link_files::write_private(
        record,
        &link_files::saved::print_json(&set)?,
    )?)
}

/// The variable in OpenClaw's `.env` holding the model's address
/// (`Model.base_url`), which a model on this machine reaches through
/// Link's proxy with the proxy's secret in it.
pub(super) const MODEL_URL: &str = "LNK_OPENCLAW_MODEL_URL";

/// The model's address in a provider's entry: the variable holding it,
/// which OpenClaw reads (`${VAR}` in a config value), so the address
/// never is on the command line of `openclaw config set`.
const MODEL_URL_VAR: &str = "${LNK_OPENCLAW_MODEL_URL}";

/// The provider entry for a local Ollama model.
fn ollama_provider(model: &harness::Model) -> Option<serde_json::Value> {
    model.base_url.as_ref()?;
    let name = model.name.as_deref()?;
    Some(json!({
        "baseUrl": MODEL_URL_VAR,
        "apiKey": "ollama-local",
        "api": "ollama",
        // The same window on both sides: OpenClaw budgets its prompt to
        // it, and asks Ollama for it (`num_ctx`), whose own default (a few
        // thousand tokens) is smaller than OpenClaw's instructions alone.
        "models": [{
            "id": name,
            "name": name,
            "contextWindow": OLLAMA_CONTEXT,
            "params": {"num_ctx": OLLAMA_CONTEXT},
        }],
    }))
}

/// The context window a local Ollama model runs with: room for OpenClaw's
/// instructions and a conversation, in what a Mac with 16 GB can hold.
const OLLAMA_CONTEXT: u32 = 32_768;

/// OpenClaw names models `<provider>/<model>` (`ollama/qwen3:8b`).
/// A hosted model's provider at another address (Link's proxy): the
/// address, and the API to speak there. OpenClaw takes a provider with a
/// `baseUrl` but no `api` for an OpenAI-compatible server, so Anthropic's
/// requests would go to `/chat/completions`, which the proxy refuses.
fn hosted_provider(model: &harness::Model) -> Option<serde_json::Value> {
    model.base_url.as_ref().filter(|_| !model.is_local())?;
    let api = match model.provider.as_str() {
        "anthropic" => "anthropic-messages",
        _ => "openai-completions",
    };
    Some(json!({"baseUrl": MODEL_URL_VAR, "api": api}))
}

pub(super) fn model_id(model: &harness::Model) -> Option<String> {
    Some(format!("{}/{}", model.provider, model.name.as_deref()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids() {
        let local = harness::Model {
            provider: "ollama".into(),
            name: Some("qwen3:8b".into()),
            base_url: Some("http://localhost:11434".into()),
            ..Default::default()
        };
        assert_eq!(model_id(&local).as_deref(), Some("ollama/qwen3:8b"));
        assert_eq!(
            ollama_provider(&local).unwrap()["models"],
            json!([{
                "id": "qwen3:8b",
                "name": "qwen3:8b",
                "contextWindow": 32768,
                "params": {"num_ctx": 32768},
            }])
        );
        let hosted = harness::Model {
            provider: "openai".into(),
            ..Default::default()
        };
        assert_eq!(model_id(&hosted), None);
        assert_eq!(hosted_provider(&hosted), None);
    }

    #[test]
    fn only_the_models_own_provider_is_kept() {
        let set = |model: &harness::Model| -> Vec<&str> {
            providers(model)
                .into_iter()
                .filter_map(|(name, entry)| entry.map(|_| name))
                .collect()
        };
        // Switched from Ollama to OpenAI's default with lan (no proxy):
        // Ollama's entry goes, and none is set.
        let hosted = harness::Model {
            provider: "openai".into(),
            ..Default::default()
        };
        assert!(set(&hosted).is_empty());
        assert_eq!(providers(&hosted).len(), 4);
        let local = harness::Model {
            provider: "ollama".into(),
            name: Some("qwen3:8b".into()),
            base_url: Some("http://localhost:11434".into()),
            ..Default::default()
        };
        assert_eq!(set(&local), ["ollama"]);
        let on_wire = harness::Model {
            provider: "anthropic".into(),
            base_url: Some("http://127.0.0.1:40781/_link/model".into()),
            ..Default::default()
        };
        assert_eq!(set(&on_wire), ["anthropic"]);
    }

    /// Only the providers Link set before, and not now, are unset: none
    /// at a configure that changes nothing, where it was three.
    #[test]
    fn only_the_providers_link_set_are_unset() {
        let dir =
            std::env::temp_dir().join(format!("lnk-openclaw-providers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let record = dir.join(PROVIDERS_SET);
        // Each unset as it goes.
        let gone = |model: &harness::Model| {
            let providers = providers(model);
            let gone = providers_gone(&record, &providers).unwrap();
            record_providers(&record, &providers, &[]).unwrap();
            gone
        };
        let local = harness::Model {
            provider: "ollama".into(),
            name: Some("qwen3:8b".into()),
            base_url: Some("http://localhost:11434".into()),
            ..Default::default()
        };
        let on_wire = harness::Model {
            provider: "anthropic".into(),
            base_url: Some("http://127.0.0.1:40781/_link/model".into()),
            ..Default::default()
        };
        let hosted = harness::Model {
            provider: "openai".into(),
            ..Default::default()
        };
        // No record yet (configured before there was one): all the others.
        assert_eq!(gone(&local), ["anthropic", "openai", "openrouter"]);
        // The same again: nothing to unset.
        assert!(gone(&local).is_empty());
        // Another provider: the one Link set before goes.
        assert_eq!(gone(&on_wire), ["ollama"]);
        assert!(gone(&on_wire).is_empty());
        // A model with no entry of its own: the last one goes, then none.
        assert_eq!(gone(&hosted), ["anthropic"]);
        assert!(gone(&hosted).is_empty());
        assert!(gone(&local).is_empty());
        // A configure that stops before it's done, or an unset that
        // fails, leaves it to the next.
        providers_gone(&record, &providers(&on_wire)).unwrap();
        assert_eq!(gone(&hosted), ["ollama", "anthropic"]);
        assert!(
            providers_gone(&record, &providers(&local))
                .unwrap()
                .is_empty()
        );
        // Ollama's unset failed: it's kept for the next.
        record_providers(&record, &providers(&hosted), &["ollama"]).unwrap();
        assert_eq!(gone(&hosted), ["ollama"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hosted_model_on_the_wire_speaks_its_own_api() {
        let on_wire = |provider: &str, base: &str| harness::Model {
            provider: provider.into(),
            api_key: "sk-ant-api03-link-wire".to_string().into(),
            base_url: Some(base.into()),
            ..Default::default()
        };
        let base = "http://127.0.0.1:40781/_link/model";
        assert_eq!(
            hosted_provider(&on_wire("anthropic", base)).unwrap(),
            json!({"baseUrl": MODEL_URL_VAR, "api": "anthropic-messages"})
        );
        for provider in ["openai", "openrouter"] {
            let v1 = format!("{base}/v1");
            assert_eq!(
                hosted_provider(&on_wire(provider, &v1)).unwrap(),
                json!({"baseUrl": MODEL_URL_VAR, "api": "openai-completions"})
            );
        }
    }
}
