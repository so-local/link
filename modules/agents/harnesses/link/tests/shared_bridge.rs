//! One bridge for two agents on Link Harness, for real, on Linux: with
//! `agents.toml`'s `shared_bridge` (and `shared_harness`) on, `lnk agent
//! start --foreground` (built in `link-core` beside this folder) runs one
//! `lnk agent bridges` carrying both agents' bots on the connection fixture
//! (`lnk-connection-fixture`, the core's fixture), against its fake service. Each owner is answered on their own bot; the bridge
//! killed while one agent's turn is with the model is started again, and
//! that turn's answer, finished meanwhile, reaches its owner once,
//! recovered, with the model asked once.
//!
//! It needs bubblewrap and `link-core` beside this folder (`LNK_CORE` for
//! another place). Without bubblewrap it skips through the kit's gate,
//! saying why, and fails instead under `LNK_REQUIRE_SANDBOX`.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{Json, Router, routing::post};
use link_harness_conformance::fixture_connection::{self, ConnectionFixture};
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

struct Removed(PathBuf);

impl Drop for Removed {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn drop(&mut self) {
        // The bridge is no agent's child: ended by its folder.
        let _ = Command::new("pkill")
            .args(["-KILL", "-f"])
            .arg(self.0.display().to_string())
            .status();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Shared<T> = Arc<Mutex<T>>;

/// Each agent, its ID, its bot's number on the connection fixture and its
/// owner there.
const AGENTS: [(&str, &str, u32, &str); 2] = [
    ("ada", "4da000000001", 1, "42"),
    ("bea", "bea000000002", 2, "43"),
];

/// The words the stand-in takes its time over: long enough to kill the
/// bridge while the turn is with it.
const SLOW: &str = "answer slowly";

/// The words the stand-in holds its answer to until the test lets it go:
/// long enough for another agent to join while the turn is with it.
const HELD: &str = "answer when let go";

/// A local model on this machine, OpenAI's chat completions at any path:
/// it answers each owner's message with "heard: <it>", after a while for
/// [`SLOW`], and once `holding` is false for [`HELD`]; what each message
/// was, as it was asked.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn stand_in(asked: Shared<Vec<String>>, holding: Arc<AtomicBool>) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            listener.set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let chat = post(move |Json(body): Json<Value>| {
                let (asked, holding) = (asked.clone(), holding.clone());
                async move {
                    let said = body["messages"]
                        .as_array()
                        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
                        .and_then(|m| m["content"].as_str())
                        .unwrap_or_default()
                        .to_string();
                    let said = said.lines().last().unwrap_or_default().to_string();
                    asked.lock().unwrap().push(said.clone());
                    if said == SLOW {
                        tokio::time::sleep(Duration::from_secs(6)).await;
                    }
                    let held = Instant::now();
                    while said == HELD
                        && holding.load(Ordering::SeqCst)
                        && held.elapsed() < Duration::from_secs(120)
                    {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    let data = json!({"id": "s", "choices": [{"delta": {"content": format!("heard: {said}")}}]});
                    axum::response::Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(axum::body::Body::from(format!(
                            "data: {data}\n\ndata: [DONE]\n\n"
                        )))
                        .unwrap()
                }
            });
            let app = Router::new().fallback_service(chat);
            axum::serve(listener, app).await.unwrap();
        });
    });
    port
}

/// Each process running `program` (its file's name) with `words` in a
/// row among its arguments, and a path under `inside` among them.
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

/// Waits for `done`, saying what the agents said if it never holds.
fn wait_for(what: &str, limit: Duration, done: impl FnMut() -> bool, said: impl Fn() -> String) {
    if let Err(timed_out) = link_test_kit::within(what, limit, done) {
        panic!("{timed_out}: {}", said());
    }
}

/// What happens to the two agents, besides each being answered.
#[derive(Clone, Copy, PartialEq)]
enum Case {
    /// Their one bridge is killed while a turn is with the model.
    BridgeKilled,
    /// Both go idle, and one wakes.
    Idle,
    /// The second starts while the first's turn is with the model.
    JoinMidTurn,
}

#[test]
fn one_bridge_carries_two_agents_and_a_turn_cut_off_is_delivered_once() {
    two_agents(Case::BridgeKilled);
}

/// With `[run] idle_after`, both agents asleep hold nothing, nor does
/// their owner's harness; one's owner writing wakes it, and its owner's
/// harness, while the other sleeps on.
#[test]
fn idle_agents_hold_nothing_and_their_owners_harness_ends() {
    two_agents(Case::Idle);
}

