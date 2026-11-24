//! A copy then a switch, round trip: what crosses, sealed or refused,
//! what staging holds, and what each machine keeps.

use super::*;

const MOVE: &str = "0123456789abcdef0123456789abcdef";

/// A manifest of `phase` for agent main on harness `fake`, whose state
/// is `~/.fake` but its install, with Signal's link.
fn manifest_for(phase: Phase) -> Manifest {
    let switch = phase == Phase::Switch;
    Manifest {
        format: PROTOCOL.current,
        phase,
        move_id: MOVE.into(),
        removed: Vec::new(),
        version: "test".into(),
        agent: Agent {
            workload: "agent:a1b2c3d4e5f6".parse().unwrap(),
            service: Some("s1s2s3s4s5s6".into()),
            name: "main".into(),
            owner: None,
            teams: Vec::new(),
        },
        harness: "fake".into(),
        model: None,
        fallback: None,
        connected: BTreeMap::new(),
        sandbox: None,
        spec: None,
        schedules: Vec::new(),
        state: match switch {
            true => State {
                include: vec!["~/.fake".into()],
                exclude: vec!["~/.fake/bin".into()],
            },
            false => State::default(),
        },
        connections: match switch {
            true => BTreeMap::from([("signal".to_string(), signal())]),
            false => BTreeMap::new(),
        },
        bucket: None,
    }
}

/// An agent on this machine, in `dir`: its harness's home, Signal's
/// folder, its Home, a conversation in its own folder, and memory's
/// index.
fn an_agent(dir: &Path) -> Parts {
    let write = |path: PathBuf, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let (home, signal_dir, files) = (dir.join("home"), dir.join("signal"), dir.join("files"));
    write(home.join(".fake/memory/fact"), "the sky is blue");
    write(home.join(".fake/bin/fake"), "a program");
    std::os::unix::fs::symlink("memory/fact", home.join(".fake/latest")).unwrap();
    write(signal_dir.join(".signal-cli/data/keys"), "linked");
    write(signal_dir.join(".signal-cli/account"), "+15557654321");
    write(files.join("notes/a.md"), "note a");
    write(files.join("b.txt"), "UNCHANGED-B");
    let own = dir.join("agent");
    write(
        own.join("chats/c1/events.jsonl"),
        "{\"seq\":1,\"event\":1}\n",
    );
    std::os::unix::fs::symlink("b.txt", files.join("link")).unwrap();
    // Memory's index: an SQLite file, copied as SQLite copies it.
    let live = dir.join("memory/link").join(INDEX);
    std::fs::create_dir_all(live.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&live).unwrap();
    db.execute_batch("CREATE TABLE t (x TEXT); INSERT INTO t VALUES ('indexed');")
        .unwrap();
    // And its parts beside it: a file, and a folder of files.
    write(dir.join("memory/link/index.sqlite.codes"), "codes");
    write(
        dir.join("memory/link/index.sqlite.words/meta.json"),
        "words",
    );
    write(dir.join("memory/link/index.sqlite.words/.lock"), "its own");
    let index = index_from(&dir.join("memory/link"), "link", &dir.join("spool")).unwrap();
    Parts {
        harness: home,
        connections: BTreeMap::from([("signal".to_string(), signal_dir)]),
        folder: link_agent_contract::folder::Folder {
            home: files,
            agent: own,
        },
        index,
        spool: dir.join("spool"),
    }
}

fn stream_of(phase: Phase, parts: &Parts, have: &Have) -> Vec<u8> {
    let mut out = Vec::new();
    write(&mut out, manifest_for(phase), parts, have, None, None).unwrap();
    out
}

