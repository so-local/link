//! The model routes, which `lnk-model`' bridge serves: a hosted
//! model's (`/_link/model`), its key added outside the sandbox, and this
//! machine's model runtimes' (`/_link/here/<port>`, and their ports on
//! Linux), for inference only. The providers, their keys' shapes and the
//! URLs a program is given for them are here; how each provider is
//! spoken to is the bridge's.

use serde::{Deserialize, Serialize};

use link_sandbox_route::{Bridge, Placeholders, Route};
use link_secretive::Duplicate;

/// This machine's model runtimes' route, and the bridge that serves
/// the model routes, are the sandbox's routes' ([`link_sandbox_route::local_model`]).
pub use link_sandbox_route::local_model::{LOCAL, Local, bridge, local, local_url};

/// Where a hosted model is on its proxy.
pub const HOSTED: &str = "/_link/model";

/// Where a hosted model at an address of its own ([`At`]) is on its
/// proxy: its model's, and its fallback's.
pub const HOSTED_AT: &str = "/_link/model-at";
pub const HOSTED_AT_FALLBACK: &str = "/_link/model-at-fallback";

/// Where a hosted model at an address of its own is reached: a declared
/// provider's address ([`super::providers`]), or a `base_url`. One host over HTTPS, and
/// its API's root before the `/v1` the provider it speaks takes, so the
/// bridge passes it inference alone, as it does the provider's own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct At {
    /// Its one host (`gw.example`).
    pub host: String,
    /// Its API's path before `/v1`, empty or `/<part>/...`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root: String,
}

impl At {
    /// `base_url`, spoken as `provider` speaks: an error for an address
    /// the bridge can't hold a key to (plain HTTP, a port of its own, a
    /// user, a query or fragment, an IP address, this machine) or whose
    /// API isn't where its provider's clients look: OpenAI's and
    /// OpenRouter's under the `/v1` it ends with, Anthropic's below the
    /// `/v1` its clients add.
    pub fn new(provider: &str, base_url: &str) -> anyhow::Result<At> {
        let refused = || format!("{base_url:?} isn't an address over HTTPS on its own port");
        let rest = base_url
            .strip_prefix("https://")
            .ok_or_else(|| anyhow::anyhow!(refused()))?;
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = host.to_ascii_lowercase();
        anyhow::ensure!(
            !host.is_empty()
                && !host.contains([':', '@', '[', '*'])
                && !rest.contains(['?', '#'])
                && host.parse::<std::net::IpAddr>().is_err()
                && host != "localhost"
                && host.contains('.')
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-.".contains(&b)),
            "{}",
            refused()
        );
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        anyhow::ensure!(
            parts.iter().all(|p| {
                *p != "."
                    && *p != ".."
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            }),
            "{}",
            refused()
        );
        let root = match (provider, parts.split_last()) {
            ("anthropic", Some((&"v1", _))) => anyhow::bail!(
                "{base_url:?} ends with /v1, which Anthropic's clients add: give it without"
            ),
            ("anthropic", _) => parts.as_slice(),
            ("openai" | "openrouter", Some((&"v1", before))) => before,
            ("openai" | "openrouter", _) => anyhow::bail!(
                "{base_url:?} doesn't end with the /v1 {provider}'s API is under, so its proxy \
                 can't pass it inference alone"
            ),
            _ => anyhow::bail!("{provider} isn't a provider Link adds keys for"),
        };
        Ok(At {
            host,
            root: root.iter().map(|p| format!("/{p}")).collect(),
        })
    }
}

/// What the hosted route's bridge is given: which key. An agent's route
/// names it, and the bridge reads it from what the store grants the
/// route's caller at each call ([`Hosted::with_key`]), so a key removed
/// stops at the next; a bind made at once, never written, may hand it.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hosted {
    /// `anthropic`, `openai` or `openrouter`.
    pub provider: String,
    #[duplicate]
    pub key: HostedKey,
    /// What the program holds in its place ([`placeholder`]).
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub placeholder: link_secretive::Secret,
    /// [`ModelKey::server_tools`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub server_tools: bool,
    /// [`ModelKey::at`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
}

