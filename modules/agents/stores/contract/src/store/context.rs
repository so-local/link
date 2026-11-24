//! Small portable views over the existing immutable event log and versioned
//! memory files. Native mappings and context selection belong to adapters.
//! Neither transcript roles nor source metadata confer authority.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Agent, Appended, Store, Written};

/// The exposed part of a transcript. Unexposed reasoning is not context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
    Summary,
}

/// Stable native correlation, scoped to this agent and Link thread.
/// Reusing it with different content is an error, not an edit to the log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub adapter: String,
    pub session: String,
    pub record: String,
}

/// An owned view of common fields. Read it from raw JSON; keep the raw
/// event as the archive rather than serializing this view over it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub role: Role,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

impl Event {
    pub fn of(raw: &Value) -> anyhow::Result<Self> {
        serde_json::from_value(raw.clone()).context("reading a portable event view")
    }
}

/// Evidence in the raw log, never the owner-facing chat projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub thread: String,
    pub seq: u64,
}

/// Reserved versioned memory files: `memory/portable/<id>.json`.
pub const MEMORY: &str = "memory/portable";

/// One memory document. Store versions are opaque CAS tokens, not dates
/// or ordered revision numbers. Unknown metadata survives reads and writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub text: String,
    /// A versioned deletion retains provenance and prevents stale native
    /// exports from making a removed document live again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modified_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Memory {
    pub fn validate(&self) -> anyhow::Result<()> {
        Self::path(&self.id)?;
        ensure!(
            !self.deleted || self.text.is_empty(),
            "a deleted memory must have no text"
        );
        Ok(())
    }

    pub fn path(id: &str) -> anyhow::Result<String> {
        super::check_name(id)?;
        Ok(format!("{MEMORY}/{id}.json"))
    }

    pub fn write(&self, store: &dyn Store, agent: &Agent, version: u64) -> anyhow::Result<Written> {
        self.validate()?;
        store.file_write(
            agent,
            &Self::path(&self.id)?,
            &serde_json::to_vec(self)?,
            version,
        )
    }
}

/// A finite context boundary, kept in the adapter's existing state/home.
/// A version of zero records a missing file and detects its creation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub thread: String,
    pub through_seq: u64,
    #[serde(default)]
    pub memory: BTreeMap<String, u64>,
}

impl Cursor {
    /// Compare for equality, including missing/deleted memories. Call again
    /// immediately before deciding if changes since a read matter to a turn.
    pub fn current(&self, store: &dyn Store, agent: &Agent) -> anyhow::Result<bool> {
        if store.last(agent, &self.thread)? != self.through_seq {
            return Ok(false);
        }
        for (path, version) in &self.memory {
            let now = store
                .file_read(agent, path)?
                .map_or(0, |(_, version)| version);
            if now != *version {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Available input, not a claim that a native model has consumed it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub cursor: Cursor,
    /// Original events, including unknown extensions and tool records.
    pub events: Vec<Value>,
    pub memory: BTreeMap<String, Memory>,
}

/// Read `Last = N`, then the raw log through N and requested memory
/// versions. This is a finite boundary, not a transaction across resources.
/// A concurrent append stays beyond the cursor; `current` detects it.
pub fn read(
    store: &dyn Store,
    agent: &Agent,
    thread: &str,
    memories: &[String],
) -> anyhow::Result<Snapshot> {
    let through_seq = store.last(agent, thread)?;
    let events: Vec<Value> = store
        .read(agent, thread, 1)?
        .into_iter()
        .filter(|event| super::seq_of(event).is_some_and(|seq| seq <= through_seq))
        .collect();
    ensure!(
        events.len() as u64 == through_seq
            && events
                .iter()
                .enumerate()
                .all(|(i, event)| super::seq_of(event) == Some(i as u64 + 1)),
        "the context log changed or has a gap: read it again"
    );
    let mut cursor = Cursor {
        thread: thread.into(),
        through_seq,
        memory: BTreeMap::new(),
    };
    let mut memory = BTreeMap::new();
    for id in memories {
        let path = Memory::path(id)?;
        let found = store.file_read(agent, &path)?;
        cursor.memory.insert(
            path.clone(),
            found.as_ref().map_or(0, |(_, version)| *version),
        );
        if let Some((bytes, _)) = found {
            let doc: Memory =
                serde_json::from_slice(&bytes).with_context(|| format!("reading {path}"))?;
            doc.validate()?;
            ensure!(doc.id == *id, "memory document {path} names another ID");
            memory.insert(id.clone(), doc);
        }
    }
    Ok(Snapshot {
        cursor,
        events,
        memory,
    })
}

/// Capture a native record, preserving all original extensions. Sequence
/// contention is retried, but an uncertain append is returned to the caller.
/// Retrying the same source after a lost acknowledgement finds its original
/// record. Native records need a stable source before their first attempt.
pub fn append(store: &dyn Store, agent: &Agent, thread: &str, raw: &Value) -> anyhow::Result<u64> {
    let mut event = raw.clone();
    let object = event
        .as_object_mut()
        .context("a native event must be an object")?;
    object.insert("seq".into(), 1.into());
    let view = Event::of(&event)?;
    let source = view
        .source
        .context("a new native capture needs source correlation")?;
    super::check_name(&source.adapter)?;
    ensure!(
        !source.session.is_empty() && !source.record.is_empty(),
        "native source IDs must not be empty"
    );
    for _ in 0..16 {
        let last = store.last(agent, thread)?;
        for old in store.read(agent, thread, 1)? {
            let old_source = old
                .get("source")
                .and_then(|value| serde_json::from_value::<Source>(value.clone()).ok());
            if old_source.as_ref() == Some(&source) {
                let seq = super::seq_of(&old).context("a captured event has no sequence")?;
                event["seq"] = seq.into();
                ensure!(
                    old == event,
                    "native source correlation was reused with different content"
                );
                return Ok(seq);
            }
        }
        event["seq"] = last
            .checked_add(1)
            .context("the event sequence is full")?
            .into();
        if let Appended::Written = store.append(agent, thread, &event)? {
            return Ok(last + 1);
        }
    }
    anyhow::bail!("the conversation kept changing: retry this native capture")
}
