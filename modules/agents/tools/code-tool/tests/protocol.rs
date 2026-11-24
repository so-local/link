//! Link code over MCP against a fake language server: this program run
//! again with `--fake-lsp`, so the protocol's edges are this test's own.
//! A file's diagnostics wait on its server's work, a change is seen,
//! definitions and references are found by place and by symbol, no line
//! is shown from outside the files folder, an answer from a server still
//! working says so, a server starting holds up no other, a server that
//! crashes says why, starts again, and leaves the others answering, a
//! server outlives the call that started it, a file no server was given
//! is still seen to change, and every server stops with the tool.
//!
//! Its own `main` (`harness = false`): a test harness would print on
//! the output the fake speaks on.

link_test_kit::program!(unit);

use std::collections::{BTreeMap, HashMap};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use link_agent_spec::CodeServer;
use link_code_tool::lsp::{path_of, read_message, uri_of, write_message};
use link_code_tool::{Code, Config, answer, serve};
use link_test_kit::TempDir;
use link_test_kit::libtest::{Counts, Selection};
use serde_json::{Value, json};

#[derive(clap::Parser)]
struct Cli {
    #[arg(long)]
    fake_lsp: bool,
    #[arg(long, requires = "fake_lsp")]
    outside: Option<String>,
    #[arg(long, requires = "fake_lsp")]
    pids: Option<PathBuf>,
    #[arg(long, requires = "fake_lsp")]
    release: Option<PathBuf>,
    #[command(flatten)]
    selection: Selection,
}

fn main() {
    let given: Cli = link_spec::entry::plain();
    if given.fake_lsp {
        fake(given.outside, given.pids, given.release);
        return;
    }
    let tests: &[(&str, fn())] = &[
        ("its_tools_say_what_it_serves", its_tools_say_what_it_serves),
        (
            "a_file_s_diagnostics_wait_for_its_server",
            a_file_s_diagnostics_wait_for_its_server,
        ),
        (
            "definitions_and_references_by_place_and_by_symbol",
            definitions_and_references_by_place_and_by_symbol,
        ),
        (
            "an_unsettled_server_s_places_say_so",
            an_unsettled_server_s_places_say_so,
        ),
        (
            "one_server_starting_holds_up_no_other",
            one_server_starting_holds_up_no_other,
        ),
        (
            "a_crashed_server_says_why_and_starts_again",
            a_crashed_server_says_why_and_starts_again,
        ),
        (
            "a_crashed_server_leaves_the_others_answering",
            a_crashed_server_leaves_the_others_answering,
        ),
        (
            "every_server_stops_with_the_tool",
            every_server_stops_with_the_tool,
        ),
        (
            "a_server_outlives_the_call_that_started_it",
            a_server_outlives_the_call_that_started_it,
        ),
        (
            "a_file_never_given_is_seen_to_change",
            a_file_never_given_is_seen_to_change,
        ),
        (
            "an_unchanged_file_waits_for_nothing_new",
            an_unchanged_file_waits_for_nothing_new,
        ),
    ];
    let selection = given.selection;
    let selected: Vec<_> = tests
        .iter()
        .filter(|(name, _)| selection.matches(name))
        .collect();
    if selection.list {
        for (name, _) in &selected {
            println!("{name}: test");
        }
        println!("\n{} tests, 0 benchmarks", selected.len());
        return;
    }
    let started = std::time::Instant::now();
    let mut counts = Counts {
        filtered: tests.len() - selected.len(),
        ..Counts::default()
    };
    println!("\nrunning {} tests", selected.len());
    for (name, test) in selected {
        match std::panic::catch_unwind(test) {
            Ok(()) => {
                println!("test {name} ... ok");
                counts.passed += 1;
            }
            Err(_) => {
                println!("test {name} ... FAILED");
                counts.failed += 1;
            }
        }
    }
    println!("\n{}", counts.summary(started.elapsed()));
    if counts.failed > 0 {
        std::process::exit(1);
    }
}