/// Which key a hosted route adds: the caller's model's or its
/// fallback's, by name; or one handed with a bind made at once, which a
/// routes file never holds.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostedKey {
    /// Its model's (`lnk agent start --model.default`).
    #[default]
    Model,
    /// Its fallback model's (`lnk agent start --model.fallback_model`).
    Fallback,
    /// This key.
    Handed(
        #[serde(with = "link_secretive::bare")]
        #[duplicate]
        link_secretive::Secret,
    ),
}

impl Hosted {
    /// The route for the agent's key `of` (named; `key` read only for its
    /// placeholder), with its placeholder among `placeholders`.
    pub fn new(
        placeholders: &Placeholders,
        provider: &str,
        of: HostedKey,
        key: &link_secretive::Secret,
        server_tools: bool,
    ) -> Self {
        Hosted {
            provider: provider.to_string(),
            key: of,
            placeholder: placeholder(placeholders, key),
            server_tools,
            at: None,
        }
    }

    /// It at an address of its own, `at`, not its provider's.
    pub fn at(self, at: At) -> Self {
        Hosted {
            at: Some(at),
            ..self
        }
    }

    /// Its route's name and where it is on its proxy: the provider's
    /// route, or, at an address of its own, its model's or its
    /// fallback's.
    pub fn mounted(&self) -> (&'static str, &'static str) {
        match (&self.at, &self.key) {
            (None, _) => ("model", HOSTED),
            (Some(_), HostedKey::Fallback) => ("model-at-fallback", HOSTED_AT_FALLBACK),
            (Some(_), _) => ("model-at", HOSTED_AT),
        }
    }

    /// The address its program is given on a proxy on `port`, as its
    /// provider's clients take one.
    pub fn url(&self, port: u16) -> Option<String> {
        let (_, prefix) = self.mounted();
        let base = format!("http://127.0.0.1:{port}{prefix}");
        match self.provider.as_str() {
            "anthropic" => Some(base),
            "openai" | "openrouter" => Some(format!("{base}/v1")),
            _ => None,
        }
    }

    /// It as the bridge adds it, with `key`, the caller's when it's named:
    /// an error unless the placeholder was made for that key.
    pub fn with_key(&self, key: link_secretive::Secret) -> anyhow::Result<ModelKey> {
        let key = ModelKey {
            provider: self.provider.clone(),
            key,
            placeholder: self.placeholder.duplicate(),
            server_tools: self.server_tools,
            at: self.at.clone(),
        };
        key.check_placeholder()?;
        Ok(key)
    }

    /// Its key, handed: never in a file.
    pub fn handed(&self) -> Option<ModelKey> {
        match &self.key {
            HostedKey::Handed(key) => self.with_key(key.duplicate()).ok(),
            _ => None,
        }
    }
}

impl From<ModelKey> for Hosted {
    /// `key`, handed with a bind made at once.
    fn from(key: ModelKey) -> Self {
        Hosted {
            provider: key.provider,
            key: HostedKey::Handed(key.key),
            placeholder: key.placeholder,
            server_tools: key.server_tools,
            at: key.at,
        }
    }
}

/// The hosted route of `hosted`, served by `bridge`.
pub fn hosted(bridge: Bridge, hosted: Hosted) -> anyhow::Result<Route> {
    anyhow::ensure!(
        PROVIDERS.contains(&hosted.provider.as_str()),
        "{} isn't a provider Link adds keys for",
        hosted.provider
    );
    let handed = matches!(hosted.key, HostedKey::Handed(_));
    let (name, prefix) = hosted.mounted();
    let given = serde_json::to_string(&hosted)?;
    Ok(Route {
        name: name.into(),
        prefix: prefix.into(),
        bridge,
        given: given.into(),
        handed,
        ..Route::default()
    })
}

/// A hosted provider's key as the bridge adds it to inference requests:
/// the program holds only its placeholder ([`ModelKey::new`]).
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelKey {
    /// `anthropic`, `openai` or `openrouter`.
    pub provider: String,
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub key: link_secretive::Secret,
    /// What the program holds in its place
    /// ([`placeholder`]): the
    /// route adds the key only for a request showing it.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub placeholder: link_secretive::Secret,
    /// Whether the provider's own tools that run on its servers (web
    /// search, web fetch, code execution, MCP servers) are left in
    /// requests. Off: removed, since they reach the internet for the
    /// program.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub server_tools: bool,
    /// The address of its own it's at, not its provider's: one host, its
    /// inference alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
}

