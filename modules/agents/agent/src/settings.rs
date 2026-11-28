//! `~/Link/Safe/Agents/<name>/agent.toml`: an agent's settings, what the
//! user chose once for it, whichever harness runs it. Credentials are
//! held separately by the secret module, never in this file.
//!
//! Every agent here has a name, a file of settings and a folder of its
//! own (`~/Link/Agents/<name>`); [`Settings::here`] finds this machine's.
//!
//! The file holds only what's the agent's: its setup and spec. Its ID,
//! service, owner, name, ports and stopped services are this machine's
//! records ([`records`]), so the file can travel in an archive.

use link_format::WithinExt;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use link_agent_contract::agent_sandbox::Route;
use link_secretive::Duplicate;

use anyhow::{bail, ensure};
use link_harness_contract::harness::Model;
use link_secretive::Secret;
// How the agent uses one of its keys, which is where it is in its
// settings: its model's, its fallback's, a connection's secret field.
use link_secret_contract::Use;

use crate::connections::Connection;
use crate::grants::Grants;
use crate::sandbox_file::Sandbox;
use serde::{Deserialize, Serialize};
mod catalog;
pub mod configuration;
mod folders;
pub mod ids;
mod loading;
pub(crate) mod paths;
pub(crate) mod ports;
mod providers;
pub mod records;
mod team_files;
mod wiring;
pub use catalog::{
    DEFAULT_HARNESS, DEFAULT_MEMORY, KNOWN, MEMORIES, MORE_HARNESSES, harnesses, known_harness,
    memory_module_of, module_of, runtime_of, takes_proxy,
};
pub use loading::{Peek, away, names, peek};
pub use paths::{
    Defaults, SETTINGS_FILE, agent_dir, agents, agents_config, boxes, config_of, configuration_of,
    home, id_of, is_key, key_of, memory_record_of, memory_service_of, memory_tool_of, owner_of,
    root, service_of, shared_harness_of, shared_record_of,
};
pub use ports::{
    BLOCK_MAX, EPHEMERAL, FIRST_PORT, block_for, first_port, free_block_here, free_service_port,
    listened_on, port_taken, ports_of, save_ports,
};
use ports::{fits, others_ports, random_free_block};
pub use providers::{
    KEY_VARS_TEXT, key_for_provider, key_from_env, key_var, key_vars, local_port, provider_of,
};
pub use team_files::{
    HostedReach, RememberTeams, TeamReach, arriving_grant, check_member_move, check_teams, team,
    team_dir, team_path, teams, teams_dir, teams_except, teams_hidden, teams_in, teams_of,
};
pub use wiring::Wired;

/// Link's `[files]` template: what tool setup proposes for a work folder
/// ([`link_agent_spec::LINK_SANDBOX`]).
pub fn default_files() -> link_harness_contract::harness::Folders {
    Sandbox::link().folders()
}

/// Each connected connection's rule (`lnk-<connection> rule --json`), for
/// [`Settings::wire_secrets`]: None, the harness holds its secret.
pub type Rules = BTreeMap<String, Option<link_connection_contract::Rule>>;

