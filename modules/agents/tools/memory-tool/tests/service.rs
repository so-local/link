//! `lnk-memory-tool serve-many`: one process serving two agents' folders,
//! each session answered from the agent its first line names and no
//! other. Neither agent's session finds, reads or writes the other's,
//! one naming an agent the service doesn't serve is refused, and the
//! service ends, its socket gone, once no session is open. A session
//! slow on its search's words holds up no other session of its agent,
//! and an agent slow to open holds up no other agent.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use link_memory_tool_contract::declared::{Agent, Agents};
use serde_json::{Value, json};

/// A session with the service at `socket`, as `agent`: its welcome, and
/// the connection.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn session(socket: &Path, agent: &str) -> (Value, UnixStream, BufReader<UnixStream>) {
    let started = std::time::Instant::now();
    let stream = loop {
        match UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(_) if started.elapsed().as_secs() < 10 => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(err) => panic!("no service at {}: {err}", socket.display()),
        }
    };
    let mut writer = stream.try_clone().unwrap();
    writeln!(writer, "{}", json!({"agent": agent})).unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    (serde_json::from_str(&line).unwrap(), writer, reader)
}

/// `name`'s answer to `arguments`, in an open session, as the model
/// reads it: a search's once it says nothing waits to be indexed, as
/// memory reads its folder in the background.
fn call(
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    if name != "search" {
        return call_once(writer, reader, id, name, arguments);
    }
    link_test_kit::wait::eventually("the folder indexed", Duration::from_secs(10), || {
        let answer = call_once(writer, reader, id, name, arguments.clone());
        answer["indexing"].is_null().then_some(answer)
    })
}

/// `name`'s answer to `arguments`, as the model reads it, at once.
fn call_once(
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                         "params": {"name": name, "arguments": arguments}});
    writeln!(writer, "{message}").unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    // As the model reads it: what the documents say is in its fenced
    // text alone; a refusal is its words.
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    serde_json::from_str(&link_untrusted::unfenced(text))
        .unwrap_or_else(|_| reply["result"].clone())
}

/// An agent's files folder, with `word` in a file of its own, and its
/// notes folder.
fn agent(root: &Path, name: &str, word: &str) -> PathBuf {
    let folder = root.join(name);
    std::fs::create_dir_all(folder.join("instructions")).unwrap();
    std::fs::create_dir_all(folder.join("memory")).unwrap();
    std::fs::write(
        folder.join(format!("memory/{name}.md")),
        format!("# {name}'s file\n\nThe secret word is {word}.\n"),
    )
    .unwrap();
    folder
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn each_session_is_its_agents_alone() {
    let root = link_test_kit::TempDir::new("service");
    let me = env!("CARGO_BIN_EXE_lnk-memory-tool");
    let served = |folder: &Path, name: &str| Agent {
        argv: vec![
            me.to_string(),
            "serve".into(),
            folder.display().to_string(),
            "--index".into(),
            root.join(format!("{name}.sqlite")).display().to_string(),
            "--notes".into(),
            folder.join("instructions/notes.md").display().to_string(),
        ],
        env: Default::default(),
    };
    let (ada, bea) = (agent(&root, "ada", "heron"), agent(&root, "bea", "otter"));
    let agents = Agents(
        [
            ("ada0000000aa".to_string(), served(&ada, "ada")),
            ("bea0000000bb".to_string(), served(&bea, "bea")),
        ]
        .into_iter()
        .collect(),
    );
    let file = root.join("agents.json");
    std::fs::write(&file, link_files::saved::print_json(&agents).unwrap()).unwrap();
    let socket = root.join("memory.sock");
    let mut service = Command::new(me)
        .args([
            "serve-many",
            &socket.display().to_string(),
            &file.display().to_string(),
        ])
        .spawn()
        .unwrap();

    // Each agent finds its own word and never the other's, by search, by
    // list or by reading the other's file by name.
    let (welcome, mut a_out, mut a_in) = session(&socket, "ada0000000aa");
    assert_eq!(welcome, json!({"ok": true}));
    let (welcome, mut b_out, mut b_in) = session(&socket, "bea0000000bb");
    assert_eq!(welcome, json!({"ok": true}));
    for (out, input, own, other, other_name) in [
        (&mut a_out, &mut a_in, "heron", "otter", "bea"),
        (&mut b_out, &mut b_in, "otter", "heron", "ada"),
    ] {
        let found = call(out, input, 1, "search", json!({"query": own})).to_string();
        assert!(found.contains(own), "{found}");
        let found = call(out, input, 2, "search", json!({"query": other})).to_string();
        assert!(!found.contains(other), "{found}");
        let listed = call(out, input, 3, "list", json!({})).to_string();
        assert!(!listed.contains(other_name), "{listed}");
        for path in [
            format!("memory/{other_name}.md"),
            format!("../{other_name}/memory/{other_name}.md"),
            format!("memory/../../{other_name}/memory/{other_name}.md"),
        ] {
            let read = call(out, input, 4, "read", json!({"path": path}));
            assert!(!read.to_string().contains(other), "{read}");
        }
    }

    // A note goes in its own agent's notes alone.
    call(
        &mut a_out,
        &mut a_in,
        5,
        "remember",
        json!({"note": "likes herons"}),
    );
    let notes = std::fs::read_to_string(ada.join("instructions/notes.md")).unwrap();
    assert!(notes.contains("likes herons"), "{notes}");
    assert!(!bea.join("instructions/notes.md").exists());

    // An agent it doesn't serve is refused, and its session closed.
    let (welcome, mut out, mut input) = session(&socket, "cid0000000cc");
    assert_eq!(welcome["ok"], Value::Null, "{welcome}");
    assert!(welcome["error"].as_str().unwrap().contains("no agent"));
    let _ = writeln!(
        out,
        "{}",
        json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
    );
    let mut line = String::new();
    assert_eq!(input.read_line(&mut line).unwrap_or(0), 0, "{line}");

    // With no session open, it ends, its socket gone first.
    drop((a_out, a_in, b_out, b_in, out, input));
    let status = service.wait().unwrap();
    assert!(status.success(), "{status}");
    assert!(!socket.exists());
}

/// A fake `/v1/embeddings`, each request in a thread of its own, that
/// takes `slow` to answer for words with "slowly" in them.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn embeddings_server(slow: Duration) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let asked: Value = serde_json::from_slice(&body).unwrap();
                let texts = asked["input"].as_array().unwrap();
                if texts.iter().any(|t| t.as_str().unwrap().contains("slowly")) {
                    std::thread::sleep(slow);
                }
                let data: Vec<Value> = (0..texts.len())
                    .map(|i| json!({"index": i, "embedding": [1.0, 0.5]}))
                    .collect();
                let reply = json!({"data": data}).to_string();
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            });
        }
    });
    url
}