/// The fake: answers as a language server would, from the files it was
/// given. A file with `SILENT` it publishes nothing for. A line with
/// `ERROR` is an error there; `fn <name>` declares a
/// name; a file with `CRASH` ends it, and one with `BUSY` begins work it
/// never ends; `outside` is defined in the file `--outside` names. One
/// started in a root named `slow` answers its start once `--release`
/// is there. A file it's told changed on disk it reads from there. Its
/// file in `--pids` holds the signal it gets when the thread that
/// started it ends, on Linux: 0, none.
fn fake(outside: Option<String>, pids: Option<PathBuf>, release: Option<PathBuf>) {
    if let Some(pids) = pids {
        #[cfg(target_os = "linux")]
        let signal = {
            let mut signal: libc::c_int = 0;
            // SAFETY: reads this process's parent-death signal into `signal`.
            unsafe {
                libc::prctl(libc::PR_GET_PDEATHSIG, &mut signal);
            }
            signal
        };
        #[cfg(not(target_os = "linux"))]
        let signal: libc::c_int = 0;
        let file = pids.join(std::process::id().to_string());
        std::fs::write(file, signal.to_string()).unwrap();
    }
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = std::io::stdout().lock();
    let mut docs: HashMap<String, String> = HashMap::new();
    let mut asked = 1000;
    while let Ok(Some(message)) = read_message(&mut input) {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let id = message.get("id").cloned();
        let reply = |out: &mut std::io::StdoutLock, result: Value| {
            write_message(out, &json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap();
        };
        match method {
            "initialize" => {
                // Its client's process may be one it can't see, so it's
                // never named.
                if !params["processId"].is_null() {
                    write_message(
                        &mut output,
                        &json!({"jsonrpc": "2.0", "id": id,
                        "error": {"code": -32602, "message": "a processId was named"}}),
                    )
                    .unwrap();
                    continue;
                }
                // Asked of the client: answered as one offering nothing.
                for request in ["workspace/configuration", "window/workDoneProgress/create"] {
                    asked += 1;
                    let params = json!({"items": [{"section": "fake"}], "token": "t"});
                    write_message(
                        &mut output,
                        &json!({"jsonrpc": "2.0", "id": asked, "method": request, "params": params}),
                    )
                    .unwrap();
                }
                let root = params["rootUri"].as_str().unwrap_or("").to_string();
                if root.ends_with("/slow")
                    && let Some(release) = &release
                {
                    let released = link_test_kit::wait::within(
                        "the slow server's release",
                        Duration::from_secs(20),
                        || release.exists(),
                    );
                    if released.is_err() {
                        write_message(
                            &mut output,
                            &json!({"jsonrpc": "2.0", "id": id,
                            "error": {"code": -32603, "message": "never released"}}),
                        )
                        .unwrap();
                        continue;
                    }
                }
                reply(
                    &mut output,
                    json!({"capabilities": {}, "serverInfo": {"name": root}}),
                );
            }
            "textDocument/didOpen" | "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap().to_string();
                let text = match method {
                    "textDocument/didOpen" => params["textDocument"]["text"].as_str(),
                    _ => params["contentChanges"][0]["text"].as_str(),
                }
                .unwrap()
                .to_string();
                if text.contains("CRASH") {
                    // Its output closes before it says why, as a busy
                    // machine can order them: why must still be told.
                    // SAFETY: its own output, which nothing writes again.
                    unsafe { libc::close(1) };
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "the fake's crash diagnostic deliberately follows its closed stdout"
                    )]
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    eprintln!("crashed on purpose");
                    std::process::exit(3);
                }
                // Report work explicitly. Only the diagnostics-waiting
                // case needs publication delayed beyond the client's
                // half-second quiet period; other cases test different
                // protocol behavior and publish without that delay.
                let progress = |out: &mut std::io::StdoutLock, kind: &str| {
                    write_message(
                        out,
                        &json!({"jsonrpc": "2.0", "method": "$/progress",
                            "params": {"token": "check", "value": {"kind": kind}}}),
                    )
                    .unwrap();
                };
                progress(&mut output, "begin");
                if text.contains("BUSY") {
                    write_message(
                        &mut output,
                        &json!({"jsonrpc": "2.0", "method": "$/progress",
                            "params": {"token": "busy", "value": {"kind": "begin"}}}),
                    )
                    .unwrap();
                }
                if text.contains("SLOW_DIAGNOSTICS") {
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "diagnostics must arrive past the client's quiet period to prove it waits for reported work"
                    )]
                    std::thread::sleep(Duration::from_millis(800));
                }
                let mut diagnostics = Vec::new();
                for (n, line) in text.lines().enumerate() {
                    if let Some(at) = line.find("ERROR") {
                        diagnostics.push(json!({
                            "range": {"start": {"line": n, "character": at},
                                "end": {"line": n, "character": at + 5}},
                            "severity": 1, "source": "fake", "code": "E1",
                            "message": "found ERROR",
                        }));
                    }
                    if line.contains("WARN") {
                        let cwd = std::env::current_dir().unwrap();
                        diagnostics.push(json!({
                            "range": {"start": {"line": n, "character": 0},
                                "end": {"line": n, "character": 1}},
                            "severity": 2,
                            "message": format!("in {}", cwd.display()),
                        }));
                    }
                }
                if !text.contains("SILENT") {
                    write_message(
                        &mut output,
                        &json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                            "params": {"uri": uri, "diagnostics": diagnostics}}),
                    )
                    .unwrap();
                }
                progress(&mut output, "end");
                docs.insert(uri, text);
            }
            "textDocument/definition" | "textDocument/references" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap();
                let line = params["position"]["line"].as_u64().unwrap() as usize;
                let character = params["position"]["character"].as_u64().unwrap() as usize;
                let text = docs.get(uri).cloned().unwrap_or_default();
                let row = text.lines().nth(line).unwrap_or("");
                let word = word_at(row, character);
                if word == "outside" && method == "textDocument/definition" {
                    let uri = uri_of(Path::new(outside.as_deref().unwrap()));
                    let range = json!({"start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1}});
                    reply(&mut output, json!({"uri": uri, "range": range}));
                    continue;
                }
                let mut found = Vec::new();
                for (uri, text) in &docs {
                    for (n, row) in text.lines().enumerate() {
                        let mut from = 0;
                        while let Some(at) = row[from..].find(&word) {
                            let at = from + at;
                            from = at + word.len();
                            let whole = word_at(row, at) == word;
                            let declared = row[..at].ends_with("fn ");
                            if whole && (declared || method == "textDocument/references") {
                                found.push(json!({"targetUri": uri,
                                    "targetRange": {"start": {"line": n, "character": 0},
                                        "end": {"line": n, "character": 1}},
                                    "targetSelectionRange": {
                                        "start": {"line": n, "character": at},
                                        "end": {"line": n, "character": from}}}));
                            }
                        }
                    }
                }
                reply(&mut output, Value::Array(found));
            }
            "workspace/symbol" => {
                let query = params["query"].as_str().unwrap_or("");
                let mut found = Vec::new();
                for (uri, text) in &docs {
                    for (n, row) in text.lines().enumerate() {
                        if let Some(at) = row.find("fn ") {
                            let name = word_at(row, at + 3);
                            if name.contains(query) {
                                // Its range starts at its keyword.
                                found.push(json!({"name": name, "kind": 12,
                                    "location": {"uri": uri, "range": {
                                        "start": {"line": n, "character": at},
                                        "end": {"line": n, "character": row.len()}}}}));
                            }
                        }
                    }
                }
                reply(&mut output, Value::Array(found));
            }
            "workspace/didChangeWatchedFiles" => {
                for change in params["changes"].as_array().into_iter().flatten() {
                    let uri = change["uri"].as_str().unwrap().to_string();
                    let read = path_of(&uri).and_then(|path| std::fs::read_to_string(path).ok());
                    match (change["type"].as_u64(), read) {
                        (Some(1 | 2), Some(text)) => docs.insert(uri, text),
                        _ => docs.remove(&uri),
                    };
                }
            }
            "shutdown" => reply(&mut output, Value::Null),
            "exit" => return,
            _ => {
                // A reply to what it asked: the client offers nothing.
                if method.is_empty() && message.get("result").is_some() {
                    let result = &message["result"];
                    assert!(result.is_null() || *result == json!([null]), "{message}");
                } else if id.is_some() {
                    reply(&mut output, Value::Null);
                }
            }
        }
        output.flush().unwrap();
    }
}

