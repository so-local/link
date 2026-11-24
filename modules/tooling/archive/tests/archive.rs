//! `lnk archive` end to end, its owner a stand-in module answering the
//! archive questions from files alone, as the agent module does.

use std::path::{Path, PathBuf};
use std::process::Output;

use link_test_kit::{TempDir, env, write_program};

link_test_kit::program!(system);

fn said(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A `lnk` listing one module, `agent`, answering the archive questions,
/// and its program beside it, which notes each question it's asked.
fn owner(home: &TempDir) -> PathBuf {
    let lnk = home.join("bin/lnk");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    write_program(
        &lnk,
        r#"#!/bin/sh
case "$*" in
  "module list --json")
    echo '[{"name": "agent", "installed": true, "answers": ["archive.check", "archive.running", "archive.carry", "archive.missing", "archive.adopt"]}]' ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    write_program(
        &home.join("bin/lnk-agent"),
        format!(
            r#"#!/bin/sh
read -r asked
echo "$*" >> "{log}"
name=$(printf '%s' "$asked" | sed 's/.*"name":"\([a-z0-9-]*\)".*/\1/')
case "$*" in
  "--link-answer archive.check")
    case "$asked" in *'"name":"broken"'*) echo "its agent.toml is refused" >&2; exit 1 ;; esac
    echo '{{"protocol": 1, "answer": {{}}}}' ;;
  "--link-answer archive.running")
    case "$asked" in
      *'"name":"busy"'*) echo '{{"protocol": 1, "answer": {{"stop": "lnk agent stop --agent busy"}}}}' ;;
      *) echo '{{"protocol": 1, "answer": {{}}}}' ;;
    esac ;;
  "--link-answer archive.carry")
    echo '{{"protocol": 1, "answer": {{}}}}' ;;
  "--link-answer archive.missing")
    echo '{{"protocol": 1, "answer": {{"needs": [{{"what": "Telegram", "setup": "lnk agent connect telegram"}}]}}}}' ;;
  "--link-answer archive.adopt")
    echo '{{"protocol": 1, "answer": {{"notes": [], "next": "lnk agent start --agent '"$name"'"}}}}' ;;
  *) exit 1 ;;
esac
"#,
            log = home.join("asked.log").display()
        ),
    )
    .unwrap();
    lnk
}

fn agent(home: &Path, name: &str) {
    let own = home.join("Link/Agents").join(name);
    std::fs::create_dir_all(own.join("home")).unwrap();
    std::fs::write(own.join("home/notes.txt"), "notes").unwrap();
    let safe = home.join("Link/Safe/Agents").join(name);
    std::fs::create_dir_all(&safe).unwrap();
    std::fs::write(safe.join("agent.toml"), "harness = \"link\"\n").unwrap();
}

fn archive(home: &TempDir, args: &[&str]) -> Output {
    env::command(env!("CARGO_BIN_EXE_lnk-archive"), home)
        .env("LNK_EXE", home.join("bin/lnk"))
        .args(args)
        .output()
        .unwrap()
}

fn ok(output: &Output) -> String {
    let text = said(&output.stdout) + &said(&output.stderr);
    assert!(output.status.success(), "{text}");
    text
}

fn refused(output: &Output, says: &str) {
    let text = said(&output.stdout) + &said(&output.stderr);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains(says), "{says}: {text}");
}

#[test]
fn an_agent_goes_from_one_machine_to_another_as_a_new_copy() {
    let (here, there) = (TempDir::new("archive-here"), TempDir::new("archive-there"));
    owner(&here);
    owner(&there);
    agent(&here, "programmer");
    let exported = ok(&archive(&here, &["export", "--select", "agent.programmer"]));
    let file = here.join("Link/Archive/programmer.tar.gz");
    assert!(file.is_file(), "{exported}");
    assert!(exported.contains("agent.programmer"), "{exported}");
    let asked = std::fs::read_to_string(here.join("asked.log")).unwrap();
    assert_eq!(
        asked,
        "--link-answer archive.check\n--link-answer archive.running\n--link-answer archive.carry\n"
    );

    let file = file.display().to_string();
    let imported = ok(&archive(&there, &["import", &file]));
    assert!(imported.contains("Imported programmer"), "{imported}");
    assert!(
        imported.contains("lnk agent connect telegram"),
        "{imported}"
    );
    assert!(
        imported.contains("lnk agent start --agent programmer"),
        "{imported}"
    );
    assert!(
        there
            .join("Link/Agents/programmer/home/notes.txt")
            .is_file()
    );
    assert!(
        there
            .join("Link/Safe/Agents/programmer/agent.toml")
            .is_file()
    );

    // Taken now, and no terminal to ask: refused, naming the way out.
    refused(
        &archive(&there, &["import", &file]),
        "--select agent.programmer=agent.",
    );
    let renamed = ok(&archive(
        &there,
        &["import", &file, "--select", "agent.programmer=agent.mine"],
    ));
    assert!(
        renamed.contains("lnk agent start --agent mine"),
        "{renamed}"
    );
    assert!(there.join("Link/Agents/mine/home/notes.txt").is_file());
}