/// A switch between machines: its secrets cross only sealed to the
/// key its receiver made, and come back whole there; one unsealed, or
/// sealed to another key, is refused.
#[test]
fn a_switchs_secrets_cross_only_sealed_to_its_receiver() {
    let dir = temp("sealed-switch");
    let parts = an_agent(&dir.join("from"));
    let moving = dir.join("moving");
    let copy = stream_of(Phase::Copy, &parts, &Have::new());
    receive_in(&moving, copy.as_slice(), None, |_| Ok(()), false).unwrap();
    let switch = |seal_to: Option<&link_secretive::SealTo>| {
        let mut manifest = manifest_for(Phase::Switch);
        manifest.bucket = Some(serde_json::json!({"password": "hunter2-password"}));
        let mut out = Vec::new();
        write(&mut out, manifest, &parts, &Have::new(), seal_to, None).unwrap();
        out
    };
    let receiver = link_secretive::Recipient::new();
    let sealed = switch(Some(receiver.seal_to()));
    assert!(
        !contains(&sealed, "hunter2"),
        "the password crossed readable"
    );
    assert!(
        !contains(&sealed, "the sky is blue") && !contains(&sealed, "+15557654321"),
        "harness or connection state crossed readable"
    );
    // Unsealed, or sealed to another key: refused, nothing kept.
    for (stream, why) in [
        (switch(None), "sealed stream refused"),
        (
            switch(Some(link_secretive::Recipient::new().seal_to())),
            "sealed stream refused",
        ),
    ] {
        let err = receive_in(
            &moving,
            stream.as_slice(),
            Some(&receiver),
            |_| Ok(()),
            false,
        )
        .err()
        .unwrap();
        assert!(format!("{err:#}").contains(why), "{err:#}");
    }
    let staging_of = || receive_in(&moving, copy.as_slice(), None, |_| Ok(()), false).unwrap();
    // The inner tar, including its END, is intact. A missing or
    // altered outer final tag must still refuse the whole switch.
    let mut altered_final = sealed.clone();
    *altered_final.last_mut().unwrap() ^= 1;
    for damaged in [&sealed[..sealed.len() - 1], &altered_final[..]] {
        staging_of();
        assert!(receive_in(&moving, damaged, Some(&receiver), |_| Ok(()), false).is_err());
        assert!(!moving.join("a1b2c3d4e5f6").exists());
    }
    staging_of();
    let received = receive_in(
        &moving,
        sealed.as_slice(),
        Some(&receiver),
        |_| Ok(()),
        false,
    )
    .unwrap();
    let Received::Switched(incoming) = received else {
        panic!("a switch came back as a copy");
    };
    assert_eq!(
        incoming.manifest.bucket,
        Some(serde_json::json!({"password": "hunter2-password"}))
    );
}

fn contains(bytes: &[u8], text: &str) -> bool {
    bytes.windows(text.len()).any(|w| w == text.as_bytes())
}

