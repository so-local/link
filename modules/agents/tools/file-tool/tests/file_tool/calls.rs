//! Link file's calls, over MCP: run in `lnk-file-tool apply` (the
//! program built here), one for a session's calls, reading the files
//! folder, writing only the folders it's given, never through a link;
//! and, on Linux with
//! bubblewrap, under `lnk sandbox run` as Link defines it, where the
//! sandbox holds to the same folders whatever `apply` is told.

use crate::{answered, finds, numbered, said};

use link_file_tool::answer;

mod stale;
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// An agent's two folders as Link lays them out: Home (`home/`), and
/// its own folder beside it (`agent/`), where its instructions, skills
/// and conversations are.
struct Agent {
    dir: TempDir,
}

impl Agent {
    fn home(&self) -> std::path::PathBuf {
        self.dir.join("home")
    }

    fn own(&self) -> std::path::PathBuf {
        self.dir.join("agent")
    }

    /// Where `path`, as the model gives it, is on disk.
    fn join(&self, path: &str) -> std::path::PathBuf {
        match link_agent_contract::folder::seen_collection(path) {
            Some(_) => self.own().join(path),
            None => self.home().join(path),
        }
    }

    /// A path of the test's own, in neither folder.
    fn scratch(&self, name: &str) -> std::path::PathBuf {
        self.dir.join(name)
    }
}

fn files(name: &str) -> Agent {
    let agent = Agent {
        dir: TempDir::new(&format!("calls-{name}")),
    };
    for folder in [
        "work",
        "instructions",
        "chats/telegram-1",
        "chats/telegram-2",
    ] {
        std::fs::create_dir_all(agent.join(folder)).unwrap();
    }
    std::fs::write(agent.join("chats/telegram-1/conversation.md"), "said\n").unwrap();
    std::fs::write(agent.join("chats/telegram-2/conversation.md"), "theirs\n").unwrap();
    agent
}

/// The program built here.
fn file() -> String {
    env!("CARGO_BIN_EXE_lnk-file-tool").to_string()
}

/// `lnk-file-tool apply` over the agent's two folders, writing `write`.
fn apply(files: &Agent, write: &[&str]) -> Vec<String> {
    let mut argv = vec![
        file(),
        "apply".into(),
        "--files".into(),
        files.home().display().to_string(),
        "--agent-folder".into(),
        files.own().display().to_string(),
    ];
    for folder in write {
        argv.extend(["--write".into(), folder.to_string()]);
    }
    argv
}

/// Call `name` in the owner's conversation `telegram:1`.
fn call(apply: &[String], name: &str, arguments: Value) -> Value {
    call_as(
        apply,
        name,
        arguments,
        json!({"link.local/thread": "telegram:1", "link.local/standing": "owner"}),
    )
}

/// Call `name` as `meta` (its `_meta`) says.
fn call_as(apply: &[String], name: &str, arguments: Value, meta: Value) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": arguments, "_meta": meta}});
    answer(apply, &message.to_string()).unwrap()["result"].clone()
}

fn refused(result: &Value) -> String {
    assert_eq!(result["isError"], true, "{result}");
    said(result).to_string()
}

/// A call's arguments are held to their declaration before any file is
/// touched: nothing is written.
#[test]
fn a_call_breaking_its_declaration_is_refused() {
    let files = files("declared");
    let apply = apply(&files, &["work"]);
    for (arguments, says) in [
        (
            json!({"file_path": "work/a.txt", "content": 5}),
            "invalid type",
        ),
        (
            json!({"file_path": "work/a.txt", "content": "a", "mode": "append"}),
            "unknown field `mode`",
        ),
    ] {
        let text = refused(&call(&apply, "write", arguments));
        assert!(text.contains(says), "{text}");
    }
    assert!(!files.join("work/a.txt").exists());
    // A path past its most is refused by its declaration, never echoed
    // back whole in an error.
    let long = format!("work/{}", "a".repeat(5000));
    let text = refused(&call(&apply, "read", json!({"file_path": long})));
    assert!(text.len() < 2000, "{}", text.len());
}

