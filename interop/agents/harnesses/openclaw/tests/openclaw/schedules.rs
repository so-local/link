//! Real adapter commands at the native/portable boundary, with no vendor runtime.
use link_test_kit::{TempDir, env};
use serde_json::{Value, json};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lnk-openclaw");

fn output(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    env::command(PROGRAM, home).args(args).output().unwrap()
}

fn native() -> Value {
    json!({"version":1,"jobs":[{"id":"inbox", "name":"Morning inbox", "enabled":true,"agentId":"main","wakeMode":"now", "schedule":{"kind":"every","everyMs":600000,"anchorMs":1780000017000_u64}, "payload":{"kind":"agentTurn","message":"Summarise client mail"},"delivery":{"mode":"announce","channel":"telegram","to":"-42","threadId":7}}]})
}

#[test]
fn native_export_clear_and_retry_preserve_the_original_and_portable_route() {
    let home = TempDir::new("openclaw-schedule-boundary");
    let native_root = home.join("native");
    let managed = home.join("managed");
    std::fs::create_dir_all(native_root.join("cron")).unwrap();
    std::fs::create_dir_all(managed.join("cron")).unwrap();
    let bytes = serde_json::to_vec(&native()).unwrap();
    std::fs::write(native_root.join("cron/jobs.json"), &bytes).unwrap();
    std::fs::write(managed.join("cron/jobs.json"), &bytes).unwrap();
    let exported = output(&home, &["state", "schedules", managed.to_str().unwrap()]);
    assert!(exported.status.success(), "{exported:?}");
    let state: Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(state["carried"].as_array().unwrap().len(), 1, "{state}");
    assert_eq!(state["carried"][0]["name"], "Morning inbox");
    assert_eq!(
        state["carried"][0]["to"],
        json!({"kind":"thread","thread":"telegram:-42.7"})
    );
    assert!(state["notCarried"].as_array().unwrap().is_empty());
    let revision = state["sourceRevision"].as_str().unwrap();
    for _ in 0..2 {
        let cleared = output(
            &home,
            &[
                "state",
                "clear-schedules",
                revision,
                managed.to_str().unwrap(),
            ],
        );
        assert!(cleared.status.success(), "{cleared:?}");
    }
    assert_eq!(
        std::fs::read(native_root.join("cron/jobs.json")).unwrap(),
        bytes
    );
    let emptied = output(&home, &["state", "schedules", managed.to_str().unwrap()]);
    let emptied: Value = serde_json::from_slice(&emptied.stdout).unwrap();
    assert!(emptied["carried"].as_array().unwrap().is_empty());
    assert_eq!(
        std::fs::read(
            managed
                .join(".lnk/schedule-imports")
                .join(format!("{revision}.json"))
        )
        .unwrap(),
        bytes
    );
}

#[test]
fn malformed_native_state_refuses_export_and_remains_untouched() {
    let home = TempDir::new("openclaw-bad-schedule");
    std::fs::create_dir_all(home.join("cron")).unwrap();
    std::fs::write(home.join("cron/jobs.json"), b"{broken").unwrap();
    let refused = output(&home, &["state", "schedules", home.to_str().unwrap()]);
    assert!(!refused.status.success());
    assert_eq!(
        std::fs::read(home.join("cron/jobs.json")).unwrap(),
        b"{broken"
    );
}
