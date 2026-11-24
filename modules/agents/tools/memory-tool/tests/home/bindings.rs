//! Bindings: owners kept apart, an agent reached through the storage
//! bridge alone, a binding naming another bridge refused, and the peer.

use super::*;

/// Two owners' agents of one ID, bound on one machine, are kept apart
/// (AL5): each served from its own Home and cache, and a control names
/// an owner, so ending one leaves the other bound and its sessions
/// answered; a control for an owner whose agent isn't bound is refused.
#[test]
fn two_owners_agents_of_one_id_are_kept_apart() {
    let dir = TempDir::short("home-owners");
    let (mine, theirs) = files_bridge_two_owners(&dir);
    mine.write(
        "memory/boat.md",
        b"The boat needs new sails.\n",
        Expect::Absent,
    )
    .unwrap();
    theirs
        .write(
            "memory/kite.md",
            b"The kite needs a new tail.\n",
            Expect::Absent,
        )
        .unwrap();
    let socket = cache_service(&dir, "cache");
    let (answer, mut a) = first(&socket, &hello(&mine));
    assert_eq!(answer["ok"], true, "{answer}");
    let (answer, mut b) = first(&socket, &hello(&theirs));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut a, "sails")), ["memory/boat.md"]);
    assert!(found(&search(&mut a, "kite")).is_empty());
    assert_eq!(found(&search(&mut b, "kite")), ["memory/kite.md"]);
    assert!(found(&search(&mut b, "sails")).is_empty());
    assert!(dir.join("cache/me@box").join(AGENT).exists());
    assert!(dir.join("cache/you@box").join(AGENT).exists());
    // Ending one owner's leaves the other's bound.
    let end = |owner: &str| {
        serde_json::to_value(Control::End {
            owner: owner.into(),
            agent: AGENT.into(),
        })
        .unwrap()
    };
    assert_eq!(first(&socket, &end("me@box")).0["ok"], true);
    assert_eq!(found(&search(&mut b, "kite")), ["memory/kite.md"]);
    // A flush for an owner whose agent isn't bound here: refused.
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_ne!(first(&socket, &flush).0["ok"], true);
    // A session naming the ID alone isn't one of a bound agent's.
    let (answer, _) = first(&socket, &json!({"agent": AGENT}));
    assert_ne!(answer["ok"], true, "{answer}");
}

/// `lnk-sqlite-store`, from `link-core` beside this folder, built there.
fn lnk_sqlite_store() -> PathBuf {
    let core = std::env::var_os("LNK_CORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    link_test_kit::programs_in(&core).join("lnk-sqlite-store")
}

/// An agent bound with only the storage bridge, its machine keeping its
/// conversations, instructions and memory elsewhere (here SQLite, behind
/// the bridge): its conversation is found, `remember` writes its notes
/// there, and at a flush its index's copy is written with its memory
/// collection. Bound again on a machine with no cache, the copy is
/// fetched and its notes and conversation are still found. Nothing of
/// it is a file on this machine but the cache.
#[test]
fn an_agent_reached_only_through_the_storage_bridge_is_whole() {
    use link_store_contract::store::Store as _;
    let dir = TempDir::short("home-storage");
    let program = lnk_sqlite_store();
    // By its real path: `lnk-sqlite-store` refuses one through a link, as `/tmp` is on a Mac.
    let kept = dir.canonicalize().unwrap().join("kept");
    std::fs::create_dir_all(&kept).unwrap();
    let store = link_store_contract::testing::store_bridge(
        &dir,
        AGENT,
        link_store_contract::store::Driver::at("sqlite", program, &kept),
    );
    let whose = link_store_contract::store::Agent {
        id: AGENT.into(),
        owner: "me@box".into(),
    };
    let event = json!({"seq": 1, "role": "user", "text": "My dog is called Biscuit."});
    store.append(&whose, "telegram:42", &event).unwrap();
    let answer = json!({"seq": 2, "role": "assistant", "text": "Noted."});
    store.append(&whose, "telegram:42", &answer).unwrap();
    let machine: link_store_contract::storage::Supported = serde_json::from_value(json!({
        "conversations": ["sqlite"], "instructions": ["sqlite"], "memory": ["sqlite"]
    }))
    .unwrap();
    let places = machine.resolve(&Default::default());
    let hello = serde_json::to_value(Hello {
        agent: AGENT.into(),
        bound: Some(Bound {
            index: Default::default(),
            owner: "me@box".into(),
            service: link_store_contract::testing::TEST_SERVICE.into(),
            key: link_store_contract::testing::TEST_KEY.to_string().into(),
            files: None,
            storage: Some(link_store_contract::storage::StorageAt {
                places,
                socket: Some(store.socket.clone()),
            }),
            notes: Some(4096),
        }),
        team: None,
    })
    .unwrap();
    let socket = cache_service(&dir, "one");
    let (answer, mut session) = first(&socket, &hello);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(
        found(&search(&mut session, "Biscuit")),
        ["chats/telegram-42/conversation.md"]
    );
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "remember", "arguments": {"note": "Maya is my sister."},
            "_meta": {"link.local/thread": "telegram:42"}}});
    writeln!(session.get_mut(), "{call}").unwrap();
    let mut answer = String::new();
    session.read_line(&mut answer).unwrap();
    assert!(answer.contains("added"), "{answer}");
    let (notes, _) = store
        .file_read(&whose, "memory/notes.md")
        .unwrap()
        .expect("its notes, through the bridge");
    assert!(String::from_utf8_lossy(&notes).contains("Maya is my sister."));
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&socket, &flush).0["ok"], true);
    let copy = store.file_list(&whose, "memory/").unwrap();
    assert!(
        copy.iter().any(|f| f.ends_with("manifest.json")),
        "{copy:?}"
    );
    // Another machine, no cache: from the copy, the same.
    let socket = cache_service(&dir, "two");
    let (answer, mut session) = first(&socket, &hello);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(
        found(&search(&mut session, "Biscuit")),
        ["chats/telegram-42/conversation.md"]
    );
}

