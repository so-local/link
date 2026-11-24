//! Count the actual protocol calls of one sample. The tiny shell
//! stand-in only dispatches saved JSON; all scenarios and assertions are Rust.

use std::path::PathBuf;

use link_measure_contract::client::{
    asked::{Agent, Inventory},
    traffic::Source,
};
use link_test_kit::{TempDir, env};
use serde_json::{Value, json};

struct World {
    root: TempDir,
    home: PathBuf,
}

impl World {
    fn new() -> Self {
        let root = TempDir::new("measure-runtime");
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        // `lnk`, and each module's program beside it, one stand-in that
        // logs what it was run as: `module list --json` for `lnk`, `<module>
        // --link-answer <question>` for a module's.
        let stand_in = r#"#!/bin/sh
here=$(dirname "$0")
case "$(basename "$0")" in
  lnk) who= ;;
  *) who="$(basename "$0" | sed 's/^lnk-//') " ;;
esac
printf '%s%s\n' "$who" "$*" >> "$here/asked"
# Each answer reads its question first, as a module does: one that
# exits before it's asked breaks the asker's pipe.
cat >/dev/null
answer() { printf '{"protocol": 1, "answer": '; cat "$1"; echo '}'; }
case "$who$*" in
  'module list --json') cat "$here/modules.json" ;;
  'agent --link-answer measure.agents') answer "$here/agents.json" ;;
  'agent --link-answer measure.inventory') answer "$here/inventory.json" ;;
  'publisher --link-answer measure.traffic') echo '{"protocol": 1, "answer": []}' ;;
  'external --link-answer measure.traffic') answer "$here/external.json" ;;
  'job --link-answer measure.job') answer "$here/profile-path.json" ;;
  *) exit 91 ;;
