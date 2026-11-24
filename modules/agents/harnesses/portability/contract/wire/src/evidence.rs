//! Persisted execution observations, shared by a harness and its consumers.
//!
//! These are observations of the runtime, never claims by a model. An
//! absent record proves nothing. A completed request is not a successful
//! task: its reply and the task's postconditions still need checking.

use std::collections::BTreeMap;

use link_interactions_contract::WorkRef;
use link_model_contract::chat::media::{Call, Part};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod export;
pub mod index;
pub mod request;
mod source;
pub use source::{ManagedReadSource, ReadAuthority, ReadProvider, ReadRegistration};
mod skill;
mod validation;
pub use skill::{SkillAnchor, SkillLoad, SkillRoute, SkillSource};

/// The shape of an evidence record. Unknown versions are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub enum Version {
    V1,
}

impl TryFrom<u32> for Version {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::V1),
            _ => Err(format!("unsupported execution evidence version {value}")),
        }
    }
}

impl From<Version> for u32 {
    fn from(_: Version) -> Self {
        1
    }
}

/// A record attached to its owning conversation event. The store's
/// successful append assigns that event's sequence, never its caller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub version: Version,
    pub record: Record,
}

impl Evidence {
    pub fn new(record: Record) -> Self {
        Self {
            version: Version::V1,
            record,
        }
    }
}

/// One call within a successfully appended proposal, scoped to its agent and
/// conversation. Provider IDs are not execution identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRef {
    pub proposal: u64,
    pub ordinal: u32,
}

impl CallRef {
    /// A native portable tool-call ID, distinct from the provider ID.
    pub fn portable_id(self) -> String {
        format!("execution-{}-{}", self.proposal, self.ordinal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Turn,
    Summary,
}

/// A proposed call, before any dispatch, with the provider's original
/// ID kept apart from the normalized ID sent back in model history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedCall {
    pub provider_id: String,
    pub call: Call,
}

/// A batch's monotonic clock starts at its proposal. Its wall clock is
/// an annotation; compare offsets only within this proposal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub phase: Phase,
    pub attempt: String,
    pub work: Option<WorkRef>,
    pub started_unix_ms: u64,
    pub calls: Vec<ProposedCall>,
}

/// Actual arguments at the harness-to-MCP boundary, after local
/// normalization. Transport metadata and endpoint URLs are excluded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpDispatch {
    pub server: String,
    pub operation: String,
    pub arguments: Captured<Value>,
}

/// The marker is written before a request may leave the process. It
/// proves possible dispatch, not remote execution or an effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Record {
    ModelRequested {
        request: request::ModelRequest,
    },
    Proposed(Proposal),
    Dispatched {
        call: CallRef,
        offset_ns: u64,
        request: McpDispatch,
    },
    Finished {
        call: CallRef,
        /// Unknown after recovery in another process, whose monotonic
        /// clock cannot reconstruct the original request's duration.
        offset_ns: Option<u64>,
        outcome: Outcome,
    },
    SkillLoaded {
        load: Box<SkillLoad>,
    },
}

/// What is known of a request. A matching JSON-RPC error is a known
/// response, distinct from a local refusal and transport uncertainty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Outcome {
    Completed { reply: Reply },
    RemoteError { error: RemoteError },
    Refused { reason: String },
    Unknown { reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteError {
    pub code: i64,
    pub message: Captured<String>,
    pub data: Option<Captured<Value>>,
}

/// A field is either retained whole or explicitly omitted. A digest
/// names omitted bytes; it does not establish their contents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Captured<T> {
    Retained {
        value: T,
    },
    Omitted {
        bytes: u64,
        sha256: String,
        reason: String,
    },
}

impl<T> Captured<T> {
    pub fn retained(&self) -> Option<&T> {
        match self {
            Self::Retained { value } => Some(value),
            Self::Omitted { .. } => None,
        }
    }
}

/// One original MCP content block. Binary contents are represented by
/// the existing stored media part, with the JSON omission explicit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub index: u32,
    pub value: Captured<Value>,
    pub media: Option<Part>,
    pub omission: Option<String>,
}

/// The exact model-facing view after conversion and storage. Its text
/// digest is not the digest of the original tool result or skill file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelView {
    pub text_sha256: String,
    pub text_bytes: u64,
    pub original_text_bytes: u64,
    pub cut_text_bytes: u64,
    pub media: Vec<Part>,
    pub omissions: Vec<String>,
}

/// A matching MCP result, preserving its structured fields even when
/// text is also present. `fields` covers every top-level field except
/// `content`, whose blocks are indexed separately. Metadata omissions,
/// byte limits, and media failures stay explicit. This is a captured
/// reply with declared omissions, never a claim of complete raw JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub is_error: bool,
    pub body: Captured<ReplyBody>,
    pub model: ModelView,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyBody {
    pub fields: BTreeMap<String, Captured<Value>>,
    pub content: Vec<Content>,
}

impl Reply {
    pub fn field(&self, name: &str) -> Option<&Captured<Value>> {
        self.body.retained()?.fields.get(name)
    }

    pub fn content(&self) -> Option<&[Content]> {
        Some(&self.body.retained()?.content)
    }
}

#[cfg(test)]
mod tests;
