//! Background commands: a command the agent starts that outlives the
//! turn and the tool session that started it, which it can list, read
//! and stop, and hears about when it ends.
//!
//! - **A supervisor per command** (`lnk-run-tool supervise`), started by
//!   the tool server and up only while its command runs. It holds a lock
//!   on the command's record for its whole life (`flock`, which the
//!   kernel releases however it dies), runs the command in the sandbox a
//!   `command` call gets, in a process group of its own, started only once
//!   its record names that group ([`gate`]), writes its
//!   output, stops it at its timeout, and records how it ended. On Linux
//!   the sandbox dies with it.
//! - **Its record**, `<state>/<id>.json`, outside the files folder: no
//!   sandbox writes it, since a sweep kills the process group it names.
//!   Written whole and moved in place.
//! - **Its output**, `background/<id>.out` in the files folder, stdout and
//!   stderr as one stream: written only through descriptors, never
//!   following a link. Past its most, its oldest half is dropped and the
//!   record counts the bytes dropped, so offsets stay the stream's.
//! - **When it ends**, the agent is told through its bridge's way in, in
//!   the conversation that started it, as from `run:<id>`: at most once,
//!   marked before it's handed over, unmarked only when the way in can't
//!   be reached.
//! - **A sweep** ([`sweep`]) finds a command whose supervisor is gone (its
//!   lock is free): marked lost, what's left of its process group killed
//!   when it's surely its own, and told. It also removes what ended and
//!   was told past its keep.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use link_files::nofollow::Dir;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod calls;
mod keeping;
mod output;
mod supervisor;
mod sweeping;
use calls::stop_for;
pub use calls::{Starting, list, start, stop};
pub use keeping::{Record, State, boot, load, now, output_name, records, save, started_at};
use keeping::{kill_left, lock_path, open_lock, record_path, stop_path, take_lock, took};
pub use output::{Cutting, Read, read_output};
use output::{Output, output_size, remove_output};
pub use supervisor::{gate, supervise, supervise_with};
pub use sweeping::{Swept, forget, has_untold, stop_all, sweep, sweep_quietly, tell};

/// The folder of the files folder each command's output is in.
pub const FOLDER: &str = "background";

/// Its output file's ending: one memory doesn't index.
const ENDING: &str = "out";

/// Who a background command's ending is told as: its agent, by its ID,
/// proved with its key from the machine's keyring (`keyring`, else
/// Link's), read when it's told, since the key is made at each start of
/// the agent ([`link_sandbox_binding`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caller {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring: Option<PathBuf>,
}

impl Caller {
    /// Its agent's binding now: an error while it isn't bound.
    pub fn binding(&self) -> anyhow::Result<link_sandbox_binding::Binding> {
        let keyring = match &self.keyring {
            Some(dir) => link_sandbox_binding::Keyring::at(dir),
            None => link_sandbox_binding::Keyring::here(),
        };
        keyring.bound(&self.agent).with_context(|| {
            format!(
                "agent {} isn't bound on this machine (it isn't running): nothing to prove its \
                 message with",
                link_format::clean(&self.agent)
            )
        })
    }
}

/// What background commands run with: the agent's spec's `[tool.run]`
/// and where its records and its bridge's way in are.
#[derive(Clone, Debug)]
pub struct Background {
    /// This program, run as `supervise` and `sweep`.
    pub program: PathBuf,
    /// Where the records are (`~/Link/Agents/<agent>/run`).
    pub state: PathBuf,
    /// The agent's bridge's way in.
    pub way_in: PathBuf,
    /// The agent each ending is told as, proved with its key from the
    /// machine's keyring ([`Caller`]).
    pub caller: Caller,
    /// The most at once.
    pub at_once: u32,
    /// The longest one runs.
    pub timeout: Duration,
    /// What's kept on disk of one's output, in bytes.
    pub output: u64,
    /// How long one is kept once it ended and was told.
    pub keep: Duration,
    /// The most started in a day, each of whose endings starts a turn.
    pub per_day: u32,
    /// The most messages in a chain: a start this far down one is refused.
    pub hops: u32,
}
