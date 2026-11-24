//! An agent on the mini harness (`lnk-mini-harness`, a harness that speaks no
//! connection), started in the foreground against a bare model, for the
//! tests of a connection's module against its fake: the core's end-to-end
//! tests, and each connection's from its own folder in the public
//! workspace. And what such a test needs beside it: ports of its own,
//! servers on 127.0.0.1, the agent's binding, and whether bubblewrap runs.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{Json, Router, extract::State, routing::post};
use link_test_kit::TempDir;
use serde_json::{Value, json};

pub type Shared<T> = Arc<Mutex<T>>;

/// The tokens [`model`] says each answer took: its prompt's, its
/// answer's, and of the prompt, those cached.
pub const USAGE: (u64, u64, u64) = (1500, 25, 1000);

/// A bare chat completions model: answers with how many messages it was
/// sent and the last one, streamed in two pieces, then its [`USAGE`];
/// keeps each request. A thread's first answer (one message) waits between
/// its pieces longer than any connection's pace, so the connection shows
/// the first piece before the whole: the
/// tests check it's streamed there. Later answers come at once.
#[expect(clippy::disallowed_methods, reason = "a fake model slow on purpose")]
pub async fn model(
    State(asked): State<Shared<Vec<Value>>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    asked.lock().unwrap().push(body);
    let last = messages
        .last()
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");
    let answer = format!("I see {} messages; the last: {last}", messages.len());
    let pause = match messages.len() {
        ..=2 => Duration::from_millis(2500),
        _ => Duration::ZERO,
    };
    let chunk = |c: &str| {
        let data = json!({"id": "gen-1", "choices": [{"delta": {"content": c}}]});
        Ok::<_, std::io::Error>(bytes::Bytes::from(format!("data: {data}\n\n")))
    };
    let (send, mut pieces) = tokio::sync::mpsc::channel(4);
    tokio::spawn(async move {
        let (first, rest) = answer.split_at(6);
        let _ = send.send(chunk(first)).await;
        tokio::time::sleep(pause).await;
        let _ = send.send(chunk(rest)).await;
        // Its usage last, as OpenRouter always gives it.
        let (input, output, cached) = USAGE;
        let usage = json!({"id": "gen-1", "choices": [], "usage": {"prompt_tokens": input,
            "completion_tokens": output, "prompt_tokens_details": {"cached_tokens": cached}}});
        let _ = send
            .send(Ok(bytes::Bytes::from(format!("data: {usage}\n\n"))))
            .await;
        let _ = send
            .send(Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n")))
            .await;
    });
    let stream = futures_util::stream::poll_fn(move |cx| pieces.poll_recv(cx));
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from_stream(stream))
        .unwrap()
}

/// Serves `app` on a random port of 127.0.0.1: the port.
pub async fn serve(app: Router) -> u16 {
    serve_at(app).await.port()
}

