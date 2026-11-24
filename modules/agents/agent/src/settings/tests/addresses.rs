//! A model at an HTTPS address of its own, a provider's a module adds or
//! a `base_url`: its key goes on a hosted route of its own for that one
//! host, inference alone, never on the provider's route, which would send
//! it to its provider's own address.

use super::*;

const KEY: &str = "ex_key_the-real-one";

fn at_address(url: &str) -> Model {
    Model {
        provider: "openai".into(),
        name: Some("gpt-6-luna".into()),
        api_key: KEY.to_string().into(),
        base_url: Some(url.into()),
        ..Model::default()
    }
}

/// A model of a provider a module adds, as `lnk agent start
/// --model.default link/auto` saves it: tagged `link`, no address of its
/// own.
fn declared_model() -> Model {
    example_models();
    Model {
        provider: "link".into(),
        name: Some("auto".into()),
        api_key: KEY.to_string().into(),
        ..Model::default()
    }
}

/// A declared provider's key on the wire: a route of its own to its declared host
/// alone, the harness handed that route, OpenAI's API and a placeholder;
/// never the provider's route, which sends a key to OpenAI, and never the
/// key.
#[test]
fn a_declared_providers_key_never_reaches_openai() {
    if !own_config() {
        return;
    }
    let declared = example_models();
    let host = declared
        .base_url
        .strip_prefix("https://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut agent = Settings::new("main");
    agent.harness = Some(DEFAULT_HARNESS.into());
    agent.model = Some(declared_model());
    // Nothing on the provider's route.
    assert!(agent.wired_model().unwrap().is_none());
    assert_eq!(agent.wire_key().unwrap(), None);
    let [key] = agent.address_keys().unwrap().try_into().ok().unwrap();
    assert_eq!(key.key, link_model_contract::route::HostedKey::Model);
    assert_eq!(key.provider, declared.speaks);
    let at = key.at.as_ref().unwrap();
    assert_eq!(at.host, host);
    assert_ne!(host, "api.openai.com");
    assert_eq!(at.root, "");
    assert_eq!(
        key.mounted(),
        ("model-at", link_model_contract::route::HOSTED_AT)
    );
    let handed = agent.for_harness(&Rules::new()).unwrap();
    assert_eq!(handed.model.provider, declared.speaks);
    assert_eq!(handed.model.name.as_deref(), Some("auto"));
    assert_eq!(
        handed.model.base_url,
        Some(format!(
            "http://127.0.0.1:{}/_link/model-at/v1",
            agent.proxy_port()
        ))
    );
    assert_eq!(handed.model.api_key, key.placeholder);
    let all = serde_json::to_string(&handed).unwrap();
    assert!(!all.contains(KEY), "{all}");
    // No typed model actions: those are the provider's route's.
    assert_eq!(handed.model.actions_url, None);
}

/// Beside a hosted fallback, each keeps its own way: the fallback the
/// model route's one key, the model its own secret.
#[test]
fn a_hosted_fallback_keeps_the_model_route() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.harness = Some(DEFAULT_HARNESS.into());
    agent.model = Some(declared_model());
    agent.fallback = Some(Model {
        provider: "anthropic".into(),
        api_key: "sk-ant-api03-secret".to_string().into(),
        name: Some("claude-haiku-5-5".into()),
        ..Model::default()
    });
    assert_eq!(
        agent.wire_key().unwrap().unwrap().key,
        link_model_contract::route::HostedKey::Fallback
    );
    assert_eq!(agent.fallback_unwired().unwrap(), None);
    assert_eq!(agent.address_keys().unwrap().len(), 1);
    let handed = agent.for_harness(&Rules::new()).unwrap();
    assert!(handed.model.base_url.unwrap().contains("/_link/model-at/"));
    assert_eq!(
        handed.fallback.unwrap().base_url,
        link_model_contract::route::url("anthropic", agent.proxy_port())
    );

    // A fallback at the address too: a route of its own, by its key.
    agent.fallback = Some(at_address("https://models.example/v1"));
    let mounted: Vec<_> = agent
        .address_keys()
        .unwrap()
        .iter()
        .map(|key| key.mounted().0)
        .collect();
    assert_eq!(mounted, ["model-at", "model-at-fallback"]);
    assert_eq!(agent.wire_key().unwrap(), None);
    let fallback = agent.for_harness(&Rules::new()).unwrap().fallback.unwrap();
    assert_eq!(
        fallback.base_url,
        Some(format!(
            "http://127.0.0.1:{}/_link/model-at-fallback/v1",
            agent.proxy_port()
        ))
    );
    assert_ne!(fallback.api_key.expose(), KEY);
}

/// An address the proxy can't hold a key to is refused, without the
/// key; with keys in the harness, it's handed as it is.
#[test]
fn an_address_the_proxy_cant_hold_to_is_refused() {
    if !own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.harness = Some(DEFAULT_HARNESS.into());
    agent.model = Some(at_address("http://models.example/v1"));
    let err = agent.for_harness(&Rules::new()).unwrap_err();
    assert!(!format!("{err:#}").contains(KEY), "{err:#}");
    assert!(agent.address_keys().is_err());
    assert_eq!(agent.wire_key().unwrap(), None);
    // An OpenAI address whose API isn't under its /v1: refused too, since
    // its proxy couldn't pass it inference alone.
    agent.model = Some(at_address("https://models.example/openai"));
    assert!(agent.address_keys().is_err());
    agent.model = Some(at_address("http://models.example/v1"));
    agent.wire_keys = Some(false);
    assert!(agent.address_keys().unwrap().is_empty());
    let handed = agent.for_harness(&Rules::new()).unwrap();
    assert_eq!(
        handed.model.base_url.as_deref(),
        Some("http://models.example/v1")
    );
    assert_eq!(handed.model.api_key.expose(), KEY);
    // The declared provider's, with keys in the harness: at its declared address.
    agent.model = Some(declared_model());
    let handed = agent.for_harness(&Rules::new()).unwrap();
    let declared = example_models();
    assert_eq!(handed.model.provider, declared.speaks);
    assert_eq!(handed.model.base_url.as_ref(), Some(&declared.base_url));
}
