//! A tool that runs outside, called from a proxied sandbox, for real, on
//! Linux: `lnk sandbox tool add` defines a stdio MCP server (a script of
//! the test's), and `lnk sandbox run --internet.hosts '*' --tools.names` calls it with curl
//! at `$LNK_TOOL_FAKE`. The call allowed by name runs outside, with the
//! HOME it was added with. The one that asks, with no desktop, waits for
//! `lnk sandbox answer` while a terminal answers here: refused once, then
//! allowed; with no terminal either, it's refused at once, saying how to
//! allow it. The tool's definition stays out of the sandbox's reach.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use link_test_kit::TempDir;

const CLIENT: &str = r#"
read -r ready || :
u=$LNK_TOOL_FAKE
post() { curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }
id=$(post -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | tr -d '\r' | grep -i '^Mcp-Session-Id: ' | cut -d' ' -f2-)
call() { post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","id":'$2',"method":"tools/call","params":{"name":"'$1'","arguments":{}}}' | tail -n 1; echo; }
echo "QUIET $(post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"delete_quietly","arguments":{}}}' | head -n 1)"
echo "ECHO $(call echo 2)"
echo "FIRST $(call delete_all 3)"
echo "SECOND $(call delete_all 4)"
cat "$LNK_SETTINGS/sandbox/tools/fake.json" && echo READ-THE-DEFINITION
"#;

/// `lnk-sandbox` with `args`, HOME `root`, no desktop and no terminal (a
/// session of its own): a question is asked only while the test answers
/// as a terminal would, with `lnk sandbox answer`.
fn lnk(root: &Path, args: &[&str]) -> Command {
    let mut command = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), root);
    command
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid only, between fork and exec.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command
}

fn ran(command: &mut Command) -> (bool, String) {
    let output = command.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), text)
}

/// Answers the next question waiting, `answer`.
fn answer_next(root: &Path, answer: &str) {
    let ask: link_interactions::delivery::DeliveryEnvelope =
        link_test_kit::eventually("the tool's owner question", Duration::from_secs(30), || {
            let (_, text) = ran(&mut lnk(root, &["question", "list", "--json"]));
            match text.lines().find(|l| l.starts_with('{')) {
                Some(line) => Ok(serde_json::from_str(line).unwrap()),
                None => Err(text),
            }
        });
    assert_eq!(
        ask.question.review.unwrap().capability,
        "run tool fake's delete_all"
    );
    let (ok, text) = ran(&mut lnk(root, &["answer", &ask.response_code, answer]));
    assert!(ok, "{text}");
}

