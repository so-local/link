//! `lnk agent connect <connection>` runs the connection's module
//! (`link_connection_contract`): it's told the agent and the bots other agents
//! have, and what it prints becomes the agent's connection. Fake modules
//! here, as scripts.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use link_test_kit::TempDir;

/// An adapter speaking Slack and Signal, which notes when it's asked
/// to link one.
const ADAPTER: &str = "#!/bin/sh\n[ \"$1\" = connections ] && echo '[\"slack\", \"signal\", \"matrix\"]'\n\
                       [ \"$1\" = link ] && echo \"$*\" >> \"$HOME/linked\"\nexit 0\n";

/// A Signal module: the agent's number and its owner's.
const SIGNAL: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Signal", "secret_fields": [], "linked": true, "bridged": true}'; exit 0; }
printf '%s\n' "$@" > "$HOME/asked"
echo '{"settings": {"account": "+15557654321", "allowed_users": ["+15551234567"]}, "bot": {"id": "+15557654321", "name": "+15557654321"}}'
"#;

/// A Slack module that says what it was asked, then pairs Dana.
const SLACK: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Slack", "secret_fields": ["bot_token","app_token"], "linked": false, "bridged": true}'; exit 0; }
printf '%s\n' "$@" > "$HOME/asked"
echo '  Got it, Dana.' >&2
echo '{"settings": {"bot_token": "xoxb-made", "app_token": "xapp-made", "allowed_users": ["U0DANA"]}, "bot": {"id": "U0BOT", "name": "link-main", "team": "Acme"}}'
"#;

/// A connection from outside Link, which Link knows nothing of.
const MATRIX: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Matrix", "secret_fields": ["token"]}'; exit 0; }
[ "$1" = rule ] && { echo 'null'; exit 0; }
echo '{"settings": {"token": "syt_made", "allowed_users": ["@me:example.org"]}, "bot": {"id": "@agent:example.org", "name": "agent"}}'
"#;

/// One that gives up.
const FAILING: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Slack", "bridged": true}'; exit 0; }
echo 'Error: Slack refused: invalid_auth' >&2
exit 1
"#;

/// One linked as a device that gives up: linking pauses a running agent,
/// as it could answer the code itself, so its start again is what fails.
#[cfg(target_os = "linux")]
const FAILING_LINKED: &str = r#"#!/bin/sh
[ "$1" = describe ] && { echo '{"title": "Slack", "linked": true, "bridged": true}'; exit 0; }
echo 'Error: Slack refused: invalid_auth' >&2
exit 1
"#;

struct World {
    _root: TempDir,
    home: PathBuf,
    bin: PathBuf,
}