/// The word around byte `at` of `row`.
fn word_at(row: &str, at: usize) -> String {
    let part = |c: char| c.is_alphanumeric() || c == '_';
    let start = row[..at.min(row.len())]
        .rfind(|c: char| !part(c))
        .map_or(0, |i| i + 1);
    let end = row[start..]
        .find(|c: char| !part(c))
        .map_or(row.len(), |i| start + i);
    row[start..end].to_string()
}

struct World {
    _dir: TempDir,
    files: PathBuf,
    outside: PathBuf,
    pids: PathBuf,
    release: PathBuf,
}

fn world(name: &str) -> World {
    let dir = TempDir::new(&format!("code-{name}"));
    let files = dir.join("files");
    std::fs::create_dir_all(files.join("work/app/src")).unwrap();
    std::fs::write(files.join("work/app/Cargo.toml"), "").unwrap();
    let outside = dir.join("secret.rs");
    std::fs::write(&outside, "a secret line\n").unwrap();
    let pids = dir.join("pids");
    std::fs::create_dir_all(&pids).unwrap();
    World {
        files,
        outside,
        pids,
        release: dir.join("release"),
        _dir: dir,
    }
}

fn code(world: &World) -> Code {
    code_waiting(world, Duration::from_secs(20))
}

/// [`code`], its calls waiting `timeout` on their server.
fn code_waiting(world: &World, timeout: Duration) -> Code {
    let fake = std::env::current_exe().unwrap().display().to_string();
    let command = vec![
        fake,
        "--fake-lsp".into(),
        "--outside".into(),
        world.outside.display().to_string(),
        "--pids".into(),
        world.pids.display().to_string(),
        "--release".into(),
        world.release.display().to_string(),
    ];
    let server = |command: Vec<String>, files: &[&str]| CodeServer {
        command: Some(command),
        files: Some(files.iter().map(|f| f.to_string()).collect()),
        roots: Some(vec!["Cargo.toml".into()]),
        settings: Default::default(),
    };
    Code::new(Config {
        env: Vec::new(),
        files: world.files.clone(),
        path: "/usr/bin:/bin".into(),
        sandbox: Vec::new(),
        servers: BTreeMap::from([
            ("rust".into(), server(command, &["rs"])),
            ("go".into(), server(vec!["gopls".into()], &["go"])),
        ]),
        unavailable: BTreeMap::from([(
            "go".into(),
            "your sandbox file's [commands] doesn't allow it".into(),
        )]),
        timeout,
    })
    .unwrap()
}

