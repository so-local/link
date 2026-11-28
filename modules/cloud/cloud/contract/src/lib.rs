//! The contract between the modules that use clouds and a cloud adapter
//! (`lnk-aws`, `lnk-gcp`, ...). Its callers: `lnk-cloud` (connecting,
//! and whether an account hosts agents), `lnk-box` (boxes and images),
//! `lnk-storage` (buckets, and an owned folder a cloud account keeps) and `lnk-agent`
//! (moving an agent to an account that hosts it, and listing those it
//! hosts). A module with commands of its own for an adapter to answer
//! asks them through [`ask`], with its own request and reply.
//!
//! An adapter is a program answering these commands:
//!
//! ```text
//! lnk-<kind> connect [--name N] [--region R] [--profile P] [--key-id K] [--project P] [--zone Z] [--credential-file F] [--machine]
//!                     signs in to an account (the cloud's own login, reused,
//!                     else asking on the terminal, questions on stderr), checks
//!                     it, and prints the [`Account`] as JSON on stdout. Its
//!                     `settings` carries selectors, `env` guarded credentials: the cloud's CLI,
//!                     `storage` and every module act as it, and nothing else
//! lnk-<kind> storage  {account} -> {"credential", "env", "writes"?, "message"?}: the
//!                     account's own sign-in for its buckets, as rclone's
//!                     settings (`type` included, never saved) and the
//!                     variables rclone runs with; `writes: false` when it
//!                     reads only (an account past what it may keep), with
//!                     the cloud's `message` saying why and how to make room
//! lnk-<kind> disconnect   {account} -> {}: what the adapter kept for the account
//!                     (a credential file), gone; `lnk cloud disconnect`
//! lnk-<kind> defaults {account} -> {"place", "size"}: where and how big a box is
//!                     by default
//! lnk-<kind> sizes    {account, place} -> {"sizes"}: the machine types a box
//!                     can be there ([`MachineSize`]), in the adapter's order
//!                     of preference; `lnk box` picks among them
//! lnk-<kind> launch   {account, place, spec} -> {"id"}: starts making a box
//! lnk-<kind> wait     {account, place, id} -> {"ip"}: once it runs
//! lnk-<kind> start    {account, place, id} -> {"ip"}: a stopped box, running again
//! lnk-<kind> stop     {account, place, id} -> {}
//! lnk-<kind> state    {account, place, id} -> {"state", "stopped_by_cloud"?}:
//!                     running, stopped, gone, ...; whether its cloud stopped
//!                     it, taking a spot machine back, when the adapter can tell
//! lnk-<kind> describe {account, place, id} -> {"boxes": [found]}: the box as its
//!                     cloud says it is ([`Found`], with its spot, disk, OS and
//!                     arch), for `lnk box spec`
//! lnk-<kind> remove   {account, place, id} -> {}: the box and its disk, gone
//! lnk-<kind> hostkeys {account, place, id} -> {"host_keys"}: the box's SSH host
//!                     keys, as `save-hostkeys` kept them on it, else as its
//!                     first boot printed them on its console ([`host_keys`]);
//!                     none until it has
//! lnk-<kind> save-hostkeys {account, place, id, host_keys} -> {}: kept on the
//!                     box's instance (its metadata, a tag), for another computer
//! lnk-<kind> tunnel --cloud A --zone P --id I
//!                     the box's SSH carried over the cloud's own HTTPS
//!                     (AWS: Systems Manager's AWS-StartSSHSession; Google
//!                     Cloud: IAP), on this program's stdin and stdout, as
//!                     ssh's ProxyCommand runs it ([`Tunnel`]) where port 22
//!                     can't be reached (`LNK_BOX_SSH=tunnel`); no request
//!                     on stdin, which is the tunnel's, and no reply
//! lnk-<kind> cleanup  {account, place, spec} -> {}: what the adapter made for
//!                     boxes (a firewall rule, a key pair), if its cloud says no
//!                     Link box uses it any more; else nothing
//! lnk-<kind> list     {account, owner, places} -> {"boxes"}: every box tagged
//!                     `lnk-owner=<owner>`, in `places` (none: everywhere);
//!                     with no owner, every box tagged `lnk-owner` at all,
//!                     each [`Found`] saying whose (`owner`)
//! lnk-<kind> add-key  {account, place, id, public_key} -> {"temporary"?}: the key
//!                     lets `ubuntu` in, merged with the others; `temporary`, for a
//!                     minute only (the caller appends it over SSH meanwhile)
//! lnk-<kind> remove-keys {account, place, id} -> {}: Link's own SSH keys
//!                     (commented `lnk ...`, `lnk-box` or `lnk-box-...`) gone
//!                     from the keys the cloud gives the box's `ubuntu`, any
//!                     other kept; nothing where the cloud gives none after
//!                     the first boot
//! lnk-<kind> tag      {account, place, id, tags} -> {}: tags (labels) added
//! lnk-<kind> label    {account, bucket, labels} -> {}: labels (tags) added to
//!                     a bucket, others kept
//! lnk-<kind> buckets  {account, labels} -> {"buckets"}: the buckets with every
//!                     one of `labels`, anywhere the account reaches
//! lnk-<kind> bucket-credential {account, bucket} -> {"credential"}: a
//!                     credential reaching only that bucket, as rclone's
//!                     settings for it; any made for it before stops working
//! lnk-<kind> revoke-key {account, bucket} -> {}: the bucket's key, and what
//!                     the adapter made for it, gone
//! lnk-<kind> delete-objects {account, bucket, objects} -> {}: those objects of
//!                     the bucket, by their names in it, deleted by the cloud
//!                     for an account whose own sign-in reads only; at most
//!                     1000 a call, and a name outside the bucket refuses all
//! lnk-<kind> image-save {account, place, id, image, tags} -> {"images": [saved]}:
//!                     the stopped box's disk saved as an image named
//!                     `lnk-<image>` ([`image_name`]), tagged with `tags`
//!                     ([`image_tags`], and [`IMAGE_HASH`] when it has a
//!                     hash), once the cloud says it's ready
//! lnk-<kind> image-list {account, owner?, places} -> {"images"}: account images;
//!                     with owner, only images tagged `lnk-owner=<owner>` ([`FoundImage`], its
//!                     [`IMAGE_HASH`] tag as `hash`), in `places` (none:
//!                     everywhere)
//! lnk-<kind> image-remove {account, place, image, owner} -> {}: the image
//!                     (by its id) and its snapshots, gone, if it's one Link
//!                     made for `owner`
//! ```
//!
//! `writes`, `message` and `delete-objects` are newer than the rest and
//! optional: an adapter that never answers `writes: false` needs no
//! `delete-objects`, and `lnk storage` deletes through rclone then.
//!
//! `launch` takes an `image` too, an id `image-list` gave: the box's disk
//! starts from it rather than from the OS's image. The image commands
//! are newer than the rest: an adapter without them fails them, and `lnk
//! box` then makes no box from an image there. An image's `hash` is newer
//! still: an adapter that tags an image with the tags it's given but
//! doesn't say an image's `hash` saves it with it, and `lnk box` then
//! finds no image there by its hash, so a setup script makes it again.
//!
//! A box's machine in the cloud is named `lnk-<id>` ([`Spec::id`]) and
//! tagged with [`Spec::tags`]: `lnk-box` (its name), `lnk-machine` (its
//! ID) and `lnk-owner` (`acct-<id>`), which `list` finds it by.
//!
//! Every command but `connect` reads one [`Request`] as JSON on stdin and
//! prints one [`Reply`] as JSON on stdout; errors go to stderr with a
//! nonzero exit. A command the adapter doesn't have fails saying `has no
//! command` on stderr ([`NoCommand`]), as Link's adapters always have. A box is an Ubuntu LTS machine ([`Spec::os`], 24.04 by
//! default) with a public address, reachable over SSH as `ubuntu`,
//! running [`Spec::user_data`] (cloud-init) at its first boot, which
//! sets up the SSH certificates it trusts. `remove-keys` is newer than
//! the rest: an adapter without it fails it, and `lnk box` says the keys
//! it couldn't remove. Its disk, spot and GPUs are [`Spec`]'s fields too, each
//! defaulting to the box Link made before specs; the adapter maps each
//! to its cloud's own, from fixed tables, and refuses what it doesn't
//! know ([`Spec::check`]).
//!
//! The adapter owns everything about its cloud (its CLI or API, its
//! names, its sign-in); `lnk-cloud` keeps the accounts, `lnk-box` the
//! boxes, and neither knows which cloud they're on.