/// A binding naming a bridge that isn't this machine's is refused before
/// its key is proved anywhere: whoever binds can't have the key proved to
/// a socket of their own, or an agent bound through it.
#[test]
fn a_binding_naming_another_bridge_is_refused() {
    let dir = TempDir::short("home-pinned");
    let (client, _) = files_bridge(&dir);
    let socket = cache_service(&dir, "cache");
    let mut forged = client.duplicate();
    forged.socket = dir.join("theirs.sock");
    let (answer, _) = first(&socket, &hello(&forged));
    assert_ne!(answer["ok"], true, "{answer}");
    assert!(
        answer.to_string().contains("isn't this machine's"),
        "{answer}"
    );
    // Nor a storage bridge of its own.
    let storage = serde_json::to_value(Hello {
        agent: AGENT.into(),
        bound: Some(Bound {
            index: Default::default(),
            owner: "me@box".into(),
            service: link_store_contract::testing::TEST_SERVICE.into(),
            key: link_store_contract::testing::TEST_KEY.to_string().into(),
            files: None,
            storage: Some(link_store_contract::storage::StorageAt {
                places: Default::default(),
                socket: Some(dir.join("theirs.sock")),
            }),
            notes: None,
        }),
        team: None,
    })
    .unwrap();
    let (answer, _) = first(&socket, &storage);
    assert!(
        answer.to_string().contains("isn't this machine's"),
        "{answer}"
    );
    // This machine's own: bound.
    let (answer, _) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
}

