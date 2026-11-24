//! The tool server's own tests: its calls, its folders, its variables
//! and its answers.

use super::*;
use std::path::Path;

/// A run of `commands`, its background commands kept where
/// `background` says.
fn declaring(commands: &[&str], background: bool) -> Run {
    Run {
        context: None,
        files: "/tmp".into(),
        path: "/usr/bin".into(),
        commands: commands.iter().map(|c| c.to_string()).collect(),
        sandbox: Vec::new(),
        env: Vec::new(),
        timeout: Duration::from_secs(108),
        output: 4096,
        background: background.then(|| Background {
            program: "lnk-run-tool".into(),
            state: "/srv/state".into(),
            way_in: "/srv/way-in".into(),
            caller: background::Caller {
                agent: "main".into(),
                keyring: None,
            },
            at_once: 2,
            timeout: Duration::from_secs(7200),
            output: 1 << 20,
            keep: Duration::from_secs(3600),
            per_day: 100,
            hops: 4,
        }),
        conversations: None,
    }
}

/// The declared calls as serve offers them: a command only where the
/// shell is among its programs, which its description names, and the
/// background calls, and its `run_in_background`, only where it keeps
/// them.
#[test]
fn the_declared_tools_are_filled_as_serve_says() {
    let names = |tools: &Value| -> Vec<String> {
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    };
    let properties = |tool: &Value| -> Vec<String> {
        let mut names: Vec<String> = tool["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    };
    for shell_less in [false, true] {
        let none = offered(&declaring(&["git", "cargo"], shell_less)).unwrap();
        assert_eq!(names(&none), Vec::<String>::new());
    }
    let some = offered(&declaring(&["sh", "git"], false)).unwrap();
    assert_eq!(names(&some), ["command"]);
    assert_eq!(properties(&some[0]), ["command", "description", "timeout"]);
    assert_eq!(some[0]["inputSchema"]["required"], json!(["command"]));
    let said = some[0]["description"].as_str().unwrap();
    assert!(said.contains("sh, git") && said.contains("108s"), "{said}");
    let kept = offered(&declaring(&["/bin/sh"], true)).unwrap();
    assert_eq!(names(&kept), ["command", "list", "read", "stop"]);
    assert_eq!(
        properties(&kept[0]),
        ["command", "description", "run_in_background", "timeout"]
    );
    let background = &kept[0]["inputSchema"]["properties"]["run_in_background"];
    assert!(
        background["description"]
            .as_str()
            .unwrap()
            .contains("at most 2h"),
        "{background}"
    );
    assert_eq!(
        kept[0]["_meta"]["link.local/summary"],
        json!(["command", "description"])
    );
}

#[test]
fn a_stream_keeps_its_start_and_its_end() {
    let mut kept = Kept::new(8);
    kept.push(b"abc");
    assert_eq!(kept.text(), "abc");
    kept.push(b"defghijklmnop");
    assert_eq!(kept.total, 16);
    assert_eq!(kept.text(), "abcd\n[... 8 bytes cut ...]\nmnop");
    kept.push(b"q");
    assert_eq!(kept.text(), "abcd\n[... 9 bytes cut ...]\nnopq");
}

/// A run of `commands` in `files` without a sandbox, keeping
/// `output` bytes of each stream.
fn running(files: &Path, commands: &[&str], output: usize) -> Run {
    Run {
        files: files.to_path_buf(),
        path: "/usr/bin:/bin".into(),
        output,
        ..declaring(commands, false)
    }
}

/// `*` alone lets a command start any program on its PATH, its
/// description says so, and the shell runs it; programs without the shell
/// offer no command, and a call is refused naming it.
#[test]
fn a_command_runs_in_the_shell_it_may_start() {
    let any = offered(&declaring(&["*"], false)).unwrap();
    let said = any[0]["description"].as_str().unwrap();
    assert!(said.contains(ANY_PROGRAM), "{said}");
    let files = link_test_kit::TempDir::new("run-any");
    let run = running(&files, &["*"], 4096);
    let prepared = prepare(
        &run,
        &json!({"command": "echo hi"}),
        &Value::Null,
        run.timeout,
    )
    .unwrap();
    let sh = link_harness_contract::harness::find_program("sh", &run.path).unwrap();
    assert_eq!(prepared.argv, [&sh.display().to_string(), "-c", "echo hi"]);
    assert_eq!(prepared.command, "echo hi");
    let named = running(&files, &["ls"], 4096);
    let refused = prepare(
        &named,
        &json!({"command": "ls"}),
        &Value::Null,
        named.timeout,
    )
    .err()
    .unwrap();
    assert!(
        refused.to_string().contains("needs the shell (sh)"),
        "{refused}"
    );
    let missing = Run {
        commands: vec!["/no/such/sh".into()],
        ..running(&files, &[], 4096)
    };
    let refused = prepare(
        &missing,
        &json!({"command": ":"}),
        &Value::Null,
        missing.timeout,
    )
    .err()
    .unwrap();
    assert!(
        refused.to_string().contains("isn't on this machine"),
        "{refused}"
    );
}

/// A call's timeout, in milliseconds, is how long its command runs, and
/// one past the most, or none, is the most.
#[test]
fn a_calls_timeout_is_held_to_the_most() {
    let files = link_test_kit::TempDir::new("run-timeout");
    let run = running(&files, &["sh"], 4096);
    let timeout = |arguments: Value| {
        prepare(&run, &arguments, &Value::Null, run.timeout)
            .unwrap()
            .timeout
    };
    assert_eq!(timeout(json!({"command": ":"})), run.timeout);
    assert_eq!(
        timeout(json!({"command": ":", "timeout": 1500})),
        Duration::from_millis(1500)
    );
    assert_eq!(
        timeout(json!({"command": ":", "timeout": 600000})),
        run.timeout
    );
    let ran = command(
        &run,
        &json!({"command": "sleep 3105", "timeout": 100}),
        &Value::Null,
    )
    .unwrap();
    assert!(ran.stopped, "not stopped at its own timeout");
    // Stopped at its own 100 ms, by the time it says it took, not the most.
    assert!(ran.took < run.timeout, "took {:?}", ran.took);
}

/// Without background commands, a call asking for one is refused before
/// anything runs, naming how to run it.
#[test]
fn a_background_call_without_background_commands_is_refused() {
    let files = link_test_kit::TempDir::new("run-no-background");
    let run = running(&files, &["sh"], 4096);
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "command", "arguments":
            {"command": ": > ran", "run_in_background": true}}});
    let answered = answer(&run, &message.to_string()).unwrap();
    assert_eq!(answered["result"]["isError"], true, "{answered}");
    let text = answered["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("without run_in_background"), "{text}");
    assert!(!files.join("ran").exists());
}

