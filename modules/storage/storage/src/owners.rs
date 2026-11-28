//! An owner's buckets as its folder moves between machines: what a
//! machine says of them (`export`), puts in place (`adopt`) and lets go
//! of (`delete`), and, through `lnk cloud`, the buckets found by their
//! labels (`find`) and keys reaching only them (`keys`, `revoke`).
//! Hidden commands, for the module that keeps the owner, which names it
//! by its ID ([`crate::files::parse_owner`]); a person names it by kind
//! and name ([`resolve`]).

use std::collections::BTreeMap;

use crate::said;
use anyhow::bail;
use link_format::{WithinExt, sentence};
use serde::{Deserialize, Serialize};

use crate::clouds::{Account, Clouds, Kind, check_name};
use crate::connect::{self, Password};
use crate::files::{Owner, OwnerKind, check_owner, from_safe, safe};
use link_cloud_contract::{BUCKET_FOLDER, BUCKET_FOLDER_NAME, BUCKET_OWNER};
use link_secretive::Secret;
use link_storage_contract::{Found as Answered, OWNER, OwnerName};

use crate::manifest::Manifest;
use crate::rclone::{self, Rclone};

/// What travels with an owner's folder in its move (`export`), what
/// `adopt` reads, a bucket as a move carries it and a credential reaching only
/// one bucket: the shapes `types.toml` declares.
pub use crate::cli::{
    Adopting, BucketCredential as Credential, Exported, ExportedBucket as Carried,
};

/// A carried bucket's ID, bare, when it has one: an error unless its
/// resource is `bucket:<id>`. One found by its labels alone (`lnk
/// storage restore`) has no ID, and is given one.
pub fn carried_id(carried: &Carried) -> anyhow::Result<Option<&str>> {
    carried
        .resource
        .as_ref()
        .map(|resource| {
            crate::clouds::Bucket::id_of(resource.as_str()).ok_or_else(|| {
                anyhow::anyhow!(
                    "{:?} isn't a bucket's ID, bucket:<id>",
                    link_format::clean(resource.as_str())
                )
            })
        })
        .transpose()
}

/// A bucket `find` found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    /// The `lnk cloud` account it's in.
    pub cloud: String,
    pub bucket: String,
    pub place: String,
    /// The owner's ID and name, as its labels say.
    pub owner: Owner,
    pub name: String,
    /// Whose account it is (`lnk-owner`), when it says: none for one made
    /// while you weren't logged in. Not part of `lnk storage find`'s
    /// answer.
    #[serde(skip)]
    pub login: Option<String>,
    /// Its labels but those naming it ([`IDENTITY`]), as [`label`] adds
    /// them. Not part of `lnk storage find`'s answer.
    #[serde(skip)]
    pub labels: BTreeMap<String, String>,
}

/// The labels that say whose bucket it is, which [`find`] finds it by:
/// made with the bucket ([`connect::label`]), never by [`label`].
pub const IDENTITY: [&str; 3] = [BUCKET_FOLDER, BUCKET_FOLDER_NAME, BUCKET_OWNER];

/// A label [`label`] adds, `<name>=<value>`: a name of Link's (`lnk-`),
/// but none of [`IDENTITY`], and both as every cloud takes them (up to
/// 63 lowercase letters, digits, `-` and `_`).
pub fn parse_label(label: &str) -> anyhow::Result<(String, String)> {
    let (name, value) = label
        .split_once('=')
        .within(format!("{label:?}: a label is <name>=<value>"))?;
    let fits = |part: &str| {
        (1..=63).contains(&part.len())
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    };
    anyhow::ensure!(
        fits(name) && fits(value),
        "{label:?}: a label's name and value are up to 63 lowercase letters, digits, - and _"
    );
    anyhow::ensure!(
        name.starts_with("lnk-") && !IDENTITY.contains(&name),
        "{label:?}: only a label of Link's (lnk-) that doesn't say whose the bucket is"
    );
    Ok((name.to_string(), value.to_string()))
}

