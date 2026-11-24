//! An agent's spec: what it is and its opinions, a TOML file
//! (`docs/architecture/specs/agent.md`). Every choice a reasonable person could
//! make differently (how much of the window to use, how many rounds a
//! turn may take, a reaction while it answers) is a key here, never a
//! constant in a harness or a bridge, which keep only the decisions:
//! what anyone with the whole evidence would choose.
//!
//! Specs meet as sets. Link's opinions are a spec Link includes
//! ([`LINK_SPEC`]); a team's file is a spec its members share; yours is
//! the agent's own. The agent runs their union, keys from later sets
//! winning: Link's, then its teams', then yours ([`resolve`]). Any of
//! them may be partial, but the union must hold every key
//! ([`REQUIRED`]), each tool's own keys its tool's to need, and two teams setting one key differently refuse the
//! start unless yours sets it too. `extends = "none"` starts from
//! nothing, not Link's set.
//!
//! The type is here, beside the harness contract, since the harness
//! module keeps one for each agent and hands the harness its opinions
//! (`Settings::spec`), and the connections' bridges theirs.

use link_format::WithinExt;
use std::collections::BTreeMap;

use anyhow::{bail, ensure};
use serde::Serialize;
mod checking;
mod keys;
mod schedules;
pub use schedules::{Schedule, with_schedules};
mod resolving;
mod teams;
mod tools;
pub use checking::{check, is_zone, message_access, millis, size};
pub use keys::{
    ACCESS, AUTO, Busy, Commands, CompactAt, Extends, Goals, Harness, Instructions, Model, NEVER,
    Quiet, Run, SECTION_MAX, SectionsMax, Source, Spec, Thinking, Turns, Window,
};
use link_spec::Specification;
pub use resolving::{Known, resolve, resolve_member, unusual};
pub(crate) use teams::check_team;
pub use teams::{
    PURPOSE_MAX, TEAM_ACCESS, TeamDefinition, TeamFile, assign_team_ids, parse_team,
    parse_team_definition, shared_folder, team_document, team_text,
};
use tools::{BACKGROUND_MAX, COUNT_MAX, MEMORY_ONLY, RUN_ONLY, SCHEDULER_ONLY, UNUSUAL};
pub use tools::{
    CodeServer, From, LINK_TOOLS, Late, SCREENS, SWITCHED, SchedulerDriver, SchedulerRecurring,
    Tool,
};

/// Link's opinions, as a spec ([`resolve`]'s first set).
pub const LINK_SPEC: &str = include_str!("agent_spec/link.toml");

/// Link's sandbox file, beside its spec: what an agent may do where its
/// own sandbox file doesn't say (the harness module reads it).
pub const LINK_SANDBOX: &str = include_str!("agent_spec/sandbox.toml");

/// The keys every agent's union must hold. A key the harness doesn't
/// take is still the union's: switching harness keeps every opinion. A
/// tool's own keys (`[tool.<name>]`) aren't among them: each is its
/// tool's to need, and it says which it lacks when it starts, so a spec
/// that extends none sets only the tools it runs.
/// Instruction section keys are required only for Link's assembly.
pub const REQUIRED: &[&str] = &[
    "name",
    "model.thinking",
    "model.thinking_budget",
    "window.unknown",
    "window.margin",
    "window.answer",
    "window.compact_at",
    "window.save_notes",
    "window.past_price_jump",
    "turns.rounds",
    "turns.calls",
    "turns.busy",
    "turns.renew",
    "turns.lapse",
    "instructions.base",
    "instructions.environment",
    "instructions.skills",
    "instructions.persona",
    "instructions.owner",
    "instructions.standing",
    "instructions.now",
    "instructions.notes",
    "instructions.max.base",
    "instructions.max.model",
    "instructions.max.tools",
    "instructions.max.environment",
    "instructions.max.skills",
    "instructions.max.persona",
    "instructions.max.owner",
    "instructions.max.standing",
    "instructions.max.now",
    "instructions.max.notes",
    "tools.answer",
    "tools.timeout",
    "connections.stream",
    "connections.activity",
    "connections.pace",
    "connections.groups",
    "connections.ask_wait",
    "connections.cache",
    "connections.compact_after",
    "connections.pauses",
    "storage.conversations",
    "run.idle_after",
];

/// Link's base section (`[instructions] base = "link"`).
pub const LINK_BASE: &str = include_str!("agent_spec/base.md");

