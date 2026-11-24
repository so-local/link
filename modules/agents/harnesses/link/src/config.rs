//! An agent's settings for Link Harness: what `configure` writes from
//! Link's settings, read again at every turn through the store
//! ([`crate::store::Store::config`]), so a change needs no restart and
//! nothing is held between turns.

use link_agent_spec::Spec;
use link_harness_contract::harness::{Model, Settings};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

link_files::specification!(Config, include_str!("config.schema.toml"));

/// `config.json` in the harness's home, as [`crate::store::Files`] keeps it.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_sources: Vec<link_harness_wire::evidence::ReadRegistration>,
    #[serde(default)]
    pub actions: link_harness_contract::sandbox::namespace::Endpoints,
    /// The model and how to reach it: behind its proxy, the proxy's
    /// address and a placeholder key (the proxy adds the real one).
    #[duplicate]
    pub model: Model,
    /// A second model, for a turn `model` can't answer ([`crate::fallback`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub fallback: Option<Model>,
    /// What Link and you know of `fallback`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_facts: Option<link_model_contract::Known>,
    /// What Link's connections send its endpoint; none: any request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub endpoint_key: Option<link_secretive::Secret>,
    /// Where its store bridge listens, when Link runs one for its
    /// conversations' driver: that driver's calls go through it, proved
    /// with `endpoint_key`, and the driver never runs here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<link_store_contract::store::BridgeAt>,
    /// Where each of its collections is kept, as its machine resolved
    /// them, and the storage bridge that reaches every place off the
    /// machine: what's kept there is read and written only through it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<link_store_contract::storage::StorageAt>,
    /// The agent's opinions, its spec as Link resolved it; none from a
    /// Link before specs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<Spec>,
    /// The keys of `spec` that are Link's, which it holds to what the
    /// model takes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defaults: Vec<String>,
    /// What Link and you know of the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_facts: Option<link_model_contract::Known>,
    /// What it runs with here, as Link resolved it at its start: its
    /// connections, and what it may write of its files (the prompt's
    /// environment section).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<link_harness_contract::harness::Environment>,
    /// The provider's faster service asked for, as the conversation a
    /// turn is of chose (`/model:set --fast`): never kept.
    #[serde(skip)]
    pub fast: bool,
    /// Whether the chat shows each tool call, as the conversation chose
    /// (`/chat:verbose`): never kept; none, as the spec says.
    #[serde(skip)]
    pub verbose: Option<bool>,
}

