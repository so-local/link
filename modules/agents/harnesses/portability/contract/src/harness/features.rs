//! What an agent can do, by name: one list of features ([`Feature`]),
//! each switched by its own keys of the agent's spec, and the features
//! a harness honors ([`super::Contract::features`]).
//!
//! A feature is on where its spec's keys turn it on, and off where a
//! key is left out, `"none"`, `"never"` or false. Link refuses to start
//! an agent whose own spec turns on a feature its harness doesn't honor
//! ([`super::Contract::refused`]).
//!
//! The ids are the ones Link's comparison of harnesses uses, so a
//! feature found there and built later is a new variant and its keys.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent_spec::{From, Resolved, Spec};

/// A feature an agent can have, named by its id (`repo-instructions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    /// The repository's own `AGENTS.md`, else `CLAUDE.md`, in its prompt.
    RepoInstructions,
    /// Who it is and who its owner is, apart from any task.
    Persona,
    /// Its skills, each one's name and what it's for.
    Skills,
    /// Commands its owner's message runs: `/help`, `/status`, `/new`, `/stop`, `/model`, `/learn`.
    Commands,
    /// A goal it pursues after each answer until it holds: `/goal`.
    Goals,
    /// A second model for a turn its model can't answer.
    ModelFallback,
    /// A message's images.
    Images,
    /// A message's documents.
    Attachments,
    /// A message's voice notes, heard as their words.
    Voice,
    /// Group chats, answering when it's addressed.
    Groups,
    /// An answer shown on its connection as it's written.
    Streaming,
    /// Each tool it calls shown on its connection as it calls it.
    Activity,
    /// Tasks handed to subagents.
    Subagents,
    /// Tasks and messages at a time or on a cadence.
    Schedules,
    /// Messages to its owner it sends first.
    Proactive,
    /// Commands and programs it runs.
    Shell,
    /// Files it reads.
    FileRead,
    /// Files it writes and changes.
    FileEdit,
    /// Files found by name and lines by a pattern.
    CodeSearch,
    /// Each change to a file answered as a diff.
    Diffs,
    /// Language servers: problems, definitions and references.
    Lsp,
}

impl Feature {
    pub const ALL: [Feature; 21] = [
        Self::RepoInstructions,
        Self::Persona,
        Self::Skills,
        Self::Commands,
        Self::Goals,
        Self::ModelFallback,
        Self::Images,
        Self::Attachments,
        Self::Voice,
        Self::Groups,
        Self::Streaming,
        Self::Activity,
        Self::Subagents,
        Self::Schedules,
        Self::Proactive,
        Self::Shell,
        Self::FileRead,
        Self::FileEdit,
        Self::CodeSearch,
        Self::Diffs,
        Self::Lsp,
    ];

