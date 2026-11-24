pub use link_principal::Principal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Who acts on an interaction: this machine store's own owner, the person
/// its local transports authenticate (`owner`), or a principal Link
/// minted (`user:<id>`, `service:<id>`). The owner is no principal: no
/// identity source vouches for them yet, so Link mints them no ID. Saved
/// and sent by its kind's tag ([`TaggedActor`]); its text, `owner` or the
/// principal's ID, is what a command line names it by.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "TaggedActor", into = "TaggedActor")]
pub enum Actor {
    Owner,
    Principal(Principal),
}

/// [`Actor`] as it's saved and sent: `{ kind = "owner" }`, or `{ kind =
/// "principal", principal = "<kind>:<id>" }`.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum TaggedActor {
    /// The local owner.
    Owner,
    /// A principal Link minted, by its ID.
    Principal { principal: String },
}

impl TryFrom<TaggedActor> for Actor {
    type Error = link_principal::NotOne;
    fn try_from(tagged: TaggedActor) -> Result<Self, Self::Error> {
        Ok(match tagged {
            TaggedActor::Owner => Actor::Owner,
            TaggedActor::Principal { principal } => Actor::Principal(principal.parse()?),
        })
    }
}

impl From<Actor> for TaggedActor {
    fn from(actor: Actor) -> Self {
        match actor {
            Actor::Owner => TaggedActor::Owner,
            Actor::Principal(principal) => TaggedActor::Principal {
                principal: principal.to_string(),
            },
        }
    }
}

/// How the local owner is written.
pub const OWNER: &str = "owner";

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Actor::Owner => f.write_str(OWNER),
            Actor::Principal(principal) => fmt::Display::fmt(principal, f),
        }
    }
}

impl FromStr for Actor {
    type Err = link_principal::NotOne;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            OWNER => Ok(Actor::Owner),
            other => other.parse().map(Actor::Principal),
        }
    }
}

impl From<Principal> for Actor {
    fn from(principal: Principal) -> Self {
        Actor::Principal(principal)
    }
}

impl schemars::JsonSchema for Actor {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Actor".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        TaggedActor::json_schema(generator)
    }
}

macro_rules! ids {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                Self::try_from(value.into())
            }
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, String> {
                if value.is_empty() || value.len() > 256 || !value.bytes().all(|b|
                    b.is_ascii_alphanumeric() || b"._:-/".contains(&b)) {
                    return Err(concat!("invalid ", stringify!($name)).into());
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String { fn from(id: $name) -> String { id.0 } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
        }
    )+};
}
ids!(
    RequestId,
    ItemId,
    ChoiceId,
    DeliveryId,
    InputId,
    ControlId,
    BatchId,
    HoldId,
    OperationId,
    ClaimId,
    TransportEventId,
    SourceId
);

/// A domain of questions, declared in the contract's `types.toml`.
pub use crate::declared::DomainId;

impl DomainId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        Self::try_from(value.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DomainId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        let id = Self(value);
        ::link_actions::value::Validate::validate(&id, "")
            .map_err(|refusal| format!("invalid DomainId: {}", refusal.why))?;
        Ok(id)
    }
}

impl From<DomainId> for String {
    fn from(id: DomainId) -> String {
        id.0
    }
}

/// A host-issued continuation, never a display name or a model-selected task.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkKind {
    Conversation,
    Child {
        id: String,
    },
    Schedule {
        id: crate::ScheduleId,
        revision: u64,
    },
    Occurrence {
        occurrence: crate::ScheduledOccurrence,
    },
    Process {
        id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkRef {
    pub kind: WorkKind,
    pub owner: Actor,
    pub agent: String,
    pub session: String,
    pub task: Option<String>,
    pub attempt: String,
    pub generation: u64,
    pub control_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requester {
    pub source: SourceId,
    pub actor: Actor,
    pub label: String,
    /// Host-selected owner route; it never identifies the continuation.
    pub routing_agent: Option<String>,
    pub work: Option<WorkRef>,
    pub tool_call: Option<String>,
}
