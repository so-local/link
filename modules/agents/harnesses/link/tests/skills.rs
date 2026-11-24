//! Link Harness using a skill, for real, on Linux: a skill in the files
//! folder's `skills/`, as `lnk agent skill add` copies one, listed in
//! the prompt, and read through Link Memory (`lnk-memory-tool serve
//! <files>`) by Link Harness in a proxied sandbox, reaching it at
//! `$LNK_MEMORY` as `lnk agent start` gives it.
//!
//! The model is a fake on this machine that knows no skill: it picks one
//! from the prompt's list by the words of what it's asked, reads its
//! `SKILL.md` and answers with the step it read. So an answer holding
//! the step found the skill in the prompt and read it through memory.
//!
//! It runs `lnk-sandbox` and `lnk-memory-tool`, built in `link-core` and
//! `link-tools` beside this folder (`tests/common`).

use crate::common;

use std::path::Path;
use std::process::Stdio;

use serde_json::{Value, json};

mod evidence;

/// What the model does next: read the skill the prompt lists whose
/// description shares a word with the question, then answer with the
/// step it read.
fn next(body: &Value) -> Value {
    let messages = body["messages"].as_array().unwrap();
    let system = messages[0]["content"].as_str().unwrap_or_default();
    let asked = messages.iter().rposition(|m| m["role"] == "user").unwrap();
    let question = messages[asked]["content"].as_str().unwrap_or_default();
    let words: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
        .map(str::to_lowercase)
        .collect();
    let Some(last) = messages[asked + 1..].last() else {
        // The skills the prompt lists, as `- <name>: <description>`.
        let skill = system
            .split_once("# Skills\n")
            .map(|(_, list)| list)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.strip_prefix("- ")?.split_once(": "))
            .find(|(_, about)| words.iter().any(|w| about.to_lowercase().contains(w)));
        return match skill {
            Some((name, _)) => json!({"tool_calls": [{"index": 0, "id": "call_1",
                "type": "function", "function": {"name": "memory__read",
                "arguments": json!({"path": format!("skills/{name}/SKILL.md")}).to_string()}}]}),
            None => json!({"content": "No skill for that."}),
        };
    };
    // A tool's answer is fenced as third-party text: read what it holds.
    let answer: Value = serde_json::from_str(&link_untrusted::unfenced(
        last["content"].as_str().unwrap_or_default(),
    ))
    .unwrap_or_else(|_| panic!("a tool's answer isn't JSON: {last}"));
    let step = answer["text"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .find(|line| line.starts_with("1. "))
        .unwrap_or("no step");
    json!({"content": format!("From {}: {step}", answer["path"].as_str().unwrap_or_default())})
}

/// The model, speaking OpenAI's chat completions, streamed; it keeps
/// every request.
async fn fake_model() -> (u16, common::Asked) {
    common::fake_model(|body, _| vec![common::delta(next(body))]).await
}

/// In the sandbox: Link Harness as `lnk agent start` runs it, and each
/// message after the first three arguments sent to it in turn, each
/// answer printed after ANSWER.
const INSIDE: &str = r#"
export HOME="$1" LNK_FILES="$2" LNK_AGENT=main LNK_PORT=18800 LNK_MEMORY="$LNK_TOOL_MEMORY"
PROGRAM=$3
"$PROGRAM" serve 2>"$HOME/harness.log" &
shift 3
for _ in $(seq 50); do curl -s --noproxy '*' http://127.0.0.1:18800/health >/dev/null && break; sleep 0.1; done
for message in "$@"; do
  printf 'ANSWER %s\n' "$(printf 'test-key\n%s' "$message" | "$PROGRAM" ask telegram:42)"
done
kill %1
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_skill_is_listed_and_read_when_asked() {
    let (sandbox, memory) = (common::sandbox(), common::memory());
    if !common::sandboxed(&[&sandbox, &memory]) {
        return;
    }
    let root = link_test_kit::TempDir::new("skills");
    let (home, harness_home, files) = (root.join("home"), root.join("harness"), root.join("files"));
    for dir in [&home, &harness_home, &files] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // Two skills, and a folder that isn't one: no SKILL.md.
    for (name, about, step) in [
        (
            "sourdough",
            "Bake sourdough bread from a starter. Use for baking questions.",
            "1. Feed the starter at 8 pm, 1:5:5.",
        ),
        (
            "tides",
            "Read a tide table for sailing.",
            "1. Find the harbour's datum.",
        ),
    ] {
        let folder = files.join("skills").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {about}\n---\n\n# Steps\n\n{step}\n"),
        )
        .unwrap();
    }
    std::fs::create_dir_all(files.join("skills/drafts")).unwrap();

    let (port, asked) = fake_model().await;
    std::fs::write(
        harness_home.join("config.json"),
        json!({"endpoint_key": "test-key", "model": {"provider": "local", "name": "fake",
            "base_url": format!("http://127.0.0.1:{port}")}})
        .to_string(),
    )
    .unwrap();

    let lnk = |args: Vec<String>| {
        // The kit's environment: its own runtime folder, so no bridge a
        // test before it left serves it.
        let mut command = link_test_kit::env::command(&sandbox, &home);
        command.args(args).current_dir(&root).stdin(Stdio::null());
        command
    };
    let path = |p: &Path| p.display().to_string();
    let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let mut add = args(&["tool", "add", "memory"]);
    add.extend(args(&[
        "--add", "search", "--add", "read", "--add", "list", "--",
    ]));
    add.extend([path(&memory), "serve".into(), path(&files)]);
    let output = lnk(add).output().unwrap();
    assert!(output.status.success(), "{output:?}");

    let program = path(Path::new(env!("CARGO_BIN_EXE_lnk-harness")));
    let bin = path(Path::new(&program).parent().unwrap());
    let mut run = args(&[
        "run",
        "--commands.toolchains",
        "shell",
        "--internet.hosts",
        "*",
        "--tools.names",
        "memory",
    ]);
    run.extend(["--localhost.models".into(), port.to_string()]);
    run.extend(["--files.read".into(), bin]);
    // The harness itself runs inside, named.
    run.extend(["--commands.allow".into(), program.clone()]);
    run.extend(["--files.write".into(), path(&files)]);
    run.extend(["--files.write".into(), path(&harness_home)]);
    run.extend(args(&["--", "sh", "-c", INSIDE, "sh"]));
    run.extend([path(&harness_home), path(&files), program]);
    run.extend(args(&["How do I start baking sourdough?"]));
    let mut command = lnk(run);
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    let log = std::fs::read_to_string(harness_home.join("harness.log")).unwrap_or_default();
    let answer = text
        .lines()
        .find_map(|l| l.strip_prefix("ANSWER "))
        .unwrap_or_else(|| panic!("{text}\n{log}"));
    let said = answer;
    assert_eq!(
        said, "From skills/sourdough/SKILL.md: 1. Feed the starter at 8 pm, 1:5:5.",
        "{text}\n{log}"
    );

    // The prompt listed both skills, by name, and only the skills: the
    // folder without a SKILL.md is none.
    let asked = asked.lock().unwrap().clone();
    let system = asked[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains(
            "- sourdough: Bake sourdough bread from a starter. Use for baking questions.\n\
             - tides: Read a tide table for sailing."
        ),
        "{system}"
    );
    assert!(!system.contains("drafts"), "{system}");
    // ... and never the skill's own instructions, until it's read.
    assert!(!system.contains("Feed the starter"), "{system}");
}