impl World {
    fn new(name: &str, slack: &str) -> Self {
        let root = TempDir::new(&format!("connect-{name}"));
        let (home, bin) = (root.join("home"), root.join("bin"));
        let agents = home.join("Link/Safe/Agents");
        for agent in ["main", "moltbot-2"] {
            std::fs::create_dir_all(agents.join(agent)).unwrap();
        }
        std::fs::create_dir_all(&bin).unwrap();
        for (program, script) in [
            ("lnk-fake", ADAPTER),
            ("lnk-slack", slack),
            ("lnk-signal", SIGNAL),
            ("lnk-matrix", MATRIX),
        ] {
            if program == "lnk-fake" {
                crate::common::write_adapter(&bin.join(program), script).unwrap();
            } else {
                link_test_kit::write_program(&bin.join(program), script).unwrap();
            }
        }
        // Any other lnk command (asking boxes) fails.
        link_test_kit::write_core(&bin.join("lnk"), "#!/bin/sh\nexit 1\n").unwrap();
        // lnk-agent finds modules beside itself first, as in lnk's folder:
        // a copy there, so these fakes are the ones it runs.
        link_test_kit::copy_program(
            Path::new(env!("CARGO_BIN_EXE_lnk-agent")),
            &bin.join("lnk-agent"),
        )
        .unwrap();
        crate::common::write_agent(&home, "main", "harness = \"fake\"\n");
        crate::common::write_agent(
            &home,
            "moltbot-2",
            &format!(
                "harness = \"fake\"\n{}{}",
                crate::common::connection(
                    "slack",
                    &["bot_token", "app_token"],
                    "allowed_users = [\"U0EVE\"]",
                    "id = \"U0TAKEN\"\nname = \"theirs\"\nteam = \"Acme\""
                ),
                crate::common::connection(
                    "signal",
                    &[],
                    "account = \"+15558888888\"\nallowed_users = [\"+15550000000\"]",
                    "id = \"+15558888888\""
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
    fn connect(&self, connection: &str) -> Output {
        Command::new(self.bin.join("lnk-agent"))
            .args(["-a", "main", "connect", connection])
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

    fn settings(&self) -> String {
        std::fs::read_to_string(self.home.join("Link/Safe/Agents/main/agent.toml")).unwrap()
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn the_connections_module_makes_it_and_the_agent_keeps_it() {
    let world = World::new("made", SLACK);
    let output = world.connect("slack");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    // The module talks on the terminal.
    assert!(stderr.contains("Got it, Dana."), "{stderr}");

    // It was told the agent, and the bot moltbot-2 has.
    let asked: Vec<String> = read(&world.home.join("asked"))
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(asked[..4], ["connect", "--agent", "main", "--taken"]);
    let taken: serde_json::Value = serde_json::from_str(&asked[4]).unwrap();
    assert_eq!(
        taken,
        serde_json::json!([{"id": "U0TAKEN", "agent": "moltbot-2"}])
    );

    // What it made is the agent's Slack, and its bot; its tokens in your
    // secrets, never its settings.
    let settings = world.settings();
    for kept in ["U0DANA", "U0BOT", "link-main"] {
        assert!(settings.contains(kept), "{kept}: {settings}");
    }
    for (field, token) in [("bot_token", "xoxb-made"), ("app_token", "xapp-made")] {
        assert!(!settings.contains(token), "{settings}");
        let r#use = link_agent_identity::own::OwnSecret::Connection {
            connection: "slack".into(),
            field: field.into(),
        }
        .to_use()
        .unwrap();
        assert_eq!(
            crate::common::kept_key(&world.home, "main", &r#use).as_deref(),
            Some(token)
        );
    }
    // moltbot-2's settings keep their bots.
    let theirs = read(&world.home.join("Link/Safe/Agents/moltbot-2/agent.toml"));
    assert!(theirs.contains("[connections.slack.bot]"), "{theirs}");
    assert!(
        theirs.contains("U0TAKEN") && theirs.contains("+15558888888"),
        "{theirs}"
    );
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_connection_from_outside_link_plugs_in() {
    let world = World::new("matrix", SLACK);
    let output = world.connect("matrix");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let settings = world.settings();
    assert!(
        !settings.contains("syt_made") && settings.contains("title = \"Matrix\""),
        "{settings}"
    );
    // Its secret field, as it says, is kept with your secrets.
    let token = link_agent_identity::own::OwnSecret::Connection {
        connection: "matrix".into(),
        field: "token".into(),
    }
    .to_use()
    .unwrap();
    assert_eq!(
        crate::common::kept_key(&world.home, "main", &token).as_deref(),
        Some("syt_made")
    );
    let status = Command::new(world.bin.join("lnk-agent"))
        .args(["-a", "main", "show", "--json"])
        .env_clear()
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&world.home))
        .env("HOME", &world.home)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.home),
        )
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .output()
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["connections"], serde_json::json!(["matrix"]));
    assert_eq!(status["bots"]["matrix"]["id"], "@agent:example.org");
}

#[test]
fn connecting_again_changes_nothing_unless_asked() {
    let world = World::new("again", SLACK);
    assert!(world.connect("slack").status.success());
    std::fs::remove_file(world.home.join("asked")).unwrap();
    let output = world.connect("slack");
    assert!(output.status.success());
    let stdout = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("main already talks on Slack"), "{stdout}");
    assert!(stdout.contains("Nothing changed"), "{stdout}");
    // Without a terminal to say yes, the module isn't run again.
    assert!(!world.home.join("asked").exists());
}

#[test]
fn a_module_that_fails_changes_nothing() {
    let world = World::new("failed", FAILING);
    let output = world.connect("slack");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid_auth"), "{stderr}");
    assert!(
        stderr.contains("Connecting Slack didn't finish"),
        "{stderr}"
    );
    assert!(!world.settings().contains("slack"), "{}", world.settings());
}

#[test]
fn a_connection_the_harness_doesnt_speak_runs_no_module() {
    let world = World::new("unspoken", SLACK);
    let output = world.connect("discord");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Its module isn't here to give its title.
    assert!(stderr.contains("fake doesn't speak discord"), "{stderr}");
    assert!(!world.home.join("asked").exists());
}

#[test]
fn signal_keeps_the_agents_number_and_whom_it_answers() {
    let world = World::new("signal", SLACK);
    let output = world.connect("signal");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    // It was told the number moltbot-2's Signal has.
    let asked = read(&world.home.join("asked"));
    assert!(asked.contains(r#""id":"+15558888888""#), "{asked}");
    let settings = world.settings();
    assert!(
        settings.contains("account = \"+15557654321\""),
        "{settings}"
    );
    assert!(settings.contains("[connections.signal.bot]"), "{settings}");
    assert!(settings.contains("+15551234567"), "{settings}");
    // With no model yet nothing starts, so nothing is linked: `lnk agent
    // start` links it.
    assert!(!world.home.join("linked").exists());
    let stdout = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("lnk agent start"), "{stdout}");
}

/// A fake systemd where the agent runs: each call kept in
/// `systemctl.log` beside it.
#[cfg(target_os = "linux")]
const SYSTEMCTL: &str = r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/systemctl.log"
exit 0
"#;

/// A fake systemd where the agent is stopped: each call kept in
/// `systemctl.log` beside it.
#[cfg(target_os = "linux")]
const SYSTEMCTL_STOPPED: &str = r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/systemctl.log"
case "$*" in *is-active*) exit 3 ;; esac
exit 0
"#;

/// A stopped agent with a model, connected: it stays stopped, and is
/// told how to start, never started as a service it didn't run as.
#[cfg(target_os = "linux")]
#[test]
fn a_stopped_agent_connected_stays_stopped() {
    let world = World::new("stays-stopped", SLACK);
    link_test_kit::write_program(&world.bin.join("systemctl"), SYSTEMCTL_STOPPED).unwrap();
    crate::common::write_agent(
        &world.home,
        "main",
        "harness = \"fake\"\n\n[model]\nprovider = \"ollama\"\napi_key = \"\"\n\
         name = \"qwen3:8b\"\nbase_url = \"http://localhost:9\"\n",
    );
    let output = world.connect("slack");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{said}");
    assert!(world.settings().contains("[connections.slack"), "{said}");
    assert!(
        said.contains("lnk agent start   start your agent"),
        "{said}"
    );
    let calls = std::fs::read_to_string(world.bin.join("systemctl.log")).unwrap_or_default();
    assert!(
        !calls
            .lines()
            .any(|l| l.contains("start") || l.contains("enable")),
        "{calls}"
    );
}

/// A running agent stopped for a connection's setup that then fails, and
/// whose start again fails before its service starts (its local model is
/// down): the stop is finished, systemd reloaded and the calendar's wake
/// taken away, not left for an agent that isn't running.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_start_again_that_fails_early_finishes_the_stop() {
    let world = World::new("fails-early", FAILING_LINKED);
    link_test_kit::write_program(&world.bin.join("systemctl"), SYSTEMCTL).unwrap();
    // The calendar's wake, as a start records it: taken away, it notes so.
    let removed = world.home.join("calendar-removed");
    link_test_kit::write_program(
        &world.bin.join("cron"),
        format!("#!/bin/sh\necho \"$*\" > '{}'\n", removed.display()),
    )
    .unwrap();
    let config = world.home.join(".lnk/safe/agents/main");
    std::fs::create_dir_all(&config).unwrap();
    let record = config.join("scheduler-cron.json");
    std::fs::write(
        &record,
        serde_json::json!({"schema": 1, "remove": [
            world.bin.join("cron").display().to_string(),
            "cron",
            "uninstall",
        ]})
        .to_string(),
    )
    .unwrap();
    crate::common::write_agent(
        &world.home,
        "main",
        "harness = \"fake\"\n\n[model]\nprovider = \"ollama\"\napi_key = \"\"\n\
         name = \"qwen3:8b\"\nbase_url = \"http://localhost:9\"\n",
    );
    let output = Command::new(world.bin.join("lnk-agent"))
        .args(["-a", "main", "connect", "slack"])
        .env_clear()
        .envs(link_test_kit::keychain::isolated(&world.home))
        .env("HOME", &world.home)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.home),
        )
        .env("XDG_RUNTIME_DIR", world.home.join("run"))
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{said}");
    assert!(said.contains("isn't answering"), "{said}");
    let calls = read(&world.bin.join("systemctl.log"));
    assert!(calls.contains("disable --no-reload --now"), "{calls}");
    assert!(
        calls
            .lines()
            .last()
            .is_some_and(|l| l.contains("daemon-reload")),
        "{calls}"
    );
    assert_eq!(read(&removed), "cron uninstall\n", "{said}");
    assert!(!record.exists());
}

