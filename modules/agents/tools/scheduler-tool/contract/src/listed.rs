//! The agents one runner fires in a time zone (`run --agents <file>`),
//! as the machine's shared bridge lists them: one file, naming its
//! schema, which both read through this type.

use serde::{Deserialize, Serialize};

use crate::cli::Flags;

/// A `--agents` file: each agent by the flags `run` takes for it alone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    pub agents: Vec<Flags>,
}

link_files::specification!(Listed, include_str!("listed.schema.toml"));