/// A copy, then a switch of what changed since: every kind of entry
/// (the harness's state, a connection's link, files and a link among
/// them, a conversation that grew, a file removed, memory's index)
/// is in staging as it is on the machine it left, and the switch
/// carries only what changed.
#[test]
fn a_copy_then_a_switch_round_trips_an_agent() {
    let dir = temp("round-trip");
    let parts = an_agent(&dir.join("from"));
    let moving = dir.join("moving");
    let copy = stream_of(Phase::Copy, &parts, &Have::new());
    // A copy carries no state and no connection: those wait for the switch.
    assert!(!contains(&copy, "the sky is blue") && !contains(&copy, "+15557654321"));
    let received = receive_in(&moving, copy.as_slice(), None, |_| Ok(()), false).unwrap();
    let Received::Copied { have, .. } = received else {
        panic!("a copy came back as a switch");
    };
    assert!(have.contains_key("files/chats/c1/events.jsonl"));
    assert!(have.contains_key(&format!("memory/link/{INDEX}")));
    let staging = moving.join("a1b2c3d4e5f6");
    let record = record_of(&staging).unwrap();
    assert_eq!(record.move_id, MOVE);

    // While it answers: a note removed, one changed, one new, and its
    // conversation grew.
    let files = &parts.folder.home;
    std::fs::remove_file(files.join("notes/a.md")).unwrap();
    std::fs::write(files.join("new.txt"), "NEW-FILE").unwrap();
    std::fs::write(
        parts.folder.agent.join("chats/c1/events.jsonl"),
        "{\"seq\":1,\"event\":1}\n{\"seq\":2,\"event\":2}\n",
    )
    .unwrap();
    let switch = stream_of(Phase::Switch, &parts, &have);
    assert!(
        !contains(&switch, "UNCHANGED-B"),
        "an unchanged file was sent again"
    );
    assert!(contains(&switch, "NEW-FILE") && contains(&switch, "the sky is blue"));
    let received = receive_in(&moving, switch.as_slice(), None, |_| Ok(()), false).unwrap();
    let Received::Switched(incoming) = received else {
        panic!("a switch came back as a copy");
    };
    assert_eq!(incoming.manifest.removed, ["files/notes/a.md"]);
    let read = |p: &str| std::fs::read_to_string(staging.join(p)).unwrap();
    assert_eq!(read("files/b.txt"), "UNCHANGED-B");
    assert_eq!(read("files/new.txt"), "NEW-FILE");
    assert!(read("files/chats/c1/events.jsonl").contains("\"event\":2"));
    assert!(!staging.join("files/notes/a.md").exists());
    assert_eq!(
        std::fs::read_link(staging.join("files/link")).unwrap(),
        Path::new("b.txt")
    );
    assert_eq!(read("harness/.fake/memory/fact"), "the sky is blue");
    assert!(!staging.join("harness/.fake/bin").exists());
    assert_eq!(
        std::fs::read_link(staging.join("harness/.fake/latest")).unwrap(),
        Path::new("memory/fact")
    );
    assert_eq!(read("connections/signal/.signal-cli/data/keys"), "linked");
    // Memory's index, put in place, is the one that was copied.
    let memory = dir.join("to-memory");
    std::fs::create_dir_all(memory.join("link")).unwrap();
    std::fs::write(memory.join("link/index.sqlite.small"), "another index's").unwrap();
    put_index(&staging.join("memory"), |engine| memory.join(engine)).unwrap();
    let db = rusqlite::Connection::open(memory.join("link").join(INDEX)).unwrap();
    let x: String = db.query_row("SELECT x FROM t", [], |r| r.get(0)).unwrap();
    assert_eq!(x, "indexed");
    // Its parts with it, and what was there before of them gone.
    let read = |p: &str| std::fs::read_to_string(memory.join("link").join(p)).unwrap();
    assert_eq!(read("index.sqlite.codes"), "codes");
    assert_eq!(read("index.sqlite.words/meta.json"), "words");
    assert!(!memory.join("link/index.sqlite.words/.lock").exists());
    assert!(!memory.join("link/index.sqlite.small").exists());
}

/// A file changed on the way is refused, its stream with it; a switch
/// refused leaves the other end with nothing of the agent.
#[test]
fn a_tampered_entry_is_refused() {
    let dir = temp("tampered");
    let parts = an_agent(&dir.join("from"));
    let moving = dir.join("moving");
    let mut copy = stream_of(Phase::Copy, &parts, &Have::new());
    let at = copy.windows(11).position(|w| w == b"UNCHANGED-B").unwrap();
    copy[at] = b'X';
    let err = receive_in(&moving, copy.as_slice(), None, |_| Ok(()), false)
        .err()
        .unwrap();
    assert!(format!("{err:#}").contains("SHA-256 differs"), "{err:#}");
    let staging = moving.join("a1b2c3d4e5f6");
    assert!(!staging.join("files/b.txt").exists());

    let mut switch = stream_of(Phase::Switch, &parts, &Have::new());
    let at = switch
        .windows(15)
        .position(|w| w == b"the sky is blue")
        .unwrap();
    switch[at] = b'T';
    let err = receive_in(&moving, switch.as_slice(), None, |_| Ok(()), false)
        .err()
        .unwrap();
    assert!(format!("{err:#}").contains("SHA-256 differs"), "{err:#}");
    assert!(!staging.exists(), "a switch refused left its copy");
}

/// A stream that names another agent than it should is refused;
/// nothing is staged.
#[test]
fn a_stream_of_another_agent_is_refused_before_staging() {
    let dir = temp("another-agent");
    // The receiver's own check refuses before anything is staged.
    let parts = an_agent(&dir.join("from"));
    let copy = stream_of(Phase::Copy, &parts, &Have::new());
    let err = receive_in(
        &dir.join("moving"),
        copy.as_slice(),
        None,
        |_| bail!("not this one"),
        false,
    )
    .err()
    .unwrap();
    assert!(format!("{err:#}").contains("not this one"), "{err:#}");
    assert!(!dir.join("moving/a1b2c3d4e5f6/files").exists());
}