/// The hosted providers the model route adds keys for.
pub const PROVIDERS: &[&str] = &["anthropic", "openai", "openrouter"];

/// The header a hosted model's key goes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyHeader {
    /// `x-api-key: <key>`: Anthropic's API keys.
    ApiKey,
    /// `Authorization: Bearer <key>`: every other key, Anthropic's OAuth
    /// tokens too.
    Bearer,
}

impl KeyHeader {
    /// Where `provider`'s `key` goes.
    pub fn of(provider: &str, key: &link_secretive::Secret) -> KeyHeader {
        match provider == "anthropic" && !key.expose().starts_with("sk-ant-oat") {
            true => KeyHeader::ApiKey,
            false => KeyHeader::Bearer,
        }
    }

    /// The header's name.
    pub fn name(self) -> &'static str {
        match self {
            KeyHeader::ApiKey => "x-api-key",
            KeyHeader::Bearer => "Authorization",
        }
    }

    /// What comes before the key in its value.
    pub fn prefix(self) -> &'static str {
        match self {
            KeyHeader::ApiKey => "",
            KeyHeader::Bearer => "Bearer ",
        }
    }
}

/// Whether `model` at `provider` searches the web whatever a request
/// asks: Perplexity's Sonar models, and OpenAI's search and deep
/// research models, directly or on OpenRouter. Its provider fetches
/// pages for the program, past any list of hosts its sandbox has.
pub fn searches_by_nature(provider: &str, model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    let model = model
        .split_once(':')
        .map_or(model.as_str(), |(base, _)| base);
    let name = match provider {
        "openai" => model,
        "openrouter" => match model.split_once('/') {
            Some(("perplexity", _)) => return true,
            Some(("openai", name)) => name,
            _ => return false,
        },
        _ => return false,
    };
    name.contains("-search-") || name.ends_with("-search") || name.contains("deep-research")
}

/// Where a program behind a proxy reaches `provider`, its proxy on
/// `port` ([`crate::sandbox::PROXY_PORT`] on Linux, `Proxy.listen` on
/// macOS): the base URL its client takes in place of the provider's
/// (`None`: not one the route knows). Anthropic's clients add `/v1`,
/// OpenAI's and OpenRouter's don't.
pub fn url(provider: &str, port: u16) -> Option<String> {
    let base = format!("http://127.0.0.1:{port}{HOSTED}");
    match provider {
        "anthropic" => Some(base),
        "openai" | "openrouter" => Some(format!("{base}/v1")),
        _ => None,
    }
}

impl ModelKey {
    /// `key` of `provider` for the proxy to add, with its placeholder
    /// among `placeholders`.
    pub fn new(
        placeholders: &Placeholders,
        provider: &str,
        key: link_secretive::Secret,
        server_tools: bool,
    ) -> Self {
        ModelKey {
            provider: provider.to_string(),
            placeholder: placeholder(placeholders, &key),
            key,
            server_tools,
            at: None,
        }
    }

    /// An error unless its placeholder is one Link made for its key
    /// ([`check_placeholder`](link_sandbox_route::check_placeholder)).
    pub fn check_placeholder(&self) -> anyhow::Result<()> {
        link_sandbox_route::check_placeholder(&self.placeholder, key_prefix(&self.key))
    }
}

/// What a program is given in place of a hosted model's `key`, one of
/// `placeholders`: its kind's prefix (`sk-ant-api03-`, `sk-or-v1-`,
/// `sk-proj-`), which clients look at, and a tag, none of the secret.
pub fn placeholder(
    placeholders: &Placeholders,
    key: &link_secretive::Secret,
) -> link_secretive::Secret {
    placeholders.placeholder(key_prefix(key), "model")
}

