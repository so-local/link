//! An account connected as this machine's own service account
//! (`connect --machine`): no credential file, and every reader acting
//! through the metadata server.

use super::*;

/// The account `connect --machine` describes, as its settings say it.
fn machine_account() -> serde_json::Value {
    serde_json::json!({
        "name": "services", "kind": "gcp", "place": "us-central1-a",
        "identity": "Google Cloud as box@my-project.iam.gserviceaccount.com",
        "login": "this machine's service account box@my-project.iam.gserviceaccount.com",
        "project": "my-project",
        "settings": {
            "GCE_METADATA_HOST": "metadata.google.internal",
            "CLOUDSDK_CORE_ACCOUNT": "box@my-project.iam.gserviceaccount.com",
            "CLOUDSDK_CORE_PROJECT": "my-project",
            "CLOUDSDK_COMPUTE_ZONE": "us-central1-a"
        }
    })
}

#[test]
fn a_machine_account_acts_through_the_metadata_server_with_no_file() {
    let dir = TempDir::new("gcp-machine-account");
    let gcloud = fake(&dir);
    let account = machine_account();
    // rclone finds the machine's service account itself.
    let storage = call(
        &dir,
        &gcloud,
        "storage",
        serde_json::json!({"account": account}),
    );
    assert_eq!(
        storage,
        serde_json::json!({"credential": {"type": "google cloud storage", "env_auth": "true"}})
    );
    // gcloud acts as the machine's account in a fresh SDK configuration,
    // never a credential file the shell names.
    call(
        &dir,
        &gcloud,
        "state",
        serde_json::json!({"account": account, "id": "lnk-work"}),
    );
    // IAM signs for it: no key file is named.
    call(
        &dir,
        &gcloud,
        "upload-once",
        serde_json::json!({"account":account,"bucket":"results","object":"job/output.tar","expires":"5m"}),
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(
        log.contains("CLOUDSDK_CORE_ACCOUNT=box@my-project.iam.gserviceaccount.com"),
        "{log}"
    );
    assert!(
        !log.contains("CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE"),
        "{log}"
    );
    assert!(!log.contains("--private-key-file"), "{log}");
    let configs: Vec<_> = log
        .lines()
        .filter_map(|line| line.strip_prefix("CLOUDSDK_CONFIG="))
        .collect();
    assert!(configs.len() >= 2, "{log}");
    assert!(
        configs.iter().all(|path| !Path::new(path).exists()),
        "temporary SDK state must be removed: {configs:?}"
    );
    // Disconnecting removes no file: a key another account kept stays.
    let kept = dir.join("home/.lnk/safe/gcp/credentials/other.json");
    std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
    std::fs::write(&kept, "{}").unwrap();
    call(
        &dir,
        &gcloud,
        "disconnect",
        serde_json::json!({"account": account}),
    );
    assert!(kept.exists());
}

#[test]
fn a_machine_account_is_never_also_a_credential_file() {
    let dir = TempDir::new("gcp-machine-and-file");
    let gcloud = fake(&dir);
    let output = lnk_gcp(&dir, &gcloud)
        .args(["connect", "--machine", "--credential-file", "key.json"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--credential-file"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.join("home/.lnk/safe/gcp/credentials").exists());
}

#[test]
fn an_account_naming_no_credential_must_connect_again() {
    let dir = TempDir::new("gcp-no-credential");
    let gcloud = fake(&dir);
    let mut account = machine_account();
    account["settings"]
        .as_object_mut()
        .unwrap()
        .remove("GCE_METADATA_HOST");
    let output = {
        let mut child = lnk_gcp(&dir, &gcloud)
            .arg("storage")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                serde_json::json!({"account": account})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
        child.wait_with_output().unwrap()
    };
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("lnk cloud connect gcp --name services"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