/// A write under the most a file may be is applied whatever it holds,
/// though JSON escapes each of its bytes to six; one past it is refused,
/// saying why.
#[test]
fn a_write_of_escaped_text_fits() {
    let files = files("escaped");
    let apply = apply(&files, &["work"]);
    let sandboxed = link_file_tool::Sandboxed::new(&apply);
    // Escaped, past twice the most a file may be: what the old cap took.
    let text = "\u{1}".repeat(3 << 19);
    let wrote = sandboxed
        .call(
            "write",
            &json!({"file_path": "work/ctl.txt", "content": text}),
        )
        .unwrap();
    assert_eq!(wrote["bytes"], 3 << 19, "{wrote}");
    let too_large = "a".repeat((4 << 20) + 1);
    let why = sandboxed
        .call(
            "write",
            &json!({"file_path": "work/big.txt", "content": too_large}),
        )
        .unwrap_err()
        .to_string();
    assert!(
        why.contains("at most 4 MiB") || why.contains("past the"),
        "{why}"
    );
}

/// An answer past the most a message may be stops its sandbox, never
/// leaves the call blocked on a pipe, and says why.
#[test]
fn an_answer_past_its_most_stops_the_sandbox() {
    // Reads its call whole, then answers without end.
    let apply: Vec<String> = ["/bin/sh", "-c", "read -r call; exec tr '\\0' x < /dev/zero"]
        .map(str::to_string)
        .to_vec();
    let sandboxed = link_file_tool::Sandboxed::new(&apply);
    let why = sandboxed
        .call("read", &json!({"file_path": "work/a.md"}))
        .unwrap_err()
        .to_string();
    assert!(why.contains("past the"), "{why}");
}