#[test]
fn export_refuses_what_its_owner_refuses() {
    let here = TempDir::new("archive-refuses");
    owner(&here);
    for name in ["busy", "broken"] {
        agent(&here, name);
    }
    refused(
        &archive(&here, &["export", "--select", "agent.busy"]),
        "lnk agent stop --agent busy",
    );
    refused(
        &archive(&here, &["export", "--select", "agent.broken"]),
        "its agent.toml is refused",
    );
    refused(
        &archive(&here, &["export", "--select", "agent.missing"]),
        "isn't here",
    );
    refused(
        &archive(&here, &["export", "--select", "box.gcp-1"]),
        "no module here exports",
    );
    refused(
        &archive(&here, &["export", "--select", "agent.busy=agent.idle"]),
        "keeps each thing's name",
    );
    refused(&archive(&here, &["export"]), "--select");
    refused(
        &archive(&here, &["export", "--all", "--select", "agent.busy"]),
        "--select",
    );
    assert!(!here.join("Link/Archive").exists());
}

#[test]
fn all_exports_every_agent_here_to_one_dated_archive() {
    let here = TempDir::new("archive-all");
    owner(&here);
    agent(&here, "programmer");
    agent(&here, "work");
    ok(&archive(&here, &["export", "--all"]));
    let written: Vec<_> = std::fs::read_dir(here.join("Link/Archive"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(
        written[0].ends_with(".tar.gz") && written[0].starts_with("20"),
        "{written:?}"
    );
    ok(&archive(&here, &["export", "--select", "agent.programmer"]));
    let default_single = here.join("Link/Archive/programmer.tar.gz");
    assert!(default_single.is_file());
    assert_eq!(
        link_archive::read_manifest(&default_single)
            .unwrap()
            .things
            .into_keys()
            .collect::<Vec<_>>(),
        ["agent.programmer"]
    );
    let to = here.join("both.tar.gz").display().to_string();
    ok(&archive(
        &here,
        &[
            "export",
            "--select",
            "agent.programmer,agent.work",
            "--to",
            &to,
        ],
    ));
    refused(
        &archive(
            &here,
            &["export", "--select", "agent.programmer", "--to", &to],
        ),
        "there already",
    );
    let single = here.join("one.tar.gz");
    ok(&archive(
        &here,
        &[
            "export",
            "--select",
            "agent.programmer",
            "--to",
            &single.display().to_string(),
        ],
    ));
    assert_eq!(
        link_archive::read_manifest(&single)
            .unwrap()
            .things
            .into_keys()
            .collect::<Vec<_>>(),
        ["agent.programmer"]
    );
    let full = TempDir::new("archive-all-import");
    owner(&full);
    ok(&archive(&full, &["import", &to]));
    assert!(full.join("Link/Agents/programmer/home/notes.txt").is_file());
    assert!(full.join("Link/Agents/work/home/notes.txt").is_file());
    let filtered = TempDir::new("archive-filtered-import");
    owner(&filtered);
    ok(&archive(
        &filtered,
        &["import", &to, "--select", "agent.work=agent.focused"],
    ));
    assert!(
        filtered
            .join("Link/Agents/focused/home/notes.txt")
            .is_file()
    );
    assert!(!filtered.join("Link/Agents/programmer").exists());
    assert!(!filtered.join("Link/Agents/work").exists());
    refused(
        &archive(
            &filtered,
            &["import", &to, "--select", "agent.work=agent.focused"],
        ),
        "focused already",
    );
    refused(
        &archive(&filtered, &["import", &to, "--select", "agent.absent"]),
        "agent.absent",
    );
    assert_eq!(
        std::fs::read_to_string(filtered.join("Link/Agents/focused/home/notes.txt")).unwrap(),
        "notes"
    );
}

/// An archive kept as a folder, as Link keeps its own agents in its
/// repository, is written as one archive and imports like any other;
/// asking nothing of its owner until import, and refusing a stray file.
#[test]
fn an_archive_kept_as_a_folder_exports_from_it_and_imports() {
    let (here, there) = (
        TempDir::new("archive-from-folder"),
        TempDir::new("archive-from-there"),
    );
    owner(&here);
    owner(&there);
    let folder = here.join("shelf/coach");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("archive.toml"),
        "schema = 1\ndescription = \"A coach\"\n\n[map]\n\"link.agents\" = \"Agents\"\n\
         \"link.agent-settings\" = \"Safe/Agents\"\n\n[\"agent.coach\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(folder.join("Agents/coach/home")).unwrap();
    std::fs::write(folder.join("Agents/coach/home/plan.md"), "# Plan\n").unwrap();
    std::fs::create_dir_all(folder.join("Safe/Agents/coach")).unwrap();
    std::fs::write(
        folder.join("Safe/Agents/coach/agent.toml"),
        "harness = \"link\"\n",
    )
    .unwrap();
    let from = folder.display().to_string();
    let exported = ok(&archive(&here, &["export", "--from", &from]));
    let file = here.join("Link/Archive/coach.tar.gz");
    assert!(file.is_file(), "{exported}");
    assert!(!here.join("asked.log").exists(), "packing asks no owner");

    let to = here.join("link-coach.tar.gz").display().to_string();
    ok(&archive(&here, &["export", "--from", &from, "--to", &to]));
    let imported = ok(&archive(&there, &["import", &to]));
    assert!(imported.contains("Imported coach"), "{imported}");
    assert!(there.join("Link/Agents/coach/home/plan.md").is_file());

    std::fs::write(folder.join("stray.txt"), "x").unwrap();
    let again = here.join("again.tar.gz").display().to_string();
    refused(
        &archive(&here, &["export", "--from", &from, "--to", &again]),
        "stray.txt",
    );
    refused(
        &archive(
            &here,
            &["export", "--from", &from, "--select", "agent.coach"],
        ),
        "--select",
    );
}
