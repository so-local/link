use crate::{identity::*, question::TextEvidence};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Admission {
    Queue,
    Steer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInput {
    pub id: InputId,
    pub actor: Actor,
    pub target: WorkRef,
    pub content: TextEvidence,
    pub admission: Admission,
    pub transport_event: TransportEventId,
    pub submitted_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseBoundary {
    FinishModelResponse,
    InterruptCurrentStep,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteScope {
    Conversation,
    TaskRecord,
    Artifacts { ids: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationAction {
    SpawnChild,
    StartChild,
    DispatchChildMessage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldTerms {
    pub children: Option<BTreeSet<String>>,
    pub blocked_actions: BTreeSet<DelegationAction>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hold {
    pub id: HoldId,
    pub imposed_by: Actor,
    pub coordinator: WorkRef,
    pub terms: HoldTerms,
    pub released: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlOperation {
    Stop,
    Pause { boundary: PauseBoundary },
    Resume { reasons: BTreeSet<ControlId> },
    StartPreparedWork,
    Archive,
    Unarchive,
    Delete { scope: DeleteScope },
    AddHold { id: HoldId, terms: HoldTerms },
    ReleaseHold { hold: HoldId },
    Redirect { input: InputId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeScope {
    SelfOnly,
    Descendants,
    Subtree,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlSelector {
    Exact { work: WorkRef },
    Selected { members: Vec<WorkRef> },
    Tree { root: WorkRef, scope: TreeScope },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkControl {
    pub id: ControlId,
    pub actor: Actor,
    pub selector: ControlSelector,
    pub operation: ControlOperation,
    pub transport_event: TransportEventId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddressedControl {
    pub id: ControlId,
    pub batch: BatchId,
    pub work: WorkRef,
    pub operation: ControlOperation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedControlBatch {
    pub id: BatchId,
    pub control: WorkControl,
    pub lineage_revision: u64,
    pub members: Vec<AddressedControl>,
}

/// Authoritative host snapshot. The interaction engine does not own task state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineageSnapshot {
    pub revision: u64,
    pub members: Vec<LineageMember>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineageMember {
    pub work: WorkRef,
    pub parent_attempt: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Open,
    Archiving,
    Archived,
    Deleting,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Execution {
    Prepared,
    Running,
    Pausing,
    Paused,
    Stopping,
    Stopped,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkState {
    pub work: WorkRef,
    pub lifecycle: Lifecycle,
    pub pause_reasons: BTreeSet<ControlId>,
    pub observed: Execution,
    pub checkpoint: Option<String>,
    pub in_flight_effects: BTreeSet<OperationId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStage {
    Accepted,
    Queued,
    Delivered,
    Included,
    StopRequested,
    Stopped,
    RedirectApplied,
    HoldRequested,
    HoldActive,
    Pausing,
    Paused,
    Applied,
    Stale,
    Denied,
    Unsupported,
    Partial,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub sequence: u64,
    pub correlation: String,
    pub target: WorkRef,
    pub stage: ReceiptStage,
    pub evidence: String,
}