#[test]
fn it_reads_writes_edits_and_finds() {
    let files = files("reads");
    let apply = apply(&files, &["work", "instructions"]);
    let listed = answer(&apply, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["edit", "glob", "grep", "read", "write"]);

    let wrote = call(
        &apply,
        "write",
        json!({"file_path": "work/plan/garden.md", "content": "tomatoes\nbasil\n"}),
    );
    assert_eq!(wrote["structuredContent"]["created"], true, "{wrote}");
    assert!(wrote["structuredContent"].get("diff").is_none(), "{wrote}");
    assert_eq!(
        answered(&wrote)["diff"],
        "--- /dev/null\n+++ b/work/plan/garden.md\n@@ -0,0 +1,2 @@\n+tomatoes\n+basil\n"
    );
    assert_eq!(
        std::fs::read_to_string(files.join("work/plan/garden.md")).unwrap(),
        "tomatoes\nbasil\n"
    );
    let read = call(&apply, "read", json!({"file_path": "work/plan/garden.md"}));
    assert_eq!(said(&read), numbered("tomatoes\nbasil\n"));
    // What it read is the fenced text's alone: the structured content
    // keeps the receipt, which names it by its digest.
    let structured = &read["structuredContent"];
    assert!(structured.get("text").is_none(), "{read}");
    let receipt: link_files::reading::ReadReceipt =
        serde_json::from_value(structured["read"].clone()).unwrap();
    receipt.validate(&said(&read)).unwrap();
    let first = call(
        &apply,
        "read",
        json!({"file_path": "work/plan/garden.md", "limit": 1}),
    );
    let text = &said(&first);
    assert!(
        text.ends_with("lines 1 to 1 of 2; 'offset' 2 reads on]\n     1→tomatoes\n"),
        "{text}"
    );
    let part = call(
        &apply,
        "read",
        json!({"file_path": "work/plan/garden.md", "offset": 2}),
    );
    let text = &said(&part);
    assert!(
        text.ends_with("lines 2 to 2 of 2]\n     2→basil\n"),
        "{text}"
    );

    let replaced = call(
        &apply,
        "edit",
        json!({"file_path": "work/plan/garden.md", "old_string": "basil", "new_string": "mint"}),
    );
    assert_eq!(replaced["structuredContent"]["replaced"], 1, "{replaced}");
    assert_eq!(
        answered(&replaced)["diff"],
        "--- a/work/plan/garden.md\n+++ b/work/plan/garden.md\n\
         @@ -1,2 +1,2 @@\n tomatoes\n-basil\n+mint\n"
    );
    let twice = call(
        &apply,
        "write",
        json!({"file_path": "work/twice.txt", "content": "a a"}),
    );
    assert_eq!(twice["structuredContent"]["bytes"], 3);
    let again = call(
        &apply,
        "write",
        json!({"file_path": "work/twice.txt", "content": "a a\n"}),
    );
    assert_eq!(again["structuredContent"]["created"], false, "{again}");
    assert_eq!(
        answered(&again)["diff"],
        "--- a/work/twice.txt\n+++ b/work/twice.txt\n@@ -1 +1 @@\n-a a\n\\ No newline at end of file\n+a a\n"
    );
    let ambiguous = call(
        &apply,
        "edit",
        json!({"file_path": "work/twice.txt", "old_string": "a", "new_string": "b"}),
    );
    assert!(refused(&ambiguous).contains("2 times"));
    let all = call(
        &apply,
        "edit",
        json!({"file_path": "work/twice.txt", "old_string": "a", "new_string": "b", "replace_all": true}),
    );
    assert_eq!(all["structuredContent"]["replaced"], 2);
    let missing = call(
        &apply,
        "edit",
        json!({"file_path": "work/twice.txt", "old_string": "zebra", "new_string": "b"}),
    );
    assert!(refused(&missing).contains("doesn't hold"));

    let by_name = call(&apply, "glob", json!({"pattern": "GARDEN.*"}));
    assert_eq!(finds(&by_name), json!([{"path": "work/plan/garden.md"}]));
    // Left out, the folder is Home: no conversation is in it.
    let by_text = call(&apply, "grep", json!({"pattern": "said"}));
    assert_eq!(finds(&by_text), json!([]));
    let own = call(
        &apply,
        "grep",
        json!({"pattern": "said", "path": "chats/telegram-1", "output_mode": "content"}),
    );
    assert_eq!(said(&own), "chats/telegram-1/conversation.md:1:said");
    // What a file says is the fenced text's alone: the structured content
    // gives where, never what.
    assert_eq!(
        finds(&own),
        json!([{"path": "chats/telegram-1/conversation.md", "line": 1}])
    );
    // Left out, `output_mode` gives the files that hold a match.
    let files_only = call(
        &apply,
        "grep",
        json!({"pattern": "said", "path": "chats/telegram-1"}),
    );
    assert_eq!(said(&files_only), "chats/telegram-1/conversation.md");
    let nothing = call(&apply, "glob", json!({}));
    let why = refused(&nothing);
    assert!(why.contains("pattern"), "{why}");
}

