//! Boxes as they are: the modules a move installs, the list, one agent of
//! two, older boxes and streams, and connections carried.

use super::*;

/// A move to a cloud needs the box and cloud modules: without them, it
/// installs them and moves.
#[test]
fn a_move_to_a_cloud_installs_the_box_and_cloud_modules() {
    let world = World::new("move-alone");
    an_agent_here(&world);
    let said = ok(&world.lnk_agent_alone(&["move", "--cloud", "aws", "--no-start"]));
    assert!(
        said.contains("lnk agent move --cloud aws needs the box module (lnk-box): installing it"),
        "{said}"
    );
    assert_eq!(world.added(), "module add box\nmodule add cloud\n");
    let there =
        std::fs::read_to_string(world.there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(!there.contains("sk-ant-test"), "{there}");
    assert_eq!(
        crate::common::kept_key(
            &world.there,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
}

/// `lnk agent list` asks each running box for its agents: an agent moved
/// there is the box's to list, once; a stopped box is unreached.
#[test]
fn the_list_asks_each_box() {
    let world = World::new("list");
    let here = &world.here;
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"fake\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(here).display()
        ),
    );
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    let listed = world.lnk_agent(&["list", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    link_test_kit::ids::assert_named(&listed);
    let agents = listed["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 1, "{listed}");
    // The box's own word for it: its machine, running there or not.
    let there: toml::Value = toml::from_str(
        &std::fs::read_to_string(world.there.join(".lnk/safe/installation.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        agents[0]["machine"]["resource"],
        format!("installation:{}", there["id"].as_str().unwrap())
    );
    assert_eq!(agents[0]["location"], serde_json::Value::Null);
    assert_eq!(
        listed["unreached"],
        serde_json::json!([{"resource": "box:p8r2c4n6w0zt", "name": "b2", "error": "it's stopped"}])
    );
    // A new agent on a box asks its questions there; a key is never on a
    // command line, so there's no flag for one.
    let keyed = world.lnk_agent(&["create", "side", "--on", "b1", "--key", "sk-ant-x"]);
    assert!(!keyed.status.success());
    assert!(String::from_utf8_lossy(&keyed.stderr).contains("unexpected argument '--key'"));
    // --here: only this computer's agents, which has none now.
    let here_only = world.lnk_agent(&["list", "--json", "--here"]);
    let here_only: serde_json::Value = serde_json::from_slice(&here_only.stdout).unwrap();
    assert_eq!(here_only["agents"], serde_json::json!([]));
    assert_eq!(here_only["unreached"], serde_json::json!([]));
    // A box that doesn't answer: the note here says where it went.
    let note = here.join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"));
    let text = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, text.replace("\"b1\"", "\"b2\"")).unwrap();
    // Gone from there: its folder and the identity the box kept.
    std::fs::remove_dir_all(world.there.join("Link/Agents/main")).unwrap();
    let _ = std::fs::remove_dir_all(world.there.join(".lnk/safe/agents/main"));
    std::fs::remove_dir_all(world.there.join("Link/Safe/Agents/main")).unwrap();
    let listed = world.lnk_agent(&["list", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    link_test_kit::ids::assert_named(&listed);
    let agents = listed["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 1, "{listed}");
    assert_eq!(agents[0]["workload"], format!("agent:{ID}"));
    assert_eq!(agents[0]["location"], "b2");
    assert_eq!(agents[0]["running"], serde_json::Value::Null);
}

/// One agent of two moves: the other stays; the one that moved is found
/// on its box by name; a box with another agent of that name refuses it.
#[test]
fn one_agent_of_two_moves_alone() {
    let world = World::new("two");
    let (here, there) = (&world.here, &world.there);
    for (name, id, port) in [("main", ID, ""), ("side", "s1d2e3f4g5h6", "port = 30000\n")] {
        crate::common::write_agent(
            here,
            name,
            &format!(
                "id = \"agent:{id}\"\nharness = \"fake\"\n{port}\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n{}",
                crate::common::connection(
                    "telegram",
                    &["bot_token"],
                    &format!(
                        "bot_token = \"{n}:bot\"\nallowed_users = [42]",
                        n = if name == "main" { 1 } else { 2 }
                    ),
                    &format!(
                        "id = \"{n}\"\nname = \"{bot}\"",
                        n = if name == "main" { 1 } else { 2 },
                        bot = if name == "side" { "side_bot" } else { "" }
                    ),
                ),
            ),
        );
        write(
            &here.join(format!("Link/Agents/{name}/home/notes.txt")),
            name,
        );
    }
    // With two here, it's named.
    let unnamed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    assert!(!unnamed.status.success());
    ok(&world.lnk_agent(&["move", "--on", "b1", "--agent", "side", "--no-start"]));
    let side = std::fs::read_to_string(there.join("Link/Safe/Agents/side/agent.toml")).unwrap();
    let identity =
        std::fs::read_to_string(there.join(".lnk/safe/agents/side/identity.toml")).unwrap();
    assert!(
        identity.contains("id = \"s1d2e3f4g5h6\""),
        "{side} {identity}"
    );
    // The bot's username comes along; the ports are the box's to give.
    assert!(side.contains("[connections.telegram.bot]"), "{side}");
    assert!(side.contains("name = \"side_bot\""), "{side}");
    assert!(!side.contains("2:bot"), "{side}");
    let bot = link_agent_identity::own::OwnSecret::Connection {
        connection: "telegram".into(),
        field: "bot_token".into(),
    }
    .to_use()
    .unwrap();
    assert_eq!(
        crate::common::kept_key(there, "side", &bot).as_deref(),
        Some("2:bot")
    );
    // Its port is the box's, the first block free there: never the one it
    // had here, and not one this test can name, since any program on this
    // machine may hold the first.
    let given = crate::common::record(there, "side", "here.toml");
    let port = given["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .unwrap_or_else(|| panic!("no port: {side} {given:?}"));
    assert!(
        (link_agent::settings::FIRST_PORT..link_agent::settings::EPHEMERAL).contains(&port),
        "{side}"
    );
    assert_eq!(
        std::fs::read_to_string(there.join("Link/Agents/side/home/notes.txt")).unwrap(),
        "side"
    );
    // main stays here, untouched.
    assert!(here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(here.join("Link/Agents/main/home/notes.txt").exists());
    assert!(!there.join("Link/Safe/Agents/main/agent.toml").exists());

    // Back, by its name: this computer finds it on the box.
    ok(&world.lnk_agent(&["-a", "side", "move", "--here", "--no-start"]));
    assert!(here.join("Link/Safe/Agents/side/agent.toml").exists());
    assert!(!there.join("Link/Safe/Agents/side/agent.toml").exists());

    // Another agent named side on the box: this one isn't put in its place.
    crate::common::write_agent(
        there,
        "side",
        "id = \"agent:o1t2h3e4r5x6\"\nservice = \"service:o2t3h4e5r6s7\"\nharness = \"fake\"\n",
    );
    let refused = world.lnk_agent(&["move", "--on", "b1", "--agent", "side", "--no-start"]);
    assert!(!refused.status.success());
    let text = String::from_utf8_lossy(&refused.stdout).to_string()
        + &String::from_utf8_lossy(&refused.stderr);
    assert!(text.contains("another agent named side"), "{text}");
    let theirs = crate::common::record(there, "side", "identity.toml");
    assert_eq!(theirs["id"], "o1t2h3e4r5x6", "{theirs:?}");
    assert!(here.join("Link/Safe/Agents/side/agent.toml").exists());
}

/// A stream of an older move protocol is refused by name, naming the
/// upgrade, and nothing here changes.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_stream_of_another_format_is_refused() {
    let world = World::new("format");
    let older = link_agent_contract::moving::PROTOCOL.oldest - 1;
    let mut stream = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut stream);
        let json = format!(
            r#"{{"format": {older}, "phase": "copy", "move": "0123456789abcdef0123456789abcdef"}}"#
        );
        link_agent_contract::moving::append_manifest(&mut tar, json.as_bytes()).unwrap();
        tar.finish().unwrap();
    }
    let mut receive = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["receive", "--no-start"])
        .env_clear()
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&world.there))
        .env("HOME", &world.there)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.there),
        )
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut announced = std::io::BufReader::new(receive.stdout.take().unwrap());
    let seal_to = link_secretive::SealTo::read_announced(&mut announced, |_| {}).unwrap();
    let mut input = receive.stdin.take().unwrap();
    std::io::Write::write_all(&mut input, b"{}\n").unwrap();
    let mut sealed = link_secretive::stream::Sender::new(
        input,
        &seal_to,
        link_agent_contract::moving::stream_about(),
    )
    .unwrap();
    std::io::Write::write_all(&mut sealed, &stream).unwrap();
    sealed.finish().unwrap();
    let output = receive.wait_with_output().unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains(&format!("speaks the move protocol {older}, below"))
            && err.contains("lnk upgrade"),
        "{err}"
    );
    assert!(!world.there.join(".lnk/safe/agents").exists());
    assert!(
        !world
            .there
            .join(".lnk/safe/agents/transfers/receiving")
            .exists()
    );
}

/// A stream is of the agent the machine running the move chose
/// (`--agent-id`): a box that sends another agent's, to bring its ID
/// here, is refused before anything of it is staged.
#[test]
fn a_stream_of_another_agent_is_refused() {
    let world = World::new("another-agent");
    an_agent_with_files(&world);
    let sent = world.lnk_agent(&["state-send", "--phase", "copy"]);
    assert!(sent.status.success(), "{}", text(&sent));
    let receive = |id: &str| {
        let mut receive =
            link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-agent"), &world.there)
                .args(["receive", "--no-start", "--phase", "copy", "--agent-id", id])
                .env("LNK_SERVICES", "off")
                .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
        let mut input = receive.stdin.take().unwrap();
        std::io::Write::write_all(&mut input, b"{}\n").unwrap();
        std::io::Write::write_all(&mut input, &sent.stdout).unwrap();
        drop(input);
        receive.wait_with_output().unwrap()
    };
    let staging = world
        .there
        .join(".lnk/safe/agents/transfers/receiving")
        .join(ID);
    let refused = receive("b2c3d4e5f6a1");
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("another agent"),
        "{}",
        text(&refused)
    );
    assert!(!staging.exists());
    // The agent it names, it takes.
    let taken = receive(ID);
    assert!(taken.status.success(), "{}", text(&taken));
    assert!(staging.exists());
}