/// A copy cut off on the way keeps what came whole: tried again with
/// what staging holds, it sends only the files that are missing.
#[test]
fn a_second_copy_after_an_interrupted_one_sends_only_the_missing_files() {
    let dir = temp("interrupted");
    let parts = an_agent(&dir.join("from"));
    let moving = dir.join("moving");
    let copy = stream_of(Phase::Copy, &parts, &Have::new());
    // Cut just after the first file of the files folder.
    let names: Vec<(String, u64, u64)> = tar::Archive::new(copy.as_slice())
        .entries()
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let size = e.header().size().unwrap();
            (
                e.path().unwrap().display().to_string(),
                e.raw_file_position(),
                size,
            )
        })
        .collect();
    let (first, at, size) = names
        .iter()
        .find(|(path, _, size)| path.starts_with("files/") && *size > 0)
        .cloned()
        .unwrap();
    let cut = (at + size).next_multiple_of(512) as usize;
    assert!(receive_in(&moving, &copy[..cut], None, |_| Ok(()), false).is_err());
    let staging = moving.join("a1b2c3d4e5f6");
    let have = held(&staging).unwrap();
    assert_eq!(have.keys().collect::<Vec<_>>(), [&first]);

    let again = stream_of(Phase::Copy, &parts, &have);
    let sent: Vec<String> = tar::Archive::new(again.as_slice())
        .entries()
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.header().entry_type() == tar::EntryType::Regular)
        .map(|e| e.path().unwrap().display().to_string())
        .collect();
    assert!(!sent.contains(&first), "{sent:?}");
    let files = names
        .iter()
        .filter(|(path, _, size)| path.starts_with("files/") && *size > 0)
        .count();
    // The manifest, the files missing, the index and its two parts, and
    // the end.
    assert_eq!(sent.len(), files - 1 + 5, "{sent:?}");
    let Received::Copied { have, .. } =
        receive_in(&moving, again.as_slice(), None, |_| Ok(()), false).unwrap()
    else {
        panic!("a copy came back as a switch");
    };
    let Received::Copied { have: expected, .. } = receive_in(
        &dir.join("uninterrupted"),
        copy.as_slice(),
        None,
        |_| Ok(()),
        false,
    )
    .unwrap() else {
        panic!("a copy expected");
    };
    assert_eq!(have, expected);
}

/// What an agent left on a machine seeds its staging there, as copies
/// of its own, when it comes back: what's the same isn't sent again.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn what_an_agent_left_seeds_its_staging() {
    let dir = temp("seeded");
    let parts = an_agent(&dir.join("from"));
    // What it left, as it was then, its Home and its own folder put
    // aside together; one file changed since.
    let kept = dir.join("kept");
    std::fs::create_dir_all(&kept).unwrap();
    for (from, to) in [(&parts.folder.home, "home"), (&parts.folder.agent, "agent")] {
        let copied = std::process::Command::new("cp")
            .arg("-R")
            .arg(from)
            .arg(kept.join(to))
            .status()
            .unwrap();
        assert!(copied.success());
    }
    std::fs::write(parts.folder.home.join("b.txt"), "CHANGED-B").unwrap();
    let staging = dir.join("moving/a1b2c3d4e5f6");
    let left = link_agent_contract::folder::Folder::new(&kept, None);
    seed_files(&left, &staging.join("files")).unwrap();
    // A file of its own: a write to staging's never reaches what it
    // left.
    let event = "chats/c1/events.jsonl";
    let before = std::fs::read(left.seen(event)).unwrap();
    let seeded = std::fs::read(staging.join("files").join(event)).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&seeded).unwrap(),
        serde_json::from_slice::<serde_json::Value>(&before).unwrap()
    );
    let mut appended = std::fs::OpenOptions::new()
        .append(true)
        .open(staging.join("files").join(event))
        .unwrap();
    std::io::Write::write_all(&mut appended, b"later\n").unwrap();
    assert_eq!(std::fs::read(left.seen(event)).unwrap(), before);
    std::fs::write(staging.join("files").join(event), &seeded).unwrap();
    let have = held(&staging).unwrap();
    assert!(have.contains_key("files/chats/c1/events.jsonl"));
    let copy = stream_of(Phase::Copy, &parts, &have);
    let sent: Vec<String> = tar::Archive::new(copy.as_slice())
        .entries()
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.header().entry_type() == tar::EntryType::Regular)
        .map(|e| e.path().unwrap().display().to_string())
        .collect();
    assert_eq!(
        sent,
        [
            "lnk-move.json",
            "files/b.txt",
            "memory/link/index.sqlite.codes",
            "memory/link/index.sqlite.words/meta.json",
            "memory/link/index.sqlite",
            "lnk-move.end"
        ]
    );
}

