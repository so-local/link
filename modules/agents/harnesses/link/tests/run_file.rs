//! Link Harness running a program and editing files for real, on Linux,
//! through `run` and `file` as `lnk agent start` defines them: each a
//! tool outside its sandbox (`lnk sandbox tool add run -- lnk-run-tool
//! serve ... -- lnk sandbox run <flags>`), each call in a sandbox of its
//! own that reads the files folder and writes only `work/`, and Link
//! harness in a proxied sandbox reaching them at `$LNK_RUN` and
//! `$LNK_FILE`.
//!
//! A fake model on this machine calls `run__command` to write a file in
//! `work/`, `file__write` to rewrite a conversation (refused), and
//! `file__read` on the file the run wrote, then answers with what it
//! read.
//!
//! It runs `lnk-sandbox`, `lnk-run-tool` and `lnk-file-tool`, built in
//! `link-core` and `link-tools` beside this folder (`tests/common`); a
//! module runs another, never links it.

use crate::common;

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// The calls the fake model makes, one a round, then its answer.
fn calls() -> [(&'static str, Value); 3] {
    [
        (
            "run__command",
            json!({"command": "echo 'the answer is 42' > work/made.txt",
                "description": "Keeps the answer"}),
        ),
        (
            "file__write",
            json!({"file_path": "chats/telegram-42/conversation.md", "content": "forged"}),
        ),
        ("file__read", json!({"file_path": "work/made.txt"})),
    ]
}

/// An OpenAI-compatible model: each round makes the next of [`calls`],
/// and once they're made answers with every tool's answer. It keeps
/// what it was asked.
async fn fake_model() -> (u16, common::Asked) {
    common::fake_model(|body, _| {
        let messages = body["messages"].as_array().unwrap();
        // Each tool's answer, read through its fence as third-party text.
        let answers: Vec<String> = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| link_untrusted::unfenced(m["content"].as_str().unwrap_or("")))
            .collect();
        let delta = match calls().get(answers.len()) {
            Some((name, arguments)) => json!({"tool_calls": [{"index": 0,
                "id": format!("call_{}", answers.len()), "type": "function",
                "function": {"name": name, "arguments": arguments.to_string()}}]}),
            None => json!({"content": answers.join("\n---\n")}),
        };
        vec![common::delta(delta)]
    })
    .await
}

/// In the sandbox: Link Harness as `lnk agent start` runs it, and one
/// message to it, whose answer is printed after ANSWER.
const INSIDE: &str = r#"
export HOME="$1" LNK_FILES="$2" LNK_AGENT=main LNK_PORT=18800
export LNK_RUN="$LNK_TOOL_RUN" LNK_FILE="$LNK_TOOL_FILE"
"$3" serve 2>"$HOME/harness.log" &
for _ in $(seq 50); do curl -s --noproxy '*' http://127.0.0.1:18800/health >/dev/null && break; sleep 0.1; done
printf 'ANSWER %s\n' "$(printf 'test-key\nWork out the answer and keep it.' | "$3" ask telegram:42)"
kill %1
"#;

