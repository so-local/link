//! Host-only session and execution-generation metadata. This is not a
//! native control protocol or a sandbox: callers keep this ledger outside
//! every runtime and authenticate runtime admission before calling it.

use std::{
    ffi::OsStr,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    bridge::{Binding, Keyring, Keys},
    store::Agent,
};
use link_files::nofollow::Dir;

const FILE: &str = "session-authority.json";
const MAX: u64 = 1 << 20;

/// A stable reference to an existing Link conversation, never a native ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub thread: String,
}

/// An adapter's opaque binding reference. Native IDs/graphs stay in the adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBinding {
    pub adapter: String,
    pub id: String,
}

/// Host-minted attempt and generation. These are correlation, never credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub session: Session,
    pub binding: NativeBinding,
    pub generation: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Active {
    attempt: Attempt,
    // A start key is not persisted here, only its fingerprint. Keyring remains
    // the authority for whether that start is still bound.
    start: String,
}

/// The ledger's file, `session-authority.json`, naming its schema.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    agent: Agent,
    sessions: Vec<Session>,
    active: Option<Active>,
}

link_files::specification!(State, include_str!("authority/state.schema.toml"));

/// An explicit local host primitive. No existing adapter/command activates it.
/// The supplied directory and its ancestors must be outside runtime mounts.
/// Serialize native execution separately; a metadata lock cannot stop processes.
pub struct Ledger {
    dir: PathBuf,
    agent: Agent,
    keys: Keyring,
}

impl Ledger {
    pub fn at(dir: &Path, agent: Agent, keys: Keyring) -> anyhow::Result<Self> {
        crate::machine::check_id(&agent.id)?;
        crate::identity::check_owner(&agent.owner)?;
        Ok(Self {
            dir: dir.into(),
            agent,
            keys,
        })
    }

    /// Explicitly associate an existing local thread with a permanent ID.
    /// No history is copied or renamed. Other ingress mappings are refused.
    pub fn session(&self, peer: &Binding, thread: &str) -> anyhow::Result<Session> {
        self.peer(peer)?;
        crate::connection::check_link_thread(thread)?;
        self.change(|state| {
            if let Some(session) = state.sessions.iter().find(|s| s.thread == thread) {
                return Ok(session.clone());
            }
            let session = Session {
                id: link_id::candidate()?,
                agent: self.agent.clone(),
                thread: thread.into(),
            };
            state.sessions.push(session.clone());
            Ok(session)
        })
    }

    /// Commit one active attempt per agent. Retrying a lost acknowledgement
    /// returns the committed attempt; replacing one requires explicit retirement.
    pub fn admit(
        &self,
        peer: &Binding,
        session: &Session,
        binding: NativeBinding,
    ) -> anyhow::Result<Attempt> {
        self.peer(peer)?;
        ensure!(
            !binding.adapter.is_empty()
                && binding.adapter.len() <= 64
                && binding
                    .adapter
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !binding.id.is_empty()
                && binding.id.len() <= 1024
                && !binding.id.contains('\0'),
            "invalid native binding reference"
        );
        self.change(|state| {
            ensure!(
                state.sessions.contains(session),
                "unknown or foreign Link session"
            );
            if let Some(active) = &state.active {
                self.current(state, peer, &active.attempt)?;
                ensure!(
                    active.attempt.session == *session && active.attempt.binding == binding,
                    "an attempt is already active"
                );
                return Ok(active.attempt.clone());
            }
            let attempt = Attempt {
                id: link_id::candidate()?,
                session: session.clone(),
                binding,
                generation: link_id::candidate()?,
            };
            state.active = Some(Active {
                attempt: attempt.clone(),
                start: fingerprint(peer),
            });
            Ok(attempt)
        })
    }

    /// Called only after the host has contained/settled the old execution.
    pub fn retire(&self, peer: &Binding, attempt: &Attempt) -> anyhow::Result<()> {
        self.change(|state| {
            self.current(state, peer, attempt)?;
            invalidate(state);
            Ok(())
        })
    }

    /// Explicit restart barrier after containing old processes, before admission.
    pub fn recover(&self) -> anyhow::Result<()> {
        self.change(|state| {
            invalidate(state);
            Ok(())
        })
    }

    fn peer(&self, peer: &Binding) -> anyhow::Result<()> {
        ensure!(
            peer.identity.agent == self.agent.id && peer.identity.owner == self.agent.owner,
            "another agent or owner's binding"
        );
        ensure!(
            self.keys
                .key_of(&peer.identity)
                .is_some_and(|secret| secret == peer.key),
            "revoked start binding"
        );
        Ok(())
    }

    fn current(&self, state: &State, peer: &Binding, attempt: &Attempt) -> anyhow::Result<()> {
        self.peer(peer)?;
        let active = state.active.as_ref().context("no active execution")?;
        ensure!(
            active.start == fingerprint(peer) && active.attempt == *attempt,
            "stale or foreign execution attempt"
        );
        Ok(())
    }

    fn change<T>(&self, change: impl FnOnce(&mut State) -> anyhow::Result<T>) -> anyhow::Result<T> {
        link_files::create_private_dir(&self.dir)?;
        let dir = Dir::open_unlinked(&self.dir)?;
        let _lock = dir.lock(libc::LOCK_EX)?;
        let mut state = match dir.read(OsStr::new(FILE), MAX) {
            Ok(bytes) => link_files::saved::parse_json::<State>(&bytes)
                .context("reading protected authority metadata")?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => State {
                agent: self.agent.clone(),
                sessions: Vec::new(),
                active: None,
            },
            Err(err) => return Err(err.into()),
        };
        ensure!(
            state.agent == self.agent,
            "incompatible or foreign authority metadata"
        );
        let result = change(&mut state)?;
        let bytes = link_files::saved::print_json(&state)?;
        ensure!(bytes.len() as u64 <= MAX, "authority metadata is full");
        let aside = format!(".authority-{}", link_id::candidate()?);
        let written = (|| -> std::io::Result<()> {
            let mut file = dir.create_new(OsStr::new(&aside), 0o600)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            dir.rename(OsStr::new(&aside), OsStr::new(FILE))?;
            dir.sync_all()
        })();
        if written.is_err() {
            let _ = dir.remove_file(OsStr::new(&aside));
        }
        written?;
        Ok(result)
    }
}

fn fingerprint(peer: &Binding) -> String {
    link_digest::sha256(peer.key.expose().as_bytes())
}

fn invalidate(state: &mut State) {
    state.active = None;
}

#[cfg(test)]
mod tests;
