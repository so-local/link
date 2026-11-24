//! Agents' buckets on AWS: S3 bucket tags (Link's labels), and a key
//! reaching only one bucket: an IAM user of its own, named as the bucket
//! is under the path `/lnk/`, with a policy for that bucket and nothing
//! else, and a permissions boundary ([`BOUNDARY`]) that holds any policy
//! it's given to the bucket named as the user is.

use link_format::WithinExt;
use std::collections::BTreeMap;

use anyhow::ensure;
use link_cloud_contract::{FoundBucket, check_bucket, check_label};

use crate::cli::Aws;

/// The inline policy's name on a bucket's user.
const POLICY: &str = "lnk-storage";

/// The IAM user holding bucket `bucket`'s key: named as the bucket, so
/// the boundary (`${aws:username}`) names the bucket too.
pub fn user_of(bucket: &str) -> String {
    bucket.to_string()
}

/// Where bucket credentials' users are: what a policy granting their making
/// names (`user/lnk/*`).
const USERS: &str = "/lnk/";

/// The user an older `lnk-aws` made for bucket `bucket`, with no
/// boundary: deleted when its bucket's credential is made again or revoked.
fn old_user_of(bucket: &str) -> String {
    format!("lnk-{bucket}")
}

/// The managed policy every bucket credential's user has as its permissions
/// boundary, made in the account the first time: whatever policy a user
/// is given, it reaches no more than the bucket named as the user is.
pub const BOUNDARY: &str = "lnk-bucket-boundary";

/// What [`BOUNDARY`] allows: the actions a bucket credential has, on the bucket
/// named as the user.
pub fn boundary_document() -> serde_json::Value {
    policy_for("${aws:username}")
}

/// The actions a bucket credential has, on bucket `bucket` (a name, or a policy
/// variable).
fn policy_for(bucket: &str) -> serde_json::Value {
    let arn = format!("arn:aws:s3:::{bucket}");
    serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": ["s3:ListBucket", "s3:GetBucketLocation", "s3:ListBucketMultipartUploads"],
                "Resource": arn,
            },
            {
                "Effect": "Allow",
                "Action": [
                    "s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:RestoreObject",
                    "s3:AbortMultipartUpload", "s3:ListMultipartUploadParts",
                ],
                "Resource": format!("{arn}/*"),
            },
        ],
    })
}

impl Aws {
    /// Where bucket `bucket` is (`us-east-1` when S3 says none).
    fn bucket_region(&self, bucket: &str) -> anyhow::Result<String> {
        let location = self
            .cli
            .json(&["s3api", "get-bucket-location", "--bucket", bucket])?;
        Ok(location["LocationConstraint"]
            .as_str()
            .filter(|r| !r.is_empty())
            .unwrap_or("us-east-1")
            .to_string())
    }

