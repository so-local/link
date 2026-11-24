//! Box's machine contract: what a workload addresses, whichever provider
//! runs the machine. A provider's own types ride in [`Provider`] payloads.

pub use link_digest::ContentId;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

mod common;
pub mod forward;
mod listed;
mod provider;
pub use common::*;
pub use listed::{KIND, Listed};
pub use provider::*;

/// The machine protocol, as `protocol.toml` declares it: what Box and a
/// VM provider say to each other ([`HereCall`], [`HereReply`]), each
/// stating its version. Each side refuses an older one by name.
pub static MACHINE_PROTOCOL: std::sync::LazyLock<link_version::Protocol> =
    std::sync::LazyLock::new(|| link_version::Protocol::shipped(include_str!("protocol.toml")));

/// The question a VM provider answers ([`HereCall`], [`HereReply`]): a
/// module whose manifest answers it is the provider, and Box finds it by
/// this name, never by the provider's.
pub const QUESTION: &str = "box.machine";

/// [`QUESTION`]'s protocol, as `machine.toml` declares it
/// (`link_version::Protocol`): what its asker and answerer each state.
pub const QUESTION_PROTOCOL: &str = include_str!("machine.toml");

/// `lnk box delete`'s question, asked once a box is gone from its cloud
/// ([`BoxIdentity`], [`Released`]): each module that gave the box
/// something to hold (a key to its storage) takes it back, and says what
/// it couldn't.
pub const REMOVED: &str = "box.removed";

/// [`REMOVED`]'s protocol, as `removed.toml` declares it.
pub const REMOVED_PROTOCOL: &str = include_str!("removed.toml");

/// What a module answering [`REMOVED`] couldn't take back from the box,
/// each said in a sentence; empty when nothing stays.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Released {
    pub stays: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxIdentity {
    pub name: String,
    pub machine_id: String,
}
impl BoxIdentity {
    pub fn selector(&self) -> String {
        format!("{}@{}", self.name, self.machine_id)
    }
    pub fn split(selector: &str) -> (&str, Option<&str>) {
        selector
            .rsplit_once('@')
            .map_or((selector, None), |(name, id)| (name, Some(id)))
    }
}
/// Public access for a remote caller whose private identity stays on its machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAccess {
    pub user: String,
    pub host_key: String,
    pub generation: u64,
    pub ticket: String,
}

/// A nonallocating admission check; boot is still distinct from workload readiness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct MachineCheck<P: Provider = Opaque> {
    pub request: P::Request,
    pub plan: P::Plan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputSource {
    Caller,
    Saved,
    HostPolicy,
    ExistingMachine,
}

/// A path is provenance for a person, never an instruction to read it remotely.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: InputSource,
    pub reference: String,
    pub digest: ContentId,
}

/// A VM input, read and validated by the VM provider, never by Box.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct VmInput<P: Provider = Opaque> {
    pub provenance: Provenance,
    pub spec: P::Spec,
}