#[tokio::test(flavor = "multi_thread")]
async fn link_harness_runs_and_edits_each_call_in_a_sandbox_of_its_own() {
    let (sandbox, run, file) = (common::sandbox(), common::run(), common::file());
    if !common::sandboxed(&[&sandbox, &run, &file]) {
        return;
    }
    // In full: a sandbox's path never has `..`.
    let file = file.canonicalize().unwrap();
    let root = link_test_kit::TempDir::new("run_file");
    let (home, harness_home, files) = (root.join("home"), root.join("harness"), root.join("files"));
    for dir in [
        &home,
        &harness_home,
        &files.join("work"),
        &files.join("chats"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
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
    // Each call's sandbox, as Link makes it from Link's sandbox file:
    // the files folder read, work/ written.
    let each_call = |also: &[String]| {
        let mut flags = vec![path(&sandbox), "run".into()];
        flags.extend(["--files.read".into(), path(&files)]);
        flags.extend(["--files.write".into(), path(&files.join("work"))]);
        flags.extend(also.iter().cloned());
        flags
    };
    let mut add = args(&["tool", "add", "run", "--add", "command", "--"]);
    add.extend([path(&run), "serve".into(), "--files".into(), path(&files)]);
    add.extend(args(&[
        "--programs-path",
        "/usr/bin:/bin",
        "--timeout",
        "20s",
    ]));
    add.extend(args(&["--output-bytes", "4096", "--program", "sh", "--"]));
    add.extend(each_call(&args(&["--commands.toolchains", "shell"])));
    let output = lnk(add).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let mut add = args(&["tool", "add", "file"]);
    for call in ["read", "write", "replace", "find"] {
        add.extend(["--add".into(), call.into()]);
    }
    add.extend(["--".into(), path(&file), "serve".into(), "--".into()]);
    add.extend(each_call(&[
        "--files.read".into(),
        path(&file),
        "--commands.allow".into(),
        path(&file),
    ]));
    add.extend(["--".into(), path(&file), "apply".into()]);
    // Its own folder is its files folder too, as Link Harness keeps an
    // agent with no own folder beside it.
    add.extend([
        "--files".into(),
        path(&files),
        "--agent-folder".into(),
        path(&files),
        "--write".into(),
        "work".into(),
    ]);
    let output = lnk(add).output().unwrap();
    assert!(output.status.success(), "{output:?}");

    // The program alone, as its launch reads it, not the folder it's in.
    let program = path(Path::new(env!("CARGO_BIN_EXE_lnk-harness")));
    let mut sandboxed = args(&[
        "run",
        "--commands.toolchains",
        "shell",
        "--internet.hosts",
        "*",
        "--tools.names",
        "run",
        "--tools.names",
        "file",
    ]);
    sandboxed.extend(["--localhost.models".into(), port.to_string()]);
    sandboxed.extend(["--files.read".into(), program.clone()]);
    // The harness itself runs inside, named.
    sandboxed.extend(["--commands.allow".into(), program.clone()]);
    sandboxed.extend(["--files.read".into(), path(&files)]);
    sandboxed.extend(["--files.write".into(), path(&files.join("chats"))]);
    sandboxed.extend(["--files.write".into(), path(&harness_home)]);
    sandboxed.extend(args(&["--", "sh", "-c", INSIDE, "sh"]));
    sandboxed.extend([path(&harness_home), path(&files), program]);
    let mut command = lnk(sandboxed);
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    let log = std::fs::read_to_string(harness_home.join("harness.log")).unwrap_or_default();
    // Its answer, whole: every line after ANSWER.
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let said = stdout
        .split_once("ANSWER ")
        .map(|(_, answer)| answer.trim_end())
        .unwrap_or_else(|| panic!("{text}\n{log}"));
    let answers: Vec<&str> = said.split("\n---\n").collect();
    assert_eq!(answers.len(), 3, "{said}\n{log}");
    // The run wrote work/, in its own sandbox.
    assert!(answers[0].starts_with("exit 0"), "{said}");
    assert_eq!(
        std::fs::read_to_string(files.join("work/made.txt")).unwrap(),
        "the answer is 42\n"
    );
    // File refused the conversation, and read what the run wrote, each
    // line numbered as its read numbers them.
    assert!(answers[1].contains("you may not write chats/"), "{said}");
    assert_eq!(answers[2], "     1→the answer is 42", "{said}");
    let copy = std::fs::read_to_string(files.join("chats/telegram-42/conversation.md"))
        .unwrap_or_default();
    assert!(!copy.contains("forged"), "{copy}");

    // The model was offered run's and file's tools, by their own names.
    let asked = asked.lock().unwrap().clone();
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
            "file__edit",
            "file__glob",
            "file__grep",
            "file__read",
            "file__write",
            "run__command"
        ]
    );
}