/// The service over `agents` (each a folder, and the arguments its
/// `serve` takes after it), on a socket in `root`.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn service(root: &Path, agents: &[(&str, &Path, &[String])]) -> (PathBuf, Child) {
    let me = env!("CARGO_BIN_EXE_lnk-memory-tool");
    let agents = Agents(
        agents
            .iter()
            .map(|(id, folder, args)| {
                let mut argv = vec![me.to_string(), "serve".into(), folder.display().to_string()];
                argv.extend(args.iter().cloned());
                let served = Agent {
                    argv,
                    env: Default::default(),
                };
                (id.to_string(), served)
            })
            .collect(),
    );
    let file = root.join("agents.json");
    std::fs::write(&file, link_files::saved::print_json(&agents).unwrap()).unwrap();
    let socket = root.join("memory.sock");
    let child = Command::new(me)
        .args([
            "serve-many",
            &socket.display().to_string(),
            &file.display().to_string(),
        ])
        .spawn()
        .unwrap();
    (socket, child)
}

/// A session waiting for its search's words to be embedded holds up no
/// other session of its agent: the other's calls are answered at once.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_slow_search_holds_up_no_other_session() {
    let root = link_test_kit::TempDir::new("service-slow");
    let ada = agent(&root, "ada", "heron");
    let url = embeddings_server(Duration::from_secs(5));
    let index = root.join("ada.sqlite").display().to_string();
    let args: Vec<String> = ["--index", &index, "--embed", "fake", "--embed-url", &url]
        .map(String::from)
        .to_vec();
    let (socket, mut service) = service(&root, &[("ada0000000aa", &ada, &args)]);

    let (welcome, mut a_out, mut a_in) = session(&socket, "ada0000000aa");
    assert_eq!(welcome, json!({"ok": true}));
    // Until search by meaning is on.
    let started = Instant::now();
    while call(
        &mut a_out,
        &mut a_in,
        1,
        "search",
        json!({"query": "heron"}),
    )["semantic"]
        .is_string()
    {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "meaning never on"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let search = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                        "params": {"name": "search", "arguments": {"query": "heron, slowly"}}});
    writeln!(a_out, "{search}").unwrap();
    std::thread::sleep(Duration::from_millis(300));

    let asked = Instant::now();
    let (welcome, mut b_out, mut b_in) = session(&socket, "ada0000000aa");
    assert_eq!(welcome, json!({"ok": true}));
    let found = call(
        &mut b_out,
        &mut b_in,
        1,
        "search",
        json!({"query": "heron"}),
    );
    let waited = asked.elapsed();
    eprintln!("another session's welcome and search: {waited:?}");
    assert!(found.to_string().contains("heron"), "{found}");
    assert!(waited < Duration::from_secs(2), "{waited:?}");

    let mut line = String::new();
    a_in.read_line(&mut line).unwrap();
    assert!(line.contains("heron"), "{line}");
    drop((a_out, a_in, b_out, b_in));
    assert!(service.wait().unwrap().success());
}