/// What a placeholder for a hosted model's `key` starts with: its kind's
/// prefix, which clients look at, or nothing.
pub fn key_prefix(key: &link_secretive::Secret) -> &'static str {
    const PREFIXES: &[&str] = &[
        "sk-ant-api03-",
        "sk-ant-oat01-",
        "sk-or-v1-",
        "sk-proj-",
        "sk-svcacct-",
        "sk-",
    ];
    let key = key.expose();
    PREFIXES.iter().find(|p| key.starts_with(*p)).unwrap_or(&"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_keeps_the_kind_not_the_key() {
        let made = Placeholders::new().unwrap();
        let tag = |p: &link_secretive::Secret| p.expose().rsplit('-').next().unwrap().to_string();
        for (key, prefix) in [
            ("sk-ant-api03-Ab-cd", "sk-ant-api03-link-wire-"),
            ("sk-or-v1-0123", "sk-or-v1-link-wire-"),
            ("sk-proj-a-b-c", "sk-proj-link-wire-"),
            ("sk-abc-def", "sk-link-wire-"),
            ("other", "link-wire-"),
        ] {
            let placeholder = placeholder(&made, &key.to_string().into());
            assert!(placeholder.expose().starts_with(prefix), "{key}");
            assert_eq!(tag(&placeholder).len(), 32);
            assert!(!placeholder.expose().contains(key));
            let held = ModelKey::new(&made, "anthropic", key.to_string().into(), false);
            held.check_placeholder().unwrap();
        }
        let key: link_secretive::Secret = "sk-ant-secret".to_string().into();
        let model = ModelKey::new(&made, "anthropic", key.duplicate(), false);
        assert_eq!(model.placeholder, placeholder(&made, &key));
        assert!(!format!("{model:?}").contains("secret"));
        assert_eq!(
            url("openrouter", 40781).as_deref(),
            Some("http://127.0.0.1:40781/_link/model/v1")
        );
        assert_eq!(url("ollama", 40781), None);
        let secret = link_sandbox_route::url_secret().unwrap();
        assert_eq!(
            local_url(20199, 11434, &secret),
            format!("http://127.0.0.1:20199/_link/url/{secret}/here/11434")
        );
    }

    /// A model at an address of its own is that one host, its API's root
    /// before the `/v1` its provider takes, mounted apart from the
    /// provider's route, its model's and its fallback's each their own.
    #[test]
    fn a_model_at_its_own_address_is_one_hosts_api() {
        let at = At::new("openai", "https://Models.Gw.Example/v1").unwrap();
        assert_eq!(
            (at.host.as_str(), at.root.as_str()),
            ("models.gw.example", "")
        );
        let at = At::new("openai", "https://gw.example/openai/v1/").unwrap();
        assert_eq!(
            (at.host.as_str(), at.root.as_str()),
            ("gw.example", "/openai")
        );
        let at = At::new("anthropic", "https://gw.example/anthropic").unwrap();
        assert_eq!(at.root, "/anthropic");
        for (provider, url) in [
            ("openai", "http://gw.example/v1"),
            ("openai", "https://gw.example:8443/v1"),
            ("openai", "https://user@gw.example/v1"),
            ("openai", "https://192.0.2.1/v1"),
            ("openai", "https://[::1]/v1"),
            ("openai", "https://localhost/v1"),
            ("openai", "https://gw.example/v1?x=1"),
            ("openai", "https://gw.example/../v1"),
            ("openai", "https://gw.example/openai"),
            ("anthropic", "https://gw.example/v1"),
            ("ollama", "https://gw.example/v1"),
        ] {
            assert!(At::new(provider, url).is_err(), "{provider} {url}");
        }
        let made = Placeholders::new().unwrap();
        let key: link_secretive::Secret = "ex_key_the-key".to_string().into();
        let at = At::new("openai", "https://models.gw.example/v1").unwrap();
        let model = Hosted::new(&made, "openai", HostedKey::Model, &key, false).at(at.clone());
        let fallback = Hosted::new(&made, "openai", HostedKey::Fallback, &key, false).at(at);
        assert_eq!(model.mounted(), ("model-at", HOSTED_AT));
        assert_eq!(
            fallback.mounted(),
            ("model-at-fallback", HOSTED_AT_FALLBACK)
        );
        assert_eq!(
            model.url(40781).as_deref(),
            Some("http://127.0.0.1:40781/_link/model-at/v1")
        );
        let route = hosted(Bridge::default(), model.duplicate()).unwrap();
        assert_eq!(
            (route.name.as_str(), route.prefix.as_str()),
            model.mounted()
        );
        route.names_only().unwrap();
        assert!(!route.given.expose().contains("the-key"));
        // The key the bridge adds knows the address it goes to.
        let added = model.with_key(key.duplicate()).unwrap();
        assert_eq!(added.at.unwrap().host, "models.gw.example");
    }

    /// An agent's route names its key, and holds none; one handed with a
    /// bind made at once is marked so, and refused in a routes file.
    #[test]
    fn a_hosted_route_names_its_key() {
        let made = Placeholders::new().unwrap();
        let key: link_secretive::Secret = "sk-ant-api03-the-key".to_string().into();
        let named = Hosted::new(&made, "anthropic", HostedKey::Model, &key, false);
        let route = hosted(Bridge::default(), named.duplicate()).unwrap();
        route.names_only().unwrap();
        assert!(!route.given.expose().contains("the-key"));
        // The bridge adds the caller's key only for its placeholder.
        assert_eq!(named.with_key(key.duplicate()).unwrap().key, key);
        assert!(named.with_key("sk-or-v1-other".to_string().into()).is_err());
        let handed = ModelKey::new(&made, "anthropic", key.duplicate(), false);
        let route = hosted(Bridge::default(), handed.into()).unwrap();
        assert!(route.names_only().is_err());
    }

    #[test]
    fn a_placeholder_is_the_same_tag_of_its_key() {
        // Pinned: a holder's placeholders, kept for its run, are the
        // same tags whatever version of Link reads them.
        let made = Placeholders::from_kept(link_secretive::OpaqueSecret::from_text(
            "pinned-placeholders-secret".to_string().into(),
        ));
        assert_eq!(
            placeholder(&made, &"sk-ant-api03-k".to_string().into()).expose(),
            "sk-ant-api03-link-wire-2b111b948d8a42d03947df37249f7fed"
        );
    }

    #[test]
    fn a_placeholder_is_refused_unless_its_whole_shape_holds() {
        // Never an empty, guessed or padded one: its whole shape, the
        // prefix its key keeps included.
        let one = Placeholders::new().unwrap();
        let key: link_secretive::Secret = "sk-ant-api03-the-key".to_string().into();
        let tag = "0123456789abcdef0123456789abcdef";
        let mut held = ModelKey::new(&one, "anthropic", key, false);
        held.check_placeholder().unwrap();
        for wrong in [
            String::new(),
            "link-wire-".into(),
            "sk-ant-api03-link-wire".into(),
            "sk-ant-api03-link-wire-xyz".into(),
            // Another kind's prefix, none, or something before it.
            format!("sk-or-v1-link-wire-{tag}"),
            format!("link-wire-{tag}"),
            format!("x sk-ant-api03-link-wire-{tag}"),
            // A tag too long, in capitals, or followed by more.
            format!("sk-ant-api03-link-wire-{tag}0"),
            format!("sk-ant-api03-link-wire-{}", tag.to_uppercase()),
            format!("sk-ant-api03-link-wire-{tag}\n"),
        ] {
            held.placeholder = wrong.clone().into();
            assert!(held.check_placeholder().is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn models_that_search_are_known_by_name() {
        for (provider, model) in [
            ("openrouter", "perplexity/sonar"),
            ("openrouter", "Perplexity/sonar-deep-research"),
            ("openrouter", "openai/gpt-4o-search-preview"),
            ("openrouter", "openai/gpt-4o-mini-search-preview:floor"),
            ("openai", "gpt-4o-search-preview-2025-03-11"),
            ("openai", "gpt-5-search-api"),
            ("openai", "o3-deep-research"),
        ] {
            assert!(searches_by_nature(provider, model), "{provider} {model}");
        }
        for (provider, model) in [
            ("openrouter", "openai/gpt-6.1-sol"),
            ("openrouter", "anthropic/claude-opus-5.5"),
            ("openrouter", "someone/perplexity"),
            ("openai", "gpt-6.1-sol"),
            ("openai", "perplexity/sonar"),
            ("anthropic", "claude-opus-5-search-preview"),
        ] {
            assert!(!searches_by_nature(provider, model), "{provider} {model}");
        }
    }
}

pub mod actions;