/// Each conversation reaches only its own folder in `chats/`: never
/// another's, a stranger's none, and a call whose harness names no
/// conversation none.
#[test]
fn a_conversation_reaches_only_its_own_chat() {
    let files = files("chats");
    let apply = apply(&files, &["work"]);
    let read = call(
        &apply,
        "read",
        json!({"file_path": "chats/telegram-1/conversation.md"}),
    );
    assert!(said(&read).contains("said"), "{read}");
    let owner = json!({"link.local/thread": "telegram:1", "link.local/standing": "owner"});
    for (arguments, name) in [
        (
            json!({"file_path": "chats/telegram-2/conversation.md"}),
            "read",
        ),
        (
            json!({"file_path": "./chats//telegram-2/conversation.md"}),
            "read",
        ),
        (json!({"pattern": "theirs", "path": "chats"}), "grep"),
        (
            json!({"pattern": "theirs", "path": "chats/telegram-2"}),
            "grep",
        ),
        (
            json!({"pattern": "*.md", "path": "chats/telegram-2"}),
            "glob",
        ),
    ] {
        let why = refused(&call_as(&apply, name, arguments.clone(), owner.clone()));
        assert!(why.contains("only its own folder"), "{arguments}: {why}");
    }
    let stranger =
        json!({"link.local/thread": "webhook:deploys", "link.local/standing": "stranger"});
    let unnamed = json!({});
    for meta in [stranger, unnamed] {
        let why = refused(&call_as(
            &apply,
            "read",
            json!({"file_path": "chats/telegram-1/conversation.md"}),
            meta.clone(),
        ));
        assert!(why.contains("stranger"), "{meta}: {why}");
    }
    // A member's conversation reaches its own.
    let member = json!({"link.local/thread": "telegram:2", "link.local/standing": "member"});
    let theirs = call_as(
        &apply,
        "read",
        json!({"file_path": "chats/telegram-2/conversation.md"}),
        member,
    );
    assert!(said(&theirs).contains("theirs"), "{theirs}");
}

/// An image or a PDF is given whole, as MCP content a model sees; any
/// other file that isn't text is refused.
#[test]
fn it_reads_an_image_or_a_pdf_whole() {
    let dir = files("media");
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    std::fs::write(dir.join("work/dot.png"), &png).unwrap();
    std::fs::write(dir.join("work/plan.pdf"), b"%PDF-1.4\n").unwrap();
    std::fs::write(dir.join("work/blob.bin"), b"\xff\xfe\0").unwrap();
    let apply = apply(&dir, &[]);

    let image = call(&apply, "read", json!({"file_path": "work/dot.png"}));
    assert_eq!(said(&image), "work/dot.png: image/png, 16 bytes");
    assert_eq!(image["content"][1]["type"], "image");
    assert_eq!(image["content"][1]["mimeType"], "image/png");
    assert_eq!(image["content"][1]["data"], "iVBORw0KGgoAAAANSUhEUg==");
    assert_eq!(
        image["structuredContent"],
        json!({"path": "work/dot.png",
        "media": "image/png", "bytes": 16})
    );

    let pdf = call(&apply, "read", json!({"file_path": "work/plan.pdf"}));
    let resource = &pdf["content"][1]["resource"];
    assert_eq!(resource["uri"], "file:///work/plan.pdf");
    assert_eq!(resource["mimeType"], "application/pdf");
    assert_eq!(resource["blob"], "JVBERi0xLjQK");

    let other = call(&apply, "read", json!({"file_path": "work/blob.bin"}));
    assert!(refused(&other).contains("isn't text (UTF-8)"));
}

#[test]
fn it_writes_only_where_it_may_and_never_through_a_link() {
    let files = files("writes");
    let apply = apply(&files, &["work"]);
    let mut session = super::session::Session::new(&files.scratch("user"), &apply);
    for (name, arguments, says) in [
        (
            "write",
            json!({"file_path": "chats/telegram-1/conversation.md", "content": "forged"}),
            "you may not write chats/",
        ),
        (
            "write",
            json!({"file_path": "instructions/instruction.md", "content": "x"}),
            "you may write only in work/",
        ),
        (
            "write",
            json!({"file_path": "work", "content": "x"}),
            "you may write only",
        ),
        (
            "write",
            json!({"file_path": "../escaped.txt", "content": "x"}),
            "isn't a path in your files folder",
        ),
        (
            "write",
            json!({"file_path": "/etc/escaped.txt", "content": "x"}),
            "isn't a path in your files folder",
        ),
        (
            "edit",
            json!({"file_path": "chats/telegram-1/conversation.md", "old_string": "said", "new_string": "x"}),
            "you may not write chats/",
        ),
        ("read", json!({"file_path": "work/none.md"}), "no file"),
        ("read", json!({"file_path": "work/out/passwd"}), "is a link"),
        (
            "write",
            json!({"file_path": "work/out/x", "content": "x"}),
            "is a link",
        ),
    ] {
        if name == "read" && says == "is a link" {
            std::os::unix::fs::symlink("/etc", files.join("work/out")).unwrap();
        }
        let result = session.call(name, arguments.clone());
        let text = refused(&result);
        assert!(text.contains(says), "{name} {arguments}: {text}");
    }
    assert_eq!(
        std::fs::read_to_string(files.join("chats/telegram-1/conversation.md")).unwrap(),
        "said\n"
    );
    session.finish();
}

