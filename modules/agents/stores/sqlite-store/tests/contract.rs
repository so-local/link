//! `lnk-sqlite-store` run as its callers run it: the store's contract, and
//! its file.

link_test_kit::program!(unit);

use link_secretive::Duplicate;
use link_store_contract::store::{Agent, Driver, Job, Place, Store as _};
use link_test_kit::TempDir;

/// The driver's file, in the folder it's given.
const FILE: &str = "events.sqlite";

/// The driver keeps the store's contract, run as its callers run it.
#[test]
fn it_keeps_the_contract() {
    let dir = TempDir::new("contract");
    let program = std::path::PathBuf::from(env!("CARGO_BIN_EXE_lnk-sqlite-store"));
    let driver = Driver::at("sqlite", program, &dir);
    let agent = Agent {
        id: "a-1".into(),
        owner: "me@box".into(),
    };
    link_store_contract::store::check(&driver, &agent);
    link_store_contract::store::check_jobs(&driver, &agent);
    link_store_contract::store::check_files(&driver, &agent);
    link_store_contract::store::check_narrowing(&driver, &agent);
    // Only the user's.
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(dir.join(FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// A file that isn't there is read as holding nothing, and isn't
/// made by a read; a link in its place is refused.
#[test]
fn a_read_makes_nothing_and_follows_no_link() {
    let root = TempDir::new("contract-reads");
    // Not made yet: a read makes none.
    let dir = root.join("store");
    let program = std::path::PathBuf::from(env!("CARGO_BIN_EXE_lnk-sqlite-store"));
    let driver = Driver::at("sqlite", program, &dir);
    let agent = Agent::default();
    assert!(driver.read(&agent, "t", 1).unwrap().is_empty());
    assert!(driver.threads(&agent).unwrap().is_empty());
    assert!(!dir.exists());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("elsewhere"), "").unwrap();
    std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join(FILE)).unwrap();
    assert!(driver.read(&agent, "t", 1).is_err());
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "hi"});
    assert!(driver.append(&agent, "t", &event).is_err());
    assert_eq!(std::fs::read(dir.join("elsewhere")).unwrap(), b"");
}

/// Behind the store bridge, as a harness in its sandbox calls it: the
/// same contract, through calls proved as the agent; another agent's
/// conversations, or a call naming another agent, refused.
#[test]
fn it_keeps_the_contract_behind_the_store_bridge() {
    let dir = TempDir::new("contract-bridged");
    let program = std::path::PathBuf::from(env!("CARGO_BIN_EXE_lnk-sqlite-store"));
    let driver = Driver::at("sqlite", program, &dir.join("chats"));
    let bridged = link_store_contract::testing::store_bridge(&dir, "a-1", driver);
    let agent = Agent {
        id: "a-1".into(),
        owner: "me@box".into(),
    };
    link_store_contract::store::check(&bridged, &agent);
    link_store_contract::store::check_jobs(&bridged, &agent);
    link_store_contract::store::check_files(&bridged, &agent);
    link_store_contract::store::check_narrowing(&bridged, &agent);
    assert!(dir.join("chats").join(FILE).exists());
    let theirs = Agent {
        id: "b-2".into(),
        ..agent.clone()
    };
    let refused = bridged.read(&theirs, "telegram:42", 1).unwrap_err();
    assert!(format!("{refused:#}").contains("refused"), "{refused:#}");
    let posing = link_store_contract::store::Bridged {
        binding: link_sandbox_binding::Binding {
            identity: link_sandbox_binding::Identity {
                agent: "b-2".into(),
                ..bridged.binding.identity.clone()
            },
            ..bridged.binding.duplicate()
        },
        ..bridged.duplicate()
    };
    // Named another agent bound here, proved with this one's key.
    link_sandbox_binding::Keyring::at(&dir.join("keys"))
        .bind(
            &posing.binding.identity,
            &link_secretive::Secret::from("1".repeat(32)),
        )
        .unwrap();
    let refused = posing.read(&theirs, "telegram:42", 1).unwrap_err();
    assert!(
        format!("{refused:#}").contains("proof isn't agent b-2's"),
        "{refused:#}"
    );
}

/// Its contract says it keeps jobs; a file written before it kept them
/// holds none, and takes them at the first write, its conversations
/// kept.
#[test]
fn a_file_from_before_jobs_takes_them() {
    let dir = TempDir::new("contract-jobs-later");
    let program = std::path::PathBuf::from(env!("CARGO_BIN_EXE_lnk-sqlite-store"));
    let driver = Driver::at("sqlite", program, &dir);
    assert!(driver.contract().unwrap().jobs);
    let agent = Agent::default();
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "hi"});
    driver.append(&agent, "t", &event).unwrap();
    // As one written before jobs: their tables gone.
    rusqlite::Connection::open(dir.join(FILE))
        .unwrap()
        .execute_batch("DROP TABLE jobs; DROP TABLE job_days;")
        .unwrap();
    assert!(driver.job_list(&agent, Place::Waiting).unwrap().is_empty());
    assert_eq!(driver.job_next(&agent).unwrap(), None);
    assert_eq!(driver.job_day(&agent).unwrap(), (0, 0));
    assert!(
        !driver
            .job_shift(&agent, "j", Place::Waiting, Place::Firing)
            .unwrap()
    );
    let job = Job {
        id: "j".into(),
        spec: serde_json::json!({"text": "later"}),
        due: 5,
    };
    driver.job_put(&agent, Place::Waiting, &job).unwrap();
    assert_eq!(driver.job_next(&agent).unwrap(), Some(5));
    assert_eq!(driver.read(&agent, "t", 1).unwrap(), [event]);
}

/// Run outside the sandbox, in a folder the sandbox writes, `lnk-sqlite-store`
/// never writes through a link planted where SQLite keeps its journal:
/// the write is refused, and what the link points at is as it was.
#[test]
fn a_journal_planted_as_a_link_is_never_written_through() {
    let dir = TempDir::new("sqlite-journal-link");
    let agent = Agent {
        id: "aaaaaaaaaaaa".into(),
        owner: "me@box".into(),
    };
    let program = std::path::PathBuf::from(env!("CARGO_BIN_EXE_lnk-sqlite-store"));
    let driver = Driver::at("sqlite", program, &dir);
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "hi"});
    driver.append(&agent, "t", &event).unwrap();
    let theirs = dir.join("theirs");
    std::fs::write(&theirs, "theirs\n").unwrap();
    std::os::unix::fs::symlink(&theirs, dir.join("events.sqlite-journal")).unwrap();
    let event = serde_json::json!({"seq": 2, "role": "user", "text": "again"});
    assert!(driver.append(&agent, "t", &event).is_err());
    assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "theirs\n");
}
