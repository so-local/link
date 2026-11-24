//! Where an agent's conversations are kept, when its spec names a store
//! driver (`[storage] conversations = "<name>"`): the contract between a
//! harness that reads the key (Link Harness) and the driver, and how a
//! conversation kept by one is read by anyone else (memory, `lnk agent
//! conversation`).
//!
//! A driver is a program, `lnk-<name>`, answering:
//!
//! ```text
//! lnk-<name> contract     prints [`Contract`] as JSON
//! lnk-<name> call         reads one [`Request`] as JSON on stdin, and prints one [`Reply`]
//! lnk-<name> serve        reads [`Request`]s, one a line, and prints each's [`Reply`] as a line,
//!                         until stdin ends: only when its contract says `serve`
//! ```
//!
//! A driver whose contract says `serve` is kept running by the store
//! bridge ([`serve_bridge`]), a few at a time, so a call costs neither a
//! process nor a fresh connection to its store ([`run`]), and by a
//! caller that runs it itself ([`Driver::call`]) the same way; any
//! other is run once a call.
//!
//! A harness in its sandbox calls it through the store bridge
//! ([`Bridged`], [`serve_bridge`]), outside the sandbox, which runs the
//! driver and holds whatever it needs to reach its store: every call is
//! the agent's identity, the request and its proof ([`crate::bridge`]),
//! and only the proven agent's conversations are answered. Without a
//! bridge, it runs where its caller runs, in its caller's sandbox, which
//! reads the driver's program: memory's, and a harness's started with
//! none. [`DIR_ENV`] names
//! the folder a driver on this machine keeps what it keeps in, the
//! agent's `chats/` in its own folder (`agent/chats`,
//! the one folder of it Link Harness writes), so it moves with the
//! agent. A driver off the machine
//! keeps nothing there.
//!
//! A driver keeps two things of a conversation. Its events, each a JSON
//! object whose `seq` is its number, from 1, one after another: nothing
//! else of an event is the driver's to read. And its lease, which says a
//! turn runs on it. What it promises:
//!
//! - **An event is written only if its number is free**, and the next:
//!   of two writers of one number, on any machines, one writes it and the
//!   other is told [`Reply::Taken`]. An event written is never changed.
//! - **Read from a number:** the events numbered `first` and on, in order.
//! - **The lease:** taken by one holder at a time, renewed by it, ended by
//!   it; one not renewed for `lapse` (its turn crashed) is taken over by
//!   one taker only.
//!
//! [`check`] runs a driver through these. What a person reads of a
//! conversation, from its events, is [`copy`]; the `compact` way of
//! keeping one, the same events compressed, is [`compact`].
//!
//! A driver whose contract says `jobs` keeps an agent's scheduled jobs
//! too, for a scheduler whose agent runs only while it answers, so they
//! outlive it on the machine and any machine it's bound to next reads
//! them ([`Request::JobPut`] and the rest). Each job is an ID, a spec
//! (JSON, nothing of it the driver's to read) and when it's due, in a
//! [`Place`]; and the agent's day's count and a mark that moves with its
//! waiting jobs. What it promises:
//!
//! - **A job is in one place at a time:** a shift moves it whole, and of
//!   two shifts of one job from one place only one is made, on any
//!   machines; the other is told it wasn't there.
//! - **What's due:** the soonest due of the jobs waiting, never of one
//!   being fired.
//!
//! [`check_jobs`] runs a driver through these; a driver that doesn't say
//! `jobs` is never asked them by the store bridge.

use link_secretive::Duplicate;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use link_format::{Done, Exit, Out, Report, WithinExt, sentence};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod bridged;
mod checking;
#[cfg(feature = "compact")]
pub mod compact;
pub mod context;
pub mod copy;
mod driver;
pub mod file;
pub mod folder;
#[cfg(feature = "files-client")]
pub mod memory_files;
#[cfg(feature = "files-client")]
pub mod objects;
mod protocol;
mod serving;
mod storing;
pub mod under;
pub use bridged::{
    ANSWER_MAX, BRIDGE_AGENT_CONNECTIONS, BRIDGE_CONNECTIONS, BRIDGE_DRIVERS, BRIDGE_LIMITS,
    BRIDGE_READ_WAIT, BRIDGE_WAIT, BridgeAt, Bridged, DriverOf, PlacedDriver, serve_bridge,
    serve_storage,
};
pub use checking::{check, check_files, check_jobs, check_narrowing};
pub use driver::{Driver, module_for};
pub use protocol::{
    AGENT_ENV, Agent, Appended, CALL_TIMEOUT, Contract, DIR_ENV, EVENT_MAX, FILE_COLLECTIONS,
    FILE_MAX, FILES_MAX, JOB_ID_MAX, JOB_MAX, Job, Kept, Lease, MEMORY_FILE_MAX, Place, Reply,
    Request, Stored, Took, VERSION, Written, check_file_path, check_file_size, check_job,
    check_job_id, check_name, check_tools, file_data, file_max, seq_of,
};
use serving::Running;
pub use serving::{BRIDGE_KEPT, BRIDGE_KEPT_IDLE, Operation, run};
pub use storing::Store;

/// Now, in milliseconds since the epoch: when a lease was renewed.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Whether a lease renewed at `renewed_ms` lapsed by `now_ms`, not
/// renewed for `lapse_ms`.
pub(crate) fn lapsed(renewed_ms: u64, now_ms: u64, lapse_ms: u64) -> bool {
    now_ms.saturating_sub(renewed_ms) >= lapse_ms
}

/// `lapse` in milliseconds, at most `u64::MAX`.
fn millis(lapse: Duration) -> u64 {
    u64::try_from(lapse.as_millis()).unwrap_or(u64::MAX)
}

fn unexpected<T>(name: &str, reply: &Reply) -> anyhow::Result<T> {
    bail!(
        "lnk-{name} answered something else: {}",
        link_format::clean(&serde_json::to_string(reply).unwrap_or_default())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lapse judged to the millisecond: a lease renewed a moment before
    /// a second turns isn't lapsed a moment after.
    #[test]
    fn a_lease_lapses_by_its_milliseconds_never_early() {
        assert!(!lapsed(100_999, 101_004, 1000));
        assert!(lapsed(100_999, 101_999, 1000));
    }

    pub(super) fn lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}
