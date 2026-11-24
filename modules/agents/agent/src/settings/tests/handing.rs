//! What the harness is handed behind its proxy: memory, the proxy's
//! addresses, a fallback and placeholder tokens on the wire.

use super::*;

#[test]
fn behind_its_proxy_the_harness_is_handed_its_memory() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.model = Some(Model {
        provider: "anthropic".into(),
        api_key: "sk-ant-api03-secret".to_string().into(),
        ..Model::default()
    });
    agent.upstream = Some("direct".into());
    // No memory: nothing to hand it.
    assert_eq!(agent.for_harness(&Rules::new()).unwrap().memory, None);
    agent.memory = Some(DEFAULT_MEMORY.into());
    // At its proxy, behind the agent's secret.
    assert_eq!(
        agent.for_harness(&Rules::new()).unwrap().memory,
        Some(format!(
            "http://127.0.0.1:{}/_link/url/{}/mcp/memory-main",
            agent.proxy_port(),
            agent.url_secret().unwrap()
        ))
    );
    // With no sandbox, at its proxy beside it, on its own port of this
    // machine's loopback.
    agent.upstream = None;
    agent.sandbox = false;
    agent.port = Some(4000);
    let port = agent.proxy_port();
    assert_eq!(port, 4000 + agent.block() - 1);
    assert_eq!(
        agent.for_harness(&Rules::new()).unwrap().memory,
        Some(format!(
            "http://127.0.0.1:{port}/_link/url/{}/mcp/memory-main",
            agent.url_secret().unwrap()
        ))
    );
    // Without its proxy, the harness can't reach it: sandboxed on the
    // network as it is.
    agent.sandbox = true;
    Grants {
        spec: link_spec::parse("[network]\nmode = \"host\"\n").unwrap(),
        ..Grants::default()
    }
    .save(agent.harness())
    .unwrap();
    assert_eq!(agent.for_harness(&Rules::new()).unwrap().memory, None);
}

#[test]
fn behind_its_proxy_the_harness_gets_the_proxys_addresses() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.upstream = Some("direct".into());
    agent.model = Some(Model {
        provider: "ollama".into(),
        name: Some("qwen3:8b".into()),
        base_url: Some("http://localhost:11434".into()),
        ..Model::default()
    });
    let model = agent.for_harness(&Rules::new()).unwrap().model;
    // On a Mac, on its proxy, behind the agent's URL secret.
    let expected = if cfg!(target_os = "macos") {
        link_model_contract::route::local_url(40781, 11434, &agent.url_secret().unwrap())
    } else {
        "http://localhost:11434".to_string()
    };
    assert_eq!(model.base_url, Some(expected));
    agent.port = Some(20200);
    let port = if cfg!(target_os = "macos") {
        20399
    } else {
        40781
    };
    assert_eq!(agent.proxy_port(), port);

    agent.model = Some(Model {
        provider: "anthropic".into(),
        api_key: "sk-ant-api03-secret".to_string().into(),
        ..Model::default()
    });
    // The agent's own choice: the harness holds the key.
    agent.wire_keys = Some(false);
    assert_eq!(
        agent.for_harness(&Rules::new()).unwrap().model.api_key,
        link_secretive::Secret::from("sk-ant-api03-secret".to_string())
    );
    // A harness not shown to take the proxy's address (one from
    // outside Link) holds it unless the agent says otherwise.
    agent.wire_keys = None;
    agent.harness = Some("my-harness".into());
    assert_eq!(
        agent.for_harness(&Rules::new()).unwrap().model.api_key,
        link_secretive::Secret::from("sk-ant-api03-secret".to_string())
    );
    agent.wire_keys = Some(true);
    assert_eq!(
        agent.for_harness(&Rules::new()).unwrap().model.api_key,
        link_model_contract::route::placeholder(
            &agent.placeholders().unwrap(),
            &"sk-ant-api03-secret".to_string().into()
        )
    );
    // Link Harness: on the wire by default.
    agent.wire_keys = None;
    agent.harness = None;
    let model = agent.for_harness(&Rules::new()).unwrap().model;
    assert_eq!(
        model.api_key,
        link_model_contract::route::placeholder(
            &agent.placeholders().unwrap(),
            &"sk-ant-api03-secret".to_string().into()
        )
    );
    assert!(
        model
            .api_key
            .expose()
            .starts_with("sk-ant-api03-link-wire-")
    );
    assert_eq!(
        model.base_url,
        Some(format!("http://127.0.0.1:{port}/_link/model"))
    );
    // With no route of its own, still behind its proxy; on the network
    // as it is, not.
    agent.upstream = None;
    assert_eq!(
        agent.for_harness(&Rules::new()).unwrap().model.api_key,
        link_model_contract::route::placeholder(
            &agent.placeholders().unwrap(),
            &"sk-ant-api03-secret".to_string().into()
        )
    );
    Grants {
        spec: link_spec::parse("[network]\nmode = \"host\"\n").unwrap(),
        ..Grants::default()
    }
    .save(DEFAULT_HARNESS)
    .unwrap();
    assert_eq!(
        agent.for_harness(&Rules::new()).unwrap().model.api_key,
        link_secretive::Secret::from("sk-ant-api03-secret".to_string())
    );
}