    /// Bucket `bucket`'s tags; none when it has none.
    fn bucket_tags(&self, bucket: &str, region: &str) -> anyhow::Result<BTreeMap<String, String>> {
        let tags = match self.cli.json(&[
            "s3api",
            "get-bucket-tagging",
            "--bucket",
            bucket,
            "--region",
            region,
        ]) {
            Ok(tags) => tags,
            Err(err) if err.to_string().contains("NoSuchTagSet") => return Ok(BTreeMap::new()),
            Err(err) => return Err(err),
        };
        Ok(tags["TagSet"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t["Key"].as_str()?.into(), t["Value"].as_str()?.into())))
            .collect())
    }

    /// `labels` added to bucket `bucket`'s tags, the others kept.
    pub fn label(&self, bucket: &str, labels: &BTreeMap<String, String>) -> anyhow::Result<()> {
        check_bucket(bucket)?;
        for (key, value) in labels {
            check_label(key)?;
            check_label(value)?;
        }
        let region = self.bucket_region(bucket)?;
        let mut tags = self.bucket_tags(bucket, &region)?;
        tags.extend(labels.clone());
        let set: Vec<_> = tags
            .iter()
            .map(|(k, v)| serde_json::json!({"Key": k, "Value": v}))
            .collect();
        self.cli.run(&[
            "s3api",
            "put-bucket-tagging",
            "--bucket",
            bucket,
            "--region",
            &region,
            "--tagging",
            &serde_json::json!({ "TagSet": set }).to_string(),
        ])?;
        Ok(())
    }

    /// Link's buckets (`lnk-bucket-*`) with every one of `labels`.
    pub fn buckets(&self, labels: &BTreeMap<String, String>) -> anyhow::Result<Vec<FoundBucket>> {
        for (key, value) in labels {
            check_label(key)?;
            check_label(value)?;
        }
        // With a prefix, S3 says each bucket's region too (an older aws
        // CLI without `--prefix` lists them all, and each is asked).
        let listed = self
            .cli
            .json(&["s3api", "list-buckets", "--prefix", "lnk-bucket-"])
            .or_else(|_| self.cli.json(&["s3api", "list-buckets"]))?;
        let buckets: Vec<(&str, Option<&str>)> = listed["Buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| Some((b["Name"].as_str()?, b["BucketRegion"].as_str())))
            .filter(|(name, _)| name.starts_with("lnk-bucket-") && check_bucket(name).is_ok())
            .collect();
        // Buckets' tags eight at a time: one aws process each.
        let tagged = buckets.chunks(8).flat_map(|some| {
            std::thread::scope(|scope| {
                let asks: Vec<_> = some
                    .iter()
                    .map(|&(bucket, region)| {
                        scope.spawn(move || {
                            let region = match region.filter(|r| !r.is_empty()) {
                                Some(region) => region.to_string(),
                                None => self.bucket_region(bucket)?,
                            };
                            let labels = self.bucket_tags(bucket, &region)?;
                            Ok(FoundBucket {
                                bucket: bucket.to_string(),
                                place: region,
                                labels,
                            })
                        })
                    })
                    .collect();
                asks.into_iter()
                    .map(|ask| {
                        ask.join()
                            .unwrap_or_else(|_| Err(anyhow::anyhow!("asking for a bucket failed")))
                    })
                    .collect::<Vec<anyhow::Result<FoundBucket>>>()
            })
        });
        let mut found = Vec::new();
        for bucket in tagged {
            let bucket = bucket?;
            if labels.iter().all(|(k, v)| bucket.labels.get(k) == Some(v)) {
                found.push(bucket);
            }
        }
        Ok(found)
    }

    /// A credential reaching only bucket `bucket`: its user (made the first
    /// time), its policy, and a new access key, the user's others deleted.
    pub fn bucket_credential(
        &self,
        bucket: &str,
    ) -> anyhow::Result<BTreeMap<String, link_secretive::Secret>> {
        check_bucket(bucket)?;
        let region = self.bucket_region(bucket)?;
        let user = user_of(bucket);
        let boundary = self.boundary()?;
        // Bounded from its making on: a policy that grants making these
        // users can require the boundary (`iam:PermissionsBoundary`).
        match self.cli.run(&[
            "iam",
            "create-user",
            "--path",
            USERS,
            "--user-name",
            &user,
            "--permissions-boundary",
            &boundary,
        ]) {
            Ok(_) => {}
            Err(err) if err.to_string().contains("EntityAlreadyExists") => {
                self.cli.run(&[
                    "iam",
                    "put-user-permissions-boundary",
                    "--user-name",
                    &user,
                    "--permissions-boundary",
                    &boundary,
                ])?;
            }
            Err(err) => return Err(err.context(format!("making the IAM user {user}"))),
        }
        let policy = policy_for(bucket);
        self.cli.run(&[
            "iam",
            "put-user-policy",
            "--user-name",
            &user,
            "--policy-name",
            POLICY,
            "--policy-document",
            &policy.to_string(),
        ])?;
        self.delete_keys(&user)?;
        // The unbounded user an older lnk-aws made for it goes.
        self.delete_user(&old_user_of(bucket))?;
        let made = self
            .cli
            .json(&["iam", "create-access-key", "--user-name", &user])?;
        let field = |name: &str| {
            made["AccessKey"][name]
                .as_str()
                .map(str::to_string)
                .within(format!("aws made a key without {name}"))
        };
        Ok(BTreeMap::from([
            ("type".into(), "s3".to_string().into()),
            ("provider".into(), "AWS".to_string().into()),
            ("access_key_id".into(), field("AccessKeyId")?.into()),
            ("secret_access_key".into(), field("SecretAccessKey")?.into()),
            ("region".into(), region.into()),
        ]))
    }

    /// Bucket `bucket`'s key gone: its user's keys, policy and the user,
    /// and the user an older lnk-aws made for it.
    pub fn revoke_key(&self, bucket: &str) -> anyhow::Result<()> {
        check_bucket(bucket)?;
        self.delete_user(&user_of(bucket))?;
        self.delete_user(&old_user_of(bucket))
    }

    /// The ARN of [`BOUNDARY`] in this account, made the first time. One
    /// there already must be Link's, word for word: one someone else made
    /// under its name could allow anything, so no key is made under it.
    fn boundary(&self) -> anyhow::Result<String> {
        let me = self
            .cli
            .json(&["sts", "get-caller-identity", "--output", "json"])?;
        let account = me["Account"]
            .as_str()
            .filter(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()))
            .within("AWS didn't say which account this is")?;
        let arn = format!("arn:aws:iam::{account}:policy/{BOUNDARY}");
        let wanted = boundary_document();
        let found = match self.cli.json(&["iam", "get-policy", "--policy-arn", &arn]) {
            Ok(found) => found,
            Err(err) if err.to_string().contains("NoSuchEntity") => {
                match self.cli.run(&[
                    "iam",
                    "create-policy",
                    "--policy-name",
                    BOUNDARY,
                    "--description",
                    "What a bucket credential Link made can reach, whatever its policy: the bucket named as its user",
                    "--policy-document",
                    &wanted.to_string(),
                ]) {
                    Ok(_) => return Ok(arn),
                    // Another bucket credential made it meanwhile: it's checked.
                    Err(err) if err.to_string().contains("EntityAlreadyExists") => {
                        self.cli.json(&["iam", "get-policy", "--policy-arn", &arn])?
                    }
                    Err(err) => return Err(err.context(format!("making the policy {BOUNDARY}"))),
                }
            }
            Err(err) => return Err(err),
        };
        let version = found["Policy"]["DefaultVersionId"]
            .as_str()
            .within("AWS didn't say the boundary's version")?;
        let held = self.cli.json(&[
            "iam",
            "get-policy-version",
            "--policy-arn",
            &arn,
            "--version-id",
            version,
        ])?;
        ensure!(
            // The CLI gives it as JSON, decoded from IAM's text.
            held["PolicyVersion"]["Document"] == wanted,
            "the policy {BOUNDARY} in this account isn't the one Link makes: a bucket credential \
             bounded by it could reach more than its bucket. Delete it (aws iam delete-policy \
             --policy-arn {arn}) and Link makes its own"
        );
        Ok(arn)
    }

    /// IAM user `user` gone, with its keys and policy; nothing if it isn't
    /// there.
    fn delete_user(&self, user: &str) -> anyhow::Result<()> {
        let gone = |err: &anyhow::Error| err.to_string().contains("NoSuchEntity");
        match self.delete_keys(user) {
            Err(err) if gone(&err) => return Ok(()),
            other => other?,
        }
        for args in [
            vec![
                "iam",
                "delete-user-policy",
                "--user-name",
                user,
                "--policy-name",
                POLICY,
            ],
            vec!["iam", "delete-user", "--user-name", user],
        ] {
            match self.cli.run(&args) {
                Err(err) if !gone(&err) => return Err(err),
                _ => {}
            }
        }
        Ok(())
    }

    /// Deletes every access key of IAM user `user`.
    fn delete_keys(&self, user: &str) -> anyhow::Result<()> {
        let keys = self
            .cli
            .json(&["iam", "list-access-keys", "--user-name", user])?;
        for id in keys["AccessKeyMetadata"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|k| k["AccessKeyId"].as_str())
        {
            self.cli.run(&[
                "iam",
                "delete-access-key",
                "--user-name",
                user,
                "--access-key-id",
                id,
            ])?;
        }
        Ok(())
    }
}

