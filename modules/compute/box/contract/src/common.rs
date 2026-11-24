//! What every provider's machines share: their lease, the lifecycle
//! asked of them, the operating system they run and the public half of
//! an SSH login. A provider's own contract uses these, never a copy.
use serde::{Deserialize, Serialize};

/// How long a machine may live: until a deadline, renewable to a
/// ceiling, or kept until its principal expires. Times are Unix
/// milliseconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Lease {
    Finite {
        expires_at: u64,
        renewable_until: u64,
    },
    Retained {
        principal_expires_at: Option<u64>,
    },
}
impl Lease {
    pub fn deadline(&self) -> Option<u64> {
        match self {
            Self::Finite { expires_at, .. } => Some(*expires_at),
            Self::Retained {
                principal_expires_at,
            } => *principal_expires_at,
        }
    }
}
/// The lifecycle asked of a machine, which its observed state follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Intent {
    Running,
    Stopped,
    Removed,
}
/// The operating system a machine runs: its kind, processor and release.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct GuestTarget {
    /// The guest's OS, its kernel and userspace: `linux` or `macos`.
    /// Whether one can run here is what the backends' probes say.
    pub os: GuestOs,
    /// The guest's architecture: `x86_64` or `aarch64`. Another than the
    /// host's needs emulation allowed and a backend that probes support
    /// for that exact pair.
    pub arch: GuestArch,
    /// The exact release identifier, at most 128 characters with no
    /// control characters, which the image's manifest and the backend's
    /// descriptor must both name.
    #[schemars(example = &"debian-13")]
    pub release: String,
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum GuestOs {
    Linux,
    Macos,
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum GuestArch {
    #[serde(rename = "x86_64")]
    X86_64,
    Aarch64,
}

/// Public material only. The caller keeps the corresponding private key locally.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SshPublicKey {
    Ed25519 { bytes: [u8; 32] },
}
