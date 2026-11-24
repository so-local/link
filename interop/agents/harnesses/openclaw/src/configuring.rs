//! Configuring OpenClaw: what configure reads, its `.env`, its MCP
//! servers and the config sections it sets.

use super::*;

/// The MCP servers Link added to OpenClaw's settings, in its home's
/// `.openclaw/`, so it removes only its own.
pub(crate) const TOOLS_RECORD: &str = "link-tools.json";

/// The settings Link hands `configure` that it never reads: the agent's
/// spec and what Link knows of its model and its surroundings
/// (teammates, the commands here), which change without OpenClaw's
/// configuration changing; and a fallback model, which it doesn't take.
const NOT_READ: &[&str] = &[
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
];

/// Of the settings `input`, what [`configure`] reads, as bytes to tell
/// whether it changed since the last one: the rest left out, so a
/// teammate added doesn't run OpenClaw's CLI again.
pub(super) fn read_by_configure(input: &str) -> anyhow::Result<Vec<u8>> {
    let mut settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(input).within("reading settings on stdin")?;
    for key in NOT_READ {
        settings.remove(*key);
    }
    // These values are written only to `.env`, which is refreshed on
    // every configure. Link rotates its endpoint key at a start, and a
    // user can rotate a provider key without changing OpenClaw's JSON
    // structure. Neither should pay for a Node process per config key.
    settings.remove("endpoint_key");
    if let Some(model) = settings.get_mut("model").and_then(|m| m.as_object_mut()) {
        model.remove("api_key");
        // Provider JSON names the environment variable, never this URL.
        // Its presence selects that provider entry; its rotating secret
        // and address are refreshed in `.env`.
        if model.get("base_url").is_some_and(|url| !url.is_null()) {
            model.insert("base_url".into(), true.into());
        }
    }
    Ok(serde_json::to_vec(&settings)?)
}

/// Refreshes the values OpenClaw reads from `.env`. This is deliberately
/// apart from its structural configuration: keys can rotate at every
/// start, while the JSON they support usually does not change.
pub(super) fn configure_environment(settings: &Settings) -> Result<(), Report> {
    let env_file = prefix().join(".env");
    // Link's own gateway token, kept across runs.
    let token = link_secretive::Secret::new(
        // Link's key for this start, which its bridge sends: else its own.
        match (
            &settings.endpoint_key,
            harness::read_env_file(&env_file, "LNK_OPENCLAW_GATEWAY_TOKEN"),
        ) {
            (Some(key), _) => key.expose_owned(),
            (None, Some(token)) => token,
            (None, None) => harness::random_token().within("making its gateway token")?,
        },
    );
    let model = &settings.model;
    if model.provider == "local" {
        return Err(
            Report::error(sentence!("OpenClaw runs local models through Ollama only."))
                .why(sentence!("For another runtime, use Hermes or an API key."))
                .next(cmd!("lnk agent harness set hermes"), "use Hermes instead")
                .exit(Exit::Usage),
        );
    }
    let mut secrets = vec![
        ("ANTHROPIC_API_KEY", None),
        ("OPENAI_API_KEY", None),
        ("OPENROUTER_API_KEY", None),
        // OpenClaw's marker for a local Ollama, which needs no key.
        (
            "OLLAMA_API_KEY",
            (model.provider == "ollama").then_some("ollama-local"),
        ),
        // Where each secret is written: its `.env`, mode 600, which
        // OpenClaw reads; never a command line.
        ("LNK_OPENCLAW_GATEWAY_TOKEN", Some(token.expose().as_str())),
        // The model's address, which holds the proxy's secret for a
        // model on this machine: its provider's entry names the variable.
        // Gone only once no entry names it (below).
        (MODEL_URL, model.base_url.as_deref()),
        // Link's bridge carries every connection: no bot's token here.
        ("TELEGRAM_BOT_TOKEN", None),
        ("SLACK_BOT_TOKEN", None),
        ("SLACK_APP_TOKEN", None),
        ("DISCORD_BOT_TOKEN", None),
    ];
    for entry in &mut secrets {
        if Some(entry.0) == model.key_env() {
            entry.1 = Some(model.api_key.expose());
        }
    }
    secrets.retain(|(name, value)| *name != MODEL_URL || value.is_some());
    harness::merge_env_file(&env_file, &secrets).within("writing its .env")?;
    Ok(())
}

