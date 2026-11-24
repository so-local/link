//! `lnk-aws`'s command line, generated from its declarations
//! (`actions.toml`): its own `connect`, which `lnk cloud connect aws`
//! passes on, and the cloud contract's protocol it answers on standard
//! input, over the contract crate's shapes.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk-aws`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

impl Command {
    /// The protocol's word for it, as `lnk-aws <word>` is called.
    pub fn word(&self) -> &'static str {
        match self {
            Self::AwsAddKey(_) => "add-key",
            Self::AwsBucketCredential(_) => "bucket-credential",
            Self::AwsBuckets(_) => "buckets",
            Self::AwsCleanup(_) => "cleanup",
            Self::AwsConnect(_) => "connect",
            Self::AwsDefaults(_) => "defaults",
            Self::AwsDescribe(_) => "describe",
            Self::AwsDisconnect(_) => "disconnect",
            Self::AwsHostkeys(_) => "hostkeys",
            Self::AwsImageList(_) => "image-list",
            Self::AwsImageRemove(_) => "image-remove",
            Self::AwsImageSave(_) => "image-save",
            Self::AwsLabel(_) => "label",
            Self::AwsLaunch(_) => "launch",
            Self::AwsList(_) => "list",
            Self::AwsObjectRead(_) => "object-read",
            Self::AwsRemove(_) => "remove",
            Self::AwsRemoveKeys(_) => "remove-keys",
            Self::AwsRevokeKey(_) => "revoke-key",
            Self::AwsSaveHostkeys(_) => "save-hostkeys",
            Self::AwsSizes(_) => "sizes",
            Self::AwsStart(_) => "start",
            Self::AwsState(_) => "state",
            Self::AwsStop(_) => "stop",
            Self::AwsStorage(_) => "storage",
            Self::AwsTag(_) => "tag",
            Self::AwsTunnel(_) => "tunnel",
            Self::AwsUploadOnce(_) => "upload-once",
            Self::AwsWait(_) => "wait",
            Self::Uninstall(_) => "uninstall",
        }
    }
}
