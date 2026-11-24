use super::*;
use link_harness_contract::asp::schedules::{DestinationThread, WhenAt, WhenCron, WhenEvery};
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
    assert!(clear_root(&root, &revision, None).is_err());
    assert_eq!(std::fs::read(root.join(JOBS)).unwrap(), changed_bytes);
    put(&root, &initial);
    clear_root(&root, &revision, None).unwrap();
    clear_root(&root, &revision, None).unwrap();
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
// Persisted cron shape at the adapter's pinned Hermes revision.
fn job() -> Value {
    json!({"id":"inbox-am", "name":"Morning inbox", "enabled":true,
        "schedule":{"kind":"interval", "minutes":10, "display":"every 10m"},
        "next_run_at":"2026-10-30T08:30:17+00:00", "prompt":"Summarise new client mail.",
        "repeat":{"times":null, "completed":0}, "skills":[],
        "deliver":"origin", "origin":{"platform":"telegram", "chat_id":"-42", "thread_id":"7"}})
}
fn native(jobs: Vec<Value>) -> Value {
    json!({"jobs":jobs})
}
fn prepare(root: &Path) {
    std::fs::write(root.join("config.yaml"), "timezone: Europe/Lisbon\n").unwrap();
}
fn unsupported() -> Value {
    let mut job = job();
    job["id"] = "unsupported".into();
    job["script"] = "backup.sh".into();
    job
}

#[test]
fn pinned_interval_phase_explicit_cron_zone_and_topic_routes_are_preserved() {
    let root = TempDir::new("hermes-schedules");
    prepare(&root);
    let mut cron = job();
    cron["id"] = "inv-monthly".into();
    cron["schedule"] = json!({"kind":"cron", "expr":"0 9 1 * *"});
    put(&root, &native(vec![job(), cron]));
    let exported = export_root(&root).unwrap();
    assert!(exported.not_carried.is_empty(), "{exported:?}");
    assert_eq!(
        exported.carried[0].when,
        When::Every(WhenEvery {
            seconds: 600,
            anchor: instant("2026-10-30T08:30:17+00:00").unwrap()
        })
    );
    assert_eq!(
        exported.carried[0].to,
        Destination::Thread(DestinationThread {
            thread: "telegram:-42.7".into()
        })
    );
    assert_eq!(
        exported.carried[1].when,
        When::Cron(WhenCron {
            cron: "0 9 1 * *".into(),
            zone: "Europe/Lisbon".into()
        })
    );
    assert_eq!(export_root(&root).unwrap(), exported);
}

#[test]
fn ambiguous_local_zone_and_unsupported_delivery_are_explicit_omissions() {
    let root = TempDir::new("hermes-ambiguous");
    let mut cron = job();
    cron["schedule"] = json!({"kind":"cron", "expr":"0 9 * * *"});
    put(&root, &native(vec![cron]));
    assert!(
        export_root(&root).unwrap().not_carried[0]
            .reason
            .contains("timezone")
    );
    let mut local = job();
    local["deliver"] = "local".into();
    put(&root, &native(vec![local]));
    assert!(
        export_root(&root).unwrap().not_carried[0]
            .reason
            .contains("output")
    );
}

#[test]
fn a_native_one_shot_keeps_its_absolute_instant_and_cannot_repeat() {
    let root = TempDir::new("hermes-once");
    let mut once = job();
    once["schedule"] = json!({"kind":"once", "run_at":"2026-10-30T08:30:17Z"});
    once["repeat"] = json!({"times":1,"completed":0});
    put(&root, &native(vec![once.clone()]));
    let exported = export_root(&root).unwrap();
    assert_eq!(
        exported.carried[0].when,
        When::At(WhenAt {
            at: instant("2026-10-30T08:30:17Z").unwrap()
        })
    );
    once["repeat"]["completed"] = 1.into();
    put(&root, &native(vec![once]));
    assert_eq!(export_root(&root).unwrap().not_carried.len(), 1);
}

#[test]
fn a_zone_changed_after_export_refuses_clearing_even_when_jobs_bytes_are_unchanged() {
    let root = TempDir::new("hermes-zone-revision");
    prepare(&root);
    let mut cron = job();
    cron["schedule"] = json!({"kind":"cron", "expr":"0 9 * * *"});
    let bytes = put(&root, &native(vec![cron]));
    let exported = export_root(&root).unwrap();
    std::fs::write(root.join("config.yaml"), "timezone: America/New_York\n").unwrap();
    assert!(
        clear_root(
            &root,
            exported.source_revision.as_deref().unwrap(),
            exported.source_context.as_deref()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(root.join(JOBS)).unwrap(), bytes);
}