/// A switch carries its schedules' identities beside its spec, which
/// names none, so each lands the schedule its runner knows.
#[test]
fn a_switch_carries_its_schedules_identities() {
    let kept = crate::schedules::Kept {
        id: link_harness_contract::asp::schedules::ScheduleId("openclaw:inbox".into()),
        revision: 3,
        origin: Some(link_harness_contract::asp::schedules::Origin {
            adapter: "openclaw".into(),
            job: "inbox".into(),
        }),
        definition: link_agent_spec::Schedule {
            name: "Morning inbox".into(),
            when: link_harness_contract::asp::schedules::When::At(
                link_harness_contract::asp::schedules::WhenAt { at: 1_790_000_000 },
            ),
            message: "Summarise client mail".into(),
            to: link_harness_contract::asp::schedules::Destination::Owner(
                link_harness_contract::asp::schedules::DestinationOwner { connection: None },
            ),
            enabled: true,
        },
    };
    let mut manifest = manifest_for(Phase::Switch);
    manifest.spec = Some(
        "schema = 1\n[[schedules]]\nname = \"Morning inbox\"\nmessage = \"Summarise client mail\"\n\
         enabled = true\nwhen = { kind = \"at\", at = 1790000000 }\nto = { kind = \"owner\" }\n"
            .into(),
    );
    manifest.schedules = vec![kept.clone()];
    let read = read_manifest(serde_json::to_value(&manifest).unwrap()).unwrap();
    assert_eq!(read.schedules, std::slice::from_ref(&kept));
    assert!(!read.spec.unwrap().contains("revision"));
    // Where it lands, the record holds them as they were.
    crate::schedules::put_record("main", &read.schedules).unwrap();
    let mut settings = Settings::new("main");
    settings.id = Some(crate::settings::ids::workload("a1b2c3d4e5f6").unwrap());
    assert_eq!(crate::schedules::record(&settings).unwrap(), [kept]);
}

#[test]
fn a_manifest_reads_with_or_without_connections() {
    let old = r#"{"format": 4, "phase": "switch",
            "move": "0123456789abcdef0123456789abcdef", "version": "0.11.0",
            "agent": {"workload": "agent:a1b2c3d4e5f6", "name": "main"}, "harness": "fake",
            "model": null,
            "state": {"include": ["~/.fake"], "exclude": []}}"#;
    let read_text = |text: &str| read_manifest(serde_json::from_str(text).unwrap());
    assert!(read_text(old).unwrap().connections.is_empty());

    let mut manifest: serde_json::Value = serde_json::from_str(old).unwrap();
    manifest["connections"] = serde_json::json!({"signal": signal()});
    assert_eq!(
        read_manifest(manifest.clone()).unwrap().connections["signal"],
        signal()
    );
    // A path out of the connection's folder: the stream is refused.
    for bad in ["~/../x", "/etc/passwd", "~/./x"] {
        manifest["connections"] = serde_json::json!({"signal": {"include": [bad]}});
        assert!(read_manifest(manifest.clone()).is_err(), "{bad}");
    }
    // A copy's: the agent and its harness alone.
    let copy = r#"{"format": 4, "phase": "copy",
            "move": "0123456789abcdef0123456789abcdef", "version": "0.15.0",
            "agent": {"workload": "agent:a1b2c3d4e5f6", "name": "main"}, "harness": "fake"}"#;
    let copy = read_text(copy).unwrap();
    assert_eq!(copy.phase, Phase::Copy);
    assert!(copy.model.is_none() && copy.state.include.is_empty());
}