/// Owner-parsed environment content and its declared files, without secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentInput {
    pub provenance: Provenance,
    pub file: String,
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct MachineInputs<P: Provider = Opaque> {
    pub vm: Option<VmInput<P>>,
    pub environment: Option<EnvironmentInput>,
}
impl<P: Provider> Default for MachineInputs<P> {
    fn default() -> Self {
        Self {
            vm: None,
            environment: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputReference {
    pub path: PathBuf,
    pub source: InputSource,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputReferences {
    pub vm: Option<InputReference>,
    pub environment: Option<InputReference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lifetime {
    Created,
    Borrowed,
    Retained,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachinePolicy {
    pub revision: u64,
    pub vm_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramArtifact {
    /// Trusted local artifact source, copied privately after digest verification.
    pub source: PathBuf,
    pub destination: String,
    pub digest: ContentId,
    pub bytes: u64,
    pub executable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestBootstrap {
    pub target: GuestTarget,
    pub artifacts: Vec<ProgramArtifact>,
    /// An explicitly approved, filtered caller bridge; absent means no outside network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<GuestNetwork>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestNetwork {
    /// Nonempty approved public HTTP/HTTPS host patterns. Private/metadata remain refused.
    pub hosts: Vec<String>,
    #[serde(default)]
    pub tcp: Vec<String>,
}
pub const GUEST_PROXY_PORT: u16 = 3291;
pub const GUEST_PROXY: &str = "http://127.0.0.1:3291";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub vm: bool,
    pub ssh: bool,
    pub link: bool,
    pub sandbox: bool,
    pub environment: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoxState {
    Creating,
    Ready,
    Stopped,
    Stopping,
    CleanupPending,
    Removed,
    Failed,
}

/// Captured creation data survives spec edits, disconnects and retained restarts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct MachineBinding<P: Provider = Opaque> {
    pub name: String,
    pub machine_id: String,
    pub request_id: P::RequestId,
    pub inputs: MachineInputs<P>,
    pub vm_source: InputSource,
    pub policy: MachinePolicy,
    pub request: P::Request,
    pub plan: P::Plan,
    pub handle: Option<P::Handle>,
    pub lease: Lease,
    pub lifetime: Lifetime,
    pub manager_root: PathBuf,
    pub bootstrap: Option<GuestBootstrap>,
    pub readiness: Readiness,
    pub state: BoxState,
    pub intent: Intent,
    pub intent_revision: u64,
    pub created_at: u64,
    pub failure: Option<String>,
    /// The provider's description, on its replies only: never saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<MachineSummary>,
}
impl<P: Provider> MachineBinding<P> {
    /// Removal is complete only when durable intent and observed state
    /// agree and no failure remains recorded.
    pub fn cleanup_complete(&self) -> bool {
        self.state == BoxState::Removed && self.intent == Intent::Removed && self.failure.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct CreateBox<P: Provider = Opaque> {
    pub name: String,
    pub machine_id: String,
    pub inputs: MachineInputs<P>,
    pub lease: Lease,
    pub lifetime: Lifetime,
}

/// A durable creation intent exists before slow resolution or backend discovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct PendingBox<P: Provider = Opaque> {
    pub input: CreateBox<P>,
    pub settings: P::Settings,
    pub cancelled: bool,
    pub cleanup_complete: bool,
    pub failure: Option<String>,
}

/// Local paths name caller-held identity files. Private key bytes never cross a protocol.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshAccess {
    pub name: String,
    pub machine_id: String,
    pub generation: u64,
    pub user: String,
    pub private_identity: PathBuf,
    pub known_hosts: PathBuf,
    pub host_alias: String,
    pub ticket: String,
    /// The `lnk` command words that carry this login's SSH connection,
    /// its ProxyCommand: the provider's own, which Box runs and never
    /// composes.
    pub proxy: Vec<String>,
    /// The guest's approved filtering proxy socket, which `lnk box
    /// network-forward` forwards into it: none without an approved bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct HereCall<P: Provider = Opaque> {
    pub version: u32,
    pub operation: HereOperation<P>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    bound = "",
    tag = "op",
    content = "input",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum HereOperation<P: Provider = Opaque> {
    Bound {
        identity: BoxIdentity,
        operation: Box<HereOperation<P>>,
    },
    Capture(InputReferences),
    Check {
        inputs: MachineInputs<P>,
        workload: bool,
    },
    Reserve(CreateBox<P>),
    Prepare {
        name: String,
        environment: EnvironmentInput,
        materialize: bool,
        start: bool,
        others: bool,
    },
    Policy,
    Create(CreateBox<P>),
    Inspect {
        name: String,
    },
    Lookup {
        name: String,
    },
    List,
    Names,
    Access {
        name: String,
    },
    Authorize {
        name: String,
        public_key: SshPublicKey,
    },
    Open {
        name: String,
        ticket: String,
    },
    Bootstrap {
        name: String,
    },
    EnvironmentReady {
        name: String,
        generation: u64,
        intent_revision: u64,
        digest: ContentId,
    },
    Start {
        name: String,
    },
    Stop {
        name: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    bound = "",
    tag = "kind",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
#[expect(
    clippy::large_enum_variant,
    reason = "Records are bounded by the subprocess message limit"
)]
pub enum HereResponse<P: Provider = Opaque> {
    NotManaged,
    Opened,
    Names(Vec<String>),
    Inputs(MachineInputs<P>),
    Checked(MachineCheck<P>),
    Policy(MachinePolicy),
    Binding(MachineBinding<P>),
    Bindings {
        bindings: Vec<MachineBinding<P>>,
        pending: Vec<PendingBox<P>>,
    },
    Pending(PendingBox<P>),
    Access(SshAccess),
    RemoteAccess(RemoteAccess),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "", deny_unknown_fields)]
pub struct HereReply<P: Provider = Opaque> {
    pub version: u32,
    pub result: Result<HereResponse<P>, String>,
}

impl<P: Provider> HereOperation<P> {
    pub fn name_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Inspect { name }
            | Self::Lookup { name }
            | Self::Access { name }
            | Self::Authorize { name, .. }
            | Self::Bootstrap { name }
            | Self::Start { name }
            | Self::Stop { name }
            | Self::Remove { name }
            | Self::Prepare { name, .. }
            | Self::EnvironmentReady { name, .. }
            | Self::Open { name, .. } => Some(name),
            _ => None,
        }
    }
    pub fn qualified(self) -> Result<Self, String> {
        if let Self::Bound {
            identity,
            mut operation,
        } = self
        {
            let name = operation
                .name_mut()
                .ok_or("identity applies only to a named box operation")?;
            if *name != identity.name {
                return Err("operation names another box".into());
            }
            *name = identity.selector();
            Ok(*operation)
        } else {
            Ok(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider's reply of a version below the oldest is refused by
    /// name, naming the upgrade.
    #[test]
    fn the_machine_protocol_refuses_an_older_peer_by_name() {
        let protocol = &*MACHINE_PROTOCOL;
        assert!(protocol.accept("the VM provider", protocol.current).is_ok());
        let older = protocol
            .accept("the VM provider", protocol.oldest - 1)
            .unwrap_err()
            .to_string();
        assert!(
            older.starts_with("the VM provider speaks the box machine protocol")
                && older.ends_with("lnk upgrade"),
            "{older}"
        );
    }
}

#[cfg(test)]
link_test_kit::program!(system);
