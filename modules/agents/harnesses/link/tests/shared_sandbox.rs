//! One Link Harness for two agents of one owner, for real, on Linux:
//! with `agents.toml`'s `shared_harness` on, `lnk agent start
//! --foreground` (built in `link-core` beside this folder) runs each
//! agent's proxy alone and one sandbox for both, reaching each proxy,
//! with `lnk-harness serve --agents` in it. Each agent answers from
//! its own conversation, through its own proxy, to a local stand-in
//! model; one stopping leaves the other answering, from the same kind of
//! sandbox started again without it.
//!
//! It needs bubblewrap and `link-core` beside this folder (`LNK_CORE` for
//! another place). Without bubblewrap it skips through the kit's gate,
//! saying why, and fails instead under `LNK_REQUIRE_SANDBOX`.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use axum::{Json, Router, routing::post};
use serde_json::{Value, json};

fn link_core() -> PathBuf {
    std::env::var_os("LNK_CORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.."))
}

fn core(program: &str) -> PathBuf {
    link_test_kit::programs_in(&link_core()).join(program)
}

/// Whether bubblewrap runs here, through the kit's gate. The core's
/// programs come first (`cargo build --workspace --bins`), so one not
/// built fails the test.
fn can_run() -> bool {
    for program in ["lnk-agent", "lnk-connection-fixture"] {
        link_test_kit::programs::require(&core(program), link_test_kit::programs::BUILD);
    }
    link_test_kit::gate::sandbox_with_landlock()
}

/// A process group, killed whole when dropped.
struct Group(std::process::Child);

impl Drop for Group {
    fn drop(&mut self) {
        link_test_kit::kill_group(&mut self.0);
    }
}

fn spawn_group(command: &mut Command) -> Group {
    use std::os::unix::process::CommandExt;
    Group(command.process_group(0).spawn().unwrap())
}

struct Removed(PathBuf);

impl Drop for Removed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A local model on this machine, OpenAI's chat completions at any path:
/// it answers each owner's message with "heard: <it>".
fn stand_in() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            listener.set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let chat = post(|Json(body): Json<Value>| async move {
                let said = body["messages"]
                    .as_array()
                    .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
                    .and_then(|m| m["content"].as_str())
                    .unwrap_or_default()
                    .to_string();
                let data = json!({"id": "s", "choices": [{"delta": {"content": format!("heard: {said}")}}]});
                axum::response::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from(format!("data: {data}\n\ndata: [DONE]\n\n")))
                    .unwrap()
            });
            let app = Router::new().fallback_service(chat);
            axum::serve(listener, app).await.unwrap();
        });
    });
    port
}

/// Each process running `program` (its file's name) with `words` in a
/// row among its arguments, and a path under `inside` among them: this
/// test's, never a sandbox's own process whose arguments name it.
fn processes(program: &str, words: &[&str], inside: &Path) -> Vec<u32> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|p| p.parse().ok()) else {
            continue;
        };
        let line = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let args: Vec<String> = line
            .split(|b| *b == 0)
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        let named = args
            .first()
            .is_some_and(|a| a.ends_with(&format!("/{program}")));
        let said = args.windows(words.len()).any(|w| w == words);
        let ours = args
            .iter()
            .any(|a| a.contains(&inside.display().to_string()));
        if named && said && ours {
            pids.push(pid);
        }
    }
    pids
}

/// What agent `name`'s bridge would send to: its endpoint, its key, and
/// whether its harness proves it's the agent's.
fn plan(home: &Path, name: &str) -> (String, String, bool) {
    let plan: Value = serde_json::from_slice(
        &std::fs::read(home.join(format!(".lnk/safe/agents/{name}/bridge.json"))).unwrap(),
    )
    .unwrap();
    (
        plan["endpoint"].as_str().unwrap().to_string(),
        plan["key"].as_str().unwrap().to_string(),
        plan["proof"] == true,
    )
}

/// Asks agent `name` at its endpoint over ACP, as its bridge would: 200
/// and what it said, or 0 and why not.
fn ask(home: &Path, name: &str, text: &str) -> (u16, String) {
    let (endpoint, key, proof) = plan(home, name);
    let bridge = link_connection_contract::Bridge {
        agent: name.into(),
        endpoint,
        key: key.into(),
        proof,
        settings: Value::Null.into(),
        ..Default::default()
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (progress, _) = tokio::sync::watch::channel(String::new());
        let asked = tokio::time::timeout(
            Duration::from_secs(30),
            link_connection_contract::ask(&bridge, "telegram:42", text, &progress),
        )
        .await;
        match asked {
            Ok(Ok(said)) => (200, said),
            Ok(Err(err)) => (0, format!("{err:#}")),
            Err(_) => (0, "no answer in 30s".into()),
        }
    })
}