/// A model served for an agent that never answers holds up neither its
/// welcome nor its calls: its session is welcomed and searches by words
/// while the model starts.
#[test]
fn a_model_that_never_answers_holds_up_no_session() {
    let root = link_test_kit::TempDir::new("service-silent");
    let ada = agent(&root, "ada", "heron");
    // Taken by the kernel, never answered: the model still starting.
    let silent = link_test_kit::net::Held::loopback();
    let url = format!("http://{}", silent.address());
    let index = root.join("ada.sqlite").display().to_string();
    let args: Vec<String> = ["--index", &index, "--embed", "fake", "--embed-url", &url]
        .map(String::from)
        .to_vec();
    let (socket, mut service) = service(&root, &[("ada0000000aa", &ada, &args)]);
    let asked = Instant::now();
    let (welcome, mut out, mut input) = session(&socket, "ada0000000aa");
    let welcomed = asked.elapsed();
    assert_eq!(welcome, json!({"ok": true}));
    // Its reads have a deadline: one waiting on the model fails, never hangs.
    input
        .get_ref()
        .set_read_timeout(Some(link_test_kit::wait::scaled(Duration::from_secs(10))))
        .unwrap();
    let found = call(&mut out, &mut input, 1, "search", json!({"query": "heron"}));
    eprintln!("welcomed in {welcomed:?}");
    assert!(welcomed < Duration::from_secs(1), "{welcomed:?}");
    assert!(found.to_string().contains("heron"), "{found}");
    assert!(
        found["semantic"]
            .as_str()
            .is_some_and(|said| said.contains("while the embedding model starts")),
        "{found}"
    );
    drop((out, input, silent));
    assert!(service.wait().unwrap().success());
}

/// An agent whose index is slow to open holds up no other agent's
/// first session.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_slow_open_holds_up_no_other_agent() {
    let root = link_test_kit::TempDir::new("service-open");
    let (ada, bea) = (agent(&root, "ada", "heron"), agent(&root, "bea", "otter"));
    let index = |name: &str| {
        vec![
            "--index".to_string(),
            root.join(format!("{name}.sqlite")).display().to_string(),
        ]
    };
    let (ada_args, bea_args) = (index("ada"), index("bea"));
    let (socket, mut service) = service(
        &root,
        &[
            ("ada0000000aa", &ada, &ada_args),
            ("bea0000000bb", &bea, &bea_args),
        ],
    );
    // Ada's index held by another for a while: opening it waits.
    let held = rusqlite::Connection::open(root.join("ada.sqlite")).unwrap();
    held.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let a = {
        let socket = socket.clone();
        std::thread::spawn(move || {
            let (welcome, out, input) = session(&socket, "ada0000000aa");
            (welcome, out, input)
        })
    };
    std::thread::sleep(Duration::from_millis(300));

    let asked = Instant::now();
    let (welcome, mut b_out, mut b_in) = session(&socket, "bea0000000bb");
    let waited = asked.elapsed();
    eprintln!("another agent's welcome: {waited:?}");
    assert_eq!(welcome, json!({"ok": true}));
    assert!(waited < Duration::from_secs(3), "{waited:?}");
    let found = call(
        &mut b_out,
        &mut b_in,
        1,
        "search",
        json!({"query": "otter"}),
    );
    assert!(found.to_string().contains("otter"), "{found}");

    std::thread::sleep(Duration::from_secs(1));
    held.execute_batch("ROLLBACK").unwrap();
    let (welcome, mut a_out, mut a_in) = a.join().unwrap();
    assert_eq!(welcome, json!({"ok": true}));
    let found = call(
        &mut a_out,
        &mut a_in,
        1,
        "search",
        json!({"query": "heron"}),
    );
    assert!(found.to_string().contains("heron"), "{found}");
    drop((a_out, a_in, b_out, b_in));
    assert!(service.wait().unwrap().success());
}

/// A message past the most a session's line may be ends that session,
/// never read whole into the service's memory, and the service goes on
/// serving the next.
#[test]
fn an_overlong_message_ends_its_session() {
    let root = link_test_kit::TempDir::new("service-long");
    let ada = agent(&root, "ada", "heron");
    let (socket, mut service) = service(&root, &[("ada0000000aa", &ada, &[])]);
    let (welcome, mut out, mut input) = session(&socket, "ada0000000aa");
    assert_eq!(welcome, json!({"ok": true}));
    let long = "x".repeat(link_memory_tool::limits::limits().message_max_bytes as usize + 10);
    // The service may close before it's all written.
    let _ = writeln!(out, "{long}");
    let mut rest = String::new();
    let _ = input.read_to_string(&mut rest);
    assert!(rest.is_empty(), "{rest}");
    let (welcome, mut out, mut input) = session(&socket, "ada0000000aa");
    assert_eq!(welcome, json!({"ok": true}));
    let found = call(&mut out, &mut input, 1, "search", json!({"query": "heron"}));
    assert!(found.to_string().contains("heron"), "{found}");
    drop((out, input));
    assert!(service.wait().unwrap().success());
}
