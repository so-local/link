//! This machine's records of an agent, in `~/.lnk/safe/agents/<name>/`,
//! where no sandbox reaches: its identity (`identity.toml`, its ID, the
//! service it acts as and its owner) and what this machine gave it
//! (`here.toml`, its ports and the services it keeps off). None of it
//! is the agent's own, so none of it is in `agent.toml`, which holds
//! only its setup and spec and travels in an archive.

use super::*;
use link_format::WithinExt;

/// An agent's identity here.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    /// Its ID, bare.
    pub id: String,
    /// The service it acts as, its bare ID.
    #[serde(default)]
    pub service: Option<String>,
    /// Whose it is ([`Settings::owner`]), by its kind.
    #[serde(default)]
    pub owner: Option<link_machine::owner::Owner>,
}

link_files::specification!(Identity, include_str!("identity.schema.toml"));

/// What this machine gave an agent: each machine gives its own.
#[derive(Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Here {
    /// [`Settings::port`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// [`Settings::block`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<u16>,
    /// [`Settings::webhook_port`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_port: Option<u16>,
    /// [`Settings::stopped_services`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped_services: Vec<String>,
    /// [`Settings::programs`]: this machine's folders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
}

link_files::specification!(Here, include_str!("here.schema.toml"));

/// `~/.lnk/safe/agents/<name>/identity.toml`.
pub fn identity_path(name: &str) -> PathBuf {
    config_of(name).join("identity.toml")
}

/// `~/.lnk/safe/agents/<name>/here.toml`.
pub fn here_path(name: &str) -> PathBuf {
    config_of(name).join("here.toml")
}

/// Whether agent `name` has an identity here.
pub fn has_identity(name: &str) -> bool {
    identity_path(name).is_file()
}

/// Agent `name`'s identity here, checked; none when it has none.
pub(super) fn identity(name: &str) -> anyhow::Result<Option<Identity>> {
    let path = identity_path(name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).within(format!("reading {}", path.display())),
    };
    let identity: Identity =
        link_files::saved::parse(&text).within(format!("reading {}", path.display()))?;
    link_machine::machine::check_id(&identity.id).within(format!("in {}", path.display()))?;
    if let Some(service) = &identity.service {
        link_machine::machine::check_id(service).within(format!("in {}", path.display()))?;
    }
    if let Some(owner) = &identity.owner {
        owner
            .clone()
            .checked()
            .within(format!("in {}", path.display()))?;
    }
    Ok(Some(identity))
}

/// What this machine gave agent `name`; nothing when it gave nothing.
pub fn here(name: &str) -> anyhow::Result<Here> {
    let path = here_path(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => link_files::saved::parse(&text).within(format!("reading {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Here::default()),
        Err(err) => Err(err).within(format!("reading {}", path.display())),
    }
}

/// Fills `settings`' records from this machine's: its identity, when
/// it has one, and what this machine gave it.
pub(super) fn apply(settings: &mut Settings) -> anyhow::Result<()> {
    if let Some(identity) = identity(&settings.name)? {
        settings.id = Some(super::ids::workload(&identity.id)?);
        settings.service = identity.service;
        // Checked as it's read: its parts hold no `@` between them.
        settings.owner = identity
            .owner
            .map(|owner| owner.checked().map(|owner| owner.text()))
            .transpose()?;
    }
    let here = here(&settings.name)?;
    settings.port = here.port;
    settings.block = here.block;
    settings.webhook_port = here.webhook_port;
    settings.stopped_services = here.stopped_services;
    settings.programs = here.path;
    Ok(())
}

/// Keeps `settings`' records here: its identity, once it has an ID, and
/// what this machine gave it.
pub(super) fn keep(settings: &Settings) -> anyhow::Result<()> {
    if let Some(id) = settings.agent_id() {
        let identity = Identity {
            id: id.to_string(),
            service: settings.service.clone(),
            owner: settings
                .owner
                .as_deref()
                .map(link_machine::owner::Owner::of)
                .transpose()?,
        };
        link_files::saved::write(&identity_path(&settings.name), &identity)?;
    }
    let here = Here {
        port: settings.port,
        block: settings.block,
        webhook_port: settings.webhook_port,
        stopped_services: settings.stopped_services.clone(),
        path: settings.programs.clone(),
    };
    let path = here_path(&settings.name);
    if here == Here::default() {
        match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                return Err(err).within(format!("removing {}", path.display()));
            }
            _ => return Ok(()),
        }
    }
    link_files::saved::write(&path, &here)
}

#[cfg(test)]
mod tests {
    use super::Here;

    /// A record keeps its port and block, and no proxy port apart from
    /// them.
    #[test]
    fn a_record_keeps_its_port_and_block_only() {
        let here: Here = link_files::saved::parse("schema = 1\nport = 20400\nblock = 2\n").unwrap();
        assert_eq!((here.port, here.block), (Some(20400), Some(2)));
        assert!(link_files::saved::parse::<Here>("schema = 1\nproxy_port = 24001\n").is_err());
    }
}
