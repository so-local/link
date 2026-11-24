//! An agent's memory, for real, on Linux: `lnk agent memory start` serves
//! Link Memory (`lnk-memory-tool`) over the agent's files as its tool,
//! one confined process for every agent of its owner, each session its
//! own agent's alone, with its embedding model served for it outside its
//! sandbox, and nothing of it running once no session is open; a command
//! in the agent's sandbox (`lnk agent harness run`) searches it at
//! `$LNK_MEMORY` through its proxy, finding a file and a conversation by
//! meaning and saying which is which, and notes something for itself,
//! which only its notes file holds: the engine writes no other file of
//! the folder, and none of it once the agent's instructions are locked.
//! `stop` takes it away.

//!
//! It runs `lnk`, `lnk-agent` and `lnk-sandbox`, built in `link-core`
//! beside this folder (`LNK_CORE` for another place), and this
//! folder's `lnk-memory-tool`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `program` of `link-core` (`lnk-agent`, `lnk-sandbox`), built there
/// with all its programs.
fn core_program(program: &str) -> PathBuf {
    let core = std::env::var_os("LNK_CORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    link_test_kit::programs_in(&core).join(program)
}

/// A core at `bin/lnk` that runs the sandbox module for `lnk sandbox ...`,
/// the harness module for `lnk agent ...`, and an embedding model for
/// `lnk model serve` (until its stdin closes), and refuses anything else,
/// for `LNK_EXE`; and this folder's `lnk-memory-tool` in `bin`, where the
/// harness module finds it. As `lnk model serve --port <port>` does, its
/// model listens at the port it's given: it writes the port to `ports`,
/// where [`embeddings_server`] listens on it, and says that address.
fn core_with_sandbox(bin: &Path, ports: &Path) -> PathBuf {
    let lnk = bin.join("lnk");
    link_test_kit::write_core(
        &lnk,
        format!(
            "#!/bin/sh\n\
             case \"$1 $2\" in\n\
             'sandbox '*) shift; exec '{}' \"$@\" ;;\n\
             'agent '*) shift; LNK_EXE=\"$0\" exec '{}' \"$@\" ;;\n\
             'model serve') while [ $# -gt 0 ]; do [ \"$1\" = --port ] && port=$2; shift; done\n\
               echo \"$port\" >'{}'\n\
               echo \"{{\\\"url\\\":\\\"http://127.0.0.1:$port\\\",\\\"description\\\":\\\"fake\\\"}}\"; exec cat >/dev/null ;;\n\
             esac\n\
             echo \"fake lnk: $*\" >&2\nexit 1\n",
            core_program("lnk-sandbox").display(),
            core_program("lnk-agent").display(),
            ports.display(),
        ),
    )
    .unwrap();
    link_test_kit::copy_program(
        Path::new(env!("CARGO_BIN_EXE_lnk-memory-tool")),
        &bin.join("lnk-memory-tool"),
    )
    .unwrap();
    lnk
}

/// Whether bubblewrap and curl run here: their gates, which say so where
/// they don't, and fail the test under `LNK_REQUIRE_SANDBOX` or
/// `LNK_REQUIRE_PROGRAMS`.
fn can_run() -> bool {
    link_test_kit::gate::sandbox_with_landlock()
        && link_test_kit::gate::program("curl", &["--version"])
}

/// An MCP client in the sandbox: a session, then a search, again until
/// it's by meaning too (the model embeds in the background).
const CLIENT: &str = r#"
u=${LNK_MEMORY:?no LNK_MEMORY}
post() { curl -s --noproxy '*' -D - -H 'Content-Type: application/json' "$@" "$u"; }
id=$(post -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' | tr -d '\r' | sed -n 's/^Mcp-Session-Id: //p')
rpc() { post -H "Mcp-Session-Id: $id" -d "$1" | tail -n 1; }
for _ in $(seq 60); do
  reply=$(rpc '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search","arguments":{"query":"tomatoes"}}}')
  case "$reply" in *'"matched":"both"'*) break ;; esac
  sleep 0.5
done
printf 'SEARCH %s\n' "$reply"
printf 'REMEMBER %s\n' "$(rpc '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"remember","arguments":{"note":"Tomatoes go in in May."},"_meta":{"link.local/thread":"telegram:42"}}}')"
"#;

/// An OpenAI-style embeddings endpoint on this machine, the same vector
/// for every text, at each port written to the pipe made at `ports`, a
/// line each: where `lnk model serve` was told to listen
/// ([`core_with_sandbox`]).
fn embeddings_server(ports: &Path) {
    let path = std::ffi::CString::new(ports.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: a path this test owns, made a named pipe.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    // Held open to write too, so each writer's line is read as it comes
    // and the pipe never reads as ended between them.
    let pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(ports)
        .unwrap();
    std::thread::spawn(move || {
        for port in BufReader::new(pipe).lines() {
            let port: u16 = port.unwrap().trim().parse().unwrap();
            let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
            std::thread::spawn(move || embed_at(listener));
        }
    });
}

/// Answers each embeddings request `listener` takes, the same vector for
/// every text.
fn embed_at(listener: TcpListener) {
    for stream in listener.incoming().flatten() {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = n.trim().parse().unwrap();
            }
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            continue;
        }
        let asked: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let data: Vec<serde_json::Value> = (0..asked["input"].as_array().unwrap().len())
            .map(|i| serde_json::json!({"index": i, "embedding": [1.0, 0.0]}))
            .collect();
        let reply = serde_json::json!({ "data": data }).to_string();
        let mut stream = stream;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
    }
}

