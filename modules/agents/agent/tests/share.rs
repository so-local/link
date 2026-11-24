//! `lnk agent share <connection>` runs the connection's module
//! (`lnk-<connection> share`, `link_connection_contract::Sharing`) with a
//! one-time code it shows its owner, and writes the chat the module took
//! it in into the agent's spec, `[access] message`, alone; `shares` lists
//! them and `unshare` takes one out. Fake modules here, as scripts.

use std::{
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use link_test_kit::TempDir;

/// An adapter whose harness tells who's speaking (`from`) and speaks no
/// connection itself: Link carries its Slack.
fn adapter(from: bool, own: &str) -> String {
    format!(
        "#!/bin/sh\n[ \"$1\" = connections ] && {{ echo '[{own}]'; exit 0; }}\n\
         [ \"$1\" = contract ] && {{ echo '{{\"version\": {}, \"from\": {from}}}'; exit 0; }}\nexit 0\n",
        link_harness_contract::harness::PROTOCOL.current
    )
}

/// A Slack module that can share: keeps what it was handed, and takes the
/// code in #team.
const SLACK: &str = r##"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Slack", "secret_fields": ["bot_token","app_token"], "bridged": true, "share": true}'; exit 0; }
[ "$1" = share ] || exit 1
cat > "$HOME/sharing"
echo '  Shared.' >&2
echo '{"chat": "chat:slack:C0TEAM", "name": "#team"}'
"##;

/// One that can't.
const SLACK_UNSHARING: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Slack", "bridged": true}'; exit 0; }
cat > "$HOME/sharing"
exit 1
"#;

/// One whose wait ends with no code.
const SLACK_TIMING_OUT: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Slack", "bridged": true, "share": true}'; exit 0; }
cat > "$HOME/sharing"
echo 'Error: no code within 10 minutes' >&2
exit 1
"#;

/// One that says it shared a chat on another connection.
const SLACK_ELSEWHERE: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Slack", "bridged": true, "share": true}'; exit 0; }
cat > "$HOME/sharing"
echo '{"chat": "chat:telegram:-1001234"}'
"#;

/// A spec of its owner's, with a comment of theirs to keep.
const SPEC: &str = "schema = 1\n# Mine: the team's agent.\n[turns]\nrounds = 9 # enough\n";

struct World {
    _root: TempDir,
    home: PathBuf,
    bin: PathBuf,
}

impl World {
    fn new(name: &str, adapter: &str, slack: &str) -> Self {
        let root = TempDir::new(&format!("share-{name}"));
        let (home, bin) = (root.join("home"), root.join("bin"));
        let agents = home.join("Link/Safe/Agents");
        for agent in ["main", "moltbot-2"] {
            std::fs::create_dir_all(agents.join(agent)).unwrap();
        }
        std::fs::create_dir_all(&bin).unwrap();
        for (program, script) in [("lnk-fake", adapter), ("lnk-slack", slack)] {
            link_test_kit::write_program(&bin.join(program), script).unwrap();
        }
        // Any other lnk command fails.
        link_test_kit::write_core(&bin.join("lnk"), "#!/bin/sh\nexit 1\n").unwrap();
        link_test_kit::copy_program(
            std::path::Path::new(env!("CARGO_BIN_EXE_lnk-agent")),
            &bin.join("lnk-agent"),
        )
        .unwrap();
        crate::common::write_agent(
            &home,
            "main",
            &format!(
                "harness = \"fake\"\n{}",
                crate::common::connection(
                    "slack",
                    &["bot_token", "app_token"],
                    "allowed_users = [\"U0DANA\"]",
                    "id = \"U0BOT\"\nname = \"link-main\"\nteam = \"Acme\""
                )
            ),
        );
        World {
            _root: root,
            home,
            bin,
        }
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn lnk(&self, args: &[&str]) -> Output {
        Command::new(self.bin.join("lnk-agent"))
            .args(["-a", "main"])
            .args(args)
            .env_clear()
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&self.home))
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", self.bin.join("lnk"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn spec_path(&self) -> PathBuf {
        self.home.join("Link/Safe/Agents/main/agent.toml")
    }

    fn spec(&self) -> String {
        let config = std::fs::read_to_string(self.spec_path()).unwrap_or_default();
        link_agent::settings::configuration::behavior_text(&config)
            .unwrap()
            .unwrap_or_default()
    }

    fn write_spec(&self, text: &str) {
        let path = self.spec_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let config = std::fs::read_to_string(&path).unwrap_or_default();
        let written = link_agent::settings::configuration::with_behavior(&config, text).unwrap();
        std::fs::write(&path, written).unwrap();
    }

    fn handed(&self) -> Option<serde_json::Value> {
        let text = std::fs::read_to_string(self.home.join("sharing")).ok()?;
        Some(serde_json::from_str(&text).unwrap())
    }
}

fn ok(output: &Output) -> (String, String) {
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    );
    assert!(output.status.success(), "{stdout}\n{stderr}");
    (stdout, stderr)
}

fn refused(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(!output.status.success(), "{stderr}");
    stderr
}