#[test]
fn a_fallback_is_handed_on_the_wire_as_the_model_is() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.harness = Some(DEFAULT_HARNESS.into());
    // A model here, and a hosted fallback: the proxy holds its key.
    agent.model = Some(Model {
        provider: "ollama".into(),
        name: Some("qwen3".into()),
        base_url: Some("http://localhost:11434".into()),
        ..Model::default()
    });
    agent.fallback = Some(Model {
        provider: "openrouter".into(),
        api_key: "sk-or-v1-secret".to_string().into(),
        name: Some("qwen/qwen3-32b".into()),
        ..Model::default()
    });
    // Its fallback's, by name: the route holds no key.
    assert_eq!(
        agent.wire_key().unwrap().unwrap().key,
        link_model_contract::route::HostedKey::Fallback
    );
    assert_eq!(agent.fallback_unwired().unwrap(), None);
    let handed = agent.for_harness(&Rules::new()).unwrap();
    // The model here as it was; the fallback at the proxy, keyless.
    assert_eq!(handed.model.provider, "ollama");
    assert!(handed.model.api_key.is_empty());
    assert_ne!(
        handed.model.base_url,
        link_model_contract::route::url("openrouter", agent.proxy_port())
    );
    let fallback = handed.fallback.unwrap();
    assert_eq!(fallback.name.as_deref(), Some("qwen/qwen3-32b"));
    assert_eq!(
        fallback.base_url,
        link_model_contract::route::url("openrouter", agent.proxy_port())
    );
    assert_eq!(
        fallback.api_key,
        link_model_contract::route::placeholder(
            &agent.placeholders().unwrap(),
            &"sk-or-v1-secret".to_string().into()
        )
    );
    assert!(
        !serde_json::to_string(&agent.for_harness(&Rules::new()).unwrap())
            .unwrap()
            .contains("sk-or-v1-secret")
    );

    // A hosted model and a fallback of another key: the proxy holds
    // one, so it's refused rather than handed to the harness.
    agent.model = Some(Model {
        provider: "anthropic".into(),
        api_key: "sk-ant-api03-secret".to_string().into(),
        ..Model::default()
    });
    assert_eq!(
        agent.wire_key().unwrap().unwrap().key,
        link_model_contract::route::HostedKey::Model
    );
    assert!(agent.fallback_unwired().unwrap().is_some());
    assert!(agent.for_harness(&Rules::new()).is_err());
    // Keys in the harness: both handed as they are.
    agent.wire_keys = Some(false);
    assert_eq!(agent.fallback_unwired().unwrap(), None);
    let handed = agent.for_harness(&Rules::new()).unwrap();
    assert_eq!(
        handed.fallback.unwrap().api_key,
        link_secretive::Secret::from("sk-or-v1-secret".to_string())
    );
    // None: none handed.
    agent.fallback = None;
    assert_eq!(agent.for_harness(&Rules::new()).unwrap().fallback, None);
}

#[test]
fn with_keys_on_the_wire_the_harness_holds_a_placeholder_bot_token() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.harness = Some(DEFAULT_HARNESS.into());
    agent.model = Some(Model {
        provider: "anthropic".into(),
        api_key: "sk-ant-api03-secret".to_string().into(),
        ..Model::default()
    });
    agent.connections.insert(
        "telegram".into(),
        Connection {
            settings: serde_json::json!({"bot_token": "123456:AAH-secret", "allowed_users": [42]})
                .into(),
            ..Connection::default()
        },
    );
    // Not asked for its rule: never handed as it is.
    assert!(agent.for_harness(&Rules::new()).is_err());
    assert!(agent.wire_secrets(&Rules::new(), &[]).is_err());
    let rules = bot_rules();
    let telegram = agent.for_harness(&rules).unwrap().telegram.unwrap();
    assert_eq!(
        telegram.bot_token,
        link_secret_contract::route::placeholder(
            &agent.placeholders().unwrap(),
            "telegram",
            &"123456:AAH-secret".to_string().into(),
            Some(":")
        )
    );
    assert!(!telegram.bot_token.expose().contains("secret"));
    assert_eq!(
        telegram.api_root,
        Some(link_secret_contract::route::url(
            agent.proxy_port(),
            "telegram"
        ))
    );
    let secrets = agent.wire_secrets(&rules, &[]).unwrap();
    assert_eq!(secrets.len(), 1);
    assert_eq!(
        (
            secrets[0].named.name.as_str(),
            secrets[0].value.expose().as_str()
        ),
        ("telegram", "123456:AAH-secret")
    );
    // Its route names the token, its connection's own, never holds it.
    assert_eq!(
        secrets[0].named.kept,
        link_secret_contract::route::Kept::Own(
            link_agent_identity::own::OwnSecret::Connection {
                connection: "telegram".into(),
                field: rules["telegram"].clone().unwrap().field,
            }
            .to_use()
            .unwrap()
        )
    );
    assert_eq!(
        secrets[0].named.rule,
        rules["telegram"].clone().unwrap().wire
    );
    // What its proxy checks is what its harness holds, until its next
    // start makes new ones.
    assert_eq!(secrets[0].named.placeholder, telegram.bot_token);
    agent.new_placeholders().unwrap();
    let again = agent.for_harness(&rules).unwrap().telegram.unwrap();
    assert_ne!(again.bot_token, telegram.bot_token);
    assert_eq!(
        agent.wire_secrets(&rules, &[]).unwrap()[0]
            .named
            .placeholder,
        again.bot_token
    );
    // A connection Link carries: the harness never gets it, nor its proxy.
    assert!(
        agent
            .wire_secrets(&rules, &["telegram".into()])
            .unwrap()
            .is_empty()
    );
    // A connection without a rule: the harness holds its secret.
    let unruled = Rules::from([("telegram".to_string(), None)]);
    assert!(agent.wire_secrets(&unruled, &[]).unwrap().is_empty());
    let telegram = agent.for_harness(&unruled).unwrap().telegram.unwrap();
    assert_eq!(
        telegram.bot_token,
        link_secretive::Secret::from("123456:AAH-secret".to_string())
    );
    // The agent's own choice, or the network as it is: the harness
    // holds it, and no
    // rule is needed.
    agent.wire_keys = Some(false);
    let telegram = agent.for_harness(&Rules::new()).unwrap().telegram.unwrap();
    assert_eq!(
        telegram.bot_token,
        link_secretive::Secret::from("123456:AAH-secret".to_string())
    );
    assert_eq!(telegram.api_root, None);
    assert!(agent.wire_secrets(&rules, &[]).unwrap().is_empty());
    agent.wire_keys = None;
    Grants {
        spec: link_spec::parse("[network]\nmode = \"host\"\n").unwrap(),
        ..Grants::default()
    }
    .save(DEFAULT_HARNESS)
    .unwrap();
    assert!(agent.wire_secrets(&rules, &[]).unwrap().is_empty());
}