/// A model that starts a background command, says so, and once told it
/// ended (a message from it, not the owner), says what it heard.
async fn background_model() -> (u16, common::Asked) {
    common::fake_model(|body, _| {
        let messages = body["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        let text = last["content"].to_string();
        let delta = if last["role"] == "user" && text.contains("background command") {
            json!({"content": format!("heard: {text}")})
        } else if last["role"] == "tool" {
            json!({"content": format!("started: {}", last["content"])})
        } else {
            json!({"tool_calls": [{"index": 0, "id": "call_0", "type": "function",
                "function": {"name": "run__command", "arguments": json!({
                    "command": "sleep 2; echo built > work/built.txt; echo all built",
                    "run_in_background": true})
                    .to_string()}}]})
        };
        vec![common::delta(delta)]
    })
    .await
}

/// In the sandbox: one message that starts a background command, the
/// harness then stopped; and, once its end reached the way in (`told`,
/// beside the harness's home), a new harness given it as the bridge
/// gives it, from `run:<id>`.
const BACKGROUND_INSIDE: &str = r#"
export HOME="$1" LNK_FILES="$2" LNK_AGENT=main LNK_PORT=18810
export LNK_RUN="$LNK_TOOL_RUN"
ask() {
  { printf 'test-key\n'; cat; } | "$PROGRAM" ask telegram:42 "$@"
}
up() {
  "$3" serve 2>>"$HOME/harness.log" &
  HARNESS=$!
  for _ in $(seq 50); do curl -s --noproxy '*' http://127.0.0.1:18810/health >/dev/null && break; sleep 0.1; done
}
PROGRAM=$3
up "$@"
printf 'FIRST %s\n' "$(printf 'Build it.' | ask)"
kill $HARNESS; wait $HARNESS
for _ in $(seq 300); do [ -s "$HOME/told" ] && break; sleep 0.1; done
up "$@"
printf 'SECOND %s\n' "$(ask --from "$(cat "$HOME/from")" <"$HOME/told")"
kill $HARNESS; wait $HARNESS
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_background_command_outlives_its_turn_and_its_end_is_heard() {
    let (sandbox, run) = (common::sandbox(), common::run());
    if !common::sandboxed(&[&sandbox, &run]) {
        return;
    }
    let root = link_test_kit::TempDir::new("run_file-bg");
    let (home, harness_home, files) = (root.join("home"), root.join("harness"), root.join("files"));
    for dir in [
        &home,
        &harness_home,
        &files.join("work"),
        &files.join("chats"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let (port, asked) = background_model().await;
    std::fs::write(
        harness_home.join("config.json"),
        json!({"endpoint_key": "test-key", "model": {"provider": "local", "name": "fake",
            "base_url": format!("http://127.0.0.1:{port}")}})
        .to_string(),
    )
    .unwrap();
    // The bridge's way in: the firing it took, handed to the sandbox as
    // the bridge would hand it to the harness.
    let way_in = root.join("way-in.sock");
    let keys = root.join("keys");
    link_store_contract::testing::bound(&keys, "aaaaaaaaaaaa");
    let taken = Arc::new(Mutex::new(Vec::<Value>::new()));
    {
        let (taken, harness_home) = (taken.clone(), harness_home.clone());
        link_agent_contract::testing::stand_in_bridge(&way_in, &keys, move |who, firing: Value| {
            // Told as its agent alone.
            assert_eq!(who.agent, "aaaaaaaaaaaa");
            std::fs::write(harness_home.join("from"), firing["from"].as_str().unwrap()).unwrap();
            std::fs::write(harness_home.join("told"), firing["text"].as_str().unwrap()).unwrap();
            taken.lock().unwrap().push(firing);
            json!({"answer": ""})
        });
    }

    let lnk = |args: Vec<String>| {
        // The kit's environment: its own runtime folder, so no bridge a
        // test before it left serves it.
        let mut command = link_test_kit::env::command(&sandbox, &home);
        command.args(args).current_dir(&root).stdin(Stdio::null());
        command
    };
    let path = |p: &Path| p.display().to_string();
    let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let mut add = args(&["tool", "add", "run"]);
    for call in ["command", "start", "list", "read", "stop"] {
        add.extend(["--add".into(), call.into()]);
    }
    add.push("--".into());
    add.extend([path(&run), "serve".into(), "--files".into(), path(&files)]);
    add.extend(args(&[
        "--programs-path",
        "/usr/bin:/bin",
        "--timeout",
        "20s",
    ]));
    add.extend(args(&["--output-bytes", "4096", "--program", "sh"]));
    add.extend(["--state".into(), path(&root.join("state"))]);
    add.extend(["--way-in".into(), path(&way_in)]);
    add.extend([
        "--as".into(),
        "aaaaaaaaaaaa".into(),
        "--keyring".into(),
        path(&keys),
    ]);
    add.extend(args(&["--background", "2", "--background-timeout", "1m"]));
    add.extend(args(&[
        "--background-output",
        "1048576",
        "--background-keep",
        "1h",
        "--background-per-day",
        "100",
    ]));
    add.extend(args(&["--hops", "8", "--"]));
    add.extend([path(&sandbox), "run".into()]);
    add.extend(["--files.read".into(), path(&files)]);
    add.extend(["--files.write".into(), path(&files.join("work"))]);
    add.extend(args(&["--commands.toolchains", "shell"]));
    let output = lnk(add).output().unwrap();
    assert!(output.status.success(), "{output:?}");

    let program = path(Path::new(env!("CARGO_BIN_EXE_lnk-harness")));
    let mut sandboxed = args(&[
        "run",
        "--commands.toolchains",
        "shell",
        "--internet.hosts",
        "*",
        "--tools.names",
        "run",
    ]);
    sandboxed.extend(["--localhost.models".into(), port.to_string()]);
    sandboxed.extend(["--files.read".into(), program.clone()]);
    // The harness itself runs inside, named.
    sandboxed.extend(["--commands.allow".into(), program.clone()]);
    sandboxed.extend(["--files.read".into(), path(&files)]);
    sandboxed.extend(["--files.write".into(), path(&files.join("chats"))]);
    sandboxed.extend(["--files.write".into(), path(&harness_home)]);
    sandboxed.extend(args(&["--", "sh", "-c", BACKGROUND_INSIDE, "sh"]));
    sandboxed.extend([path(&harness_home), path(&files), program]);
    let mut command = lnk(sandboxed);
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    let log = std::fs::read_to_string(harness_home.join("harness.log")).unwrap_or_default();
    let first = text
        .lines()
        .find_map(|l| l.strip_prefix("FIRST "))
        .unwrap_or_else(|| panic!("{text}\n{log}"));
    assert!(
        first.contains("started: ") && first.contains("Started "),
        "{first}\n{log}"
    );
    // Its end came once, to the conversation that started it, from it,
    // after the turn and the harness that started it had ended.
    let taken = taken.lock().unwrap().clone();
    assert_eq!(taken.len(), 1, "{taken:?}");
    assert_eq!(taken[0]["thread"], "telegram:42");
    let from = taken[0]["from"].as_str().unwrap();
    assert!(from.starts_with("run:"), "{from}");
    let ended = taken[0]["text"].as_str().unwrap();
    assert!(
        ended.contains("exit 0") && ended.contains("all built"),
        "{ended}"
    );
    assert_eq!(
        std::fs::read_to_string(files.join("work/built.txt")).unwrap(),
        "built\n"
    );
    // Heard as from it, never the owner.
    let second = text
        .lines()
        .find_map(|l| l.strip_prefix("SECOND "))
        .unwrap_or_else(|| panic!("{text}\n{log}"));
    let id = from.trim_start_matches("run:");
    assert!(
        second.contains(&format!(
            "from your background command {id}, not your owner"
        )),
        "{second}\n{log}"
    );
    let asked = asked.lock().unwrap().clone();
    let offered: Vec<&str> = asked[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    for name in ["run__command", "run__list", "run__read", "run__stop"] {
        assert!(offered.contains(&name), "{offered:?}");
    }
}