/// A second agent starting restarts its owner's shared harness, which
/// lets the first's turn with the model finish first: answered once, its
/// model asked once, never "couldn't answer".
#[test]
fn an_agent_joining_never_cuts_off_another_agents_turn() {
    two_agents(Case::JoinMidTurn);
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn two_agents(case: Case) {
    let idle = case == Case::Idle;
    if !can_run() {
        return;
    }
    link_fetch::install_crypto_provider();
    let root = std::env::temp_dir().join(format!("lnk-sb{}-{}", case as u8, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _removed = Removed(root.clone());
    let home = root.join("home");
    let config = home.join(".lnk");
    std::fs::create_dir_all(link_files::layout::Hidden::Agents.at(&config)).unwrap();
    link_files::write_private(
        &link_files::layout::Hidden::AgentPolicy.at(&config),
        b"schema = 1\nshared_harness = true\nshared_bridge = true\n",
    )
    .unwrap();
    let asked: Shared<Vec<String>> = Arc::default();
    let holding = Arc::new(AtomicBool::new(true));
    let model = stand_in(asked.clone(), holding.clone());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let chat = runtime.block_on(ConnectionFixture::serve());
    // Each agent's ports, held until the test ends.
    let mut blocks = Vec::new();
    for (name, id, bot, owner) in AGENTS {
        let block = crate::common::free_block();
        let files = home.join(format!("Link/Agents/{name}/home"));
        std::fs::create_dir_all(&files).unwrap();
        // Its bot's token in your secrets, as Link keeps an agent's keys.
        link_agent_contract::testing::write_agent(
            &home,
            name,
            &format!(
                "id = \"agent:{id}\"\nharness = \"link\"\nfiles = \"{}\"\nport = {}\nblock = {}\n\
                 [model]\nprovider = \"local\"\nname = \"stand-in\"\n\
                 base_url = \"http://127.0.0.1:{model}/v1\"\napi_key = \"\"\n{}",
                files.display(),
                block.number(),
                crate::common::BLOCK,
                fixture_connection::section(&fixture_connection::token(bot), owner),
            ),
        );
        blocks.push(block);
    }
    if idle {
        for (name, ..) in AGENTS {
            let path = home.join(format!("Link/Safe/Agents/{name}/agent.toml"));
            let settings = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                &path,
                format!("{settings}\n[behavior.run]\nidle_after = \"2s\"\n"),
            )
            .unwrap();
        }
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
        use std::os::unix::process::CommandExt;
        let log = root.join(format!("{name}.log"));
        Group(
            link_test_kit::env::command(&harness_module, &home)
                .args(["start", "--foreground", "--agent", name])
                .env("PATH", &path)
                .env("LNK_EXE", core("lnk"))
                .envs(link_test_kit::keychain::isolated(&home))
                .env(fixture_connection::SERVICE, &chat.url)
                .env("NO_PROXY", "127.0.0.1,localhost")
                .stdout(std::fs::File::create(&log).unwrap())
                .stderr(Stdio::from(
                    std::fs::File::options().append(true).open(&log).unwrap(),
                ))
                .process_group(0)
                .spawn()
                .unwrap(),
        )
    };
    let all = || {
        let mut said = String::new();
        for log in ["ada.log", "bea.log"] {
            said += &std::fs::read_to_string(root.join(log)).unwrap_or_default();
        }
        said += &std::fs::read_to_string(home.join(".lnk/safe/agents/bridges/bridges.log"))
            .unwrap_or_default();
        for dir in std::fs::read_dir(home.join(".lnk/data/agents/shared-harnesses"))
            .into_iter()
            .flatten()
            .flatten()
        {
            said += &std::fs::read_to_string(dir.path().join("shared.log")).unwrap_or_default();
        }
        let sent: Vec<_> = AGENTS
            .map(|(_, _, bot, _)| chat.sent(&fixture_connection::token(bot)))
            .into();
        format!("{said}\nsent: {sent:?}")
    };
    let say = |token: &str, owner: &str, text: &str| chat.message(token, owner, text);
    // How many times the bot whose token is `token` sent `text` whole.
    let sent = |token: &str, owner: &str, text: &str| {
        let sent = chat.sent_to(token, owner).into_iter();
        sent.filter(|said| said.ends_with(text)).count()
    };
    let [(ada, _, ada_bot, ada_owner), (bea, _, bea_bot, bea_owner)] = AGENTS;
    let (ada_token, bea_token) = (
        &fixture_connection::token(ada_bot),
        &fixture_connection::token(bea_bot),
    );
    let _ada = start(ada);
    if case == Case::JoinMidTurn {
        say(ada_token, ada_owner, HELD);
        wait_for(
            "the held turn with the model",
            Duration::from_secs(120),
            || asked.lock().unwrap().iter().any(|a| a == HELD),
            all,
        );
        let _bea = start(bea);
        wait_for(
            "the owner's harness asked to start again",
            Duration::from_secs(120),
            || all().contains("The agents served changed: starting again."),
            all,
        );
        holding.store(false, Ordering::SeqCst);
        let answer = format!("heard: {HELD}");
        wait_for(
            "the held turn's answer",
            Duration::from_secs(60),
            || sent(ada_token, ada_owner, &answer) > 0,
            all,
        );
        say(bea_token, bea_owner, "hello bea");
        wait_for(
            "bea's answer",
            Duration::from_secs(120),
            || sent(bea_token, bea_owner, "heard: hello bea") > 0,
            all,
        );
        // A moment for any second delivery to show.
        std::thread::sleep(Duration::from_secs(3));
        let times = asked.lock().unwrap().iter().filter(|a| *a == HELD).count();
        assert_eq!(times, 1, "the model was asked again: {}", all());
        assert_eq!(sent(ada_token, ada_owner, &answer), 1, "{}", all());
        let cut = chat.sent_to(ada_token, ada_owner);
        assert!(
            !cut.iter().any(|said| said.contains("couldn't answer")),
            "{}",
            all()
        );
        return;
    }
    let _bea = start(bea);

    // Each owner answered on their own bot, by one bridge for both.
    say(ada_token, ada_owner, "hello ada");
    say(bea_token, bea_owner, "hello bea");
    wait_for(
        "ada's answer",
        Duration::from_secs(120),
        || sent(ada_token, ada_owner, "heard: hello ada") > 0,
        all,
    );
    wait_for(
        "bea's answer",
        Duration::from_secs(120),
        || sent(bea_token, bea_owner, "heard: hello bea") > 0,
        all,
    );
    // The second start's bridge, finding the first's, leaves within a
    // moment.
    wait_for(
        "one bridge",
        Duration::from_secs(10),
        || processes("lnk-agent", &["bridges"], &home).len() == 1,
        all,
    );
    let bridges = processes("lnk-agent", &["bridges"], &home);
    assert!(processes("lnk-agent", &["bridge"], &home).is_empty());
    assert_eq!(sent(bea_token, bea_owner, "heard: hello ada"), 0);
    assert_eq!(sent(ada_token, ada_owner, "heard: hello bea"), 0);
    if idle {
        // Its proxy held alone, named by its network folder, by its ID.
        let network = link_files::layout::Runtime::AgentNetwork.relative();
        let held = |id: &str| {
            processes("lnk-sandbox", &["--hold"], &home)
                .iter()
                .any(|pid| {
                    std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| {
                        String::from_utf8_lossy(&c).contains(&format!("{network}/{id}/"))
                    })
                })
        };
        let ids: Vec<&str> = AGENTS.iter().map(|(_, id, ..)| *id).collect();
        let asleep = |id: &str| {
            home.join(format!(".lnk/safe/agents/bridges/asleep/{id}"))
                .exists()
        };
        wait_for(
            "both asleep, and their owner's harness ended",
            Duration::from_secs(60),
            || {
                ids.iter().all(|id| asleep(id))
                    && !held(ids[0])
                    && !held(ids[1])
                    && processes("lnk-harness", &["serve", "--agents"], &home).is_empty()
            },
            all,
        );
        say(ada_token, ada_owner, "awake?");
        wait_for(
            "ada woken",
            Duration::from_secs(60),
            || sent(ada_token, ada_owner, "heard: awake?") > 0,
            all,
        );
        assert!(held(ids[0]) && !held(ids[1]), "{}", all());
        assert!(asleep(ids[1]) && !asleep(ids[0]));
        assert_eq!(
            processes("lnk-harness", &["serve", "--agents"], &home).len(),
            1
        );
        let conversation = std::fs::read_to_string(
            home.join("Link/Agents/ada/agent/chats/connection-fixture-42/conversation.md"),
        )
        .unwrap_or_default();
        assert!(conversation.contains("hello ada"), "{conversation}");
        return;
    }

    // The bridge killed while Ada's turn is with the model: started again,
    // it delivers that turn's answer, which Ada's harness finished
    // meanwhile, once.
    say(ada_token, ada_owner, SLOW);
    wait_for(
        "the slow turn with the model",
        Duration::from_secs(60),
        || asked.lock().unwrap().iter().any(|a| a == SLOW),
        all,
    );
    // SIGKILL one bridge only; its already-running turn must survive.
    let pid = libc::pid_t::try_from(bridges[0]).unwrap();
    assert!(pid > 1);
    // SAFETY: a positive PID names the bridge found above, never a group.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    wait_for(
        "the bridge started again",
        Duration::from_secs(30),
        || {
            let now = processes("lnk-agent", &["bridges"], &home);
            now.len() == 1 && now != bridges
        },
        all,
    );
    let answer = format!("heard: {SLOW}");
    wait_for(
        "the slow turn's answer",
        Duration::from_secs(90),
        || sent(ada_token, ada_owner, &answer) > 0,
        all,
    );
    // A moment for any second delivery to show.
    std::thread::sleep(Duration::from_secs(3));
    let times = asked.lock().unwrap().iter().filter(|a| *a == SLOW).count();
    assert_eq!(times, 1, "the model was asked again: {}", all());
    assert_eq!(sent(ada_token, ada_owner, &answer), 1, "{}", all());
    assert_eq!(sent(bea_token, bea_owner, &answer), 0);

    // Bea, after it all.
    say(bea_token, bea_owner, "still there");
    wait_for(
        "bea's answer after",
        Duration::from_secs(60),
        || sent(bea_token, bea_owner, "heard: still there") > 0,
        all,
    );
}
