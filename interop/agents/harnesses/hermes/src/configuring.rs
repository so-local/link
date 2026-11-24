//! Configuring Hermes: its `.env`, with none of its own channels (Link's
//! bridge carries them to its ACP, through the hub), its MCP servers, its
//! ACP's toolset, its model's provider and the config it sets; and the
//! hub's key, in a file of Link's beside it.

use super::*;

/// The settings consumed by Hermes's structural configuration. Rotating
/// credentials and proxy addresses are refreshed in its env files and do
/// not require starting the Python CLI once per config key.
pub(super) fn read_by_configure(input: &str) -> anyhow::Result<Vec<u8>> {
    let mut settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(input).within("reading settings on stdin")?;
    let window = match settings.get("model_facts").filter(|value| !value.is_null()) {
        Some(value) => {
            let facts =
                serde_json::from_value(value.clone()).within("reading known model facts")?;
            let model = serde_json::from_value(settings.get("model").cloned().unwrap_or_default())
                .within("reading the selected model")?;
            context_length(&model, Some(&facts))
        }
        None => None,
    };
    for key in [
        "actions",
        "spec",
        "identity",
        "remembered",
        "defaults",
        "model_facts",
        "environment",
        "fallback",
        "fallback_facts",
        "tools",
        "memory",
    ] {
        settings.remove(key);
    }
    settings.remove("endpoint_key");
    settings.insert("model_context_length".into(), serde_json::to_value(window)?);
    if let Some(model) = settings.get_mut("model").and_then(|m| m.as_object_mut()) {
        model.remove("api_key");
        if model.get("base_url").is_some_and(|url| !url.is_null()) {
            model.insert("base_url".into(), true.into());
        }
    }
    Ok(serde_json::to_vec(&settings)?)
}

/// Refreshes credentials and addresses Hermes reads from env files.
pub(super) fn configure_environment(settings: &Settings) -> anyhow::Result<()> {
    let env_file = hermes_home().join(".env");
    // Link's key for this start, which its bridge sends: else the one
    // kept, else a new one. The hub's, never Hermes's to read.
    let endpoint_key = match (
        &settings.endpoint_key,
        harness::read_env_file(&link_env(), ENDPOINT_KEY),
    ) {
        (Some(key), _) => key.expose_owned(),
        (None, Some(key)) if !key.is_empty() => key,
        _ => harness::random_token()?,
    };
    harness::merge_env_file(&link_env(), &[(ENDPOINT_KEY, Some(endpoint_key.as_str()))])?;
    // OpenAI's address as Hermes reads it for `openai-api`: Link's proxy,
    // which adds the key (without it, OpenAI's own).
    let openai_base = (settings.model.provider == "openai")
        .then(|| settings.model.base_url.clone())
        .flatten();
    let model_url = model_url(&settings.model);
    let mut secrets = vec![
        (MODEL_URL, model_url.as_deref()),
        ("ANTHROPIC_API_KEY", None),
        ("OPENAI_API_KEY", None),
        ("OPENAI_BASE_URL", openai_base.as_deref()),
        ("OPENROUTER_API_KEY", None),
    ];
    // Link's bridge carries every connection: none of Hermes's own is
    // left on, so no two read one bot.
    secrets.extend(CHANNEL_KEYS.iter().map(|key| (*key, None)));
    for entry in &mut secrets {
        if Some(entry.0) == settings.model.key_env() {
            entry.1 = Some(settings.model.api_key.expose());
        }
    }
    harness::merge_env_file(&env_file, &secrets)?;
    Ok(())
}