fn call(code: &Code, name: &str, arguments: Value) -> (String, Value, bool) {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}});
    let result = answer(code, &message.to_string()).unwrap()["result"].clone();
    let raw = result["content"][0]["text"].as_str().unwrap_or("");
    // What a server or a file says is fenced: diagnostics as such, a
    // place's line as a file's, and what a server said in an error as a
    // program's output.
    let failed = result["isError"] == true;
    let kind = match (failed, name) {
        (true, _) => "output",
        (false, "diagnostics") => "diagnostics",
        (false, _) => "file",
    };
    if let Some(fenced) = link_untrusted::fenced(raw) {
        assert_eq!(fenced.kind, kind, "{raw}");
    }
    let text = link_untrusted::unfenced(raw);
    (text, result["structuredContent"].clone(), failed)
}

fn its_tools_say_what_it_serves() {
    let world = world("tools");
    let code = code(&world);
    let listed = answer(&code, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["definition", "diagnostics", "references"]);
    let init = answer(
        &code,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    )
    .unwrap();
    let said = init["result"]["instructions"].as_str().unwrap();
    assert!(said.contains("Servers here: rust ("), "{said}");
    assert!(
        said.contains("Not here: go (gopls: your sandbox file's"),
        "{said}"
    );
    let unknown = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "rename", "arguments": {}}});
    let refused = answer(&code, &unknown.to_string()).unwrap();
    assert_eq!(refused["error"]["code"], -32602);
    // A call's arguments are held to its declaration before any server
    // is asked.
    let (text, _, failed) = call(
        &code,
        "diagnostics",
        json!({"file_path": "src/main.rs", "all": true}),
    );
    assert!(failed && text.contains("unknown field `all`"), "{text}");
}