/// The spec an agent runs: the union, and where each key came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub spec: Spec,
    /// Each key (`turns.rounds`, `tool.memory.keep`) and whose it is.
    pub from: BTreeMap<String, From>,
}

/// Parses `text` as an agent's spec, strictly, its values checked.
pub fn parse(text: &str) -> anyhow::Result<Spec> {
    let spec: Spec = link_spec::parse_spec(text)?;
    check(&spec)?;
    Ok(spec)
}

fn editable_document(text: &str) -> anyhow::Result<toml_edit::DocumentMut> {
    Ok(Spec::format().document(text)?.0)
}

/// `text`, an agent's spec, with its `[access] message` set to `message`
/// and nothing else changed: its comments, its order and its other keys
/// kept, as `lnk agent share` and `unshare` write it. None left, the
/// key goes, and the table with it when
/// it holds nothing else. An error when `text` isn't a spec, or wouldn't
/// be one after.
pub fn with_message_access(text: &str, message: &[crate::access::Chat]) -> anyhow::Result<String> {
    parse(text)?;
    let mut doc = editable_document(text)?;
    if message.is_empty() {
        if let Some(access) = doc.get_mut("access").and_then(|a| a.as_table_like_mut()) {
            access.remove("message");
            if access.is_empty() {
                doc.remove("access");
            }
        }
    } else {
        let mut chats = toml_edit::Array::new();
        for chat in message {
            chats.push(chat.as_str());
        }
        if doc.get("access").is_none() {
            doc.insert("access", toml_edit::table());
        }
        let access = doc
            .get_mut("access")
            .and_then(|a| a.as_table_like_mut())
            .within("[access] isn't a table")?;
        access.insert("message", toml_edit::value(chats));
    }
    let written = doc.to_string();
    parse(&written)?;
    Ok(written)
}

/// Changes only a tool's enabled opinion, preserving comments and other keys.
pub fn with_tool_enabled(text: &str, tool: &str, enabled: bool) -> anyhow::Result<String> {
    parse(text)?;
    let mut doc = editable_document(text)?;
    if doc.get("tool").is_none() {
        doc.insert("tool", toml_edit::table());
    }
    let tools = doc
        .get_mut("tool")
        .and_then(|item| item.as_table_like_mut())
        .within("[tool] isn't a table")?;
    if tools.get(tool).is_none() {
        tools.insert(tool, toml_edit::table());
    }
    let own = tools
        .get_mut(tool)
        .and_then(|item| item.as_table_like_mut())
        .within("the tool's opinions aren't a table")?;
    let mut value = toml_edit::Value::from(enabled);
    if let Some(previous) = own.get("enabled").and_then(|item| item.as_value()) {
        *value.decor_mut() = previous.decor().clone();
    }
    own.insert("enabled", toml_edit::Item::Value(value));
    let written = doc.to_string();
    parse(&written)?;
    Ok(written)
}

/// Link's spec, parsed once.
pub fn link() -> Spec {
    // Checked by a test: Link's own spec always parses. Its names are
    // this release's, so it's read as written, never following the
    // release's history, which a sandboxed harness can't ask `lnk` for.
    static LINK: std::sync::LazyLock<Spec> = std::sync::LazyLock::new(|| {
        link_spec::parse_current::<Spec>(LINK_SPEC)
            .and_then(|spec| check(&spec).map(|()| spec))
            .unwrap_or_default()
    });
    LINK.clone()
}

pub type Member = link_agent_team::Member<Spec>;
pub type MemberDefinition = link_agent_team::Member<Spec, Option<String>>;
pub use crate::declared::{
    TeamIndexer, TeamIndexerMember, TeamMemory, TeamPayer, TeamPayerMember, TeamSettings,
};
pub type Team = link_agent_team::Team<Spec, TeamSettings>;

/// The index keeper a team's spec names, for membership's check.
impl link_agent_team::Settings for TeamSettings {
    fn indexer(&self) -> Option<&str> {
        match &self.memory.indexer {
            TeamIndexer::Auto(_) => None,
            TeamIndexer::Member(named) => Some(&named.member),
        }
    }
}

pub use link_connection_policy::{Activity, Connection, Groups, duration_ms};

pub use link_store_contract::storage::{COMPACT, LOG, Storage};

#[cfg(test)]
mod tests;