    /// Its id: how a spec's errors, a contract and Link's docs name it.
    pub fn id(self) -> &'static str {
        match self {
            Self::RepoInstructions => "repo-instructions",
            Self::Persona => "persona",
            Self::Skills => "skills",
            Self::Commands => "commands",
            Self::Goals => "goals",
            Self::ModelFallback => "model-fallback",
            Self::Images => "images",
            Self::Attachments => "attachments",
            Self::Voice => "voice",
            Self::Groups => "groups",
            Self::Streaming => "streaming",
            Self::Activity => "activity",
            Self::Subagents => "subagents",
            Self::Schedules => "schedules",
            Self::Proactive => "proactive",
            Self::Shell => "shell",
            Self::FileRead => "file-read",
            Self::FileEdit => "file-edit",
            Self::CodeSearch => "code-search",
            Self::Diffs => "diffs",
            Self::Lsp => "lsp",
        }
    }

    /// It in a few words, as a switch says what won't come along
    /// (`schedules`).
    pub fn words(self) -> &'static str {
        match self {
            Self::RepoInstructions => "the repository's instructions",
            Self::Persona => "persona",
            Self::Skills => "skills",
            Self::Commands => "commands",
            Self::Goals => "goals",
            Self::ModelFallback => "a fallback model",
            Self::Images => "images",
            Self::Attachments => "documents",
            Self::Voice => "voice notes",
            Self::Groups => "group chats",
            Self::Streaming => "answers shown as they're written",
            Self::Activity => "the tools it calls shown as it calls them",
            Self::Subagents => "subagents",
            Self::Schedules => "schedules",
            Self::Proactive => "messages it sends first",
            Self::Shell => "its shell",
            Self::FileRead => "reading files",
            Self::FileEdit => "editing files",
            Self::CodeSearch => "code search",
            Self::Diffs => "diffs",
            Self::Lsp => "language servers",
        }
    }

    /// The feature `id` names, if one does.
    pub fn of(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    /// The spec's keys that switch it, dotted (`tool.file.enabled`): it's
    /// on when each group holds a key that's on. A key of `[connections]`
    /// is also on where a connection's own table turns it on
    /// (`connection.slack.images`).
    pub fn switches(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::RepoInstructions => &[&["instructions.repository"]],
            Self::Persona => &[&["instructions.persona", "instructions.owner"]],
            Self::Skills => &[&["instructions.skills"]],
            Self::Commands => &[&[
                "commands.help",
                "commands.status",
                "commands.create",
                "commands.stop",
                "commands.model",
                "commands.write",
                "commands.resume",
                "commands.fork",
                "commands.rename",
                "commands.export",
                "commands.queue",
                "commands.steer",
                "commands.compact",
                "commands.ask",
                "commands.skills",
                "commands.verbose",
            ]],
            Self::Goals => &[&["commands.goal"], &["goals.turns"]],
            Self::ModelFallback => &[&["model.fallback"]],
            Self::Images => &[&["connections.images"]],
            Self::Attachments => &[&["connections.documents"]],
            Self::Voice => &[&["connections.voice"]],
            Self::Groups => &[&["connections.groups"]],
            Self::Streaming => &[&["connections.stream"]],
            Self::Activity => &[&["connections.stream"], &["connections.activity"]],
            Self::Subagents | Self::Schedules | Self::Proactive => &[&["tool.scheduler.enabled"]],
            Self::Shell => &[&["tool.run.enabled"]],
            Self::FileRead | Self::FileEdit => &[&["tool.file.enabled"]],
            Self::CodeSearch => &[&["tool.file.enabled"], &["tool.file.find"]],
            Self::Diffs => &[&["tool.file.enabled"], &["tool.file.diffs"]],
            Self::Lsp => &[&["tool.code.enabled"]],
        }
    }

    /// Its first key, as a spec's table and key (`[tool.file] find`):
    /// how a refusal names where to turn it off.
    pub fn table(self) -> String {
        let key = self.switches()[0][0];
        let (table, key) = key.rsplit_once('.').unwrap_or(("", key));
        format!("[{table}] {key}")
    }

    /// Whether `spec`, an agent's union, turns it on.
    pub fn on(self, spec: &Spec) -> bool {
        let spec = toml::Value::try_from(spec).unwrap_or(toml::Value::Boolean(false));
        self.on_in(&spec)
    }

    fn on_in(self, spec: &toml::Value) -> bool {
        if matches!(self, Self::RepoInstructions | Self::Persona | Self::Skills)
            && spec
                .get("instructions")
                .and_then(|i| i.get("source"))
                .and_then(toml::Value::as_str)
                .and_then(|source| crate::agent_spec::Source::parse(source).ok())
                .is_some_and(|source| source != crate::agent_spec::Source::Link)
        {
            return false;
        }
        self.switches()
            .iter()
            .all(|keys| keys.iter().any(|key| paths(spec, key).any(|v| is_on(&v))))
    }

    /// The features `spec`, an agent's union, turns on.
    pub fn of_spec(spec: &Spec) -> BTreeSet<Feature> {
        let spec = toml::Value::try_from(spec).unwrap_or(toml::Value::Boolean(false));
        Self::ALL.into_iter().filter(|f| f.on_in(&spec)).collect()
    }

    /// The features `resolved` turns on that its agent chose: each on,
    /// with a key of it the agent's own spec turns on, not left to Link's
    /// spec or its teams'.
    pub fn chosen(resolved: &Resolved) -> BTreeSet<Feature> {
        let spec = toml::Value::try_from(&resolved.spec).unwrap_or(toml::Value::Boolean(false));
        let chose = |key: &str| {
            paths(&spec, key).any(|v| is_on(&v))
                && resolved
                    .from
                    .iter()
                    .any(|(at, whose)| matches!(whose, From::Yours) && names(at, key))
        };
        Self::of_spec(&resolved.spec)
            .into_iter()
            .filter(|f| f.switches().iter().flat_map(|k| k.iter()).any(|k| chose(k)))
            .collect()
    }
}

