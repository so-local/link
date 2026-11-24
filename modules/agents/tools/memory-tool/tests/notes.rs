//! The agent's notes, as Link serves them: `command` offers `remember`
//! only where Link names the notes (`LNK_MEMORY_NOTES`), as large as the
//! spec says; `serve --notes` writes them over MCP, and nothing else.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use link_memory_tool_contract::{FROM_ENV, NOTES_ENV, SPEC_ENV};
use serde_json::{Value, json};

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn remember_is_offered_only_where_link_names_the_notes() {
    let command = |notes: Option<&str>, spec: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-memory-tool"));
        command.args(["launch", "show", "/f", "/s"]);
        command
            .env_remove(NOTES_ENV)
            .env_remove(SPEC_ENV)
            .env_remove(FROM_ENV);
        if let Some(notes) = notes {
            command.env(NOTES_ENV, notes);
        }
        if let Some(spec) = spec {
            command.env(SPEC_ENV, spec);
        }
        command.output().unwrap()
    };
    let served = |notes: Option<&str>, spec: Option<&str>| {
        let output = command(notes, spec);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    // Not named: the reads alone, nothing written.
    let reads = served(None, None);
    assert_eq!(reads["allow"], json!(["links", "list", "read", "search"]));
    assert!(!reads["argv"].to_string().contains("--notes"));
    // Named: `remember` allowed, as large as the spec says.
    let notes = served(
        Some("/f/instructions/notes.md"),
        Some(r#"{"notes": "2 KB"}"#),
    );
    assert_eq!(
        notes["allow"],
        json!(["links", "list", "read", "search", "remember"])
    );
    let argv = notes["argv"].to_string();
    assert!(
        argv.contains(r#""--notes","/f/instructions/notes.md","--notes-most","2048""#),
        "{argv}"
    );
    // The engine's own most, without the spec's.
    let own = served(Some("/f/instructions/notes.md"), None);
    assert!(own["argv"].to_string().contains(r#""--notes-most","4096""#));
    // Notes outside the folder, in its conversations, not markdown, or a
    // most that isn't a size: refused.
    for (file, spec) in [
        ("/elsewhere/notes.md", None),
        ("/f/chats/t/conversation.md", None),
        ("/f/instructions/../x.md", None),
        ("/f/instructions/notes.txt", None),
        ("/f/instructions/notes.md", Some(r#"{"notes": "lots"}"#)),
    ] {
        let output = command(Some(file), spec);
        assert!(!output.status.success(), "{file} {spec:?}");
        assert!(output.stdout.is_empty(), "{file}");
    }
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn remember_writes_the_notes_over_mcp() {
    let root = link_test_kit::TempDir::new("notes");
    std::fs::create_dir_all(root.join("instructions")).unwrap();
    std::fs::create_dir_all(root.join("memory")).unwrap();
    let notes = root.join("instructions/notes.md");
    let mut command = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-memory-tool"), &root);
    command
        .arg("serve")
        .arg(&root)
        .arg("--notes")
        .arg(&notes)
        .args(["--notes-most", "60"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut server = link_test_kit::process::ChildGroup::spawn(&mut command).unwrap();
    let mut input = server.take_stdin().unwrap();
    let mut output = BufReader::new(server.take_stdout().unwrap());
    let mut ask = |message: Value| -> Value {
        writeln!(input, "{message}").unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let mut id = 0;
    let mut call = |ask: &mut dyn FnMut(Value) -> Value, name: &str, arguments: Value| {
        id += 1;
        ask(json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}}))["result"]
            .clone()
    };

    let init = ask(json!({"jsonrpc": "2.0", "id": 100, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18"}}));
    let instructions = init["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("'remember'"), "{instructions}");
    assert!(
        instructions.contains("Conversation history is not a saved memory"),
        "{instructions}"
    );
    assert!(
        instructions.contains("after 'remember' succeeds"),
        "{instructions}"
    );
    let tools = ask(json!({"jsonrpc": "2.0", "id": 101, "method": "tools/list"}));
    let remember = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "remember")
        .unwrap()
        .clone();
    let about = remember["description"].as_str().unwrap();
    assert!(
        !about.contains("notes.md") && about.contains("60 bytes"),
        "{about}"
    );

    let added = call(&mut ask, "remember", json!({"note": "Maya is my sister."}));
    assert_eq!(
        added["structuredContent"],
        json!({"done": "added", "path": "instructions/notes.md", "bytes": 21, "most": 60})
    );
    // Not searched: the notes are read every turn, and only `memory/`
    // is memory; what's there is found.
    std::fs::write(root.join("memory/family.md"), "My sister lives in Lyon.\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let found = call(&mut ask, "search", json!({"query": "sister"}));
    let paths: Vec<&str> = found["structuredContent"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["memory/family.md"], "{found}");
    // Past its most: refused, the model told how to make room.
    let full = call(&mut ask, "remember", json!({"note": "x".repeat(50)}));
    assert_eq!(full["isError"], true);
    assert!(
        full["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("change or forget some first"),
        "{full}"
    );
    let changed = call(
        &mut ask,
        "remember",
        json!({"note": "Maya is my older sister.", "replaces": "Maya"}),
    );
    assert_eq!(changed["structuredContent"]["done"], "changed");
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "- Maya is my older sister.\n"
    );
    let bad = call(&mut ask, "remember", json!({"note": 4}));
    assert_eq!(bad["isError"], true);
    drop(input);
    let status = link_test_kit::wait::eventually(
        "memory server exits after input closes",
        std::time::Duration::from_secs(5),
        || server.try_status().unwrap(),
    );
    assert!(status.success());
}

#[test]
fn without_notes_there_is_no_remember() {
    let root = link_test_kit::TempDir::new("notes-none");
    std::fs::create_dir_all(root.join("instructions")).unwrap();
    let index = link_memory_tool::Index::open(&root, None).unwrap();
    let tools = link_memory_tool::mcp::answer(
        &index,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string(),
    )
    .unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["links", "list", "read", "search"]);
    let call = link_memory_tool::mcp::answer(
        &index,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "remember", "arguments": {"note": "x"}}})
        .to_string(),
    )
    .unwrap();
    assert!(
        call["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no tool")
    );
    assert!(!root.join("instructions/notes.md").exists());
}

/// Where the agent's harness is sent others' messages (`LNK_MEMORY_FROM`),
/// `command` serves `remember` with `--from-others`, and `remember` writes only
/// in a conversation of the owner's that the call names: never a
/// teammate's, a team's, a subagent's or a stranger's, nor one the call
/// doesn't name, and it says why. Without it, every conversation is the
/// owner's.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn remember_is_the_owners_alone_where_others_send_messages() {
    let command = Command::new(env!("CARGO_BIN_EXE_lnk-memory-tool"))
        .args(["launch", "show", "/f", "/s"])
        .env(NOTES_ENV, "/f/instructions/notes.md")
        .env(FROM_ENV, "1")
        .env_remove(SPEC_ENV)
        .output()
        .unwrap();
    assert!(command.status.success(), "{command:?}");
    let served: Value = serde_json::from_slice(&command.stdout).unwrap();
    assert!(
        served["argv"].to_string().contains(r#""--from-others""#),
        "{served}"
    );

    for from in [true, false] {
        let root = link_test_kit::TempDir::new("notes-from");
        std::fs::create_dir_all(root.join("instructions")).unwrap();
        std::fs::create_dir_all(root.join("memory")).unwrap();
        let notes = root.join("instructions/notes.md");
        let mut server = Command::new(env!("CARGO_BIN_EXE_lnk-memory-tool"));
        server.arg("serve").arg(&root).arg("--notes").arg(&notes);
        if from {
            server.arg("--from-others");
        }
        let mut server = server
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = server.stdin.take().unwrap();
        let mut output = BufReader::new(server.stdout.take().unwrap());
        let mut id = 0;
        let mut remember = |note: &str, meta: Option<Value>| -> Value {
            id += 1;
            let mut params = json!({"name": "remember", "arguments": {"note": note}});
            if let Some(meta) = meta {
                params["_meta"] = meta;
            }
            writeln!(
                input,
                "{}",
                json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params})
            )
            .unwrap();
            let mut line = String::new();
            output.read_line(&mut line).unwrap();
            serde_json::from_str::<Value>(&line).unwrap()["result"].clone()
        };
        let thread = |t: &str| Some(json!({"link.local/thread": t, "link.local/hops": 1}));
        let others = [
            ("a teammate's", thread("agent-g7h8j9k0m1n2")),
            ("a team's", thread("team-t4m9q2x7k1ab")),
            ("a subagent's", thread("task-a1b2c3d4e5f6")),
            ("a stranger's", thread("webhook:deploys")),
            ("one not named", None),
            (
                "one named as nothing",
                Some(json!({"link.local/thread": 7})),
            ),
        ];
        for (whose, meta) in others {
            let said = remember(&format!("Noted in {whose}."), meta);
            match from {
                true => {
                    assert_eq!(said["isError"], true, "{whose}: {said}");
                    let text = said["content"][0]["text"].as_str().unwrap();
                    assert!(text.contains("remembering is the owner's alone"), "{text}");
                }
                false => assert_ne!(said["isError"], true, "{whose}: {said}"),
            }
        }
        // The owner's: a chat of theirs, and `lnk agent chat send`'s.
        for owners in ["telegram:123456789", "link:main"] {
            let said = remember(&format!("Noted in {owners}."), thread(owners));
            assert_ne!(said["isError"], true, "{owners}: {said}");
        }
        let written = std::fs::read_to_string(&notes).unwrap();
        assert!(
            written.contains("Noted in telegram:123456789."),
            "{written}"
        );
        assert_eq!(
            written.contains("Noted in a teammate's."),
            !from,
            "{written}"
        );
        drop(input);
        server.wait().unwrap();
    }
}
