//! A cloud that hosts agents: chats through it, a move there and back,
//! and SQLite state moving to a host and back.

use super::*;

#[test]
fn create_on_a_host_makes_no_local_agent_and_the_list_finds_its_id() {
    let world = World::new("create-hosted");
    let created = ok(&world.lnk_agent(&["create", "work", "--cloud", "host", "--yes"]));
    assert!(created.contains("Created work on host"), "{created}");
    assert!(!world.here.join("Link/Safe/Agents/work/agent.toml").exists());
    let listed = ok_stdout(&world.lnk_agent(&["list", "--json"]));
    let listed: serde_json::Value = serde_json::from_str(listed.lines().last().unwrap()).unwrap();
    let work = listed["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "work" && a["location"].is_null())
        .unwrap();
    link_test_kit::ids::assert_named(&listed);
    let id = work["workload"]
        .as_str()
        .and_then(|w| w.strip_prefix("agent:"))
        .unwrap();
    link_machine::machine::check_id(id).unwrap();
    assert!(
        world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{id}.toml"))
            .exists()
    );
    assert_eq!(work["machine"]["cloud"], true);
}

/// An agent a cloud hosts, and a backup it keeps, are billed by that
/// cloud, so each is made only after a yes: with no terminal to ask and no
/// `--yes`, `lnk agent create --cloud`, `lnk agent move --cloud` and `lnk
/// agent backup` of a hosted agent make nothing there, naming what they'd
/// make and `--yes`; with `--yes`, each goes ahead.
#[test]
fn nothing_a_host_bills_is_made_without_a_yes() {
    let world = World::new("hosted-billed");
    let refused = |args: &[&str], what: &str, command: &str| {
        let out = world.lnk_agent(args);
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{said}");
        assert!(
            said.contains(what) && said.contains("billed by that cloud") && said.contains("--yes"),
            "{said}"
        );
        assert!(
            !host_log(&world).iter().any(|c| c == command),
            "{:?}",
            host_log(&world)
        );
    };
    refused(
        &["create", "work", "--cloud", "host"],
        "Make the agent work in host",
        "agent-create",
    );
    ok(&world.lnk_agent(&["create", "work", "--cloud", "host", "--yes"]));
    an_agent_with_files(&world);
    refused(
        &["move", "--cloud", "host"],
        "Make the agent main in host",
        "agent-in",
    );
    assert!(world.files(&world.here).join("notes.txt").exists());
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    refused(
        &["backup", "--agent", "main"],
        "Make a backup of main in host",
        "agent-backup",
    );
    ok(&world.lnk_agent(&["backup", "--agent", "main", "--yes"]));
    assert_eq!(
        host_log(&world)
            .iter()
            .filter(|c| ["agent-create", "agent-in", "agent-backup"].contains(&c.as_str()))
            .collect::<Vec<_>>(),
        ["agent-create", "agent-in", "agent-backup"]
    );
}

#[test]
fn connect_telegram_on_a_host_sends_the_same_fields_without_a_local_agent() {
    let world = World::new("connect-hosted");
    ok(&world.lnk_agent(&["create", "work", "--cloud", "host", "--yes"]));
    let said = ok(&world
        .lnk_agent_command(&["connect", "telegram", "--agent", "work"])
        .env("LNK_TELEGRAM_TOKEN", "12345:abcdefghijklmnopqrstuvwxyz")
        .env("LNK_TELEGRAM_USER", "123")
        .output()
        .unwrap());
    assert!(said.contains("Telegram connected"), "{said}");
    assert!(!said.contains("12345:abcdefghijklmnopqrstuvwxyz"));
    let connected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(world.root.join("host/connected.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(connected["connection"], "telegram");
    assert_eq!(connected["token"], "12345:abcdefghijklmnopqrstuvwxyz");
    assert_eq!(connected["user"], "123");
    assert!(!world.here.join("Link/Safe/Agents/work/agent.toml").exists());
}

#[test]
fn a_managed_host_gets_no_model_credentials_and_the_source_restores_them() {
    let world = World::new("managed-hosted-models");
    an_agent_with_files(&world);
    write(&world.root.join("host/managed-models"), "");
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    let manifest =
        std::fs::read_to_string(world.root.join(format!("host/agents/{ID}/manifest.json")))
            .unwrap();
    assert!(!manifest.contains("sk-ant-test"));
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert!(manifest["model"].is_null());
    assert!(manifest["fallback"].is_null());
    let checkpoint = world
        .here
        .join(format!(".lnk/safe/agents/transfers/moved/{ID}.models"));
    assert!(checkpoint.is_file());
    assert!(
        !std::fs::read_to_string(&checkpoint)
            .unwrap()
            .contains("sk-ant-test")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&checkpoint).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    ok(&world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]));
    assert!(!checkpoint.exists());
    assert_eq!(
        crate::common::kept_key(
            &world.here,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
    let restored = world.lnk_agent(&["state-send", "--phase", "switch", "--agent", "main"]);
    // A switch still requires its receiver: restored keys never print on stdout.
    assert!(!restored.status.success());
    assert!(!String::from_utf8_lossy(&restored.stdout).contains("sk-ant-test"));
}

/// From a box to a cloud account that hosts agents: the key made for
/// that box to reach the agent's buckets is revoked, as one coming back
/// here is, so the box it left reaches them no more.
#[test]
fn a_box_an_agent_leaves_for_a_host_loses_its_bucket_key() {
    let world = World::new("box-to-host-buckets");
    an_agent_with_files(&world);
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--agent", "main", "--yes"]));
    let buckets = std::fs::read_to_string(world.root.join("buckets")).unwrap();
    assert!(
        buckets.ends_with(&format!("storage revoke --owner agent:{ID}\n")),
        "{buckets}"
    );
}

#[test]
fn a_controller_notes_a_model_key_at_its_box_after_a_managed_move() {
    let world = World::new("managed-hosted-from-box");
    an_agent_with_files(&world);
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    write(&world.root.join("host/managed-models"), "");
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--agent", "main", "--yes"]));
    let own = crate::common::own_secrets(
        &world.here,
        ID,
        &link_agent_identity::own::OwnSecret::Model.to_use().unwrap(),
    );
    let copies = own[0].1["copies"].as_array().unwrap();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0]["name"].as_str(), Some("b1"));
    assert!(
        world
            .there
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.models"))
            .is_file()
    );
    let manifest =
        std::fs::read_to_string(world.root.join(format!("host/agents/{ID}/manifest.json")))
            .unwrap();
    assert!(!manifest.contains("sk-ant-test"));
}

#[test]
fn a_model_key_removed_while_hosted_is_not_restored_by_a_move_back() {
    let world = World::new("managed-hosted-revoked-model");
    an_agent_with_files(&world);
    write(&world.root.join("host/managed-models"), "");
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    let removed =
        link_test_kit::env::command(link_test_kit::programs().join("lnk-secret"), &world.here)
            .args([
                "delete",
                "--here",
                "--service",
                &format!("service:{}", crate::common::service_of(ID)),
                "--own",
                "model",
            ])
            .env_clear()
            .env("HOME", &world.here)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&world.here),
            )
            .env("PATH", "/usr/bin:/bin")
            .env("LNK_SERVICES", "off")
            .output()
            .unwrap();
    ok(&removed);
    let back = world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]);
    assert!(!back.status.success());
    let said = String::from_utf8_lossy(&back.stderr);
    assert!(said.contains("model credential was removed"), "{said}");
    assert!(!said.contains("sk-ant-test"));
    assert!(
        crate::common::kept_key(
            &world.here,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .is_none()
    );
}

