//! `~/.lnk/safe/storage/clouds.toml`: the clouds you connected. Their
//! keys are in the engine's own config beside it (`rclone.conf`), also
//! readable only by you.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::bail;
use serde::{Deserialize, Serialize};

/// A kind of cloud `lnk storage create` knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Amazon S3.
    Aws,
    /// Google Cloud Storage.
    Gcp,
    /// Any other S3-compatible storage: R2, Backblaze, Wasabi, MinIO.
    S3,
    /// A folder on a drive, such as an external one.
    Drive,
    /// Any other storage rclone supports (Backblaze B2, Dropbox, Drive,
    /// OneDrive, SFTP, ...), set up by rclone's own questions. Its rclone
    /// type is [`Cloud::backend`].
    Rclone,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Aws => "aws",
            Kind::Gcp => "gcp",
            Kind::S3 => "s3",
            Kind::Drive => "drive",
            Kind::Rclone => "rclone",
        }
    }

    /// The storage class `push --archive` asks for, where there is one.
    pub fn archive_class(self) -> Option<&'static str> {
        match self {
            Kind::Aws => Some("DEEP_ARCHIVE"),
            Kind::Gcp => Some("ARCHIVE"),
            Kind::S3 | Kind::Drive | Kind::Rclone => None,
        }
    }

    /// Whether its buckets sign links that expire (`lnk storage link create`):
    /// S3 and what speaks it. Google Cloud Storage's signed URLs need a
    /// service account's private key, which the engine can't use for them.
    pub fn signs_links(self) -> bool {
        matches!(self, Kind::Aws | Kind::S3)
    }

    /// The order `pull` reads from, cheapest first: a folder costs
    /// nothing, then other S3-compatible storage (R2 and Backblaze charge
    /// little or nothing to download), then the big clouds.
    pub fn read_order(self) -> u8 {
        match self {
            Kind::Drive => 0,
            Kind::S3 | Kind::Rclone => 1,
            Kind::Gcp | Kind::Aws => 2,
        }
    }
}

/// Whether `class` (as a cloud reports it) is an archive tier.
pub fn is_archive(class: &str) -> bool {
    matches!(
        class,
        "DEEP_ARCHIVE" | "GLACIER" | "GLACIER_IR" | "ARCHIVE" | "COLDLINE"
    )
}

/// Whether reading `class` needs a restore first (S3's Glacier tiers).
pub fn needs_restore(class: &str) -> bool {
    matches!(class, "DEEP_ARCHIVE" | "GLACIER")
}

/// Which connected clouds a command acts on: `--cloud` names `lnk cloud`
/// accounts (aws, gcp), `--remote` the others (s3, a folder, rclone's).
/// Neither names every one.
#[derive(Debug, Clone, Default)]
pub struct Only {
    pub clouds: Vec<String>,
    pub remotes: Vec<String>,
}

impl Only {
    pub fn is_empty(&self) -> bool {
        self.clouds.is_empty() && self.remotes.is_empty()
    }
}

/// A connected cloud: an account and its keys, with one bucket per
/// folder Link backs up (your files, each owner's), so each can have its
/// own settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// What you call it (`aws`, or `--name`); also its keys' section in
    /// the engine's config.
    pub name: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// A `folder` cloud's folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// An `rclone` cloud's rclone type (`b2`, `drive`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// The bucket for each folder, by its key (`personal`, or an owner's
    /// ID).
    #[serde(default)]
    pub buckets: BTreeMap<String, Bucket>,
    /// The `lnk cloud` account it signs in as (`--cloud`), which labels
    /// owners' buckets and makes keys for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Only for the owner with this ID: its key reaches that owner's
    /// bucket and nothing else (a box's, from `lnk storage adopt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_for: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    /// Its ID, minted when Link adds the bucket here and reserved in its
    /// ledger ([`crate::ids`]), or the one it brought with an owner's
    /// move; written `bucket:<id>` in output ([`Bucket::resource`]). One
    /// without it is refused as the file is read ([`Clouds::load_from`]).
    #[serde(default)]
    pub id: String,
    /// The bucket, or on storage with folders, the folder.
    pub name: String,
    /// Files are encrypted before they leave this machine (the engine's
    /// crypt layer, in the section `<cloud>-<folder>-crypt`, the folder's
    /// key name-safe: [`crate::files::safe`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub encrypted: bool,
}

