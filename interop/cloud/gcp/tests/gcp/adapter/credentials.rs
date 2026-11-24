use super::*;

#[test]
fn a_personal_credential_file_binds_new_and_previously_saved_accounts() {
    let dir = TempDir::new("gcp-personal-credential");
    let gcloud = fake(&dir);
    let sdk = dir.join("home/.config/gcloud");
    std::fs::create_dir_all(sdk.join("configurations")).unwrap();
    std::fs::write(sdk.join("configurations/config_default"), "[auth]\naccess_token_file = /other/token\nimpersonate_service_account = other@example.iam.gserviceaccount.com\n").unwrap();
    let source = dir.join("personal.json");
    std::fs::write(
        &source,
        r#"{"type":"authorized_user","refresh_token":"fixture"}"#,
    )
    .unwrap();
    let output = lnk_gcp(&dir, &gcloud)
        .args(["connect", "--name", "user", "--credential-file"])
        .arg(source)
        .args(["--project", "my-project", "--zone", "us-central1-a"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut account: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let kept = dir.join("home/.lnk/safe/gcp/credentials/user.json");
    assert_eq!(
        account["settings"]["CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE"],
        kept.display().to_string()
    );
    // Old saved accounts contain only the libraries' credential selector.
    account["settings"]
        .as_object_mut()
        .unwrap()
        .remove("CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE");
    account["settings"]["CLOUDSDK_CONFIG"] = serde_json::json!(sdk);
    call(
        &dir,
        &gcloud,
        "state",
        serde_json::json!({"account":account,"id":"lnk-work"}),
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    let overrides: Vec<_> = log
        .lines()
        .filter(|line| line.starts_with("CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE="))
        .collect();
    assert!(overrides.len() >= 2, "{log}");
    let configs: Vec<_> = log
        .lines()
        .filter_map(|line| line.strip_prefix("CLOUDSDK_CONFIG="))
        .collect();
    assert!(configs.len() >= 2, "{log}");
    assert!(
        configs.iter().all(|path| !Path::new(path).exists()),
        "temporary SDK state must be removed: {configs:?}"
    );
    assert!(
        overrides
            .iter()
            .all(|line| *line
                == format!("CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE={}", kept.display())),
        "{log}"
    );
}
