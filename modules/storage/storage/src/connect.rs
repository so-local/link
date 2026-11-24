//! `lnk storage create` and `connect`: signing in to a cloud (on AWS and
//! Google Cloud, as its `lnk cloud` account, connected first if it isn't),
//! making a bucket for each folder or finding the one that exists, and
//! proving the keys work before saving anything.

use std::path::{Path, PathBuf};

use crate::said;
use anyhow::{bail, ensure};
use link_format::{Foreign, WithinExt, sentence};
use ring::rand::SecureRandom;

use crate::clouds::{Account, Bucket, Cloud, Clouds, Kind, check_name};
use crate::files::{Folder, PERSONAL};
use crate::rclone::Rclone;

#[derive(Debug, Default)]
pub struct Options {
    pub name: Option<String>,
    /// A bucket that already exists (or, for a drive, the folder inside
    /// it); else Link makes one.
    pub bucket: Option<String>,
    pub region: Option<String>,
    /// aws: the `~/.aws` profile to use, connecting its account.
    pub profile: Option<String>,
    /// gcp: the project new buckets go in.
    pub project: Option<String>,
    /// s3: the endpoint URL.
    pub endpoint: Option<String>,
    /// aws, s3: an access key id, instead of an existing login.
    pub key_id: Option<String>,
    /// Encrypt your files' bucket.
    pub encrypt: bool,
    /// An owner's bucket, when it already exists (from another machine):
    /// the owner's ID and the bucket. It's always encrypted.
    pub owner_bucket: Option<(crate::files::Owner, String)>,
    /// aws, gcp: the `lnk cloud` account it is, and is named as
    /// (`--cloud`) [default: the account named as its kind].
    pub account: Option<String>,
    /// folder: where.
    pub path: Option<PathBuf>,
    /// rclone: the rclone type (`b2`, `drive`, `sftp`, ...).
    pub backend: Option<String>,
    /// rclone: settings given up front (`host=example.com`); rclone asks
    /// for the rest when there's a terminal.
    pub settings: Vec<(String, String)>,
}

/// rclone types that wrap other remotes or aren't storage: `connect`
/// can't set them up on their own.
const NOT_STORAGE: &[&str] = &[
    "alias", "archive", "cache", "chunker", "combine", "compress", "crypt", "hasher", "http",
    "local", "memory", "union",
];

