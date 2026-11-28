//! `lnk agent move`, `send`, `receive`, `pause`, `resume`, `left`,
//! `deliver` and `backup`: an agent, from one machine to another. `move`
//! runs on your computer: it upgrades `lnk` on a box that runs another version, then
//! moves the agent in two phases. The copy, while the agent answers:
//! that machine's `lnk agent state-send --phase copy` piped into the other's
//! `lnk agent receive --phase copy` (through `lnk box run` for a box, over
//! SSH), its files and memory's index into staging there. The switch, in
//! seconds: the agent paused where it is (`lnk agent pause`), what changed
//! since the copy and everything else of it piped the same way, put in
//! place and started there. Once it answers there with its ID, the
//! machine it left deletes its copy (`left`) and keeps a note of where it
//! went. A switch that fails leaves the agent answering where it was
//! (`lnk agent resume`), and the other end with nothing. One place runs
//! each agent at a time; the others stay where they are.
//!
//! To or from a cloud account that hosts agents
//! ([`hosted`](link_agent::hosted)), the account's adapter takes
//! the place of the other machine's `send` and `receive` (`agent-in`,
//! `agent-receive`, `agent-out`, `agent-send`, `agent-left`, `agent-abort`):
//! `move` only pipes.

use std::process::Command;

use anyhow::bail;
use link_agent::adapter::Adapter;
use link_agent::grants::Grants;
use link_agent::sandbox_file::Sandbox;
use link_agent::settings::{self, Settings};
use link_secretive::Duplicate;
use link_secretive::Secret;
use serde::{Deserialize, Serialize};

pub(crate) mod away;
pub(crate) mod backups;
pub(crate) mod buckets;
pub(crate) mod claim;
pub(crate) mod delivering;
pub(crate) mod journal;
pub(crate) mod machines;
pub(crate) mod receiving;
pub(crate) mod sending;

/// Asks before agent `name` is made in `account`, a cloud that hosts
/// agents (`lnk agent create --cloud`, `lnk agent move --cloud`): the
/// cloud runs it and keeps its files, billed by that cloud, so it's made
/// only once you say yes (`yes`, `--yes`).
pub(crate) fn hosted_billed(
    out: &mut link_format::Out,
    account: &str,
    name: &str,
    yes: bool,
) -> Result<(), link_format::Report> {
    use link_format::{Name, prose, sentence};
    link_env_contract::remote::billed(
        out,
        account,
        sentence!(
            "Make the agent {} in {}, billed by that cloud?",
            Name::new("agent", name),
            Name::new("cloud", account)
        ),
        prose!(
            "The cloud runs it and keeps its files, billed at its rates until you move it away or delete it."
        ),
        yes,
    )
}

/// The line `move` sends `receive` before the stream: what the machine
/// in the middle adds. Its secrets ([`Sent`]) cross only in `sealed`,
/// sealed to the key `receive` made for the switch and said.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Header {
    /// A key for a hosted model, when the agent's model can't come along.
    #[serde(skip)]
    key: Option<Secret>,
    /// `key` and `bucket_credentials`, sealed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<link_secretive::Sealed<Secret<Vec<u8>>>>,
    /// Where its traffic leaves from there (`settings.upstream`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upstream: Option<String>,
    /// For a box: a credential reaching only each of the agent's buckets
    /// (`lnk storage keys`), by cloud.
    #[serde(skip)]
    bucket_credentials: Option<link_secretive::Secret<link_secretive::Json>>,
    /// What you granted the agent, when it leaves the machine running the
    /// move. Otherwise the stream's say is taken only in part
    /// ([`transfer::settle_sandbox`](link_agent::transfer::settle_sandbox)):
    /// a box it comes from writes the stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sandbox: Option<Sandbox>,
    /// What its harness needs, when the agent leaves the machine running
    /// the move: its grants there, with any need asked about there before
    /// it moved. They go with the agent, added to the harness's on the
    /// machine it goes to, which doesn't ask again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grants: Option<Grants>,
    /// `lnk agent move --yes`: what its harness asks for there beyond
    /// Link's defaults is allowed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    yes: bool,
    /// Each team a cloud hosting agents keeps that the agent is on, as the
    /// machine running the move read it from that cloud: its file and
    /// where its relay and folder service are, kept where it goes, which
    /// may have no account of its own to ask.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    teams: Vec<link_agent::hosted_teams::Carried>,
}

