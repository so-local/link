use crate::identity::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    OperationOnce,
    Run {
        id: String,
        generation: u64,
    },
    File {
        target: String,
        expected_revision: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundOperation {
    pub source: SourceId,
    pub schema: u32,
    pub id: OperationId,
    pub payload_digest: String,
    pub review_digest: String,
    pub scope: Scope,
    pub work: Option<WorkRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    ReplyOnly,
    Authorize(Box<BoundOperation>),
}

/// Source-rendered capability facts are distinct from the requester's reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionReview {
    pub capability: String,
    pub targets: Vec<String>,
    pub lifetime: String,
    pub consequences: Vec<String>,
    pub requester_reason: Option<String>,
    pub exact_operation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectState {
    None,
    Awaiting,
    Denied,
    Authorized,
    RequiresClarification,
    Applying { claim: ClaimId },
    Applied { claim: ClaimId, detail: String },
    Failed { claim: ClaimId, reason: String },
    Unknown { claim: ClaimId, reason: String },
    Revoked { reason: String },
}

/// Fresh evidence from the source's serialized execution boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceChecks {
    pub operation: BoundOperation,
    pub authority_current: bool,
    pub generation_current: bool,
    pub control_fence_clear: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectOutcome {
    Applied { detail: String },
    Failed { reason: String },
    Unknown { reason: String },
}