/// The ID of the owner `text` names, `<kind>:<name>`, as the module
/// that keeps that kind answers [`OWNER`], with its name; that module
/// takes its ID's id in place of a name. An error says why there's none.
pub fn resolve(text: &str) -> anyhow::Result<(Owner, String)> {
    let (kind, name) = text.split_once(':').within(format!(
        "{:?} names no owner: <kind>:<name>",
        link_format::clean(text)
    ))?;
    let kind: OwnerKind = kind.parse().map_err(|err| {
        anyhow::anyhow!(
            "{:?} isn't an owner's kind: {err}",
            link_format::clean(kind)
        )
    })?;
    anyhow::ensure!(!name.is_empty(), "{text:?} names no owner: <kind>:<name>");
    let asked = OwnerName {
        kind: kind.clone(),
        name: name.to_string(),
    };
    let (mut kept, mut unstarted) = (false, false);
    for answer in OWNER.ask(&asked) {
        match answer.reply {
            Ok(Answered::Here { owner, name: shown }) => {
                anyhow::ensure!(
                    owner.kind() == kind,
                    "the {} module answered {} for {kind}:{name}",
                    answer.module,
                    link_format::clean(&owner)
                );
                check_owner(&owner)?;
                return Ok((owner, shown));
            }
            Ok(Answered::Unstarted) => unstarted = true,
            Ok(Answered::None) => kept = true,
            Ok(Answered::Unkept) | Err(_) => {}
        }
    }
    let (kind, name) = (link_format::clean(kind.as_str()), link_format::clean(name));
    match (unstarted, kept) {
        (true, _) => bail!("{kind}:{name} has no ID yet: start it first"),
        (false, true) => bail!("there's no {kind} named {name} here"),
        (false, false) => bail!("no module here keeps owners of the kind {kind}"),
    }
}

/// Owner `owner`'s buckets here and its password: none when it has none.
pub fn export(owner: &Owner) -> anyhow::Result<Option<Exported>> {
    let id = owner.as_str();
    let clouds = Clouds::load()?;
    let buckets: BTreeMap<String, Carried> = clouds
        .clouds
        .iter()
        .filter_map(|a| {
            let bucket = a.buckets.get(id)?;
            Some((
                a.name.clone(),
                Carried {
                    name: bucket.name.clone(),
                    resource: Some(crate::cli::BucketId(bucket.resource())),
                },
            ))
        })
        .collect();
    if buckets.is_empty() {
        return Ok(None);
    }
    Ok(Some(Exported {
        password: connect::owner_password(id)?.map(Secret::from),
        buckets,
    }))
}