/// Signal, which Link carries for a harness that speaks none, answers on
/// the box without linking again: its account moves, its `signal-cli`
/// doesn't. Hermes and OpenClaw speak none either: Link's bridge carries
/// each of their connections, so one sent during the switch waits on its
/// platform for the bridge where the agent went, and is answered once.
#[test]
fn a_carried_connection_moves_linked() {
    for harness in ["bare", "hermes", "openclaw"] {
        a_carried_connection_moves_linked_on(harness);
    }
}

fn a_carried_connection_moves_linked_on(harness: &str) {
    let world = World::new(&format!("linked-{harness}"));
    let (here, there) = (&world.here, &world.there);
    crate::common::write_adapter(&world.bin.join("lnk-bare"), BARE).unwrap();
    executable(&world.bin.join("lnk-signal"), SIGNAL);
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"{harness}\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n\n\
             {}",
            world.files(here).display(),
            crate::common::connection(
                "signal",
                &[],
                "account = \"+15557654321\"\nallowed_users = [\"+15551234567\"]",
                "id = \"+15557654321\""
            )
        ),
    );
    let signal = |home: &Path| home.join(".lnk/safe/agents/main/connections/signal");
    let cli = signal(here).join(".signal-cli");
    write(&cli.join("data/data/accounts.json"), "linked");
    write(&cli.join("account"), "+15557654321");
    write(&cli.join("0.14.8/bin/signal-cli"), "a Mac program");
    write(&cli.join("signal-cli"), "the wrapper");
    // Telegram has a token: nothing of it is a folder to move.
    write(
        &here.join(".lnk/safe/agents/main/connections/telegram/x"),
        "not a link",
    );

    // What Hermes and OpenClaw ask for beyond Link's defaults, allowed.
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start", "--yes"]));
    let cli = signal(there).join(".signal-cli");
    assert_eq!(
        std::fs::read_to_string(cli.join("data/data/accounts.json")).unwrap(),
        "linked"
    );
    assert_eq!(
        std::fs::read_to_string(cli.join("account")).unwrap(),
        "+15557654321"
    );
    assert!(!cli.join("0.14.8").exists() && !cli.join("signal-cli").exists());
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&signal(there).metadata().unwrap().permissions());
    assert_eq!(mode & 0o777, 0o700);
    assert!(
        !there
            .join(".lnk/safe/agents/main/connections/telegram")
            .exists()
    );
    let settings = std::fs::read_to_string(there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(settings.contains("+15557654321"), "{settings}");
    // Here, the link is gone with the agent: one device, on one machine.
    assert!(!signal(here).exists());
}