/// Refresh native MCP endpoints even when only the proxy's address or
/// credential changed, without invoking the structural Node CLI.
pub(super) fn configure_endpoints(settings: &Settings) -> Result<(), Report> {
    let servers = settings.mcp_servers();
    let gone = harness::tools_gone(&prefix().join(TOOLS_RECORD), &servers)?;
    write_mcp_servers(&prefix().join("openclaw.json"), &servers, &gone)?;
    Ok(())
}

pub(super) fn configure(settings: &Settings) -> Result<(), Report> {
    if !bin().is_file() {
        return Err(Report::error(sentence!("OpenClaw isn't installed."))
            .next(cmd!("lnk agent start"), "install it by starting the agent"));
    }
    let model = &settings.model;
    // Link's bridge carries every connection: the sections of OpenClaw's
    // channels that are plugins of their own are unset, Telegram's off
    // ([`sections`]).
    for connection in CHANNELS_OFF {
        let _ = openclaw()
            .args(["config", "unset", &format!("channels.{connection}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    for (key, value) in sections(settings, &prefix()) {
        harness::run_quiet(
            link_secretive::process::command(bin())
                .args([
                    "config",
                    "set",
                    key,
                    &value.to_string(),
                    "--strict-json",
                    "--merge",
                ])
                .env_remove("OPENCLAW_HOME")
                .env(crate::FS_SAFE.0, crate::FS_SAFE.1),
        )?;
    }

    let providers = providers(model);
    let record = prefix().join(PROVIDERS_SET);
    // Another model's, set earlier: OpenClaw would pick it when no model
    // is named. Only those Link set; gone already is fine. One that
    // doesn't go stays in the record, to be unset next time.
    let mut kept = Vec::new();
    for name in providers_gone(&record, &providers)? {
        let unset = openclaw()
            .args(["config", "unset", &format!("models.providers.{name}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if !unset.is_ok_and(|s| s.success()) {
            kept.push(name);
        }
    }
    for (name, entry) in &providers {
        if let Some(provider) = entry {
            harness::run_quiet(openclaw().args([
                "config",
                "set",
                &format!("models.providers.{name}"),
                &provider.to_string(),
                "--strict-json",
                "--merge",
            ]))?;
        }
    }
    record_providers(&record, &providers, &kept)?;
    // No entry names the model's address now: OpenClaw refuses a config
    // naming a variable that isn't set, so it goes only then.
    if model.base_url.is_none() && kept.is_empty() {
        harness::merge_env_file(&prefix().join(".env"), &[(MODEL_URL, None)])
            .within("writing its .env")?;
    }
    // A fresh install skipped onboarding, which sets up and migrates the
    // state database; the gateway refuses to start until this has run.
    // It runs here, in the box and its setup sandbox (`<name>-setup`),
    // while the gateway is stopped. Once per install of this release
    // (an upgrade migrates it again), until it succeeds.
    // Silent: its report is long, and shows only if doctor fails.
    let doctored = doctored();
    if std::fs::read_to_string(&doctored).ok().as_deref() != Some(VERSION) {
        match harness::run_quiet(openclaw().args(["doctor", "--fix", "--non-interactive"])) {
            Ok(()) => link_files::write_private(&doctored, VERSION.as_bytes())
                .within("recording its doctor")?,
            Err(err) => link_format::aside(
                Report::warning(sentence!("The OpenClaw doctor reported a problem."))
                    .why(sentence!(
                        "The system said: {}.",
                        Foreign::new(&err.to_string())
                    ))
                    .next(
                        cmd!("lnk agent start"),
                        "start anyway; it runs the doctor again",
                    ),
            ),
        }
    }
    // A hosted model's key is Link's to give, through its proxy: OpenClaw's
    // own copy of it goes, once doctor has taken an older install's
    // profiles in.
    let hosted = providers
        .iter()
        .any(|(name, entry)| *name == model.provider && *name != "ollama" && entry.is_some());
    if hosted && std::fs::read_to_string(&doctored).ok().as_deref() == Some(VERSION) {
        forget_keys(&model.provider);
    }
    match model_id(model) {
        // `models set` accepts a model its catalog doesn't list yet.
        Some(id) => harness::run_quiet(openclaw().args(["models", "set", &id]))?,
        // No model named: OpenClaw's own default for the provider.
        None => {
            let _ = openclaw()
                .args(["config", "unset", "agents.defaults.model"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    Ok(())
}

/// The key profiles of `provider` a listing (`models auth list --json`)
/// names: an API key's or a token's, never a login's (OAuth), which
/// can't be given another way.
fn key_profiles(listed: &serde_json::Value, provider: &str) -> Vec<String> {
    listed["profiles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["provider"] == provider)
        .filter(|p| matches!(p["type"].as_str(), Some("api_key" | "token")))
        .filter_map(|p| p["id"].as_str().map(str::to_string))
        .collect()
}

/// The files an older install kept auth profiles in, among `files` of an
/// agent's folder: `auth-profiles.json`, and the backups doctor keeps
/// beside it once it has taken them in.
fn profile_files(files: impl Iterator<Item = PathBuf>) -> Vec<PathBuf> {
    files
        .filter(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("auth-profiles"))
        })
        .collect()
}

/// OpenClaw's own copies of `provider`'s key: each key profile logged
/// out of each agent's store, and the files an older install kept them
/// in, which doctor has taken in, removed. One that doesn't go is said.
fn forget_keys(provider: &str) {
    let listed = openclaw()
        .args(["models", "auth", "list", "--provider", provider, "--json"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .unwrap_or_default();
    for id in key_profiles(&listed, provider) {
        if let Err(err) =
            harness::run_quiet(openclaw().args(["models", "auth", "logout", &id, "--yes"]))
        {
            link_format::aside(
                Report::warning(sentence!(
                    "OpenClaw still holds a key of its own for {}.",
                    provider.to_string()
                ))
                .why(sentence!(
                    "The system said: {}.",
                    Foreign::new(&err.to_string())
                )),
            );
        }
    }
    let agents = std::fs::read_dir(prefix().join("agents"))
        .into_iter()
        .flatten()
        .flatten();
    for agent in agents {
        let files = std::fs::read_dir(agent.path().join("agent"))
            .into_iter()
            .flatten()
            .flatten();
        for file in profile_files(files.map(|f| f.path())) {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// Secrets `values` taken out of each agent's auth profiles files in
/// home `home` (a copy of an install): each profile holding one goes, and
/// a file left with none goes with it. Its store's profiles are taken
/// out by configure ([`forget_keys`]), which runs OpenClaw.
pub(super) fn forget_values(home: &Path, values: &[link_secretive::Secret]) -> anyhow::Result<()> {
    let values: Vec<&str> = values.iter().map(|v| v.expose().as_str()).collect();
    let agents = std::fs::read_dir(home.join(".openclaw/agents"))
        .into_iter()
        .flatten()
        .flatten();
    for agent in agents {
        let files = std::fs::read_dir(agent.path().join("agent"))
            .into_iter()
            .flatten()
            .flatten();
        for file in profile_files(files.map(|f| f.path())) {
            let text =
                std::fs::read_to_string(&file).within(format!("reading {}", file.display()))?;
            if !values.iter().any(|v| text.contains(v)) {
                continue;
            }
            match without(&text, &values) {
                Some(kept) => link_files::write_private(&file, kept.as_bytes()),
                None => std::fs::remove_file(&file),
            }
            .within(format!("taking secrets out of {}", file.display()))?;
        }
    }
    Ok(())
}

/// Profiles file `text` without the profiles holding any of `values`, as
/// JSON; none when no profile is left, or it isn't a profiles file.
fn without(text: &str, values: &[&str]) -> Option<String> {
    let mut file: serde_json::Value = serde_json::from_str(text).ok()?;
    let profiles = file.get_mut("profiles")?.as_object_mut()?;
    profiles.retain(|_, p| {
        let held = p.to_string();
        !values.iter().any(|v| held.contains(v))
    });
    if profiles.is_empty() {
        return None;
    }
    serde_json::to_string_pretty(&file).ok()
}

pub(super) fn openclaw() -> Command {
    let mut command = link_secretive::process::command(bin());
    command
        .env_remove("OPENCLAW_HOME")
        .env(crate::FS_SAFE.0, crate::FS_SAFE.1);
    command
}

/// Each server as OpenClaw's MCP server: at its address, over
/// streamable HTTP.
/// Each server, as [`mcp_servers`] makes it, written into OpenClaw's
/// config at `path` under `mcp.servers`, and those `gone` taken out; the
/// rest of it as it was. Written here, never with `openclaw config set`:
/// an address holds the proxy's secret, and a command's arguments are any
/// user's to see. The file is this user's alone. It's read as JSON: it's
/// the adapter's own, in its own home, written by `openclaw config set`,
/// which writes JSON; one edited by hand into JSON5 (comments, trailing
/// commas) is refused with its path and left as it was, never rewritten.
fn write_mcp_servers(
    path: &Path,
    servers: &std::collections::BTreeMap<String, String>,
    gone: &[String],
) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut config: serde_json::Value = match text.trim() {
        "" => json!({}),
        _ => serde_json::from_str(&text).within(format!("reading {}", path.display()))?,
    };
    let root = config
        .as_object_mut()
        .within(format!("reading {} as an object", path.display()))?;
    let mcp = root.entry("mcp").or_insert_with(|| json!({}));
    if !mcp.is_object() {
        *mcp = json!({});
    }
    let listed = mcp
        .as_object_mut()
        .expect("an object")
        .entry("servers")
        .or_insert_with(|| json!({}));
    if !listed.is_object() {
        *listed = json!({});
    }
    let listed = listed.as_object_mut().expect("an object");
    for name in gone {
        listed.remove(name);
    }
    for (name, server) in mcp_servers(servers) {
        listed.insert(name.clone(), server);
    }
    link_files::write_private(path, serde_json::to_string_pretty(&config)?.as_bytes())?;
    Ok(())
}

fn mcp_servers(
    servers: &std::collections::BTreeMap<String, String>,
) -> Vec<(&String, serde_json::Value)> {
    servers
        .iter()
        .map(|(name, url)| (name, json!({"url": url, "transport": "streamable-http"})))
        .collect()
}

/// OpenClaw's channels that are plugins of their own, and WhatsApp:
/// their sections unset at every start, as Link's bridge carries every
/// connection, so none is kept on.
const CHANNELS_OFF: &[&str] = &["slack", "discord", "signal", "whatsapp"];

/// The config sections Link sets, merged into whatever else is there.
/// The gateway's token in OpenClaw's config: the variable its `.env`
/// holds it in, which OpenClaw reads (`${VAR}` in a config value).
const GATEWAY_TOKEN_VAR: &str = "${LNK_OPENCLAW_GATEWAY_TOKEN}";

fn sections(settings: &Settings, prefix: &Path) -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "gateway",
            json!({
                "mode": "local",
                "port": port(),
                "bind": "loopback",
                // The token itself is in its `.env`, never on the command
                // line that sets this: OpenClaw reads the variable.
                "auth": {"mode": "token", "token": GATEWAY_TOKEN_VAR},
                // Its chat completions off: Link's bridge reaches it over ACP, through the hub.
                "http": {"endpoints": {"chatCompletions": {"enabled": false}}},
            }),
        ),
        (
            "agents.defaults",
            json!({"workspace": prefix.join("workspace")}),
        ),
        ("logging", json!({"file": prefix.join("logs/openclaw.log")})),
        ("discovery.mdns", json!({"mode": "off"})),
        ("browser", browser(settings, cfg!(target_os = "macos"))),
        // Link's bridge carries Telegram too.
        ("channels.telegram", json!({"enabled": false})),
        // Link's scheduler fires the agent's schedules: OpenClaw's own
        // cron off, as its channels are, and its cron tool denied, so the
        // agent sets a schedule with Link's scheduler, never one that
        // would never fire.
        ("cron", json!({"enabled": false})),
        ("tools", json!({"deny": denied(prefix)})),
    ]
}

/// The tools OpenClaw's config at `prefix` denies, with its native cron
/// tool among them: `config set --merge` replaces a list, so the agent's
/// own denials are kept by naming them again.
fn denied(prefix: &Path) -> Vec<String> {
    let config = std::fs::read(prefix.join("openclaw.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let mut denied: Vec<String> = config
        .as_ref()
        .and_then(|config| config["tools"]["deny"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|tool| tool.as_str().map(str::to_owned))
        .collect();
    if !denied.iter().any(|tool| tool == NATIVE_CRON_TOOL) {
        denied.push(NATIVE_CRON_TOOL.into());
    }
    denied
}

/// OpenClaw's own tool that sets a schedule on its cron.
const NATIVE_CRON_TOOL: &str = "cron";

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a key's profiles of the model's provider are logged out: an
    /// API key's and a token's, never a login's, nor another provider's.
    #[test]
    fn only_the_provider_s_keys_are_forgotten() {
        let listed = serde_json::json!({"profiles": [
            {"id": "anthropic:manual", "provider": "anthropic", "type": "api_key"},
            {"id": "anthropic:setup", "provider": "anthropic", "type": "token"},
            {"id": "anthropic:me", "provider": "anthropic", "type": "oauth"},
            {"id": "openai:manual", "provider": "openai", "type": "api_key"},
        ]});
        assert_eq!(
            key_profiles(&listed, "anthropic"),
            ["anthropic:manual", "anthropic:setup"]
        );
        assert!(key_profiles(&serde_json::Value::Null, "anthropic").is_empty());
    }

    /// The old profiles file and doctor's backups of it go, nothing else.
    #[test]
    fn the_old_profiles_and_their_backups_are_found() {
        let files = [
            "auth-profiles.json",
            "auth-profiles.json.1761500000.bak",
            "auth-state.json",
            "models.json",
        ]
        .map(|f| PathBuf::from("/a/agents/main/agent").join(f));
        let found = profile_files(files.into_iter());
        assert_eq!(found.len(), 2, "{found:?}");
    }

    /// A secret Link took leaves every profiles file of a copy; a login's
    /// profile and a file without it stay.
    #[test]
    fn a_taken_secret_leaves_the_profiles() {
        let home = link_test_kit::TempDir::new("clean");
        let agent = home.join(".openclaw/agents/main/agent");
        std::fs::create_dir_all(&agent).unwrap();
        let profile = |key: &str| json!({"type": "api_key", "provider": "anthropic", "key": key});
        let login = json!({"type": "oauth", "provider": "anthropic", "access": "a"});
        let write = |name: &str, profiles: serde_json::Value| {
            let file = json!({"version": 1, "profiles": profiles});
            std::fs::write(agent.join(name), file.to_string()).unwrap();
        };
        write(
            "auth-profiles.json",
            json!({"anthropic:default": profile("sk-taken"), "anthropic:me": login}),
        );
        write(
            "auth-profiles.json.1.bak",
            json!({"a:d": profile("sk-taken")}),
        );
        write(
            "auth-profiles.json.2.bak",
            json!({"a:d": profile("sk-other")}),
        );
        forget_values(&home, &[link_secretive::Secret::new("sk-taken".into())]).unwrap();
        let kept = std::fs::read_to_string(agent.join("auth-profiles.json")).unwrap();
        assert!(
            !kept.contains("sk-taken") && kept.contains("anthropic:me"),
            "{kept}"
        );
        assert!(!agent.join("auth-profiles.json.1.bak").exists());
        assert!(agent.join("auth-profiles.json.2.bak").exists());
    }

    /// A change to what configure never reads (a teammate added, a
    /// command found here, the spec) leaves it skipped; one to what it
    /// reads doesn't.
    #[test]
    fn only_what_configure_reads_decides_whether_it_runs() {
        let read = |settings: serde_json::Value| read_by_configure(&settings.to_string()).unwrap();
        let before = read(json!({
            "actions": {"task": "http://127.0.0.1/first-secret"},
            "endpoint_key": "first-bridge-key",
            "model": {"provider": "anthropic", "name": "claude", "api_key": "first-model-key", "base_url": "http://127.0.0.1/first-model-secret"},
            "environment": {"teammates": []},
            "spec": {"turns": {"rounds": 8}},
        }));
        let teammate = read(json!({
            "actions": {"task": "http://127.0.0.1/rotated-secret"},
            "endpoint_key": "rotated-bridge-key",
            "spec": {"turns": {"rounds": 9}},
            "environment": {"teammates": [{"name": "scout"}]},
            "defaults": ["window.max"],
            "model_facts": {"window": 200000},
            "model": {"provider": "anthropic", "name": "claude", "api_key": "rotated-model-key", "base_url": "http://127.0.0.1/rotated-model-secret"},
        }));
        assert_eq!(before, teammate);
        let model = read(json!({"model": {"provider": "anthropic", "name": "other"}}));
        assert_ne!(before, model);
    }

    /// The agent's own denials stay, its native cron tool added once.
    #[test]
    fn its_cron_tool_is_denied_beside_the_agent_s_own_denials() {
        let prefix = link_test_kit::TempDir::new("openclaw-denied");
        assert_eq!(denied(&prefix), ["cron"]);
        std::fs::write(
            prefix.join("openclaw.json"),
            r#"{"tools": {"deny": ["exec", "cron", 7]}}"#,
        )
        .unwrap();
        assert_eq!(denied(&prefix), ["exec", "cron"]);
        std::fs::write(
            prefix.join("openclaw.json"),
            r#"{"tools": {"deny": ["exec"]}}"#,
        )
        .unwrap();
        assert_eq!(denied(&prefix), ["exec", "cron"]);
    }

    #[test]
    fn config_sections() {
        let settings = Settings {
            actions: Default::default(),
            files: "/Users/me/Link".into(),
            agent: None,
            model: harness::Model {
                provider: "anthropic".into(),
                api_key: "sk-ant-secret".to_string().into(),
                ..Default::default()
            },
            telegram: Some(harness::Telegram {
                bot_token: "123:secret".to_string().into(),
                allowed_users: vec![42],
                api_root: None,
            }),
            slack: None,
            discord: None,
            signal: None,
            endpoint_key: None,
            store: None,
            storage: None,
            tools: Default::default(),
            read_sources: Vec::new(),
            memory: None,
            allowed: Vec::new(),
            unsandboxed: false,
            proxied: true,
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
        let sections = sections(&settings, Path::new("/Users/me/.openclaw"));
        let all = serde_json::to_string(&sections).unwrap();
        // Keys stay out of the config (and off command lines).
        assert!(!all.contains("secret"), "{all}");
        let get = |name| sections.iter().find(|(k, _)| *k == name).unwrap().1.clone();
        // The gateway's token too: OpenClaw reads it from its `.env`.
        assert_eq!(
            get("gateway")["auth"]["token"],
            "${LNK_OPENCLAW_GATEWAY_TOKEN}"
        );
        assert!(get("agents.defaults").get("model").is_none());
        // Link's bridge carries Telegram, given or not.
        assert_eq!(get("channels.telegram"), json!({"enabled": false}));
        assert_eq!(get("cron"), json!({"enabled": false}));
        assert_eq!(get("tools"), json!({"deny": ["cron"]}));
        assert_eq!(
            get("gateway")["http"]["endpoints"]["chatCompletions"]["enabled"],
            false
        );
        assert_eq!(get("gateway")["bind"], "loopback");
        // Behind its proxy, its browser is on, set every time so a yes given
        // later turns it back on.
        assert_eq!(get("browser"), json!({"enabled": true}));
        assert!(!sections.iter().any(|(k, _)| *k == "channels.slack"));
        assert!(!sections.iter().any(|(k, _)| *k == "channels.discord"));
        assert!(!sections.iter().any(|(k, _)| *k == "channels.signal"));
    }

    #[test]
    fn each_tool_is_an_mcp_server_at_its_proxy() {
        let settings = Settings {
            memory: Some("http://127.0.0.1:40781/_link/mcp/memory-main".into()),
            ..Default::default()
        };
        let all = settings.mcp_servers();
        let servers = mcp_servers(&all);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].0, "memory");
        assert_eq!(
            servers[0].1,
            json!({"url": "http://127.0.0.1:40781/_link/mcp/memory-main", "transport": "streamable-http"})
        );
        // Written into its config: OpenClaw's own, and a server of the
        // user's, stay; Link's gone one is taken out.
        let dir = std::env::temp_dir().join(format!("lnk-openclaw-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("openclaw.json");
        std::fs::write(
            &path,
            r#"{"gateway":{"port":1},"mcp":{"servers":{"mine":{"url":"http://mine"},"old":{"url":"http://old"}}}}"#,
        )
        .unwrap();
        write_mcp_servers(&path, &all, &["old".to_string()]).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["gateway"]["port"], 1);
        assert_eq!(config["mcp"]["servers"]["mine"]["url"], "http://mine");
        assert_eq!(
            config["mcp"]["servers"]["memory"]["url"],
            "http://127.0.0.1:40781/_link/mcp/memory-main"
        );
        assert!(config["mcp"]["servers"].get("old").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
