//! Native parsing and the real migration/switch consumer. Only the harness
//! process and installer are stand-ins; state commands use the real adapters.
use link_test_kit::{TempDir, env};
use serde_json::{Value, json};
use std::path::PathBuf;

struct World {
    _root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    agent: PathBuf,
}

impl World {
    fn new() -> Self {
        let root = TempDir::short_out_of_tmp("sp");
        let home = root.join("home");
        let bin = root.join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let programs = link_test_kit::programs();
        let agent = crate::common::harness(&bin);
        for (name, real) in [
            ("openclaw", "lnk-openclaw"),
            ("hermes", "lnk-hermes"),
            ("harness", "lnk-harness"),
        ] {
            let real = programs.join(real);
            assert!(real.is_file(), "build {} before this test", real.display());
            let script = format!(
                r#"#!/bin/sh
case "$1" in
  install)
    # What an install leaves that `state` runs: Hermes's sealed program
    # payload in its staging, its launcher with no sessions to give or take
    # and its ACP entrypoint.
    root="${{LNK_EXEC_DIR:?}}"
    launcher="$root/hermes-agent/.hermes/bin"
    environment="$root/installs/0000000000000000/environments/selected"
    mkdir -p "$launcher" "$environment" "$root/tools" "$root/pm-runtime/lib/python3.11/site-packages"
    printf '#!/bin/sh\ncat >/dev/null\necho "[]"\n' > "$launcher/hermes"
    printf '#!/bin/sh\nexit 0\n' > "$launcher/hermes-acp"
    chmod +x "$launcher/hermes" "$launcher/hermes-acp"
    printf '%s' '{{"repo":"hermes-agent","venv":"installs/0000000000000000/environments/selected","store":"tools"}}' > "$root/manifest.json"
    printf 'version = 3.11\n' > "$environment/pyvenv.cfg"
    : > "$root/tools/fixture"
    printf 'fixture interpreter' > "$root/tools/python"
    printf '%s' '{{"python":"../tools/python","sitePackages":"lib/python3.11/site-packages"}}' > "$root/pm-runtime/pm-runtime.json"
    : > "$root/pm-runtime/lib/python3.11/site-packages/fixture"
    exit 0 ;;
  health) exit 0 ;;
  configure) cat >/dev/null; exit 0 ;;
  command) echo '{{"argv":["/usr/bin/true"],"write":[],"read":[]}}'; exit 0 ;;
  endpoint) echo '""'; exit 0 ;;
  *) exec '{}' "$@" ;;
esac
"#,
                real.display()
            );
            link_test_kit::write_program(&bin.join(format!("lnk-{name}")), script).unwrap();
        }
        let core = format!(
            r#"#!/bin/sh
case "$1" in
  agent) shift; exec '{}' "$@" ;;
  *) exec '{}' "$@" ;;
esac
"#,
            agent.display(),
            programs.join("lnk").display()
        );
        link_test_kit::write_core(&bin.join("lnk"), core).unwrap();
        let native = home.join(".openclaw");
        std::fs::create_dir_all(native.join("cron")).unwrap();
        let jobs = json!({"version":1,"jobs":[
            {"id":"inv-monthly","name":"Draft invoices","enabled":true,"agentId":"main","wakeMode":"now","sessionTarget":"isolated",
                "schedule":{"kind":"cron","expr":"0 9 1 * *","tz":"Europe/Lisbon"},"payload":{"kind":"agentTurn","message":"Draft invoices"},
                "delivery":{"mode":"announce","channel":"telegram"}},
            {"id":"inbox-am","name":"Morning inbox","enabled":true,"agentId":"main","wakeMode":"now","sessionTarget":"isolated",
                "schedule":{"kind":"every","everyMs":600000,"anchorMs":1780000017000_u64},"payload":{"kind":"agentTurn","message":"Summarise client mail"},
                "delivery":{"mode":"announce","channel":"telegram","to":"-42","threadId":7}}
        ]});
        std::fs::write(
            native.join("cron/jobs.json"),
            serde_json::to_vec(&jobs).unwrap(),
        )
        .unwrap();
        Self {
            _root: root,
            home,
            bin,
            agent,
        }
    }

    fn command(&self, args: &[&str]) -> std::process::Command {
        let mut command = env::command(&self.agent, &self.home);
        command
            .args(args)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SERVICES", "off")
            .env("ANTHROPIC_API_KEY", "sk-ant-test-schedules");
        command
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command(args).output().unwrap()
    }

    fn definitions(&self) -> Vec<link_agent_spec::Schedule> {
        let text =
            std::fs::read_to_string(self.home.join("Link/Safe/Agents/main/agent.toml")).unwrap();
        let settings: link_agent::settings::Settings = link_files::saved::parse(&text).unwrap();
        settings.behavior.unwrap().schedules.unwrap()
    }
}