fn a_file_s_diagnostics_wait_for_its_server() {
    let world = world("diagnostics");
    let code = code(&world);
    let main = world.files.join("work/app/src/main.rs");
    std::fs::write(
        &main,
        "fn main() { // SLOW_DIAGNOSTICS\n    ERROR;\n    WARN\n}\n",
    )
    .unwrap();
    let (text, value, failed) = call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/main.rs"}),
    );
    assert!(!failed, "{text}");
    assert_eq!(value["total"], 2, "{text}");
    assert_eq!(value["complete"], true, "{text}");
    assert!(
        text.contains("work/app/src/main.rs:2:5: error: found ERROR (fake E1)"),
        "{text}"
    );
    // Started in its project's root, the folder holding Cargo.toml.
    let root = world.files.canonicalize().unwrap().join("work/app");
    assert!(
        text.contains(&format!("warning: in {}", root.display())),
        "{text}"
    );
    // A change is seen: the file as it is now.
    std::fs::write(&main, "fn main() {}\n").unwrap();
    let (text, value, _) = call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/main.rs"}),
    );
    assert_eq!(text, "No problems found in work/app/src/main.rs.");
    assert_eq!(value["total"], 0);
    // Every one the servers report, without a file.
    std::fs::write(world.files.join("work/app/src/lib.rs"), "ERROR\n").unwrap();
    call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/lib.rs"}),
    );
    let (text, _, _) = call(&code, "diagnostics", json!({}));
    assert_eq!(
        text,
        "work/app/src/lib.rs:1:1: error: found ERROR (fake E1)"
    );
    // A file it was given before is given again as it is now when another
    // is asked about: nothing watches files, so this is how a server
    // sees a change.
    std::fs::write(world.files.join("work/app/src/lib.rs"), "fn lib() {}\n").unwrap();
    call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/main.rs"}),
    );
    let (text, _, _) = call(&code, "diagnostics", json!({}));
    assert_eq!(text, "No problems found.");
    // A file gone is closed, and its problems go with it, though its
    // server says nothing more of it.
    std::fs::write(world.files.join("work/app/src/lib.rs"), "ERROR\n").unwrap();
    call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/lib.rs"}),
    );
    std::fs::remove_file(world.files.join("work/app/src/lib.rs")).unwrap();
    let (text, _, _) = call(&code, "diagnostics", json!({}));
    assert_eq!(text, "No problems found.");
    // What it refuses, saying why.
    for (file, says) in [
        ("../secret.rs", "isn't in your files folder"),
        (
            "main.go",
            "would run gopls, but your sandbox file's [commands]",
        ),
        (
            "work/app/Cargo.toml",
            "no language server serves .toml files",
        ),
    ] {
        std::fs::write(world.files.join("main.go"), "").unwrap();
        let (text, _, failed) = call(&code, "diagnostics", json!({"file_path": file}));
        assert!(failed && text.contains(says), "{file}: {text}");
    }
    code.stop();
}