#[derive(Duplicate, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Behavior configured in the same file as its setup.
    #[serde(
        default = "configuration::default_behavior",
        skip_serializing_if = "Option::is_none"
    )]
    pub behavior: Option<link_agent_spec::Spec>,
    /// The agent's ID, which never changes, reserved by `lnk agent create`
    /// ([`ids::reserve`]) and carried by its moves, `agent:<id>` by its type
    /// ([`Settings::agent_id`] the bare ID).
    /// Kept in this machine's records ([`records`]), never `agent.toml`.
    #[serde(skip)]
    pub id: Option<link_id::Workload>,
    /// The service it acts as, `service:<id>`: who its secrets are given
    /// to and its roles name. Minted with it under an ID of its own
    /// ([`ids::SERVICES`]), carried by its moves and retired with it.
    #[serde(skip)]
    pub service: Option<String>,
    /// Whose it is: an account (`acct-<id>`), or before you sign
    /// in, the user running Link here (`<user>@<machine>`), which the
    /// account replaces at its next start ([`Settings::settle_owner`]).
    #[serde(skip)]
    pub owner: Option<String>,
    /// Its name, which is its folder's (`~/Link/Safe/Agents/<name>/agent.toml`).
    #[serde(skip)]
    pub name: String,
    /// The harness `lnk agent start` runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// The services of the install it was brought in from,
    /// kept from starting in `~/.lnk/safe/agents/stopped-services`, by their paths in the home folder:
    /// `lnk agent delete` turns each back on.
    #[serde(skip)]
    pub stopped_services: Vec<String>,
    /// The folders of the `PATH` of whoever last ran `lnk agent start`
    /// here: where its run and code tools find its programs, before the
    /// system's ([`crate::programs`]).
    #[serde(skip)]
    pub programs: Vec<String>,
    /// Its harness's base port, given to the adapter as `LNK_PORT`: every
    /// agent's own block, `main`'s too, so no two users' agents start on
    /// one adapter's default. None until this machine gives it one.
    #[serde(skip)]
    pub port: Option<u16>,
    /// How many ports from `port` are its ([`block_for`]): what its
    /// harness takes, and one for its proxy. None: [`PORT_BLOCK`], as
    /// every agent had before harnesses said.
    #[serde(skip)]
    pub block: Option<u16>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "link_files::relative::in_home"
    )]
    pub files: Option<PathBuf>,
    /// Whether it runs in its sandbox: off from `lnk agent start
    /// --no-sandbox` until `lnk agent start --sandbox`, whatever restarts
    /// it between. No other command turns it back on.
    #[serde(default = "yes")]
    pub sandbox: bool,
    /// On a Mac, whether the agent keeps it from sleeping while it runs:
    /// off where its spec's `[run] let_sleep` is true, or for one start
    /// after `lnk agent start --let-sleep`, until the next start.
    #[serde(default = "yes")]
    pub awake: bool,
    /// Where the harness's traffic goes through its proxy (the sandbox's
    /// filtering proxy): `direct` or a SOCKS upstream. Set on the machine
    /// it runs on, by `lnk agent move` or `lnk agent route`.
    /// None: `direct`, unless it runs on the network as it is or reaches
    /// no internet
    /// ([`Settings::proxy_of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    /// Behind its proxy: the harness holds a placeholder for its hosted
    /// model's key, and the proxy adds the real one, outside the sandbox
    /// (`lnk agent keys wire`, or `agent` for the harness to hold it).
    /// None: the harness's default ([`takes_proxy`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_keys: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub model: Option<Model>,
    /// A hosted model a harness that can falls back to for a turn its
    /// model can't answer (`lnk agent start --model.fallback_model`): off, the prompt
    /// past its window, or its provider busy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub fallback: Option<Model>,
    /// The connections it talks on (`lnk agent connect`), by name, each as
    /// its module made and described it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[duplicate]
    pub connections: BTreeMap<String, Connection>,
    /// Its webhooks (`lnk agent webhook create`), by name: each the address
    /// its bridge takes posts at, and their secrets, kept with your
    /// secrets like its connections' tokens.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[duplicate]
    pub webhooks: BTreeMap<String, crate::webhooks::Webhook>,
    /// The port on 127.0.0.1 its bridge takes its webhooks' posts at:
    /// chosen at its first `lnk agent webhook create` and kept, so a tunnel
    /// to it reaches it after every restart.
    #[serde(skip)]
    pub webhook_port: Option<u16>,
    /// Its memory's engine (`lnk agent memory start`), served to whichever
    /// harness runs it; none, no memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// Whether it was asked which of Link's tools it gets (`lnk agent
    /// start`, in a terminal), whatever it answered: asked again only by
    /// `lnk agent tool add`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tools_offered: bool,
    /// Whether it was asked for memory's embedding model, whatever it
    /// answered: asked again only by `lnk model install`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub meaning_offered: bool,
    /// Its keys as your secrets held them when they were loaded or last
    /// saved here ([`Settings::own`]): `save` writes them only when they
    /// change.
    #[serde(skip)]
    #[duplicate]
    own_kept: BTreeMap<Use, Secret>,
    /// Loaded without its keys ([`Settings::load_without_keys`]): `save`
    /// leaves the values your secrets hold as they are.
    #[serde(skip)]
    keys_left: bool,
}