#[test]
fn a_sandbox_calls_a_tool_outside_and_what_asks_waits_for_your_answer() {
    if !link_test_kit::gate::bubblewrap() || !link_test_kit::gate::program("curl", &["--version"]) {
        return;
    }
    let root = TempDir::new("tools");
    let server = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp-server.sh");
    let (ok, text) = ran(&mut lnk(
        &root,
        &["tool", "add", "fake", "--add", "echo", "--", "sh", server],
    ));
    assert!(ok, "{text}");
    let settings = root.join(".lnk/safe");
    assert!(
        settings.join("sandboxes/tools/fake.json").is_file(),
        "{text}"
    );

    let client = format!("LNK_SETTINGS={}\n{CLIENT}", settings.display());
    let run = |root: &Path| {
        lnk(
            root,
            &[
                "run",
                "--commands.toolchains",
                "shell",
                "--internet.hosts",
                "*",
                "--tools.names",
                "fake",
                "--",
                "sh",
                "-c",
                &client,
            ],
        )
    };
    // Nobody here to ask: refused at once, rather than at `ask_wait`.
    let (_, text) = ran(&mut run(&root));
    assert!(
        text.contains("no authorized owner answer was received: refused")
            && text.contains("lnk sandbox tool add fake --add delete_all"),
        "{text}"
    );
    // A terminal answering here, as a proxy started in one does, for the
    // run's sandbox (`run-<its pid>`), held before its call asks: what asks
    // waits for an answer, from it or any other.
    let mut running =
        link_test_kit::ChildGroup::spawn_session(run(&root).stdin(Stdio::piped())).unwrap();
    let mut stdout = running.take_stdout().unwrap();
    let mut stderr = running.take_stderr().unwrap();
    let store = link_interactions::Store::at(link_files::layout::Hidden::Approvals.in_home(&root));
    let answering = store
        .presence(&format!("sandbox:run-{}", running.id()), None)
        .unwrap();
    running.take_stdin().unwrap().write_all(b"ready\n").unwrap();
    answer_next(&root, "no");
    answer_next(&root, "yes");
    link_test_kit::eventually(
        "the answered tool calls finish",
        Duration::from_secs(30),
        || running.try_status().unwrap(),
    );
    running.stop();
    drop(answering);
    let mut text = String::new();
    stdout.read_to_string(&mut text).unwrap();
    stderr.read_to_string(&mut text).unwrap();
    let line = |tag: &str| {
        text.lines()
            .find(|l| l.starts_with(tag))
            .unwrap_or_default()
            .to_string()
    };
    let home = format!("home={}", root.display());
    assert!(line("ECHO").contains(&home), "{text}");
    assert!(
        line("FIRST").contains("The reviewed operation was not authorized.")
            && line("FIRST").contains("user_submission"),
        "{text}"
    );
    assert!(line("SECOND").contains(&home), "{text}");
    // A call sent as a notification, without an id, never reaches the
    // tool: it would run without asking.
    assert!(line("QUIET").contains("400"), "{text}");
    let calls = std::fs::read_to_string(root.join("calls")).unwrap_or_default();
    assert!(!calls.contains("delete_quietly"), "{calls}");
    assert!(calls.contains("\"echo\""), "{calls}");
    assert!(!text.contains("READ-THE-DEFINITION"), "{text}");
    // Each one answered and gone.
    let (_, waiting) = ran(&mut lnk(&root, &["question", "list"]));
    assert!(waiting.contains("No questions are waiting"), "{waiting}");
    // An answer to no question waiting does nothing, and says so failing.
    let (ok, text) = ran(&mut lnk(&root, &["answer", "abcdef", "yes"]));
    assert!(!ok && text.contains("is waiting"), "{text}");

    // Not named, not reached; named but not defined, refused at once.
    let (_, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--",
            "sh",
            "-c",
            &client,
        ],
    ));
    assert!(!text.contains("home="), "{text}");
    let (ok, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--tools.names",
            "nope",
            "--",
            "true",
        ],
    ));
    assert!(!ok && text.contains("no tool nope"), "{text}");
    // Each proxy, its program over, took its traffic file with it.
    let mut left = vec![root.to_path_buf()];
    let mut kept = Vec::new();
    while let Some(dir) = left.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            match entry.file_type().is_ok_and(|t| t.is_dir()) {
                true => left.push(path),
                false if dir.ends_with("traffic") => kept.push(path),
                false => {}
            }
        }
    }
    assert!(kept.is_empty(), "{kept:?}");
}