/// `.`: the whole files folder, a repository an agent works in, but its
/// conversations, its harness's.
#[test]
fn the_whole_folder_is_written_but_its_conversations() {
    let files = files("whole");
    let apply = apply(&files, &["."]);
    for path in ["README.md", "src/main.rs", "work/notes.md"] {
        let result = call(&apply, "write", json!({"file_path": path, "content": "x"}));
        assert_ne!(result["isError"], true, "{path}: {result}");
        assert_eq!(std::fs::read_to_string(files.join(path)).unwrap(), "x");
    }
    for (arguments, says) in [
        (
            json!({"file_path": "chats/telegram-1/conversation.md", "content": "forged"}),
            "you may not write chats/",
        ),
        (
            json!({"file_path": "../escaped.txt", "content": "x"}),
            "isn't a path in your files folder",
        ),
    ] {
        let text = refused(&call(&apply, "write", arguments.clone()));
        assert!(text.contains(says), "{arguments}: {text}");
    }
    assert_eq!(
        std::fs::read_to_string(files.join("chats/telegram-1/conversation.md")).unwrap(),
        "said\n"
    );
}

/// A session's calls are applied by one `apply`, started at the first and
/// kept for the next; one that ends is started again at the next call.
#[test]
fn a_sessions_calls_share_one_apply() {
    let files = files("session");
    let started = files.scratch("started");
    // `apply`, saying its process when it starts.
    let mut argv: Vec<String> = vec![
        "/bin/sh".into(),
        "-c".into(),
        r#"echo $$ >> "$0"; exec "$@""#.into(),
        started.display().to_string(),
    ];
    argv.extend(apply(&files, &["work"]));
    let starts = || {
        std::fs::read_to_string(&started)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let sandboxed = link_file_tool::Sandboxed::new(&argv);
    for n in 0..3 {
        let path = format!("work/{n}.md");
        sandboxed
            .call("write", &json!({"file_path": path, "content": "a"}))
            .unwrap();
        let read = sandboxed.call("read", &json!({"file_path": path})).unwrap();
        assert_eq!(read["text"], numbered("a"), "{read}");
    }
    let refused = sandboxed.call("read", &json!({"file_path": "work/missing.md"}));
    assert!(refused.is_err());
    assert_eq!(starts().len(), 1, "{:?}", starts());

    // Ended between calls, it's started again.
    // The one process, never a group: never through the `kill` program.
    let pid: libc::pid_t = starts()[0].parse().unwrap();
    assert!(pid > 1);
    // SAFETY: a plain kill of the process this test started.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let read = sandboxed.call("read", &json!({"file_path": "work/0.md"}));
    let read = match read {
        Ok(read) => read,
        // The call that found it ended says so; the next starts it again.
        Err(err) => {
            assert!(format!("{err:#}").contains("didn't run"), "{err:#}");
            sandboxed
                .call("read", &json!({"file_path": "work/0.md"}))
                .unwrap()
        }
    };
    assert_eq!(read["text"], numbered("a"), "{read}");
    assert_eq!(starts().len(), 2, "{:?}", starts());
}

/// A program that can't start a sandbox says why, and so for each call.
#[test]
fn a_sandbox_that_fails_says_why() {
    let argv: Vec<String> = ["/bin/sh", "-c", "echo no sandbox here >&2; exit 3"]
        .map(String::from)
        .into();
    let sandboxed = link_file_tool::Sandboxed::new(&argv);
    for _ in 0..2 {
        let err = sandboxed
            .call("read", &json!({"file_path": "work/a.md"}))
            .unwrap_err();
        assert!(format!("{err:#}").contains("no sandbox here"), "{err:#}");
    }
}

/// `lnk-sandbox`, from `link-core` beside this folder, built there.
#[cfg(target_os = "linux")]
fn lnk_sandbox() -> std::path::PathBuf {
    let core = std::env::var_os("LNK_CORE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    link_test_kit::programs_in(&core.join("../modules/sandbox")).join("lnk-sandbox")
}

/// Under the sandbox Link defines for it, with `apply` told it may write
/// `conversations` too: the sandbox still refuses, as the file says.
#[cfg(target_os = "linux")]
#[test]
fn the_sandbox_holds_whatever_apply_is_told() {
    let program = lnk_sandbox();
    // Probed in a home of its own, never the shared temporary folder's.
    let probe = link_test_kit::TempDir::new("sandbox-probe");
    if !link_test_kit::gate::sandbox(&program, &probe) {
        return;
    }
    let files = files("sandbox");
    let home = files.scratch("user");
    std::fs::create_dir_all(&home).unwrap();
    let file = file();
    let mut argv = vec![
        "/usr/bin/env".into(),
        format!("HOME={}", home.display()),
        program.display().to_string(),
        "run".into(),
        "--files.read".into(),
        files.home().display().to_string(),
        "--files.read".into(),
        files.own().display().to_string(),
        "--files.write".into(),
        files.join("work").display().to_string(),
        "--files.read".into(),
        file.clone(),
        "--commands.allow".into(),
        file.clone(),
        "--".into(),
    ];
    argv.extend(apply(&files, &["work", "chats"]));
    let wrote = call(
        &argv,
        "write",
        json!({"file_path": "work/a.md", "content": "a"}),
    );
    assert_eq!(wrote["structuredContent"]["bytes"], 1, "{wrote}");
    let read = call(&argv, "read", json!({"file_path": "work/a.md"}));
    assert_eq!(said(&read), numbered("a"));
    let forged = call(
        &argv,
        "write",
        json!({"file_path": "chats/telegram-1/conversation.md", "content": "forged"}),
    );
    refused(&forged);
    // A session's calls, in the one sandbox it starts.
    let messages: String = [
        json!({"file_path": "work/b.md", "content": "b"}),
        json!({"file_path": "chats/telegram-1/conversation.md", "content": "forged"}),
    ]
    .iter()
    .enumerate()
    .map(|(id, arguments)| {
        let message = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "write", "arguments": arguments}});
        format!("{message}\n")
    })
    .collect();
    let mut replies = Vec::new();
    let offer = link_file_tool::Offer::default();
    link_file_tool::serve(&argv, &offer, messages.as_bytes(), &mut replies).unwrap();
    let replies: Vec<Value> = String::from_utf8(replies)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(
        replies[0]["result"]["structuredContent"]["bytes"], 1,
        "{replies:?}"
    );
    refused(&replies[1]["result"]);
    assert_eq!(
        std::fs::read_to_string(files.join("work/b.md")).unwrap(),
        "b"
    );
    assert_eq!(
        std::fs::read_to_string(files.join("chats/telegram-1/conversation.md")).unwrap(),
        "said\n"
    );
}

