//! The seam between Box and the provider that runs its machines.
//!
//! Box's records carry what a provider needs to keep (its request, plan,
//! handle, configuration and input specification) without Box reading
//! them. Each record is generic over a [`Provider`] naming those types:
//! Box uses [`Opaque`], which carries them as plain JSON, and a provider
//! names its own typed ones. A record is the same JSON value either way,
//! so a saved record is read by both, and only the provider validates
//! what is inside.
use crate::GuestTarget;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt::{Debug, Display};

/// What a record can carry: compared, cloned and written as JSON.
pub trait Payload: Clone + Debug + PartialEq + Eq + Serialize + DeserializeOwned {}
impl<T: Clone + Debug + PartialEq + Eq + Serialize + DeserializeOwned> Payload for T {}

/// The types one provider keeps in Box's records.
pub trait Provider: Clone + Debug + PartialEq + Eq {
    /// A caller's input describing the machine, such as a VM spec.
    type Spec: Payload;
    /// The provider's resolved creation request.
    type Request: Payload;
    /// The provider's admitted placement.
    type Plan: Payload;
    /// The provider's identity for one launch of the machine.
    type Handle: Payload + Handle;
    /// The provider's identity for one creation request.
    type RequestId: Payload + Display;
    /// The provider's settings, kept with a pending creation.
    type Settings: Payload;
}

/// What Box reads of a provider's handle.
pub trait Handle {
    /// The machine's identity at its provider, for a person to read.
    fn id(&self) -> String;
    /// The launch generation, advanced by every start.
    fn generation(&self) -> u64;
}

/// Box's view of a provider: every payload carried, none read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opaque;
impl Provider for Opaque {
    type Spec = ProviderData;
    type Request = ProviderData;
    type Plan = ProviderData;
    type Handle = MachineHandle;
    type RequestId = String;
    type Settings = ProviderData;
}

/// A provider's payload, carried as the JSON value the provider wrote:
/// Box may write its object keys in another order, never another value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderData(serde_json::Value);

/// A provider's handle, as Box reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineHandle {
    pub id: String,
    pub generation: u64,
}
impl Handle for MachineHandle {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}

/// What a provider says of its machine for listing and access: never
/// saved, filled on every reply from the provider's own records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineSummary {
    pub target: GuestTarget,
    /// The account a login reaches.
    pub user: String,
    /// The host the machine runs on.
    pub place: String,
    pub vcpus: u32,
}

/// A record read as another provider's view of the same JSON: Box's
/// opaque view as a provider's typed one, refused where it doesn't fit.
pub fn convert<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U, serde_json::Error> {
    serde_json::from_value(serde_json::to_value(value)?)
}
