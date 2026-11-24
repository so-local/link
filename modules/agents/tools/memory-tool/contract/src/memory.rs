//! The contract between the harness module (`lnk-agent`) and a memory
//! engine's adapter (`lnk-memory-tool`, ...), for `lnk agent memory start
//! <engine>`.
//!
//! An adapter is a program answering the commands this crate declares
//! (`actions.toml`, generated in [`crate::declared`]), each listed under
//! its `[implements]`:
//!
//! ```text
//! lnk-<engine> install                 install the engine, or leave it; no questions asked
//! lnk-<engine> command <folder> <state> [<agent>]
//!                                      prints a `Launch` as JSON: how to serve an MCP
//!                                      server over stdio for <folder> (the agent's Home, or
//!                                      a team's shared folder), keeping what it keeps (its
//!                                      index, its database) in <state>; with <agent>, the
//!                                      agent's own folder ([`crate::folder`]), its
//!                                      `chats/` and `skills/` read there, not in
//!                                      <folder>
//! lnk-<engine> serve-many <socket> <agents> [--cache <folder> --stay]
//!                                      for an engine whose `Launch::shared` says so: serves
//!                                      every agent <agents> names (`Agents`) on the Unix
//!                                      socket <socket>, each session's first line a
//!                                      [`Hello`] naming its agent, answered with a
//!                                      [`Welcome`], then MCP as over stdio; it ends once no
//!                                      session has been open for a moment (with `--stay`,
//!                                      never). A session's [`Hello`] may hand it the agent's
//!                                      bridges ([`Bound`]): its collections are then read and
//!                                      written through them, its index a cache (in
//!                                      `--cache` for an agent <agents> doesn't name) of the
//!                                      copy kept with its `memory` collection, written back
//!                                      in batches, and on a [`Control`]
//! ```
//!
//! Link builds each line from the generated arguments
//! ([`crate::declared::LaunchShowArgs`] and the like, their `argv()`),
//! and an engine's parser, generated from the same declarations, reads it.
//!
//! `lnk-agent` runs the engine as a tool outside the agent's sandbox
//! (`lnk sandbox tool add`), itself confined: it reads `<folder>`,
//! `<agent>`'s conversations and skills, and the engine's own program,
//! and writes only `<state>`, with the network only
//! when it asks for this machine's models ([`Launch::models`](crate::declared::Launch::models)). An
//! embedding model it names ([`Launch::model`](crate::declared::Launch::model)) is served for it outside
//! its sandbox, `lnk model serve --model <model> --json`, while it runs, and its
//! address is in [`MODEL_ENV`], as a harness is given its tools'. The
//! agent reaches the engine through its proxy, at [`MEMORY_ENV`] in its
//! environment. What the engine's tools are is the engine's own: the tool
//! bridge passes them through as they are.
//!
//! Where the agent may write its instructions (its sandbox file's
//! `[files] write` holds `instructions`), the engine may write one file
//! of them, the agent's notes: [`NOTES_ENV`] names it to `command`, and
//! the engine's sandbox writes the folder it's in. Nothing else of
//! either folder is ever written.
//!
//! An engine that serves many folders from one process says so
//! ([`Launch::shared`](crate::declared::Launch::shared)): then one process serves every agent of one owner
//! on the machine, confined to the union of what each agent's would be
//! (their folders read, their states and notes written), and each
//! session is answered from the agent Link opened it for, named in its
//! [`Hello`] by Link, outside every sandbox, never by the agent.

use link_secretive::Duplicate;

use serde::{Deserialize, Serialize};

pub mod embedding;

/// The variable holding the agent's memory's address, in its harness's
/// environment: an MCP server over HTTP.
pub const MEMORY_ENV: &str = "LNK_MEMORY";

/// The variable holding the address of the model [`Launch::model`](crate::declared::Launch::model) names,
/// in the engine's environment: an OpenAI-style API (`/v1/embeddings`)
/// on this machine, served for it outside its sandbox. Empty when it
/// couldn't be served: no runtime here has the model.
pub const MODEL_ENV: &str = "LNK_MEMORY_MODEL";

/// The agent's opinions for its memory, in the environment of every
/// command of the engine's: its spec's `[tool.memory]` as JSON
/// ([`crate::agent_spec::Tool`]), `model` the embedding model it ranks
/// by and `tokens` the most a search returns. Unset: the engine's own.
pub const SPEC_ENV: &str = "LNK_MEMORY_SPEC";

/// The agent's notes file, in `command`'s environment: a path in
/// `<agent>` (`memory/notes.md` in it) the engine may write, what
/// the agent notes for itself, its size the spec's `notes`
/// ([`SPEC_ENV`]). Unset, the agent may not write its instructions, and
/// the engine writes nothing of the folder.
pub const NOTES_ENV: &str = "LNK_MEMORY_NOTES";

/// Whether the agent's harness is sent messages from others (its
/// contract's `from`), in `command`'s environment: `1` when it is. Then
/// each call names its conversation (`_meta` `link.local/thread`), and an
/// engine that writes the agent's notes ([`NOTES_ENV`]) writes them only
/// in a conversation that is the owner's, refusing a call that names
/// none. Unset, every conversation is the owner's.
pub const FROM_ENV: &str = "LNK_MEMORY_FROM";

