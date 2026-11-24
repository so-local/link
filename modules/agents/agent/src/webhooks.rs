//! An agent's own webhooks (`lnk agent webhook`): an address on
//! 127.0.0.1 its bridge takes signed posts at, one per webhook
//! (`/h/<hook id>`), each with a secret of its own, which reaches the agent
//! as a stranger's message from `webhook:<name>`. Its module,
//! `lnk-webhook-connection`, sifts and takes each post ([`crate::pushed`]); only
//! this machine reaches it, unless its owner puts an address outside in front.

use link_format::WithinExt;
use std::collections::BTreeMap;

use anyhow::ensure;
use link_secretive::Duplicate;
use link_secretive::Secret;
use serde::{Deserialize, Serialize};

use crate::settings::{self, Settings};

pub use link_connection_contract::webhook::{CONNECTION, PREFIX, Sender};

/// How long a webhook's previous secret is still taken after `roll`, for
/// its sender to switch: a day.
pub const ROLL_KEEP: u64 = 24 * 3600;

/// How many random bytes a webhook's secret is.
const SECRET_BYTES: usize = 32;

/// One of an agent's webhooks, by its name in [`Settings::webhooks`]. Its
/// secrets are kept with your secrets, granted to the agent's own use
/// ([`link_agent_identity::own::OwnSecret::Webhook`]), never in its settings' file:
/// only its bridge, outside its sandbox, reads them.
#[derive(Duplicate, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Webhook {
    /// Its hook's id, the address's last part: random, 128 bits.
    pub id: String,
    #[serde(default, skip_serializing_if = "Sender::is_link")]
    pub from: Sender,
    #[serde(skip)]
    #[duplicate]
    pub secret: Secret,
    /// The secret before it, while it rolls.
    #[serde(skip)]
    #[duplicate]
    pub previous: Secret,
    /// Until when (seconds since 1970) the previous secret is taken: none,
    /// there's none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_until: Option<u64>,
    /// How many questions it asks the owner a day, at most; none, Link's
    /// ([`crate::pushed::QUESTIONS_PER_DAY`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions_per_day: Option<u32>,
}

impl Webhook {
    /// The previous secret, while it's taken at `now`.
    pub fn previous_at(&self, now: u64) -> Option<&Secret> {
        self.previous_until
            .filter(|until| *until > now)
            .map(|_| &self.previous)
            .filter(|previous| !previous.is_empty())
    }

    /// Forgets the previous secret if it's no longer taken at `now`.
    fn prune(&mut self, now: u64) {
        if self.previous_at(now).is_none() {
            self.previous_until = None;
            self.previous = Secret::default();
        }
    }