impl Aws {
    /// Signs one conditional PUT with the account's own CLI credentials.
    pub fn upload_once(
        &self,
        bucket: &str,
        object: &str,
        expires: link_cloud_contract::Deadline,
    ) -> anyhow::Result<link_cloud_contract::upload::Upload> {
        use link_cloud_contract::upload;
        check_bucket(bucket)?;
        upload::check_object(object)?;
        link_cloud_contract::deadline::check(expires)
            .map_err(|_| anyhow::anyhow!("an upload lasts at most seven days"))?;
        let lasts = expires.get().as_secs();
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Credentials {
            access_key_id: String,
            secret_access_key: link_secretive::Secret,
            #[serde(default)]
            session_token: Option<link_secretive::Secret>,
            #[serde(default)]
            expiration: Option<String>,
        }
        let credentials: Credentials = serde_json::from_slice(&self.cli.run(&[
            "configure",
            "export-credentials",
            "--format",
            "process",
        ])?)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let until = upload::timestamp(now + lasts)?;
        if let Some(expiration) = &credentials.expiration {
            let compact: String = expiration
                .chars()
                .filter(|c| !matches!(c, '-' | ':'))
                .collect();
            anyhow::ensure!(
                compact.as_str() >= until.as_str(),
                "the AWS login expires before this job's deadline; use a login lasting through it"
            );
        }
        let region = self.bucket_region(bucket)?;
        signed_put(
            bucket,
            object,
            &region,
            lasts,
            &upload::timestamp(now)?,
            &credentials.access_key_id,
            &credentials.secret_access_key,
            credentials.session_token.as_ref(),
        )
    }