/// What of a [`Header`] is secret, as it's sealed.
#[derive(Default, Serialize, Deserialize)]
struct Sent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    key: Option<Secret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    bucket_credentials: Option<link_secretive::Secret<link_secretive::Json>>,
}

impl Header {
    /// What its secrets are sealed as.
    fn about() -> link_secretive::About {
        link_secretive::About {
            owner: "move".into(),
            kind: "header".into(),
            version: 1,
        }
    }

    /// As its line, its secrets sealed to `to`.
    fn sealed_to(&self, to: &link_secretive::SealTo) -> anyhow::Result<String> {
        let sent = Sent {
            key: self.key.duplicate(),
            bucket_credentials: self.bucket_credentials.duplicate(),
        };
        let sealed = link_secretive::Sealed::seal_json(&sent, to, Self::about())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let line = Header {
            sealed: Some(sealed),
            upstream: self.upstream.clone(),
            sandbox: self.sandbox.clone(),
            grants: self.grants.clone(),
            yes: self.yes,
            teams: self.teams.clone(),
            ..Header::default()
        };
        Ok(serde_json::to_string(&line)?)
    }

    /// Its secrets, opened by `recipient` from what was sealed.
    /// Without a seal it carries none: its secrets are read nowhere else.
    fn open(&mut self, recipient: &link_secretive::Recipient) -> anyhow::Result<()> {
        let Some(sealed) = self.sealed.take() else {
            return Ok(());
        };
        let sent: Sent = recipient
            .open_json(&sealed, &Self::about())
            .map_err(|e| anyhow::anyhow!("opening what move sent first: {e}"))?;
        self.key = sent.key;
        self.bucket_credentials = sent.bucket_credentials;
        Ok(())
    }
}

/// Where: a box's name, or this computer.
fn place(at: &Option<String>) -> String {
    match at {
        Some(name) => format!("your box {name}"),
        None => "this computer".into(),
    }
}

/// Where, for a cloud account that hosts agents.
fn hosted_place(account: &str) -> String {
    format!("your cloud account {}", link_format::clean(account))
}

/// `-a <agent>` for an `lnk agent` command about agent `agent`.
pub fn naming(agent: &str) -> [&str; 2] {
    ["-a", agent]
}

/// `lnk agent <args>` about agent `agent`, where it is: here, or on a box
/// through `lnk box run`.
fn lnk_at(at: &Option<String>, agent: &str, args: &[&str]) -> Command {
    let mut command = link_core::lnk();
    match at {
        Some(name) => command
            .args(["box", "run", name, "--", "agent"])
            .args(naming(agent)),
        None => command.arg("agent").args(naming(agent)),
    };
    command.args(args);
    command
}

/// An agent, and where it runs.
pub struct Found {
    pub id: Option<String>,
    /// The service it acts as, its ID: its settings', or as the machine it
    /// runs on lists it.
    pub service: Option<String>,
    pub name: String,
    /// The box it runs on; none: here.
    pub at: Option<String>,
    /// Its harness, as the box it runs on says (`lnk agent list`); here,
    /// its settings'.
    pub harness: Option<String>,
    /// Its settings, when it's here.
    pub here: Option<Settings>,
    /// Whether `at` is a cloud account that runs it, not a box.
    pub runs_agents: bool,
}