    /// What its module's `sift` and `take` are handed of it: its name and
    /// who signs it; nothing secret.
    pub fn settings(&self, name: &str) -> serde_json::Value {
        serde_json::to_value(link_connection_contract::webhook::Settings {
            name: name.to_string(),
            from: self.from,
        })
        .expect("a webhook's settings are JSON")
    }
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Its address on this machine, at `port`.
pub fn address(port: u16, id: &str) -> String {
    format!("http://127.0.0.1:{port}/h/{id}")
}

/// An error unless `agent`'s harness says who each message is from, and
/// keeps a conversation's tools: a webhook's post is a stranger's, and a
/// harness that can't say so would read it as yours, one that can't keep
/// a conversation's tools would give it every tool the agent has.
pub fn check_says_who(agent: &Settings) -> anyhow::Result<()> {
    let harness = agent.harness();
    let adapter = crate::adapter::Adapter::find(harness)?;
    let contract = adapter.contract()?;
    ensure!(
        contract.from && adapter.endpoint(agent.port).is_some(),
        "{harness} can't say who a message is from, so a webhook's post would reach {} as \
         yours: a webhook needs a harness that can (lnk agent harness set link)",
        agent.name
    );
    ensure!(
        contract.tools,
        "{harness} can't keep a conversation's tools, so a webhook's post would get every \
         tool {} has: a webhook needs a harness that can (lnk agent harness set link)",
        agent.name
    );
    Ok(())
}

/// What a new webhook, or a rolled one, prints once.
pub struct Made {
    pub address: String,
    pub port: u16,
    pub secret: Secret,
}

/// Adds webhook `name` to `agent`: its hook's id and a new secret, kept
/// with your secrets, and the agent's port for its webhooks chosen if it
/// has none. `says_who` checks its harness first ([`check_says_who`]).
pub fn add(
    agent: &mut Settings,
    name: &str,
    from: Sender,
    says_who: impl Fn(&Settings) -> anyhow::Result<()>,
) -> anyhow::Result<Made> {
    link_machine::machine::check_name(name)?;
    ensure!(
        !agent.webhooks.contains_key(name),
        "{} has a webhook {name} already: lnk agent webhook rotate {name} gives it a new secret",
        agent.name
    );
    says_who(agent)?;
    let (secret, kept) = new_secret()?;
    let id = link_files::random_token()?;
    let port = match agent.webhook_port {
        Some(port) => port,
        None => settings::free_service_port(agent)?,
    };
    agent.webhook_port = Some(port);
    agent.webhooks.insert(
        name.to_string(),
        Webhook {
            id: id.clone(),
            from,
            secret: kept,
            ..Webhook::default()
        },
    );
    agent.save()?;
    Ok(Made {
        address: address(port, &id),
        port,
        secret,
    })
}

/// Lets webhook `name` ask the owner `per_day` questions a day at most.
pub fn questions_per_day(agent: &mut Settings, name: &str, per_day: u32) -> anyhow::Result<()> {
    let webhook = agent
        .webhooks
        .get_mut(name)
        .within(format!("{} has no webhook {name}", agent.name))?;
    webhook.questions_per_day = Some(per_day);
    agent.save()
}

/// A new secret: what's printed, and what's kept, the same.
fn new_secret() -> anyhow::Result<(Secret, Secret)> {
    let secret = link_secretive::OpaqueSecret::random(SECRET_BYTES)?
        .keep()
        .within("a new secret isn't text")?;
    Ok((secret.duplicate(), secret))
}

/// Gives webhook `name` a new secret at `now`, the one before it taken
/// for [`ROLL_KEEP`] more; with `done`, drops the one before at once,
/// and makes none.
pub fn roll(
    agent: &mut Settings,
    name: &str,
    done: bool,
    now: u64,
) -> anyhow::Result<Option<Made>> {
    let port = agent.webhook_port;
    let webhook = agent.webhooks.get_mut(name).within(format!(
        "{} has no webhook {name}: lnk agent webhook list",
        agent.name
    ))?;
    if done {
        webhook.previous_until = None;
        webhook.previous = Secret::default();
        agent.save()?;
        return Ok(None);
    }
    let (secret, kept) = new_secret()?;
    webhook.previous = std::mem::replace(&mut webhook.secret, kept);
    webhook.previous_until = Some(now + ROLL_KEEP);
    let id = webhook.id.clone();
    agent.save()?;
    let port = port.within("the agent has no port for its webhooks")?;
    Ok(Some(Made {
        address: address(port, &id),
        port,
        secret,
    }))
}

/// Removes webhook `name`; its secrets go from your secrets as it's saved.
pub fn remove(agent: &mut Settings, name: &str) -> anyhow::Result<()> {
    ensure!(
        agent.webhooks.remove(name).is_some(),
        "{} has no webhook {name}: lnk agent webhook list",
        agent.name
    );
    agent.save()
}

/// One webhook as `list` shows it: never its secret.
#[derive(Debug, Serialize)]
pub struct Listed {
    pub name: String,
    pub from: Sender,
    pub address: String,
    /// Until when its previous secret is still taken, while it rolls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_until: Option<u64>,
}

pub fn list(agent: &Settings, now: u64) -> Vec<Listed> {
    let port = agent.webhook_port.unwrap_or_default();
    agent
        .webhooks
        .iter()
        .map(|(name, webhook)| Listed {
            name: name.clone(),
            from: webhook.from,
            address: address(port, &webhook.id),
            previous_until: webhook.previous_until.filter(|until| *until > now),
        })
        .collect()
}

/// Webhook `name`'s hook, at `now`: its previous secret while it's taken.
fn hook_of(name: &str, webhook: &Webhook, now: u64) -> crate::pushed::Hook {
    crate::pushed::Hook {
        connection: CONNECTION.to_string(),
        settings: webhook.settings(name),
        secret: webhook.secret.duplicate(),
        previous: webhook.previous_at(now).map(Secret::duplicate),
        // A stranger's, in its own conversation, named as it is.
        from: format!("{PREFIX}:{name}"),
        thread: format!("{PREFIX}:{name}"),
        questions_per_day: webhook
            .questions_per_day
            .unwrap_or(crate::pushed::QUESTIONS_PER_DAY),
    }
}

/// Which version of a file was read: its inode, size and when it last
/// changed. A file written whole ([`link_files::write_private`]) is a new
/// inode each time.
#[derive(Clone, Debug, PartialEq)]
struct Version(Option<(u64, u64, std::time::SystemTime)>);

impl Version {
    fn of(path: &std::path::Path) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Self(
            std::fs::metadata(path)
                .ok()
                .and_then(|m| Some((m.ino(), m.len(), m.modified().ok()?))),
        )
    }
}