/// Telegram's rule, as `lnk-telegram rule --json` gives it.
fn bot_rules() -> Rules {
    let rule = serde_json::from_value(serde_json::json!({
        "field": "bot_token",
        "root": "api_root",
        "host": "api.telegram.org",
        "paths": [
            {"prefix": "bot{secret}/", "rest": "name"},
            {"prefix": "file/bot{secret}/", "rest": "path"},
        ],
        "refuse": ["bot{secret}/setwebhook", "bot{secret}/logout", "bot{secret}/close"],
        "public": ":",
    }))
    .unwrap();
    Rules::from([("telegram".to_string(), Some(rule))])
}

#[test]
fn every_sandboxed_harness_with_network_is_behind_its_proxy() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    let harness = DEFAULT_HARNESS;
    assert_eq!(agent.proxy_of(harness).as_deref(), Some("direct"));
    // With its harness's need of the network as it is, no proxy.
    Grants {
        spec: link_spec::parse("[network]\nmode = \"host\"\n").unwrap(),
        ..Grants::default()
    }
    .save(harness)
    .unwrap();
    assert_eq!(agent.proxy_of(harness), None);
    // Without network, which you took from the agent: its proxy still,
    // reaching no host, which serves its harness's own port and carries
    // Link's ways in, so it starts.
    Grants::default().save(harness).unwrap();
    let only_tools =
        Sandbox::parse("schema = 1\n[commands.macos]\ndeveloper_tools = true\n").unwrap();
    crate::sandbox_file::save("main", &only_tools).unwrap();
    assert_eq!(agent.proxy_of(harness).as_deref(), Some("direct"));
    // The agent's own network as it is: the same, on any harness.
    let host =
        Sandbox::parse("schema = 1\n[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n")
            .unwrap();
    crate::sandbox_file::save("main", &host).unwrap();
    assert_eq!(agent.proxy_of("other"), None);
    // A route (an exit) always keeps it to its proxy.
    agent.upstream = Some("socks5://127.0.0.1:1080".into());
    assert_eq!(
        agent.proxy_of(harness).as_deref(),
        Some("socks5://127.0.0.1:1080")
    );
    // No sandbox: a proxy beside it, whatever its spec, never an exit.
    let mut open = Settings::new("main");
    open.sandbox = false;
    assert_eq!(open.proxy_of(harness).as_deref(), Some("direct"));
    assert!(open.keys_wired().unwrap());
}

/// Each start makes the URLs' secret new: what's handed after holds the
/// new one, and an address read before carries one no longer kept.
#[test]
fn each_start_makes_its_urls_secret_new() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.upstream = Some("direct".into());
    agent.memory = Some(DEFAULT_MEMORY.into());
    let before = agent.url_secret().unwrap();
    assert_eq!(agent.url_secret().unwrap(), before, "kept within a start");
    let read = agent.memory_url().unwrap();
    let new = agent.new_url_secret().unwrap();
    assert_ne!(new, before);
    assert_eq!(agent.url_secret().unwrap(), new);
    let now = agent.memory_url().unwrap();
    assert!(now.contains(&new) && !read.contains(&new), "{read} {now}");
}
