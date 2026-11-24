//! Buckets: an agent's labelled, found and keyed alone, and every bucket
//! call signed in as the account.

use super::*;

/// Answers each S3 and IAM call the way AWS would.
const S3: &str = r#"case "$*" in
  "s3api list-buckets") echo '{"Buckets": [{"Name": "lnk-bucket-a1b2c3d4e5f6"}, {"Name": "lnk-bucket-other000000"}, {"Name": "someone-elses"}]}' ;;
  "s3api get-bucket-location --bucket lnk-bucket-a1b2c3d4e5f6") echo '{"LocationConstraint": "eu-west-1"}' ;;
  "s3api get-bucket-location "*) echo '{"LocationConstraint": null}' ;;
  "s3api get-bucket-tagging --bucket lnk-bucket-a1b2c3d4e5f6 "*) echo '{"TagSet": [{"Key": "lnk-folder", "Value": "p8r2c4n6w0zt"}, {"Key": "kept", "Value": "yes"}]}' ;;
  "s3api get-bucket-tagging "*) echo 'An error occurred (NoSuchTagSet)' >&2; exit 254 ;;
  "s3api put-bucket-tagging "*) echo "$*" > "$(dirname "$0")/tagged" ;;
  "sts get-caller-identity "*) echo '{"Account": "123456789012"}' ;;
  "iam get-policy "*) if [ -f "$(dirname "$0")/boundary" ]; then echo '{"Policy": {"DefaultVersionId": "v1"}}'; else echo 'An error occurred (NoSuchEntity)' >&2; exit 254; fi ;;
  "iam create-policy "*) prev=''; for a do [ "$prev" = --policy-document ] && printf '%s' "$a" > "$(dirname "$0")/boundary"; prev=$a; done; echo '{}' ;;
  "iam get-policy-version "*) printf '{"PolicyVersion": {"Document": %s}}\n' "$(cat "$(dirname "$0")/boundary")" ;;
  "iam create-user "*) echo 'An error occurred (EntityAlreadyExists)' >&2; exit 254 ;;
  "iam put-user-permissions-boundary "*) ;;
  "iam put-user-policy "*) echo "$*" > "$(dirname "$0")/policy" ;;
  "iam list-access-keys "*) echo '{"AccessKeyMetadata": [{"AccessKeyId": "AKIAOLD"}]}' ;;
  "iam delete-access-key "*|"iam delete-user-policy "*|"iam delete-user "*) ;;
  "iam create-access-key "*) echo '{"AccessKey": {"AccessKeyId": "AKIANEW", "SecretAccessKey": "the-new-secret"}}' ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac"#;

