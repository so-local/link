//! Which connections are carried and kept out, the held messages each is
//! paired with, and the storage bridge a plan runs.

use super::*;

/// Through the agent's own socket, a file of a collection its sandbox
/// only reads is never changed; through memory's, its `memory`
/// collection and, where its notes are written, its notes are, and
/// nothing else it only reads, another file of its instructions none.
#[test]
fn memory_changes_only_its_own_besides_what_the_agent_writes() {
    use link_store_contract::storage::{Collection, Supported};
    let machine: Supported = toml::from_str(
            "conversations = [\"sqlite\"]\ninstructions = [\"sqlite\"]\nskills = [\"sqlite\"]\nmemory = [\"sqlite\"]",
        )
        .unwrap();
    let plan = StoragePlan {
        notes_most: None,
        places: machine.resolve(&Default::default()),
        socket: PathBuf::from("/s"),
        own: PathBuf::from("/f"),
        write: vec!["work".into()],
        memory: vec!["memory".into(), NOTES.into()],
    };
    let me = link_sandbox_binding::Identity {
        agent: "aaaaaaaaaaaa".into(),
        service: String::new(),
        owner: "me@box".into(),
        teams: Vec::new(),
    };
    let found = |name: &str, dir: &std::path::Path| {
        Ok(link_store_contract::store::Driver::at(
            name,
            PathBuf::from("/x"),
            dir,
        ))
    };
    let write = |path: &str| link_store_contract::store::Request::FileWrite {
        agent: link_store_contract::store::Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        },
        path: path.into(),
        data: String::new(),
        version: 0,
    };
    let agents = plan.driver_for_with(me.clone(), found);
    let memorys = plan.for_memory().driver_for_with(me.clone(), found);
    for (path, collection) in [
        ("memory/link-memory/manifest.json", Collection::Memory),
        ("memory/notes.md", Collection::Memory),
    ] {
        assert!(agents(&me, collection, &write(path)).is_err(), "{path}");
        assert!(memorys(&me, collection, &write(path)).is_ok(), "{path}");
    }
    assert!(memorys(&me, Collection::Skills, &write("skills/s/SKILL.md")).is_err());
    // Notes require their own grant even beside the writable index.
    let mut read_only = plan.clone();
    read_only.memory.retain(|p| p != NOTES);
    let read_only = read_only.for_memory();
    assert!(!read_only.changes(NOTES));
    assert!(read_only.changes("memory/link-memory/manifest.json"));
    // Memory never changes the agent's instructions.
    assert!(
        memorys(
            &me,
            Collection::Instructions,
            &write("instructions/base.md")
        )
        .is_err()
    );
}

/// A held message is given only from whom its connection is paired
/// with, by number or by name.
#[test]
fn a_held_message_is_from_whom_the_connection_is_paired_with() {
    let telegram = serde_json::json!({"bot_token": "1:a", "allowed_users": [42]});
    assert!(paired(&telegram, "42"));
    assert!(!paired(&telegram, "7"));
    let slack = serde_json::json!({"allowed_users": ["U0DANA"]});
    assert!(paired(&slack, "U0DANA"));
    assert!(!paired(&slack, "U0EVE"));
    assert!(!paired(&serde_json::json!({}), "42"));
}

/// A harness starting doesn't answer its first asks: nothing is said
/// until it hasn't answered for a while, and an answer starts the
/// count again.
#[test]
fn a_harness_starting_is_no_error() {
    let start = Instant::now();
    let mut failing = Failing::default();
    for after in [0, 2, 6, 14] {
        assert_eq!(failing.failed(start + Duration::from_secs(after)), None);
    }
    assert_eq!(
        failing.failed(start + Duration::from_secs(30)),
        Some(Duration::from_secs(30))
    );
    let mut failing = Failing::default();
    assert_eq!(failing.failed(start + Duration::from_secs(60)), None);
}
use crate::connections::Connection;

/// Connects `name` with `settings`, its module carrying messages.
fn connect(agent: &mut Settings, name: &str, settings: serde_json::Value, linked: bool) {
    agent.connections.insert(
        name.into(),
        Connection {
            settings: settings.into(),
            about: link_connection_contract::Describe {
                bridged: true,
                linked,
                ..Default::default()
            },
            ..Connection::default()
        },
    );
}