/// A team's shared folder, at `teams/<team>/`: what the agent's granted
/// of it read and written, the rest of it refused, and another team's
/// not there at all.
#[test]
fn a_teams_folder_is_reached_as_granted() {
    let files = files("teams");
    let shared = TempDir::new("calls-shared");
    for folder in ["drafts", "sources", "private"] {
        std::fs::create_dir_all(shared.join(folder)).unwrap();
    }
    std::fs::write(shared.join("sources/paper.md"), "a claim\n").unwrap();
    std::fs::write(shared.join("private/notes.md"), "keep out\n").unwrap();
    let mut apply = apply(&files, &["work"]);
    apply.extend([
        "--team-folder".into(),
        format!("research={}", shared.display()),
        "--team-write".into(),
        "research/drafts".into(),
        "--team-read".into(),
        "research/sources".into(),
    ]);
    let wrote = call(
        &apply,
        "write",
        json!({"file_path": "teams/research/drafts/report.md", "content": "A report.\n"}),
    );
    assert_eq!(wrote["isError"], Value::Null, "{wrote}");
    assert_eq!(
        std::fs::read_to_string(shared.join("drafts/report.md")).unwrap(),
        "A report.\n"
    );
    let read = call(
        &apply,
        "read",
        json!({"file_path": "teams/research/sources/paper.md"}),
    );
    assert_eq!(said(&read), numbered("a claim\n"), "{read}");
    let found = call(
        &apply,
        "grep",
        json!({"pattern": "claim", "path": "teams/research/sources"}),
    );
    assert_eq!(
        finds(&found)[0]["path"],
        "teams/research/sources/paper.md",
        "{found}"
    );
    for (name, arguments) in [
        (
            "write",
            json!({"file_path": "teams/research/sources/x.md", "content": "x"}),
        ),
        (
            "read",
            json!({"file_path": "teams/research/private/notes.md"}),
        ),
        (
            "write",
            json!({"file_path": "teams/research/drafts", "content": "x"}),
        ),
        (
            "write",
            json!({"file_path": "teams/research/top.md", "content": "x"}),
        ),
        ("read", json!({"file_path": "teams/other/drafts/x.md"})),
    ] {
        let why = refused(&call(&apply, name, arguments.clone()));
        assert!(
            why.contains("team research") || why.contains("no file"),
            "{arguments}: {why}"
        );
    }
    // The whole folder, written: anything in it but the folder itself.
    let mut whole = self::apply(&files, &[]);
    whole.extend([
        "--team-folder".into(),
        format!("research={}", shared.display()),
        "--team-write".into(),
        "research/.".into(),
    ]);
    let wrote = call(
        &whole,
        "write",
        json!({"file_path": "teams/research/top.md", "content": "x"}),
    );
    assert_eq!(wrote["isError"], Value::Null, "{wrote}");
}

