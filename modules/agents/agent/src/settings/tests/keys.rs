//! Hosted models' providers and keys: from a key's prefix, a model's
//! name, the provider's own variable or the saved one.

use super::*;

#[test]
fn a_hosted_models_key_comes_from_its_providers_variable_or_the_saved_one() {
    let local = Model {
        actions_url: None,
        provider: "ollama".into(),
        api_key: link_secretive::Secret::default(),
        name: Some("qwen3:8b".into()),
        base_url: Some("http://127.0.0.1:11434".into()),
    };
    let anthropic = Model {
        actions_url: None,
        provider: "anthropic".into(),
        api_key: "sk-ant-saved".to_string().into(),
        name: Some("claude-haiku-5-5".into()),
        base_url: None,
    };
    let env = |var: &str| (var == "ANTHROPIC_API_KEY").then(|| "sk-ant-env".to_string());
    let none = |_: &str| None;
    // From a local model, the variable of the provider it's sent to.
    assert_eq!(
        key_for_provider_in("anthropic", Some(&local), env),
        Some("sk-ant-env".to_string().into())
    );
    // The variable over the saved key.
    assert_eq!(
        key_for_provider_in("anthropic", Some(&anthropic), env),
        Some("sk-ant-env".to_string().into())
    );
    // Another of the same provider: the saved key, without a variable.
    assert_eq!(
        key_for_provider_in("anthropic", Some(&anthropic), none),
        Some("sk-ant-saved".to_string().into())
    );
    // Another provider's: never the saved key, nor another's variable.
    assert_eq!(key_for_provider_in("openai", Some(&anthropic), none), None);
    assert_eq!(key_for_provider_in("openai", Some(&anthropic), env), None);
    assert_eq!(key_for_provider_in("openrouter", None, env), None);
}

#[test]
fn providers_come_from_the_key() {
    assert_eq!(provider_of("sk-ant-api03-x"), Some("anthropic"));
    assert_eq!(provider_of("sk-or-v1-x"), Some("openrouter"));
    assert_eq!(provider_of("sk-proj-x"), Some("openai"));
    assert_eq!(provider_of("AIza..."), None);
}

#[test]
fn keys_come_from_the_providers_own_variables() {
    let vars = |set: &'static [(&'static str, &'static str)]| {
        move |var: &str| {
            set.iter()
                .find(|(v, _)| *v == var)
                .map(|(_, k)| k.to_string())
        }
    };
    let anthropic = Model {
        provider: "anthropic".into(),
        ..Model::default()
    };
    let local = Model {
        provider: "ollama".into(),
        ..Model::default()
    };
    // Nothing set: none.
    assert_eq!(key_in(None, vars(&[])).unwrap(), None);
    // An empty one is none too.
    assert_eq!(
        key_in(None, vars(&[("OPENAI_API_KEY", " ")])).unwrap(),
        None
    );
    // One set: that one, whichever provider.
    assert_eq!(
        key_in(None, vars(&[("OPENAI_API_KEY", "sk-1")])).unwrap(),
        Some("sk-1".to_string().into())
    );
    // Several set, and no model: Link won't choose.
    let both = &[
        ("ANTHROPIC_API_KEY", "sk-ant-1"),
        ("OPENAI_API_KEY", "sk-1"),
    ];
    let err = key_in(None, vars(both)).unwrap_err().to_string();
    assert!(err.contains("ANTHROPIC_API_KEY, OPENAI_API_KEY"), "{err}");
    // A hosted model takes its own provider's, not another's.
    assert_eq!(
        key_in(Some(&anthropic), vars(both)).unwrap(),
        Some("sk-ant-1".to_string().into())
    );
    assert_eq!(
        key_in(Some(&anthropic), vars(&[("OPENAI_API_KEY", "sk-1")])).unwrap(),
        None
    );
    // A model on this computer takes none.
    assert_eq!(key_in(Some(&local), vars(both)).unwrap(), None);
}