/// Puts owner `owner`'s buckets in place here: its encryption on the
/// buckets this machine already has for it, the buckets `adopting`
/// names in clouds of the same name here, and a cloud of its own for
/// each key (`<cloud>-<kind>-<id>`, for this owner only). Returns what
/// to tell the user; a cloud that fails is said and skipped.
pub fn adopt(
    rclone: &Rclone,
    owner: &Owner,
    adopting: &Adopting,
    found: Option<Vec<Found>>,
) -> anyhow::Result<Vec<String>> {
    check_owner(owner)?;
    let id = owner.as_str();
    let obscured = match (&adopting.plain, &adopting.password) {
        (Some(plain), _) => Some(Secret::from(rclone.obscure(plain.expose())?)),
        (None, Some(obscured)) => Some(obscured.duplicate()),
        (None, None) => None,
    };
    let Some(obscured) = obscured else {
        bail!("no password for its buckets: they can't be read here");
    };
    let _lock = crate::files::config_lock()?;
    let mut clouds = Clouds::load()?;
    let mut said = Vec::new();
    let mut added = Vec::new();
    // Buckets it had here: their encryption back.
    for account in clouds.clouds.clone() {
        let Some(cloud) = account.cloud(id).filter(|c| c.encrypted) else {
            continue;
        };
        let config = rclone::config_in(&crate::files::config_dir()?);
        let before = rclone::section(&config, &cloud.crypt_section())?;
        connect::encrypt_obscured(rclone, &cloud, obscured.expose())?;
        let failed = match rclone.password_opens(&cloud) {
            Ok(true) => {
                added.push(account.name.clone());
                continue;
            }
            Ok(false) => format!(
                "{}: the password doesn't open {}",
                account.name, cloud.bucket
            ),
            Err(err) => format!("{}: {err:#}", account.name),
        };
        said.push(failed);
        // What was there before stays.
        match before {
            Some(settings) => {
                let settings: Vec<(&str, &str)> = settings
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                rclone.set_section(&cloud.crypt_section(), &settings)?;
            }
            None => rclone.remove_section(&cloud.crypt_section())?,
        }
    }
    // Buckets it has in a cloud of the same name here. What names them
    // may be a box, so only a name Link gives an owner's bucket, in any
    // cloud, and in a cloud `lnk cloud` signs in to, only a bucket
    // labelled as this owner's: never another of yours.
    let mut labelled: Option<Vec<Found>> = found;
    for (name, carried) in &adopting.buckets {
        let Some(account) = clouds.clouds.iter_mut().find(|a| a.name == *name) else {
            continue;
        };
        let bucket = &carried.name;
        let carried_id = match carried_id(carried) {
            Ok(carried_id) => carried_id,
            Err(err) => {
                said.push(format!("{name}: {err:#}"));
                continue;
            }
        };
        if account.buckets.contains_key(id) || !account.serves(id) {
            continue;
        }
        if !owned_bucket(bucket, owner) {
            said.push(format!(
                "{name}: {} isn't named as an owner's bucket; not using it",
                link_format::clean(bucket)
            ));
            continue;
        }
        if let Some(cloud) = &account.account {
            let found = labelled.get_or_insert_with(|| match of_owner(owner) {
                Ok(found) => found,
                Err(err) => {
                    said.push(format!("{err:#}"));
                    Vec::new()
                }
            });
            if !found
                .iter()
                .any(|f| f.cloud == *cloud && f.bucket == *bucket && f.owner == *owner)
            {
                said.push(format!(
                    "{name}: {bucket} isn't labelled as this owner's bucket; not using it"
                ));
                continue;
            }
        }
        match connect::add_bucket(
            rclone,
            account,
            id,
            Some(bucket.clone()),
            carried_id,
            true,
            Password::Obscured(obscured.expose()),
            &mut no_prompt,
        ) {
            Ok(_) => added.push(name.clone()),
            Err(err) => said.push(format!("{name}: {err:#}")),
        }
    }
    // A credential for each bucket not reached already: a cloud of its own.
    for (name, credential) in &adopting.credentials {
        let reached = clouds.clouds.iter().any(|a| {
            a.buckets
                .get(id)
                .is_some_and(|b| b.name == credential.bucket)
        });
        if reached {
            continue;
        }
        let carried = adopting
            .buckets
            .values()
            .find(|b| b.name == credential.bucket)
            .map(carried_id)
            .transpose()
            .map(Option::flatten);
        let carried = match carried {
            Ok(carried) => carried,
            Err(err) => {
                said.push(format!("{name}: {err:#}"));
                continue;
            }
        };
        match keyed(
            rclone,
            &mut clouds,
            owner,
            name,
            credential,
            carried,
            obscured.expose(),
        ) {
            Ok(account) => added.push(account),
            Err(err) => said.push(format!("{name}: {err:#}")),
        }
    }
    clouds.save()?;
    if added.is_empty() {
        bail!(
            "none of its buckets is reachable here{}",
            match said.is_empty() {
                true => String::new(),
                false => format!(": {}", said.join("; ")),
            }
        );
    }
    for name in &added {
        if let Some(cloud) = clouds.get(name).and_then(|a| a.cloud(id)) {
            let mut manifest = Manifest::open(id)?;
            if let Err(err) = crate::ops::refresh(rclone, &mut manifest, &cloud) {
                said.push(format!("{name}: listing it: {err:#}"));
            }
            manifest.save()?;
            said.insert(0, format!("Backups to {}.", cloud.describe()));
        }
    }
    Ok(said)
}

