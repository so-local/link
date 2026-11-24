use crate::{
    approval::{Effect, PermissionReview},
    identity::*,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnswerShape {
    Confirmation,
    Text {
        max_bytes: usize,
        allow_empty: bool,
    },
    Choice {
        options: Vec<Choice>,
        min_selected: usize,
        max_selected: usize,
        allow_other: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: ChoiceId,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub recommended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionItem {
    pub id: ItemId,
    pub prompt: String,
    pub answer_shape: AnswerShape,
    #[serde(default)]
    pub default_answer: Option<ItemAnswer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionDraft {
    pub title: String,
    pub questions: Vec<QuestionItem>,
    pub caller_idempotency_key: String,
    #[serde(default)]
    pub requested_timeout: Option<EffectiveTimeout>,
}

/// Model input has no recipient, authority, continuation, response code or source identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionProposal {
    pub title: String,
    pub questions: Vec<QuestionItem>,
    #[serde(default)]
    pub requested_timeout: Option<EffectiveTimeout>,
}

impl QuestionProposal {
    /// Namespace providers validate nested objects against the offered schema.
    pub fn schema() -> schemars::Schema {
        schemars::generate::SchemaSettings::default()
            .with(|settings| settings.inline_subschemas = true)
            .into_generator()
            .into_root_schema_for::<Self>()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemAnswer {
    Confirmation {
        yes: bool,
    },
    Text {
        text: String,
    },
    Choice {
        selected: Vec<ChoiceId>,
        other_text: Option<String>,
        comment: Option<String>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSubmission {
    #[serde(default)]
    pub answers: BTreeMap<ItemId, ItemAnswer>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResponseValue {
    Submit(SetSubmission),
    Dismiss { comment: Option<String> },
}

/// What a question left unanswered becomes, declared in the contract's
/// `types.toml`.
pub use crate::declared::Fallback;

/// The schema a model is offered for a fallback (`QuestionProposal`),
/// each choice described.
#[derive(JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "Fallback")]
#[allow(dead_code, reason = "only its schema is read")]
enum FallbackSchema {
    /// Nothing: it expires unanswered.
    Expire,
    /// Its default answer.
    UseDefault,
    /// Its recommended answer.
    UseRecommended,
}

impl JsonSchema for Fallback {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        FallbackSchema::schema_name()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        FallbackSchema::json_schema(generator)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectiveTimeout {
    pub after_secs: u64,
    #[serde(default)]
    pub item_fallbacks: BTreeMap<ItemId, Fallback>,
    pub policy_revision: String,
}

/// How questions wait and fall back, by domain, declared in the
/// contract's `types.toml`.
pub use crate::declared::{DomainOpinion, InteractionOpinions};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionSet {
    pub version: u32,
    pub id: RequestId,
    pub revision: u64,
    pub domain: DomainId,
    pub requester: Requester,
    pub recipient: Actor,
    pub title: String,
    pub questions: Vec<QuestionItem>,
    pub effect: Effect,
    pub review: Option<PermissionReview>,
    pub timeout: EffectiveTimeout,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseCandidate {
    pub request: RequestId,
    pub revision: u64,
    pub delivery: DeliveryId,
    pub value: ResponseValue,
    pub transport_event: TransportEventId,
}

/// Produced by authenticated ingress, never by model tool arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ingress {
    pub actor: Actor,
    pub route: String,
    pub surface: String,
    pub input_kind: InputKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Typed,
    VoiceTranscription,
    Control,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Provenance {
    User { actor: Actor, accepted_at: u64 },
    TimeoutDefault { policy_revision: String },
    TimeoutRecommended { policy_revision: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextEvidence {
    pub text: String,
    pub supplied_by: Actor,
    pub input_kind: InputKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionEvidence {
    pub choice: Choice,
    pub supplied_by: Actor,
    pub selected_by: Option<Actor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemResult {
    Answered {
        answer: ItemAnswer,
        provenance: Provenance,
        selections: Vec<SelectionEvidence>,
        text: Vec<TextEvidence>,
    },
    Skipped {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    UserSubmission,
    Timeout,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub request: RequestId,
    pub requester: Requester,
    pub domain: DomainId,
    pub trigger: Trigger,
    pub items: BTreeMap<ItemId, ItemResult>,
    pub comment: Option<TextEvidence>,
    pub closed_reason: Option<String>,
}
