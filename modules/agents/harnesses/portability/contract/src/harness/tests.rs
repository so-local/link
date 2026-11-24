/// An agent's home for a harness is listed only where every folder
/// to it is one, never through a link.
#[cfg(unix)]
#[test]
fn homes_follow_no_link() {
    use std::os::unix::fs::symlink;
    let root = link_test_kit::TempDir::new("harness-homes");
    let agents = root.join("Agents");
    let elsewhere = root.join("elsewhere");
    for dir in [
        agents.join("main/harnesses/example"),
        agents.join("boxes/harnesses"),
        agents.join("other"),
        elsewhere.join("harnesses/example"),
        elsewhere.join("example"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // The home a link, its `harnesses` a link, the agent a link.
    symlink(
        elsewhere.join("example"),
        agents.join("boxes/harnesses/example"),
    )
    .unwrap();
    symlink(elsewhere.join("harnesses"), agents.join("other/harnesses")).unwrap();
    symlink(&elsewhere, agents.join("linked")).unwrap();
    assert_eq!(
        homes_in(&agents, "example"),
        vec![("main".to_string(), agents.join("main/harnesses/example"))]
    );
    assert!(homes_in(&agents, "other-harness").is_empty());
    assert!(homes_in(&root.join("none"), "example").is_empty());
}

use super::*;

/// A team's folders: names of its shared folder, or `.` for all of
/// it, never `.` written beside another, never one named twice.
#[test]
fn a_teams_folders_are_checked() {
    let folders = |write: &[&str], read: &[&str]| TeamFolders {
        write: write.iter().map(|s| s.to_string()).collect(),
        read: read.iter().map(|s| s.to_string()).collect(),
    };
    for good in [
        folders(&["drafts"], &["sources"]),
        folders(&["."], &[]),
        folders(&["drafts"], &["."]),
        folders(&[], &[]),
    ] {
        good.check("research").unwrap();
    }
    for bad in [
        folders(&["."], &["sources"]),
        folders(&["drafts"], &["drafts"]),
        folders(&["Drafts"], &[]),
        folders(&["a/b"], &[]),
        folders(&[".."], &[]),
    ] {
        assert!(bad.check("research").is_err(), "{bad:?}");
    }
    let (write, read) = folders(&["drafts"], &["."]).paths(std::path::Path::new("/t/Shared"));
    assert_eq!(write, [PathBuf::from("/t/Shared/drafts")]);
    assert_eq!(read, [PathBuf::from("/t/Shared")]);
    assert!(folders(&["."], &[]).writes("drafts"));
    assert!(!folders(&["drafts"], &["."]).writes("sources"));
    // In the sandbox file, under a team's name.
    let files: Folders =
        toml::from_str("write = [\"work\"]\n[teams.research]\nwrite = [\"drafts\"]\n").unwrap();
    files.check().unwrap();
    assert!(files.teams["research"].writes("drafts"));
    let bad: Folders = toml::from_str("[teams.\"no way\"]\nwrite = [\"x\"]\n").unwrap();
    assert!(bad.check().is_err());
}

/// `.` writes the whole of Home, a repository, beside only the
/// folders of the agent's own it names: its conversations stay its
/// harness's.
#[test]
fn the_whole_of_home_is_dot() {
    let folders = |write: &[&str], read: &[&str]| Folders {
        write: write.iter().map(|s| s.to_string()).collect(),
        read: read.iter().map(|s| s.to_string()).collect(),
        teams: BTreeMap::new(),
    };
    let whole = folders(&["."], &[]);
    whole.check().unwrap();
    assert!(whole.whole());
    assert!(whole.writes("src") && whole.writes(layout::WORK));
    assert!(!whole.writes(layout::CONVERSATIONS));
    // Its instructions aren't in Home: named beside it, written.
    assert!(!whole.writes(layout::INSTRUCTIONS));
    let both = folders(&[".", "instructions"], &["chats"]);
    both.check().unwrap();
    assert!(both.writes(layout::INSTRUCTIONS) && both.writes("src"));
    assert!(!both.writes(layout::SKILLS));
    for bad in [
        folders(&[".", "work"], &[]),
        folders(&["."], &["drafts"]),
        folders(&["work"], &["."]),
        folders(&[".."], &[]),
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
    assert!(!Folders::default().whole());
}

#[test]
fn a_need_is_a_spec() {
    let needs: Needs = serde_json::from_str(
        r#"{"needs": [{"spec": {"network": {"mode": "host"}, "internet": {"hosts": ["*"]}},
                       "why": "its websockets ignore HTTPS_PROXY"}],
            "unsandboxed": {"why": "it runs ps"}}"#,
    )
    .unwrap();
    let checked = needs.checked().unwrap();
    assert_eq!(checked.len(), 1);
    assert_eq!(
        checked[0].0.network.as_ref().unwrap().mode,
        Some(crate::sandbox::spec::NetworkMode::Host)
    );
    assert_eq!(checked[0].1, "its websockets ignore HTTPS_PROXY");
    // Only another OS's: nothing needed here.
    let other = match cfg!(target_os = "macos") {
        true => "linux",
        false => "macos",
    };
    let elsewhere: Needs = serde_json::from_str(&format!(
        r#"{{"needs": [{{"spec": {{"commands": {{"{other}": {{"developer_tools": true}}}}}}, "why": "x"}}]}}"#
    ))
    .unwrap();
    assert!(elsewhere.checked().unwrap().is_empty());
    // Nothing said: nothing needed.
    assert_eq!(
        serde_json::from_str::<Needs>("{}").unwrap(),
        Needs::default()
    );
    let refused = |json: &str| {
        serde_json::from_str::<Needs>(json)
            .map_err(anyhow::Error::from)
            .and_then(|n| n.checked())
            .is_err()
    };
    for bad in [
        r#"{"needs": [{"spec": {"root": true}, "why": "x"}]}"#,
        r#"{"needs": [{"spec": {"lan": {"hosts": ["*"]}}, "why": ""}]}"#,
        r#"{"needs": [{"spec": {"lan": {"hosts": ["*"]}}, "why": "a\nb"}]}"#,
        r#"{"needs": [{"spec": {"files": {"read": ["./x"]}}, "why": "x"}]}"#,
        r#"{"needs": [{"spec": {"name": "x", "lan": {"hosts": ["*"]}}, "why": "x"}]}"#,
        r#"{"needs": [{"name": "lan", "why": "x"}]}"#,
        // One that opens nothing on any OS, or lifts the agent's limit.
        r#"{"needs": [{"spec": {}, "why": "x"}]}"#,
        r#"{"needs": [{"spec": {"internet": {"hosts": ["b.com"], "per_minute": 1000}}, "why": "x"}]}"#,
        r#"{"needs": [{"spec": {"internet": {"hosts": ["b.com"], "macos": {"per_minute": 1000}}}, "why": "x"}]}"#,
        r#"{"unsandboxed": {"why": ""}}"#,
        r#"{"keychain": true}"#,
        r#"{"risks": [{"name": "lan", "why": "x"}]}"#,
        r#"{"risks": [{"name": "unsandboxed", "why": "x"}]}"#,
        r#"{"risks": [{"name": "Browser", "why": "x"}]}"#,
        r#"{"risks": [{"name": "-x", "why": "x"}]}"#,
        r#"{"risks": [{"name": "a b", "why": "x"}]}"#,
        r#"{"risks": [{"name": "browser", "why": ""}]}"#,
        r#"{"risks": [{"name": "browser", "why": "x", "without": ""}]}"#,
        r#"{"risks": [{"name": "browser", "why": "x"}, {"name": "browser", "why": "y"}]}"#,
        r#"{"risks": [{"name": "browser", "why": "x", "port": 1}]}"#,
    ] {
        assert!(refused(bad), "{bad}");
    }
}