pub(super) fn configure(settings: &Settings) -> anyhow::Result<()> {
    for (key, value) in config(settings) {
        match value {
            Some(value) => harness::run_quiet(hermes()?.args(["config", "set", key, &value]))?,
            // Left to Hermes: its default for the provider.
            None => {
                let _ = hermes()?
                    .args(["config", "unset", key])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
    }
    acp_toolsets(&hermes_home().join("config.yaml"))
}

/// Tool addresses rotate with the proxy. Refresh them without running
/// the structural Python CLI again, including added and removed tools.
pub(super) fn configure_endpoints(settings: &Settings) -> anyhow::Result<()> {
    // Its tools outside the sandbox, each an MCP server over HTTP on its
    // proxy; those Link added before and gone since, removed.
    let servers = settings.mcp_servers();
    let gone = harness::tools_gone(&hermes_home().join("link-tools.json"), &servers)?;
    mcp_servers(&hermes_home().join("config.yaml"), &servers, &gone)
}

/// The tools `hermes acp` runs, in its `config.yaml` at `path`: the
/// gateway agent's (`hermes-api-server`), the tools it had before Link
/// reached it over ACP, in place of the editor's (`hermes-acp`), its
/// default. Its MCP servers come with them. Its own cron off
/// (`agent.disabled_toolsets`): Link's scheduler fires its schedules,
/// and Hermes's ticks only in a gateway Link doesn't run.
fn acp_toolsets(path: &std::path::Path) -> anyhow::Result<()> {
    use serde_norway::{Mapping, Value};
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut config: Value = match text.trim() {
        "" => Mapping::new().into(),
        _ => serde_norway::from_str(&text).within(format!("reading {}", path.display()))?,
    };
    let root = config
        .as_mapping_mut()
        .within(format!("reading {} as a mapping", path.display()))?;
    let key = Value::from("platform_toolsets");
    if !root.get(&key).is_some_and(Value::is_mapping) {
        root.insert(key.clone(), Mapping::new().into());
    }
    let platforms = root
        .get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .expect("made a mapping above");
    platforms.insert("acp".into(), vec![Value::from(ACP_TOOLSET)].into());
    let key = Value::from("agent");
    if !root.get(&key).is_some_and(Value::is_mapping) {
        root.insert(key.clone(), Mapping::new().into());
    }
    let agent = root
        .get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .expect("made a mapping above");
    let key = Value::from("disabled_toolsets");
    if !agent.get(&key).is_some_and(Value::is_sequence) {
        agent.insert(key.clone(), Vec::<Value>::new().into());
    }
    let disabled = agent
        .get_mut(&key)
        .and_then(Value::as_sequence_mut)
        .expect("made a sequence above");
    if !disabled.contains(&Value::from(CRON_TOOLSET)) {
        disabled.push(CRON_TOOLSET.into());
    }
    link_files::write_private(path, serde_norway::to_string(&config)?.as_bytes())?;
    Ok(())
}

/// Hermes's own cron's toolset, off: Link's scheduler is the agent's.
const CRON_TOOLSET: &str = "cronjob";

/// The toolset `hermes acp` runs: the gateway's API agent's.
const ACP_TOOLSET: &str = "hermes-api-server";

/// The hub's key, in [`link_env`].
pub(super) const ENDPOINT_KEY: &str = "ENDPOINT_KEY";

/// Where Link keeps the hub's key: beside Hermes's home, never in its
/// `.env`, which Hermes reads into its own environment.
pub(super) fn link_env() -> PathBuf {
    link_env_in(&hermes_home())
}

pub(super) fn link_env_in(hermes: &std::path::Path) -> PathBuf {
    hermes.join("link.env")
}

/// Each server as Hermes's MCP server in its `config.yaml` at `path`, by
/// its address (a URL makes it one over HTTP), and those `gone` taken
/// out; the rest of it as it was. Written here, never with `hermes config
/// set`: an address holds the proxy's secret, and a command's arguments
/// are any user's to see. The file is this user's alone.
fn mcp_servers(
    path: &std::path::Path,
    servers: &std::collections::BTreeMap<String, String>,
    gone: &[String],
) -> anyhow::Result<()> {
    use serde_norway::{Mapping, Value};
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut config: Value = match text.trim() {
        "" => Mapping::new().into(),
        _ => serde_norway::from_str(&text).within(format!("reading {}", path.display()))?,
    };
    let root = config
        .as_mapping_mut()
        .within(format!("reading {} as a mapping", path.display()))?;
    let key = Value::from("mcp_servers");
    if !root.get(&key).is_some_and(Value::is_mapping) {
        root.insert(key.clone(), Mapping::new().into());
    }
    let mcp = root
        .get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .expect("made a mapping above");
    for name in gone {
        mcp.remove(name.as_str());
    }
    for (name, url) in servers {
        let mut server = mcp
            .get(name.as_str())
            .and_then(Value::as_mapping)
            .cloned()
            .unwrap_or_default();
        server.insert("url".into(), url.as_str().into());
        mcp.insert(name.as_str().into(), server.into());
    }
    if mcp.is_empty() {
        root.remove(&key);
    }
    link_files::write_private(path, serde_norway::to_string(&config)?.as_bytes())?;
    Ok(())
}

/// The `.env` keys of Hermes's own channels: removed at every start, as
/// Link's bridge carries every connection to its ACP, so none is kept on
/// (WhatsApp's among them).
const CHANNEL_KEYS: &[&str] = &[
    "TELEGRAM_BOT_TOKEN",
    "TELEGRAM_ALLOWED_USERS",
    "TELEGRAM_REQUIRE_MENTION",
    "TELEGRAM_HOME_CHANNEL",
    "SLACK_BOT_TOKEN",
    "SLACK_APP_TOKEN",
    "SLACK_ALLOWED_USERS",
    "SLACK_REQUIRE_MENTION",
    "SLACK_HOME_CHANNEL",
    "DISCORD_BOT_TOKEN",
    "DISCORD_ALLOWED_USERS",
    "DISCORD_REQUIRE_MENTION",
    "DISCORD_HOME_CHANNEL",
    "SIGNAL_ACCOUNT",
    "LNK_SIGNAL_CLI",
    "SIGNAL_HTTP_URL",
    "SIGNAL_ALLOWED_USERS",
    "SIGNAL_GROUP_ALLOWED_USERS",
    "SIGNAL_REQUIRE_MENTION",
    "SIGNAL_HOME_CHANNEL",
    "WHATSAPP_ENABLED",
    "WHATSAPP_MODE",
    "WHATSAPP_ALLOWED_USERS",
    "WHATSAPP_GROUP_POLICY",
    "WHATSAPP_GROUP_ALLOWED_USERS",
    "WHATSAPP_REQUIRE_MENTION",
    "WHATSAPP_HOME_CHANNEL",
];

/// Hermes's name for a provider: OpenAI's API is `openai-api`, since a
/// bare `openai` is its old name for OpenRouter.
fn hermes_provider(provider: &str) -> String {
    match provider {
        "openai" => "openai-api".into(),
        other => other.into(),
    }
}

/// The variable in Hermes's `.env` holding the model's address
/// ([`model_url`]), which a model on this machine reaches through
/// Link's proxy with the proxy's secret in it.
const MODEL_URL: &str = "LNK_HERMES_MODEL_URL";

/// The model's address in `config.yaml`: the variable holding it, which
/// Hermes reads (`${VAR}` in a config value), so the address is never on
/// the command line of `hermes config set`.
const MODEL_URL_VAR: &str = "${LNK_HERMES_MODEL_URL}";

/// Where Hermes reaches the model, when not at its provider's own
/// address: a model running here as a custom OpenAI-compatible endpoint
/// (with `/v1`), a hosted one at Link's proxy, which adds its key.
fn model_url(model: &harness::Model) -> Option<String> {
    let base = model.base_url.as_deref()?;
    Some(match model.is_local() {
        true => format!("{}/v1", base.trim_end_matches('/')),
        false => base.to_string(),
    })
}

/// A relay URL hides the provider's hostname from native discovery. Supply
/// its known window without probing unrelated local-model or registry APIs.
fn context_length(
    model: &harness::Model,
    facts: Option<&link_harness_contract::models::Known>,
) -> Option<u32> {
    if model.is_local() {
        return None;
    }
    let facts = facts?;
    (Some(facts.name.as_str()) == model.name.as_deref())
        .then_some(facts.facts.window)
        .flatten()
        .filter(|window| *window > 0)
}

/// The `config.yaml` keys Link sets (`None`: unset).
pub(super) fn config(settings: &Settings) -> Vec<(&'static str, Option<String>)> {
    let model = &settings.model;
    // A model running here is a custom OpenAI-compatible endpoint to
    // Hermes (its docs' recipe for a local Ollama). A hosted one with an
    // address is reached there: Link's proxy, which adds its key.
    let provider = match (model.is_local(), &model.base_url) {
        (true, Some(_)) => "custom".to_string(),
        _ => hermes_provider(&model.provider),
    };
    vec![
        ("model.provider", Some(provider)),
        (
            "model.base_url",
            model_url(model).map(|_| MODEL_URL_VAR.to_string()),
        ),
        ("model.default", model.name.clone()),
        (
            "model.context_length",
            context_length(&settings.model, settings.model_facts.as_ref())
                .map(|window| window.to_string()),
        ),
        ("terminal.backend", Some("local".into())),
        ("terminal.cwd", Some(settings.files.display().to_string())),
        // Its tools get a home of their own instead of reaching for
        // ~/.ssh and ~/.gitconfig, which the sandbox blocks anyway.
        ("terminal.home_mode", Some("profile".into())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rotating_keys_and_proxy_addresses_do_not_change_structure() {
        let read = |settings: serde_json::Value| read_by_configure(&settings.to_string()).unwrap();
        let before = read(json!({
            "actions": {"task": "http://127.0.0.1/first-action-secret"},
            "endpoint_key": "first-endpoint-key",
            "model": {"provider": "anthropic", "name": "claude", "api_key": "first-model-key", "base_url": "http://127.0.0.1/first-model-secret"},
            "files": "/work"
        }));
        let rotated = read(json!({
            "actions": {"task": "http://127.0.0.1/rotated-action-secret"},
            "endpoint_key": "rotated-endpoint-key",
            "model": {"provider": "anthropic", "name": "claude", "api_key": "rotated-model-key", "base_url": "http://127.0.0.1/rotated-model-secret"},
            "files": "/work"
        }));
        assert_eq!(before, rotated);
        let another = read(json!({
            "model": {"provider": "anthropic", "name": "other", "base_url": "http://127.0.0.1/model"},
            "files": "/work"
        }));
        assert_ne!(before, another);
    }

    #[test]
    fn known_model_window_is_configured_and_invalidates_only_relevant_cached_facts() {
        let mut settings = Settings {
            model: harness::Model {
                provider: "anthropic".into(),
                name: Some("model".into()),
                ..Default::default()
            },
            model_facts: Some(link_harness_contract::models::Known {
                name: "model".into(),
                facts: link_harness_contract::models::Facts {
                    window: Some(1_000_000),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let fingerprint =
            |s: &Settings| read_by_configure(&serde_json::to_string(s).unwrap()).unwrap();
        assert!(config(&settings).contains(&("model.context_length", Some("1000000".into()))));
        let first = fingerprint(&settings);
        settings.model_facts.as_mut().unwrap().facts.answer = Some(64000);
        assert_eq!(first, fingerprint(&settings));
        settings.model_facts.as_mut().unwrap().facts.window = Some(2_000_000);
        assert_ne!(first, fingerprint(&settings));
        settings.model.provider = "ollama".into();
        assert!(config(&settings).contains(&("model.context_length", None)));
        settings.model.provider = "anthropic".into();
        settings.model_facts.as_mut().unwrap().name = "other-model".into();
        assert!(config(&settings).contains(&("model.context_length", None)));
        settings.model_facts = None;
        assert!(config(&settings).contains(&("model.context_length", None)));
    }

    /// `hermes acp` runs the gateway agent's tools, its own cron off,
    /// whatever else the owner turned off kept.
    #[test]
    fn its_acp_runs_the_gateway_s_tools_without_its_own_cron() {
        let home = link_test_kit::TempDir::new("acp-toolsets");
        let path = home.path().join("config.yaml");
        std::fs::write(&path, "agent:\n  disabled_toolsets: [browser]\n").unwrap();
        acp_toolsets(&path).unwrap();
        acp_toolsets(&path).unwrap();
        let config: serde_norway::Value =
            serde_norway::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["platform_toolsets"]["acp"],
            serde_norway::Value::from(vec![ACP_TOOLSET])
        );
        assert_eq!(
            config["agent"]["disabled_toolsets"],
            serde_norway::Value::from(vec!["browser", CRON_TOOLSET])
        );
    }

    #[test]
    fn config_keeps_keys_out() {
        let settings = Settings {
            files: "/Users/me/Link".into(),
            agent: None,
            model: harness::Model {
                provider: "openrouter".into(),
                api_key: "sk-or-secret".to_string().into(),
                ..Default::default()
            },
            telegram: None,
            slack: None,
            discord: None,
            signal: None,
            endpoint_key: None,
            store: None,
            storage: None,
            tools: Default::default(),
            read_sources: Vec::new(),
            actions: Default::default(),
            memory: None,
            allowed: Vec::new(),
            unsandboxed: false,
            proxied: false,
            spec: None,
            identity: None,
            remembered: None,
            defaults: Vec::new(),
            model_facts: None,
            fallback: None,
            fallback_facts: None,
            environment: None,
            connections: Default::default(),
        };
        let config = config(&settings);
        assert!(!format!("{config:?}").contains("secret"));
        assert!(config.contains(&("model.provider", Some("openrouter".into()))));
        assert!(config.contains(&("model.default", None)));
        assert!(config.contains(&("terminal.cwd", Some("/Users/me/Link".into()))));
    }

    #[test]
    fn each_tool_is_an_mcp_server_at_its_proxy() {
        let settings = Settings {
            tools: [(
                "gh".to_string(),
                "http://127.0.0.1:40781/_link/mcp/gh".to_string(),
            )]
            .into(),
            ..Default::default()
        };
        let mut settings = settings;
        settings.memory = Some("http://127.0.0.1:40781/_link/mcp/memory-main".into());
        let dir = std::env::temp_dir().join(format!("lnk-hermes-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        // Hermes's own, and a server of the user's, stay; Link's gone one
        // is taken out.
        std::fs::write(
            &path,
            "model:\n  default: m1\nmcp_servers:\n  mine:\n    url: http://mine\n  old:\n    url: http://old\n",
        )
        .unwrap();
        mcp_servers(&path, &settings.mcp_servers(), &["old".to_string()]).unwrap();
        let config: serde_norway::Value =
            serde_norway::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["model"]["default"], "m1");
        assert_eq!(config["mcp_servers"]["mine"]["url"], "http://mine");
        assert_eq!(
            config["mcp_servers"]["gh"]["url"],
            "http://127.0.0.1:40781/_link/mcp/gh"
        );
        assert_eq!(
            config["mcp_servers"]["memory"]["url"],
            "http://127.0.0.1:40781/_link/mcp/memory-main"
        );
        assert!(config["mcp_servers"].get("old").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Link's bridge carries every connection: every key of Hermes's own
    /// channels is in the list `configure` removes.
    #[test]
    fn none_of_its_own_channels_is_left_on() {
        for key in [
            "TELEGRAM_BOT_TOKEN",
            "SLACK_APP_TOKEN",
            "DISCORD_BOT_TOKEN",
            "SIGNAL_ACCOUNT",
        ] {
            assert!(CHANNEL_KEYS.contains(&key), "{key}");
        }
    }

    #[test]
    fn a_hosted_model_on_the_wire_keeps_its_provider() {
        let settings = Settings {
            model: harness::Model {
                provider: "anthropic".into(),
                api_key: "sk-ant-api03-link-wire".to_string().into(),
                base_url: Some("http://127.0.0.1:40781/_link/model".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let config = config(&settings);
        assert!(config.contains(&("model.provider", Some("anthropic".into()))));
        assert!(config.contains(&("model.base_url", Some(MODEL_URL_VAR.into()))));
        assert_eq!(
            model_url(&settings.model).as_deref(),
            Some("http://127.0.0.1:40781/_link/model")
        );
    }

    #[test]
    fn openai_is_hermes_s_openai_api_at_its_proxy() {
        let settings = Settings {
            model: harness::Model {
                provider: "openai".into(),
                name: Some("gpt-6-luna".into()),
                base_url: Some("http://127.0.0.1:40781/_link/model/v1".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        // A bare `openai` is Hermes's old name for OpenRouter.
        assert!(config(&settings).contains(&("model.provider", Some("openai-api".into()))));
        // The others keep Link's names.
        assert_eq!(hermes_provider("anthropic"), "anthropic");
    }

    #[test]
    fn a_local_model_is_a_custom_endpoint() {
        let settings = Settings {
            model: harness::Model {
                provider: "ollama".into(),
                name: Some("qwen3:8b".into()),
                base_url: Some("http://localhost:11434".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let config = config(&settings);
        assert!(config.contains(&("model.provider", Some("custom".into()))));
        assert!(config.contains(&("model.base_url", Some(MODEL_URL_VAR.into()))));
        assert_eq!(
            model_url(&settings.model).as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert!(config.contains(&("model.default", Some("qwen3:8b".into()))));
    }
}