/// A command starts in the files folder opened, by its descriptor: one
/// swapped for a link between the check and the start is never
/// followed.
#[test]
fn a_command_starts_in_the_folder_checked_not_one_swapped_in() {
    let place = link_test_kit::TempDir::new("run-swapped");
    let outside = link_test_kit::TempDir::new("run-outside");
    std::fs::create_dir_all(place.join("files")).unwrap();
    let run = running(&place.join("files"), &["sh"], 4096);
    let prepared = prepare(
        &run,
        &json!({"command": "pwd -P"}),
        &Value::Null,
        run.timeout,
    )
    .unwrap();
    std::fs::rename(place.join("files"), place.join("moved")).unwrap();
    std::os::unix::fs::symlink(outside.path(), place.join("files")).unwrap();
    let ran = spawn(&run, prepared, None).unwrap();
    let moved = std::fs::canonicalize(place.join("moved")).unwrap();
    assert_eq!(ran.stdout.text().trim_end(), moved.display().to_string());
}

/// The variables a command gets reach the sandbox on its descriptor,
/// which checks them as a caller's, never on its command line through
/// a program of their own (`env`); one the sandbox refuses is refused
/// here first.
#[test]
fn variables_reach_the_sandbox_checked_not_through_env() {
    let files = link_test_kit::TempDir::new("run-variables");
    let run = Run {
        sandbox: vec!["sandbox".into(), "run".into()],
        env: vec![("DISPLAY".into(), ":1".into())],
        ..running(&files, &["sh"], 4096)
    };
    let prepared = prepare(&run, &json!({"command": ":"}), &Value::Null, run.timeout).unwrap();
    let sh = link_harness_contract::harness::find_program("sh", &run.path).unwrap();
    let sh = sh.display().to_string();
    assert_eq!(
        prepared.argv,
        ["sandbox", "run", "--with-fd", "4", "--", &sh, "-c", ":"]
    );
    let with = prepared.with.unwrap();
    assert_eq!(with.env.get("DISPLAY").map(String::as_str), Some(":1"));
    for refused in ["LD_PRELOAD=evil.so", "PATH=/tmp", "HTTPS_PROXY=x", "A=b\0c"] {
        assert!(variable(refused).is_err(), "{refused}");
    }
}