/// The store driver the agent's spec keeps its conversations by
/// (`[storage] conversations`, when it names one), in `command`'s
/// environment: the only one the engine runs to read a conversation
/// whose folder holds `store.<name>`. A folder naming another is never
/// read: the agent writes its folders, and never picks what the engine
/// runs. Unset, the engine runs no driver.
pub const DRIVER_ENV: &str = "LNK_MEMORY_DRIVER";

/// The most an agent's notes hold, in bytes, unless its spec's
/// `[tool.memory] notes` says.
pub const NOTES_MOST: u64 = 4096;

/// The most agent's notes hold, as its memory's opinions `tool` say.
pub fn notes_most(tool: &crate::agent_spec::Tool) -> anyhow::Result<u64> {
    match &tool.notes {
        None => Ok(NOTES_MOST),
        Some(notes) => {
            let most = crate::agent_spec::size(notes)?;
            anyhow::ensure!(most > 0, "memory's notes hold at least a byte");
            Ok(most)
        }
    }
}

/// A session's first line to a shared engine: the agent it's for, an ID
/// of [`Agents`](crate::declared::Agents) or one bound ([`Bound`]), written by Link outside every
/// sandbox, never by the agent.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub agent: String,
    /// The agent's bridges, and its key for them, where its collections
    /// are reached through them: the engine then reads and writes them
    /// only there, each a socket of the machine's own (the service's
    /// `--files-bridge`, `--storage-bridge`, or its own in `--bridges`),
    /// and serves it only once a bridge takes the key. A session for an
    /// agent bound already names the same again, and is served as it was
    /// bound; a session without it is served only an agent [`Agents`](crate::declared::Agents)
    /// names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub bound: Option<Bound>,
    /// A team's memory, not the agent's own ([`TeamSession`]): reached
    /// through its files bridge as the agent, with the agent's key, and
    /// its index the one kept beside the team's folder. Only with `bound`
    /// naming a files bridge, which serves the index only to a member
    /// granted all of the folder. Its own session of the agent's, by
    /// owner, agent and the team's ID: never its notes, never its
    /// conversations, and ended with the agent's ([`Control::End`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamSession>,
}

/// A team's memory, as Link names it for an agent's session of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamSession {
    /// The team's permanent ID: the session and its cache are the team's
    /// by it, never by a name another team may take.
    pub id: String,
    /// The team as the agent names it to its files bridge now.
    pub named: TeamRef,
    /// Whether the agent writes all of the team's folder: then it keeps
    /// the team's index, writing it back beside the folder; else its
    /// session reads the copy there and writes nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub writes: bool,
    /// The current team policy permits paid semantic queries by readers.
    /// The embedding service still checks current membership and funding.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub semantic: bool,
}

/// A team as a member names it, at each call ([`link_store_contract`]'s
/// files bridge): `<name>` its own account's, `<account>/<name>` another's.
pub use link_store_contract::files::TeamRef;

/// Where an engine's index copy is reached for a binding. Its notes are
/// always part of the memory collection; the engine's index is its own state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexAt {
    /// The resolved memory place, or beside Home when there is no remote place.
    #[default]
    Memory,
    /// Beside Home through the supplied files bridge.
    Home,
}

/// An agent as its bridges reach it: its owner, its key for this start,
/// which proves the engine's calls for it ([`crate::bridge`]) and is
/// dropped at [`Control::End`] (never printed), and its bridges.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    /// Where the engine keeps its index copy.
    #[serde(default)]
    pub index: IndexAt,
    pub owner: String,
    /// The service the agent acts as, its bare ID: who its calls through
    /// its bridges are proved as, beside its ID ([`crate::bridge`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub service: String,
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub key: link_secretive::Secret,
    /// The files bridge's socket ([`crate::files`]), where its Home is
    /// kept in a bucket: its `memory/` read there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<std::path::PathBuf>,
    /// Where its collections are kept, and the storage bridge reaching
    /// them ([`crate::storage::StorageAt`]): its conversations, its notes
    /// and its index's copy, wherever its machine keeps each.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<crate::storage::StorageAt>,
    /// The most its notes may hold, in bytes, where it may write them
    /// (`remember`, its notes in its `memory` collection, through
    /// the storage bridge): none, it writes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<u64>,
}

/// What Link tells a shared engine about a bound agent, named by its
/// owner and ID as every session is, as a session's whole first line,
/// answered with a [`Welcome`] once done. Taken only from the user the
/// engine runs as, as every session is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Control {
    /// `path` of its Home changed: read it again soon, if it's one the
    /// engine keeps. With `team`, a team's ID, `path` of that team's
    /// folder, for its session of the team's memory ([`Hello::team`]).
    Changed {
        owner: String,
        agent: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<String>,
    },
    /// Write its index back to its copy now, and each of its teams'
    /// memories' to theirs: it's about to sleep.
    Flush { owner: String, agent: String },
    /// It slept: its key is dropped, its teams' memories' sessions too,
    /// and nothing more is done for it until it's bound again. Its caches
    /// stay.
    End { owner: String, agent: String },
}

/// A shared engine's answer to a [`Hello`]: `ok`, or why not (an agent
/// it doesn't serve), after which it closes the session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
