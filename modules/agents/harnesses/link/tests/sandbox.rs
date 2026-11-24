//! Link Harness calling a tool for real, on Linux: `lnk sandbox tool add
//! memory -- lnk-memory-tool serve <files>`, and Link Harness in a
//! proxied sandbox (`lnk sandbox run --internet.hosts '*' --tools.names memory`), reaching it
//! at `$LNK_MEMORY` as `lnk agent start` gives it. A fake model on this
//! machine, let through with `--localhost.models`, calls `memory__search`; the
//! answer is Link Memory's, and the conversation keeps the call.
//!
//! It runs `lnk-sandbox` and `lnk-memory-tool`, built in `link-core` and
//! `link-tools` beside this folder (`tests/common`); a module runs
//! another, never links it.

use crate::common;

use std::path::Path;
use std::process::Stdio;

use serde_json::{Value, json};

/// An OpenAI-compatible model: offered `memory__search`, it calls it for
/// "Albuquerque"; given its answer, it says it back. It keeps what it
/// was asked.
async fn fake_model() -> (u16, common::Asked) {
    common::fake_model(|body, _| {
        let last = body["messages"].as_array().unwrap().last().unwrap().clone();
        let offered = body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|t| t["function"]["name"] == "memory__search");
        let delta = match (last["role"].as_str(), offered) {
            (Some("user"), true) => json!({"tool_calls": [{"index": 0, "id": "call_1",
                "type": "function", "function": {"name": "memory__search",
                "arguments": "{\"query\": \"Albuquerque\"}"}}]}),
            _ => {
                json!({"content": format!("Found: {}", link_untrusted::unfenced(last["content"].as_str().unwrap_or("nothing")))})
            }
        };
        vec![common::delta(delta)]
    })
    .await
}

/// In the sandbox: Link Harness as `lnk agent start` runs it, and one
/// message to it, whose answer is printed after ANSWER.
const INSIDE: &str = r#"
export HOME="$1" LNK_FILES="$2" LNK_AGENT=main LNK_PORT=18800 LNK_MEMORY="$LNK_TOOL_MEMORY"
"$3" serve 2>"$HOME/harness.log" &
for _ in $(seq 50); do curl -s --noproxy '*' http://127.0.0.1:18800/health >/dev/null && break; sleep 0.1; done
printf 'ANSWER %s\n' "$(printf 'test-key\nWhat did I say about Albuquerque?' | "$3" ask telegram:42)"
kill %1
"#;

#[tokio::test(flavor = "multi_thread")]
async fn link_harness_searches_memory_through_the_tool_bridge() {
    let (sandbox, memory) = (common::sandbox(), common::memory());
    if !common::sandboxed(&[&sandbox, &memory]) {
        return;
    }
    let root = link_test_kit::TempDir::new("sandbox");
    let (home, harness_home, files) = (root.join("home"), root.join("harness"), root.join("files"));
    for dir in [&home, &harness_home, &files.join("memory/notes")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(
        files.join("memory/notes/trip.md"),
        "# Trip\n\nWe drive to Albuquerque on Friday, leaving at 7.\n",
    )
    .unwrap();
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
    let mut add = args(&["tool", "add", "memory", "--add", "search", "--"]);
    add.extend([path(&memory), "serve".into(), path(&files)]);
    let output = lnk(add).output().unwrap();
    assert!(output.status.success(), "{output:?}");

    // The program alone, as its launch reads it, not the folder it's in.
    let program = path(Path::new(env!("CARGO_BIN_EXE_lnk-harness")));
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
    run.extend(["--files.read".into(), program.clone()]);
    // The harness itself runs inside, named.
    run.extend(["--commands.allow".into(), program.clone()]);
    run.extend(["--files.write".into(), path(&files)]);
    run.extend(["--files.write".into(), path(&harness_home)]);
    run.extend(args(&["--", "sh", "-c", INSIDE, "sh"]));
    run.extend([path(&harness_home), path(&files), program]);
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
    // Link Memory's answer: the note, found by the model's call.
    assert!(said.contains("memory/notes/trip.md"), "{said}\n{log}");
    assert!(said.contains("leaving at 7"), "{said}");

    // The model was offered Link Memory's tools, by their own names.
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "{asked:?}");
    let mut offered: Vec<&str> = asked[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    offered.sort();
    assert_eq!(
        offered,
        [
            "memory__links",
            "memory__list",
            "memory__read",
            "memory__search"
        ]
    );

    // The call and its answer are events of the conversation.
    let folder = files.join("chats/telegram-42");
    let events = common::conversation_events(&folder);
    let event = |seq: u64| -> Value { events[seq as usize - 1].clone() };
    assert_eq!(event(2)["calls"][0]["name"], "memory__search");
    assert_eq!(event(3)["role"], "tool");
    assert!(
        event(3)["text"]
            .as_str()
            .unwrap()
            .contains("memory/notes/trip.md")
    );
    assert_eq!(event(4)["role"], "assistant");
}