/// [`serve`], the address.
pub async fn serve_at(app: Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// How many ports an agent of these tests takes from its base port: its
/// harness's one and one more, as the mini, fixture and Link harnesses'
/// contracts each say (`"ports": 1`).
pub const BLOCK: u16 = 2;

/// A block of [`BLOCK`] ports no one listens on, for an agent's, and no other
/// test's here while it's held: given back when dropped. It's outside the
/// ports an agent moves to, from 20000 to 32768, so an agent of another
/// test here that moves never lands on it before its own agent listens
/// there; and outside the ports the system hands out, where the fakes'
/// servers listen ([`blocks`]). The test holds its ports from the moment
/// they're found free until it lets them go for its agent ([`Block::let_go`]),
/// so no other program here takes one in between, and holds the block
/// itself across test programs until it's dropped ([`HeldNumber`]), so
/// another run of a test program never takes it while its agent binds.
///
/// [`HeldNumber`]: link_test_kit::HeldNumber
pub struct Block(
    pub u16,
    Vec<std::net::TcpListener>,
    #[expect(dead_code, reason = "held for its lock, let go when dropped")]
    link_test_kit::HeldNumber,
);

impl Block {
    /// Its agent's settings for it, as lines of its `agent.toml`: its base
    /// port and its block's size, so its start takes these ports and
    /// looks for other programs on none past them.
    pub fn settings(&self) -> String {
        format!("port = {}\nblock = {BLOCK}\n", self.0)
    }

    /// The listener on `port` of this block, held on by the test, its
    /// agent's from the start: a port another program holds.
    pub fn keep(&mut self, port: u16) -> std::net::TcpListener {
        let at = self
            .1
            .iter()
            .position(|l| l.local_addr().is_ok_and(|a| a.port() == port))
            .unwrap_or_else(|| panic!("port {port} isn't held in block {}", self.0));
        self.1.swap_remove(at)
    }

    /// Its ports let go, but for those [kept](Block::keep), for its agent
    /// to take.
    pub fn let_go(&mut self) {
        self.1.clear();
    }
}

/// A block's ports are the test's from the moment they're found free:
/// no other program here takes one before its agent would. Let go, the
/// test holds none of them but one kept, which stays the test's. (Not
/// that they're then free: let go, any program here may take one, a
/// test of another test program among them.)
#[test]
fn a_block_is_held_until_it_is_let_go() {
    let taken = |port: u16| std::net::TcpListener::bind(("127.0.0.1", port)).is_err();
    let mut block = free_block();
    let base = block.0;
    assert!((base..base + BLOCK).all(taken), "a port of {base} was free");
    let kept = block.keep(base + 1);
    block.let_go();
    assert!(block.1.is_empty(), "a port was still held");
    assert!(taken(base + 1), "the port kept was let go");
    assert_eq!(kept.local_addr().unwrap().port(), base + 1);
}

impl Drop for Block {
    fn drop(&mut self) {
        // Never a panic here, as a test's panic unwinds: one would abort
        // every test of the program.
        let mut blocks = BLOCKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        blocks.held.retain(|base| *base != self.0);
        blocks.given_back.push((self.0, Instant::now()));
    }
}

/// The [`Block`]s held, and when each given back was.
struct Blocks {
    held: Vec<u16>,
    given_back: Vec<(u16, Instant)>,
}

static BLOCKS: Mutex<Blocks> = Mutex::new(Blocks {
    held: Vec::new(),
    given_back: Vec::new(),
});

/// How long a block given back rests before another test takes it: what
/// its agent started may outlive it a moment, a bridge holding the
/// agent's port as it goes.
const REST: Duration = Duration::from_secs(10);

/// Where [`Block`]s are, from and to: between the ports an agent moves to
/// and the system's own for outgoing connections (macOS's from 49152),
/// or past those (Linux's to 60999), up to the last base port a harness
/// takes, one with a whole default block above it
/// ([`link_harness_contract::harness::base_port`]).
fn blocks() -> (u32, u32) {
    let system = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range")
        .ok()
        .and_then(|range| {
            let mut ends = range.split_whitespace().map(|n| n.parse::<u32>().ok());
            Some((ends.next()??, ends.next()??))
        })
        .unwrap_or((49152, 65535));
    match system.0.saturating_sub(32768) >= u32::from(BLOCK) * 20 {
        true => (32768, system.0),
        false => (
            system.1 + 1,
            u32::from(u16::MAX - link_harness_contract::harness::PORT_BLOCK) + 1,
        ),
    }
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
pub fn free_block() -> Block {
    let (from, to) = blocks();
    let started = Instant::now();
    loop {
        let mut blocks = BLOCKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Neither held nor resting: trying a held one's ports would take
        // them from its agent a moment.
        let open: Vec<u16> = (from..to - u32::from(BLOCK))
            .step_by(usize::from(BLOCK))
            .map(|base| base as u16)
            .filter(|base| {
                !blocks.held.contains(base)
                    && blocks
                        .given_back
                        .iter()
                        .all(|(b, at)| b != base || at.elapsed() >= REST)
            })
            .collect();
        // A random one of them.
        let random = crate::random() as usize;
        let Some(&base) = open.get(random % open.len().max(1)) else {
            // Every block held or resting: one is given back as its test
            // ends, and a test waits for it a while, then fails rather
            // than waiting for good, the lock let go first, so the
            // program's other tests go on.
            let held = blocks.held.len();
            drop(blocks);
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "no block of {BLOCK} free ports in two minutes: {held} held by this test program",
            );
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        // Another test program's, while held: passed over as held here.
        let Some(held) = link_test_kit::HeldNumber::hold("agent-ports", u32::from(base)) else {
            blocks.given_back.push((base, Instant::now()));
            continue;
        };
        let free: Option<Vec<std::net::TcpListener>> = (base..base + BLOCK)
            .map(|port| std::net::TcpListener::bind(("127.0.0.1", port)).ok())
            .collect();
        if let Some(listeners) = free {
            blocks.held.push(base);
            blocks.given_back.retain(|(b, _)| *b != base);
            return Block(base, listeners, held);
        }
    }
}

/// The agent in the foreground, and everything it started, stopped when
/// it's dropped: each of its programs runs from, or names, its folder.
pub struct Running(pub Child, pub PathBuf);

impl Drop for Running {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        // The folder whole, not as the start of another test's: one
        // test's `agent-measure` is the start of another's
        // `agent-measure-tokens`.
        let _ = Command::new("pkill")
            .args(["-KILL", "-f"])
            .arg(format!("{}(/| |$)", self.1.display()))
            .status();
    }
}

pub use link_agent_contract::testing::connection;

/// The longest an agent's start may take before it answers: `lnk agent
/// start`'s own bound on its harness's start, by which it answers or ends.
pub const STARTING: Duration = Duration::from_secs(600);

/// Waits until agent `name` in the home `home` answers its harness's
/// health check, on the port its start gave its harness (its
/// `ports.json`, moved or not), or fails with what `ended` says once its
/// start has ended. A test's deadline for an answer starts here, never at
/// the start's spawn, however long a busy machine took to start it.
pub async fn answering(home: &Path, name: &str, mut ended: impl FnMut() -> Option<String>) {
    let ports = home.join(format!(".lnk/safe/agents/{name}/ports.json"));
    link_test_kit::wait::eventually_async(
        &format!("agent {name} answers its health check"),
        STARTING,
        || {
            if let Some(why) = ended() {
                panic!("agent {name}'s start ended before it answered: {why}");
            }
            let port = std::fs::read(&ports)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|saved| saved["ports"][0].as_u64())
                .and_then(|port| u16::try_from(port).ok());
            port.is_some_and(|port| link_harness_contract::harness::http_ok(port, "/health"))
        },
    )
    .await;
}