use link_secretive::Duplicate;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{bail, ensure};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// A report that says `headline` and, as its why, the error's own words.
pub fn failed(headline: link_format::Sentence, err: anyhow::Error) -> link_format::Report {
    use link_format::{Prose, Report, Sentence};
    Report::error(headline)
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .cause(err)
}

/// [`failed`], as a warning.
pub fn warned(headline: link_format::Sentence, err: anyhow::Error) -> link_format::Report {
    use link_format::{Prose, Report, Sentence};
    Report::warning(headline)
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .cause(err)
}

pub mod deadline;
pub mod declared;
pub mod ending;
pub mod first_boot;
pub mod prices;
pub mod upload;
pub mod want;
pub use deadline::Deadline;
pub mod account_input;
mod calling;
mod messages;
mod names;
pub mod ownership;
mod spec;
pub use calling::{
    NoCommand, Tunnel, account, adapter, ask, call, call_as, call_quietly, call_quietly_as,
    read_request, read_request_line, read_stdin, reply, spawn,
};
pub use messages::{
    Found, FoundBucket, FoundImage, MAX_DELETE_OBJECTS, Reply, Request, carried, said,
};
pub use names::{
    BUCKET_FOLDER, BUCKET_FOLDER_NAME, BUCKET_OWNER, IMAGE_HASH, box_tags, bucket_labels,
    check_bucket, check_image, check_image_hash, check_kind, check_label, host_key, host_keys,
    image_name, image_tags, links_ssh_key, machine_name,
};
pub use spec::{Arch, Attach, MachineSize, OSES, Os, Spec};

