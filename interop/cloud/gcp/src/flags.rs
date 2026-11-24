//! `lnk-gcp`'s command line, generated from its declarations
//! (`actions.toml`): its own `connect`, which `lnk cloud connect gcp`
//! passes on, and the cloud contract's protocol it answers on standard
//! input, over the contract crate's shapes.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk-gcp`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

impl Command {
    /// The protocol's word for it, as `lnk-gcp <word>` is called.
    pub fn word(&self) -> &'static str {
        match self {
            Self::GcpAddKey(_) => "add-key",
            Self::GcpBucketCredential(_) => "bucket-credential",
            Self::GcpBuckets(_) => "buckets",
            Self::GcpCleanup(_) => "cleanup",
            Self::GcpConnect(_) => "connect",
            Self::GcpDefaults(_) => "defaults",
            Self::GcpDescribe(_) => "describe",
            Self::GcpDisconnect(_) => "disconnect",
            Self::GcpHostkeys(_) => "hostkeys",
            Self::GcpImageList(_) => "image-list",
            Self::GcpImageRemove(_) => "image-remove",
            Self::GcpImageSave(_) => "image-save",
            Self::GcpLabel(_) => "label",
            Self::GcpLaunch(_) => "launch",
            Self::GcpList(_) => "list",
            Self::GcpObjectRead(_) => "object-read",
            Self::GcpRemove(_) => "remove",
            Self::GcpRemoveKeys(_) => "remove-keys",
            Self::GcpRevokeKey(_) => "revoke-key",
            Self::GcpSaveHostkeys(_) => "save-hostkeys",
            Self::GcpSizes(_) => "sizes",
            Self::GcpStart(_) => "start",
            Self::GcpState(_) => "state",
            Self::GcpStop(_) => "stop",
            Self::GcpStorage(_) => "storage",
            Self::GcpTag(_) => "tag",
            Self::GcpTunnel(_) => "tunnel",
            Self::GcpUploadOnce(_) => "upload-once",
            Self::GcpWait(_) => "wait",
            Self::GcpResourceJournalSchema(_) => "resource-journal-schema",
            Self::Uninstall(_) => "uninstall",
        }
    }
}