#[test]
fn a_connection_is_what_this_machine_says_of_it() {
    use crate::connections::Connection;
    use link_connection_contract::Describe;
    let sent = Describe {
        title: "\u{1b}[2JTelegram".into(),
        secret_fields: vec![],
        cli: true,
        ..Describe::default()
    };
    let came: BTreeMap<String, Connection> = ["telegram", "box", "Bad/Name", "matrix"]
        .into_iter()
        .map(|name| {
            let connection = Connection {
                settings: serde_json::json!({"bot_token": "123:secret"}).into(),
                about: sent.clone(),
                ..Connection::default()
            };
            (name.to_string(), connection)
        })
        .collect();
    // Only Telegram's module is here; a name that isn't a connection's
    // never runs one.
    let here = |name: &str| {
        assert_ne!(name, "Bad/Name");
        (name == "telegram").then(|| Describe {
            title: "Telegram".into(),
            secret_fields: vec!["bot_token".into()],
            ..Describe::default()
        })
    };
    let (kept, left_out) = described_here(came, here);
    assert_eq!(kept.keys().collect::<Vec<_>>(), ["telegram"]);
    let about = &kept["telegram"].about;
    assert_eq!(about.title, "Telegram");
    assert_eq!(about.secret_fields, ["bot_token"]);
    assert!(!about.cli);
    assert_eq!(
        kept["telegram"].settings.expose()["bot_token"],
        "123:secret"
    );
    assert_eq!(left_out, ["Bad/Name", "box", "matrix"]);
}

#[test]
fn paths_stay_in_the_box() {
    let dir = Path::new("/box");
    assert_eq!(
        in_box(dir, &["~/.fake/x".into()]).unwrap(),
        [PathBuf::from("/box/.fake/x")]
    );
    for bad in ["/etc", "~/../escape", "~/./x", ".fake"] {
        assert!(in_box(dir, &[bad.into()]).is_err(), "{bad}");
    }
}

#[test]
fn each_machine_keeps_a_model_it_can_reach() {
    let local = Model {
        provider: "ollama".into(),
        name: Some("qwen3:8b".into()),
        base_url: Some("http://localhost:11434".into()),
        ..Model::default()
    };
    let hosted = Model {
        provider: "anthropic".into(),
        api_key: "sk-ant-x".to_string().into(),
        ..Model::default()
    };
    let up = |_: &Model| true;
    let down = |_: &Model| false;
    // Back on the Mac, its own local model again.
    assert_eq!(
        settle_model(Some(&local), Some(&hosted), None, true, up).unwrap(),
        local
    );
    // Its local model is off: the hosted one that came.
    assert_eq!(
        settle_model(Some(&local), Some(&hosted), None, true, down).unwrap(),
        hosted
    );
    // To a box, from a Mac's local model: only with a key.
    let err = settle_model(None, Some(&local), None, true, down).unwrap_err();
    assert!(err.to_string().contains("ANTHROPIC_API_KEY"), "{err}");
    assert_eq!(
        settle_model(None, Some(&local), Some(hosted.duplicate()), true, down).unwrap(),
        hosted
    );
    // From a box, a model on this machine's loopback isn't taken,
    // whatever answers there.
    let err = settle_model(None, Some(&local), None, false, up).unwrap_err();
    assert!(err.to_string().contains("ANTHROPIC_API_KEY"), "{err}");
}

/// A stream's `[commands] allow = ["*"]`, every program, is the machine
/// it came from's word, as a path is: left out, said as the flag that
/// gives it.
#[test]
fn a_stream_never_allows_every_program() {
    let came = Sandbox::parse("schema = 1\n[commands]\nallow = [\"*\"]\n").unwrap();
    let (kept, left_out) = settle_sandbox(None, None, Some(&came), &|_, _, _| None);
    let kept = kept.unwrap();
    assert!(
        kept.commands().is_none_or(|c| c.allow.is_empty()),
        "{:?}",
        kept.commands()
    );
    assert_eq!(left_out, ["--commands.allow '*'"]);
}