/// A cloud for owner `owner` alone, from `credential`: named `<name>-<kind>-<id>`
/// ([`safe`]), its bucket encrypted with `obscured`. A new credential can take a while to work
/// everywhere in its cloud: it's tried for a minute.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off while a new credential becomes known"
)]
fn keyed(
    rclone: &Rclone,
    clouds: &mut Clouds,
    owner: &Owner,
    name: &str,
    credential: &Credential,
    carried: Option<&str>,
    obscured: &str,
) -> anyhow::Result<String> {
    let id = owner.as_str();
    let account_name = format!("{name}-{}", safe(id));
    check_name(&account_name)?;
    let kind = check_credential(credential)?;
    let settings: Vec<(&str, &str)> = credential
        .credential
        .iter()
        .map(|(k, v)| (k.as_str(), v.expose().as_str()))
        .collect();
    let config = rclone::config_in(&crate::files::config_dir()?);
    rclone::remove_section(&config, &format!("{account_name}-{}-crypt", safe(id)))?;
    rclone.set_section(&account_name, &settings)?;
    clouds.clouds.retain(|a| a.name != account_name);
    let mut account = Account {
        name: account_name.clone(),
        kind,
        // Not a secret, but kept among rclone's settings, beside its keys.
        region: credential
            .credential
            .get("region")
            .map(|r| r.expose_owned()),
        path: None,
        backend: None,
        buckets: BTreeMap::new(),
        account: None,
        only_for: Some(id.to_string()),
    };
    let mut tries = 0;
    loop {
        let result = connect::add_bucket(
            rclone,
            &mut account,
            id,
            Some(credential.bucket.clone()),
            carried,
            true,
            Password::Obscured(obscured),
            &mut no_prompt,
        );
        match result {
            Ok(_) => break,
            Err(err) if tries < 12 => {
                tries += 1;
                let _ = err;
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err(err) => {
                let _ = rclone.remove_section(&account_name);
                return Err(err);
            }
        }
    }
    clouds.clouds.push(account);
    Ok(account_name)
}

/// Whether `bucket` is a name Link makes for a bucket:
/// `lnk-bucket-<12 random>`.
fn made_bucket(bucket: &str) -> bool {
    let random = bucket.strip_prefix("lnk-bucket-").unwrap_or_default();
    random.len() == 12
        && random
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Whether `bucket` is a name Link gives owner `owner`'s bucket: one it
/// makes in a cloud of buckets, or its folder, `local-link/<kind>-<id>`
/// ([`safe`]), in one of folders.
fn owned_bucket(bucket: &str, owner: &Owner) -> bool {
    made_bucket(bucket) || bucket.strip_prefix("local-link/") == Some(safe(owner).as_str())
}

/// What `keys` makes, and nothing else: a credential may come from a box (what
/// a folder moving back from one carries), and rclone settings such as
/// `service_account_file`, `env_auth` or `endpoint` would sign in as you
/// and send the token wherever the box says.
fn check_credential(credential: &Credential) -> anyhow::Result<Kind> {
    anyhow::ensure!(
        made_bucket(&credential.bucket),
        "{:?} isn't a bucket Link made",
        credential.bucket
    );
    let (kind, allowed): (Kind, &[&str]) = match credential
        .credential
        .get("type")
        .map(|v| v.expose().as_str())
    {
        Some("s3") => (
            Kind::Aws,
            &[
                "type",
                "provider",
                "access_key_id",
                "secret_access_key",
                "region",
            ],
        ),
        Some("google cloud storage") => (
            Kind::Gcp,
            &["type", "service_account_credentials", "bucket_policy_only"],
        ),
        other => bail!("a credential of another kind ({other:?})"),
    };
    if let Some(setting) = credential
        .credential
        .keys()
        .find(|k| !allowed.contains(&k.as_str()))
    {
        bail!("a credential with a setting keys never have ({setting:?})");
    }
    if kind == Kind::Gcp {
        // A service account's key, whose token comes only from Google:
        // no other kind of credential, which could read files here.
        let credentials: serde_json::Value = serde_json::from_str(
            credential
                .credential
                .get("service_account_credentials")
                .map(|v| v.expose().as_str())
                .unwrap_or_default(),
        )
        .within("a credential with no service account in it")?;
        anyhow::ensure!(
            credentials["type"] == "service_account"
                && credentials
                    .get("token_uri")
                    .is_none_or(|uri| uri == "https://oauth2.googleapis.com/token"),
            "a key that isn't a service account's key from Google"
        );
    }
    if kind == Kind::Aws {
        anyhow::ensure!(
            credential
                .credential
                .get("provider")
                .map(|v| v.expose().as_str())
                == Some("AWS"),
            "a credential for another S3 provider"
        );
        let region = credential
            .credential
            .get("region")
            .map(|v| v.expose().as_str())
            .unwrap_or_default();
        anyhow::ensure!(
            !region.is_empty()
                && region
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{region:?} isn't a region"
        );
    }
    Ok(kind)
}

/// Deletes what is kept of owner `owner` here, which moved away: a
/// cloud only for it goes, keys and all; in the others, its encryption
/// goes (the password is its, and travels with it) and its bucket stays
/// known, so it's found again when the owner comes back.
pub fn delete(owner: &Owner) -> anyhow::Result<()> {
    check_owner(owner)?;
    let id = owner.as_str();
    let _lock = crate::files::config_lock()?;
    let mut clouds = Clouds::load()?;
    let config = rclone::config_in(&crate::files::config_dir()?);
    for account in &clouds.clouds {
        if let Some(cloud) = account.cloud(id) {
            rclone::remove_section(&config, &cloud.crypt_section())?;
        }
        if account.only_for.as_deref() == Some(id) {
            rclone::remove_section(&config, &account.name)?;
        }
    }
    clouds.clouds.retain(|a| a.only_for.as_deref() != Some(id));
    clouds.save()
}

/// Your `lnk cloud` accounts' names (`lnk cloud list --json`); none
/// without the cloud module.
fn cloud_accounts() -> Vec<String> {
    link_core::lnk()
        .args(["cloud", "list", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Vec<serde_json::Value>>(&o.stdout).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|a| a["name"].as_str().map(str::to_string))
        .collect()
}

/// The buckets labelled with `labels` in each of your `lnk cloud`
/// accounts; a cloud that can't say is said on stderr.
pub fn find(labels: &BTreeMap<String, String>) -> Vec<Found> {
    let request = serde_json::json!({"labels": labels});
    // Each account at once: a cloud's CLI and its API each take a while.
    let replies: Vec<_> = std::thread::scope(|scope| {
        let asks: Vec<_> = cloud_accounts()
            .into_iter()
            .map(|cloud| {
                let request = &request;
                scope.spawn(move || {
                    let reply = connect::call_cloud(&cloud, "buckets", request);
                    (cloud, reply)
                })
            })
            .collect();
        asks.into_iter().filter_map(|ask| ask.join().ok()).collect()
    });
    let mut found = Vec::new();
    for (cloud, reply) in replies {
        match reply {
            Ok(reply) => {
                for bucket in reply.buckets.unwrap_or_default() {
                    // As labelled: a cloud answering more than was asked
                    // is held to it.
                    if labels.iter().any(|(k, v)| bucket.labels.get(k) != Some(v)) {
                        continue;
                    }
                    let (Some(owner), Some(name)) = (
                        bucket.labels.get(BUCKET_FOLDER).and_then(|f| from_safe(f)),
                        bucket.labels.get(BUCKET_FOLDER_NAME).cloned(),
                    ) else {
                        continue;
                    };
                    found.push(Found {
                        cloud: cloud.clone(),
                        bucket: bucket.bucket,
                        place: bucket.place,
                        owner,
                        name,
                        login: bucket.labels.get(BUCKET_OWNER).cloned(),
                        labels: bucket
                            .labels
                            .into_iter()
                            .filter(|(k, _)| !IDENTITY.contains(&k.as_str()))
                            .collect(),
                    });
                }
            }
            Err(err) => said::warn(
                sentence!("Looking for buckets in {} didn't work.", cloud.as_str()),
                &err,
            ),
        }
    }
    found
}

/// Every owner's bucket in your `lnk cloud` accounts, by its labels:
/// those labelled as yours (`lnk-owner`), when you're logged in.
pub fn find_all() -> Vec<Found> {
    let labels: BTreeMap<String, String> = connect::owner()
        .map(|login| (BUCKET_OWNER.to_string(), login))
        .into_iter()
        .collect();
    find(&labels)
}

/// The buckets of the owner `text` names, `<kind>:<name>`, in your `lnk
/// cloud` accounts (those labelled as yours, when you're logged in): by
/// its ID's id in place of its name when that's one some bucket is
/// labelled with, else by its name. The owner may not be on this
/// machine, so its labels are all that names it.
pub fn find_named(text: &str) -> anyhow::Result<Vec<Found>> {
    let (kind, name) = text.split_once(':').within(format!(
        "{:?} names no owner: <kind>:<name>",
        link_format::clean(text)
    ))?;
    let kind: OwnerKind = kind.parse().map_err(|err| {
        anyhow::anyhow!(
            "{:?} isn't an owner's kind: {err}",
            link_format::clean(kind)
        )
    })?;
    let login = connect::owner();
    let with = |label: &str, value: String| {
        let mut labels = BTreeMap::from([(label.to_string(), value)]);
        if let Some(login) = &login {
            labels.insert(BUCKET_OWNER.to_string(), login.clone());
        }
        find(&labels)
    };
    if let Ok(owner) = crate::files::parse_owner(text) {
        let found: Vec<Found> = with(BUCKET_FOLDER, safe(&owner))
            .into_iter()
            .filter(|f| f.owner == owner)
            .collect();
        if !found.is_empty() {
            return Ok(found);
        }
    }
    link_machine::machine::check_name(name)?;
    Ok(with(BUCKET_FOLDER_NAME, name.to_string())
        .into_iter()
        .filter(|f| f.owner.kind() == kind && f.name == name)
        .collect())
}

/// Owner `owner`'s buckets, found by its label: never one labelled as
/// someone else's (`lnk-owner`, when you're logged in), while one made
/// before you logged in, with no account, still is. Two in one cloud are
/// refused, both named: one of them isn't the owner's.
fn of_owner(owner: &Owner) -> anyhow::Result<Vec<Found>> {
    let labels = BTreeMap::from([(BUCKET_FOLDER.to_string(), safe(owner))]);
    let login = connect::owner();
    let found = find(&labels)
        .into_iter()
        .filter(|f| f.owner == *owner && is_ours(f, login.as_deref()))
        .collect();
    one_per_cloud(found)
}

/// Whether `found` isn't labelled as another login's than `login`'s.
fn is_ours(found: &Found, login: Option<&str>) -> bool {
    match (&found.login, login) {
        (Some(theirs), Some(ours)) => theirs == ours,
        _ => true,
    }
}

/// `found`, unless two of its buckets are in one cloud: an owner has one
/// bucket in each, so one of them was made to take its place.
pub fn one_per_cloud(found: Vec<Found>) -> anyhow::Result<Vec<Found>> {
    for (i, a) in found.iter().enumerate() {
        if let Some(b) = found[i + 1..]
            .iter()
            .find(|b| b.cloud == a.cloud && b.owner == a.owner)
        {
            bail!(
                "{} has two buckets labelled as {}'s, {} and {}: one isn't. Delete \
                 the one Link didn't make (lnk cloud shows the account), then try again.",
                a.cloud,
                a.owner,
                link_format::clean(&a.bucket),
                link_format::clean(&b.bucket)
            );
        }
    }
    Ok(found)
}

/// A credential reaching only each of owner `owner`'s buckets, by cloud; a
/// cloud that can't make one (the account may not make users or service
/// accounts) is said on stderr and left out.
pub fn keys(owner: &Owner) -> anyhow::Result<BTreeMap<String, Credential>> {
    check_owner(owner)?;
    keys_of(of_owner(owner)?)
}

/// [`keys`], for the buckets `found` (an owner's, as [`find`] gave them).
pub fn keys_of(found: Vec<Found>) -> anyhow::Result<BTreeMap<String, Credential>> {
    let mut keys = BTreeMap::new();
    for found in found {
        let request = serde_json::json!({"bucket": found.bucket});
        match connect::call_cloud(&found.cloud, "bucket-credential", &request) {
            Ok(reply) => {
                let credential = Some(reply.credential)
                    .filter(|credential| !credential.is_empty())
                    .within("the adapter made no credential")?;
                keys.insert(
                    found.cloud.clone(),
                    Credential {
                        bucket: found.bucket,
                        credential,
                    },
                );
            }
            Err(err) => said::warn(
                sentence!(
                    "There is no credential for {} in {}.",
                    found.bucket.as_str(),
                    found.cloud.as_str()
                ),
                &err,
            ),
        }
    }
    Ok(keys)
}

/// `lnk storage label`: `labels` added to owner `owner`'s buckets,
/// through `lnk cloud` (each its account's), so whoever finds them later
/// reads them (`lnk storage restore`): set with your accounts, out of
/// the owner's reach. Where they went, by cloud; a bucket that couldn't
/// be labelled is said on stderr and left out.
pub fn label(owner: &Owner, labels: &[String]) -> anyhow::Result<BTreeMap<String, String>> {
    check_owner(owner)?;
    let labels: BTreeMap<String, String> = labels
        .iter()
        .map(|l| parse_label(l))
        .collect::<anyhow::Result<_>>()?;
    anyhow::ensure!(!labels.is_empty(), "no label given: <name>=<value>");
    let mut labelled = BTreeMap::new();
    for found in of_owner(owner)? {
        let request = serde_json::json!({"bucket": found.bucket, "labels": labels});
        match connect::call_cloud(&found.cloud, "label", &request) {
            Ok(_) => {
                labelled.insert(found.cloud, found.bucket);
            }
            Err(err) => said::warn(
                sentence!(
                    "{} couldn't be labelled in {}.",
                    found.bucket.as_str(),
                    found.cloud.as_str()
                ),
                &err,
            ),
        }
    }
    Ok(labelled)
}

/// The keys `keys` made for owner `owner`'s buckets, gone: each tried,
/// and an error naming each that stays, so what asked (a move, a box
/// deleted) says the key may still work.
pub fn revoke(owner: &Owner) -> anyhow::Result<()> {
    check_owner(owner)?;
    revoke_each(of_owner(owner)?, |found| {
        connect::revoke_key(&found.cloud, &found.bucket)
    })
}

/// [`revoke`] of the buckets `found`, each with `revoke_key`.
fn revoke_each(
    found: Vec<Found>,
    revoke_key: impl Fn(&Found) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut stay = Vec::new();
    for found in found {
        if let Err(err) = revoke_key(&found) {
            said::warn(
                sentence!(
                    "The key for {} is still there in {}.",
                    found.bucket.as_str(),
                    found.cloud.as_str()
                ),
                &err,
            );
            stay.push(format!(
                "{} in {}",
                link_format::clean(&found.bucket),
                link_format::clean(&found.cloud)
            ));
        }
    }
    anyhow::ensure!(
        stay.is_empty(),
        "the key for {} is still there",
        stay.join(", ")
    );
    Ok(())
}

fn no_prompt(asked: connect::Asked) -> anyhow::Result<String> {
    bail!("{asked:?}: nothing to ask here")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bucket made before you logged in, with no account, is still
    /// the owner's; one labelled as another login's never is.
    #[test]
    fn a_bucket_labelled_as_anothers_is_never_the_owners() {
        let found = |login: Option<&str>| Found {
            cloud: "aws".into(),
            bucket: "lnk-bucket-k3v9q2x7m1ab".into(),
            place: "eu-west-1".into(),
            owner: "job:a1b2c3d4e5f6".parse().unwrap(),
            name: "main".into(),
            login: login.map(str::to_string),
            labels: BTreeMap::new(),
        };
        assert!(is_ours(&found(Some("github-1")), Some("github-1")));
        assert!(is_ours(&found(None), Some("github-1")));
        assert!(!is_ours(&found(Some("github-2")), Some("github-1")));
        assert!(is_ours(&found(Some("github-2")), None));
    }

    /// A key a cloud won't revoke fails the revoke, naming its bucket, so
    /// a move or a box's removal says it may still work; the rest are
    /// each tried.
    #[test]
    fn a_key_that_stays_fails_the_revoke() {
        let found = |cloud: &str| Found {
            cloud: cloud.into(),
            bucket: format!("lnk-bucket-{cloud}"),
            place: "eu-west-1".into(),
            owner: "job:a1b2c3d4e5f6".parse().unwrap(),
            name: "main".into(),
            login: None,
            labels: BTreeMap::new(),
        };
        let tried = std::cell::RefCell::new(Vec::new());
        let refused = revoke_each(vec![found("aws"), found("gcp")], |found| {
            tried.borrow_mut().push(found.cloud.clone());
            match found.cloud.as_str() {
                "aws" => anyhow::bail!("access denied"),
                _ => Ok(()),
            }
        })
        .unwrap_err();
        assert_eq!(*tried.borrow(), ["aws", "gcp"]);
        let refused = format!("{refused:#}");
        assert!(refused.contains("lnk-bucket-aws in aws"), "{refused}");
        assert!(!refused.contains("gcp"), "{refused}");
        revoke_each(vec![found("gcp")], |_| Ok(())).unwrap();
    }

    fn credential(bucket: &str, settings: &[(&str, &str)]) -> Credential {
        Credential {
            bucket: bucket.into(),
            credential: settings
                .iter()
                .map(|(k, v)| (k.to_string(), (*v).to_string().into()))
                .collect(),
        }
    }

    const GOOGLE: &str =
        r#"{"type":"service_account","token_uri":"https://oauth2.googleapis.com/token"}"#;

    /// The credentials the adapters make pass; a box can't slip in a setting
    /// that signs in as you (a credentials file, the environment) or
    /// sends the token elsewhere, nor name a bucket Link didn't make.
    #[test]
    fn only_credentials_as_the_adapters_make_them() {
        let aws = [
            ("type", "s3"),
            ("provider", "AWS"),
            ("access_key_id", "AKIA"),
            ("secret_access_key", "secret"),
            ("region", "us-east-1"),
        ];
        let gcp = [
            ("type", "google cloud storage"),
            ("service_account_credentials", GOOGLE),
            ("bucket_policy_only", "true"),
        ];
        assert!(
            check_credential(&credential("lnk-bucket-abc123def456", &aws)).unwrap() == Kind::Aws
        );
        assert!(
            check_credential(&credential("lnk-bucket-abc123def456", &gcp)).unwrap() == Kind::Gcp
        );

        let refused = |bucket: &str, settings: &[(&str, &str)]| {
            assert!(
                check_credential(&credential(bucket, settings)).is_err(),
                "{settings:?}"
            );
        };
        refused("my-photos", &aws);
        refused("lnk-bucket-abc", &aws);
        for extra in [
            ("endpoint", "https://attacker.example"),
            ("env_auth", "true"),
            (
                "service_account_file",
                "/home/you/.lnk/safe/gcp/credentials/gcp.json",
            ),
        ] {
            refused("lnk-bucket-abc123def456", &[&aws[..], &[extra]].concat());
            refused("lnk-bucket-abc123def456", &[&gcp[..], &[extra]].concat());
        }
        refused(
            "lnk-bucket-abc123def456",
            &[&aws[..1], &[("provider", "Minio")], &aws[2..]].concat(),
        );
        refused(
            "lnk-bucket-abc123def456",
            &[&aws[..4], &[("region", "us-east-1 --x")]].concat(),
        );
        for credentials in [
            r#"{"type":"external_account","credential_source":{"file":"/etc/passwd"}}"#,
            r#"{"type":"service_account","token_uri":"https://attacker.example/token"}"#,
            "not json",
        ] {
            refused(
                "lnk-bucket-abc123def456",
                &[
                    ("type", "google cloud storage"),
                    ("service_account_credentials", credentials),
                ],
            );
        }
    }

    #[test]
    fn a_label_added_is_links_and_never_says_whose_a_bucket_is() {
        assert_eq!(
            parse_label("lnk-layout=2").unwrap(),
            ("lnk-layout".to_string(), "2".to_string())
        );
        for refused in [
            "lnk-layout",
            "layout=2",
            "lnk-folder=job-a1b2c3d4e5f6",
            "lnk-folder-name=main",
            "lnk-owner=acct-1",
            "lnk-Layout=2",
            "lnk-layout=",
            "lnk-layout=two words",
        ] {
            assert!(parse_label(refused).is_err(), "{refused}");
        }
    }
}