fn ok(output: std::process::Output) -> String {
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{said}");
    said
}

#[test]
fn interrupted_native_import_and_switches_preserve_schedules_with_execution_stood_in() {
    if cfg!(target_os = "linux") {
        let map = std::fs::read_to_string("/proc/self/uid_map").unwrap();
        if !link_machine::machine::whole_uid_map(&map) {
            link_test_kit::gate::lacks(
                "host identity",
                "the control-plane fixture needs a whole UID map",
                None,
            );
            return;
        }
    }
    let world = World::new();
    // Exercise recovery after the real Agent journal step. Change the
    // fixture's execution policy before resuming its remaining steps.
    let stopped = world
        .command(&["migrate", "--harness", "openclaw", "--yes"])
        .env("LNK_TEST_MIGRATE_STOP", "1")
        .output()
        .unwrap();
    assert!(!stopped.status.success());
    let path = world.home.join("Link/Safe/Agents/main/agent.toml");
    let mut setup: toml::Table = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    setup.insert("sandbox".into(), toml::Value::Boolean(false));
    std::fs::write(path, toml::to_string(&setup).unwrap()).unwrap();
    let original = std::fs::read(world.home.join(".openclaw/cron/jobs.json")).unwrap();
    let said = ok(world.run(&["migrate", "--harness", "openclaw", "--yes"]));
    for name in ["Draft invoices", "Morning inbox"] {
        assert!(said.contains(name), "{said}");
    }
    let definitions = world.definitions();
    assert_eq!(definitions.len(), 2);
    assert_eq!(definitions[0].name, "Draft invoices");
    assert_eq!(definitions[1].name, "Morning inbox");
    // Their identities are the agent's record's, the native jobs they came
    // from among them, never the spec's.
    let record: Value = serde_json::from_slice(
        &std::fs::read(world.home.join(".lnk/safe/agents/main/schedules.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["schema"], 1, "{record}");
    let record = record["schedules"].as_array().unwrap();
    assert_eq!(record.len(), 2, "{record:?}");
    assert!(
        record.iter().all(|k| k["origin"]["adapter"] == "openclaw"),
        "{record:?}"
    );
    let spec =
        std::fs::read_to_string(world.home.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(
        !spec.contains("revision") && !spec.contains("origin"),
        "{spec}"
    );
    assert_eq!(
        std::fs::read(world.home.join(".openclaw/cron/jobs.json")).unwrap(),
        original
    );
    let managed = world
        .home
        .join(link_files::layout::Hidden::AgentData.relative_to_home())
        .join("main/harnesses/openclaw/.openclaw/cron/jobs.json");
    let cleared: Value = serde_json::from_slice(&std::fs::read(managed).unwrap()).unwrap();
    assert!(cleared["jobs"].as_array().unwrap().is_empty());
    // Supply a configured model without calling it. Harness execution is /usr/bin/true.
    let path = world.home.join("Link/Safe/Agents/main/agent.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut setup: toml::Table = toml::from_str(&text).unwrap();
    setup.insert(
        "model".into(),
        toml::Value::try_from(json!({"provider":"anthropic","name":"claude-haiku-5-5"})).unwrap(),
    );
    std::fs::write(path, toml::to_string(&setup).unwrap()).unwrap();
    for harness in ["link", "hermes", "openclaw", "link"] {
        ok(world.run(&["harness", "set", harness, "--yes", "--foreground"]));
        assert_eq!(
            world.definitions(),
            definitions,
            "definitions changed switching to {harness}"
        );
    }
    assert_eq!(
        std::fs::read(world.home.join(".openclaw/cron/jobs.json")).unwrap(),
        original
    );
}
