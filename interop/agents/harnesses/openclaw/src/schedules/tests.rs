use super::*;
use link_harness_contract::asp::schedules::{
    DestinationOwner, DestinationThread, WhenCron, WhenEvery,
};
use link_test_kit::TempDir;
use serde_json::json;

fn put(root: &Path, value: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(value).unwrap();
    std::fs::create_dir_all(root.join("cron")).unwrap();
    std::fs::write(root.join(JOBS), &bytes).unwrap();
    bytes
}

#[test]
fn malformed_native_state_is_an_error_and_never_cleared() {
    let root = TempDir::new("native-malformed");
    std::fs::create_dir_all(root.join("cron")).unwrap();
    std::fs::write(root.join(JOBS), b"{broken").unwrap();
    assert!(export_root(&root).is_err());
    assert_eq!(std::fs::read(root.join(JOBS)).unwrap(), b"{broken");
}

#[test]
fn clearing_checks_the_export_revision_and_keeps_a_retryable_archive() {
    let root = TempDir::new("native-handoff");
    let initial = native(vec![job()]);
    let bytes = put(&root, &initial);
    let exported = export_root(&root).unwrap();
    let revision = exported.source_revision.unwrap();
    let changed = native(vec![job(), unsupported()]);
    let changed_bytes = put(&root, &changed);
    assert!(clear_root(&root, &revision).is_err());
    assert_eq!(std::fs::read(root.join(JOBS)).unwrap(), changed_bytes);
    put(&root, &initial);
    clear_root(&root, &revision).unwrap();
    clear_root(&root, &revision).unwrap();
    assert_eq!(
        std::fs::read(root.join(ARCHIVE).join(format!("{revision}.json"))).unwrap(),
        bytes
    );
    assert!(export_root(&root).unwrap().carried.is_empty());
}

#[test]
fn unsupported_jobs_are_named_with_a_reason_and_other_jobs_survive() {
    let root = TempDir::new("native-omissions");
    prepare(&root);
    put(&root, &native(vec![job(), unsupported()]));
    let exported = export_root(&root).unwrap();
    assert_eq!(exported.carried.len(), 1);
    assert_eq!(exported.not_carried.len(), 1);
    assert_eq!(exported.not_carried[0].job, "unsupported");
    assert!(!exported.not_carried[0].reason.is_empty());
    let mut invalid = job();
    invalid["schedule"] = json!({"kind":"cron", "expr":"90 25 * * *", "tz":"Europe/Lisbon"});
    put(&root, &native(vec![invalid]));
    let exported = export_root(&root).unwrap();
    assert_eq!(exported.not_carried.len(), 1);
    assert!(exported.carried.is_empty());
}

#[test]
fn duplicate_source_identity_is_refused_instead_of_double_firing() {
    let root = TempDir::new("native-duplicate");
    prepare(&root);
    put(&root, &native(vec![job(), job()]));
    assert!(export_root(&root).is_err());
}
// Persisted cron shape at the adapter's pinned OpenClaw revision.
fn job() -> Value {
    json!({"id":"inv-monthly", "name":"Draft invoices", "agentId":"main", "enabled":true,
        "createdAtMs":1780000000000_u64, "sessionTarget":"isolated", "wakeMode":"now",
        "schedule":{"kind":"cron", "expr":"0 9 1 * *", "tz":"Europe/Lisbon"},
        "payload":{"kind":"agentTurn", "message":"Draft this month's invoices."},
        "delivery":{"mode":"announce", "channel":"telegram"}})
}
fn native(jobs: Vec<Value>) -> Value {
    json!({"version":1,"jobs":jobs})
}
fn prepare(_root: &Path) {}
fn unsupported() -> Value {
    let mut job = job();
    job["id"] = "unsupported".into();
    job["payload"] = json!({"kind":"command", "command":"backup"});
    job
}

#[test]
fn pinned_cron_interval_phase_owner_and_topic_routes_are_preserved() {
    let root = TempDir::new("openclaw-schedules");
    let mut every = job();
    every["id"] = "inbox-am".into();
    every["name"] = "Morning inbox".into();
    every["schedule"] = json!({"kind":"every", "everyMs":600000, "anchorMs":1780000017000_u64});
    every["delivery"] = json!({"mode":"announce", "channel":"telegram", "to":"-42", "threadId":7});
    put(&root, &native(vec![job(), every]));
    let exported = export_root(&root).unwrap();
    assert!(exported.not_carried.is_empty(), "{exported:?}");
    assert_eq!(exported.carried[0].name, "Draft invoices");
    assert_eq!(
        exported.carried[0].to,
        Destination::Owner(DestinationOwner {
            connection: Some("telegram".into())
        })
    );
    assert_eq!(
        exported.carried[0].when,
        When::Cron(WhenCron {
            cron: "0 9 1 * *".into(),
            zone: "Europe/Lisbon".into()
        })
    );
    assert_eq!(
        exported.carried[1].when,
        When::Every(WhenEvery {
            seconds: 600,
            anchor: 1780000017
        })
    );
    assert_eq!(
        exported.carried[1].to,
        Destination::Thread(DestinationThread {
            thread: "telegram:-42.7".into()
        })
    );
    assert_eq!(export_root(&root).unwrap(), exported);
}