/// The ID of the agent `start` makes.
const AGENT_ID: &str = "m1n1a9e2t5s7";

/// An agent on the mini harness, started in the foreground with `section`
/// (the connection's settings) and `env`, against a bare model. Dropped, it
/// stops, then its folder goes.
pub struct Agent {
    /// First: fields drop in order, so it stops before `root` goes.
    pub running: Running,
    /// Its ports, given back once it stopped.
    pub block: Block,
    pub root: TempDir,
    /// Its runtime folder (`XDG_RUNTIME_DIR`), its own, never this user's,
    /// short enough for the sockets Link makes in it.
    runtime: TempDir,
    pub home: PathBuf,
    /// The agent's files.
    pub files: PathBuf,
    pub log: PathBuf,
    /// What the model was asked.
    pub asked: Shared<Vec<Value>>,
    /// The folders its programs are in.
    pub programs: Vec<PathBuf>,
    harness: String,
}

/// The programs of a connection's own tests, from its folder (`folder`,
/// its manifest's): the core's (`lnk`, `lnk-agent`, `lnk-mini-harness`) first,
/// then the folder of `module`, its `lnk-<connection>`. In one workspace
/// both are its `target/debug`, found before anything on the PATH.
pub fn programs_beside(folder: &Path, module: &Path) -> Vec<PathBuf> {
    let core = link_test_kit::programs_in(&folder.join("../../.."));
    vec![core, module.parent().expect("a folder").to_path_buf()]
}

/// An agent on the mini harness, its programs in `programs`: `lnk`,
/// `lnk-agent` and `lnk-mini-harness` in the first (the core's `target/debug`),
/// a connection's module in any. It starts with `prepare` given its home,
/// and its block still held.
pub async fn start(
    programs: &[PathBuf],
    name: &str,
    sandboxed: bool,
    section: &str,
    env: &[(&str, String)],
    prepare: impl FnOnce(&Path, &mut Block),
) -> Agent {
    start_with(programs, name, sandboxed, section, env, prepare, "mini").await
}

/// The same integration with fake media support, for connection transport tests.
pub async fn start_fixture(
    programs: &[PathBuf],
    name: &str,
    sandboxed: bool,
    section: &str,
    env: &[(&str, String)],
    prepare: impl FnOnce(&Path, &mut Block),
) -> Agent {
    start_with(
        programs,
        name,
        sandboxed,
        section,
        env,
        prepare,
        "harness-fixture",
    )
    .await
}

