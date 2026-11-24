//! The files bridge: an agent's Home where it's kept as objects in a
//! bucket, not a folder on the machine that runs the agent. A bridge as
//! [`crate::bridge`] defines one: every call is the agent's identity, the
//! action (read, write, list a path of its Home; get or put its memory's
//! index) and its proof, and the bridge, outside every sandbox, holds
//! what reaches the bucket and reaches only the calling agent's Home
//! ([`Homes`]).
//!
//! **Objects.** A [`Store`] is a bucket's objects, each with a version
//! (the bucket's generation): a write names the version it read
//! ([`Expect`]), and is refused if the object has changed since
//! ([`Conflict`]), so two writers of one file never lose a write
//! silently. Home's files are under `home/`, its memory's index under
//! `index/`. [`DirStore`] keeps objects in a folder (a stand-in, and a
//! machine's); [`Crypt`] keeps them encrypted in another store, in
//! rclone's crypt format ([`crypt`]), names too, under a key of the
//! agent's own.
//!
//! **Teams.** A `read`, `write` or `list` naming a `team` ([`TeamRef`])
//! reaches that team's folder instead of Home: what [`Homes::team`] gives
//! it at that very call, so an agent taken off a team, or a team renamed
//! or removed, is refused at its next call, never served from what was
//! true when it was bound. It reaches only the folders its [`TeamFolders`]
//! grant, writes only from the version read, and lists only what it
//! reaches. An `index-*` call naming a team reaches the index kept beside
//! that team's folder, in the team owner's storage, and only for a member
//! granted all of the folder: its team's memory ([`InTeam`] is the
//! caller's side). A member's write tells [`Homes::team_changed`].
//!
//! **A team's folder elsewhere.** A team whose folder this bridge doesn't
//! hold (a hosted team's, kept in its owner's bucket, for a member on its
//! own machine) is answered by that folder's service: [`Homes::remote_team`]
//! names it at each call, and [`RemoteTeam`] hands the call on with the
//! member's own token, made outside the sandbox. The service runs the same
//! checks ([`TeamCalls`]) for the member its token names, at every call.
//!
//! [`TeamFolders`]: crate::harness::TeamFolders
//!
//! **Calls.** [`Action`] on the wire, answered with [`Answer`]: `read`
//! and `write` a file of Home's, `list` what's under a folder of it,
//! `index-get`, `index-put`, `index-list` and `index-delete` the objects
//! of its index. A write past
//! Home's cap is refused, saying how much it holds; a path outside Home,
//! a hidden one, or one its grants don't let it write, too. [`Client`] is
//! the caller's side (the `files-client` feature); the bridge's own side,
//! `serve`, `Homes`, `DirStore` and `Crypt`, is the `files-bridge`
//! feature's, for whatever serves it.

#[cfg(feature = "files-bridge")]
mod approvals;
mod client;
#[cfg(feature = "files-bridge")]
pub mod crypt;
#[cfg(feature = "files-bridge")]
mod dir;
#[cfg(feature = "files-bridge")]
mod encrypted;
mod objects;
mod remote;
#[cfg(feature = "files-bridge")]
pub use approvals::PreparedWrite;
#[cfg(feature = "files-bridge")]
mod serving;
mod wire;
pub use crate::constants::TEAM_FILES_ROUTE;
pub use client::{Client, InTeam, size};
#[cfg(feature = "files-bridge")]
pub use dir::DirStore;
#[cfg(feature = "files-bridge")]
pub use encrypted::Crypt;
pub use objects::{
    Capacity, Conflict, Expect, Object, Store, Version, check_name, is_conflict, spool,
};
#[cfg(feature = "files-bridge")]
pub use remote::{OnTeam, RemoteTeam, RemoteTeams, TokenSource};
pub use remote::{member_grants, team_route};
#[cfg(all(test, feature = "files-bridge"))]
use serving::{Bridge, get, index_of, serve_bridge};
#[cfg(feature = "files-bridge")]
pub use serving::{
    Home, Homes, TEAM_INDEX_MAX, TeamCalls, TeamFolder, serve, serve_until, team_index_capacity,
    team_role,
};
pub use wire::{Action, Answer, TeamRef, check_path};

use link_digest::sha256;
use link_secretive::Duplicate;
#[cfg(feature = "files-bridge")]
use std::collections::BTreeMap;
#[cfg(feature = "files-bridge")]
use std::os::unix::net::UnixListener;
#[cfg(feature = "files-bridge")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "files-bridge")]
use std::sync::Mutex;
#[cfg(feature = "files-bridge")]
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, bail, ensure};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::bridge::Binding;
use crate::bridge::{Failure, Why};
#[cfg(feature = "files-bridge")]
use crate::bridge::{Identity, Keys};

/// Where Home's files are among the bucket's objects.
pub const HOME: &str = "home/";

/// Where its memory's index is: `index/<engine>/...`.
pub const INDEX: &str = "index/";

/// The largest file of Home's a call writes: as Link File writes.
pub const WRITE_MAX: u64 = 4 << 20;

/// The largest object a call reads: a document memory indexes, or a
/// segment of its index.
pub const READ_MAX: u64 = 32 << 20;

/// The most an answer is, as JSON: the largest object read, in base64,
/// and room for the rest.
pub const ANSWER_MAX: u64 = READ_MAX / 3 * 4 + (1 << 20);

// An index object at its most fits in a bridge call.
const _: () = assert!(ANSWER_MAX <= crate::bridge::CALL_MAX);

/// The most a listing gives.
pub const LIST_MAX: usize = 100_000;

/// The most proved calls a bridge serves at once.
#[cfg(feature = "files-bridge")]
const CONNECTIONS: usize = 16;

/// The most of them one agent's calls hold, so the rest are left to
/// the others.
#[cfg(feature = "files-bridge")]
const AGENT_CONNECTIONS: usize = 4;

/// The most of them one owner's agents hold together.
#[cfg(feature = "files-bridge")]
const OWNER_CONNECTIONS: usize = 8;

/// The most peers (users at the socket's other end) the bridge keeps
/// room for proving at once ([`crate::bridge::PEERS`], as every bridge):
/// a machine serving it to more users (a box's slots) lets those past it
/// be turned away while the others prove.
pub const PEERS: usize = crate::bridge::PEERS;

/// How long a call's action, once proved, may take to arrive, all of it.
#[cfg(feature = "files-bridge")]
const ACTION_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// How long its answer may wait to be taken.
#[cfg(feature = "files-bridge")]
const WRITE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// What a files bridge takes at once and waits for
/// ([`crate::bridge::serve`]): before the proof, as every bridge, 4
/// connections of one peer's, room kept for [`PEERS`] peers.
#[cfg(feature = "files-bridge")]
pub const LIMITS: crate::bridge::Limits = crate::bridge::Limits {
    name: "the files bridge",
    calls: CONNECTIONS,
    per_agent: AGENT_CONNECTIONS,
    per_owner: OWNER_CONNECTIONS,
    action_max: crate::bridge::CALL_MAX,
    action_wait: ACTION_WAIT,
    answer_wait: WRITE_WAIT,
};

pub mod index;

// Most of what they test is the bridge's server half.
#[cfg(all(test, feature = "files-bridge"))]
mod tests;