/// What a stream says the agent may do opens nothing of this machine
/// itself: that comes from here, or the machine running the move.
#[test]
fn a_stream_never_opens_this_machine() {
    let came = Sandbox::parse(
            "schema = 1\n[files]\nwrite = [\"work\"]\nread = [\"~\"]\n[network]\nmode = \"host\"\n\
             [internet]\nhosts = [\"api.anthropic.com\"]\ntcp = [\"github.com:22\"]\nper_minute = 60\n\
             [lan]\nhosts = [\"*\"]\n[localhost]\nports = [5432]\n[tools]\nnames = [\"gh\"]\n\
             [browser]\nhosts = [\"*\"]\n[commands]\nallow = [\"python3\", \"/opt/tool\"]\n\
             [teams.t]\nwrite = [\".\"]\n",
        )
        .unwrap();
    let (kept, left_out) = settle_sandbox(None, None, Some(&came), &|_, _, _| None);
    let kept = kept.unwrap();
    // What narrows is kept; what reaches past the sandbox isn't.
    let internet = kept.spec.internet.as_ref().unwrap();
    assert_eq!(internet.hosts.as_ref().unwrap(), &["api.anthropic.com"]);
    assert_eq!(internet.per_minute, Some(60));
    assert!(internet.tcp.is_none());
    assert!(kept.spec.network.is_none() && kept.spec.lan.is_none());
    assert!(kept.spec.localhost.is_none() && kept.spec.tools.is_none());
    assert!(kept.spec.browser.is_none() && !kept.reads_home());
    assert!(kept.spec.teams.is_empty(), "a team's folder only from here");
    assert_eq!(kept.folders().write, ["work"]);
    assert_eq!(kept.commands().unwrap().allow, ["python3"]);
    assert_eq!(
        left_out,
        [
            "--files.read '~'",
            "--network.mode host",
            "--internet.tcp github.com:22",
            "--lan.hosts '*'",
            "--localhost.ports 5432",
            "--tools.names gh",
            "--browser.hosts '*'",
            "--commands.allow /opt/tool",
            "team t"
        ]
    );
    let here = Sandbox::parse("schema = 1\n[files]\nread = [\"~\"]\n").unwrap();
    let none: TeamGrant<'_> = &|_, _, _| None;
    assert_eq!(
        settle_sandbox(None, Some(&here), Some(&came), none),
        (Some(here.clone()), vec![])
    );
    // Given, it's what the machine running the move gave, but a team's
    // folder this machine keeps no member of.
    let mut given = came.clone();
    given.spec.teams = Default::default();
    assert_eq!(
        settle_sandbox(Some(&came), Some(&here), None, none),
        (Some(given), vec![])
    );
    assert_eq!(settle_sandbox(None, None, None, none), (None, vec![]));
}

/// A team's folder is derived again where the agent arrives, never copied:
/// kept only for a team that machine keeps it a member of, as that
/// machine derives it, whether the machine running the move gave it or a
/// stream says it; a grant it can't derive is left out and said.
#[test]
fn a_teams_folder_is_derived_again_where_it_arrives() {
    use link_harness_contract::harness::TeamFolders;
    let came = Sandbox::parse(
        "schema = 1\n[teams.research]\nwrite = [\"drafts\"]\n[teams.design]\nwrite = [\".\"]\n\
         [teams.old]\nwrite = [\".\"]\n",
    )
    .unwrap();
    let notes = TeamFolders {
        write: Vec::new(),
        read: vec!["notes".into()],
    };
    // research keeps an own member's vouched grant; design, an outside
    // member's, grants what its roster says; old isn't kept here at all.
    let derive = |team: &str, came: &TeamFolders, vouched: bool| match team {
        "research" => vouched.then(|| came.clone()),
        "design" => Some(notes.clone()),
        _ => None,
    };
    let (kept, left_out) = settle_sandbox(None, None, Some(&came), &derive);
    let kept = kept.unwrap();
    assert_eq!(
        kept.spec.teams,
        std::collections::BTreeMap::from([("design".to_string(), notes.clone())])
    );
    assert_eq!(left_out, ["team old", "team research"]);
    let (given, left_out) = settle_sandbox(Some(&came), None, None, &derive);
    let given = given.unwrap();
    assert_eq!(given.spec.teams["research"].write, ["drafts"]);
    assert_eq!(given.spec.teams["design"], notes);
    assert!(!given.spec.teams.contains_key("old"));
    assert!(left_out.is_empty());
}