async fn start_with(
    programs: &[PathBuf],
    name: &str,
    sandboxed: bool,
    section: &str,
    env: &[(&str, String)],
    prepare: impl FnOnce(&Path, &mut Block),
    harness: &str,
) -> Agent {
    let bin = &programs[0];
    let path = programs
        .iter()
        .map(|p| p.display().to_string())
        .chain(["/usr/bin".into(), "/bin".into()])
        .collect::<Vec<_>>()
        .join(":");
    let root = TempDir::new(&format!("agent-{name}"));
    let home = root.join("home");
    let files = home.join("Link/Agents/main/home");
    std::fs::create_dir_all(home.join(".lnk/safe/agents")).unwrap();

    let asked: Shared<Vec<Value>> = Arc::default();
    let model_port = serve(
        Router::new()
            .route("/v1/chat/completions", post(model))
            .with_state(asked.clone()),
    )
    .await;
    let mut block = free_block();
    // Its connections' tokens with your secrets, where Link keeps them.
    link_agent_contract::testing::write_agent(
        &home,
        "main",
        &format!(
            "id = \"agent:{AGENT_ID}\"\nharness = \"{harness}\"\n{}\n[model]\nprovider = \"local\"\nname = \"bare\"\n\
             base_url = \"http://127.0.0.1:{model_port}\"\n\n{section}",
            block.settings()
        ),
    );
    prepare(&home, &mut block);
    block.let_go();

    let log = root.join("agent.log");
    // The kit's environment: its home, an isolated Keychain and its own
    // runtime folder, where its bridges listen, never one another test's
    // agent of this ID left.
    let runtime = TempDir::short("rt");
    runtime.set_mode(0o700).unwrap();
    let mut command = link_test_kit::env::command(bin.join("lnk-agent"), &home);
    command
        .env("XDG_RUNTIME_DIR", runtime.path())
        .args(["start", "--foreground", "--run.files"])
        .arg(&files)
        .env("PATH", path)
        .env("LNK_EXE", bin.join("lnk"))
        // Never this machine's own services: their names don't depend on
        // HOME, so they'd be your agent's.
        .env("LNK_SERVICES", "off")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&log).unwrap())
        .stderr(std::fs::File::options().append(true).open(&log).unwrap());
    if !sandboxed {
        command.arg("--no-sandbox");
    }
    let running = Running(command.spawn().unwrap(), root.to_path_buf());
    Agent {
        running,
        block,
        root,
        runtime,
        home,
        files,
        log,
        asked,
        programs: programs.to_vec(),
        harness: harness.into(),
    }
}