fn definitions_and_references_by_place_and_by_symbol() {
    let world = world("places");
    let code = code(&world);
    let lib = "pub fn helper() {}\n\npub fn é_then() {\n    helper();\n    outside();\n}\n";
    std::fs::write(world.files.join("work/app/src/lib.rs"), lib).unwrap();
    let file = "work/app/src/lib.rs";
    // `helper` called on line 4.
    let (text, value, failed) = call(
        &code,
        "definition",
        json!({"file_path": file, "line": 4, "character": 6}),
    );
    assert!(!failed, "{text}");
    assert_eq!(text, "work/app/src/lib.rs:1:8: pub fn helper() {}");
    assert_eq!(value["places"][0]["line"], 1);
    let (text, value, _) = call(
        &code,
        "references",
        json!({"file_path": file, "line": 1, "character": 9}),
    );
    assert_eq!(value["total"], 2, "{text}");
    assert!(
        text.contains("work/app/src/lib.rs:4:5: helper();"),
        "{text}"
    );
    // By its name: found where it's declared, at the name itself.
    let (text, _, _) = call(&code, "definition", json!({"symbol": "helper"}));
    assert_eq!(text, "work/app/src/lib.rs:1:8: pub fn helper() {}");
    let (text, value, _) = call(
        &code,
        "references",
        json!({"symbol": "helper", "file_path": file}),
    );
    assert_eq!(value["total"], 2, "{text}");
    let (text, _, failed) = call(&code, "definition", json!({"symbol": "help"}));
    assert!(failed && text.contains("close ones:\nhelper"), "{text}");
    // A place outside the files folder: named, never its line.
    let (text, value, _) = call(
        &code,
        "definition",
        json!({"file_path": file, "line": 5, "character": 5}),
    );
    assert!(!text.contains("a secret line"), "{text}");
    assert!(value["places"][0].get("text").is_none(), "{value}");
    assert_eq!(
        path_of(&uri_of(&world.outside)).unwrap(),
        PathBuf::from(value["places"][0]["path"].as_str().unwrap())
    );
    for arguments in [
        json!({"file_path": file, "line": 4}),
        json!({"symbol": "helper", "line": 1, "character": 1}),
        json!({}),
    ] {
        let (text, _, failed) = call(&code, "definition", arguments.clone());
        assert!(
            failed && text.contains("give a file_path, a line and a character"),
            "{arguments}: {text}"
        );
    }
    let (text, _, failed) = call(
        &code,
        "definition",
        json!({"file_path": file, "line": 40, "character": 1}),
    );
    assert!(failed && text.contains("it has 6 lines"), "{text}");
    code.stop();
}

/// A server still working when its share of the call's time ran out
/// answers, and the answer says it may not be all.
fn an_unsettled_server_s_places_say_so() {
    let world = world("unsettled");
    let code = code_waiting(&world, Duration::from_secs(2));
    let lib = "pub fn helper() {}\nfn b() { helper(); } // BUSY\n";
    std::fs::write(world.files.join("work/app/src/lib.rs"), lib).unwrap();
    let file = "work/app/src/lib.rs";
    for call_name in ["references", "definition"] {
        let (text, value, failed) = call(
            &code,
            call_name,
            json!({"file_path": file, "line": 2, "character": 11}),
        );
        assert!(!failed, "{text}");
        assert_eq!(value["complete"], false, "{text}");
        assert!(text.contains("may not be all"), "{text}");
    }
    code.stop();
}

/// One server starting never holds up a call to another: the slow one
/// answers its start only once the other's call has.
fn one_server_starting_holds_up_no_other() {
    let world = world("starting");
    let code = code(&world);
    std::fs::create_dir_all(world.files.join("slow")).unwrap();
    std::fs::write(world.files.join("slow/Cargo.toml"), "").unwrap();
    std::fs::write(world.files.join("slow/a.rs"), "ERROR\n").unwrap();
    std::fs::write(world.files.join("work/app/src/main.rs"), "ERROR\n").unwrap();
    std::thread::scope(|scope| {
        let slow = scope.spawn(|| call(&code, "diagnostics", json!({"file_path": "slow/a.rs"})));
        link_test_kit::wait::eventually("the slow server", Duration::from_secs(20), || {
            std::fs::read_dir(&world.pids).unwrap().count() == 1
        });
        let (text, _, failed) = call(
            &code,
            "diagnostics",
            json!({"file_path": "work/app/src/main.rs"}),
        );
        assert!(!failed && text.contains("found ERROR"), "{text}");
        std::fs::write(&world.release, "").unwrap();
        let (text, _, failed) = slow.join().unwrap();
        assert!(!failed && text.contains("slow/a.rs:1:1"), "{text}");
    });
    code.stop();
}

