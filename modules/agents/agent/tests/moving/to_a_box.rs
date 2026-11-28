//! An agent moves to a box and back: what it keeps and sends,
//! and what its harness asks for before it starts there.

use super::*;

#[test]
fn an_agent_moves_to_a_box_and_back_with_its_state_not_its_install() {
    let world = World::new("back");
    let (here, there) = (&world.here, &world.there);
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"fake\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n\
             base_url = \"http://127.0.0.1:8\"\n\n\
             [fallback]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\nname = \"claude-haiku-5-5\"\n\
             base_url = \"http://127.0.0.1:9\"\n\n\
             {}{}{}",
            world.files(here).display(),
            crate::common::connection(
                "telegram",
                &["bot_token"],
                "bot_token = \"123:bot\"\nallowed_users = [42]",
                "id = \"123\""
            ),
            crate::common::connection(
                "slack",
                &["bot_token", "app_token"],
                "bot_token = \"xoxb-slack-bot\"\napp_token = \"xapp-slack-app\"\n\
                 allowed_users = [\"U0DANA\"]",
                "id = \"U0BOT\"\nname = \"link-main\"\nteam = \"Acme\""
            ),
            crate::common::connection(
                "discord",
                &["bot_token"],
                "bot_token = \"MTIz.discord.bot\"\nallowed_users = [\"200000000000000002\"]",
                ""
            )
        ),
    );
    // Its harness's grants here, with a need it asked for and was given,
    // and more it doesn't ask for, and what you granted the agent.
    write(
        &world.bin.join("fake-needs.json"),
        r#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "its sockets ignore the proxy"}]}"#,
    );
    write(
        &here.join(".lnk/safe/agents/harness-grants/fake.toml"),
        "schema = 1\nrisks = [\"browser\"]\nanswered = [\"browser\"]\n[network]\nmode = \"host\"\n\
         [localhost]\nports = [8080]\n[asked.network]\nmode = \"host\"\n[asked.localhost]\n\
         ports = [8080]\n",
    );
    write(
        &here.join("Link/Safe/Agents/main/sandbox.toml"),
        "schema = 1\n[files]\nread = [\"~\"]\n[internet]\nhosts = [\"*\"]\ntcp = [\"github.com:22\"]\n",
    );
    write(&world.agent(here).join("memory/first"), "remembered");
    write(&world.agent(here).join("bin/fake"), "a Mac program");
    write(&world.agent(here).join(".env"), "KEY=here");
    write(&world.files(here).join("notes.txt"), "from the Mac");

    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    // The box had lnk alone: the harness, its sandbox and the bucket
    // module came first, and its harness from outside Link, from where
    // this computer added it, trusting only the key it pinned.
    assert_eq!(
        std::fs::read_to_string(world.root.join("readied")).unwrap(),
        "module add agent secret model storage\n\
         module add --url https://github.com/someone/link-fake --yes --signer ssh-ed25519 FAKE\n\
         sandbox check --ready --yes\n"
    );
    // On the box: the state and files, not the install or its keys.
    assert_eq!(
        std::fs::read_to_string(world.agent(there).join("memory/first")).unwrap(),
        "remembered"
    );
    assert!(!world.agent(there).join("bin").exists());
    assert!(!world.agent(there).join(".env").exists());
    assert_eq!(
        std::fs::read_to_string(world.files(there).join("notes.txt")).unwrap(),
        "from the Mac"
    );
    let settings = std::fs::read_to_string(there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(settings.contains("harness = \"fake\""), "{settings}");
    // Its keys, kept with your secrets there, never its settings.
    let connection = |connection: &str, field: &str| {
        link_agent_identity::own::OwnSecret::Connection {
            connection: connection.into(),
            field: field.into(),
        }
        .to_use()
        .unwrap()
    };
    for (r#use, key) in [
        (
            link_agent_identity::own::OwnSecret::Model.to_use().unwrap(),
            "sk-ant-test",
        ),
        (
            link_agent_identity::own::OwnSecret::Fallback
                .to_use()
                .unwrap(),
            "sk-ant-test",
        ),
        (connection("telegram", "bot_token"), "123:bot"),
        (connection("slack", "bot_token"), "xoxb-slack-bot"),
        (connection("slack", "app_token"), "xapp-slack-app"),
        (connection("discord", "bot_token"), "MTIz.discord.bot"),
    ] {
        assert!(!settings.contains(key), "{key}: {settings}");
        assert_eq!(
            crate::common::kept_key(there, "main", &r#use).as_deref(),
            Some(key),
            "{use:?}"
        );
    }
    // Its fallback came with its key, at its provider's own address,
    // never one the stream named.
    assert!(settings.contains("claude-haiku-5-5"), "{settings}");
    assert!(!settings.contains("127.0.0.1:9"), "{settings}");
    // So is its model.
    assert!(!settings.contains("127.0.0.1:8"), "{settings}");
    // Here, its keys are gone, and where they went is noted for `lnk
    // secret delete`.
    let store = std::fs::read_to_string(here.join(".lnk/safe/secrets/secrets.toml")).unwrap();
    assert!(!store.contains("sk-ant-test"), "{store}");
    // Each by its secret's ID, the agent's service and its use: never a
    // name.
    let fallback = crate::common::own_secrets(
        here,
        ID,
        &link_agent_identity::own::OwnSecret::Fallback
            .to_use()
            .unwrap(),
    );
    assert_eq!(fallback.len(), 1, "{store}");
    assert_eq!(
        fallback[0].1["copies"].to_string(),
        "[{ name = \"b1\" }]",
        "{store}"
    );
    assert!(!store.contains("MODEL_KEY"), "{store}");
    let left: toml::Table = store.parse().unwrap();
    assert!(left["left"].get(&fallback[0].0).is_some(), "{store}");
    let model = crate::common::own_secrets(
        here,
        ID,
        &link_agent_identity::own::OwnSecret::Model.to_use().unwrap(),
    );
    assert_eq!(
        model[0].1["principal"].as_str(),
        Some(format!("service:{}", crate::common::service_of(ID)).as_str())
    );
    // Its connections too: Slack's owner and bot.
    for slack in ["U0DANA", "U0BOT", "200000000000000002"] {
        assert!(settings.contains(slack), "{slack}: {settings}");
    }
    // The same agent: its ID came with it.
    let identity =
        std::fs::read_to_string(there.join(".lnk/safe/agents/main/identity.toml")).unwrap();
    assert!(
        identity.contains(&format!("id = \"{ID}\"")),
        "{settings} {identity}"
    );
    // What its harness needs went with it, given by this computer: the
    // box doesn't ask again. So did what you granted it, all of it, since
    // this computer gave it.
    // Of its harness's grants, only what it asks for there: not the rest
    // of this computer's file.
    let grants =
        std::fs::read_to_string(there.join(".lnk/safe/agents/harness-grants/fake.toml")).unwrap();
    for granted in [
        "[network]\nmode = \"host\"",
        "[asked.network]\nmode = \"host\"",
    ] {
        assert!(grants.contains(granted), "{granted}: {grants}");
    }
    for not in ["localhost", "unsandboxed", "browser"] {
        assert!(!grants.contains(not), "{not}: {grants}");
    }
    // Read here through the one table, and carried as one.
    let own = std::fs::read_to_string(there.join("Link/Safe/Agents/main/sandbox.toml")).unwrap();
    for granted in ["\"~\"", "github.com:22"] {
        assert!(own.contains(granted), "{granted}: {own}");
    }
    assert!(!settings.contains("location"), "{settings}");
    // On a box, its only way out is its proxy, from the box itself.
    assert!(settings.contains("upstream = \"direct\""), "{settings}");
    // Here, its settings, state, install and keys are gone, its files kept
    // a while, and a note says where it went: no second one starts.
    assert!(!here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(!here.join("Link/Agents/main").exists());
    let kept = only_entry(&here.join(".lnk/data/agents/move"));
    // Its own tools here went with it.
    assert_eq!(
        std::fs::read_to_string(world.root.join("tools-removed")).unwrap(),
        format!(
            "sandbox tool remove memory-{ID}\nsandbox tool remove scheduler-{ID}\n\
             sandbox tool remove run-{ID}\nsandbox tool remove file-{ID}\n\
             sandbox tool remove context-{ID}\nsandbox tool remove mcp-{ID}\n\
             sandbox tool remove namespace-task-{ID}\nsandbox tool remove namespace-cron-{ID}\n\
             sandbox tool remove namespace-tool-{ID}\nsandbox tool remove namespace-file-{ID}\n\
             sandbox tool remove namespace-agent-{ID}\n\
             sandbox tool remove namespace-question-{ID}\n"
        )
    );
    assert_eq!(
        std::fs::read_to_string(kept.join("home/notes.txt")).unwrap(),
        "from the Mac"
    );
    let note =
        std::fs::read_to_string(here.join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml")))
            .unwrap();
    assert!(note.contains("name = \"b1\""), "{note}");
    // Its buckets came along, with a key reaching them alone; here, their
    // password went with it.
    let adopted: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(world.root.join("adopted-there")).unwrap())
            .unwrap();
    assert_eq!(adopted["password"], "OBSCURED");
    assert_eq!(
        adopted["buckets"]["aws"],
        serde_json::json!({"name": "local-link-k3v9q2x7m1ab", "resource": "bucket:b1u2c3k4e5t6"})
    );
    assert_eq!(
        adopted["credentials"]["aws"]["bucket"],
        "local-link-k3v9q2x7m1ab"
    );
    let buckets = std::fs::read_to_string(world.root.join("buckets")).unwrap();
    assert_eq!(buckets, format!("storage leave --owner agent:{ID}\n"));
    let second = world.lnk_agent(&["start"]);
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("main runs on your box b1 since"));
    let status = world.lnk_agent(&["show", "--json"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"location\":\"b1\""));

    // The agent works there: new memories, changed files.
    write(
        &world.agent(there).join("memory/second"),
        "learned on the box",
    );
    std::fs::remove_file(world.agent(there).join("memory/first")).unwrap();
    write(&world.files(there).join("notes.txt"), "edited on the box");

    // Named by where it is: this computer has no agent of its own now.
    ok(&world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]));
    assert!(world.agent(here).join("memory/second").exists());
    // Its state is the box's: what it forgot there is gone here too.
    assert!(!world.agent(here).join("memory/first").exists());
    // No install came: it installs again here.
    assert!(!world.agent(here).join("bin").exists());
    // Back here, with your accounts: no key, and the box's is revoked.
    let adopted: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(world.root.join("adopted-here")).unwrap())
            .unwrap();
    assert_eq!(adopted["password"], "OBSCURED");
    assert!(adopted["credentials"].is_null(), "{adopted}");
    let buckets = std::fs::read_to_string(world.root.join("buckets")).unwrap();
    assert!(
        buckets.ends_with(&format!(
            "storage leave --owner agent:{ID}\nstorage revoke --owner agent:{ID}\n"
        )),
        "{buckets}"
    );
    // Back here, the note is gone, and the box keeps one instead.
    assert!(
        !here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists()
    );
    assert!(!there.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(
        there
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists()
    );
    assert_eq!(
        std::fs::read_to_string(world.files(here).join("notes.txt")).unwrap(),
        "edited on the box"
    );
    let local = std::fs::read_to_string(here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    let identity =
        std::fs::read_to_string(here.join(".lnk/safe/agents/main/identity.toml")).unwrap();
    assert!(
        identity.contains(&format!("id = \"{ID}\"")),
        "{local} {identity}"
    );
    // Back here, its own network again (the LAN, a local model).
    assert!(!local.contains("upstream"), "{local}");
    // Its keys are here again, and none is left on the box it left.
    let store = std::fs::read_to_string(here.join(".lnk/safe/secrets/secrets.toml")).unwrap();
    assert!(!store.contains("left"), "{store}");
    assert_eq!(
        crate::common::kept_key(
            here,
            "main",
            &link_agent_identity::own::OwnSecret::Fallback
                .to_use()
                .unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
}

/// Where a move sends the agent's keys is noted before they're sent: a
/// switch the box refuses leaves the agent here, its keys noted at the
/// box all the same.
#[test]
fn a_move_notes_where_its_keys_go_before_it_sends_them() {
    let world = World::new("noted");
    an_agent_with_files(&world);
    write(&world.root.join("refuse-switch"), "");
    let failed = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    let said = String::from_utf8_lossy(&failed.stdout);
    assert!(!failed.status.success(), "{said}");
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    let store = std::fs::read_to_string(world.here.join(".lnk/safe/secrets/secrets.toml")).unwrap();
    let kept = crate::common::own_secrets(
        &world.here,
        ID,
        &link_agent_identity::own::OwnSecret::Model.to_use().unwrap(),
    );
    assert_eq!(
        kept[0].1["copies"].to_string(),
        "[{ name = \"b1\" }]",
        "{store}"
    );
    assert_eq!(
        crate::common::kept_key(
            &world.here,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
}

/// An agent running in a terminal moves as one in the background does:
/// the move's pause stops it there, as `lnk agent stop` would, and it
/// goes to the box, never refused for where it runs.
#[test]
fn an_agent_running_in_a_terminal_moves_and_stops_there() {
    use std::io::BufRead;
    let world = World::new("terminal");
    an_agent_with_files(&world);
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return;
    }
    // Its terminal's process, holding its foreground lock as a start
    // does, its number in it; the default SIGTERM ends it.
    let lock = world.here.join(".lnk/safe/agents/main/foreground.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let mut holder = link_test_kit::env::command("python3", &world.root);
    holder
        .args([
            "-c",
            "import fcntl,os,sys,time\nf=open(sys.argv[1],'a+')\nfcntl.flock(f,fcntl.LOCK_EX)\n\
             f.truncate(0);f.write(str(os.getpid()));f.flush()\nprint('held',flush=True)\n\
             time.sleep(120)",
        ])
        .arg(&lock)
        .stdout(Stdio::piped());
    let mut holder = link_test_kit::ChildGroup::spawn(&mut holder).unwrap();
    let mut held = String::new();
    std::io::BufReader::new(holder.take_stdout().unwrap())
        .read_line(&mut held)
        .unwrap();
    assert_eq!(held, "held\n");

    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    assert_eq!(
        std::fs::read_to_string(world.files(&world.there).join("notes.txt")).unwrap(),
        "from the Mac"
    );
    let stopped = holder.try_status().unwrap();
    assert!(stopped.is_some(), "the agent still runs in its terminal");
}

/// A machine whose secrets won't give its keys back hasn't left: `lnk
/// agent left` fails, so the move that called it keeps its journal and
/// the note of where the keys went, and its settings and its owner
/// delivery's reader of the interaction store stay for the next try;
/// never a success that drops the note while the keys stay. Once it
/// leaves, its reader goes with it.
#[test]
fn left_is_refused_while_its_keys_stay() {
    let world = World::new("keys-stay");
    an_agent_with_files(&world);
    crate::common::owner_reader(&world.here, "main");
    let store = world.here.join(".lnk/safe/secrets/secrets.toml");
    // A store no `forget` can change, its lock a folder: the agent's
    // keys stay in it, still readable.
    let lock = world.here.join(".lnk/safe/secrets/secrets.toml.lock");
    let _ = std::fs::remove_file(&lock);
    std::fs::create_dir(&lock).unwrap();
    let refused = world.lnk_agent(&["left", "--on", "b1"]);
    let said = text(&refused);
    assert!(!refused.status.success(), "{said}");
    assert!(said.contains("keys stay in your secrets"), "{said}");
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(crate::common::owner_reads(&world.here, "main"));
    assert!(
        !world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists(),
        "{said}"
    );
    // Its secrets changeable again, it leaves, and its keys and reader go.
    std::fs::remove_dir(&lock).unwrap();
    let left = world.lnk_agent(&["left", "--on", "b1"]);
    assert!(left.status.success(), "{}", text(&left));
    assert!(!crate::common::owner_reads(&world.here, "main"));
    let store = std::fs::read_to_string(&store).unwrap();
    assert!(!store.contains("sk-ant-test"), "{store}");
}

/// A move whose `left` here fails, its keys kept, keeps its journal: the
/// next `lnk agent start` here removes the copy before anything starts,
/// refused while it can't, and never starts it beside the one on the box.
#[test]
fn a_move_keeps_its_journal_while_its_copy_stays() {
    let world = World::new("copy-stays");
    an_agent_with_files(&world);
    let store = world.here.join(".lnk/safe/secrets/secrets.toml");
    let lock = world.here.join(".lnk/safe/secrets/secrets.toml.lock");
    // Its keys noted at the box and sent, its secrets here become a store
    // no `forget` can change, its lock a folder.
    write(
        &world.root.join("before-switch"),
        &format!("rm -f '{0}'; mkdir '{0}'\n", lock.display()),
    );
    let moved = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    let said = text(&moved);
    assert!(moved.status.success(), "{said}");
    assert!(said.contains("couldn't be removed"), "{said}");
    assert!(
        world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    // Its copy here can't go yet: start refuses, starting nothing.
    let refused = world.lnk_agent(&["start"]);
    let said = text(&refused);
    assert!(!refused.status.success(), "{said}");
    assert!(said.contains("couldn't be removed"), "{said}");
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    // Its secrets changeable again, the next start removes it first, and
    // refuses: it runs on the box.
    std::fs::remove_dir(&lock).unwrap();
    let again = world.lnk_agent(&["start"]);
    let said = text(&again);
    assert!(!again.status.success(), "{said}");
    assert!(said.contains("runs on"), "{said}");
    assert!(!world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    let store = std::fs::read_to_string(&store).unwrap();
    assert!(!store.contains("sk-ant-test"), "{store}");
}

/// To a cloud account that hosts agents: its keys are noted there, left
/// with the agent; back here, nothing is left there.
#[test]
fn a_cloud_accounts_copy_is_noted_until_the_agent_leaves_it() {
    let world = World::new("account-copy");
    an_agent_with_files(&world);
    crate::common::set_record(
        &world.here,
        "main",
        "identity.toml",
        "owner",
        serde_json::json!({"kind": "account", "id": "github-1"}),
    );
    write(&world.here.join("account.json"), r#"{"owner": "github-1"}"#);
    ok(&world.lnk_agent(&["move", "--cloud", "host", "--yes"]));
    let store = std::fs::read_to_string(world.here.join(".lnk/safe/secrets/secrets.toml")).unwrap();
    let left = crate::common::own_secrets(
        &world.here,
        ID,
        &link_agent_identity::own::OwnSecret::Model.to_use().unwrap(),
    );
    let copy = &left[0].1["copies"][0];
    assert_eq!(
        (copy["cloud"].as_bool(), copy["name"].as_str()),
        (Some(true), Some("host")),
        "{store}"
    );
    // As the owner it went there as, for `lnk secret delete` to ask about.
    assert_eq!(copy["owner"].as_str(), Some("github-1"), "{store}");
    ok(&world.lnk_agent(&["move", "--here", "--agent", "main@host", "--no-start"]));
    let store = std::fs::read_to_string(world.here.join(".lnk/safe/secrets/secrets.toml")).unwrap();
    assert!(!store.contains("left"), "{store}");
}

/// A box says which agent it has by its ID, but never one this computer
/// knows went elsewhere: it can't take another agent's place here, revoke
/// the key of the box that agent is on, or end the note of where it is.
#[test]
fn a_box_claiming_an_agent_that_went_elsewhere_is_refused() {
    let world = World::new("claimed");
    an_agent_here(&world);
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    let note = world
        .here
        .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"));
    // As if the agent with this ID had gone to b2 since, and b1 kept a copy.
    let text = std::fs::read_to_string(&note).unwrap();
    let elsewhere = text.replace("name = \"b1\"", "name = \"b2\"");
    assert_ne!(text, elsewhere);
    std::fs::write(&note, &elsewhere).unwrap();

    let refused = world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("runs on your box b2"), "{said}");
    let buckets = std::fs::read_to_string(world.root.join("buckets")).unwrap();
    assert!(!buckets.contains("revoke"), "{buckets}");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), elsewhere);
    assert!(!world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(
        world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
}

/// An adapter that needs what `needs.json` beside it says (nothing
/// without it), whose harness's state is `~/.needy`.
const NEEDY: &str = r#"#!/bin/sh
here=$(dirname "$0")
case "$1" in
  contract) printf '{"version":1,"proxy":false}' ;;
  needs) cat > /dev/null; [ -f "$here/needs.json" ] && cat "$here/needs.json" ;;
  state) echo '{"include": ["~/.needy"], "exclude": []}' ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  traces) echo '[]' ;;
  *) exit 0 ;;
esac
"#;

/// Coming back from a box, what its harness needs here is asked by the
/// move, before it starts here: refused, its copy here goes again and the
/// note says it's on the box, so no second copy is left here to start.
#[test]
fn a_move_asks_what_its_harness_needs_before_it_starts_there() {
    let world = World::new("needs");
    executable(&world.bin.join("lnk-needy"), NEEDY);
    crate::common::write_agent(
        &world.here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"needy\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(&world.here).display()
        ),
    );
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    // Since: it needs lan, which this computer never allowed it.
    write(
        &world.bin.join("needs.json"),
        r#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "its sockets ignore the proxy"}]}"#,
    );
    let refused = world.lnk_agent(&["move", "--here", "--agent", "main"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("needy asks for --network.mode host") && said.contains("--yes"),
        "{said}"
    );
    assert!(said.contains("its sockets ignore the proxy"), "{said}");
    // No copy here, the note says it's on the box, nothing granted here.
    assert!(!world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    let note = std::fs::read_to_string(
        world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml")),
    )
    .unwrap();
    assert!(note.contains("name = \"b1\""), "{note}");
    assert!(
        world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    let grants = std::fs::read_to_string(
        world
            .here
            .join(".lnk/safe/agents/harness-grants/needy.toml"),
    )
    .unwrap_or_default();
    assert!(!grants.contains("mode = \"host\""), "{grants}");
}

/// The same, answered yes in a terminal (a pseudo-terminal of
/// `script`'s): granted where it goes, started there in its sandbox
/// though it ran with none where it was, and the copy it left gone, the
/// note saying where it is.
#[cfg(target_os = "linux")]
#[test]
fn a_move_answered_yes_starts_the_agent_where_it_goes_with_the_grant() {
    let world = World::new("needs-yes");
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    executable(&world.bin.join("lnk-needy"), NEEDY);
    crate::common::write_agent(
        &world.here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"needy\"\nfiles = \"{}\"\nsandbox = false\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(&world.here).display()
        ),
    );
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    // Since: it needs lan, which only a yes gives.
    write(
        &world.bin.join("needs.json"),
        r#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "its sockets ignore the proxy"}]}"#,
    );
    let said = ok(&move_here_answering(&world, b"y\n"));
    assert!(
        said.contains("needy asks to do more than Link's defaults allow")
            && said.contains("agent here that runs needy.")
            && said.contains("main runs on this computer now.")
            && !said.contains("There is no sandbox."),
        "{said}"
    );
    // Granted here, for the harness, with what it answered.
    let grants = std::fs::read_to_string(
        world
            .here
            .join(".lnk/safe/agents/harness-grants/needy.toml"),
    )
    .unwrap();
    assert!(grants.contains("mode = \"host\""), "{grants}");
    // Started here, in its sandbox: its service written.
    let service = world
        .here
        .join(format!(".config/systemd/user/lnk-agent-{ID}.service"));
    let unit = std::fs::read_to_string(&service).unwrap();
    assert!(!unit.contains("\"unsandboxed\""), "{unit}");
    // No sandbox was its setting on the machine it left: not here.
    let settings =
        std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(!settings.contains("sandbox = false"), "{settings}");
    // One copy: the box's is gone, and its note says it's here.
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    assert!(
        world
            .there
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists()
    );
    assert!(
        !world
            .here
            .join(format!(".lnk/safe/agents/transfers/moved/{ID}.toml"))
            .exists()
    );
}

/// `lnk agent move --here --agent main`, in a pseudo-terminal of
/// `script`'s, answered `answer`.
#[cfg(target_os = "linux")]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn move_here_answering(world: &World, answer: &[u8]) -> Output {
    let harness = link_test_kit::programs().join("lnk-agent");
    let mut moving = Command::new("script")
        .args([
            "-qec",
            &format!("'{}' move --here --agent main", harness.display()),
            "/dev/null",
        ])
        .env_clear()
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&world.here))
        .env("HOME", &world.here)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.here),
        )
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut moving.stdin.take().unwrap(), answer).unwrap();
    moving.wait_with_output().unwrap()
}