#[test]
fn an_agents_bucket_is_labelled_found_and_keyed_alone() {
    let dir = TempDir::new("adapter-buckets");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(&dir, S3);
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "us-east-1", "env": {}});
    let bucket = "lnk-bucket-a1b2c3d4e5f6";

    call(
        &dir,
        &aws,
        "label",
        serde_json::json!({"account": account, "bucket": bucket, "labels": {"lnk-folder-name": "moltbot-2"}}),
    );
    // Its tags merged, in its own region.
    let tagged = std::fs::read_to_string(dir.join("tagged")).unwrap();
    assert!(tagged.contains("--region eu-west-1"), "{tagged}");
    for tag in ["kept", "lnk-folder", "moltbot-2"] {
        assert!(tagged.contains(tag), "{tag} in {tagged}");
    }

    let found = call(
        &dir,
        &aws,
        "buckets",
        serde_json::json!({"account": account, "labels": {"lnk-folder": "p8r2c4n6w0zt"}}),
    );
    assert_eq!(
        found["buckets"],
        serde_json::json!([{"bucket": bucket, "place": "eu-west-1",
            "labels": {"kept": "yes", "lnk-folder": "p8r2c4n6w0zt"}}])
    );

    let credential = call(
        &dir,
        &aws,
        "bucket-credential",
        serde_json::json!({"account": account, "bucket": bucket}),
    );
    assert_eq!(
        credential["credential"],
        serde_json::json!({"type": "s3", "provider": "AWS", "access_key_id": "AKIANEW",
            "secret_access_key": "the-new-secret", "region": "eu-west-1"})
    );
    // Its own user, named as the bucket, with a policy for that bucket
    // alone; the old key goes.
    let policy = std::fs::read_to_string(dir.join("policy")).unwrap();
    assert!(
        policy.contains(&format!("--user-name {bucket} ")),
        "{policy}"
    );
    assert!(
        policy.contains(&format!("arn:aws:s3:::{bucket}/*")),
        "{policy}"
    );
    assert!(!policy.contains("\"s3:*\""), "{policy}");
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert!(log.contains(
        "iam delete-access-key --user-name lnk-bucket-a1b2c3d4e5f6 --access-key-id AKIAOLD"
    ));
    // Bounded: the boundary made once in the account, holding any policy
    // to the bucket named as the user; the user made with it, or given it.
    let boundary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("boundary")).unwrap()).unwrap();
    assert_eq!(
        boundary["Statement"][1]["Resource"],
        "arn:aws:s3:::${aws:username}/*"
    );
    let arn = "arn:aws:iam::123456789012:policy/lnk-bucket-boundary";
    assert!(
        log.contains(&format!(
            "iam create-user --path /lnk/ --user-name {bucket} --permissions-boundary {arn}"
        )),
        "{log}"
    );
    assert!(
        log.contains(&format!(
            "iam put-user-permissions-boundary --user-name {bucket} --permissions-boundary {arn}"
        )),
        "{log}"
    );
    // The unbounded user an older lnk-aws made goes.
    assert!(
        log.contains(&format!("iam delete-user --user-name lnk-{bucket}")),
        "{log}"
    );
    // A second key finds Link's boundary there and makes none.
    call(
        &dir,
        &aws,
        "bucket-credential",
        serde_json::json!({"account": account, "bucket": bucket}),
    );
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert_eq!(log.matches("iam create-policy ").count(), 1, "{log}");

    call(
        &dir,
        &aws,
        "revoke-key",
        serde_json::json!({"account": account, "bucket": bucket}),
    );
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert!(
        log.contains(&format!("iam delete-user --user-name {bucket}")),
        "{log}"
    );

    // A boundary someone else made under Link's name could allow anything:
    // no key is made under it.
    std::fs::write(
        dir.join("boundary"),
        r#"{"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "*", "Resource": "*"}]}"#,
    )
    .unwrap();
    let refused = lnk_aws(&dir, &aws)
        .arg("bucket-credential")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(
                serde_json::json!({"account": account, "bucket": bucket})
                    .to_string()
                    .as_bytes(),
            )?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("isn't the one Link makes"), "{said}");
}

/// Buckets act as the account too: its profile or its key, as rclone's
/// settings, with no AWS CLI needed.
#[test]
fn buckets_sign_in_as_the_account() {
    let dir = TempDir::new("adapter-storage");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = dir.join("no-aws-here");
    let profile = call(
        &dir,
        &aws,
        "storage",
        serde_json::json!({"account": {"name": "aws", "kind": "aws", "place": "eu-west-1",
            "env": {"AWS_PROFILE": "work", "AWS_REGION": "eu-west-1"}}}),
    );
    assert_eq!(
        profile,
        serde_json::json!({"credential": {"type": "s3", "provider": "AWS", "env_auth": "true", "profile": "work"}})
    );
    let key = call(
        &dir,
        &aws,
        "storage",
        serde_json::json!({"account": {"name": "aws", "kind": "aws", "place": "eu-west-1",
            "env": {"AWS_ACCESS_KEY_ID": "AKIA1", "AWS_SECRET_ACCESS_KEY": "s3cret"}}}),
    );
    assert_eq!(
        key,
        serde_json::json!({"credential": {"type": "s3", "provider": "AWS",
            "access_key_id": "AKIA1", "secret_access_key": "s3cret"}})
    );
}
