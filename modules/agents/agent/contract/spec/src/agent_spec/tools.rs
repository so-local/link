//! An agent's tools: each one's table ([`Tool`]), its code servers, and
//! the keys and limits only some tools take.

use super::*;

/// A tool's opinions and a code server's, as `types.toml` declares them.
pub use crate::declared::{
    AgentCodeServer as CodeServer, AgentLate as Late, AgentSchedulerDriver as SchedulerDriver,
    AgentSchedulerRecurring as SchedulerRecurring, AgentTool as Tool,
};

/// Link's own tools, whose `[tool.<name>]` takes only the keys below.
pub const LINK_TOOLS: &[&str] = &["memory", "scheduler", "web", "run", "file", "code", "mcp"];

impl CodeServer {
    /// Its program and its arguments; none, no server.
    pub fn command(&self) -> &[String] {
        self.command.as_deref().unwrap_or_default()
    }

    /// The extensions of the files it serves.
    pub fn files(&self) -> &[String] {
        self.files.as_deref().unwrap_or_default()
    }

    /// The files that mark a project's root.
    pub fn roots(&self) -> &[String] {
        self.roots.as_deref().unwrap_or_default()
    }

    /// Its settings, by section, as one object: empty when it has none.
    pub fn settings(&self) -> serde_json::Value {
        serde_json::Value::Object(
            self.settings
                .iter()
                .map(|(section, value)| (section.clone(), value.clone()))
                .collect(),
        )
    }
}

/// The keys only memory's table takes.
pub(super) const MEMORY_ONLY: &[&str] = &[
    "keep",
    "max_age",
    "tokens",
    "model",
    "notes",
    "dimensions",
    "memory",
    "reserve",
    "file",
    "threads",
    "deadline",
    "folders",
    "kept_off",
];

/// The keys only the scheduler's table takes.
pub(super) const SCHEDULER_ONLY: &[&str] = &[
    "per_day",
    "subagents",
    "hops",
    "late",
    "ask",
    "recurring",
    "zone",
    "interval",
    "firing",
    "driver",
];

/// Link's tools an agent's spec turns off (`[tool.<name>] enabled =
/// false`): those Link starts for it. The others are given and taken
/// with `lnk agent tool add` and `remove`.
pub const SWITCHED: &[&str] = &["scheduler", "run", "file", "code", "mcp"];

/// Link's screens an agent's spec may switch (`[tool.<name>] enabled`):
/// true gives it at a start where the machine runs it, false withholds
/// it, and saying nothing leaves what `lnk agent tool add` gave. Link's
/// spec sets neither.
pub const SCREENS: &[&str] = &["browser", "desktop"];

/// The keys only run's table takes, beside `output`.
pub(super) const RUN_ONLY: &[&str] = &[
    "background",
    "background_timeout",
    "background_output",
    "background_keep",
    "background_per_day",
];

/// The most background commands at once a spec may allow: a guard on a
/// number's size, each being a process and its sandbox.
pub(super) const BACKGROUND_MAX: u32 = 64;

/// The most a count of a spec's may be (rounds, calls, the scheduler's
/// caps): a guard on a number's size only, so a typo can't run for ever.
/// How much is the spec's to say; past [`UNUSUAL`], `lnk agent start`
/// says so ([`unusual`]).
pub(super) const COUNT_MAX: u32 = u16::MAX as u32;

/// Counts past which an agent runs further than any Link's tests do:
/// each key, and how far.
pub(super) const UNUSUAL: &[(&str, u32)] = &[
    ("turns.rounds", 64),
    ("turns.calls", 128),
    ("tool.scheduler.per_day", 100_000),
    ("tool.scheduler.subagents", 64),
    ("tool.scheduler.hops", 64),
];

/// Where a key of the union came from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase", tag = "from", content = "name")]
pub enum From {
    /// Link's spec.
    Link,
    /// A team's, by its name.
    Team(String),
    /// The agent's own: its file, or what Link knows of it here (its
    /// name, owner and teams).
    Yours,
    /// This machine's, in place of the place the agent prefers (named),
    /// which the machine doesn't support (`agents.toml`'s `[storage]`).
    Machine(String),
}

impl From {
    /// How `lnk agent spec show` marks it.
    pub fn mark(&self) -> String {
        match self {
            Self::Link => "Link's".into(),
            Self::Team(team) => format!("team {team}'s"),
            Self::Yours => "yours".into(),
            Self::Machine(preferred) => {
                format!("this machine's: it doesn't support {preferred}")
            }
        }
    }
}