/// Reading a tool's resource or getting its prompt asks as its calls do:
/// with nobody to ask, refused at once, naming the `--add` that lets it
/// through; allowed by its method, it reaches the server.
#[test]
fn a_tools_resources_and_prompts_ask_as_its_calls_do() {
    if !link_test_kit::gate::bubblewrap() || !link_test_kit::gate::program("curl", &["--version"]) {
        return;
    }
    let root = TempDir::new("tool-reads");
    let server = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp-server.sh");
    let client = r#"
u=$LNK_TOOL_FAKE
post() { curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }
id=$(post -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | tr -d '\r' | grep -i '^Mcp-Session-Id: ' | cut -d' ' -f2-)
echo "READ $(post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"file:///etc/hostname"}}' | tail -n 1)"
echo "PROMPT $(post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","id":3,"method":"prompts/get","params":{"name":"p"}}' | tail -n 1)"
"#;
    let add = |allow: &[&str]| {
        let mut args = vec!["tool", "add", "fake"];
        for call in allow {
            args.extend(["--add", call]);
        }
        args.extend(["--", "sh", server]);
        let (ok, text) = ran(&mut lnk(&root, &args));
        assert!(ok, "{text}");
    };
    let run = || {
        let (_, text) = ran(&mut lnk(
            &root,
            &[
                "run",
                "--commands.toolchains",
                "shell",
                "--internet.hosts",
                "*",
                "--tools.names",
                "fake",
                "--",
                "sh",
                "-c",
                client,
            ],
        ));
        let line = |tag: &str| {
            text.lines()
                .find(|l| l.starts_with(tag))
                .unwrap_or_default()
                .to_string()
        };
        (line("READ"), line("PROMPT"), text.clone())
    };
    add(&["echo"]);
    let (read, prompt, text) = run();
    assert!(
        read.contains("lnk sandbox tool add fake --add resources/read"),
        "{text}"
    );
    assert!(
        prompt.contains("lnk sandbox tool add fake --add prompts/get"),
        "{text}"
    );
    add(&["resources/read", "prompts/get"]);
    let (read, prompt, text) = run();
    assert!(read.contains(r#""result":{}"#), "{text}");
    assert!(prompt.contains(r#""result":{}"#), "{text}");
}

/// A session started before its tool changed (added again, with another
/// `--env` such as a replaced token) is gone: the old process and its
/// environment aren't used again, and the next session runs as the tool
/// is now.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_changed_tool_ends_its_sessions() {
    if !link_test_kit::gate::bubblewrap() || !link_test_kit::gate::program("curl", &["--version"]) {
        return;
    }
    let root = TempDir::new("tools-changed");
    let server = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp-server.sh");
    // A token by its name, its value from the environment, as a secret
    // is given (never `--env TOKEN=<value>` on the command line).
    let add = |token: &str| {
        let (ok, text) = ran(lnk(
            &root,
            &[
                "tool", "add", "fake", "--add", "echo", "--env", "TOKEN", "--", "sh", server,
            ],
        )
        .env("TOKEN", token));
        assert!(ok, "{text}");
    };
    add("old");
    let settings = root.join(".lnk/safe");
    // A call, a wait while the tool is added again, the same session's
    // next call, and a new session's.
    let client = format!(
        r#"LNK_SETTINGS={}
u=$LNK_TOOL_FAKE
post() {{ curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }}
start() {{ post -d '{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"t","version":"1"}}}}}}' | tr -d '\r' | grep -i '^Mcp-Session-Id: ' | cut -d' ' -f2-; }}
call() {{ post -H "Mcp-Session-Id: $1" -d '{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"echo","arguments":{{}}}}}}'; }}
id=$(start)
echo "BEFORE $(call $id | tail -n 1)"
sleep 3
echo "SAME $(call $id | head -n 1)"
echo "NEW $(call $(start) | tail -n 1)"
"#,
        settings.display()
    );
    let running = lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--tools.names",
            "fake",
            "--",
            "sh",
            "-c",
            &client,
        ],
    )
    .spawn()
    .unwrap();
    // Once the first call is in, the token is replaced.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !std::fs::read_to_string(root.join("calls")).is_ok_and(|c| !c.is_empty()) {
        assert!(Instant::now() < deadline, "the first call never came");
        std::thread::sleep(Duration::from_millis(50));
    }
    add("new");
    let output = running.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    let line = |tag: &str| {
        text.lines()
            .find(|l| l.starts_with(tag))
            .unwrap_or_default()
            .to_string()
    };
    assert!(line("BEFORE").contains("token=old"), "{text}");
    assert!(line("SAME").contains("404"), "{text}");
    assert!(line("NEW").contains("token=new"), "{text}");
    let calls = std::fs::read_to_string(root.join("calls")).unwrap();
    assert_eq!(calls.lines().count(), 2, "{calls}");
}