fn a_crashed_server_says_why_and_starts_again() {
    let world = world("crash");
    let code = code(&world);
    let main = world.files.join("work/app/src/main.rs");
    std::fs::write(&main, "CRASH\n").unwrap();
    let (text, _, failed) = call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/main.rs"}),
    );
    assert!(failed && text.contains("crashed on purpose"), "{text}");
    std::fs::write(&main, "ERROR\n").unwrap();
    let (text, _, failed) = call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/main.rs"}),
    );
    assert!(!failed && text.contains("found ERROR"), "{text}");
    assert_eq!(std::fs::read_dir(&world.pids).unwrap().count(), 2);
    code.stop();
}

fn a_crashed_server_leaves_the_others_answering() {
    let world = world("crash-one");
    let code = code(&world);
    std::fs::create_dir_all(world.files.join("other")).unwrap();
    std::fs::write(world.files.join("other/Cargo.toml"), "").unwrap();
    std::fs::write(world.files.join("work/app/src/main.rs"), "ERROR\n").unwrap();
    std::fs::write(world.files.join("other/a.rs"), "CRASH\n").unwrap();
    let (text, _, failed) = call(
        &code,
        "diagnostics",
        json!({"file_path": "work/app/src/main.rs"}),
    );
    assert!(!failed && text.contains("found ERROR"), "{text}");
    let (text, _, failed) = call(&code, "diagnostics", json!({"file_path": "other/a.rs"}));
    assert!(failed && text.contains("crashed on purpose"), "{text}");
    let (text, _, failed) = call(&code, "diagnostics", json!({}));
    assert!(!failed && text.contains("found ERROR"), "{text}");
    code.stop();
}

fn every_server_stops_with_the_tool() {
    let world = world("stop");
    let code = code(&world);
    std::fs::create_dir_all(world.files.join("other")).unwrap();
    std::fs::write(world.files.join("other/Cargo.toml"), "").unwrap();
    std::fs::write(world.files.join("work/app/src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(world.files.join("other/a.rs"), "fn a() {}\n").unwrap();
    let mut input = String::new();
    for (id, file) in ["work/app/src/main.rs", "other/a.rs"].iter().enumerate() {
        input.push_str(
            &json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": "diagnostics", "arguments": {"file_path": file}}})
            .to_string(),
        );
        input.push('\n');
    }
    let mut output = Vec::new();
    serve(&code, input.as_bytes(), &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap().lines().count(), 2);
    // One server per project root, each gone once the tool's input ends.
    let pids: Vec<i32> = std::fs::read_dir(&world.pids)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().parse().unwrap())
        .collect();
    assert_eq!(pids.len(), 2, "{pids:?}");
    for pid in pids {
        // SAFETY: signal 0 only asks whether the process is there.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "server {pid} outlived the tool");
    }
}

/// The lines a test feeds [`serve`] one at a time, as a harness would.
struct Fed {
    lines: std::sync::mpsc::Receiver<String>,
    left: Vec<u8>,
}

impl std::io::Read for Fed {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        if self.left.is_empty() {
            match self.lines.recv() {
                Ok(line) => self.left = format!("{line}\n").into_bytes(),
                Err(_) => return Ok(0),
            }
        }
        let n = into.len().min(self.left.len());
        into[..n].copy_from_slice(&self.left[..n]);
        self.left.drain(..n);
        Ok(n)
    }
}

/// What [`serve`] answers, sent on line by line.
struct Answers {
    lines: std::sync::mpsc::Sender<String>,
    kept: Vec<u8>,
}