impl Agent {
    /// Waits until it answers ([`answering`]): a test's first message is
    /// timed from here.
    pub async fn answering(&mut self) {
        let (home, log, running) = (&self.home, &self.log, &mut self.running.0);
        answering(home, "main", || {
            let status = running.try_wait().ok().flatten()?;
            Some(format!(
                "{status}\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            ))
        })
        .await;
    }

    /// Fails the test if the agent stopped, or `since` is a minute old
    /// (the kit's minute, scaled for a slow machine).
    pub fn check(&mut self, since: Instant, waiting: &str) {
        let log = std::fs::read_to_string(&self.log).unwrap_or_default();
        if let Ok(Some(status)) = self.running.0.try_wait() {
            panic!("the agent stopped ({status}):\n{log}");
        }
        assert!(
            since.elapsed() < link_test_kit::wait::scaled(Duration::from_secs(60)),
            "{waiting}\n{log}"
        );
    }

    /// The model was asked `times` in all, the last time with the direct
    /// thread so far; the conversation is in `folder` of the agent's
    /// files; the harness holds the endpoint's key and none of `secrets`.
    pub fn remembered(&self, folder: &str, secrets: &[&str], times: usize) {
        let asked = self.asked.lock().unwrap().clone();
        assert_eq!(
            asked.len(),
            times,
            "a stranger's message reached the model: {asked:?}"
        );
        let last = asked.last().unwrap();
        let second: Vec<&str> = last["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["content"].as_str().unwrap())
            .collect();
        assert_eq!(
            second,
            ["Hello", "I see 1 messages; the last: Hello", "And again"]
        );
        assert_eq!(last["model"], "bare");

        // Kept in the harness's home, as the thread's file.
        let kept = self.thread(folder);
        assert!(kept.contains("And again"), "{kept}");
        let config = std::fs::read_to_string(self.harness_home().join("config.json")).unwrap();
        assert!(config.contains("endpoint_key"), "{config}");
        for secret in secrets {
            assert!(!config.contains(secret), "{config}");
        }
    }

    /// The owner's mention in a group reached the model once, as `text`
    /// alone, in a thread of its own: the conversation is in `folder` of
    /// the agent's files.
    pub fn asked_in_group(&self, folder: &str, text: &str) {
        let asked = self.asked.lock().unwrap().clone();
        let with: Vec<&Value> = asked
            .iter()
            .filter(|a| {
                let messages = a["messages"].as_array().unwrap();
                messages.last().unwrap()["content"] == text
            })
            .collect();
        assert_eq!(with.len(), 1, "the mention wasn't asked once: {asked:?}");
        // The mention alone: nothing of another thread or a system message.
        assert_eq!(with[0]["messages"].as_array().unwrap().len(), 1);
        let kept = self.thread(folder);
        assert!(kept.contains(text), "{kept}");
    }

    /// Runs `lnk-<connection> send`, as the agent's bridge does for an
    /// answer nobody waits for (a schedule's): `text` to `chat` (as its
    /// thread names it after the connection), the connection's `settings` and
    /// its folder handed on stdin.
    pub fn send_on(
        &self,
        connection: &str,
        settings: &Value,
        chat: &str,
        text: &str,
    ) -> std::process::Output {
        self.send_with(connection, settings, chat, text, &[])
    }

    /// The case every connection with a token passes: `lnk-<connection> send`,
    /// its `settings` holding a token the connection's fake refuses (reached
    /// as `env` says), exits `link_connection_contract::TOKEN_REFUSED`, apart
    /// from any other failure.
    pub fn refuses_token(
        &self,
        connection: &str,
        settings: &Value,
        chat: &str,
        env: &[(&str, String)],
    ) {
        let sent = self.send_with(connection, settings, chat, "Hi.", env);
        assert_eq!(
            sent.status.code(),
            Some(link_connection_contract::TOKEN_REFUSED.into()),
            "lnk-{connection} send with a refused token: {}",
            String::from_utf8_lossy(&sent.stderr)
        );
    }

    /// [`Agent::send_on`], `lnk-<connection>` given `env` besides.
    pub fn send_with(
        &self,
        connection: &str,
        settings: &Value,
        chat: &str,
        text: &str,
        env: &[(&str, String)],
    ) -> std::process::Output {
        use std::io::Write;
        // Proved as the agent, with its key for this start, as its way in
        // hands it.
        let binding = binding(&self.home, "main");
        let sending = json!({"bridge": {"agent": "main", "endpoint": "", "model": "bare",
            "identity": binding.identity, "key": binding.key.expose(),
            "settings": settings,
            "state": self.home.join(".lnk/safe/agents/main/connections").join(connection)},
            "chat": chat, "text": text});
        let program = self
            .programs
            .iter()
            .map(|p| p.join(format!("lnk-{connection}")))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("no lnk-{connection} in {:?}", self.programs));
        let mut child = link_test_kit::env::command(program, &self.home)
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .arg("send")
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{sending}").unwrap();
        child.wait_with_output().unwrap()
    }

    /// The selected harness's home.
    pub fn harness_home(&self) -> PathBuf {
        self.home
            .join(".lnk/data/agents/main/harnesses")
            .join(&self.harness)
    }

    /// Thread `folder` (`telegram-42`, the connection, `-` and the chat) as
    /// the mini harness keeps it: its file names the thread with `%3A`
    /// for its `:` (`telegram%3A42`).
    pub fn thread(&self, folder: &str) -> String {
        let path = self
            .harness_home()
            .join("threads")
            .join(format!("{}.jsonl", folder.replacen('-', "%3A", 1)));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    pub fn stop(self) {
        drop(self);
    }
}

/// Whether a wrapped agent sandbox runs here: namespaces and Landlock,
/// through the kit's gates and their capability requirement variables.
#[cfg(target_os = "linux")]
pub fn bubblewrap() -> bool {
    link_test_kit::gate::sandbox_with_landlock()
}

/// Agent `name`'s binding in the keyring of the machine whose home is
/// `home`: its identity and its key for this start, under its ID (its
/// identity here names it).
pub fn binding(home: &std::path::Path, name: &str) -> link_sandbox_binding::Binding {
    let identity = link_agent_contract::testing::record(home, name, "identity.toml");
    let id = identity["id"]
        .as_str()
        .unwrap_or_else(|| panic!("agent {name} has no identity: {identity:?}"));
    link_sandbox_binding::Keyring::at(&home.join(".lnk/safe/agents/credentials"))
        .bound(id)
        .unwrap_or_else(|| panic!("agent {name} isn't bound while it runs"))
}