/// Waits for `done`, saying what the agents said if it never holds.
fn wait_for(what: &str, limit: Duration, done: impl FnMut() -> bool, said: impl Fn() -> String) {
    if let Err(timed_out) = link_test_kit::within(what, limit, done) {
        panic!("{timed_out}: {}", said());
    }
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn two_agents_share_one_harness_each_behind_its_own_proxy() {
    if !can_run() {
        return;
    }
    link_fetch::install_crypto_provider();
    let root = std::env::temp_dir().join(format!("lnk-sh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _removed = Removed(root.clone());
    let home = root.join("home");
    let config = home.join(".lnk");
    std::fs::create_dir_all(link_files::layout::Hidden::Agents.at(&config)).unwrap();
    link_files::write_private(
        &link_files::layout::Hidden::AgentPolicy.at(&config),
        b"schema = 1\nshared_harness = true\n",
    )
    .unwrap();
    let model = stand_in();
    // Each agent's ports, held until the test ends.
    let mut blocks = Vec::new();
    for (name, id) in [("ada", "4da000000001"), ("bea", "bea000000002")] {
        let block = crate::common::free_block();
        let files = home.join(format!("Link/Agents/{name}/home"));
        std::fs::create_dir_all(&files).unwrap();
        link_agent_contract::testing::write_agent(
            &home,
            name,
            &format!(
                "id = \"agent:{id}\"\nharness = \"link\"\nfiles = \"{}\"\nport = {}\nblock = {}\n\
                 [model]\nprovider = \"local\"\nname = \"stand-in\"\n\
                 base_url = \"http://127.0.0.1:{model}/v1\"\napi_key = \"\"\n",
                files.display(),
                block.number(),
                crate::common::BLOCK
            ),
        );
        blocks.push(block);
    }
    let harness_module = core("lnk-agent");
    let bin = harness_module.parent().unwrap().to_path_buf();
    let adapter = PathBuf::from(env!("CARGO_BIN_EXE_lnk-harness"));
    let path = format!(
        "{}:{}:/usr/bin:/bin",
        bin.display(),
        adapter.parent().unwrap().display()
    );
    let start = |name: &str| {
        let log = root.join(format!("{name}.log"));
        spawn_group(
            Command::new(&harness_module)
                .args(["start", "--foreground", "--agent", name])
                .env_clear()
                .env("HOME", &home)
                .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
                .env("PATH", &path)
                .env("LNK_EXE", core("lnk"))
                .envs(link_test_kit::keychain::isolated(&home))
                .stdout(std::fs::File::create(&log).unwrap())
                .stderr(Stdio::from(
                    std::fs::File::create(root.join(format!("{name}.err"))).unwrap(),
                )),
        )
    };
    let read = |name: &str| {
        std::fs::read_to_string(root.join(format!("{name}.log"))).unwrap_or_default()
            + &std::fs::read_to_string(root.join(format!("{name}.err"))).unwrap_or_default()
    };
    // What every agent and the shared harness said.
    let all = || {
        let mut shared = String::new();
        for dir in std::fs::read_dir(home.join(".lnk/data/agents/shared-harnesses"))
            .into_iter()
            .flatten()
            .flatten()
        {
            shared += &std::fs::read_to_string(dir.path().join("shared.log")).unwrap_or_default();
        }
        format!(
            "ada:\n{}\nbea:\n{}\nshared:\n{shared}",
            read("ada"),
            read("bea")
        )
    };
    let ada = start("ada");
    wait_for(
        "ada answering",
        Duration::from_secs(120),
        || {
            root.join("home/.lnk/safe/agents/ada/bridge.json").is_file()
                && ask(&home, "ada", "hello ada").0 == 200
        },
        all,
    );
    let bea = start("bea");
    wait_for(
        "bea answering",
        Duration::from_secs(120),
        || {
            root.join("home/.lnk/safe/agents/bea/bridge.json").is_file()
                && ask(&home, "bea", "hello bea").0 == 200
        },
        all,
    );

    // One harness, in one sandbox, for both; each agent's own proxy runs
    // alone.
    let shared = processes("lnk-harness", &["serve", "--agents"], &home);
    assert_eq!(shared.len(), 1, "{}{}", read("ada"), read("bea"));
    assert_eq!(processes("lnk-sandbox", &["--hold"], &home).len(), 2);
    // Each answers from its own conversation, through its own proxy.
    // (The harness says when and where before the message.)
    let (status, said) = ask(&home, "ada", "Ada's secret");
    assert!(
        status == 200 && said.ends_with("\nAda's secret"),
        "{status} {said}: {}",
        read("ada")
    );
    let (status, said) = ask(&home, "bea", "Bea's secret");
    assert!(
        status == 200 && said.ends_with("\nBea's secret"),
        "{status} {said}: {}",
        read("bea")
    );
    let conversation = |name: &str| {
        let dir = home.join(format!("Link/Agents/{name}/agent/chats/telegram-42"));
        std::fs::read_to_string(dir.join("conversation.md")).unwrap_or_default()
    };
    assert!(conversation("ada").contains("Ada's secret") && !conversation("ada").contains("Bea's"));
    assert!(conversation("bea").contains("Bea's secret") && !conversation("bea").contains("Ada's"));
    // Each one's model calls went through its own proxy, on its own wire.
    let wire = Command::new(core("lnk-sandbox"))
        .args(["log", "show", "--json", "-n", "200"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
        .output()
        .unwrap();
    let wire = String::from_utf8_lossy(&wire.stdout).to_string();
    for name in ["ada-link", "bea-link"] {
        assert!(wire.contains(name), "no call on {name}'s wire: {wire}");
    }

    // Ada stops: Bea goes on, from the harness started again without Ada.
    drop(ada);
    wait_for(
        "the harness without ada",
        Duration::from_secs(60),
        || {
            let pids = processes("lnk-harness", &["serve", "--agents"], &home);
            pids.len() == 1 && pids != shared
        },
        all,
    );
    let mut said = String::new();
    wait_for(
        "bea answering again",
        Duration::from_secs(30),
        || {
            let (status, text) = ask(&home, "bea", "Still there?");
            said = text;
            status == 200
        },
        all,
    );
    assert!(said.ends_with("\nStill there?"), "{said}: {}", read("bea"));
    assert_ne!(ask(&home, "ada", "Hello?").0, 200);

    // Bea stops: nothing of the shared harness is left.
    drop(bea);
    wait_for(
        "no shared harness",
        Duration::from_secs(60),
        || {
            processes("lnk-harness", &["serve", "--agents"], &home).is_empty()
                && processes("lnk-agent", &["shared"], &home).is_empty()
        },
        all,
    );
}