/// What connecting asks of the person: the command asks it, as it can.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked<'a> {
    /// An S3 storage's endpoint URL.
    Endpoint,
    /// Whose access key id.
    KeyId(&'a str),
    /// Whose secret access key.
    SecretKey(&'a str),
    /// The encryption password of a bucket that exists, for an owner's
    /// folder or your files; none given makes a new one for a new bucket.
    Password { owned: bool, reconnecting: bool },
    /// A yes to making a new bucket for your files in the cloud named,
    /// which bills it: any answer is a yes, a no an error.
    MakeBucket(&'a str),
}

/// Asks the person what is `Asked`, and answers with what they typed.
pub type Prompt<'a> = &'a mut dyn FnMut(Asked) -> anyhow::Result<String>;

pub struct Connected {
    pub account: Account,
    /// Encryption passwords Link made, by folder, to show once.
    pub new_passwords: Vec<(String, link_secretive::Secret)>,
    /// Which login it uses, for people.
    pub login: String,
}

/// Connects a cloud of `kind` and makes (or finds) the bucket for your
/// files, and an owner's with `--owner-bucket`; an owner's otherwise
/// comes on its first push. `folders` are the folders Link backs up. On
/// failure, nothing is saved.
pub fn connect(
    rclone: &Rclone,
    folders: &[Folder],
    taken: &[Account],
    kind: Kind,
    options: Options,
    prompt: Prompt,
) -> anyhow::Result<Connected> {
    let name = match (&options.name, kind, &options.backend) {
        (Some(name), ..) => name.clone(),
        // An aws or gcp cloud is named as the `lnk cloud` account it is.
        (None, Kind::Aws | Kind::Gcp, _) => options
            .account
            .clone()
            .unwrap_or_else(|| kind.name().into()),
        (None, Kind::Rclone, Some(backend)) => backend.clone(),
        (None, ..) => kind.name().into(),
    };
    check_name(&name).map_err(|e| {
        if options.name.is_none() {
            anyhow::anyhow!("{e} (name it with --name)")
        } else {
            e
        }
    })?;
    if taken.iter().any(|c| c.name == name) {
        let other = if matches!(kind, Kind::Aws | Kind::Gcp) {
            "--cloud"
        } else {
            "--name"
        };
        bail!(
            "{name} is already connected: lnk storage disconnect {name} first, or pick another {other}"
        );
    }
    let mut account = Account {
        name: name.clone(),
        kind,
        region: None,
        path: None,
        backend: options.backend.clone(),
        buckets: Default::default(),
        account: options.account.clone(),
        only_for: None,
    };
    let result = (|| {
        let login = sign_in(rclone, folders, &mut account, &options, prompt)?;
        let mut new_passwords = Vec::new();
        if options.bucket.is_none() && bucket_based(rclone, &account)? {
            prompt(Asked::MakeBucket(&name))?;
        }
        let personal = add_bucket(
            rclone,
            &mut account,
            PERSONAL,
            options.bucket.clone(),
            None,
            options.encrypt,
            Password::Ask { owned: false },
            prompt,
        )?;
        new_passwords.extend(personal.map(|p| (PERSONAL.to_string(), p.into())));
        if let Some((owner, bucket)) = &options.owner_bucket {
            let owned = add_bucket(
                rclone,
                &mut account,
                owner,
                Some(bucket.clone()),
                None,
                true,
                Password::Ask { owned: true },
                prompt,
            )?;
            new_passwords.extend(owned.map(|p| (owner.to_string(), p.into())));
        }
        Ok(Connected {
            account: account.clone(),
            new_passwords,
            login,
        })
    })();
    if result.is_err() {
        forget(rclone, &account);
    }
    result
}

/// Settings given as `key=value` for rclone's setup: plain lowercase keys
/// only (never `type`, which would change the storage, nor anything
/// rclone could read as an option), and none that rclone treats as secret
/// (`secret`): those are asked for at rclone's prompt, never taken on a
/// command line other users can see.
fn check_settings(settings: &[(String, String)], secret: &[String]) -> anyhow::Result<()> {
    for (key, _) in settings {
        let plain = !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        ensure!(plain, "{key:?} isn't a setting name");
        ensure!(
            key != "type",
            "the storage type is the first argument, not a setting"
        );
        ensure!(
            !secret.contains(key),
            "{key} is secret: leave it out, and rclone asks for it (hidden) instead"
        );
    }
    Ok(())
}

/// Refuses a drive cloud whose backup folder is, holds or is inside one
/// of Link's folders (through links too): a "copy" there could be the
/// file itself. Checked before every command, since the backup location
/// can change after `connect`.
pub fn check_apart(cloud: &Cloud, folders: &[Folder]) -> anyhow::Result<()> {
    let (Kind::Drive, Some(path)) = (cloud.kind, &cloud.path) else {
        return Ok(());
    };
    let stored = Path::new(path).join(&cloud.bucket);
    for folder in folders {
        if crate::files::overlap(&stored, &folder.root) {
            bail!(
                "{}'s backup folder ({}) and {} ({}) are one inside the other, maybe through a link: refusing to use it",
                cloud.name,
                stored.display(),
                folder.label(),
                folder.root.display()
            );
        }
    }
    Ok(())
}

/// Removes `account`'s keys and encryption from the engine's config.
pub fn forget(rclone: &Rclone, account: &Account) {
    let _ = rclone.remove_section(&account.name);
    for folder in account.buckets.keys() {
        let _ = rclone.remove_section(&format!(
            "{}-{}-crypt",
            account.name,
            crate::files::safe(folder)
        ));
    }
}

/// Where an encryption password comes from.
#[derive(Debug, Clone, Copy)]
pub enum Password<'a> {
    /// Ask for it, for an owner's bucket or your files'; an empty answer
    /// makes a new one.
    Ask { owned: bool },
    /// Link makes one.
    New,
    /// This one (the same as another cloud's, so there's one to keep).
    Given(&'a str),
    /// This one, as rclone keeps it (obscured): the owner's, from its
    /// other clouds or another machine.
    Obscured(&'a str),
}

/// The encryption password of the folder whose key is `id` (an
/// owner's ID) as its buckets here keep it (obscured), from any cloud it
/// has one in: one password per owner.
pub fn owner_password(id: &str) -> anyhow::Result<Option<String>> {
    let config = crate::rclone::config_in(&crate::files::config_dir()?);
    for account in Clouds::load()?.clouds {
        if let Some(cloud) = account.cloud(id).filter(|c| c.encrypted)
            && let Some(settings) = crate::rclone::section(&config, &cloud.crypt_section())?
            && let Some((_, password)) = settings.iter().find(|(k, _)| k == "password")
        {
            return Ok(Some(password.clone()));
        }
    }
    Ok(None)
}

/// Labels owner `id` (named `name`)'s bucket `bucket` in `account` with
/// its name-safe ID ([`crate::files::safe`]), name and Link account,
/// through `lnk cloud` (its `--cloud` account), so another machine finds
/// it. Best effort: said on stderr when it can't.
pub fn label(account: &Account, bucket: &str, id: &crate::files::Owner, name: &str) {
    let Some(cloud) = &account.account else {
        return;
    };
    let owner = owner();
    let labels =
        link_cloud_contract::bucket_labels(&crate::files::safe(id), name, owner.as_deref());
    let request = serde_json::json!({"bucket": bucket, "labels": labels});
    if let Err(err) = call_cloud(cloud, "label", &request) {
        said::warn(
            sentence!(
                "{} couldn't be labelled as {}'s, so another machine won't find it.",
                bucket,
                name
            ),
            &err,
        );
    }
}

/// The key [`crate::owners::keys`] made for bucket `bucket`, and what
/// its cloud made to hold it, gone, through `lnk cloud` account `cloud`.
/// Done when there is none: a bucket never given a key revokes cleanly.
pub fn revoke_key(cloud: &str, bucket: &str) -> anyhow::Result<()> {
    call_cloud(cloud, "revoke-key", &serde_json::json!({"bucket": bucket})).map(|_| ())
}

/// Deletes each of `account`'s buckets with `delete`, first revoking with
/// `revoke` (given its `lnk cloud` account and the bucket: [`revoke_key`])
/// any key made for it, so nothing made for a bucket outlives it. A
/// cloud with no `lnk cloud` account never made one. Stops at the first
/// that fails, a bucket whose key is still there kept.
pub fn delete_buckets(
    account: &Account,
    revoke: &mut dyn FnMut(&str, &str) -> anyhow::Result<()>,
    delete: &mut dyn FnMut(&Cloud) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for folder in account.buckets.keys() {
        let Some(cloud) = account.cloud(folder) else {
            continue;
        };
        if let Some(keys) = &account.account {
            revoke(keys, &cloud.bucket).within(format!(
                "couldn't revoke the key made for {}, so it wasn't deleted",
                cloud.bucket
            ))?;
        }
        delete(&cloud).within(format!("couldn't delete {}", cloud.bucket))?;
    }
    Ok(())
}

/// Adapter command `command` as `lnk cloud` account `cloud` (`lnk cloud
/// env <cloud> --json`), with `request`'s `bucket`, `labels` and
/// `objects`: the adapter's reply. The way the box module reaches an adapter too.
pub fn call_cloud(
    cloud: &str,
    command: &str,
    request: &serde_json::Value,
) -> anyhow::Result<link_cloud_contract::Reply> {
    let account = cloud_env(cloud)?.within(format!(
        "no cloud account named {cloud:?}: lnk cloud connect aws (or gcp)"
    ))?;
    let kind = account.kind.clone();
    let request = link_cloud_contract::Request {
        account,
        bucket: request["bucket"].as_str().map(str::to_string),
        labels: serde_json::from_value(request["labels"].clone()).unwrap_or_default(),
        objects: serde_json::from_value(request["objects"].clone()).unwrap_or_default(),
        ..link_cloud_contract::Request::default()
    };
    link_cloud_contract::call(&kind, command, &request)
}

/// Whose buckets these are: `acct-<id>` of the login here (`lnk
/// auth show --json`), if it knows one.
pub fn owner() -> Option<String> {
    link_core::owner::account_owner()
}

/// Whether a folder's place in `account` is a bucket of its own, which
/// its cloud bills, rather than a folder in one place (a `folder` cloud's,
/// or an rclone type that has no buckets).
pub fn bucket_based(rclone: &Rclone, account: &Account) -> anyhow::Result<bool> {
    Ok(match account.kind {
        Kind::Aws | Kind::Gcp | Kind::S3 => true,
        Kind::Drive => false,
        Kind::Rclone => rclone.bucket_based(&format!("{}:", account.name))?,
    })
}

/// Adds `folder`'s bucket to `account`: `existing`, or a new one, made,
/// encrypted with `encrypt` (the password from `password`), and tested,
/// under `id`, the ID it brought with an owner's move, else a new one
/// ([`crate::ids`]). Returns a password Link made, to show once. On
/// failure `account` is as it was.
#[expect(
    clippy::too_many_arguments,
    reason = "each says one thing about the bucket"
)]
pub fn add_bucket(
    rclone: &Rclone,
    account: &mut Account,
    folder: &str,
    existing: Option<String>,
    id: Option<&str>,
    encrypt: bool,
    password: Password,
    prompt: Prompt,
) -> anyhow::Result<Option<String>> {
    let bucket_based = bucket_based(rclone, account)?;
    let reconnecting = existing.is_some();
    let name = match existing {
        Some(bucket) => bucket,
        None if bucket_based => new_bucket_name()?,
        None => format!("{FOLDER}/{}", crate::files::safe(folder)),
    };
    if bucket_based {
        check_bucket_name(&name)?;
    } else {
        check_folder_name(&name)?;
    }
    let nested = |a: &str, b: &str| a == b || b.starts_with(&format!("{a}/"));
    if account
        .buckets
        .values()
        .any(|b| nested(&b.name, &name) || nested(&name, &b.name))
    {
        bail!("{name} is, or is inside, the bucket of another folder; each gets its own");
    }
    let (id, minted) = match id {
        Some(id) => {
            crate::ids::arrive(id)?;
            (id.to_string(), false)
        }
        None => (crate::ids::reserve()?, true),
    };
    account.buckets.insert(
        folder.to_string(),
        Bucket {
            id: id.clone(),
            name: name.clone(),
            encrypted: encrypt,
        },
    );
    let cloud = account.cloud(folder).expect("just added");
    let made = std::cell::Cell::new(false);
    let result = (|| {
        let password = if encrypt {
            if let Password::Obscured(obscured) = password {
                encrypt_obscured(rclone, &cloud, obscured)?;
                return Ok(None);
            }
            let given = match password {
                Password::Ask { owned } => prompt(Asked::Password {
                    owned,
                    reconnecting,
                })?,
                Password::New | Password::Obscured(_) => String::new(),
                Password::Given(password) => password.to_string(),
            };
            ensure!(
                !(given.is_empty() && reconnecting),
                "{name} already exists: give the encryption password you saved when it was made"
            );
            let (password, new) = if given.is_empty() {
                (random_text(32)?, true)
            } else {
                (given, false)
            };
            encrypt_bucket(rclone, &cloud, &password)?;
            new.then_some(password)
        } else {
            None
        };
        if reconnecting {
            ensure!(
                rclone.exists(&cloud)?,
                "there's no {name} in {}: lnk storage create makes new buckets",
                cloud.name
            );
        } else {
            rclone
                .make_bucket(&cloud)
                .within(format!("couldn't make the bucket {name}"))?;
            made.set(true);
        }
        rclone.test(&cloud)?;
        // A wrong password still writes; it just can't read what's there.
        if encrypt && reconnecting {
            ensure!(
                rclone.password_opens(&cloud)?,
                "that password doesn't open the files already in {name}"
            );
        }
        Ok(password)
    })();
    // A bucket made here goes again, so a failed create leaves nothing
    // behind in the cloud to pay for or to find.
    let result = match result {
        Err(err) if made.get() => match rclone.remove_empty(&cloud) {
            Ok(()) => Err(err.context(format!("{name} was made, and deleted again"))),
            Err(left) => Err(err.context(format!(
                "{name} was made but couldn't be deleted again ({left:#}): delete it in {}'s console",
                cloud.name
            ))),
        },
        other => other,
    };
    if result.is_err() {
        account.buckets.remove(folder);
        let _ = rclone.remove_section(&cloud.crypt_section());
        // Minted for a bucket never added: given to nothing else.
        if minted {
            let _ = crate::ids::retire([id.as_str()]);
        }
    }
    result
}

/// The `lnk cloud` account `cloud` signs in as (`--cloud`, else the one
/// named as the cloud), connected now when there's none: `lnk cloud
/// connect` asks what it needs, here.
fn cloud_account(
    cloud: &mut Account,
    options: &Options,
) -> anyhow::Result<link_cloud_contract::Account> {
    let kind = cloud.kind.name();
    let name = options
        .account
        .clone()
        .unwrap_or_else(|| cloud.name.clone());
    let found = match cloud_env(&name)? {
        Some(found) => {
            ensure!(
                found.kind == kind,
                "the cloud account {name} is {}, not {kind}",
                link_format::clean(&found.kind)
            );
            ensure!(
                options.profile.is_none() && options.key_id.is_none(),
                "the cloud account {name} is connected already, with its own sign-in: \
                 lnk cloud connect {kind} --name {name} changes it"
            );
            found
        }
        None => {
            let mut connect = link_core::lnk();
            connect.args(["cloud", "connect", kind, "--name", &name, "--quiet"]);
            let flags = [
                (
                    "--region",
                    options.region.as_ref().filter(|_| kind == "aws"),
                ),
                ("--profile", options.profile.as_ref()),
                ("--key-id", options.key_id.as_ref()),
                ("--project", options.project.as_ref()),
            ];
            for (flag, value) in flags {
                if let Some(value) = value {
                    connect.args([flag, value]);
                }
            }
            let status = connect.status().within("running lnk cloud connect")?;
            ensure!(
                status.success(),
                "connecting the {kind} account {name} didn't finish (above)"
            );
            cloud_env(&name)?.within(format!("lnk cloud connect {kind} didn't connect {name}"))?
        }
    };
    cloud.account = Some(name);
    Ok(found)
}

/// `lnk cloud` account `name` (`lnk cloud env <name> --json`); None when
/// there's no such account.
pub fn cloud_env(name: &str) -> anyhow::Result<Option<link_cloud_contract::Account>> {
    link_core::ensure("cloud", "A bucket in a cloud account")?;
    let output = link_core::lnk()
        .args(["cloud", "env", name, "--json"])
        .stdin(std::process::Stdio::null())
        .output()
        .within("running lnk cloud env")?;
    if output.status.success() {
        return serde_json::from_slice(&output.stdout)
            .map(Some)
            .within("reading lnk cloud env");
    }
    let said = String::from_utf8_lossy(&output.stderr);
    if said.contains("no cloud account named") {
        return Ok(None);
    }
    bail!(
        "couldn't ask lnk cloud for the account {name}: {}",
        link_format::clean(said.trim().trim_start_matches("Error: "))
    )
}

/// Writes the cloud's section in the engine's config: its kind, place
/// and keys.
fn sign_in(
    rclone: &Rclone,
    folders: &[Folder],
    cloud: &mut Account,
    options: &Options,
    prompt: Prompt,
) -> anyhow::Result<String> {
    let section = cloud.name.clone();
    let section = section.as_str();
    match cloud.kind {
        Kind::Drive => {
            let path = options
                .path
                .as_deref()
                .within("which folder? lnk storage create drive --path /Volumes/MyDrive")?;
            let path = path
                .canonicalize()
                .within(format!("{} isn't there", path.display()))?;
            ensure!(path.is_dir(), "{} isn't a folder", path.display());
            for folder in folders {
                let root = &folder.root;
                let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                ensure!(
                    !path.starts_with(&root) && !root.starts_with(&path),
                    "{} and {} ({}) can't be inside one another",
                    path.display(),
                    folder.label(),
                    root.display()
                );
            }
            let remote = path.to_str().within("the folder's path must be UTF-8")?;
            rclone.set_section(section, &[("type", "alias"), ("remote", remote)])?;
            cloud.path = Some(remote.to_string());
            Ok(format!("the folder {remote}"))
        }
        Kind::Aws | Kind::Gcp => {
            let account = cloud_account(cloud, options)?;
            let mut settings = Vec::new();
            if cloud.kind == Kind::Aws {
                let region = options
                    .region
                    .clone()
                    .or_else(|| account.region.clone())
                    .unwrap_or_else(|| "us-east-1".into());
                cloud.region = Some(region.clone());
                settings.extend([
                    ("type", "s3".to_string()),
                    ("provider", "AWS".into()),
                    ("region", region.clone()),
                ]);
                if region != "us-east-1" {
                    settings.push(("location_constraint", region));
                }
            } else {
                let project = options
                    .project
                    .clone()
                    .or_else(|| account.project.clone())
                    .within("a project is needed to make a bucket: --project")?;
                settings.extend([
                    ("type", "google cloud storage".to_string()),
                    ("project_number", project),
                    ("bucket_policy_only", "true".into()),
                ]);
                if let Some(region) = &options.region {
                    cloud.region = Some(region.clone());
                    settings.push(("location", region.clone()));
                }
            }
            // Nothing of its sign-in here: each run gets the account's.
            set(rclone, section, &settings)?;
            rclone.sign_in_as(section, &account.name);
            Ok(format!(
                "the cloud account {}: {}",
                account.name,
                link_format::clean(if account.identity.is_empty() {
                    &account.login
                } else {
                    &account.identity
                })
            ))
        }
        Kind::S3 => {
            let endpoint = match &options.endpoint {
                Some(endpoint) => endpoint.clone(),
                None => prompt(Asked::Endpoint)?,
            };
            ensure!(
                endpoint.starts_with("https://") || endpoint.starts_with("http://"),
                "the endpoint must be a URL"
            );
            let (id, secret) = keys(options, prompt, "the storage's")?;
            let mut settings = vec![
                ("type", "s3".to_string()),
                ("provider", "Other".into()),
                ("endpoint", endpoint),
                ("access_key_id", id),
                ("secret_access_key", secret),
            ];
            if let Some(region) = &options.region {
                cloud.region = Some(region.clone());
                settings.push(("region", region.clone()));
            }
            set(rclone, section, &settings)?;
            Ok("the access key you gave".into())
        }
        Kind::Rclone => {
            let backend = options
                .backend
                .as_deref()
                .within("which rclone type? e.g. lnk storage create b2")?;
            if NOT_STORAGE.contains(&backend) {
                bail!(
                    "{backend} isn't storage lnk storage can connect on its own{}",
                    if backend == "local" {
                        ": for a drive, lnk storage create drive --path <folder>"
                    } else if backend == "crypt" {
                        ": for encryption, connect the storage with --encrypt"
                    } else {
                        ""
                    }
                );
            }
            let providers = rclone.providers()?;
            let Some((_, secret)) = providers.iter().find(|(name, _)| name == backend) else {
                bail!(
                    "rclone has no storage type {backend:?}; lnk storage rclone help backends lists them"
                );
            };
            check_settings(&options.settings, secret)?;
            let ask = std::io::IsTerminal::is_terminal(&std::io::stdin());
            if ask {
                said::note(sentence!(
                    "rclone asks how to reach your {} storage: Enter keeps a default.",
                    Foreign::new(backend)
                ));
            }
            let settings: Vec<(&str, &str)> = options
                .settings
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            rclone.create_interactive(section, backend, &settings, ask)?;
            Ok(format!("rclone's {backend} setup"))
        }
    }
}

fn set(rclone: &Rclone, section: &str, settings: &[(&str, String)]) -> anyhow::Result<()> {
    let settings: Vec<(&str, &str)> = settings.iter().map(|(k, v)| (*k, v.as_str())).collect();
    rclone.set_section(section, &settings)
}

/// An access key id and secret, from `--key-id` and a prompt.
fn keys(options: &Options, prompt: Prompt, whose: &str) -> anyhow::Result<(String, String)> {
    let id = match &options.key_id {
        Some(id) => id.clone(),
        None => prompt(Asked::KeyId(whose))?,
    };
    let secret = prompt(Asked::SecretKey(whose))?;
    ensure!(
        !id.is_empty() && !secret.is_empty(),
        "a key id and secret are needed"
    );
    Ok((id, secret))
}

/// Encrypts `cloud`'s bucket with `password`: a crypt section over it.
fn encrypt_bucket(rclone: &Rclone, cloud: &Cloud, password: &str) -> anyhow::Result<()> {
    let obscured = rclone.obscure(password)?;
    encrypt_obscured(rclone, cloud, &obscured)
}

/// Encrypts `cloud`'s bucket with a password as rclone keeps it.
pub fn encrypt_obscured(rclone: &Rclone, cloud: &Cloud, obscured: &str) -> anyhow::Result<()> {
    rclone.set_section(
        &cloud.crypt_section(),
        &[
            ("type", "crypt"),
            ("remote", &cloud.plain_remote()),
            ("password", obscured),
            ("filename_encryption", "standard"),
            ("directory_name_encryption", "true"),
        ],
    )
}

/// The folder Link keeps its folders' files in (`local-link/personal`,
/// `local-link/<kind>-<id>`), on storage that has folders rather than buckets
/// (a drive, Dropbox, SFTP).
const FOLDER: &str = "local-link";

/// `lnk-bucket-<12 random>`: bucket names are global on AWS and GCP, and
/// anyone can test whether one exists, so the name says nothing about you.
fn new_bucket_name() -> anyhow::Result<String> {
    Ok(format!("lnk-bucket-{}", random_text(12)?))
}

/// What AWS and GCP accept: 3 to 63 lowercase letters, digits, `-` and
/// `.`, starting and ending with a letter or digit.
fn check_bucket_name(name: &str) -> anyhow::Result<()> {
    let ok = (3..=63).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.ends_with(|c: char| c.is_ascii_alphanumeric());
    ensure!(
        ok,
        "{name:?} can't name a bucket: 3 to 63 lowercase letters, digits, - and ."
    );
    Ok(())
}

/// A folder path on storage with folders: relative, no `..`, no line
/// breaks.
fn check_folder_name(name: &str) -> anyhow::Result<()> {
    let ok = !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\n', '\r'])
        && !name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    ensure!(
        ok,
        "{name:?} can't name a folder there: a relative path, like Backups/Link"
    );
    Ok(())
}

/// `len` random lowercase letters and digits.
fn random_text(len: usize) -> anyhow::Result<String> {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let rng = ring::rand::SystemRandom::new();
    let mut text = String::new();
    while text.len() < len {
        let mut byte = [0u8];
        rng.fill(&mut byte)
            .map_err(|_| anyhow::anyhow!("no randomness"))?;
        // Only below 252 (7 x 36), so every character is as likely.
        if byte[0] < 252 {
            text.push(CHARS[byte[0] as usize % CHARS.len()] as char);
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_names() {
        let name = new_bucket_name().unwrap();
        assert!(name.starts_with("lnk-bucket-"));
        assert_eq!(name.len(), "lnk-bucket-".len() + 12);
        check_bucket_name(&name).unwrap();
        assert_ne!(name, new_bucket_name().unwrap());
        check_folder_name("local-link").unwrap();
        check_folder_name("Backups/Link").unwrap();
        assert!(check_folder_name("/etc").is_err());
        assert!(check_folder_name("a/../b").is_err());
        assert!(check_folder_name("").is_err());
        assert!(check_bucket_name("ab").is_err());
        assert!(check_bucket_name("Link").is_err());
        assert!(check_bucket_name("-link").is_err());
        assert!(check_bucket_name("link-buckets").is_ok());
    }

    #[test]
    fn settings_are_plain_and_never_secret() {
        let secret = vec!["pass".to_string(), "key".to_string()];
        let set = |k: &str| vec![(k.to_string(), "v".to_string())];
        assert!(check_settings(&set("host"), &secret).is_ok());
        assert!(check_settings(&set("chunk_size"), &secret).is_ok());
        for bad in ["pass", "key", "type", "--config", "Host", "a-b", ""] {
            assert!(check_settings(&set(bad), &secret).is_err(), "{bad}");
        }
    }

    /// A cloud of `kind` with a bucket for your files and one for an
    /// owner's, signing in as `lnk cloud` account `keys` when it has one.
    fn two_buckets(kind: Kind, keys: Option<&str>) -> Account {
        let bucket = |id: &str, name: &str| Bucket {
            id: id.into(),
            name: name.into(),
            encrypted: false,
        };
        Account {
            name: "qa".into(),
            kind,
            region: None,
            path: None,
            backend: None,
            buckets: [
                (PERSONAL.into(), bucket("a1b2c3d4e5f6", "lnk-bucket-mine")),
                (
                    "job:k3v9q2x7m1ab".into(),
                    bucket("b1c2d3e4f5a6", "lnk-bucket-owned"),
                ),
            ]
            .into(),
            account: keys.map(str::to_string),
            only_for: None,
        }
    }

    /// What deleting `account`'s buckets did, in turn, with `revoke`
    /// answering each revoke.
    fn deleting(
        account: &Account,
        revoke: impl Fn(&str) -> anyhow::Result<()>,
    ) -> (anyhow::Result<()>, Vec<String>) {
        let calls = std::cell::RefCell::new(Vec::new());
        let result = delete_buckets(
            account,
            &mut |cloud, bucket| {
                calls.borrow_mut().push(format!("revoke {cloud} {bucket}"));
                revoke(bucket)
            },
            &mut |cloud| {
                calls.borrow_mut().push(format!("delete {}", cloud.bucket));
                Ok(())
            },
        );
        (result, calls.into_inner())
    }

    /// Each bucket's key, made through its `lnk cloud` account, is revoked
    /// before the bucket is deleted, so no key or identity outlives it; one
    /// never given a key (its adapter answers done) deletes all the same.
    #[test]
    fn a_buckets_key_is_revoked_before_it_is_deleted() {
        for kind in [Kind::Gcp, Kind::Aws] {
            let (result, calls) = deleting(&two_buckets(kind, Some("qa-cloud")), |_| Ok(()));
            result.unwrap();
            assert_eq!(
                calls,
                [
                    "revoke qa-cloud lnk-bucket-owned",
                    "delete lnk-bucket-owned",
                    "revoke qa-cloud lnk-bucket-mine",
                    "delete lnk-bucket-mine",
                ]
            );
        }
    }

    /// A key that couldn't be revoked keeps its bucket, so it can be tried
    /// again; a cloud with no `lnk cloud` account made no key, so nothing
    /// is revoked.
    #[test]
    fn a_bucket_whose_key_stays_is_kept() {
        let account = two_buckets(Kind::Gcp, Some("qa-cloud"));
        let (result, calls) = deleting(&account, |bucket| match bucket {
            "lnk-bucket-mine" => anyhow::bail!("PERMISSION_DENIED"),
            _ => Ok(()),
        });
        let said = format!("{:#}", result.unwrap_err());
        assert!(
            said.contains("lnk-bucket-mine, so it wasn't deleted"),
            "{said}"
        );
        assert_eq!(
            calls,
            [
                "revoke qa-cloud lnk-bucket-owned",
                "delete lnk-bucket-owned",
                "revoke qa-cloud lnk-bucket-mine",
            ]
        );
        let (result, calls) = deleting(&two_buckets(Kind::S3, None), |_| unreachable!());
        result.unwrap();
        assert_eq!(calls, ["delete lnk-bucket-owned", "delete lnk-bucket-mine"]);
    }

    #[test]
    fn random() {
        let a = random_text(32).unwrap();
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_text(32).unwrap());
    }
}
