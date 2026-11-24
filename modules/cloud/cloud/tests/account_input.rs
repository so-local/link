//! Credential-bearing context transfer uses exact reserved arguments and stdin.

link_test_kit::program!(system);

use std::io::Write;
use std::process::{Output, Stdio};
use std::time::Duration;

fn import(home: &std::path::Path, input: &serde_json::Value) -> Output {
    let mut child = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-cloud"), home)
        .arg(link_cloud_contract::account_input::IMPORT_ARGUMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(input).unwrap())
        .unwrap();
    link_test_kit::wait::exited("account transfer", &mut child, Duration::from_secs(5));
    child.wait_with_output().unwrap()
}

fn frame() -> serde_json::Value {
    serde_json::json!({"version":1,"account":{
        "name":"qa-work","kind":"aws","place":"eu-west-2",
        "identity":"fixture account","login":"fixture raw key","region":"eu-west-2",
        "profile":"qa-profile","project":"fixture-project",
        "settings":{"AWS_CONFIG_FILE":"/explicit/config","AWS_SHARED_CREDENTIALS_FILE":"/explicit/credentials"},
        "env":{"AWS_ACCESS_KEY_ID":"fixture-id","AWS_SECRET_ACCESS_KEY":"fixture-sensitive-value","AWS_SESSION_TOKEN":"fixture-session"}
    }})
}

#[test]
fn exact_connected_account_roundtrips_without_signin_or_credential_output() {
    let dir = link_test_kit::temp::TempDir::new("account-input");
    let schema = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-cloud"), dir.path())
        .arg(link_cloud_contract::account_input::SCHEMA_ARGUMENT)
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: link_cloud_contract::account_input::Schema =
        serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(schema.version, 1);
    let input = frame();
    let accepted = import(dir.path(), &input);
    assert!(accepted.status.success());
    let ack: link_cloud_contract::account_input::Accepted =
        serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(ack.name, "qa-work");
    assert!(!String::from_utf8_lossy(&accepted.stdout).contains("fixture-sensitive-value"));
    assert!(!String::from_utf8_lossy(&accepted.stderr).contains("fixture-sensitive-value"));
    let loaded = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-cloud"), dir.path())
        .args(["env", "qa-work", "--json"])
        .output()
        .unwrap();
    assert!(loaded.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&loaded.stdout).unwrap(),
        input["account"]
    );
    let mut replacement = frame();
    replacement["account"]["project"] = "other-project".into();
    assert!(
        !import(dir.path(), &replacement).status.success(),
        "never replace an existing account"
    );
}

#[test]
fn invalid_account_input_never_echoes_embedded_credentials() {
    for invalid in [
        serde_json::json!({"version":2,"account":frame()["account"]}),
        serde_json::json!({"version":1,"account":{"name":"fixture-sensitive-value"}}),
    ] {
        let dir = link_test_kit::temp::TempDir::new("account-input-refused");
        let refused = import(dir.path(), &invalid);
        assert!(!refused.status.success());
        assert!(!String::from_utf8_lossy(&refused.stdout).contains("fixture-sensitive-value"));
        assert!(!String::from_utf8_lossy(&refused.stderr).contains("fixture-sensitive-value"));
    }
}