/// A tool that's a server at a URL: `lnk sandbox tool add --url`, its
/// header (a token) sent from outside, never seen in the sandbox.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_sandbox_calls_a_server_at_a_url_with_a_header_it_never_sees() {
    if !link_test_kit::gate::bubblewrap()
        || !link_test_kit::gate::program("curl", &["--version"])
        || !link_test_kit::gate::program("python3", &["--version"])
    {
        return;
    }
    let root = TempDir::new("tools-url");
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp-http.py");
    let port_file = root.join("port");
    let mut server = Command::new("python3")
        .args([fixture, port_file.to_str().unwrap()])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !port_file.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let port = std::fs::read_to_string(&port_file).unwrap();
    let url = format!("http://127.0.0.1:{port}/mcp");
    let headers = root.join("remote-headers");
    std::fs::write(&headers, "Authorization: Bearer s3cret-token\n").unwrap();
    let (ok, text) = ran(&mut lnk(
        &root,
        &[
            "tool",
            "add",
            "remote",
            "--url",
            &url,
            "--header-file",
            &headers.display().to_string(),
            "--ask",
            "none",
        ],
    ));
    assert!(ok, "{text}");
    assert!(!text.contains("s3cret"), "{text}");
    let (ok, text) = ran(&mut lnk(&root, &["tool-call", "list", "remote"]));
    assert!(ok && text.contains("echo"), "{text}");

    let client = r#"
u=$LNK_TOOL_REMOTE
post() { curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }
id=$(post -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | tr -d '\r' | grep -i '^Mcp-Session-Id: ' | cut -d' ' -f2-)
echo "CALL $(post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{}}}' | tail -n 1)"
env | grep -q s3cret && echo TOKEN-IN-THE-SANDBOX
"#;
    let (_, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--tools.names",
            "remote",
            "--",
            "sh",
            "-c",
            client,
        ],
    ));
    let _ = server.kill();
    let _ = server.wait();
    let call = text
        .lines()
        .find(|l| l.starts_with("CALL"))
        .unwrap_or_default();
    assert!(call.contains("echoed with Bearer s3cret-token"), "{text}");
    assert!(!text.contains("TOKEN-IN-THE-SANDBOX"), "{text}");
}

/// A shared tool runs one server for every session of every sandbox
/// naming it: two sandboxes, one after the other, call one server; it
/// stops once no session has been open for the sandbox's `shared_idle`,
/// and the next session starts it again.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_shared_tool_runs_once_for_every_sandbox() {
    if !link_test_kit::gate::bubblewrap() || !link_test_kit::gate::program("curl", &["--version"]) {
        return;
    }
    let root = TempDir::new("tools-shared");
    std::fs::create_dir_all(root.join(".lnk/safe/sandboxes")).unwrap();
    std::fs::write(
        root.join(".lnk/safe/sandboxes/settings.toml"),
        "schema = 1\nshared_idle = \"3s\"\ntool_idle = \"1s\"\n",
    )
    .unwrap();
    let server = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp-server.sh");
    let (ok, text) = ran(&mut lnk(
        &root,
        &[
            "tool", "add", "fake", "--shared", "--add", "echo", "--", "sh", server,
        ],
    ));
    assert!(ok, "{text}");
    let client = r#"