/// Coming back from a box, a harness that runs only with no sandbox is
/// refused before anything is asked, even with a yes in a terminal (a
/// pseudo-terminal of `script`'s): an agent lands in its sandbox, since
/// no sandbox was consent for the machine it left, so its copy here goes
/// again and it stays on the box.
#[cfg(target_os = "linux")]
#[test]
fn a_move_of_a_harness_that_needs_no_sandbox_lands_refused() {
    let world = World::new("needs-unsandboxed");
    executable(&world.bin.join("lnk-needy"), NEEDY);
    crate::common::write_agent(
        &world.here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"needy\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(&world.here).display()
        ),
    );
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    write(
        &world.bin.join("needs.json"),
        r#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "its sockets ignore the proxy"}],
            "unsandboxed": {"why": "it runs ps"}}"#,
    );
    let moved = move_here_answering(&world, b"y\n");
    assert!(!moved.status.success());
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&moved.stdout),
        String::from_utf8_lossy(&moved.stderr)
    );
    assert!(
        said.contains("needy runs only with no sandbox.")
            && said.contains("It says: it runs ps")
            && said.contains("--no-sandbox")
            && !said.contains("asks to do more"),
        "{said}"
    );
    // Nothing granted here, one copy, on the box.
    let grants = std::fs::read_to_string(
        world
            .here
            .join(".lnk/safe/agents/harness-grants/needy.toml"),
    )
    .unwrap_or_default();
    assert!(!grants.contains("mode = \"host\""), "{grants}");
    assert!(!world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(
        world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
}