#[test]
fn only_the_connections_the_harness_lacks_are_carried() {
    let mut agent = Settings::new("main");
    connect(
        &mut agent,
        "telegram",
        serde_json::json!({"bot_token": "123:secret", "allowed_users": [42]}),
        false,
    );
    connect(
        &mut agent,
        "slack",
        serde_json::json!({"allowed_users": []}),
        false,
    );
    connect(
        &mut agent,
        "discord",
        serde_json::json!({"allowed_users": []}),
        false,
    );
    assert_eq!(carried(&agent, &[]), vec!["discord", "slack", "telegram"]);
    assert_eq!(
        carried(&agent, &["telegram".into()]),
        vec!["discord", "slack"]
    );
    assert_eq!(
        carried(&agent, &["telegram".into(), "slack".into()]),
        vec!["discord"]
    );
    let all = ["telegram".into(), "slack".into(), "discord".into()];
    assert!(carried(&agent, &all).is_empty());
    // A connection whose module carries nothing: the harness's own only.
    agent.connections.get_mut("discord").unwrap().about.bridged = false;
    assert_eq!(carried(&agent, &[]), vec!["slack", "telegram"]);
    agent.connections.get_mut("discord").unwrap().about.bridged = true;

    let plan = Plan {
        endpoint: "http://127.0.0.1:18800/acp".into(),
        key: "k".to_string().into(),
        proof: true,
        connections: vec!["telegram".into()],
        from: true,
        recover: true,
        tools: true,
        parts: true,
        storage: None,
        files: None,
    };
    let request = request(&agent, &plan, "telegram").unwrap();
    assert!(request.proof && request.parts);
    assert_eq!(request.settings.expose()["bot_token"], "123:secret");
    assert_eq!(request.settings.expose()["allowed_users"][0], 42);
    assert!(request.state.ends_with("agents/main/connections/telegram"));
    assert_eq!(
        super::request(&agent, &plan, "slack")
            .unwrap()
            .settings
            .expose()["allowed_users"],
        serde_json::json!([])
    );
    assert!(super::request(&agent, &plan, "matrix").is_none());

    // A linked connection: its settings hold only numbers.
    connect(
        &mut agent,
        "signal",
        serde_json::json!({"account": "+15557654321", "allowed_users": ["+15551234567"]}),
        true,
    );
    assert!(carried(&agent, &all).contains(&"signal".to_string()));
    let signal = super::request(&agent, &plan, "signal").unwrap();
    assert_eq!(signal.settings.expose()["account"], "+15557654321");
    assert!(signal.state.ends_with("agents/main/connections/signal"));

    let mut handed = agent.for_harness_unwired();
    keep_out(&mut handed, &carried(&agent, &["telegram".into()])).unwrap();
    assert!(handed.telegram.is_some() && handed.slack.is_none());
    assert!(handed.discord.is_none() && handed.signal.is_none());
}

#[test]
fn a_connection_from_outside_link_is_carried_and_kept_out_by_name() {
    let mut agent = Settings::new("main");
    connect(
        &mut agent,
        "matrix",
        serde_json::json!({"token": "syt_secret", "allowed_users": ["@me:example.org"]}),
        false,
    );
    assert_eq!(carried(&agent, &[]), vec!["matrix"]);
    let request = request(&agent, &Plan::default(), "matrix").unwrap();
    assert_eq!(request.settings.expose()["token"], "syt_secret");
    let mut handed = agent.for_harness_unwired();
    assert_eq!(handed.connections["matrix"]["token"], "syt_secret");
    keep_out(&mut handed, &carried(&agent, &[])).unwrap();
    assert!(handed.connections.is_empty());
}

#[test]
fn a_linked_connection_is_carried_for_a_harness_that_speaks_none() {
    let mut agent = Settings::new("main");
    connect(
        &mut agent,
        "signal",
        serde_json::json!({"account": "+15557654321", "allowed_users": ["+15551234567"]}),
        true,
    );
    assert_eq!(carried(&agent, &[]), vec!["signal"]);
    assert!(carried(&agent, &["signal".into()]).is_empty());
    let request = request(&agent, &Plan::default(), "signal").unwrap();
    assert_eq!(
        request.settings.expose()["allowed_users"][0],
        "+15551234567"
    );
    assert!(request.state.ends_with("agents/main/connections/signal"));
    let mut handed = agent.for_harness_unwired();
    assert!(handed.signal.is_some());
    keep_out(&mut handed, &carried(&agent, &[])).unwrap();
    assert!(handed.signal.is_none());
}

/// The store bridge's socket, where the harness put a link: the link
/// is replaced, never followed, what it pointed at is as it was, and
/// the socket is only this user's.
#[test]
fn the_store_bridges_socket_follows_no_link() {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let dir = link_test_kit::TempDir::short("store-link");
    let yours = dir.join("yours");
    std::fs::write(&yours, "keep").unwrap();
    std::fs::set_permissions(&yours, std::fs::Permissions::from_mode(0o644)).unwrap();
    let socket = dir.join(STORE_SOCKET);
    std::os::unix::fs::symlink(&yours, &socket).unwrap();
    let _listener = link_sandbox_bridge::socket::listen_private(&socket).unwrap();
    let mode = std::fs::metadata(&yours).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o644);
    let mode = std::fs::symlink_metadata(&socket)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(std::fs::read_to_string(&yours).unwrap(), "keep");
    assert!(
        std::fs::symlink_metadata(&socket)
            .unwrap()
            .file_type()
            .is_socket()
    );
}