/// `glob`'s `pattern` (and `grep`'s `glob`) is a glob of a file's name,
/// or of its path below the folder searched when it holds `/`, any case;
/// `grep`'s `pattern` a regular expression, refused, saying so, when it
/// isn't one.
#[test]
fn it_finds_by_a_glob_and_a_regular_expression() {
    let files = files("globs");
    for (path, text) in [
        ("work/src/main.rs", "fn main() {\n    run(1);\n}\n"),
        ("work/src/lib/Test_Lib.PY", "def test_lib(): pass\n"),
        ("work/src/test_top.py", "def test_top(): pass\n"),
        ("work/tests/test_other.py", "x = 1\n"),
    ] {
        std::fs::create_dir_all(files.join(path).parent().unwrap()).unwrap();
        std::fs::write(files.join(path), text).unwrap();
    }
    let apply = apply(&files, &["work"]);
    let paths = |name: &str, arguments: Value| -> Vec<String> {
        let found = call(&apply, name, arguments);
        finds(&found)
            .as_array()
            .unwrap_or_else(|| panic!("{found}"))
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        paths("glob", json!({"pattern": "*.rs"})),
        ["work/src/main.rs"]
    );
    assert_eq!(
        paths("glob", json!({"pattern": "test_*.py", "path": "work"})),
        [
            "work/src/test_top.py",
            "work/src/lib/Test_Lib.PY",
            "work/tests/test_other.py"
        ]
    );
    // By its path below the folder searched, `**/` any folders, or none.
    assert_eq!(
        paths(
            "glob",
            json!({"pattern": "src/**/test_*.py", "path": "work"})
        ),
        ["work/src/test_top.py", "work/src/lib/Test_Lib.PY"]
    );
    assert_eq!(
        paths("glob", json!({"pattern": "src/*.py", "path": "work"})),
        ["work/src/test_top.py"]
    );
    let lines = call(
        &apply,
        "grep",
        json!({"glob": "*.rs", "pattern": r"^\s+run\(\d\);$", "output_mode": "content"}),
    );
    assert_eq!(
        finds(&lines),
        json!([{"path": "work/src/main.rs", "line": 2}])
    );
    assert_eq!(said(&lines), "work/src/main.rs:2:    run(1);");
    assert_eq!(
        paths("grep", json!({"pattern": "def test_(lib|top)"})).len(),
        2,
        "a regular expression"
    );
    let why = refused(&call(&apply, "grep", json!({"pattern": "run("})));
    assert!(why.contains("isn't a regular expression"), "{why}");
    assert!(why.contains("escape"), "{why}");
}