/// A call's sandbox, which hides every conversation's folder, reads its
/// own conversation's, as its harness names it, when its message is the
/// owner's or a member's; a stranger's call, one with no standing, and
/// one naming no conversation read none.
#[test]
fn a_call_reads_only_its_own_conversation() {
    let files = link_test_kit::TempDir::new("run-conversations");
    let chats = files.join("chats");
    let own = link_harness_contract::store::folder::folder_of("telegram:42");
    std::fs::create_dir_all(chats.join(&own)).unwrap();
    let run = Run {
        sandbox: vec!["sandbox".into(), "run".into()],
        conversations: Some(chats.clone()),
        ..running(&files, &["sh"], 4096)
    };
    let read = |meta: Value| {
        let argv = prepare(&run, &json!({"command": ":"}), &meta, run.timeout)
            .unwrap()
            .argv;
        argv.windows(2)
            .find(|pair| pair[0] == "--files.read")
            .map(|pair| pair[1].clone())
    };
    let thread = link_harness_contract::connection::access::THREAD_META;
    let standing = link_harness_contract::connection::access::STANDING_META;
    let expected = Some(chats.join(&own).display().to_string());
    assert_eq!(
        read(json!({thread: "telegram:42", standing: "owner"})),
        expected
    );
    assert_eq!(
        read(json!({thread: "telegram:42", standing: "member"})),
        expected
    );
    assert_eq!(
        read(json!({thread: "telegram:42", standing: "stranger"})),
        None
    );
    assert_eq!(read(json!({thread: "telegram:42"})), None);
    assert_eq!(read(json!({standing: "owner"})), None);
    assert_eq!(read(json!({thread: "telegram:7", standing: "owner"})), None);
}

/// At most so many calls run at once on a session, and one with a
/// running call's id is refused; one ended makes room.
#[test]
fn calls_at_once_are_capped() {
    let calls = Calls::default();
    let most = link_run_tool_contract::limits().calls_at_once;
    let entered: Vec<_> = (0..most).map(|n| calls.enter(&json!(n)).unwrap()).collect();
    assert!(calls.enter(&json!(most)).is_none());
    drop(entered);
    let one = calls.enter(&json!(1)).unwrap();
    assert!(calls.enter(&json!(1)).is_none());
    drop(one);
}

/// A cancelled call's command is killed with its group, its answer
/// coming at once rather than at its timeout.
#[test]
fn a_cancelled_call_is_killed() {
    let files = link_test_kit::TempDir::new("run-cancelled");
    let run = running(&files, &["sh"], 4096);
    let calls = Calls::default();
    std::thread::scope(|scope| {
        let call = scope.spawn(|| {
            let message = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": {"name": "command", "arguments":
                    {"command": "sleep 3105 & sleep 3105"}}});
            answer_in(&run, &calls, &message.to_string()).unwrap()
        });
        link_test_kit::eventually("the call's group started", Duration::from_secs(10), || {
            calls
                .running()
                .get(&json!(7).to_string())
                .is_some_and(|call| call.group.is_some())
        });
        let cancel = json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": {"requestId": 7}});
        assert!(answer_in(&run, &calls, &cancel.to_string()).is_none());
        let answered = call.join().unwrap();
        let value = &answered["result"]["structuredContent"];
        assert_eq!(value["stopped"], false, "{answered}");
        assert_eq!(value["exit"], Value::Null, "{answered}");
    });
}

/// `exited` leaves a child that ended unreaped, so its ID, its
/// group's, is never another's before its group is killed.
#[test]
fn an_exited_child_is_left_to_be_reaped() {
    let mut child = link_secretive::process::command("true").spawn().unwrap();
    let pid = child.id() as i32;
    link_test_kit::eventually("the child to exit", Duration::from_secs(10), || exited(pid));
    // SAFETY: signal 0 to our own child, unreaped: it's only looked for.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "reaped by exited");
    assert!(child.wait().unwrap().success());
}

/// Both streams, past what's kept, and the room around them fit the
/// answer cap the output was clamped to: the errors' end survives
/// the harness's cut, and both fit the tools bridge's message cap.
#[test]
fn an_answer_kept_to_its_clamp_fits_with_the_errors_end() {
    // The longest name a fence shows, in characters of four bytes.
    let name = "\u{1F600}".repeat(400);
    for answer in [4096, 32 << 10, 1 << 20, 16 << 20] {
        let most = link_run_tool_contract::output_within(16 << 20, answer) as usize;
        let mut stdout = Kept::new(most);
        let mut stderr = Kept::new(most);
        stdout.push(&vec![b'"'; most * 3]);
        // Text below the answer cap; at the message cap, invalid bytes,
        // each shown as three (U+FFFD).
        let byte = if answer < 1 << 20 { b'e' } else { 0xff };
        stderr.push(&vec![byte; most * 3]);
        stderr.push(b"the end");
        let ran = Ran {
            exit: None,
            stopped: true,
            stdout,
            stderr,
            took: Duration::from_secs(108),
        };
        let text = ran.text(&name).unwrap();
        if answer < 1 << 20 {
            assert!(text.len() as u64 <= answer, "{} > {answer}", text.len());
        }
        let fenced = text.split_once("\n\nerrors:\n").unwrap().1;
        let errors = link_untrusted::fenced(fenced).unwrap().text;
        assert!(errors.ends_with("the end"), "{answer}");
        let reply = result(
            json!(1),
            json!({"content": [{"type": "text", "text": text}],
                "structuredContent": ran.value()}),
        );
        assert!(
            reply.to_string().len() < link_sandbox_client::routes::TOOL_MESSAGE_MAX,
            "{answer}"
        );
    }
}