#[test]
fn a_shared_chat_is_written_into_the_spec_alone_and_taken_out_again() {
    let world = World::new("written", &adapter(true, ""), SLACK);
    world.write_spec(SPEC);

    let (stdout, stderr) = ok(&world.lnk(&["share", "add", "slack", "--json"]));
    // The code its owner is shown is the one the module is handed, once,
    // with the connection's settings and how long to wait.
    let handed = world.handed().expect("the module was run");
    let code = handed["code"].as_str().unwrap();
    assert_eq!(code.len(), 8, "{handed}");
    assert!(stderr.contains(&format!("@link-main {code}")), "{stderr}");
    assert_eq!(handed["wait_secs"], 600);
    assert_eq!(handed["settings"]["allowed_users"][0], "U0DANA");
    let shared: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        shared,
        serde_json::json!({"connection": "slack", "chat": "C0TEAM", "name": "#team"})
    );

    // Its owner's spec, as it was, with `[access] message` added.
    let spec = world.spec();
    assert!(spec.starts_with(SPEC), "{spec}");
    let parsed = link_agent_spec::parse(&spec).unwrap();
    assert_eq!(
        parsed.message_access().message,
        [link_agent_identity::access::chat("slack", "C0TEAM").unwrap()]
    );

    // Shared again: still once.
    ok(&world.lnk(&["share", "add", "slack"]));
    assert_eq!(world.spec().matches("chat:slack:C0TEAM").count(), 1);

    let (listed, _) = ok(&world.lnk(&["share", "list", "--json"]));
    let listed: serde_json::Value = serde_json::from_str(listed.trim()).unwrap();
    assert_eq!(listed, serde_json::json!([shared]));
    let (said, _) = ok(&world.lnk(&["share", "list"]));
    assert!(said.contains("#team (C0TEAM)"), "{said}");

    // Unshared by its name, the spec is its owner's again.
    let stderr = refused(&world.lnk(&["share", "remove", "#other", "--connection", "slack"]));
    assert!(stderr.contains("isn't shared with #other"), "{stderr}");
    ok(&world.lnk(&["share", "remove", "#team", "--connection", "slack"]));
    assert_eq!(world.spec().trim(), SPEC.trim());
    let (listed, _) = ok(&world.lnk(&["share", "list", "--json"]));
    assert_eq!(listed.trim(), "[]");
}

#[test]
fn an_agent_with_no_spec_gets_one_holding_only_the_chat() {
    let world = World::new("no-spec", &adapter(true, ""), SLACK);
    ok(&world.lnk(&["share", "add", "slack"]));
    let parsed = link_agent_spec::parse(&world.spec()).unwrap();
    assert_eq!(parsed.message_access().message.len(), 1);
    // Unshared by its id, nothing of it is left.
    ok(&world.lnk(&["share", "remove", "C0TEAM", "--connection", "slack"]));
    assert!(
        link_agent_spec::parse(&world.spec())
            .unwrap()
            .access
            .is_none()
    );
}

/// Each refusal runs no module and leaves the spec as it was.
#[test]
fn what_cant_be_shared_is_refused_before_any_code() {
    let cases: [(&str, String, &str, Option<&str>, &str); 5] = [
        (
            "no-from",
            adapter(false, ""),
            SLACK,
            None,
            "can't tell who's speaking",
        ),
        (
            "speaks-it",
            adapter(true, "\"slack\""),
            SLACK,
            None,
            "speaks Slack itself",
        ),
        (
            "module-cant",
            adapter(true, ""),
            SLACK_UNSHARING,
            None,
            "can't be shared",
        ),
        (
            "never",
            adapter(true, ""),
            SLACK,
            Some("schema = 1\n[connection.slack]\ngroups = \"never\"\n"),
            "groups = \"never\"",
        ),
        (
            "telegram",
            adapter(true, ""),
            SLACK,
            None,
            "has no telegram",
        ),
    ];
    for (name, adapter, slack, spec, says) in cases {
        let world = World::new(name, &adapter, slack);
        if let Some(spec) = spec {
            world.write_spec(spec);
        }
        let connection = if name == "telegram" {
            "telegram"
        } else {
            "slack"
        };
        let stderr = refused(&world.lnk(&["share", "add", connection]));
        assert!(stderr.contains(says), "{name}: {stderr}");
        assert!(world.handed().is_none(), "{name}: the module ran");
        assert_eq!(world.spec(), spec.unwrap_or(""), "{name}");
    }
}

#[test]
fn a_wait_that_ends_writes_nothing() {
    let world = World::new("timed-out", &adapter(true, ""), SLACK_TIMING_OUT);
    world.write_spec(SPEC);
    let stderr = refused(&world.lnk(&["share", "add", "slack"]));
    assert!(stderr.contains("no code within 10 minutes"), "{stderr}");
    assert!(stderr.contains("No chat was shared"), "{stderr}");
    assert!(world.handed().is_some());
    assert_eq!(world.spec(), SPEC);
}

#[test]
fn a_chat_on_another_connection_writes_nothing() {
    let world = World::new("elsewhere", &adapter(true, ""), SLACK_ELSEWHERE);
    world.write_spec(SPEC);
    let stderr = refused(&world.lnk(&["share", "add", "slack"]));
    assert!(stderr.contains("not a chat on slack"), "{stderr}");
    assert_eq!(world.spec(), SPEC);
}

/// A spec reached through a link is never read or written.
#[test]
fn a_linked_spec_is_refused() {
    let world = World::new("linked", &adapter(true, ""), SLACK);
    let elsewhere = world.home.join("elsewhere.toml");
    std::fs::write(&elsewhere, SPEC).unwrap();
    std::fs::create_dir_all(world.spec_path().parent().unwrap()).unwrap();
    std::fs::remove_file(world.spec_path()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, world.spec_path()).unwrap();
    assert!(!world.lnk(&["share", "add", "slack"]).status.success());
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), SPEC);
    assert!(world.spec_path().is_symlink());
}