esac
"#;
        for program in [
            "lnk",
            "lnk-agent",
            "lnk-publisher",
            "lnk-external",
            "lnk-job",
        ] {
            link_test_kit::write_program(&root.join(program), stand_in).unwrap();
        }
        Self { root, home }
    }

    fn agents(&self, agents: &[Agent], external: bool) {
        let mut modules = vec![
            json!({"name": "agent", "installed": true,
            "answers": ["measure.agents", "measure.inventory"]}),
            json!({"name": "publisher", "installed": true,
                "answers": ["measure.traffic"]}),
        ];
        if external {
            modules.push(json!({"name": "external", "installed": true,
                "answers": ["measure.traffic"]}));
        }
        std::fs::write(self.root.join("modules.json"), json!(modules).to_string()).unwrap();
        std::fs::write(
            self.root.join("agents.json"),
            serde_json::to_vec(agents).unwrap(),
        )
        .unwrap();
        let inventory = Inventory {
            agents: agents.to_vec(),
            traffic: vec![Source {
                provider: "publisher".into(),
                directory: self.root.join("traffic"),
            }],
        };
        std::fs::write(
            self.root.join("inventory.json"),
            serde_json::to_vec(&inventory).unwrap(),
        )
        .unwrap();
        let _ = std::fs::remove_file(self.root.join("asked"));
    }

    fn trusted_home(&self) -> bool {
        let path = self.home.join("trust-probe");
        link_files::write_private(&path, b"probe = true\n").unwrap();
        link_test_kit::gate::settings(&path)
    }

    fn sample(&self) -> (Value, Vec<String>) {
        let program = std::env::var_os("LNK_TEST_MEASURE_PROGRAM")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_lnk-measure").into());
        let output = env::command(program, &self.home)
            .arg("--json")
            .env("LNK_EXE", self.root.join("lnk"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = serde_json::from_slice(&output.stdout).unwrap();
        let calls = std::fs::read_to_string(self.root.join("asked"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        (value, calls)
    }

    fn agent(&self, id: &str, running: bool) -> Agent {
        Agent {
            id: id.into(),
            name: id.into(),
            key: id.into(),
            running,
            pids: vec![std::process::id()],
            ..Agent::default()
        }
    }
}

#[test]
fn empty_and_stopped_agents_never_ask_for_traffic() {
    let w = World::new();
    if !w.trusted_home() {
        return;
    }
    for agents in [vec![], vec![w.agent("a1", false)]] {
        w.agents(&agents, true);
        let (sample, calls) = w.sample();
        assert_eq!(
            calls,
            [
                "module list --json",
                "agent --link-answer measure.inventory"
            ]
        );
        assert_eq!(sample["units"][0]["kind"], "machine");
        assert!(
            sample["units"][0]["measures"]
                .as_object()
                .is_some_and(|m| !m.is_empty())
        );
        assert_eq!(sample["units"].as_array().unwrap().len(), 1 + agents.len());
    }
}

#[test]
fn snapshots_need_no_command_and_other_providers_answer_once_per_sample() {
    let w = World::new();
    if !w.trusted_home() {
        return;
    }
    let pid = std::process::id();
    let path = w.root.join(format!("traffic/{pid}.json"));
    link_files::write_private(
        &path,
        json!({"pid": pid, "sandbox": "main-link",
        "up": 10, "down": 20, "started": link_measure_contract::client::started(pid)})
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let child = link_test_kit::ChildGroup::spawn(env::command("sleep", &w.home).arg("30")).unwrap();
    std::fs::write(
        w.root.join("external.json"),
        json!([{"pid": child.id(), "sandbox":"outside",
        "up":30, "down":40}])
        .to_string(),
    )
    .unwrap();
    for external in [false, true] {
        w.agents(&[w.agent("a1", true), w.agent("a2", true)], external);
        let (sample, calls) = w.sample();
        let mut expected = vec![
            "module list --json",
            "agent --link-answer measure.inventory",
        ];
        if external {
            expected.push("external --link-answer measure.traffic");
        }
        assert_eq!(
            calls, expected,
            "a published provider was invoked or discovery repeated"
        );
        for id in ["a1", "a2"] {
            let own = sample["units"]
                .as_array()
                .unwrap()
                .iter()
                .find(|u| u["id"] == id)
                .unwrap();
            assert_eq!(
                own["measures"]["link.network.out.sum"]["now"],
                if external { 40.0 } else { 10.0 }
            );
            assert_eq!(
                own["measures"]["link.network.in.sum"]["now"],
                if external { 60.0 } else { 20.0 }
            );
        }
    }
}

#[test]
fn older_or_unavailable_inventory_providers_keep_their_protocol() {
    let w = World::new();
    if !w.trusted_home() {
        return;
    }
    for advertises in [false, true] {
        w.agents(&[w.agent("a1", true)], false);
        if advertises {
            // A registry newer than the installed provider: fall back.
            std::fs::write(w.root.join("inventory.json"), "unsupported").unwrap();
        } else {
            let path = w.root.join("modules.json");
            let mut modules: Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            modules[0]["answers"]
                .as_array_mut()
                .unwrap()
                .retain(|q| q != "measure.inventory");
            std::fs::write(path, modules.to_string()).unwrap();
        }
        let (sample, calls) = w.sample();
        let mut expected = vec!["module list --json"];
        if advertises {
            expected.push("agent --link-answer measure.inventory");
        }
        expected.extend([
            "agent --link-answer measure.agents",
            "publisher --link-answer measure.traffic",
        ]);
        assert_eq!(calls, expected);
        assert!(
            sample["units"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["id"] == "a1")
        );
    }
}

fn measure(w: &World, args: &[&str]) -> std::process::Output {
    let program = std::env::var_os("LNK_TEST_MEASURE_PROGRAM")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_lnk-measure").into());
    env::command(program, &w.home)
        .args(args)
        .env("LNK_SERVICES", "off")
        .env("LNK_EXE", w.root.join("lnk"))
        .output()
        .unwrap()
}

#[test]
fn each_home_names_its_own_watcher() {
    let root = TempDir::new("measure-names");
    let (one, other) = (root.join("one"), root.join("other"));
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    assert_ne!(link_measure::label_of(&one), link_measure::label_of(&other));
    assert_ne!(link_measure::unit_of(&one), link_measure::unit_of(&other));
    assert_ne!(link_measure::label_of(&one), link_measure::OLD_LABEL);
    assert_ne!(link_measure::unit_of(&one), link_measure::OLD_UNIT);
    // However the home is reached: a job's HOME names the service that runs it.
    let reached = root.join("one/../one");
    assert_eq!(
        link_measure::label_of(&one),
        link_measure::label_of(&reached)
    );
    assert!(link_measure::unit_of(&one).ends_with(".service"));
}

#[test]
fn stopping_takes_this_homes_service_of_the_old_name_with_it() {
    let w = World::new();
    let old = if cfg!(target_os = "macos") {
        w.home
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", link_measure::OLD_LABEL))
    } else {
        w.home
            .join(".config/systemd/user")
            .join(link_measure::OLD_UNIT)
    };
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, "an earlier lnk's").unwrap();
    let stopped = measure(&w, &["stop"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(!old.exists());
}

#[test]
fn settings_are_saved_shown_as_a_table_and_refused_with_a_next() {
    let w = World::new();
    let saved = measure(&w, &["set", "--every", "30s"]);
    assert!(saved.status.success());
    let shown = String::from_utf8_lossy(&saved.stdout);
    assert!(shown.contains("every") && shown.contains("30s") && shown.contains("yours"));
    let refused = measure(&w, &["set", "--folders-max", "0"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(refused.stdout.is_empty());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("lnk measure get"));
}

#[test]
fn get_shows_the_settings_and_where_looks_go() {
    let w = World::new();
    let shown = measure(&w, &["get"]);
    assert!(shown.status.success(), "{shown:?}");
    let said = String::from_utf8_lossy(&shown.stdout);
    assert!(said.contains("every") && said.contains("Link's"), "{said}");
    assert!(said.contains("Measures stay on this machine."), "{said}");
}

#[test]
fn set_and_export_set_refuse_saying_nothing_to_set() {
    let w = World::new();
    let set = measure(&w, &["set"]);
    assert_eq!(set.status.code(), Some(2), "{set:?}");
    assert!(String::from_utf8_lossy(&set.stderr).contains("lnk measure get"));
    let export = measure(&w, &["export", "set"]);
    assert_eq!(export.status.code(), Some(2), "{export:?}");
    assert!(String::from_utf8_lossy(&export.stderr).contains("--url"));
    let both = measure(&w, &["export", "set", "--off", "--job", "a1"]);
    assert_eq!(both.status.code(), Some(2), "{both:?}");
}

#[test]
fn an_endpoint_with_a_login_is_refused_and_a_header_file_is_the_way() {
    let w = World::new();
    let refused = measure(
        &w,
        &["export", "set", "--url", "http://user:pw@localhost:4318/"],
    );
    assert_eq!(refused.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--header-file"));
}

#[test]
fn retained_job_profiles_are_read_without_starting_the_watcher() {
    use link_measure_contract::{Point, Report, Unit, otlp};
    let w = World::new();
    let path = w.root.join("profile.jsonl");
    let samples = [
        Report {
            unit: Unit {
                id: "a1".into(),
                kind: "job".into(),
                ..Unit::default()
            },
            points: vec![Point::new("link.cpu.used.sum", 2.5, 100)],
        },
        Report {
            unit: Unit {
                id: "a1/build".into(),
                kind: "stage".into(),
                parent: Some("a1".into()),
                ..Unit::default()
            },
            points: vec![Point::new("link.memory.used.sum", 100.0, 100)],
        },
    ];
    std::fs::write(&path, otlp(&samples, "lnk-job").to_string()).unwrap();
    std::fs::write(
        w.root.join("modules.json"),
        json!([{
            "name": "job", "installed": true, "answers": ["measure.job"]
        }])
        .to_string(),
    )
    .unwrap();
    std::fs::write(w.root.join("profile-path.json"), json!(path).to_string()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let output = measure(&w, &["--job", "a1", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["units"][0]["kind"], "job");
    assert_eq!(
        value["units"][0]["measures"]["link.cpu.used.sum"]["value"],
        2.5
    );
    assert_eq!(value["units"][1]["parent"], "a1");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(w.root.join("asked"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["module list --json", "job --link-answer measure.job"]
    );
    assert!(
        !measure(&w, &["--job", "../other", "--json"])
            .status
            .success()
    );
    assert!(
        !measure(&w, &["--job", "a1", "--agent", "main"])
            .status
            .success()
    );
    assert!(
        !measure(&w, &["export", "set", "--job", "a1"])
            .status
            .success()
    );
}

mod source_export;
mod source_selectors;