/// An agent's webhooks as last read, by their hooks' ids, and the
/// versions of the files they were read from: its settings, and your
/// secrets.
struct Read {
    versions: (Version, Version),
    webhooks: BTreeMap<String, (String, Webhook)>,
}

/// What [`crate::pushed::PushedIn`] finds agent `agent`'s hooks with: its
/// webhooks read once, and again only once its settings or your secrets
/// change (an `add`, a `roll`, a `remove`), so a post costs no read of
/// either, and a secret rolled is taken with no restart.
pub fn hooks(agent: &str) -> crate::pushed::Hooks {
    let agent = agent.to_string();
    let read: std::sync::Mutex<Option<Read>> = std::sync::Mutex::new(None);
    std::sync::Arc::new(move |id: &str| {
        let versions = (
            Version::of(&Settings::path_of(&agent)),
            Version::of(&link_secret_contract::path()),
        );
        let mut read = read.lock().unwrap_or_else(|e| e.into_inner());
        if read.as_ref().is_none_or(|r| r.versions != versions) {
            match Settings::load(&agent) {
                Ok(settings) => {
                    let webhooks = settings
                        .webhooks
                        .into_iter()
                        .map(|(name, webhook)| (webhook.id.clone(), (name, webhook)))
                        .collect();
                    *read = Some(Read { versions, webhooks });
                }
                Err(err) => {
                    link_format::daemon::warn!("reading {agent}'s webhooks: {err:#}");
                    *read = None;
                    return None;
                }
            }
        }
        let (name, webhook) = read.as_ref()?.webhooks.get(id)?;
        Some(hook_of(name, webhook, now()))
    })
}

/// Forgets each webhook's previous secret once it's no longer taken: as
/// any `lnk agent webhook` command saves.
pub fn prune(agent: &mut Settings, now: u64) {
    for webhook in agent.webhooks.values_mut() {
        webhook.prune(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rolled(until: Option<u64>) -> Webhook {
        Webhook {
            id: "a".repeat(32),
            secret: Secret::new("new".into()),
            previous: Secret::new("old".into()),
            previous_until: until,
            ..Webhook::default()
        }
    }

    /// The previous secret is taken until its expiry, and not from then.
    #[test]
    fn the_previous_secret_is_taken_until_it_expires() {
        let webhook = rolled(Some(1_000));
        assert_eq!(webhook.previous_at(999).map(Secret::expose).unwrap(), "old");
        assert!(webhook.previous_at(1_000).is_none());
        assert!(rolled(None).previous_at(0).is_none());
        let mut webhook = rolled(Some(1_000));
        webhook.prune(1_000);
        assert!(webhook.previous.is_empty() && webhook.previous_until.is_none());
        let mut webhook = rolled(Some(1_000));
        webhook.prune(999);
        assert_eq!(webhook.previous_until, Some(1_000));
    }

    /// What its settings' file and its module are given: no secret.
    #[test]
    fn no_secret_leaves_with_its_settings() {
        let webhook = rolled(Some(1_000));
        let kept = toml::to_string(&webhook).unwrap();
        assert!(!kept.contains("new") && !kept.contains("old"), "{kept}");
        assert_eq!(
            webhook.settings("deploys"),
            serde_json::json!({"name": "deploys", "from": "link"})
        );
        let github = Webhook {
            from: Sender::Github,
            ..rolled(None)
        };
        assert!(
            toml::to_string(&github)
                .unwrap()
                .contains("from = \"github\"")
        );
    }

    /// A new secret is 32 random bytes, as hex, and never the same.
    #[test]
    fn a_new_secret_is_32_random_bytes() {
        let (shown, kept) = new_secret().unwrap();
        assert_eq!(shown.expose(), kept.expose());
        assert_eq!(shown.expose().len(), 2 * SECRET_BYTES);
        assert_ne!(new_secret().unwrap().0.expose(), shown.expose());
    }
}