    pub fn object_read(&self, bucket: &str, object: &str) -> anyhow::Result<Option<String>> {
        check_bucket(bucket)?;
        link_cloud_contract::upload::check_object(object)?;
        match self
            .cli
            .run(&["s3", "cp", &format!("s3://{bucket}/{object}"), "-"])
        {
            Ok(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            Err(err)
                if err.to_string().contains("NoSuchKey") || err.to_string().contains("(404)") =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }
}

/// AWS Signature V4. If-None-Match is signed: omitting it invalidates the URL.
#[expect(
    clippy::too_many_arguments,
    reason = "each is a part of the request Signature V4 signs"
)]
fn signed_put(
    bucket: &str,
    object: &str,
    region: &str,
    expires: u64,
    date: &str,
    access: &str,
    secret: &link_secretive::Secret,
    token: Option<&link_secretive::Secret>,
) -> anyhow::Result<link_cloud_contract::upload::Upload> {
    use link_digest as digest;
    use link_secretive::hmac;
    fn encoded(text: &str) -> String {
        text.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    let host = format!("s3.{region}.amazonaws.com");
    let path = format!("/{bucket}/{object}");
    let scope = format!("{}/{region}/s3/aws4_request", &date[..8]);
    let seed = secret.expose_map(|secret| format!("AWS4{secret}"));
    let day = hmac::sign(seed.expose().as_bytes(), &date.as_bytes()[..8]);
    let region_key = hmac::sign(&day, region.as_bytes());
    let service = hmac::sign(&region_key, b"s3");
    let signing = hmac::sign(&service, b"aws4_request");
    let query = |token: Option<&str>| {
        let mut query = BTreeMap::from([
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".into()),
            ("X-Amz-Credential", format!("{access}/{scope}")),
            ("X-Amz-Date", date.into()),
            ("X-Amz-Expires", expires.to_string()),
            ("X-Amz-SignedHeaders", "host;if-none-match".into()),
        ]);
        if let Some(token) = token {
            query.insert("X-Amz-Security-Token", token.into());
        }
        query
            .iter()
            .map(|(k, v)| format!("{}={}", encoded(k), encoded(v)))
            .collect::<Vec<_>>()
            .join("&")
    };
    // The session token, where there is one, rides in the query: the
    // query, and the URL made of it, are secrets from here.
    let query = match token {
        Some(token) => token.expose_map(|token| query(Some(token.as_str()))),
        None => query(None).into(),
    };
    let url = query.expose_map(|query| {
        let canonical = format!(
            "PUT\n{path}\n{query}\nhost:{host}\nif-none-match:*\n\nhost;if-none-match\nUNSIGNED-PAYLOAD"
        );
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
            digest::sha256(canonical.as_bytes())
        );
        let signature = hmac::hex(&hmac::sign(&signing, to_sign.as_bytes()));
        format!("https://{host}{path}?{query}&X-Amz-Signature={signature}")
    });
    Ok(link_cloud_contract::upload::Upload {
        url,
        headers: BTreeMap::from([("if-none-match".into(), "*".into())]),
    })
}

#[cfg(test)]
mod upload_tests {
    #[test]
    fn a_signed_put_cannot_be_changed_to_a_read_or_an_overwrite() {
        let secret = link_secretive::Secret::from("example-secret".to_string());
        let signed = super::signed_put(
            "results",
            "job/output.tar",
            "us-east-1",
            14400,
            "20261015T120000Z",
            "EXAMPLE",
            &secret,
            None,
        )
        .unwrap();
        assert_eq!(signed.headers["if-none-match"], "*");
        let url = signed.url.expose();
        assert!(url.contains("X-Amz-SignedHeaders=host%3Bif-none-match"));
        assert!(url.contains("X-Amz-Expires=14400"));
        assert!(url.contains("X-Amz-Signature="));
        assert!(!url.contains("example-secret"));
        assert!(!format!("{signed:?}").contains("X-Amz-Signature"));
    }
}