impl Serialize for Feature {
    fn serialize<S: serde::Serializer>(&self, se: S) -> Result<S::Ok, S::Error> {
        se.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for Feature {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let id = String::deserialize(de)?;
        Self::of(&id).ok_or_else(|| serde::de::Error::custom(format!("{:?} isn't a feature", id)))
    }
}

/// How a harness honors a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Honor {
    /// As the agent's spec says: it reads the feature's keys, or Link
    /// does for it (a connection Link carries to its endpoint).
    Spec,
    /// Its own way, whatever the spec says: Link leaves it to the
    /// harness (a harness reading `CLAUDE.md` itself).
    Native,
}

/// The features a harness honors, each how.
pub type Honors = BTreeMap<Feature, Honor>;

/// Reads [`super::Contract::features`]: a feature or a way to honor it
/// this Link doesn't know (a newer adapter's) is left out, never the
/// whole contract refused.
pub(super) fn read<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<Honors>, D::Error> {
    let Some(Value::Object(map)) = Option::<Value>::deserialize(de)? else {
        return Ok(None);
    };
    Ok(Some(
        map.into_iter()
            .filter_map(|(id, how)| Some((Feature::of(&id)?, serde_json::from_value(how).ok()?)))
            .collect(),
    ))
}

/// What an adapter before the list honors, as its flags say: what Link
/// gave it for each flag, as the spec said; groups and streaming
/// wherever Link carries its connections. No commands and no fallback:
/// those were Link Harness's alone.
fn of_flags(contract: &super::Contract) -> Honors {
    let mut honors = Honors::new();
    let mut add = |on: bool, features: &[Feature]| {
        if on {
            honors.extend(features.iter().map(|f| (*f, Honor::Spec)));
        }
    };
    use Feature::*;
    add(true, &[Groups, Streaming]);
    add(contract.instructions, &[RepoInstructions, Persona, Skills]);
    add(contract.parts, &[Images, Attachments]);
    add(contract.from, &[Subagents, Schedules, Proactive]);
    add(
        contract.folders,
        &[Shell, FileRead, FileEdit, CodeSearch, Diffs, Lsp],
    );
    honors
}

impl super::Contract {
    /// The features it honors: its list, else what its flags imply.
    pub fn honors(&self) -> Honors {
        self.features.clone().unwrap_or_else(|| of_flags(self))
    }

    /// The first feature `resolved`, an agent's spec, chose to turn on
    /// ([`Feature::chosen`]) that it doesn't honor: Link refuses
    /// to start the agent on it, naming the feature.
    pub fn refused(&self, resolved: &Resolved) -> Option<Feature> {
        let honors = self.honors();
        Feature::chosen(resolved)
            .into_iter()
            .find(|feature| !honors.contains_key(feature))
    }
}

/// Whether a spec's value turns a feature on: anything but none, never
/// and false.
fn is_on(value: &toml::Value) -> bool {
    match value {
        toml::Value::Boolean(on) => *on,
        toml::Value::String(word) => !matches!(word.as_str(), "none" | "never" | ""),
        _ => true,
    }
}

/// `key`'s values in `spec`: its own, and for a key of `[connections]`,
/// each connection's of the same name.
fn paths<'a>(spec: &'a toml::Value, key: &'a str) -> impl Iterator<Item = toml::Value> + 'a {
    let own = at(spec, key.split('.'));
    let connections = key.strip_prefix("connections.").map(|leaf| {
        spec.get("connection")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flat_map(|connections| connections.values())
            .filter_map(move |connection| connection.get(leaf).cloned())
    });
    own.into_iter().chain(connections.into_iter().flatten())
}

fn at<'a>(spec: &toml::Value, mut parts: impl Iterator<Item = &'a str>) -> Option<toml::Value> {
    parts.try_fold(spec, |value, part| value.get(part)).cloned()
}

/// Whether `at`, a key of a spec's union, is `key` or, for a key of
/// `[connections]`, a connection's of the same name.
fn names(at: &str, key: &str) -> bool {
    at == key
        || key.strip_prefix("connections.").is_some_and(|leaf| {
            at.strip_prefix("connection.")
                .and_then(|rest| rest.split_once('.'))
                .is_some_and(|(_, own)| own == leaf)
        })
}

#[cfg(test)]
mod tests;
