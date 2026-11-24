//! The portable schedules a runner projects into its job store
//! (`run --definitions <file>`), as the agent module publishes them from
//! the agent's spec: one file, naming its schema, which both read through
//! this type.

use link_agent_state_protocol::schedules::Schedule;
use serde::{Deserialize, Serialize};

/// A `--definitions` file: the agent's canonical portable schedules.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definitions {
    pub schedules: Vec<Schedule>,
}

link_files::specification!(Definitions, include_str!("definitions.schema.toml"));
