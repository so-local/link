//! What `lnk measure` asks of the modules built on it
//! ([`link_core::answers`]): the agents running here ([`AGENTS`]), and
//! what each sandbox's proxy has carried ([`TRAFFIC`]).

use std::path::PathBuf;

use link_core::answers::Question;
use serde::{Deserialize, Serialize};

/// The agents here, each with its processes, folders and files: units
/// of their own, measured as their processes' sum.
pub const AGENTS: Question<(), Vec<Agent>> =
    Question::new("measure.agents", include_str!("asked/agents.toml"));

/// Agent discovery with published sources. The older `AGENTS` answer
/// remains unchanged for strict older readers and external providers.
pub const INVENTORY: Question<(), Inventory> =
    Question::new("measure.inventory", include_str!("asked/inventory.toml"));

/// The retained profile of a job, located by its owning module. The
/// observer reads this file without consuming or changing it.
pub const JOB: Question<Job, Option<PathBuf>> =
    Question::new("measure.job", include_str!("asked/job.toml"));

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub traffic: Vec<super::traffic::Source>,
}

/// An agent here, as [`AGENTS`] answers it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// How this machine's keyring names it, and so its model route's
    /// count: its ID.
    pub key: String,
    /// Who it's for, as its settings say.
    pub owner: Option<String>,
    /// The teams it's in, by their IDs.
    pub teams: Vec<String>,
    pub running: bool,
    /// Its main processes, its own service's first.
    pub pids: Vec<u32>,
    /// Its services' control groups (Linux): its own, and its
    /// connections' service's.
    pub groups: Vec<PathBuf>,
    /// Where its harness appends its own measures.
    pub measures: Option<PathBuf>,
    /// Where the harness module appends its starts' and stops' times.
    pub times: Option<PathBuf>,
    /// Its folders: `~/Link/Agents/<name>`, and its files folder when
    /// that's elsewhere.
    pub folders: Vec<PathBuf>,
}

/// What each sandbox's proxy here has carried.
pub const TRAFFIC: Question<(), Vec<Traffic>> =
    Question::new("measure.traffic", include_str!("asked/traffic.toml"));

/// What one proxy has carried, as [`TRAFFIC`] answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traffic {
    /// The proxy's process.
    pub pid: u32,
    pub sandbox: String,
    pub up: u64,
    pub down: u64,
    /// When the proxy started ([`crate::client::started`]), so its
    /// file isn't taken for a later process's of the same number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each question this owns declares its protocol beside it.
    #[test]
    fn its_questions_declare_their_protocols() {
        for protocol in [
            AGENTS.protocol(),
            INVENTORY.protocol(),
            JOB.protocol(),
            TRAFFIC.protocol(),
        ] {
            assert!(protocol.oldest <= protocol.current, "{protocol:?}");
        }
    }
}
