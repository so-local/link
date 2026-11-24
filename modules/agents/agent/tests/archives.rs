//! An agent carried by `lnk archive`: the real archive module asking this
//! module's answers, behind a core that lists only this module.

use std::path::{Path, PathBuf};
use std::process::Output;

use link_test_kit::{TempDir, env};

/// A machine whose `lnk` lists the agent module as answering the archive
/// questions, the real `lnk-agent` beside it.
fn machine(test: &str) -> (TempDir, PathBuf) {
    let home = TempDir::new(test);
    let bin = home.join("bin");
    crate::common::harness(&bin);
    let lnk = bin.join("lnk");
    link_test_kit::write_core(
        &lnk,
        r#"#!/bin/sh
case "$*" in
  "module list --json")
    echo '[{"name": "agent", "installed": true, "bare": true, "answers": ["archive.check", "archive.running", "archive.carry", "archive.missing", "archive.adopt"]}]' ;;
  *) echo "fake lnk: $*" >&2; exit 1 ;;
esac
"#,
    )
    .unwrap();
    (home, lnk)
}

fn archive(home: &Path, lnk: &Path, args: &[&str]) -> Output {
    env::command(link_test_kit::programs().join("lnk-archive"), home)
        .env("LNK_EXE", lnk)
        .args(args)
        .output()
        .unwrap()
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

fn ok(output: &Output) -> String {
    let text = said(output);
    assert!(output.status.success(), "{text}");
    text
}

const ID: &str = "c0d3r1a2b3c4";

/// Agent `name` on Link Harness with a hosted model, a skill, a note
/// and a Telegram connection.
fn an_agent(home: &Path, name: &str, harness: &str) {
    crate::common::write_agent(
        home,
        name,
        &format!(
            "id = \"agent:{ID}\"\nport = 23000\nharness = \"{harness}\"\nfiles = \"~/Link/Agents/{name}/home\"\n\
             [model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-secret-key\"\n\n{}",
            crate::common::connection(
                "telegram",
                &["token"],
                "token = \"123:SECRET\"",
                "id = \"7\"\nname = \"programmer_bot\"",
            )
        ),
    );
    let own = home.join("Link/Agents").join(name);
    std::fs::create_dir_all(own.join("home")).unwrap();
    std::fs::write(own.join("home/notes.txt"), "notes").unwrap();
    let skill = own.join("agent/skills/review");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: review\ndescription: Review a diff\n---\n# Review\n",
    )
    .unwrap();
}

/// The evidence of a call proposed in work this machine names: the
/// agent's ID, its session and its attempt here.
fn proposed_here() -> link_harness_wire::evidence::Evidence {
    use link_harness_wire::evidence::{Evidence, Phase, Proposal, Record};
    Evidence::new(Record::Proposed(Proposal {
        phase: Phase::Turn,
        attempt: "turn-1".into(),
        work: Some(link_interactions_contract::WorkRef {
            kind: link_interactions_contract::WorkKind::Conversation,
            owner: link_interactions_contract::Actor::Owner,
            agent: ID.into(),
            session: "link:main".into(),
            task: None,
            attempt: "turn-1".into(),
            generation: 1,
            control_revision: 0,
        }),
        started_unix_ms: 0,
        calls: Vec::new(),
    }))
}

/// Its `link:main` chat: a message, then the evidence of a call
/// proposed in work this machine names.
fn a_chat(home: &Path, name: &str) {
    let chat = home
        .join("Link/Agents")
        .join(name)
        .join("agent/chats/link-main");
    std::fs::create_dir_all(&chat).unwrap();
    let said =
        serde_json::json!({"seq": 1, "role": "user", "text": "Hi", "at": "2026-11-20T09:00:00Z"});
    let proposed = serde_json::json!({"seq": 2, "role": "evidence", "text": "",
        "at": "2026-11-20T09:00:01Z", "evidence": proposed_here()});
    std::fs::write(chat.join("events.jsonl"), format!("{said}\n{proposed}\n")).unwrap();
}

/// What a `.tar.gz` holds, decompressed.
fn unpacked(path: &Path) -> String {
    let mut text = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap()),
        &mut text,
    )
    .unwrap();
    String::from_utf8_lossy(&text).into_owned()
}