/// The kind a bucket's ID is written with in output: `bucket:<id>`.
pub const BUCKET_KIND: &str = "bucket";

impl Bucket {
    /// Its ID as output writes it, `bucket:<id>`.
    pub fn resource(&self) -> String {
        format!("{BUCKET_KIND}:{}", self.id)
    }

    /// The ID `text` writes as `bucket:<id>`: none when it's bare or of
    /// another kind.
    pub fn id_of(text: &str) -> Option<&str> {
        link_id::candidate_of(BUCKET_KIND, text)
    }
}

impl Account {
    /// Whether `folder` can have a bucket in this cloud: every folder, but
    /// in a cloud only for one owner.
    pub fn serves(&self, folder: &str) -> bool {
        self.only_for.as_deref().is_none_or(|id| id == folder)
    }

    /// The folders it has a bucket for, by key: your files first, then
    /// each owner's by its ID, as `clouds.toml` keeps them. A key that's
    /// neither is left out: no folder Link backs up has it.
    pub fn folders(&self) -> Vec<&str> {
        let personal = self
            .buckets
            .keys()
            .filter(|key| *key == crate::files::PERSONAL);
        let owners = self
            .buckets
            .keys()
            .filter(|key| crate::files::parse_owner(key).is_ok());
        personal.chain(owners).map(String::as_str).collect()
    }

    /// `folder`'s bucket in this cloud, as the engine works with it.
    pub fn cloud(&self, folder: &str) -> Option<Cloud> {
        let bucket = self.buckets.get(folder)?;
        Some(Cloud {
            name: self.name.clone(),
            kind: self.kind,
            folder: folder.to_string(),
            bucket: bucket.name.clone(),
            region: self.region.clone(),
            path: self.path.clone(),
            backend: self.backend.clone(),
            encrypted: bucket.encrypted,
        })
    }
}

/// One folder's bucket in one cloud: what every command works with.
#[derive(Debug, Clone, PartialEq)]
pub struct Cloud {
    /// The cloud's name (`aws`); copies are recorded under it.
    pub name: String,
    pub kind: Kind,
    /// The folder this bucket backs up, by its key (`personal`, an
    /// owner's ID).
    pub folder: String,
    /// The bucket in that cloud, or the folder on storage with folders.
    pub bucket: String,
    pub region: Option<String>,
    pub path: Option<String>,
    pub backend: Option<String>,
    pub encrypted: bool,
}

impl Cloud {
    /// The engine's name for the bucket: `aws:lnk-bucket-a1b2c3`, or for
    /// an encrypted one, its crypt section: `aws-personal-crypt:`.
    pub fn remote(&self) -> String {
        if self.encrypted {
            format!("{}:", self.crypt_section())
        } else {
            self.plain_remote()
        }
    }

    /// The bucket itself, under any encryption: `aws:lnk-bucket-a1b2c3`.
    pub fn plain_remote(&self) -> String {
        format!("{}:{}", self.name, self.bucket)
    }

    /// The section of the engine's config that encrypts this bucket.
    pub fn crypt_section(&self) -> String {
        format!("{}-{}-crypt", self.name, crate::files::safe(&self.folder))
    }

