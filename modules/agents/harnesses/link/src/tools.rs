//! Tools: MCP servers that run outside the harness, which it reaches as
//! a client over MCP's HTTP transport. Behind its proxy, those are the
//! tools Link's tool bridge runs for the agent (`$LNK_TOOL_<NAME>`, and
//! its memory at `$LNK_MEMORY`), so a tool written once works for every
//! harness, and Link Harness runs none of their code.
//!
//! A turn offers each server's tools as it last listed them
//! ([`Listed`]), and opens a session with a server only to call it. The
//! session is kept for the turns after ([`Kept`]), and ended once no turn
//! has used it for [`IDLE`], so a tool's process (memory's index and
//! model, say) starts once, not with every message. What's listed is
//! derived from the servers: a process that hasn't listed a server yet
//! lists it at its turn's start, all servers at once, so any process can
//! take the next turn. What each listed is kept beside the agent's
//! settings too ([`Lists`]): a server that fails to list its tools in a
//! new process is offered as the last listed it, so the prompt, and the
//! provider's cache, hold across a restart, and asked again at the next
//! turn, so a tool added since appears. A process's first ask waits for
//! its sandbox to grant the tool, which a restart grants as it finishes.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::bail;
use link_connection_contract::SseLines;
use link_format::WithinExt;
use serde_json::{Value, json};

use crate::agent::Agent;
use crate::event::{Call, Media};
mod answers;
mod capture;
mod skills;
pub(crate) use capture::{captured, dispatch, request_projection, whole_every};
pub(crate) use skills::loaded;
mod keeping;
mod listing;
mod observation;
mod servers;
mod sessions;
mod toolbox;
use answers::cut;
pub use answers::{Answered, answered};
pub use keeping::Kept;
pub use listing::{KeptTool, List, Listed, Lists};
pub use observation::{Dispatch, Observation, Observer};
pub use servers::{Server, from_env, names_a_tool};
use sessions::{Grant, Interrupted, NAME_MAX, Refused, TooLong, plain_name};
pub use sessions::{Session, start_limit};
pub use toolbox::Toolbox;

/// The key of a call's `_meta` its server's own opinions are in.
pub const SPEC_META: &str = "link.local/spec";

/// The key of a listed tool's `_meta` naming, in order, the arguments
/// that say what a call of it works on (`["path"]`), for its title.
pub const SUMMARY_META: &str = "link.local/summary";

/// Between a server's name and its tool's, in the name the model calls.
pub const SEPARATOR: &str = link_connection_contract::policy::TOOL_SEPARATOR;

/// The most tools one server may offer.
const TOOLS_MAX: usize = 128;

/// The most of a tool's description kept, in bytes: the rest is cut.
const DESCRIPTION_MAX: usize = 2 << 10;

/// The most of what a server says of itself (MCP's `instructions`)
/// kept, in bytes: the rest is cut.
const INSTRUCTIONS_MAX: usize = 2 << 10;

/// The most a tool's schema may take, as JSON: a tool with a larger one
/// is left out.
const SCHEMA_MAX: usize = 16 << 10;

/// The most every tool offered takes, together: names, descriptions and
/// schemas, in bytes. A tool past it is left out, so the tools can't
/// crowd out the conversation in every prompt.
const OFFERED_MAX: usize = 128 << 10;

/// The most arguments a tool's calls may be summed up by ([`SUMMARY_META`]).
const SUMMARY_MAX: usize = 4;

/// The most pages of `tools/list` read from one server, each cursor once.
const PAGES_MAX: usize = 16;

/// How long a turn waits for a server to start a session and list its
/// tools, before it's left out of the turn: at its first listing, at each
/// start after, and at each listing after a call, which keeps what it
/// listed before. A first listing still going then goes on, within
/// [`start_limit`], for the turns after.
pub const OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How long a call may take when no spec says: Link's (`[tools] timeout`).
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const INTERACTION_MAX_WAIT_MS: u64 = 24 * 60 * 60 * 1000;

/// How long a server that failed to start is left out of turns before
/// it's tried again: a broken tool doesn't hold up every message.
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// How long a kept session lives: ended once no turn has used it for
/// the first, and once it started the second ago, however busy, so
/// what it started without (memory's model, not up yet) is tried again
/// and an old process doesn't run for good. The agent's spec's
/// (`[tool.memory] keep`, `max_age`).
pub type Life = (std::time::Duration, std::time::Duration);

pub use link_model_contract::chat::Tool;

/// A server's tools as it listed them, the model's name for each and
/// its own, and what it said of itself when its session started.
#[derive(Clone, Debug, Default)]
struct Offered {
    tools: Vec<(Tool, String)>,
    /// MCP's `instructions`, cut at [`INSTRUCTIONS_MAX`]: how to use its
    /// tools together, for the prompt's tools section.
    instructions: String,
    /// The arguments each tool's call is summed up by in its title
    /// ([`SUMMARY_META`]), by the model's name for it: for a turn's
    /// activity only, so never kept ([`Lists`]) or compared, and none of
    /// a list a process kept.
    summaries: HashMap<String, Vec<String>>,
    /// The model's names of the tools that only read (MCP's
    /// `readOnlyHint`), whose calls in one step run beside each other
    /// ([`crate::turn`]'s order of a step's calls): never kept, so a tool
    /// offered as a process kept it is taken as one that changes things.
    reads: std::collections::HashSet<String>,
}

/// As offered to the model: whatever its titles' summaries.
impl PartialEq for Offered {
    fn eq(&self, other: &Self) -> bool {
        self.tools == other.tools && self.instructions == other.instructions
    }
}