impl Write for Answers {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.kept.extend_from_slice(bytes);
        while let Some(end) = self.kept.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.kept.drain(..=end).collect();
            let _ = self
                .lines
                .send(String::from_utf8_lossy(&line).trim().to_string());
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A server started by one call, on a thread of its own that ends with
/// it, serves the next call: its watch follows this process, not that
/// thread.
fn a_server_outlives_the_call_that_started_it() {
    let world = world("outlives");
    let code = code(&world);
    std::fs::write(world.files.join("work/app/src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(world.files.join("work/app/src/lib.rs"), "ERROR\n").unwrap();
    let (feed, lines) = std::sync::mpsc::channel();
    let (sent, answers) = std::sync::mpsc::channel();
    let ask = |id: u64, file: &str| {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "diagnostics", "arguments": {"file_path": file}}})
        .to_string()
    };
    std::thread::scope(|scope| {
        let input = BufReader::new(Fed {
            lines,
            left: Vec::new(),
        });
        let output = Answers {
            lines: sent,
            kept: Vec::new(),
        };
        let served = scope.spawn(|| serve(&code, input, output));
        let wait = link_test_kit::wait::scaled(Duration::from_secs(20));
        feed.send(ask(1, "work/app/src/main.rs")).unwrap();
        let first = answers.recv_timeout(wait).unwrap();
        assert!(first.contains("No problems found"), "{first}");
        // Asked once the first call's thread has answered and ended.
        feed.send(ask(2, "work/app/src/lib.rs")).unwrap();
        let second = answers.recv_timeout(wait).unwrap();
        assert!(second.contains("found ERROR"), "{second}");
        drop(feed);
        served.join().unwrap().unwrap();
    });
    // One server served both, never told to end with a thread.
    let pids: Vec<_> = std::fs::read_dir(&world.pids)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(pids.len(), 1, "{pids:?}");
    assert_eq!(std::fs::read_to_string(&pids[0]).unwrap(), "0");
}

/// A file under a server's root it was never given is seen to change:
/// it's told the file was created, changed, and deleted, since it's
/// refused watching files itself.
fn a_file_never_given_is_seen_to_change() {
    let world = world("watched");
    let code = code(&world);
    let lib = "work/app/src/lib.rs";
    std::fs::write(world.files.join(lib), "pub fn given() {}\n").unwrap();
    let (text, _, failed) = call(&code, "diagnostics", json!({"file_path": lib}));
    assert!(!failed, "{text}");
    // Never given to the server: only on disk.
    let other = world.files.join("work/app/src/other.rs");
    std::fs::write(&other, "pub fn on_disk() {}\n").unwrap();
    let (text, _, failed) = call(&code, "definition", json!({"symbol": "on_disk"}));
    assert!(!failed, "{text}");
    assert_eq!(text, "work/app/src/other.rs:1:8: pub fn on_disk() {}");
    std::fs::write(&other, "pub fn renamed_there() {}\n").unwrap();
    let (text, _, failed) = call(&code, "definition", json!({"symbol": "renamed_there"}));
    assert!(!failed, "{text}");
    std::fs::remove_file(&other).unwrap();
    let (text, _, failed) = call(&code, "definition", json!({"symbol": "renamed_there"}));
    assert!(failed && text.contains("no symbol named"), "{text}");
    code.stop();
}

/// A file given unchanged has nothing new to publish, so its call waits
/// only for its server to be quiet, never for a publication: within a
/// call's time shorter than `unpublished-ms`, it settles.
fn an_unchanged_file_waits_for_nothing_new() {
    let world = world("unchanged");
    let code = code_waiting(&world, Duration::from_secs(2));
    let main = "work/app/src/main.rs";
    std::fs::write(world.files.join(main), "fn main() {} // SILENT\n").unwrap();
    // Opened, it's waited on for a publication that never comes.
    let (text, value, failed) = call(&code, "diagnostics", json!({"file_path": main}));
    assert!(!failed, "{text}");
    assert_eq!(value["complete"], false, "{text}");
    let (text, value, failed) = call(&code, "diagnostics", json!({"file_path": main}));
    assert!(!failed, "{text}");
    assert_eq!(value["complete"], true, "{text}");
    code.stop();
}
