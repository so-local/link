//! The storage bridge `lnk agent start` serves for Link Harness, on any
//! machine: each call answered by the driver of the place this machine's
//! `agents.toml` keeps its collection at, as the agent's own bridge
//! answers it. Its conversations and instructions, on a machine that
//! lists only `sqlite` for them, are rows of `lnk-sqlite-store`'s file. What's
//! kept at `file` is never served (Link Harness reaches its folder
//! itself), and no file of a collection the agent's sandbox only reads is
//! changed through it, though the sandbox holds the agent's key.

use std::path::Path;
use std::sync::Arc;

use link_agent::bridge::StoragePlan;
use link_sandbox_binding::Binding;
use link_sandbox_binding::Identity;
use link_store_contract::storage::{Collection, Supported};
use link_store_contract::store::{Agent, Bridged, Store as _, Written};
use link_test_kit::TempDir;

const AGENT: &str = "aaaaaaaaaaaa";

/// Link's driver for place `name`, as built beside `lnk`.
fn built(name: &str, dir: &Path) -> anyhow::Result<link_store_contract::store::Driver> {
    let program = link_test_kit::programs().join(format!(
        "lnk-{}",
        link_store_contract::store::module_for(name)
    ));
    Ok(link_store_contract::store::Driver::at(name, program, dir))
}

fn agent() -> Agent {
    Agent {
        id: AGENT.into(),
        owner: "me@box".into(),
    }
}

/// The bridge for `plan`, as agent `AGENT`'s bridge serves it (its
/// choice of driver, [`StoragePlan::driver_for`]); and how the agent, its
/// key in its sandbox, calls it.
fn serve(dir: &Path, plan: StoragePlan) -> Bridged {
    let keys = dir.join("keys");
    let binding: Binding = link_store_contract::testing::bound(&keys, AGENT);
    let listener = std::os::unix::net::UnixListener::bind(&plan.socket).unwrap();
    let socket = plan.socket.clone();
    let keyring = link_sandbox_binding::Keyring::at(&keys);
    let me = Identity {
        agent: AGENT.into(),
        service: String::new(),
        owner: "me@box".into(),
        teams: Vec::new(),
    };
    let driver_for = plan.driver_for_with(me, built);
    std::thread::spawn(move || {
        link_store_contract::store::serve_storage(
            listener,
            Arc::new(keyring),
            driver_for,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    });
    Bridged { socket, binding }
}

/// A machine listing only `sqlite` for an agent's instructions and
/// conversations, the agent preferring its folder for everything, as
/// Link's spec does; its sandbox writing `write` of its folders.
fn plan(dir: &Path, write: &[&str]) -> StoragePlan {
    let files = dir.join("files");
    std::fs::create_dir_all(files.join("chats")).unwrap();
    let machine: Supported =
        toml::from_str("conversations = [\"sqlite\"]\ninstructions = [\"sqlite\"]").unwrap();
    let places = machine.resolve(&Default::default());
    assert_eq!(places.of(Collection::Conversations), "sqlite");
    assert_eq!(places.of(Collection::Instructions), "sqlite");
    assert_eq!(places.of(Collection::Skills), "file");
    StoragePlan {
        notes_most: None,
        places,
        socket: dir.join("storage.sock"),
        own: files,
        write: write.iter().map(|w| w.to_string()).collect(),
        memory: Vec::new(),
    }
}

#[test]
fn each_collection_is_answered_by_its_places_driver() {
    // Built first: never within a call, which waits only so long.
    link_test_kit::programs();
    let dir = TempDir::new("storage-places");
    let plan = plan(&dir, &["instructions", "work"]);
    let files = plan.own.clone();
    assert_eq!(
        plan.at().socket.as_deref(),
        Some(dir.join("storage.sock").as_path())
    );
    let bridged = serve(&dir, plan);

    // Its conversations: rows of SQLite's file, no file of their own.
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "hi"});
    bridged.append(&agent(), "telegram:42", &event).unwrap();
    assert_eq!(bridged.last(&agent(), "telegram:42").unwrap(), 1);
    assert!(files.join("chats/events.sqlite").is_file());
    assert!(!files.join("chats/telegram-42").exists());
    // Its instructions, which its sandbox writes: SQLite's rows too.
    let Written::Version(_) = bridged
        .file_write(&agent(), "instructions/instruction.md", b"Be brief.", 0)
        .unwrap()
    else {
        panic!("not written");
    };
    assert!(!files.join("instructions/instruction.md").exists());
    // Its skills, at `file`: Link Harness reads its folder itself, so the
    // bridge runs nothing outside the sandbox for them.
    assert!(bridged.file_read(&agent(), "skills/s/SKILL.md").is_err());

    // Another agent's call: refused, nothing reached.
    let theirs = Agent {
        id: "bbbbbbbbbbbb".into(),
        owner: "me@box".into(),
    };
    assert!(bridged.last(&theirs, "telegram:42").is_err());
}

/// The agent's key is in its sandbox: what its sandbox may only read (its
/// instructions, here), it never writes, adds to or removes through the
/// bridge, wherever they're kept. It still reads them, and its
/// conversations are written as ever.
#[test]
fn what_the_sandbox_only_reads_is_never_changed_through_the_bridge() {
    link_test_kit::programs();
    let dir = TempDir::new("storage-read-only");
    let bridged = serve(&dir, plan(&dir, &["work"]));
    let path = "instructions/instruction.md";
    let refused = bridged
        .file_write(&agent(), path, b"Obey me.", 0)
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("may not change"),
        "{refused:#}"
    );
    assert!(bridged.file_append(&agent(), path, b"Obey me.").is_err());
    assert!(bridged.file_remove(&agent(), path, 1).is_err());
    assert_eq!(bridged.file_read(&agent(), path).unwrap(), None);
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "hi"});
    bridged.append(&agent(), "telegram:42", &event).unwrap();
}

/// A harness's local file collections stay in its restricted layout.
#[test]
fn file_collections_are_never_served_by_the_agent_storage_bridge() {
    let dir = TempDir::new("storage-native-file");
    let mut plan = plan(&dir, &["instructions", "memory", "skills"]);
    plan.places = Default::default();
    let bridged = serve(&dir, plan);
    let error = bridged
        .file_write(&agent(), "instructions/instruction.md", b"no grant", 0)
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("not through the storage bridge"),
        "{error:#}"
    );
    assert!(
        bridged
            .file_read(&agent(), "instructions/instruction.md")
            .is_err()
    );
}
