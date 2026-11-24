//! The stable kinds of credentials described by Link's inventory. Labels keep
//! their existing JSON spelling; identifiers and owning references do not depend
//! on how a credential viewer phrases its other columns.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! credentials {
    ($($kind:ident => ($id:literal, $label:literal, $page:literal)),+ $(,)?) => {
        /// A credential category with one stable identifier and owning explanation.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum CredentialKind { $($kind),+ }

        impl CredentialKind {
            /// Every known kind, from the same declaration used by runtime values.
            pub const ALL: &'static [Self] = &[$(Self::$kind),+];

            pub const fn id(self) -> &'static str {
                match self { $(Self::$kind => $id),+ }
            }

            /// The established display and JSON label. Renaming it changes the wire.
            pub const fn label(self) -> &'static str {
                match self { $(Self::$kind => $label),+ }
            }

            /// The authoritative public documentation, relative to the workspace.
            pub const fn documentation(self) -> &'static str {
                match self { $(Self::$kind => concat!("modules/security/docs/config/credentials/", $page)),+ }
            }
        }
    };
}

credentials! {
    InstallationKey => ("installation-key", "This installation's key", "installation.md#the-installation-key"),
    InstallationSealingKey => ("installation-sealing-key", "This installation's sealing key", "installation.md#the-sealing-key"),
    InstallationAssertion => ("installation-assertion", "An installation assertion", "installation.md#the-installation-assertion"),
    DeviceCode => ("device-code", "A device code", "installation.md#the-device-code"),
    AccessToken => ("access-token", "An access token", "access-token.md"),
    VisitorPass => ("visitor-pass", "A visitor's pass", "relay.md#a-visitors-pass"),
    OperatorToken => ("operator-token", "An operator token", "relay.md#an-operator-token"),
    TunnelPassword => ("tunnel-password", "A tunnel or share-link password", "relay.md#passwords-for-tunnels-and-share-links"),
    ShareLink => ("share-link", "A share link", "relay.md#a-share-link"),
    AgentKey => ("agent-key", "An agent's key", "agent.md#an-agents-key"),
    MemoryEmbeddingProof => ("memory-embedding-proof", "A memory embedding proof", "agent.md#a-memory-embedding-proof"),
    SandboxProxySecret => ("sandbox-proxy-secret", "A sandbox proxy's secret", "agent.md#the-sandbox-proxys-secret"),
    HarnessToken => ("harness-token", "A harness's token", "held.md#a-harnesss-token"),
    ModelKey => ("model-key", "A model's key", "held.md#a-models-key"),
    ConnectionToken => ("connection-token", "A connection's token", "held.md#a-connections-token"),
    ToolKey => ("tool-key", "A tool's key", "held.md#a-tools-key-and-a-remote-tools-headers"),
    RemoteToolHeaders => ("remote-tool-headers", "A remote tool's headers", "held.md#a-tools-key-and-a-remote-tools-headers"),
    ExportHeader => ("export-header", "An export's header", "held.md#an-exports-headers"),
    CloudAccountKey => ("cloud-account-key", "A cloud account's key", "held.md#a-cloud-accounts-keys"),
    BucketKeys => ("bucket-keys", "A bucket's keys", "held.md#a-buckets-keys-and-encryption-password"),
    BucketEncryptionPassword => ("bucket-encryption-password", "A bucket's encryption password", "held.md#a-buckets-keys-and-encryption-password"),
    StorageRefreshToken => ("storage-refresh-token", "A storage token that refreshes itself", "held.md#a-storage-token-that-refreshes-itself"),
    Secret => ("kept-secret", "A secret you keep", "held.md#a-secret-you-keep"),
    UnreadableSecrets => ("unreadable-secrets", "Secrets you keep, unreadable", "held.md#when-saved-secrets-cannot-be-read"),
    BoxSshKey => ("box-ssh-key", "Link's SSH key for boxes", "held.md#links-ssh-key-for-boxes"),
    BoxSshSecret => ("box-ssh-secret", "The box SSH secret", "held.md#the-box-ssh-secret"),
    WebhookSecret => ("webhook-secret", "A webhook's secret", "webhook.md"),
    ServiceSigningKey => ("service-signing-key", "A service's signer", "README.md#each-services-own"),
}

/// A known category, or an opaque label received from an outside module. Source code constructs one from `CredentialKind`; arbitrary strings
/// enter only through the existing wire format during deserialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credential(Label);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Label {
    Known(CredentialKind),
    Outside(String),
}

impl Credential {
    pub fn kind(&self) -> Option<CredentialKind> {
        match self.0 {
            Label::Known(kind) => Some(kind),
            Label::Outside(_) => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match &self.0 {
            Label::Known(kind) => kind.label(),
            Label::Outside(label) => label,
        }
    }
}

impl From<CredentialKind> for Credential {
    fn from(kind: CredentialKind) -> Self {
        Self(Label::Known(kind))
    }
}

impl std::ops::Deref for Credential {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Credential {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for Credential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for Credential {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Credential {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let label = String::deserialize(deserializer)?;
        Ok(
            match CredentialKind::ALL
                .iter()
                .find(|kind| kind.label() == label)
            {
                Some(kind) => (*kind).into(),
                None => Self(Label::Outside(label)),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_one_stable_id_and_wire_label() {
        let mut ids = std::collections::BTreeSet::new();
        let mut labels = std::collections::BTreeSet::new();
        for kind in CredentialKind::ALL {
            assert!(ids.insert(kind.id()), "duplicate id {}", kind.id());
            assert!(
                labels.insert(kind.label()),
                "duplicate label {}",
                kind.label()
            );
            assert!(!kind.id().is_empty());
            assert!(
                kind.id()
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            );
            let value: Credential = (*kind).into();
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(
                serde_json::from_str::<Credential>(&json).unwrap().kind(),
                Some(*kind)
            );
            assert_eq!(json, serde_json::to_string(kind.label()).unwrap());
        }
    }

    #[test]
    fn existing_module_json_keeps_its_shape_and_unknown_labels_round_trip() {
        let old = r#"{"what":"An agent's key","of":"work","kept":"keyring","may":"Proves calls"}"#;
        let held: super::super::Held = serde_json::from_str(old).unwrap();
        assert_eq!(held.what.kind(), Some(CredentialKind::AgentKey));
        assert_eq!(
            serde_json::to_value(&held).unwrap(),
            serde_json::from_str::<serde_json::Value>(old).unwrap()
        );
        let outside = r#"{"what":"Outside module's credential","kept":"its own store","may":"Calls its service"}"#;
        let held: super::super::Held = serde_json::from_str(outside).unwrap();
        assert_eq!(held.what.kind(), None);
        assert_eq!(held.what.as_str(), "Outside module's credential");
        assert_eq!(
            serde_json::to_value(&held).unwrap(),
            serde_json::from_str::<serde_json::Value>(outside).unwrap()
        );
    }
}