/// A harness says its install runs what it downloads, and it comes
/// back as said; left out, it doesn't, and a near name is refused.
#[test]
fn an_install_that_runs_its_downloads_says_so() {
    let needs: Needs = serde_json::from_str(r#"{"install_runs_downloads": true}"#).unwrap();
    assert!(needs.install_runs_downloads);
    let json = serde_json::to_string(&needs).unwrap();
    assert_eq!(json, r#"{"install_runs_downloads":true}"#);
    assert_eq!(serde_json::from_str::<Needs>(&json).unwrap(), needs);
    // A need of no permission: nothing to ask as one.
    assert_eq!(needs.checked().unwrap(), []);
    assert_eq!(serde_json::to_string(&Needs::default()).unwrap(), "{}");
    for bad in [
        r#"{"install_runs_download": true}"#,
        r#"{"install_runs_downloads": "yes"}"#,
    ] {
        assert!(serde_json::from_str::<Needs>(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_risk_is_the_harnesss_own_name() {
    let needs: Needs = serde_json::from_str(
        r#"{"risks": [{"name": "open-bridge", "why": "it asks for no credential"},
                      {"name": "browser", "why": "its ports", "without": "it's off"}]}"#,
    )
    .unwrap();
    // No permission among them: a risk is no permission.
    assert_eq!(needs.checked().unwrap(), []);
    assert_eq!(needs.risks[1].without.as_deref(), Some("it's off"));
    assert!(is_risk_name("browser"));
    assert!(!is_risk_name(&"a".repeat(MAX_RISK_NAME + 1)));
    assert!(!is_risk_name("tool"));
}

#[test]
fn configure_is_skipped_only_when_nothing_changed() {
    let dir = std::env::temp_dir().join(format!("lnk-configured-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.json");
    let stamp = dir.join("configured");
    let configured = Configured::at(stamp.clone(), "1.0/2026.1", vec![config.clone()]);
    assert!(!configured.unchanged(b"{}"));
    std::fs::write(&config, "a").unwrap();
    configured.done(b"{}").unwrap();
    assert!(configured.unchanged(b"{}"));
    // Other settings, another version, or its file changed: again.
    assert!(!configured.unchanged(b"{\"x\":1}"));
    assert!(!Configured::at(stamp.clone(), "1.1/2026.1", vec![config.clone()]).unchanged(b"{}"));
    std::fs::write(&config, "b").unwrap();
    assert!(!configured.unchanged(b"{}"));
    configured.done(b"{}").unwrap();
    std::fs::remove_file(&config).unwrap();
    assert!(!configured.unchanged(b"{}"));
    std::fs::write(&config, "b").unwrap();
    assert!(configured.unchanged(b"{}"));
    configured.forget();
    assert!(!configured.unchanged(b"{}"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_adapter_removes_only_the_tools_it_added_that_are_gone() {
    let dir = std::env::temp_dir().join(format!("lnk-tools-gone-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let record = dir.join("link-tools.json");
    let _ = std::fs::remove_file(&record);
    let tools = |names: &[&str]| -> BTreeMap<String, String> {
        names
            .iter()
            .map(|n| {
                (
                    n.to_string(),
                    format!("http://127.0.0.1:40781/_link/mcp/{n}"),
                )
            })
            .collect()
    };
    assert!(
        tools_gone(&record, &tools(&["gh", "memory"]))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        std::fs::read_to_string(&record).unwrap(),
        r#"{"tools":["gh","memory"],"schema":1}"#
    );
    assert_eq!(tools_added(&record), ["gh", "memory"]);
    assert_eq!(tools_gone(&record, &tools(&["memory"])).unwrap(), ["gh"]);
    assert!(tools_gone(&record, &tools(&[])).unwrap() == ["memory"]);
    assert!(tools_gone(&record, &tools(&[])).unwrap().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn installers_run_only_if_pinned() {
    let installer = Installer {
        name: "Test",
        url: "https://example.com/install.sh",
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        companions: &[],
    };
    assert!(verify(installer.name, installer.sha256, b"").is_ok());
    let err = verify(installer.name, installer.sha256, b"429: rate limited").unwrap_err();
    assert!(err.to_string().contains("isn't the one it pins"), "{err}");

    let dir = link_test_kit::TempDir::new("installer");
    assert_eq!(
        companion_path(&dir, "policy.sh").unwrap(),
        dir.join("policy.sh")
    );
    for unsafe_name in ["", ".", "..", "../policy.sh", "dir/policy.sh", "/policy.sh"] {
        assert!(
            companion_path(&dir, unsafe_name).is_err(),
            "{unsafe_name:?}"
        );
    }
}

#[test]
fn a_base_port_needs_its_whole_block() {
    assert_eq!(port_or(None, 18789), 18789);
    assert_eq!(port_or(Some("20200"), 18789), 20200);
    for bad in ["", "x", "80", "65400", "70000", "-1"] {
        assert_eq!(port_or(Some(bad), 8642), 8642, "{bad}");
    }
}

#[test]
fn env_files_keep_other_lines() {
    let dir = std::env::temp_dir().join(format!("lnk-env-{}", std::process::id()));
    let path = dir.join(".env");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        "# mine\nOTHER=1\nANTHROPIC_API_KEY=old\nexport TELEGRAM_BOT_TOKEN=x\n",
    )
    .unwrap();
    merge_env_file(
        &path,
        &[
            ("ANTHROPIC_API_KEY", Some("sk-\"new\"")),
            ("TELEGRAM_BOT_TOKEN", None),
        ],
    )
    .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        "# mine\nOTHER=1\nANTHROPIC_API_KEY=\"sk-\\\"new\\\"\"\n"
    );
    assert_eq!(read_env_file(&path, "OTHER").as_deref(), Some("1"));
    assert_eq!(read_env_file(&path, "TELEGRAM_BOT_TOKEN"), None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // A newline would smuggle in a line of its own: refused, file kept.
    for bad in ["sk\nAPI_SERVER_HOST=0.0.0.0", "sk\r", "sk\0", "sk\x1b[2J"] {
        let err = merge_env_file(&path, &[("ANTHROPIC_API_KEY", Some(bad))]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_the_key_proves_an_endpoint() {
    let challenge = random_token().unwrap();
    assert!(is_challenge(&challenge));
    let proof = endpoint_proof(
        &link_secretive::Secret::from("the-key".to_string()),
        &challenge,
    );
    assert_eq!(proof.len(), 64);
    assert!(proof_matches(
        &link_secretive::Secret::from("the-key".to_string()),
        &challenge,
        &proof
    ));
    // Another key, another challenge, or no key: no proof.
    assert!(!proof_matches(
        &link_secretive::Secret::from("another-key".to_string()),
        &challenge,
        &proof
    ));
    assert!(!proof_matches(
        &link_secretive::Secret::from("the-key".to_string()),
        &random_token().unwrap(),
        &proof
    ));
    assert!(!proof_matches(
        &link_secretive::Secret::from("".to_string()),
        &challenge,
        &endpoint_proof(&link_secretive::Secret::from("".to_string()), &challenge)
    ));
    // The key itself, or anything not a proof, isn't one.
    let longer = format!("{proof}0");
    let not_hex = "zz".repeat(32);
    for bad in ["the-key", "", &proof[..63], &longer, &not_hex] {
        assert!(
            !proof_matches(
                &link_secretive::Secret::from("the-key".to_string()),
                &challenge,
                bad
            ),
            "{bad}"
        );
    }
    for bad in ["", "abc", &"g".repeat(32), &"a".repeat(129)] {
        assert!(!is_challenge(bad), "{bad}");
    }
}

/// A binding's key is never printed, and an adapter that says
/// nothing of starting blank is never started so.
#[test]
fn a_bind_never_shows_its_key() {
    let bind: Bind = serde_json::from_str(
        r#"{"agent": {"name": "main", "id": "a1b2c3", "owner": "acct-1", "teams": ["t1"]},
            "key": "per-binding-secret",
            "settings": {"files": "/files", "model": {"provider": "anthropic",
                         "api_key": "placeholder"}, "endpoint_key": "placeholder"},
            "store": "/run/box/store.sock"}"#,
    )
    .unwrap();
    let shown = format!("{bind:?} {bind:#?}");
    assert!(!shown.contains("per-binding-secret"), "{shown}");
    assert!(
        shown.contains("a1b2c3") && shown.contains("store.sock"),
        "{shown}"
    );
    assert_eq!(bind.files, None);
    // It proves its bridge calls as the agent, with the binding's key.
    let binding = bind.binding();
    assert_eq!(
        (
            binding.identity.agent.as_str(),
            binding.key.expose().as_str()
        ),
        ("a1b2c3", "per-binding-secret")
    );
    assert!(!format!("{binding:?}").contains("per-binding-secret"));
    // Anything else on the line is refused, not ignored.
    assert!(
        serde_json::from_str::<Bind>(
            r#"{"agent": {"name": "m", "id": "a", "owner": "o"}, "key": "k",
                "settings": {"files": "/f", "model": {"provider": "local"}}, "port": 1}"#
        )
        .is_err()
    );
    assert_eq!(serde_json::to_string(&Bound::default()).unwrap(), "{}");
    let old: Contract = serde_json::from_str(r#"{"version": 1, "proxy": true}"#).unwrap();
    assert!(!old.blank);
    assert!(
        !serde_json::to_string(&Contract::new(true))
            .unwrap()
            .contains("blank")
    );
}

#[test]
fn a_contract_without_proof_reads_as_none() {
    let old: Contract = serde_json::from_str(r#"{"version": 2, "proxy": true}"#).unwrap();
    assert!(!old.proof);
    assert_eq!(
        serde_json::to_string(&Contract::new(true)).unwrap(),
        format!(r#"{{"version":{},"proxy":true}}"#, PROTOCOL.current)
    );
    let proving = Contract {
        proof: true,
        ..Contract::new(true)
    };
    let json = serde_json::to_string(&proving).unwrap();
    assert!(json.contains(r#""proof":true"#), "{json}");
    assert!(!json.contains("agent"), "{json}");
    // One that says nothing of the agent reads none.
    assert!(!old.agent);
    let reading: Contract =
        serde_json::from_str(r#"{"version": 1, "proxy": true, "agent": true}"#).unwrap();
    assert!(reading.agent);
    // Nor of a conversation's tools: it's never sent a list.
    assert!(!old.tools);
    assert!(!json.contains("tools"), "{json}");
}

#[test]
fn endpoints_are_on_the_loopback() {
    assert_eq!(
        endpoint_port("http://127.0.0.1:18800/v1/chat/completions").unwrap(),
        18800
    );
    assert_eq!(endpoint_port("http://localhost:20200").unwrap(), 20200);
    for bad in [
        "https://127.0.0.1:18800/v1",
        "http://example.com:18800/v1",
        "http://127.0.0.1.example.com:80/",
        "http://127.0.0.1/v1",
        "http://127.0.0.1:80/v1",
        "http://127.0.0.1:x/v1",
        "http://127.0.0.1:18800/v1 x",
    ] {
        assert!(endpoint_port(bad).is_err(), "{bad}");
    }
}

#[test]
fn settings_round_trip() {
    let settings = Settings {
        files: "/Users/me/Link".into(),
        agent: None,
        model: Model {
            provider: "openrouter".into(),
            api_key: "sk-or-1".to_string().into(),
            ..Model::default()
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
        environment: Some(Environment {
            connections: Vec::new(),
            folders: Folders::default(),
            commands: Vec::new(),
            teams: Vec::new(),
        }),
        connections: Default::default(),
    };
    let json = serde_json::to_string(&settings).unwrap();
    for absent in [
        "telegram",
        "slack",
        "discord",
        "signal",
        "name",
        "endpoint_key",
        "memory",
        "read_sources",
        "allowed",
        "unsandboxed",
        "proxied",
        "fallback",
    ] {
        assert!(!json.contains(absent), "{json}");
    }
    assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    // A fallback is its own, never taken for a connection.
    // Copies, read back as a harness reads them.
    let model = serde_json::to_string(&settings.model).unwrap();
    let fell = Settings {
        fallback: Some(serde_json::from_str(&model).unwrap()),
        ..serde_json::from_str(&json).unwrap()
    };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&fell).unwrap()).unwrap();
    assert_eq!(back, fell);
    assert!(back.connections.is_empty());
    assert_eq!(settings.model.key_env(), Some("OPENROUTER_API_KEY"));
    assert!(!settings.model.is_local());
    assert_eq!(random_token().unwrap().len(), 32);
}

/// A connection from outside Link is under its name, as Link's own are.
#[test]
fn other_connections_sit_beside_links_own() {
    let json = serde_json::json!({
        "files": "/x",
        "model": {"provider": "anthropic", "api_key": ""},
        "telegram": {"bot_token": "1:a", "allowed_users": [42]},
        "matrix": {"token": "syt", "allowed_users": ["@me:example.org"]},
    });
    let settings: Settings = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(settings.telegram.as_ref().unwrap().allowed_users, [42]);
    assert_eq!(settings.connections.keys().collect::<Vec<_>>(), ["matrix"]);
    assert_eq!(serde_json::to_value(&settings).unwrap(), json);
}

/// `proxied` is no connection.
#[test]
fn proxied_is_handed_only_when_so() {
    let json = serde_json::json!({
        "files": "/x",
        "model": {"provider": "anthropic", "api_key": ""},
        "proxied": true,
    });
    let settings: Settings = serde_json::from_value(json.clone()).unwrap();
    assert!(settings.proxied && settings.connections.is_empty());
    assert_eq!(serde_json::to_value(&settings).unwrap(), json);
    assert!(NOT_CONNECTIONS.contains(&"proxied"));
}

/// An installer trusts the machine's CAs too, the first bundle there is,
/// for Node and for uv, Python and curl: behind a proxy that re-signs
/// TLS, their own refuse the registries. What it's given stays.
#[test]
fn installers_trust_the_machines_cas() {
    let dir = link_test_kit::TempDir::new("cas");
    let bundle = dir.join("ca-bundle.crt");
    std::fs::write(&bundle, "").unwrap();
    let missing = dir.join("none.crt");
    let cas = [missing.to_str().unwrap(), bundle.to_str().unwrap()];
    let given = |command: &std::process::Command, variable: &str| {
        command
            .get_envs()
            .find(|(k, _)| *k == variable)
            .and_then(|(_, v)| v.map(std::path::PathBuf::from))
    };
    // A child with none of this process's secrets, and neither variable
    // of this machine's, whatever it sets.
    let bare = || {
        let mut command = link_secretive::process::command("true");
        command
            .env_remove("NODE_EXTRA_CA_CERTS")
            .env_remove("SSL_CERT_FILE");
        command
    };
    let mut command = bare();
    super::trusting_machine_cas(&mut command, &cas);
    for variable in ["NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE"] {
        assert_eq!(
            given(&command, variable),
            Some(bundle.clone()),
            "{variable}"
        );
    }
    let mut own = bare();
    own.env("SSL_CERT_FILE", &missing);
    super::trusting_machine_cas(&mut own, &cas);
    assert_eq!(given(&own, "SSL_CERT_FILE"), Some(missing.clone()));
    assert_eq!(given(&own, "NODE_EXTRA_CA_CERTS"), Some(bundle.clone()));
    let mut none = bare();
    super::trusting_machine_cas(&mut none, &cas[..1]);
    assert_eq!(given(&none, "SSL_CERT_FILE"), None);
}

/// The harness protocol an adapter built here states is the current one,
/// and an agent refuses one below its oldest by name.
#[test]
fn an_adapter_states_the_harness_protocol_and_an_older_one_is_refused() {
    assert_eq!(Contract::new(true).version, PROTOCOL.current);
    let older = PROTOCOL
        .accept("lnk-old", PROTOCOL.oldest - 1)
        .unwrap_err()
        .to_string();
    assert!(
        older.starts_with("lnk-old speaks the harness protocol") && older.ends_with("lnk upgrade"),
        "{older}"
    );
}