#[test]
fn an_exported_agent_is_imported_as_a_new_agent_without_its_secrets() {
    let (here, here_lnk) = machine("archive-agent-here");
    let (there, there_lnk) = machine("archive-agent-there");
    an_agent(&here, "programmer", "link");
    a_chat(&here, "programmer");
    let exported = ok(&archive(
        &here,
        &here_lnk,
        &["export", "--select", "agent.programmer"],
    ));
    let file = here.join("Link/Archive/programmer.tar.gz");
    assert!(file.is_file(), "{exported}");
    // State only: no key, token, ID or port of this machine's.
    let inside = unpacked(&file);
    for secret in [
        "sk-ant-secret-key",
        "123:SECRET",
        ID,
        "23000",
        "identity.toml",
    ] {
        assert!(!inside.contains(secret), "{secret} is in the archive");
    }
    assert!(inside.contains("Agents/programmer/agent/skills/review/SKILL.md"));

    let path = file.display().to_string();
    let imported = ok(&archive(
        &there,
        &there_lnk,
        &["import", &path, "--select", "agent.programmer=agent.mine"],
    ));
    assert!(
        imported.contains("lnk agent start --agent mine"),
        "{imported}"
    );
    assert!(imported.contains("anthropic"), "{imported}");
    assert!(
        imported.contains("lnk agent connect telegram --agent mine"),
        "{imported}"
    );
    let identity = crate::common::record(&there, "mine", "identity.toml");
    let id = identity["id"].as_str().unwrap();
    assert_ne!(id, ID, "a copy has an ID of its own");
    assert!(identity["service"].is_string() && identity["owner"]["kind"].is_string());
    assert!(crate::common::record(&there, "mine", "here.toml")["port"].is_u64());
    let settings = std::fs::read_to_string(there.join("Link/Safe/Agents/mine/agent.toml")).unwrap();
    assert!(settings.contains("~/Link/Agents/mine/home"), "{settings}");
    assert_eq!(
        std::fs::read_to_string(there.join("Link/Agents/mine/home/notes.txt")).unwrap(),
        "notes"
    );
    // Its chat whole, each event read as it was written, the work it was
    // proposed in none: what named this machine's stayed here.
    let log =
        std::fs::read_to_string(there.join("Link/Agents/mine/agent/chats/link-main/events.jsonl"))
            .unwrap();
    let events: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 2, "{log}");
    assert_eq!(events[0]["text"], "Hi");
    assert!(
        log.lines().nth(1).unwrap().starts_with(r#"{"seq":2,"#),
        "{log}"
    );
    let evidence: link_harness_wire::evidence::Evidence =
        serde_json::from_value(events[1]["evidence"].clone()).unwrap();
    let mut expected = proposed_here();
    if let link_harness_wire::evidence::Record::Proposed(proposal) = &mut expected.record {
        proposal.work = None;
    }
    assert_eq!(evidence, expected);
    // Here, its chat is as it was.
    let kept = std::fs::read_to_string(
        here.join("Link/Agents/programmer/agent/chats/link-main/events.jsonl"),
    )
    .unwrap();
    assert!(kept.contains(ID), "{kept}");
    // In its kind's list, by its new name.
    assert_eq!(link_agent_names(&there), ["mine"]);
}

/// The agents a machine's own files say it has.
fn link_agent_names(home: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(home.join(".lnk/safe/agents"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("identity.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn an_agent_on_another_harness_is_refused_by_name() {
    let (here, lnk) = machine("archive-agent-other");
    an_agent(&here, "work", "fake");
    let refused = archive(&here, &lnk, &["export", "--select", "agent.work"]);
    assert!(!refused.status.success());
    assert!(
        said(&refused).contains("only an agent on Link Harness"),
        "{}",
        said(&refused)
    );
    assert!(
        said(&refused).contains("lnk agent harness set link --agent work"),
        "{}",
        said(&refused)
    );
    assert!(!here.join("Link/Archive/work.tar.gz").exists());
}

#[test]
fn what_an_archive_grants_is_said_and_an_unknown_control_file_refused() {
    let (here, here_lnk) = machine("archive-agent-grants");
    let (there, there_lnk) = machine("archive-agent-granted");
    an_agent(&here, "programmer", "link");
    let control = here.join("Link/Safe/Agents/programmer");
    std::fs::write(control.join("approvals.toml"), "yes = true\n").unwrap();
    let refused = archive(
        &here,
        &here_lnk,
        &["export", "--select", "agent.programmer"],
    );
    assert!(!refused.status.success());
    assert!(
        said(&refused).contains("approvals.toml"),
        "{}",
        said(&refused)
    );
    std::fs::remove_file(control.join("approvals.toml")).unwrap();
    // A system's own file there is passed over.
    std::fs::write(control.join(".DS_Store"), "").unwrap();
    std::fs::write(
        control.join("sandbox.toml"),
        "schema = 1\n[network]\nmode = \"host\"\n",
    )
    .unwrap();
    // Exported from a machine where it ran without its sandbox.
    let settings = control.join("agent.toml");
    let text = std::fs::read_to_string(&settings).unwrap();
    let text: String = text
        .lines()
        .filter(|line| !line.starts_with("sandbox ="))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&settings, format!("sandbox = false\n{text}")).unwrap();
    ok(&archive(
        &here,
        &here_lnk,
        &["export", "--select", "agent.programmer"],
    ));
    let path = here
        .join("Link/Archive/programmer.tar.gz")
        .display()
        .to_string();
    let imported = ok(&archive(&there, &there_lnk, &["import", &path]));
    assert!(imported.contains("opens more than Link's"), "{imported}");
    assert!(
        imported.contains("lnk agent permission remove --agent programmer"),
        "{imported}"
    );
}