/// The test's own runtime folder under `home`, where the service's
/// bridges are, never this user's.
fn runtime(home: &Path) -> PathBuf {
    let runtime = link_test_kit::env::runtime(home);
    link_files::create_private_dir(&runtime).unwrap();
    runtime
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn lnk(home: &Path, bin: &Path, core: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(core_program("lnk-agent"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(home))
        .env("XDG_RUNTIME_DIR", runtime(home))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", core)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), text)
}

/// A session with an owner's memory service, as the proxy opens one for
/// a harness: `lnk agent memory connect`, MCP on its stdio.
struct Session {
    child: std::process::Child,
    output: BufReader<std::process::ChildStdout>,
    id: u64,
}

impl Session {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn open(home: &Path, bin: &Path, core: &Path, service: &Path, agent: &str) -> Self {
        let mut child = Command::new(core_program("lnk-agent"))
            .args(["memory", "connect"])
            .arg(service)
            .args(["--agent-id", agent])
            .env_clear()
            .env("HOME", home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(home))
            .env("XDG_RUNTIME_DIR", runtime(home))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("LNK_EXE", core)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut session = Self {
            child,
            output,
            id: 0,
        };
        session.ask(serde_json::json!({"jsonrpc": "2.0", "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "t", "version": "1"}}}));
        session
    }

    fn ask(&mut self, mut message: serde_json::Value) -> String {
        self.id += 1;
        message["id"] = self.id.into();
        let input = self.child.stdin.as_mut().unwrap();
        writeln!(input, "{message}").unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        line
    }

    /// A search's reply once the index's first walk has finished: a
    /// call never waits for it, and says so while it hasn't.
    fn search(&mut self, query: &str) -> String {
        link_test_kit::wait::eventually(
            "the first walk",
            std::time::Duration::from_secs(10),
            || {
                let reply = self.ask(serde_json::json!({"jsonrpc": "2.0", "method": "tools/call",
                    "params": {"name": "search", "arguments": {"query": query}}}));
                (!reply.contains("walking")).then_some(reply)
            },
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

/// Each process whose command line is an engine's `service`, working
/// in `root`.
fn engine_services(root: &Path) -> Vec<u32> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        let line = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let args: Vec<&[u8]> = line.split(|b| *b == 0).collect();
        if args
            .first()
            .is_some_and(|a| a.ends_with(b"lnk-memory-tool"))
            && args.get(1) == Some(&&b"serve-many"[..])
            && std::fs::read_link(entry.path().join("cwd")).is_ok_and(|cwd| cwd.starts_with(root))
        {
            pids.push(pid);
        }
    }
    pids
}

/// The `lnk-sandbox run ...` command line `pid` runs in: its
/// nearest forebear's that is one.
fn sandbox_of(pid: u32) -> Vec<String> {
    let mut pid = pid;
    loop {
        let line = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let args: Vec<String> = line
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        if args.get(1).is_some_and(|word| word == "run") {
            return args;
        }
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let after = &stat[stat.rfind(')').unwrap() + 2..];
        pid = after.split(' ').nth(1).unwrap().parse().unwrap();
        assert!(pid > 1, "no sandbox above the engine");
    }
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn an_agent_searches_its_memory_through_its_proxy() {
    if !can_run() {
        return;
    }
    let root = link_test_kit::TempDir::new("agent");
    let (home, bin) = (root.join("home"), root.join("bin"));
    let files = home.join("Link/Agents/main/home");
    std::fs::create_dir_all(home.join(".lnk/safe/agents")).unwrap();
    std::fs::create_dir_all(files.join("memory/notes")).unwrap();
    // Its conversations in its own folder, beside Home.
    let own = home.join("Link/Agents/main/agent");
    std::fs::create_dir_all(own.join("chats/telegram-42")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    // Its key in your secrets, as Link keeps an agent's keys.
    link_agent_contract::testing::write_agent(
        &home,
        "main",
        &format!(
            "id = \"agent:m4i9n2x7k1ab\"\nharness = \"fake\"\nfiles = \"{}\"\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-x\"\n",
            files.display()
        ),
    );
    std::fs::write(
        files.join("memory/notes/garden.md"),
        "# The garden\n\nPlant the tomatoes in May.\n",
    )
    .unwrap();
    std::fs::write(
        own.join("chats/telegram-42/conversation.md"),
        "# telegram:42\n\n## You, 2026-09-29T14:03:07Z\n\nWhen do the tomatoes go in?\n",
    )
    .unwrap();
    let ports = root.join("ports");
    embeddings_server(&ports);
    let core = core_with_sandbox(&bin, &ports);

    let (ok, text) = lnk(&home, &bin, &core, &["memory", "start"]);
    assert!(ok, "{text}");
    assert!(text.contains("Link Memory"), "{text}");
    let settings = std::fs::read_to_string(home.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(settings.contains("memory = \"link\""), "{settings}");
    // Its tool is named by the agent's ID, which no other owner's agent
    // shares.
    let tool_file = home.join(".lnk/safe/sandboxes/tools/memory-m4i9n2x7k1ab.json");
    // Its tool is a session with its owner's memory service, the agent
    // named by Link: the engine runs once for all of the owner's agents.
    let tool: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&tool_file).unwrap()).unwrap();
    let argv: Vec<String> = tool["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    assert_eq!(argv[1..4], ["agent", "memory", "connect"], "{argv:?}");
    assert_eq!(argv[5], "--agent-id", "{argv:?}");
    assert_eq!(argv[6], "m4i9n2x7k1ab", "{argv:?}");
    let service = PathBuf::from(&argv[4]);
    assert!(
        service.starts_with(home.join(".lnk/data/agents/memory")),
        "{argv:?}"
    );
    // What it serves the agent with: its files read, its state and notes
    // written, its model served outside.
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(service.join("agents/m4i9n2x7k1ab.json")).unwrap())
            .unwrap();
    // Its state and its notes in its own folder, beside Home; of that
    // folder it reads its conversations and skills alone.
    let state = own.join("memory/link");
    // Its notes beside its memory's state: `memory/notes.md`.
    let notes = own.join("memory");
    assert_eq!(Path::new(record["folder"].as_str().unwrap()), files);
    assert_eq!(Path::new(record["state"].as_str().unwrap()), state);
    assert_eq!(Path::new(record["notes"].as_str().unwrap()), notes);
    assert_eq!(
        record["reads"],
        serde_json::json!([own.join("chats"), own.join("skills")])
    );
    // Link Document Converter, which it runs, read where it's installed: beside
    // the workspace's programs.
    let documents = link_test_kit::programs().join("lnk-document-converter");
    assert_eq!(
        record["programs"],
        serde_json::json!([documents.canonicalize().unwrap()])
    );
    assert_eq!(record["model"], "qwen3-embedding");
    assert!(record.to_string().contains("--notes"), "{record}");
    // Its notes written unasked, as its reads are.
    assert!(tool.to_string().contains(r#""remember""#), "{tool}");

    // A second agent of the same owner, with a file of its own.
    let beans = home.join("Link/Agents/other/home");
    std::fs::create_dir_all(beans.join("memory/notes")).unwrap();
    link_agent_contract::testing::write_agent(
        &home,
        "other",
        &format!(
            "id = \"agent:07t3h5e1r2cd\"\nharness = \"fake\"\nfiles = \"{}\"\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-y\"\n",
            beans.display()
        ),
    );
    std::fs::write(
        beans.join("memory/notes/beans.md"),
        "# Beans\n\nSow the beans in April.\n",
    )
    .unwrap();
    let (ok, text) = lnk(&home, &bin, &core, &["-a", "other", "memory", "start"]);
    assert!(ok, "{text}");
    assert!(service.join("agents/07t3h5e1r2cd.json").is_file(), "{text}");

    // One session each, outside any sandbox, as the proxy opens them: one
    // service serves both, each from its own files alone.
    let mut main_session = Session::open(&home, &bin, &core, &service, "m4i9n2x7k1ab");
    let mut other_session = Session::open(&home, &bin, &core, &service, "07t3h5e1r2cd");
    let (mine, theirs) = (
        main_session.search("tomatoes"),
        main_session.search("beans"),
    );
    assert!(
        mine.contains("garden.md") && !theirs.contains("beans.md"),
        "{mine} {theirs}"
    );
    let (mine, theirs) = (
        other_session.search("beans"),
        other_session.search("tomatoes"),
    );
    assert!(
        mine.contains("beans.md") && !theirs.contains("garden.md"),
        "{mine} {theirs}"
    );
    let services = engine_services(&root);
    assert_eq!(services.len(), 1, "{services:?}");
    // In its sandbox, each agent's notes are written, their
    // conversations and the rest of their files never; it reads its own
    // program, not the folder it's in, with `lnk` and every module.
    let sandboxed = sandbox_of(services[0]);
    let at = |flag: &str, path: &Path| {
        sandboxed
            .windows(2)
            .any(|w| w[0] == flag && Path::new(&w[1]) == path)
    };
    assert!(
        at("--files.write", &state) && at("--files.read", &files) && at("--files.read", &beans),
        "{sandboxed:?}"
    );
    assert!(at("--files.write", &notes), "{sandboxed:?}");
    // Each agent's way to its storage bridge, a folder of its owner's in
    // the runtime folder, which every agent's sandbox hides: where its
    // machine keeps what memory reads off its folder. The service is told
    // that folder, and takes only the sockets in it.
    let bridges = link_files::layout::MemoryBridges::in_runtime(
        &link_files::layout::Root::Runtime.default_in(&runtime(&home)),
        service.file_name().unwrap().to_str().unwrap(),
    );
    let folder_of = |agent: &str| bridges.socket(agent).parent().unwrap().to_path_buf();
    assert!(
        at("--files.read", &folder_of("m4i9n2x7k1ab"))
            && at("--files.read", &folder_of("07t3h5e1r2cd")),
        "{sandboxed:?}"
    );
    assert!(
        sandboxed
            .windows(2)
            .any(|w| w[0] == "--bridges" && Path::new(&w[1]) == bridges.folder()),
        "{sandboxed:?}"
    );
    assert!(
        !at("--files.write", &files) && !at("--files.write", &own.join("chats")),
        "{sandboxed:?}"
    );
    // It writes its working folder alone of the service's: never the
    // records of what it serves, what it's handed of them, or its log.
    assert!(
        at("--files.write", &service.join("run")) && !at("--files.write", &service),
        "{sandboxed:?}"
    );
    // Whichever `lnk-memory-tool` the harness module found: beside it,
    // or this test's in `bin`.
    let program = sandboxed
        .windows(2)
        .find(|w| w[0] == "--files.read" && w[1].ends_with("/lnk-memory-tool"))
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| panic!("it reads no lnk-memory-tool: {sandboxed:?}"));
    assert!(
        !at("--files.read", program.parent().unwrap()),
        "{sandboxed:?}"
    );
    // It reaches its embedding model, served on this machine, at that
    // model's port alone on Linux, never the machine's whole network.
    assert!(
        sandboxed
            .windows(2)
            .any(|w| w[0] == "--localhost.linux.models" && w[1].parse::<u16>().is_ok()),
        "{sandboxed:?}"
    );
    assert!(
        !sandboxed
            .windows(2)
            .any(|w| w == ["--network.mode", "host"]),
        "{sandboxed:?}"
    );
    let ends = sandboxed.iter().position(|a| a == "--").unwrap();
    let probe = format!(
        "echo x > '{3}/memory/probe' && ! (echo x > '{3}/chats/telegram-42/probe') \
         && ! (echo x > '{0}/notes/probe') && ! (echo x > '{1}/notes/probe') \
         && ! (echo x > '{2}/agents/m4i9n2x7k1ab.json') \
         && (echo PLANTED >> '{2}/service.log' || true) && echo PROBED",
        files.display(),
        beans.display(),
        service.display(),
        own.display()
    );
    let probed = Command::new(core_program("lnk-sandbox"))
        .args(&sandboxed[1..ends])
        .args(["--commands.toolchains", "shell", "--", "sh", "-c", &probe])
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", runtime(&home))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&probed.stdout).to_string()
        + &String::from_utf8_lossy(&probed.stderr);
    assert!(said.contains("PROBED"), "{said}");
    assert!(!own.join("chats/telegram-42/probe").exists());
    let log = std::fs::read_to_string(service.join("service.log")).unwrap_or_default();
    assert!(!log.contains("PLANTED"), "{log}");
    std::fs::remove_file(notes.join("probe")).unwrap();
    // An agent's sandbox never sees the service, its socket or what it
    // serves: only Link names an agent to it.
    let look = format!(
        "for p in '{0}' '{0}/agents' '{1}'; do test -e \"$p\" && echo \"SEEN $p\"; done; echo LOOKED",
        service.display(),
        service.join("agents/07t3h5e1r2cd.json").display()
    );
    let (ok, text) = lnk(
        &home,
        &bin,
        &core,
        &[
            "-a",
            "main",
            "harness",
            "run",
            "--harness",
            "fake",
            "--",
            "sh",
            "-c",
            &look,
        ],
    );
    assert!(
        ok && text.contains("LOOKED") && !text.contains("SEEN"),
        "{text}"
    );
    // With no session open, nothing of it runs.
    drop((main_session, other_session));
    let started = std::time::Instant::now();
    while !engine_services(&root).is_empty() {
        assert!(
            started.elapsed().as_secs() < 30,
            "memory's service kept running"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Its client polls: the programs it runs beyond the defaults, granted.
    for program in ["seq", "sleep"] {
        let (ok, text) = lnk(
            &home,
            &bin,
            &core,
            &[
                "-a",
                "main",
                "permission",
                "add",
                "--commands.allow",
                program,
            ],
        );
        assert!(ok, "{text}");
    }

    // Defined as it would be again: left as it is.
    let modified = || std::fs::metadata(&tool_file).unwrap().modified().unwrap();
    let before = modified();
    let (ok, text) = lnk(&home, &bin, &core, &["-a", "main", "memory", "start"]);
    assert!(ok, "{text}");
    assert_eq!(modified(), before);

    let (ok, text) = lnk(
        &home,
        &bin,
        &core,
        &[
            "-a",
            "main",
            "harness",
            "run",
            "--harness",
            "fake",
            "--",
            "sh",
            "-c",
            CLIENT,
        ],
    );
    assert!(ok, "{text}");
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix("SEARCH "))
        .unwrap_or_else(|| panic!("{text}"));
    let search: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|_| panic!("{text}"));
    let mut kinds: Vec<&str> = search["result"]["structuredContent"]["results"]
        .as_array()
        .unwrap_or_else(|| panic!("{text}"))
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    kinds.sort();
    assert_eq!(kinds, ["conversation", "file"], "{text}");
    // Found by meaning too: the sandbox reached the model served for it.
    assert!(line.contains(r#""matched":"both""#), "{text}");
    // A note, unasked: the agent's notes hold it.
    let remembered = text
        .lines()
        .find_map(|l| l.strip_prefix("REMEMBER "))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(remembered.contains(r#""done":"added""#), "{text}");
    // Saying the conversation it was saved in.
    assert_eq!(
        std::fs::read_to_string(notes.join("notes.md")).unwrap(),
        "- Tomatoes go in in May. (from telegram:42)\n"
    );

    // Its instructions locked (its sandbox file's `[files]`): defined
    // again, with no notes and nothing of its files written.
    std::fs::create_dir_all(home.join(".lnk/safe/agents/main")).unwrap();
    std::fs::write(
        home.join("Link/Safe/Agents/main/sandbox.toml"),
        "schema = 1\n[files]\nwrite = [\"work\"]\nread = [\"conversations\", \"instructions\"]\n",
    )
    .unwrap();
    let (ok, text) = lnk(&home, &bin, &core, &["-a", "main", "memory", "start"]);
    assert!(ok, "{text}");
    let tool = std::fs::read_to_string(&tool_file).unwrap();
    assert!(!tool.contains("remember"), "{tool}");
    let record = std::fs::read_to_string(service.join("agents/m4i9n2x7k1ab.json")).unwrap();
    assert!(!record.contains("--notes"), "{record}");
    let kept: serde_json::Value = serde_json::from_str(&record).unwrap();
    assert!(kept["notes"].is_null(), "{record}");
    // Its index is kept in the agent's own folder, outside Home.
    assert!(own.join("memory/link/index.sqlite").is_file(), "{text}");

    let (ok, text) = lnk(&home, &bin, &core, &["-a", "main", "memory", "stop"]);
    assert!(ok, "{text}");
    // Its owner's service serves it no more.
    assert!(!service.join("agents/m4i9n2x7k1ab.json").exists());
    let (ok, text) = lnk(
        &home,
        &bin,
        &core,
        &[
            "-a",
            "main",
            "harness",
            "run",
            "--harness",
            "fake",
            "--",
            "sh",
            "-c",
            "echo \"MEMORY ${LNK_MEMORY:-none}\"",
        ],
    );
    assert!(ok && text.contains("MEMORY none"), "{text}");
}