/// A change past the most a diff answers with, or of a file that wasn't
/// text, answers `diff_cut` and the new text's SHA-256 in its place.
#[test]
fn a_large_change_answers_its_sha256() {
    let files = files("large");
    let apply = apply(&files, &["work"]);
    let long: String = (1..=4000).map(|n| format!("line {n}\n")).collect();
    let wrote = call(
        &apply,
        "write",
        json!({"file_path": "work/long.md", "content": long}),
    );
    let answer = &wrote["structuredContent"];
    assert_eq!(answer["diff_cut"], true, "{wrote}");
    assert_eq!(answer["diff"], Value::Null, "{wrote}");
    assert_eq!(answer["sha256"], link_digest::sha256(long.as_bytes()));
    assert!(wrote.to_string().len() < 1 << 10, "{wrote}");
    let replaced = call(
        &apply,
        "edit",
        json!({"file_path": "work/long.md", "old_string": "line", "new_string": "row", "replace_all": true}),
    );
    let after = long.replace("line", "row");
    assert_eq!(
        replaced["structuredContent"]["diff_cut"], true,
        "{replaced}"
    );
    assert_eq!(
        replaced["structuredContent"]["sha256"],
        link_digest::sha256(after.as_bytes())
    );
    // One line of it changed: a diff again.
    let one = call(
        &apply,
        "edit",
        json!({"file_path": "work/long.md", "old_string": "row 7\n", "new_string": "row seven\n"}),
    );
    let diff = answered(&one)["diff"].as_str().unwrap().to_string();
    assert!(diff.contains("-row 7\n+row seven\n"), "{one}");
    std::fs::write(files.join("work/bytes.bin"), [0xff, 0xfe]).unwrap();
    let over = call(
        &apply,
        "write",
        json!({"file_path": "work/bytes.bin", "content": "x"}),
    );
    assert_eq!(over["structuredContent"]["diff_cut"], true, "{over}");
    assert_eq!(over["structuredContent"]["created"], false, "{over}");
}

/// `edit` with `replace_all` that would make a file over the most written
/// is refused before it's built; `grep` passes over a pipe, never waiting on
/// it, and `read` refuses one.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_huge_replace_and_a_pipe_are_refused() {
    let files = files("huge");
    let apply = apply(&files, &["work"]);
    std::fs::write(files.join("work/many.txt"), "a".repeat(100_000)).unwrap();
    let huge = call(
        &apply,
        "edit",
        json!({"file_path": "work/many.txt", "old_string": "a", "new_string": "b".repeat(100), "replace_all": true}),
    );
    assert!(refused(&huge).contains("would be over"), "{huge}");
    let pipe = files.join("work/pipe");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    if made.is_ok_and(|s| s.success()) {
        let by_text = call(&apply, "grep", json!({"pattern": "nothing here"}));
        assert_eq!(finds(&by_text), json!([]), "{by_text}");
        let read = call(&apply, "read", json!({"file_path": "work/pipe"}));
        assert!(refused(&read).contains("isn't a file"), "{read}");
    }
}
