//! Agents' buckets on Google Cloud: bucket labels, and a key reaching only
//! one bucket, a service account of its own (`lnk-<...>`) given
//! `roles/storage.objectAdmin` on that bucket and nothing else.

use link_format::WithinExt;
use link_secretive::Secret;
use std::collections::BTreeMap;

use link_cloud_contract::{FoundBucket, check_bucket, check_label};

use crate::cli::Gcp;

/// The role a bucket's service account has on it.
const ROLE: &str = "roles/storage.objectAdmin";

/// The key file gcloud wrote, as one line of JSON: gcloud indents it over
/// many lines, and a storage setting is one line (rclone's config file
/// holds one setting a line).
fn one_line(key: &str) -> anyhow::Result<String> {
    let key: serde_json::Value =
        serde_json::from_str(key).within("the key gcloud made isn't JSON")?;
    anyhow::ensure!(key.is_object(), "the key gcloud made isn't a JSON object");
    Ok(serde_json::to_string(&key)?)
}

/// The service account holding bucket `bucket`'s key: `lnk-<its random
/// part>` for Link's own buckets, else `lnk-b-<a hash of its name>`.
pub fn account_of(bucket: &str) -> String {
    match bucket.strip_prefix("lnk-bucket-") {
        Some(rest)
            if (2..=26).contains(&rest.len())
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) =>
        {
            format!("lnk-{rest}")
        }
        _ => format!("lnk-b-{}", crate::cli::short_hash(bucket)),
    }
}

impl Gcp {
    /// Bucket `bucket`'s service account's email.
    fn account_email(&self, bucket: &str) -> anyhow::Result<String> {
        Ok(format!(
            "{}@{}.iam.gserviceaccount.com",
            account_of(bucket),
            self.project
        ))
    }

    /// `labels` added to bucket `bucket`'s, the others kept.
    pub fn label(&self, bucket: &str, labels: &BTreeMap<String, String>) -> anyhow::Result<()> {
        check_bucket(bucket)?;
        for (key, value) in labels {
            check_label(key)?;
            check_label(value)?;
        }
        let update: Vec<String> = labels.iter().map(|(k, v)| format!("{k}={v}")).collect();
        self.cli.run(&[
            "storage",
            "buckets",
            "update",
            &format!("gs://{bucket}"),
            &format!("--update-labels={}", update.join(",")),
        ])?;
        Ok(())
    }

    /// Link's buckets (`lnk-bucket-*`) in the project with every one of
    /// `labels`.
    pub fn buckets(&self, labels: &BTreeMap<String, String>) -> anyhow::Result<Vec<FoundBucket>> {
        for (key, value) in labels {
            check_label(key)?;
            check_label(value)?;
        }
        let listed = self
            .cli
            .json(&["storage", "buckets", "list", "--format=json"])?;
        Ok(listed
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| {
                let bucket = b["name"].as_str()?;
                if !bucket.starts_with("lnk-bucket-") || check_bucket(bucket).is_err() {
                    return None;
                }
                let found: BTreeMap<String, String> = b["labels"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect();
                labels
                    .iter()
                    .all(|(k, v)| found.get(k) == Some(v))
                    .then(|| FoundBucket {
                        bucket: bucket.to_string(),
                        place: b["location"].as_str().unwrap_or_default().to_lowercase(),
                        labels: found,
                    })
            })
            .collect())
    }

    /// A credential reaching only bucket `bucket`: its service account (made the
    /// first time) with the object role on it, and a new key, the
    /// account's others deleted.
    pub fn bucket_credential(&self, bucket: &str) -> anyhow::Result<BTreeMap<String, Secret>> {
        check_bucket(bucket)?;
        let email = self.account_email(bucket)?;
        match self.cli.run(&[
            "iam",
            "service-accounts",
            "create",
            &account_of(bucket),
            &format!("--display-name=Link: bucket {bucket}"),
        ]) {
            Ok(_) => {}
            Err(err) if err.to_string().contains("already exists") => {}
            Err(err) => return Err(err.context(format!("making the service account {email}"))),
        }
        self.cli.grant(&[
            "storage",
            "buckets",
            "add-iam-policy-binding",
            &format!("gs://{bucket}"),
            &format!("--member=serviceAccount:{email}"),
            &format!("--role={ROLE}"),
        ])?;
        self.delete_keys(&email)?;
        let file = crate::cli::private_file("key.json", "")?;
        let path = file.path().display().to_string();
        self.cli.run(&[
            "iam",
            "service-accounts",
            "keys",
            "create",
            &path,
            &format!("--iam-account={email}"),
        ])?;
        let credentials =
            one_line(&std::fs::read_to_string(file.path()).within("reading the key gcloud made")?)?;
        Ok(BTreeMap::from([
            ("type".into(), "google cloud storage".to_string().into()),
            ("service_account_credentials".into(), credentials.into()),
            ("bucket_policy_only".into(), "true".to_string().into()),
        ]))
    }