fn yes() -> bool {
    true
}

// Its `[behavior]` is the agent spec, whose tools' names follow history.
link_files::specification!(
    Settings,
    include_str!("settings/settings.schema.toml"),
    references =
        link_spec::follow::sites_under("behavior", &link_spec::schema::<link_agent_spec::Spec>())
);

/// Each agent's settings, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Settings>()
}

impl Settings {
    /// A new agent, not saved yet.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            behavior: configuration::default_behavior(),
            sandbox: true,
            awake: true,
            ..Self::default()
        }
    }

    /// A block of ports here for an agent that has none: one that arrived,
    /// since each machine gives its own and its file came along with it,
    /// so a save doesn't take it for new.
    pub fn take_ports_here(&mut self) -> anyhow::Result<()> {
        if self.port.is_none() {
            self.port = Some(free_block_here(&self.name, self.block())?);
        }
        Ok(())
    }

    /// How many ports from its base port are its.
    pub fn block(&self) -> u16 {
        self.block.unwrap_or(PORT_BLOCK)
    }

    /// Makes its block of ports `size` long, for a harness that takes
    /// that many ([`block_for`]): at the same base port when no other
    /// agent's block is in the way (always, when it shrinks), else at a
    /// free block picked as a move picks one ([`free_block_here`]). Whether
    /// anything changed; nothing does for an agent with no block here yet.
    pub fn fit_block(&mut self, size: u16) -> anyhow::Result<bool> {
        let Some(base) = self.port else {
            return Ok(false);
        };
        if self.block() == size {
            return Ok(false);
        }
        let taken = others_ports(&self.name)?;
        let from = first_port()?;
        if !fits(from, base, size, &taken) {
            self.port =
                Some(random_free_block(from, &taken, size).within("no ports left for this agent")?);
        }
        self.block = Some(size);
        Ok(true)
    }

    /// Its own spec, as `lnk agent start --spec` kept it: none when it
    /// has none, and runs Link's and its teams'.
    pub fn own_spec(&self) -> anyhow::Result<Option<link_agent_spec::Spec>> {
        let path = Self::path_of(&self.name);
        if !path.exists() {
            return Ok(self.behavior.clone());
        }
        let text = read_spec_text(&path)?;
        match configuration::behavior_spec(&text).within(format!("in {}", path.display()))? {
            Some(spec) => Ok(Some(spec)),
            None => Ok(configuration::default_behavior()),
        }
    }

    /// The spec it runs: Link's, its teams', then its own, as sets
    /// ([`link_agent_spec::resolve`]).
    pub fn spec(&self) -> anyhow::Result<link_agent_spec::Resolved> {
        self.resolve_spec(self.own_spec()?.as_ref())
    }

    pub fn resolve_spec(
        &self,
        own: Option<&link_agent_spec::Spec>,
    ) -> anyhow::Result<link_agent_spec::Resolved> {
        let teams = match self.agent_id() {
            Some(id) => teams_of(id)?,
            None => Vec::new(),
        };
        let known = link_agent_spec::Known {
            name: self.name.clone(),
        };
        let mut resolved = match self.agent_id() {
            Some(id) => link_agent_spec::resolve_member(own, &teams, &known, id)?,
            None => link_agent_spec::resolve(own, &teams, &known)?,
        };
        configuration::unclaim_defaults(&mut resolved);
        Ok(resolved)
    }

    /// Persist one tool switch in its own spec, without overwriting an owner edit.
    pub fn set_tool_enabled(&self, tool: &str, enabled: bool) -> anyhow::Result<bool> {
        let path = Self::path_of(&self.name);
        let read = || -> anyhow::Result<Option<String>> {
            match path.try_exists()? {
                true => read_spec_text(&path).map(Some),
                false => Ok(None),
            }
        };
        let before = read()?;
        let behavior = before
            .as_deref()
            .map(configuration::behavior_text)
            .transpose()?
            .flatten()
            .or(self
                .behavior
                .as_ref()
                .map(link_spec::print_spec)
                .transpose()?);
        let current = behavior
            .as_deref()
            .map(link_agent_spec::parse)
            .transpose()?;
        if (self.resolve_spec(current.as_ref())?.tool(tool).enabled == Some(true)) == enabled {
            return Ok(false);
        }
        // No behavior of its own yet: an empty spec, naming its schema.
        let behavior = match behavior {
            Some(behavior) => behavior,
            None => link_spec::print_spec(&link_agent_spec::Spec::default())?,
        };
        let written = link_agent_spec::with_tool_enabled(&behavior, tool, enabled)?;
        self.resolve_spec(Some(&link_agent_spec::parse(&written)?))?;
        ensure!(
            read()? == before,
            "{} changed meanwhile: run it again",
            path.display()
        );
        let full = configuration::with_behavior(before.as_deref().unwrap_or_default(), &written)?;
        link_files::write_private(&path, full.as_bytes())?;
        Ok(true)
    }

    /// Who it is, as every bridge call names it
    /// ([`link_sandbox_binding::Identity`]): its ID (its name, while it has none), its owner, and the IDs of the teams it's on, as
    /// the teams' files say now.
    pub fn identity(&self) -> link_sandbox_binding::Identity {
        let teams = match self.agent_id() {
            Some(id) => teams_of(id)
                .unwrap_or_default()
                .into_iter()
                .map(|team| team.id)
                .collect(),
            None => Vec::new(),
        };
        link_sandbox_binding::Identity {
            agent: self.key(),
            service: self.service.clone().unwrap_or_default(),
            owner: self.owner().unwrap_or_default(),
            teams,
        }
    }

    /// Whose it is, as its settings say: the local owner when they say
    /// nobody.
    pub fn owner(&self) -> anyhow::Result<String> {
        match &self.owner {
            Some(owner) => Ok(owner.clone()),
            None => link_agent_contract::identity::local_owner(),
        }
    }

    /// Settles whose it is before it starts: nobody's becomes the owner's
    /// now ([`link_core::owner::current_owner`]), and the local
    /// owner's becomes the account's once you've signed in, keeping all
    /// it has. An account's stays.
    pub fn settle_owner(&mut self) -> anyhow::Result<()> {
        let local = match &self.owner {
            None => true,
            Some(owner) => link_agent_contract::identity::is_local(owner),
        };
        if local {
            self.owner = Some(link_core::owner::current_owner()?);
        }
        Ok(())
    }

    /// Where agent `name`'s settings are: `agent.toml` in its folder in
    /// the personal Safe (`~/Link/Safe/Agents/<name>`), which no sandbox
    /// reaches.
    pub fn path_of(name: &str) -> PathBuf {
        configuration_of(name).join(SETTINGS_FILE)
    }

    pub fn path(&self) -> PathBuf {
        Self::path_of(&self.name)
    }
}

pub use link_agent_contract::folder::unsafe_files;

/// A spec's text, at most a spec's size, never through a link.
fn read_spec_text(path: &Path) -> anyhow::Result<String> {
    Ok(link_files::fs::read_control(path, link_spec::MAX_BYTES, "an agent's settings'")?.text)
}

pub use link_harness_contract::harness::PORT_BLOCK;

#[cfg(test)]
pub(crate) mod tests;

/// An error unless `name` can name a new one of `owner`'s (`agents`,
/// `teams`) here: a name, and not one the layout keeps in the owner's
/// folders for itself, as an agent can't be named `journal`.
pub fn check_new_name(owner: &str, name: &str) -> anyhow::Result<()> {
    link_machine::machine::check_name(name)?;
    anyhow::ensure!(
        !link_files::layout::reserved(owner).contains(name),
        "{name:?} is a name Link keeps in its folder of {owner}: pick another name"
    );
    Ok(())
}