    /// Where it is, for people: `aws (bucket lnk-bucket-a1b2c3, us-east-1)`.
    pub fn describe(&self) -> String {
        let place = match (self.kind, &self.path) {
            (Kind::Drive, Some(path)) => format!("{path}/{}", self.bucket),
            (Kind::Rclone, _) => self.bucket.clone(),
            _ => format!("bucket {}", self.bucket),
        };
        let region = self
            .region
            .as_deref()
            .map(|r| format!(", {r}"))
            .unwrap_or_default();
        let encrypted = if self.encrypted { ", encrypted" } else { "" };
        let kind = match (self.kind, &self.backend) {
            (Kind::Rclone, Some(backend)) if *backend != self.name => format!("{backend}, "),
            (Kind::Rclone, _) => String::new(),
            (kind, _) if self.name == kind.name() => String::new(),
            (kind, _) => format!("{}, ", kind.name()),
        };
        format!("{} ({kind}{place}{region}{encrypted})", self.name)
    }
}

/// A name for a cloud: lowercase letters, digits and `-`, starting with a
/// letter, not ending in `-crypt` (Link's, for encrypted buckets).
pub fn check_name(name: &str) -> anyhow::Result<()> {
    let ok = name.len() <= 32
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.ends_with("-crypt");
    if !ok {
        bail!(
            "{name:?} can't name a cloud: start with a letter, use up to 32 lowercase letters, digits and -, like aws or my-drive, and don't end in -crypt, which Link keeps for encrypted buckets"
        );
    }
    Ok(())
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Clouds {
    #[serde(default, rename = "cloud")]
    pub clouds: Vec<Account>,
}

link_files::specification!(Clouds, include_str!("clouds.schema.toml"));

impl Clouds {
    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(crate::files::config_dir()?.join("clouds.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&Self::path()?)
    }

    pub fn load_from(path: &std::path::Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let clouds: Self = link_files::saved::parse(&text)
                    .within(format!("reading {}", path.display()))?;
                let mut seen = std::collections::BTreeSet::new();
                for account in &clouds.clouds {
                    for bucket in account.buckets.values() {
                        anyhow::ensure!(
                            link_id::is_candidate(&bucket.id),
                            "in {}: {}'s bucket {} has no ID Link minted: remove the [[cloud]] \
                             entry named {} from that file, then connect it again with lnk \
                             storage create",
                            path.display(),
                            account.name,
                            bucket.name,
                            account.name
                        );
                        anyhow::ensure!(
                            seen.insert(bucket.id.as_str()),
                            "in {}: bucket:{} names two buckets",
                            path.display(),
                            bucket.id
                        );
                    }
                }
                Ok(clouds)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).within(format!("reading {}", path.display())),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path()?;
        link_files::write_private(&path, link_files::saved::print(self)?.as_bytes())
            .within(format!("writing {}", path.display()))
    }

    pub fn get(&self, name: &str) -> Option<&Account> {
        self.clouds.iter().find(|c| c.name == name)
    }

    /// The clouds `only` names, or all of them.
    pub fn pick(&self, only: &Only) -> anyhow::Result<Vec<Account>> {
        if self.clouds.is_empty() {
            bail!("no clouds yet: lnk storage create aws (or gcp, s3, drive --path <folder>)");
        }
        if only.is_empty() {
            return Ok(self.clouds.clone());
        }
        let named = |accounts: bool| -> Vec<&str> {
            self.clouds
                .iter()
                .filter(|c| c.account.is_some() == accounts)
                .map(|c| c.name.as_str())
                .collect()
        };
        let clouds = only.clouds.iter().map(|name| {
            self.clouds
                .iter()
                .find(|c| c.account.as_deref() == Some(name.as_str()))
                .cloned()
                .within(format!(
                    "no cloud account {name:?} connected (lnk cloud list); connected: {}",
                    named(true).join(", ")
                ))
        });
        let remotes = only.remotes.iter().map(|name| {
            self.clouds
                .iter()
                .find(|c| c.account.is_none() && c.name == *name)
                .cloned()
                .within(format!(
                    "no remote {name:?} (lnk storage list); connected: {}",
                    named(false).join(", ")
                ))
        });
        clouds.chain(remotes).collect()
    }

    /// `folder`'s bucket in each of `accounts` that has one.
    pub fn buckets(accounts: &[Account], folder: &str) -> Vec<Cloud> {
        accounts.iter().filter_map(|a| a.cloud(folder)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(check_name("aws").is_ok());
        assert!(check_name("my-drive2").is_ok());
        assert!(check_name("AWS").is_err());
        assert!(check_name("2aws").is_err());
        assert!(check_name("a b").is_err());
        assert!(check_name("x:y").is_err());
        assert!(check_name("aws-crypt").is_err());
        assert!(check_name("").is_err());
    }

    #[test]
    fn remotes() {
        let mut account = Account {
            name: "aws".into(),
            kind: Kind::Aws,
            region: Some("eu-west-1".into()),
            path: None,
            backend: None,
            buckets: BTreeMap::new(),
            account: None,
            only_for: None,
        };
        assert_eq!(account.cloud("personal"), None);
        account.buckets.insert(
            "personal".into(),
            Bucket {
                id: "a1b2c3d4e5f7".into(),
                name: "lnk-bucket-abc123".into(),
                encrypted: false,
            },
        );
        account.buckets.insert(
            "job:a1b2c3d4e5f6".into(),
            Bucket {
                id: "a1b2c3d4e5f6".into(),
                name: "lnk-bucket-def456".into(),
                encrypted: true,
            },
        );
        account.buckets.insert(
            "../elsewhere".into(),
            Bucket {
                id: "a1b2c3d4e5f8".into(),
                name: "lnk-bucket-ghi789".into(),
                encrypted: false,
            },
        );
        // Your files first, then each owner's; a key that's neither, never.
        assert_eq!(account.folders(), ["personal", "job:a1b2c3d4e5f6"]);
        account.buckets.remove("../elsewhere");
        let personal = account.cloud("personal").unwrap();
        assert_eq!(personal.remote(), "aws:lnk-bucket-abc123");
        assert_eq!(
            personal.describe(),
            "aws (bucket lnk-bucket-abc123, eu-west-1)"
        );
        let owned = account.cloud("job:a1b2c3d4e5f6").unwrap();
        assert_eq!(owned.remote(), "aws-job-a1b2c3d4e5f6-crypt:");
        assert_eq!(owned.plain_remote(), "aws:lnk-bucket-def456");
        let text = toml::to_string(&Clouds {
            clouds: vec![account.clone()],
        })
        .unwrap();
        let back: Clouds = toml::from_str(&text).unwrap();
        assert_eq!(back.clouds, [account]);
    }

    #[test]
    fn cloud_names_an_account_and_remote_names_the_rest() {
        let connected = |name: &str, kind, account: Option<&str>| Account {
            name: name.into(),
            kind,
            region: None,
            path: None,
            backend: None,
            buckets: BTreeMap::new(),
            account: account.map(str::to_string),
            only_for: None,
        };
        let clouds = Clouds {
            clouds: vec![
                connected("gcp-ci", Kind::Gcp, Some("gcp-ci")),
                connected("drive", Kind::Drive, None),
            ],
        };
        let only = |clouds: &[&str], remotes: &[&str]| Only {
            clouds: clouds.iter().map(|c| (*c).to_string()).collect(),
            remotes: remotes.iter().map(|r| (*r).to_string()).collect(),
        };
        let names =
            |picked: Vec<Account>| -> Vec<String> { picked.into_iter().map(|a| a.name).collect() };
        assert_eq!(
            names(clouds.pick(&Only::default()).unwrap()),
            ["gcp-ci", "drive"]
        );
        assert_eq!(
            names(clouds.pick(&only(&["gcp-ci"], &[])).unwrap()),
            ["gcp-ci"]
        );
        assert_eq!(
            names(clouds.pick(&only(&[], &["drive"])).unwrap()),
            ["drive"]
        );
        // Each flag names only its own kind of cloud.
        assert!(clouds.pick(&only(&["drive"], &[])).is_err());
        assert!(clouds.pick(&only(&[], &["gcp-ci"])).is_err());
    }
}
