//! Hosted models' providers and their keys: from a key's prefix, and
//! from the provider's own variable or the saved one.

use super::*;

/// The port of a `http://localhost:<port>` (or 127.0.0.1, [::1]) address.
pub fn local_port(url: &str) -> Option<u16> {
    let url = reqwest::Url::parse(url).ok()?;
    let local = matches!(url.host_str()?, "localhost" | "127.0.0.1" | "[::1]" | "::1");
    local.then(|| url.port_or_known_default()).flatten()
}

/// The provider a key belongs to, from its prefix.
pub fn provider_of(key: &str) -> Option<&'static str> {
    if key.starts_with("sk-ant-") {
        Some("anthropic")
    } else if key.starts_with("sk-or-") {
        Some("openrouter")
    } else if key.starts_with("sk-") {
        Some("openai")
    } else {
        None
    }
}

/// The variable provider `provider`'s key is read from.
pub fn key_var(provider: &str) -> Option<&'static str> {
    KEY_VARS
        .iter()
        .find(|(p, _)| *p == provider)
        .map(|(_, var)| *var)
}

/// Hosted provider `provider`'s key, without asking: its own variable's,
/// else the key saved with a model of that provider. Never another
/// provider's, since a model's name says whom it's sent to.
pub fn key_for_provider(provider: &str, saved: Option<&Model>) -> Option<Secret> {
    key_for_provider_in(provider, saved, |var| {
        Secret::from_env(var).map(Secret::into_exposed)
    })
}

pub(super) fn key_for_provider_in(
    provider: &str,
    saved: Option<&Model>,
    get: impl Fn(&str) -> Option<String>,
) -> Option<Secret> {
    let from_env = key_var(provider)
        .and_then(&get)
        .map(|key| Secret::from(key.trim().to_string()))
        .filter(|key| !key.is_empty());
    from_env.or_else(|| {
        saved
            .filter(|m| m.provider == provider && !m.api_key.is_empty())
            .map(|m| m.api_key.duplicate())
    })
}

/// Each hosted provider with the variable its own tools read its key
/// from.
const KEY_VARS: [(&str, &str); 3] = [
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("openai", "OPENAI_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
];

/// The variables a hosted model's key can come from, for what we say.
pub const KEY_VARS_TEXT: &str = "ANTHROPIC_API_KEY, OPENAI_API_KEY or OPENROUTER_API_KEY";

/// Every variable a hosted model's key can come from.
pub fn key_vars() -> impl Iterator<Item = &'static str> {
    KEY_VARS.iter().map(|(_, var)| *var)
}

/// A hosted model's key from the provider's own variable. For an agent
/// with a hosted model, its provider's variable (so a new key renews it);
/// for one with a model on this computer, none; for one with no model, the
/// only one of the three that is set, and an error if several are, since
/// Link won't pick whom to bill.
pub fn key_from_env(model: Option<&Model>) -> anyhow::Result<Option<Secret>> {
    key_in(model, |var| Secret::from_env(var).map(Secret::into_exposed))
}

pub(super) fn key_in(
    model: Option<&Model>,
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<Secret>> {
    let set = |var: &str| {
        get(var)
            .map(|key| Secret::from(key.trim().to_string()))
            .filter(|key| !key.is_empty())
    };
    if let Some(model) = model {
        return Ok(KEY_VARS
            .iter()
            .find(|(provider, _)| *provider == model.provider)
            .and_then(|(_, var)| set(var)));
    }
    let found: Vec<_> = KEY_VARS
        .iter()
        .filter_map(|(_, var)| set(var).map(|key| (*var, key)))
        .collect();
    match found.as_slice() {
        [] => Ok(None),
        [(_, key)] => Ok(Some(key.duplicate())),
        several => bail!(
            "{} are all set: leave set only the one to use",
            several
                .iter()
                .map(|(var, _)| *var)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