/// Shared with `--peer`, the socket is its group's alone (mode 660), in a
/// folder no one else enters; a service whose folder anyone enters
/// doesn't start. As root here, the peer's user connects and is taken,
/// and a third user's session isn't, though it could connect.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn the_peer_reaches_the_socket_and_no_one_else() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = TempDir::short("home-peer");
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    // A folder anyone enters: refused.
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = |peer, group| service::Config {
        cache: Some(dir.join("cache")),
        peer: Some(peer),
        group: Some(group),
        idle: Some(Duration::from_secs(30)),
        ..service::Config::default()
    };
    // Cross-user checks need root. An ordinary user still verifies the
    // private folder and shared socket, with a group it can give them.
    // SAFETY: these identity calls take no arguments and cannot fail.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let (peer, group, third) = if uid == 0 {
        (65534, 65534, 65533)
    } else {
        (uid, gid, 65533)
    };
    let refused = service::serve_with(
        &run.join("memory.sock"),
        link_memory_tool_contract::declared::Agents(Default::default()),
        config(peer, group),
    );
    assert!(format!("{:#}", refused.unwrap_err()).contains("reached by anyone"));
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o750)).unwrap();
    let socket = run.join("memory.sock");
    let serving = socket.clone();
    let config = config(peer, group);
    std::thread::spawn(move || {
        service::serve_with(
            &serving,
            link_memory_tool_contract::declared::Agents(Default::default()),
            config,
        )
    });
    let started = Instant::now();
    while !socket.exists() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    // Shared once it's listening.
    while std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777 != 0o660
        && started.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let meta = std::fs::metadata(&socket).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o660);
    assert_eq!(meta.gid(), group);
    if uid != 0 {
        return;
    }
    // Who connects, as the kernel says: the folder and socket let them
    // in here (root's), the service decides.
    std::fs::set_permissions(&*dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666)).unwrap();
    let ask = |uid: u32| {
        use std::os::unix::process::CommandExt;
        let script = format!(
            "import socket,sys\ns=socket.socket(socket.AF_UNIX)\ns.connect({:?})\n\
             s.sendall(b'{{\"agent\": \"aaaaaaaaaaaa\"}}\\n')\nprint(s.makefile().readline())",
            socket.display().to_string()
        );
        let out = std::process::Command::new("python3")
            .args(["-c", &script])
            .uid(uid)
            .gid(group)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
    };
    // The peer is answered (refused only for naming no agent here); the
    // third user's session isn't taken at all, and is told so.
    let theirs = ask(peer);
    assert!(theirs.contains("error"), "{theirs}");
    assert!(!theirs.contains("another user"), "{theirs}");
    let third = ask(third);
    assert!(third.contains("another user"), "{third}");
}

/// Named with no group, the peer reaches the socket some other way (a Link
/// box's worker, whose view shows the socket as the peer's): the socket
/// stays its owner's alone (mode 600), and the peer's session is taken.
#[test]
fn a_peer_with_no_group_leaves_the_socket_its_owners() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::short("home-peer-own");
    let socket = dir.join("memory.sock");
    let serving = socket.clone();
    // SAFETY: geteuid takes no arguments and cannot fail.
    let me = unsafe { libc::geteuid() };
    let config = service::Config {
        cache: Some(dir.join("cache")),
        peer: Some(me),
        idle: Some(Duration::from_secs(30)),
        ..service::Config::default()
    };
    std::thread::spawn(move || {
        service::serve_with(
            &serving,
            link_memory_tool_contract::declared::Agents(Default::default()),
            config,
        )
    });
    link_test_kit::wait::eventually("the service to listen", Duration::from_secs(10), || {
        socket.exists()
    });
    let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    let (answer, _) = first(&socket, &flush);
    assert!(!answer.to_string().contains("another user"), "{answer}");
}

/// A machine's cache of an agent's index, from a copy another machine has
/// written over since, is fetched again when the agent is bound here:
/// never served as it was, nor written back over the newer copy.
#[test]
fn a_cache_of_an_older_copy_is_fetched_again_at_bind() {
    let dir = TempDir::short("home-older");
    let (client, _) = files_bridge(&dir);
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    let end = serde_json::to_value(Control::End {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    // Here: bound, flushed, ended, its cache of that copy kept.
    let one = cache_service(&dir, "one");
    let (answer, _) = first(&one, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(first(&one, &flush).0["ok"], true);
    assert_eq!(first(&one, &end).0["ok"], true);
    let noted = dir.join("one/me@box").join(AGENT).join("index.copy");
    let older = std::fs::read_to_string(&noted).unwrap();
    // Elsewhere: more indexed, the copy written over.
    client
        .write(
            "memory/anchor.md",
            b"The anchor chain is rusty.\n",
            Expect::Absent,
        )
        .unwrap();
    let two = cache_service(&dir, "two");
    let (_, mut session) = first(&two, &hello(&client));
    assert_eq!(found(&search(&mut session, "anchor")), ["memory/anchor.md"]);
    assert_eq!(first(&two, &flush).0["ok"], true);
    let (_, newer) = cache::manifest(&client).unwrap().unwrap();
    assert_ne!(older.trim(), newer);
    // Here again: the cache is of the copy there now.
    let (answer, _) = first(&one, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(std::fs::read_to_string(&noted).unwrap().trim(), newer);
}