#[test]
fn removing_a_source_after_an_unfinished_managed_send_releases_its_model_key() {
    let world = World::new("managed-source-removed");
    an_agent_with_files(&world);
    let unfinished = world.lnk_agent(&[
        "state-send",
        "--phase",
        "switch",
        "--without-model-credentials",
    ]);
    assert!(!unfinished.status.success());
    let checkpoint = world
        .here
        .join(format!(".lnk/safe/agents/transfers/moved/{ID}.models"));
    assert!(checkpoint.is_file());
    ok(&world.lnk_agent(&["delete", "main", "--yes", "--delete-files"]));
    assert!(!checkpoint.exists());
    assert!(
        crate::common::kept_key(
            &world.here,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .is_none()
    );
}

/// An agent a cloud account hosts is talked to through that account's
/// adapter (`agent-chat`), on the `link` connection's threads: its answer
/// as it comes, a message that joined a running turn and a refusal said,
/// and nothing held for the next chat.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_hosted_agent_is_chatted_with_through_its_cloud() {
    let world = World::new("hosted-chat");
    an_agent_with_files(&world);
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    let said = ok(&world.lnk_agent(&[
        "chat", "send", "--agent", "main", "-m", "hi there", "--json",
    ]));
    let chatted: serde_json::Value = serde_json::from_str(said.lines().last().unwrap()).unwrap();
    assert_eq!(chatted["thread"], "link:main");
    assert_eq!(chatted["answer"], "you said: hi there");
    assert_eq!(chatted["held"], serde_json::json!([]));
    // Without --json, the answer as it comes, on its own thread.
    let said = ok(&world.lnk_agent(&[
        "chat", "send", "--agent", "main", "--chat", "work", "-m", "and now",
    ]));
    assert!(said.contains("you said: and now"), "{said}");
    let chats = std::fs::read_to_string(world.root.join("host/chats.jsonl")).unwrap();
    let chats: Vec<serde_json::Value> = chats
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(chats.len(), 2);
    assert_eq!(chats[0]["agent"], ID);
    assert_eq!(chats[0]["thread"], "link:main");
    assert_eq!(chats[0]["text"], "hi there");
    assert_eq!(chats[1]["thread"], "link:work");
    // A thread that isn't one is refused before the cloud is asked.
    let bad = world.lnk_agent(&[
        "chat", "send", "--agent", "main", "--chat", "a b", "-m", "hi",
    ]);
    assert!(!bad.status.success());
    assert_eq!(
        std::fs::read_to_string(world.root.join("host/chats.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    // A message that joined a running turn: its answer stays there.
    write(&world.root.join("host/chat-steered"), "");
    let steered = world.lnk_agent(&["chat", "send", "--agent", "main", "-m", "more"]);
    assert!(!steered.status.success());
    let said = String::from_utf8_lossy(&steered.stderr);
    assert!(
        said.contains("joined the turn already running in link:main"),
        "{said}"
    );
    std::fs::remove_file(world.root.join("host/chat-steered")).unwrap();
    // Refused there: why, as the cloud says it.
    write(
        &world.root.join("host/chat-refused"),
        r#"{"why": "main is moving"}"#,
    );
    let refused = world.lnk_agent(&["chat", "send", "--agent", "main", "-m", "hi"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("main is moving"), "{said}");
    std::fs::remove_file(world.root.join("host/chat-refused")).unwrap();
    // Ctrl-C while it answers: the chat stops waiting, says so, and its
    // adapter ends with it.
    write(&world.root.join("host/chat-stalls"), "");
    let mut chatting = world
        .lnk_agent_command(&["chat", "send", "--agent", "main", "-m", "slowly"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The stand-in makes its pid file, then writes its pid: the file's
    // being there is not yet the pid.
    let pid_file = world.root.join("host/chat.pid");
    let adapter: i32 = link_test_kit::eventually(
        "the adapter's pid",
        std::time::Duration::from_secs(60),
        || std::fs::read_to_string(&pid_file).ok()?.trim().parse().ok(),
    );
    // SAFETY: a signal to the child this test started.
    unsafe { libc::kill(chatting.id() as i32, libc::SIGINT) };
    let started = std::time::Instant::now();
    while chatting.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the chat waits on its adapter"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let stopped = chatting.wait_with_output().unwrap();
    assert_eq!(stopped.status.code(), Some(130));
    let said = String::from_utf8_lossy(&stopped.stderr);
    assert!(said.contains("in its conversation where it runs"), "{said}");
    // SAFETY: signal 0 only asks whether the process is there.
    let gone = || unsafe { libc::kill(adapter, 0) } != 0;
    let started = std::time::Instant::now();
    while !gone() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the adapter lives on"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::fs::remove_file(world.root.join("host/chat-stalls")).unwrap();
    // A host whose adapter fails: said, nonzero.
    write(&world.root.join("host/fail-agent-chat"), "no chat today\n");
    let failed = world.lnk_agent(&["chat", "send", "--agent", "main", "-m", "hi"]);
    assert!(!failed.status.success());
    let said = String::from_utf8_lossy(&failed.stderr);
    assert!(said.contains("agent-chat failed"), "{said}");
}

/// To a cloud account that hosts agents and back: the copy and the
/// switch go through its adapter, and every file comes back the same.
#[test]
fn an_agent_moves_to_a_cloud_that_hosts_agents_and_back() {
    let world = World::new("hosted");
    an_agent_with_files(&world);
    let before = every_file(&world.files(&world.here));
    let said = ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    assert!(said.contains("Copied 4 files"), "{said}");
    assert!(said.contains("Switched in "), "{said}");
    assert!(
        said.contains("main runs on your cloud account host now"),
        "{said}"
    );
    assert_eq!(
        host_log(&world),
        ["agent-in", "agent-receive copy", "agent-receive switch"]
    );
    let hosted = world.root.join(format!("host/agents/{ID}"));
    let mut received = every_file(&hosted.join("files"));
    let events = received.remove("chats/telegram-42/events.jsonl").unwrap();
    assert!(events.contains("\"seq\":1"));
    received.remove("chats/telegram-42/conversation.md");
    let mut before = before;
    before.remove("chats/telegram-42/events.jsonl");
    assert_eq!(received, before);
    assert_eq!(
        std::fs::read_to_string(hosted.join("harness/.fake/memory/first")).unwrap(),
        "remembered"
    );
    // No key, upstream or grants for it there: its manifest only.
    let manifest = std::fs::read_to_string(hosted.join("manifest.json")).unwrap();
    assert!(!manifest.contains("upstream"), "{manifest}");
    // Here: gone, with a note that it's there; the list finds it there.
    assert!(!world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    let note = std::fs::read_to_string(
        world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml")),
    )
    .unwrap();
    assert!(note.contains("cloud = true"), "{note}");
    let listed = ok_stdout(&world.lnk_agent(&["list", "--json"]));
    let listed: serde_json::Value = serde_json::from_str(listed.lines().last().unwrap()).unwrap();
    let main = listed["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["workload"] == format!("agent:{ID}") && a["location"].is_null())
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(main["machine"]["name"], "host");
    assert_eq!(main["machine"]["cloud"], true);
    // The account answered with its agents, said; whether it hosts them,
    // asked of its adapter (by the move, and once by the list), is kept:
    // no list asks again.
    assert_eq!(
        listed["answered"],
        serde_json::json!([{"name": "host", "cloud": true}])
    );
    let asked = || {
        std::fs::read_to_string(world.root.join("host/log"))
            .unwrap()
            .lines()
            .filter(|l| l.trim() == "agent-runs")
            .count()
    };
    let asked_first = asked();
    // An answer without its agents says nothing of them: the account is
    // unreached, never answered.
    write(&world.root.join("host/silent-agent-list"), "");
    let listed = ok_stdout(&world.lnk_agent(&["list", "--json"]));
    let listed: serde_json::Value = serde_json::from_str(listed.lines().last().unwrap()).unwrap();
    assert_eq!(listed["answered"], serde_json::json!([]));
    assert!(
        listed["unreached"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["name"] == "host" && u["cloud"] == true),
        "{listed}"
    );
    std::fs::remove_file(world.root.join("host/silent-agent-list")).unwrap();
    assert_eq!(asked(), asked_first);
    // It runs no programs there: `run` is refused, saying why and how
    // to get it back.
    let run = world.lnk_agent(&["tool", "add", "run", "--agent", "main"]);
    assert!(!run.status.success());
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("without running programs") && said.contains("lnk agent move here"),
        "{said}"
    );
    // Backed up there, never stopped; restored there to that backup.
    write(&hosted.join("files/work/plan.md"), "first plan");
    let backed = ok(&world.lnk_agent(&["backup", "--agent", "main", "--yes"]));
    assert!(backed.contains("Backed up main on"), "{backed}");
    write(&hosted.join("files/work/plan.md"), "second plan");
    let restored = ok(&world.lnk_agent(&["restore", "main", "--yes"]));
    assert!(restored.contains("Restored main on"), "{restored}");
    assert_eq!(
        std::fs::read_to_string(hosted.join("files/work/plan.md")).unwrap(),
        "first plan"
    );
    // A host that keeps no backups: said, with what does.
    write(&world.root.join("host/no-backups"), "");
    let none = world.lnk_agent(&["backup", "--agent", "main", "--yes"]);
    assert!(!none.status.success());
    let said = String::from_utf8_lossy(&none.stderr);
    assert!(said.contains("keeps no backups"), "{said}");
    std::fs::remove_file(world.root.join("host/no-backups")).unwrap();
    assert_eq!(
        host_log(&world)[3..],
        ["agent-backup", "agent-restore", "agent-backup"]
    );
    std::fs::remove_file(hosted.join("files/work/plan.md")).unwrap();

    // There it learns something and gets a new file; back here, both come.
    // A message came for it while it was paused there: handed to it here.
    // One held for another move's agent, not this one's generation, never
    // is.
    write(
        &world.root.join("host/held.json"),
        r#"[{"connection": "telegram", "chat": "42", "from": "42", "text": "held"},
            {"connection": "telegram", "chat": "42", "from": "42", "text": "stale",
             "generation": "ffffffffffffffffffffffffffffffff"}]"#,
    );
    write(&hosted.join("files/memory/learned.md"), "learned there");
    write(
        &hosted.join("harness/.fake/memory/second"),
        "learned there too",
    );
    let said = ok(&world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]));
    assert!(said.contains("Switched in "), "{said}");
    // What it left here 0 days ago started the copy: only the new file came.
    assert!(said.contains("Copied 1 file,"), "{said}");
    // Not started here (`--no-start`), nothing carries its connections to
    // give it the message: kept with its settings, only yours to read,
    // for its next start; none left on the way.
    assert!(
        said.contains("1 message(s) wait for main: given when it starts."),
        "{said}"
    );
    let kept = world.here.join(".lnk/safe/agents/main/held.jsonl");
    let held = std::fs::read_to_string(&kept).unwrap();
    assert!(
        held.contains(r#""chat":"42""#) && held.contains(r#""text":"held""#),
        "{held}"
    );
    assert!(
        said.contains("was held for another move of main: not given here")
            && said.contains("stale"),
        "{said}"
    );
    assert!(!held.contains("stale"), "{held}");
    // Only the generation it arrived by takes it.
    let generation =
        std::fs::read_to_string(world.here.join(".lnk/safe/agents/main/generation")).unwrap();
    assert!(
        held.contains(&format!(r#""generation":"{generation}""#)),
        "{held}"
    );
    // Given again to it, the stale one is still refused.
    let stale = world
        .lnk_agent_command(&["deliver", "--agent", "main"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(
                br#"{"connection":"telegram","chat":"42","from":"42","text":"stale","generation":"ffffffffffffffffffffffffffffffff"}
"#,
            )?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(text(&stale).contains("not given here"), "{}", text(&stale));
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), held);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&kept).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        !world
            .here
            .join(".lnk/safe/agents/transfers/held/main.jsonl")
            .exists()
    );
    // Asked again before it starts: still kept, once.
    let again = ok(&world.lnk_agent(&["deliver", "--agent", "main"]));
    assert!(again.contains("0 message(s) wait for main"), "{again}");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), held);
    assert_eq!(
        host_log(&world)[6..],
        [
            "agent-out",
            "agent-send copy",
            "agent-send switch",
            "agent-left"
        ]
    );
    let mut expected = before;
    expected.insert("memory/learned.md".into(), "learned there".into());
    // Its conversation, kept in Home before, back in its own folder.
    let conversation = "chats/telegram-42/events.jsonl";
    expected.remove("chats/telegram-42/conversation.md");
    assert_eq!(every_file(&world.files(&world.here)), expected);
    let own = every_file(&world.here.join("Link/Agents/main/agent"));
    // The same events, line by line, whatever order their keys were
    // written in: a key's place in an object is no part of it.
    let lines = |text: &str| -> Vec<serde_json::Value> {
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    };
    assert_eq!(lines(&own[conversation]), lines(&events));
    assert_eq!(
        std::fs::read_to_string(world.agent(&world.here).join("memory/second")).unwrap(),
        "learned there too"
    );
    assert!(!hosted.exists());
    assert!(
        !world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists()
    );
    let local =
        std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    let identity =
        std::fs::read_to_string(world.here.join(".lnk/safe/agents/main/identity.toml")).unwrap();
    assert!(
        identity.contains(&format!("id = \"{ID}\"")),
        "{local} {identity}"
    );
}

/// The command line uses the same collection transfer as a hosted service.
#[test]
fn sqlite_conversations_notes_and_schedules_move_to_a_host_and_back() {
    use link_store_contract::store::{Agent, Driver, Job, Place, Store};
    let world = World::new("sqlite-hosted");
    an_agent_with_files(&world);
    let config = world.here.join("Link/Safe/Agents/main/agent.toml");
    crate::common::set_record(
        &world.here,
        "main",
        "identity.toml",
        "owner",
        serde_json::json!({"kind": "machine-user", "user": "tester", "machine": "computer"}),
    );
    let own = world.here.join("Link/Agents/main/agent");
    std::fs::create_dir_all(own.join("chats")).unwrap();
    let settings = std::fs::read_to_string(&config).unwrap();
    write(
        &config,
        &link_agent::settings::configuration::with_behavior(
            &settings,
            "schema = 1\n[storage]\nconversations = \"sqlite\"\ninstructions = \"sqlite\"\nschedule = \"sqlite\"\nmemory = \"sqlite\"\n",
        )
        .unwrap(),
    );
    let program = link_test_kit::programs().join("lnk-sqlite-store");
    let source = Driver::at("sqlite", program, &own.join("chats"));
    let whose = Agent {
        id: ID.into(),
        owner: "tester@computer".into(),
    };
    source
        .append(
            &whose,
            "task:1",
            &serde_json::json!({"seq":1,"role":"user","text":"saved in SQLite"}),
        )
        .unwrap();
    source
        .tools_narrow(&whose, "task:1", &["memory__search".into()])
        .unwrap();
    source
        .file_write(&whose, "memory/notes.md", b"Likes tea.", 0)
        .unwrap();
    source
        .job_put(
            &whose,
            Place::Waiting,
            &Job {
                id: "morning".into(),
                due: 1_800_000_000,
                spec: serde_json::json!({"text":"wake"}),
            },
        )
        .unwrap();
    source.job_set_day(&whose, 20_000, 7).unwrap();
    // The receiving machine supports file alone. The preference stays in the spec.
    write(
        &world.here.join(".lnk/safe/agents/policy.toml"),
        "[storage]\nconversations = [\"sqlite\", \"file\"]\ninstructions = [\"sqlite\", \"file\"]\nschedule = [\"sqlite\", \"file\"]\nmemory = [\"sqlite\", \"file\"]\n",
    );
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    let hosted = world.root.join(format!("host/agents/{ID}"));
    assert!(hosted.join("schedule/waiting/morning.json").exists());
    assert!(!hosted.join("files/chats/events.sqlite").exists());
    // Bring the same data back through the file driver's import.
    write(
        &world.here.join(".lnk/safe/agents/policy.toml"),
        "[storage]\nconversations = [\"file\"]\ninstructions = [\"file\"]\nschedule = [\"file\"]\nmemory = [\"file\"]\n",
    );
    ok(&world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]));
    let dest = link_store_contract::store::file::Folder::at(&own, ID).unwrap();
    assert_eq!(dest.read(&whose, "task:1", 1).unwrap().len(), 1);
    assert_eq!(
        dest.tools_read(&whose, "task:1").unwrap(),
        Some(vec!["memory__search".into()])
    );
    assert_eq!(
        dest.file_read(&whose, "memory/notes.md")
            .unwrap()
            .unwrap()
            .0,
        b"Likes tea."
    );
    assert_eq!(
        dest.job_list(&whose, Place::Waiting).unwrap()[0].id,
        "morning"
    );
    assert_eq!(dest.job_day(&whose).unwrap(), (20_000, 7));
    let spec =
        std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(spec.contains("sqlite"));
}

/// A move to an account that fails at its switch, the account not saying
/// it's over (`agent-abort` failing): the far copy isn't counted gone,
/// since an account starts the agent as it takes the switch, so it stays
/// stopped here, and the journal keeps the move for the next start here
/// to finish.
#[test]
fn a_move_to_an_account_that_doesnt_end_it_is_finished_later() {
    let world = World::new("hosted-abort-to");
    an_agent_with_files(&world);
    write(&world.root.join("host/fail-agent-receive-switch"), "no\n");
    write(&world.root.join("host/fail-agent-abort"), "not now\n");
    let failed = world.lnk_agent(&["move", "--cloud", "host", "--yes"]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(!failed.status.success(), "{said}");
    assert!(
        said.contains("may run on your cloud account host"),
        "{said}"
    );
    assert!(said.contains("stays stopped on this computer"), "{said}");
    let journal = world
        .here
        .join(format!(".lnk/safe/agents/transfers/moving/{ID}.toml"));
    assert!(journal.exists());
    std::fs::remove_file(world.root.join("host/fail-agent-receive-switch")).unwrap();
    std::fs::remove_file(world.root.join("host/fail-agent-abort")).unwrap();
    let started = world.lnk_agent(&["start"]);
    let started = crate::moving::text(&started);
    assert!(
        started.contains("nothing of it is left on your cloud account host"),
        "{started}"
    );
    assert!(!journal.exists());
    assert_eq!(host_log(&world).last().unwrap(), "agent-abort");
}

/// A move from an account that fails, the account not saying the agent
/// answers there again: the journal keeps the move, and the next move
/// tells it again before it moves the agent.
#[test]
fn a_move_from_an_account_that_doesnt_end_it_is_finished_by_the_next() {
    let world = World::new("hosted-abort-from");
    an_agent_with_files(&world);
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    write(&world.root.join("host/fail-agent-send-switch"), "no\n");
    write(&world.root.join("host/fail-agent-abort"), "not now\n");
    let failed = world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]);
    let said = crate::moving::text(&failed);
    assert!(!failed.status.success(), "{said}");
    assert!(
        said.contains("didn't say main answers there again"),
        "{said}"
    );
    let journal = world
        .here
        .join(format!(".lnk/safe/agents/transfers/moving/{ID}.toml"));
    assert!(journal.exists());
    std::fs::remove_file(world.root.join("host/fail-agent-send-switch")).unwrap();
    std::fs::remove_file(world.root.join("host/fail-agent-abort")).unwrap();
    let moved = ok(&world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]));
    assert!(moved.contains("An earlier move of main"), "{moved}");
    assert!(moved.contains("main runs on this computer now"), "{moved}");
    assert!(!journal.exists());
}

/// What a command printed for a program to read, on its stdout.
fn ok_stdout(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}