    /// Bucket `bucket`'s key gone: its role on the bucket and its service
    /// account, keys and all.
    pub fn revoke_key(&self, bucket: &str) -> anyhow::Result<()> {
        check_bucket(bucket)?;
        let email = self.account_email(bucket)?;
        let gone = |err: &anyhow::Error| {
            let text = err.to_string();
            text.contains("NOT_FOUND")
                || text.contains("not found")
                || text.contains("does not exist")
        };
        for args in [
            vec![
                "storage".to_string(),
                "buckets".into(),
                "remove-iam-policy-binding".into(),
                format!("gs://{bucket}"),
                format!("--member=serviceAccount:{email}"),
                format!("--role={ROLE}"),
            ],
            vec![
                "iam".into(),
                "service-accounts".into(),
                "delete".into(),
                email.clone(),
            ],
        ] {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            match self.cli.run(&args) {
                Err(err) if !gone(&err) => return Err(err),
                _ => {}
            }
        }
        Ok(())
    }

    /// Deletes the keys made for service account `email`.
    fn delete_keys(&self, email: &str) -> anyhow::Result<()> {
        let keys = self.cli.json(&[
            "iam",
            "service-accounts",
            "keys",
            "list",
            &format!("--iam-account={email}"),
            "--managed-by=user",
            "--format=json",
        ])?;
        for name in keys
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|k| k["name"].as_str())
        {
            let id = name.rsplit('/').next().unwrap_or(name);
            self.cli.run(&[
                "iam",
                "service-accounts",
                "keys",
                "delete",
                id,
                &format!("--iam-account={email}"),
            ])?;
        }
        Ok(())
    }
}

impl Gcp {
    /// GCS signs the generation precondition along with the PUT method.
    pub fn upload_once(
        &self,
        bucket: &str,
        object: &str,
        expires: link_cloud_contract::Deadline,
    ) -> anyhow::Result<link_cloud_contract::upload::Upload> {
        check_bucket(bucket)?;
        link_cloud_contract::upload::check_object(object)?;
        link_cloud_contract::deadline::check(expires)
            .map_err(|_| anyhow::anyhow!("an upload lasts at most seven days"))?;
        let object = format!("gs://{bucket}/{object}");
        let duration = format!("--duration={}s", expires.get().as_secs());
        // The credential override authenticates gcloud, but sign-url needs
        // the service-account file explicitly to select its local signer.
        // Only the connected account may choose it, never the shell.
        let signing_file = self
            .cli
            .env
            .get("CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE")
            .map(|path| path.expose_map(|path| format!("--private-key-file={path}")));
        let mut args = vec![
            "storage",
            "sign-url",
            &object,
            &duration,
            "--http-verb=PUT",
            "--headers=x-goog-if-generation-match=0",
            "--format=json",
        ];
        // The file's path, not its contents: gcloud reads the key itself.
        if let Some(file) = &signing_file {
            args.push(file.expose());
        }
        let signed = self.cli.json(&args).within(
            "signing a result upload: Google Cloud needs a service account that can sign URLs",
        )?;
        let url = signed[0]["signed_url"]
            .as_str()
            .within("gcloud returned no signed URL")?;
        Ok(link_cloud_contract::upload::Upload {
            url: url.to_string().into(),
            headers: BTreeMap::from([("x-goog-if-generation-match".into(), "0".into())]),
        })
    }

    pub fn object_read(&self, bucket: &str, object: &str) -> anyhow::Result<Option<String>> {
        check_bucket(bucket)?;
        link_cloud_contract::upload::check_object(object)?;
        match self
            .cli
            .run(&["storage", "cat", &format!("gs://{bucket}/{object}")])
        {
            Ok(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            Err(err) if missing(&err.to_string()) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

/// Whether `gcloud storage cat` failed with `text` because the object
/// isn't there, in either of the ways gcloud has said it.
fn missing(text: &str) -> bool {
    text.contains("No URLs matched")
        || text.contains("matched no objects or files")
        || text.contains("404")
}

#[cfg(test)]
mod tests {
    use super::account_of;

    /// An object not there yet reads as none, as gcloud says it.
    #[test]
    fn an_object_not_there_is_none() {
        assert!(super::missing(
            "ERROR: (gcloud.storage.cat) The following URLs matched no objects or files:\
             gs://b/qa/1/record.json"
        ));
        assert!(super::missing("No URLs matched: gs://b/x"));
        assert!(!super::missing("AccessDenied"));
    }

    #[test]
    fn a_bucket_names_its_service_account() {
        assert_eq!(account_of("lnk-bucket-a1b2c3d4e5f6"), "lnk-a1b2c3d4e5f6");
        let other = account_of("my.bucket");
        assert!(other.starts_with("lnk-b-") && other.len() <= 30, "{other}");
    }
}