/// The clouds Link has adapters for, with what they are.
pub const KNOWN: &[(&str, &str)] = &[("aws", "Amazon Web Services"), ("gcp", "Google Cloud")];

/// Accounts supplied by installed modules from an existing sign-in.
pub const ACCOUNTS: link_core::answers::Question<AccountQuestion, DiscoveredAccounts> =
    link_core::answers::Question::new("cloud.accounts", include_str!("accounts.toml"));

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountQuestion {}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredAccounts {
    pub accounts: Vec<Account>,
}

/// A connected account, as its adapter describes it: generated from the
/// contract's declarations (`types.toml`).
pub use declared::Account;

#[expect(
    clippy::derivable_impls,
    reason = "the type is generated, so derives nothing"
)]
impl Default for Account {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: String::new(),
            place: String::new(),
            identity: String::new(),
            login: String::new(),
            region: None,
            profile: None,
            project: None,
            settings: BTreeMap::new(),
            env: BTreeMap::new(),
            machine: None,
        }
    }
}

impl Duplicate for Account {
    fn duplicate(&self) -> Self {
        Self {
            name: self.name.clone(),
            kind: self.kind.clone(),
            place: self.place.clone(),
            identity: self.identity.clone(),
            login: self.login.clone(),
            region: self.region.clone(),
            profile: self.profile.clone(),
            project: self.project.clone(),
            settings: self.settings.clone(),
            env: self.env.duplicate(),
            machine: self.machine,
        }
    }
}

impl Account {
    /// A selector as supplied by a current or older adapter.
    pub fn setting(&self, name: &str) -> Option<&str> {
        self.settings.get(name).map(String::as_str).or_else(|| {
            setting(name)
                .then(|| self.env.get(name).map(|v| v.expose().as_str()))
                .flatten()
        })
    }
    /// Migrate older adapters' combined maps. Only known settings move;
    /// an unfamiliar value remains guarded as a secret.
    pub fn split_settings(&mut self) {
        let names: Vec<_> = self
            .env
            .keys()
            .filter(|name| setting(name))
            .cloned()
            .collect();
        for name in names {
            if let Some(value) = self.env.remove(&name) {
                self.settings
                    .entry(name)
                    .or_insert_with(|| value.into_exposed());
            }
        }
    }

    /// The explicit sign-in of the cloud's CLI. Settings are converted
    /// only at this child boundary; credential values stay guarded.
    pub fn child_environment(&self) -> BTreeMap<String, link_secretive::Secret> {
        let mut values = self.env.duplicate();
        values.extend(
            self.settings
                .iter()
                .map(|(name, value)| (name.clone(), link_secretive::Secret::new(value.clone()))),
        );
        values
    }
}

fn setting(name: &str) -> bool {
    matches!(
        name,
        "AWS_PROFILE"
            | "AWS_REGION"
            | "AWS_DEFAULT_REGION"
            | "AWS_DEFAULT_OUTPUT"
            | "AWS_PAGER"
            | "AWS_CONFIG_FILE"
            | "AWS_SHARED_CREDENTIALS_FILE"
            | "GOOGLE_APPLICATION_CREDENTIALS"
            | "CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE"
            | "CLOUDSDK_CONFIG"
            | "CLOUDSDK_ACTIVE_CONFIG_NAME"
            | "CLOUDSDK_CORE_PROJECT"
            | "CLOUDSDK_CORE_ACCOUNT"
            | "CLOUDSDK_CORE_DISABLE_PROMPTS"
            | "CLOUDSDK_COMPUTE_ZONE"
    )
}

#[cfg(test)]
mod account_settings_tests {
    use super::*;

    /// The question this owns declares its protocol beside it.
    #[test]
    fn its_question_declares_its_protocol() {
        let protocol = ACCOUNTS.protocol();
        assert_eq!((protocol.current, protocol.oldest), (1, 1));
    }

    #[test]
    fn selectors_and_credentials_stay_separate_until_the_child_boundary() {
        let mut account = Account {
            env: [
                (
                    "AWS_PROFILE".into(),
                    link_secretive::Secret::new("work".into()),
                ),
                (
                    "AWS_SECRET_ACCESS_KEY".into(),
                    link_secretive::Secret::new("signing-value".into()),
                ),
                (
                    "GOOGLE_APPLICATION_CREDENTIALS".into(),
                    link_secretive::Secret::new("/private/login.json".into()),
                ),
            ]
            .into(),
            ..Default::default()
        };
        account.split_settings();
        assert_eq!(account.settings["AWS_PROFILE"], "work");
        assert_eq!(
            account.settings["GOOGLE_APPLICATION_CREDENTIALS"],
            "/private/login.json"
        );
        assert_eq!(account.env.len(), 1);
        assert!(account.setting("AWS_SECRET_ACCESS_KEY").is_none());
        assert!(!format!("{account:?}").contains("signing-value"));
        let child = account.child_environment();
        assert_eq!(child["AWS_PROFILE"].expose(), "work");
        assert_eq!(child["AWS_SECRET_ACCESS_KEY"].expose(), "signing-value");
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