/// A plain start whose harness then fails to set up, after its service
/// was stopped: the stop is finished too, systemd reloaded and the
/// calendar's wake taken away.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_start_that_fails_after_its_stop_finishes_it() {
    let world = World::new("start-fails", SLACK);
    link_test_kit::write_program(&world.bin.join("systemctl"), SYSTEMCTL).unwrap();
    // An adapter whose setup fails.
    crate::common::write_adapter(
        &world.bin.join("lnk-fake"),
        "#!/bin/sh\n[ \"$1\" = configure ] && { echo 'Error: no setup' >&2; exit 1; }\nexit 0\n",
    )
    .unwrap();
    let removed = world.home.join("calendar-removed");
    link_test_kit::write_program(
        &world.bin.join("cron"),
        format!("#!/bin/sh\necho \"$*\" > '{}'\n", removed.display()),
    )
    .unwrap();
    let config = world.home.join(".lnk/safe/agents/main");
    std::fs::create_dir_all(&config).unwrap();
    let record = config.join("scheduler-cron.json");
    std::fs::write(
        &record,
        serde_json::json!({"schema": 1, "remove": [
            world.bin.join("cron").display().to_string(),
            "cron",
            "uninstall",
        ]})
        .to_string(),
    )
    .unwrap();
    let output = Command::new(world.bin.join("lnk-agent"))
        .args(["-a", "main", "start", "--no-sandbox", "--run.files"])
        .arg(world.home.join("files"))
        .env_clear()
        .env("ANTHROPIC_API_KEY", "sk-ant-test")
        .envs(link_test_kit::keychain::isolated(&world.home))
        .env("HOME", &world.home)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.home),
        )
        .env("XDG_RUNTIME_DIR", world.home.join("run"))
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{said}");
    let calls = read(&world.bin.join("systemctl.log"));
    assert!(
        calls.contains("disable --no-reload --now"),
        "{said}\n{calls}"
    );
    assert!(
        calls
            .lines()
            .last()
            .is_some_and(|l| l.contains("daemon-reload")),
        "{said}\n{calls}"
    );
    assert_eq!(read(&removed), "cron uninstall\n", "{said}");
    assert!(!record.exists());
}