/// Memory's way in failing (here its path too long for a socket, as a
/// long home and owner make it) leaves the store bridge listening: its
/// socket answers, never one bound and let go, which refuses every call.
#[test]
fn memory_failing_leaves_the_store_bridge_listening() {
    let dir = link_test_kit::TempDir::short("store-memory");
    let store = dir.join(STORE_SOCKET);
    let memory = dir.join("m".repeat(120)).join("storage.sock");
    let (listener, memory) = listen_storage(&store, &memory).unwrap();
    assert!(memory.is_none());
    std::os::unix::net::UnixStream::connect(&store).unwrap();
    drop(listener);
    // Both, where memory's path fits.
    let (_listener, memory) = listen_storage(&store, &dir.join("m/storage.sock")).unwrap();
    assert!(memory.is_some());
    std::os::unix::net::UnixStream::connect(dir.join("m/storage.sock")).unwrap();
}

/// The store bridge uses its conversations' folder only while it, and
/// each folder from the files folder down to it, is a folder: a link
/// at `chats/`, or at a folder above it, is refused.
#[test]
fn the_store_bridge_follows_no_linked_folder() {
    let dir = link_test_kit::TempDir::new("store-folder-link");
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let files = dir.join("files");
    let conversations = files.join("chats");
    std::fs::create_dir_all(&conversations).unwrap();
    assert!(real_folders(&conversations, &files).is_ok());
    // Not made yet: fine, the driver makes it.
    assert!(real_folders(&files.join("later"), &files).is_ok());
    // A linked `chats/`.
    std::fs::remove_dir(&conversations).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &conversations).unwrap();
    assert!(real_folders(&conversations, &files).is_err());
    // A linked parent.
    let deeper = dir.join("deep");
    std::fs::create_dir_all(&deeper).unwrap();
    let linked_files = dir.join("linked");
    std::os::unix::fs::symlink(&deeper, &linked_files).unwrap();
    std::fs::create_dir_all(deeper.join("chats")).unwrap();
    assert!(real_folders(&linked_files.join("chats"), &linked_files).is_err());
    // Not inside the files folder at all.
    assert!(real_folders(&elsewhere, &files).is_err());
}

/// A storage bridge is planned only for a harness that writes its
/// folders of the files folder alone: never for one that writes the
/// whole folder, which could put a link where a driver keeps what it
/// keeps. Each collection is answered by its place's driver: `file`'s
/// in the files folder, told whose it is; another's in
/// `chats/`, checked for a link first.
#[test]
fn a_storage_bridge_only_for_a_harness_keeping_the_layout() {
    let mut boxed = crate::adapter::Boxed::for_tests();
    let settings = Settings::default();
    // No layout: it writes the whole folder.
    assert_eq!(StoragePlan::of(&settings, &boxed).unwrap(), None);
    // The layout, but the whole folder written (`.`).
    boxed.folders = Some(link_harness_contract::harness::Folders {
        write: vec![".".into()],
        read: Vec::new(),
        teams: Default::default(),
    });
    assert_eq!(StoragePlan::of(&settings, &boxed).unwrap(), None);
    // Its folders alone: a bridge.
    boxed.folders = Some(link_harness_contract::harness::Folders {
        write: vec!["work".into()],
        read: vec!["instructions".into()],
        teams: Default::default(),
    });
    let machine: link_store_contract::storage::Supported =
        toml::from_str("conversations = [\"sqlite\"]").unwrap();
    let plan = StoragePlan::with(machine.resolve(&Default::default()), &boxed);
    assert_eq!(plan.own, PathBuf::from("/nonexistent/agent"));
    assert_eq!(
        plan.socket,
        PathBuf::from("/nonexistent/home/h").join(STORE_SOCKET)
    );
    let at = plan.at();
    assert_eq!(at.socket.as_deref(), Some(plan.socket.as_path()));
    use link_store_contract::storage::Collection;
    assert_eq!(at.places.of(Collection::Conversations), "sqlite");
    assert_eq!(at.places.of(Collection::Instructions), "file");
    // A folder to check from, always: none checks nothing.
    assert!(real_folders(&PathBuf::from("chats"), &PathBuf::new()).is_err());
}