u=$LNK_TOOL_FAKE
post() { curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }
id=$(post -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | tr -d '\r' | grep -i '^Mcp-Session-Id: ' | cut -d' ' -f2-)
echo "LIST $(post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","id":7,"method":"tools/list"}' | tail -n 1)"
echo "ECHO $(post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"echo","arguments":{}}}' | tail -n 1)"
"#;
    let session = || {
        let (ok, text) = ran(&mut lnk(
            &root,
            &[
                "run",
                "--commands.toolchains",
                "shell",
                "--internet.hosts",
                "*",
                "--tools.names",
                "fake",
                "--",
                "sh",
                "-c",
                client,
            ],
        ));
        assert!(ok, "{text}");
        let line = |tag: &str| {
            text.lines()
                .find(|l| l.starts_with(tag))
                .unwrap_or_default()
                .to_string()
        };
        // Each session's answers carry its own ids.
        assert!(line("LIST").contains(r#""id":7"#), "{text}");
        assert!(line("LIST").contains("delete_all"), "{text}");
        assert!(line("ECHO").contains(r#""id":8"#), "{text}");
        assert!(line("ECHO").contains("home="), "{text}");
    };
    let starts = || {
        std::fs::read_to_string(root.join("starts"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    session();
    session();
    assert_eq!(starts(), 1, "two sandboxes, one server");
    let calls = std::fs::read_to_string(root.join("calls")).unwrap();
    assert_eq!(calls.lines().count(), 2, "{calls}");
    // Idle past shared_idle: stopped, its socket gone.
    let sockets =
        || {
            std::fs::read_dir(link_files::layout::Runtime::SharedTools.at(
                &link_files::layout::Root::Runtime.default_in(&link_test_kit::env::runtime(&root)),
            ))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "sock"))
            .count()
        };
    let deadline = Instant::now() + Duration::from_secs(30);
    while sockets() > 0 {
        assert!(Instant::now() < deadline, "the hub never stopped");
        std::thread::sleep(Duration::from_millis(100));
    }
    session();
    assert_eq!(starts(), 2, "started again for the next session");
}

/// A shared tool for a sandbox whose proxy runs with another HOME, as an
/// agent's runs with its harness's: wrapped with Link's settings, run
/// with the harness's home, where the harness planted a link to the
/// tool's definition and settings of its own. The hub is started with
/// Link's settings folder: it listens there, never in the harness's
/// home, and keeps Link's `shared_idle`, not the harness's.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_shared_tool_keeps_to_links_settings_whatever_its_proxys_home() {
    use std::io::Write;
    if !link_test_kit::gate::bubblewrap() || !link_test_kit::gate::program("curl", &["--version"]) {
        return;
    }
    let root = link_test_kit::TempDir::new("tools-apart");
    let (harness, net) = (root.join("harness"), root.join("net"));
    let real = root.join(".lnk/safe/sandboxes");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(
        real.join("settings.toml"),
        "schema = 1\nshared_idle = \"2s\"\ntool_idle = \"1s\"\n",
    )
    .unwrap();
    let server = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp-server.sh");
    let (ok, text) = ran(&mut lnk(
        &root,
        &[
            "tool", "add", "fake", "--shared", "--add", "echo", "--", "sh", server,
        ],
    ));
    assert!(ok, "{text}");
    // The harness's own: a link to the definition, and settings that
    // would keep the hub up.
    let planted = harness.join(".lnk/safe/sandboxes");
    std::fs::create_dir_all(planted.join("tools")).unwrap();
    std::fs::create_dir_all(&net).unwrap();
    std::os::unix::fs::symlink(
        real.join("tools/fake.json"),
        planted.join("tools/fake.json"),
    )
    .unwrap();
    std::fs::write(
        planted.join("settings.toml"),
        "schema = 1\nshared_idle = \"1h\"\n",
    )
    .unwrap();
    let client = r#"
u=$LNK_TOOL_FAKE
post() { curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }
id=$(post -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | tr -d '\r' | grep -i '^Mcp-Session-Id: ' | cut -d' ' -f2-)
echo "ECHO $(post -H "Mcp-Session-Id: $id" -d '{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"echo","arguments":{}}}' | tail -n 1)"
"#;
    // Its tools' route, on the sandbox's bridge for Link's settings here.
    let bridge = link_sandbox_route::Bridge::at(
        &root.join("run"),
        &link_files::layout::Root::Hidden.default_in(&root),
        std::path::Path::new(env!("CARGO_BIN_EXE_lnk-sandbox")),
        "sandbox",
        None,
        &link_files::layout::Hidden::AgentCredentials.in_home(&root),
    );
    let tools = link_sandbox_client::routes::Tools {
        tools: vec!["fake".into()],
        desktop: Vec::new(),
        asking: None,
    };
    let tools_route = link_sandbox_client::routes::tools(bridge, tools).unwrap();
    let request = serde_json::json!({
        "confinement": {
            "name": "apart",
            "write": [harness],
            "read": [],
            "commands": "any",
            "net": {"proxied": {
                "proxy": {"upstream": "direct", "dir": net, "routes": [tools_route]},
                "ports": [],
            }},
        },
        "argv": ["/bin/sh", "-c", client],
    });
    let mut wrap = lnk(&root, &["wrap"]).stdin(Stdio::piped()).spawn().unwrap();
    wrap.stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let wrapped = wrap.wait_with_output().unwrap();
    assert!(
        wrapped.status.success(),
        "{}",
        String::from_utf8_lossy(&wrapped.stderr)
    );
    let wrapped: serde_json::Value = serde_json::from_slice(&wrapped.stdout).unwrap();
    let argv: Vec<String> = serde_json::from_value(wrapped["argv"].clone()).unwrap();
    let env: std::collections::BTreeMap<String, String> =
        serde_json::from_value(wrapped["env"].clone()).unwrap();
    let (ok, text) = ran(Command::new(&argv[0])
        .args(&argv[1..])
        .envs(&env)
        .env("HOME", &harness)
        .env_remove("XDG_CONFIG_HOME")
        .current_dir(&harness)
        .stdin(Stdio::null()));
    assert!(ok, "{text}");
    let echo = text
        .lines()
        .find(|l| l.starts_with("ECHO"))
        .unwrap_or_default();
    assert!(echo.contains(&format!("home={}", root.display())), "{text}");
    let sockets =
        |dir: &Path| {
            std::fs::read_dir(link_files::layout::Runtime::SharedTools.at(
                &link_files::layout::Root::Runtime.default_in(&link_test_kit::env::runtime(dir)),
            ))
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "sock"))
                    .count()
            })
            .unwrap_or(0)
        };
    assert_eq!(sockets(&planted), 0, "never in the harness's home");
    // Link's shared_idle, not the harness's hour.
    let deadline = Instant::now() + Duration::from_secs(30);
    while sockets(&real) > 0 {
        assert!(
            Instant::now() < deadline,
            "the hub kept the harness's settings"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A shared tool's server that doesn't end when its input closes is
/// ended with its hub, once idle: never left running.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_shared_server_ends_with_its_hub() {
    if !link_test_kit::gate::bubblewrap() || !link_test_kit::gate::program("curl", &["--version"]) {
        return;
    }
    let root = link_test_kit::TempDir::short("stubborn");
    std::fs::create_dir_all(root.join(".lnk/safe/sandboxes")).unwrap();
    std::fs::write(
        root.join(".lnk/safe/sandboxes/settings.toml"),
        "schema = 1\nshared_idle = \"2s\"\ntool_idle = \"1s\"\n",
    )
    .unwrap();
    let server = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mcp-stubborn.sh"
    );
    let (ok, text) = ran(&mut lnk(
        &root,
        &["tool", "add", "stubborn", "--shared", "--", "sh", server],
    ));
    assert!(ok, "{text}");
    let client = r#"
u=$LNK_TOOL_STUBBORN
curl -s --noproxy '*' -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' "$u" >/dev/null
echo DONE
"#;
    let (ok, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--tools.names",
            "stubborn",
            "--",
            "sh",
            "-c",
            client,
        ],
    ));
    assert!(ok && text.contains("DONE"), "{text}");
    let pid: i32 = std::fs::read_to_string(root.join("stubborn.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 only asks whether the process is there.
    let alive = || unsafe { libc::kill(pid, 0) } == 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while alive() {
        assert!(Instant::now() < deadline, "the server outlived its hub");
        std::thread::sleep(Duration::from_millis(100));
    }
}
