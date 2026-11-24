//! Bind each credential kind to its owning implementation. The exhaustive match
//! makes a new kind require a binding without coupling core to any adapter.

use link_core::uninstall::CredentialKind;

/// The source file in this workspace and production symbol that holds,
/// produces or checks this kind: none for a kind whose producer this
/// workspace doesn't hold. Generic Secret values keep their purpose at that
/// owning boundary.
pub(super) fn source(kind: CredentialKind) -> Option<(&'static str, &'static str)> {
    match kind {
        CredentialKind::InstallationSealingKey => {
            Some(("packages/secretive/src/sealed.rs", "Recipient"))
        }
        CredentialKind::InstallationAssertion => Some(("packages/api/src/jwt.rs", "assertion")),
        CredentialKind::AccessToken => Some(("packages/api/src/jwt.rs", "access_token")),
        CredentialKind::AgentKey => Some(("modules/agents/agent/src/settings/loading.rs", "held")),
        CredentialKind::MemoryEmbeddingProof => Some((
            "modules/agents/tools/memory-tool/contract/src/memory/embedding.rs",
            "Authentication",
        )),
        CredentialKind::SandboxProxySecret => {
            Some(("modules/security/sandbox/proxy/src/filter.rs", "Filter"))
        }
        CredentialKind::HarnessToken => Some((
            "interop/agents/harnesses/openclaw/src/installing.rs",
            "held",
        )),
        CredentialKind::ModelKey => Some(("modules/agents/agent/src/settings/loading.rs", "save")),
        CredentialKind::ConnectionToken => {
            Some(("modules/agents/agent/src/connections.rs", "Connection"))
        }
        CredentialKind::ToolKey => Some(("modules/security/sandbox/src/commands/tool.rs", "tool")),
        CredentialKind::RemoteToolHeaders => {
            Some(("modules/security/sandbox/src/commands/tool.rs", "tool"))
        }
        CredentialKind::ExportHeader => Some(("modules/tooling/measure/src/main.rs", "main")),
        CredentialKind::CloudAccountKey => Some(("modules/cloud/cloud/src/lib.rs", "held")),
        CredentialKind::BucketKeys => {
            Some(("modules/storage/storage/src/rclone/config.rs", "held"))
        }
        CredentialKind::BucketEncryptionPassword => {
            Some(("modules/storage/storage/src/rclone/config.rs", "held"))
        }
        CredentialKind::StorageRefreshToken => Some((
            "modules/agents/stores/contract/src/authority.rs",
            "StoreSecret",
        )),
        CredentialKind::Secret => Some((
            "modules/security/secret/contract/src/secrets/removing.rs",
            "held_in",
        )),
        CredentialKind::UnreadableSecrets => Some((
            "modules/security/secret/contract/src/secrets/removing.rs",
            "held",
        )),
        CredentialKind::BoxSshKey => Some(("modules/compute/box/src/ssh/hosts.rs", "key")),
        CredentialKind::BoxSshSecret => Some(("modules/compute/box/src/ssh/certs.rs", "kept")),
        CredentialKind::WebhookSecret => Some(("packages/secretive/src/inbound.rs", "Inbound")),
        CredentialKind::ServiceSigningKey => Some(("packages/api/src/jwt.rs", "sign")),
        CredentialKind::InstallationKey
        | CredentialKind::DeviceCode
        | CredentialKind::VisitorPass
        | CredentialKind::OperatorToken
        | CredentialKind::TunnelPassword
        | CredentialKind::ShareLink => None,
    }
}