/// The agent `named` names (`name`, or `name@machine`), wherever it runs:
/// here, or on a box that answers `lnk agent list`. With no name, the
/// only agent here, else the only one anywhere.
pub fn find(named: Option<&str>) -> anyhow::Result<Found> {
    let (name, machine) = match named.and_then(|n| n.split_once('@')) {
        Some((name, machine)) => (Some(name), Some(machine)),
        None => (named, None),
    };
    let this = link_machine::installation::installation().ok();
    // As you name it (`@here`, or its name), and as a machine answers (its ID).
    let is_here = |m: &str| m == "here" || this.as_ref().is_some_and(|t| t.name == m || t.id == m);
    let this_id = |m: Option<String>| {
        this.as_ref()
            .is_some_and(|t| m.as_deref() == Some(t.id.as_str()))
    };
    let local = |name: &str| -> anyhow::Result<Found> {
        let settings = Settings::load(name)?;
        Ok(Found {
            id: settings.agent_id().map(str::to_string),
            service: settings.service.clone(),
            name: settings.name.clone(),
            at: None,
            harness: Some(settings.harness().to_string()),
            here: Some(settings),
            runs_agents: false,
        })
    };
    if let Some(name) = name {
        link_machine::machine::check_name(name)?;
        if machine.is_none_or(is_here) && Settings::path_of(name).exists() {
            return local(name);
        }
    } else if let [one] = settings::names()?.as_slice() {
        return local(one);
    }
    let agents = crate::listing::gather(false, true)?.agents;
    let matches: Vec<&serde_json::Value> = agents
        .iter()
        // An agent's own word, not a note of where it went.
        .filter(|a| a["location"].is_null())
        .filter(|a| name.is_none_or(|n| a["name"] == n))
        .filter(|a| {
            machine.is_none_or(|m| {
                a["machine"]["name"] == m || crate::listing::machine_of(a).as_deref() == Some(m)
            })
        })
        .collect();
    match matches.as_slice() {
        [one] => {
            let at = one["machine"]["name"].as_str().map(str::to_string);
            let name = one["name"].as_str().unwrap_or_default().to_string();
            if this_id(crate::listing::machine_of(one)) {
                return local(&name);
            }
            Ok(Found {
                id: crate::listing::workload_of(one),
                // As where it runs lists it, else as this computer noted
                // when it left.
                service: match crate::listing::service_of(one) {
                    Some(service) => Some(service),
                    None => match crate::listing::workload_of(one) {
                        Some(id) => crate::moved::of_id(&id)?.and_then(|note| note.service),
                        None => None,
                    },
                },
                name,
                at,
                harness: one["harness"].as_str().map(str::to_string),
                here: None,
                runs_agents: one["machine"]["cloud"] == true,
            })
        }
        [] => match name {
            Some(name) => match crate::moved::of_name(name)? {
                Some(note) => bail!(
                    "{name} runs on {}, which didn't answer: lnk box list shows whether it's up",
                    note.place()
                ),
                None => bail!("no agent named {name} here or on your boxes: lnk agent list"),
            },
            None => bail!("no agent here or on your boxes: lnk agent start makes one"),
        },
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|a| {
                    format!(
                        "{}@{}",
                        a["name"].as_str().unwrap_or_default(),
                        a["machine"]["name"].as_str().unwrap_or_default()
                    )
                })
                .collect();
            bail!("which agent? {}", names.join(", "))
        }
    }
}

/// What of harness `adapter`'s home is the agent's ([`Adapter::state`]),
/// with what each connection the harness speaks keeps there of its own: a
/// connection whose module installs its program in the home (`describe`'s
/// `cli`, as Signal's `signal-cli`) says it (`lnk-<connection> cli state
/// --json`, `{"state": ["~/..."]}`). One that doesn't answer adds none.
pub fn state_of(
    adapter: &Adapter,
    settings: &Settings,
) -> anyhow::Result<link_harness_contract::harness::State> {
    let mut state = adapter.state()?;
    let speaks = adapter.connections();
    for (name, connection) in &settings.connections {
        if !connection.about.cli || !speaks.contains(name) {
            continue;
        }
        state.include.extend(connection_state(name));
    }
    Ok(state)
}

/// What connection `connection`'s module keeps in a harness's home (`lnk-<connection>
/// cli state --json`): `~/`-relative paths, none that leaves the home.
fn connection_state(connection: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Said {
        state: Vec<String>,
    }
    let Some(program) = link_core::find_module(connection) else {
        return Vec::new();
    };
    link_secretive::process::ours(program)
        .args(["cli", "state", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Said>(&o.stdout).ok())
        .map_or_else(Vec::new, |said| {
            said.state
                .into_iter()
                .filter(|path| {
                    path.strip_prefix("~/").is_some_and(|rest| {
                        !rest.is_empty()
                            && rest
                                .split('/')
                                .all(|part| !part.is_empty() && part != "." && part != "..")
                    })
                })
                .collect()
        })
}