impl Config {
    /// What of Link's settings Link Harness keeps: never a connection's
    /// token (Link carries its connections).
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            read_sources: settings.read_sources.clone(),
            actions: settings.actions.clone(),
            model: settings.model.duplicate(),
            fallback: settings.fallback.duplicate(),
            fallback_facts: settings.fallback_facts.clone(),
            endpoint_key: settings.endpoint_key.duplicate(),
            store: settings.store.clone(),
            storage: settings.storage.clone(),
            spec: settings.spec.clone(),
            defaults: settings.defaults.clone(),
            model_facts: settings.model_facts.clone(),
            environment: settings.environment.clone(),
            fast: false,
            verbose: None,
        }
    }

    /// As a conversation `chosen` chose: its model in place of the
    /// agent's, how hard it thinks in place of its spec's, and fast.
    pub fn chosen(mut self, chosen: &crate::commands::Chosen) -> Self {
        if let Some(model) = &chosen.model {
            self.model.name = Some(model.clone());
        }
        if let Some(level) = chosen.thinking {
            let mut spec = self.opinions();
            let model = spec.model.get_or_insert_with(Default::default);
            model.thinking = Some(level);
            let own = match self.model.provider.as_str() {
                "anthropic" => model.anthropic.as_mut(),
                "openai" => model.openai.as_mut(),
                "openrouter" => model.openrouter.as_mut(),
                "ollama" => model.ollama.as_mut(),
                "local" => model.local.as_mut(),
                _ => None,
            };
            if let Some(own) = own {
                own.thinking = Some(level);
            }
            self.spec = Some(spec);
            self.defaults.retain(|key| key != "model.thinking");
        }
        self.fast = chosen.fast;
        self.verbose = chosen.verbose;
        self
    }

    /// Its opinions: its spec, else Link's.
    /// A harness holds none of its own.
    pub fn opinions(&self) -> Spec {
        self.spec.clone().unwrap_or_else(link_agent_spec::link)
    }

    /// How often a running turn renews its lease, and how long a lease
    /// not renewed lasts before its turn is taken for crashed: its spec's
    /// `[turns] renew` and `lapse`, else Link's. Link's spec always holds
    /// both (a test says so); were neither read, a lease would hold until
    /// its turn ends.
    pub fn lease(&self) -> (std::time::Duration, std::time::Duration) {
        let of = |spec: &Spec| {
            let turns = spec.turns.clone().unwrap_or_default();
            let get = |duration: Option<link_actions::value::Duration>| duration.map(|d| d.get());
            (get(turns.renew), get(turns.lapse))
        };
        let (renew, lapse) = of(&self.opinions());
        let (links_renew, links_lapse) = of(&link_agent_spec::link());
        (
            renew.or(links_renew).unwrap_or(std::time::Duration::MAX),
            lapse.or(links_lapse).unwrap_or(std::time::Duration::MAX),
        )
    }

    /// Whether a message on thread `thread`'s connection brings it a part of
    /// `kind`, as its spec says: `[connections] images`, `documents` and
    /// `voice`, its
    /// connection's own over them. Left out, it doesn't.
    pub fn takes(&self, thread: &str, kind: crate::event::Kind) -> bool {
        let connection = thread
            .split_once(':')
            .map_or("", |(connection, _)| connection);
        let connection = self.opinions().connection(connection);
        let on = match kind {
            crate::event::Kind::Image => connection.images,
            crate::event::Kind::Document => connection.documents,
            crate::event::Kind::Voice => connection.voice,
        };
        on == Some(true)
    }

    /// The keys of its opinions that are Link's: every one, from a Link
    /// before specs.
    pub fn links(&self) -> Vec<String> {
        match &self.spec {
            Some(_) => self.defaults.clone(),
            None => link_agent_spec::resolve(
                None,
                &[],
                &link_agent_spec::Known {
                    name: "agent".into(),
                },
            )
            .map(|r| r.links())
            .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_brings_what_its_spec_takes() {
        use crate::event::Kind::{Document, Image};
        let link = Config::default();
        assert!(link.takes("telegram:1", Image) && link.takes("default", Document));
        let spec =
            "[connections]\nimages = false\n[connection.slack]\nimages = true\ndocuments = false\n";
        let own = Config {
            spec: Some(link_agent_spec::parse(&format!("schema = 1\n{spec}")).unwrap()),
            ..Config::default()
        };
        assert!(!own.takes("telegram:1", Image) && own.takes("slack:C1", Image));
        // Left out of a spec that is the agent's whole: off.
        assert!(!own.takes("telegram:1", Document) && !own.takes("slack:C1", Document));
    }

    #[test]
    fn no_connection_token_is_kept() {
        let settings = Settings {
            model: Model {
                provider: "anthropic".into(),
                api_key: "sk-ant-api03-link-wire".to_string().into(),
                ..Model::default()
            },
            telegram: Some(link_harness_contract::harness::Telegram {
                bot_token: "123:secret".to_string().into(),
                allowed_users: vec![42],
                api_root: None,
            }),
            endpoint_key: Some("k".to_string().into()),
            ..Settings::default()
        };
        let json = serde_json::to_string(&Config::from_settings(&settings)).unwrap();
        assert!(!json.contains("secret"), "{json}");
        assert!(json.contains("link-wire"), "{json}");
    }
}
