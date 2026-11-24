//! Read authority selected by the host, apart from the contents a
//! service claims to have read. Endpoints never enter execution proof.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadProvider {
    File,
    Memory,
}

/// Identity of a first-party read process whose actual definition was
/// checked by the host. A familiar server name alone is no authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedReadSource {
    pub provider: ReadProvider,
    pub owner: String,
    pub agent: String,
    pub definition: String,
    pub argv_sha256: String,
}

/// Private startup binding, retained in private harness configuration.
/// Execution and portable evidence retain only `source`, never this
/// binding or its endpoint. Matching uses the final endpoint and operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRegistration {
    pub server: String,
    pub operation: String,
    pub endpoint: String,
    pub source: ManagedReadSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReadAuthority {
    /// The harness's own explicit instruction-loading boundary.
    Runtime,
    Managed {
        source: ManagedReadSource,
    },
    /// An unregistered/custom service's statement, never promoted to
    /// a trusted read merely by its path, name, or payload.
    ServiceAsserted {
        server: String,
        operation: String,
    },
}
