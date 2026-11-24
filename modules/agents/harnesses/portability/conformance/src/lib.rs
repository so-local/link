//! The conformance kit: whether a harness program answers the contract in
//! `link_harness_contract::harness`, checked by running it as `lnk-agent` would,
//! without Link: `contract`, `install`, `configure`, `command` and
//! `health` with its home as `HOME`, `needs` outside its proxy and
//! behind it (`proxied`), then its endpoint, against a bare chat
//! completions model.
//!
//! Every harness's tests run it: the core's on its mini harness, and each
//! harness's own from its repository, with `link-core` beside it. Its
//! [`agent`] is an agent on the mini harness, for the tests of a
//! connection's module, the core's and each connection's from its own folder;
//! [`connection-fixture`] is the connection fixture's service, faked, for a test that
//! needs a connection but isn't of one.
//!
//! ```no_run
//! link_harness_conformance::check(std::path::Path::new("target/debug/lnk-mini-harness"));
//! ```
//!
//! What it checks of the endpoint, for an adapter that has one (a
//! harness that speaks no connection):
//!
//! - it refuses a request without the key, or with another;
//! - when its `contract` says `from`, it answers a message from
//!   someone who isn't the owner (`Said::from`: a schedule, a
//!   background command, someone in a chat the agent is shared with and
//!   their name), telling the model who, and refuses one from no sender a
//!   message may be from, or a name that isn't a person's;
//! - it answers requests naming its agent in `Link-Agent`, as Link's
//!   bridges send them, and, when its `contract` says `agent`, refuses
//!   one naming another agent before the model sees it, and answers one
//!   naming none as its own agent's;
//! - it proves it holds the key before it's sent one, when its
//!   `contract` says `proof`, and sets `TCP_NODELAY` on the connection
//!   it accepted, read from the harness's own socket where this system
//!   says (Linux, given the right to take a child's descriptor; `lsof`
//!   on macOS) and passed over where it doesn't;
//! - it answers, streamed and whole, with what the model said;
//! - it keeps each thread, and keeps threads apart;
//! - a turn cut short, its caller gone before the answer, runs to its
//!   end, and the answer is kept in the thread;
//! - it calls a tool through the bridge: the kit's own MCP server, given
//!   as Link gives a tool (in the settings' `tools`, which the hub gives
//!   each session as its `mcpServers`, and at `$LNK_TOOL_KIT`), called
//!   when the model asks, its answer sent back to the model;
//! - when its `contract` says `tools`, a thread narrowed to some tools
//!   (`Said::tools`) is offered only those, from then on: a later list
//!   narrows it again and never widens it, a message without one keeps
//!   it, and a call says the thread's tools in its `_meta`
//!   (`link.local/tools`); a list that isn't one is refused.
//!
//! A harness that takes a local model only one way is checked with that
//! provider ([`Check::provider`]). Its `install` and `configure` get
//! this machine's proxy settings, as a real harness's downloads need.

#[cfg(unix)]
pub mod agent;
#[cfg(unix)]
pub mod fixture_connection;
#[cfg(target_os = "linux")]
pub mod runtime;

mod agent_state;
mod endpoint;
mod kit;
mod sandbox;
mod saying;
mod serving;
use agent_client_protocol as acp;
use saying::{Saying, say};
use serving::{last_text, model, network_env, tool, tool_named};

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, bail, ensure};
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use link_harness_contract::harness;
use link_harness_contract::harness::Contract;
use link_harness_contract::harness::Launch;
use link_harness_contract::harness::Model;
use link_harness_contract::harness::Settings;
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// The model each check's harness is given: `local`, so the harness
/// reaches it at its address with no key.
pub const MODEL: &str = "bare";

/// The endpoint's key in the settings.
const KEY: &str = "conformance-endpoint-key";

/// The agent the kit runs the harness for (`LNK_AGENT`), named in each
/// request, as Link's bridges name it.
const AGENT: &str = "main";

/// Its stable identity, separate from its display name.
const AGENT_ID: &str = "aaaaaaaaaaaa";

/// An agent the harness doesn't run.
const OTHER_AGENT: &str = "conformance-other";

/// Someone in a chat the agent is shared with, and their name there:
/// any script, as `Said::from_name` carries it.
const PERSON: &str = "member:slack:T0ACME/U0ALICE";
const PERSON_NAME: &str = "Zoë Ng";

/// Panics, naming what failed, unless `adapter` (an `lnk-<name>`)
/// answers the contract.
pub fn check(adapter: &Path) {
    Check::new(adapter).run();
}

/// [`check`], as a result.
pub fn try_check(adapter: &Path) -> anyhow::Result<()> {
    Check::new(adapter).try_run()
}

/// A check of one adapter, with what its harness needs to be given.
pub struct Check {
    adapter: PathBuf,
    provider: String,
    stand_in: Option<StandIn>,
}

/// A harness the kit installs in place of the real one.
struct StandIn {
    program: String,
    entrypoints: Vec<String>,
    files: Vec<(String, String)>,
    /// Whether it finds Link's tools in its environment
    /// ([`Check::tools_in_settings`]), not its sessions.
    tools_in_settings: bool,
    native_state: bool,
    prelude: String,
}

impl Check {
    /// `adapter` (an `lnk-<name>`), with a `local` model.
    pub fn new(adapter: &Path) -> Self {
        Self {
            adapter: adapter.to_path_buf(),
            provider: "local".into(),
            stand_in: None,
        }
    }

    /// Its harness stood in for, so the check downloads nothing: the
    /// harness fixture (`lnk-harness-fixture`, built beside the adapter) at
    /// `program` in its home, run whenever it's given `acp` among its
    /// arguments, or a program ending `-acp` (an ACP adapter's own),
    /// answering anything else with nothing; and each of `files`, a path in its home and its text, written there first, so
    /// the adapter's `install` finds its harness installed. The model it
    /// asks is the kit's.
    pub fn stand_in(mut self, program: &str, files: &[(&str, &str)]) -> Self {
        self.stand_in = Some(StandIn {
            program: program.into(),
            entrypoints: Vec::new(),
            files: files
                .iter()
                .map(|(path, text)| (path.to_string(), text.to_string()))
                .collect(),
            tools_in_settings: false,
            native_state: true,
            prelude: String::new(),
        });
        self
    }

    /// Another published entrypoint for the same fixture, after [`Check::stand_in`].
    pub fn stand_in_entrypoint(mut self, program: &str) -> Self {
        self.stand_in
            .as_mut()
            .expect("a stand-in first")
            .entrypoints
            .push(program.into());
        self
    }

    /// Fixture-owned command handling before the default ACP/configuration dispatch.
    /// A native foreground service can report readiness and exercise its real owner.
    pub fn stand_in_prelude(mut self, prelude: &str) -> Self {
        self.stand_in.as_mut().expect("a stand-in first").prelude = prelude.into();
        self
    }

    /// Its harness takes no MCP server in a session, as ACP's
    /// `mcpServers`, so its adapter writes Link's tools into the harness's
    /// own settings: its stand-in takes none either, and finds them as
    /// Link's sandbox gives them (`$LNK_TOOL_<NAME>`). After
    /// [`Check::stand_in`].
    pub fn tools_in_settings(mut self) -> Self {
        if let Some(stand_in) = &mut self.stand_in {
            stand_in.tools_in_settings = true;
        }
        self
    }

    /// Match a runtime that exposes ACP sessions but cannot import foreign
    /// state. This exercises its real adapter's portable continuation path.
    pub fn without_native_state(mut self) -> Self {
        if let Some(stand_in) = &mut self.stand_in {
            stand_in.native_state = false;
        }
        self
    }

    /// The model's provider in the settings, in place of `local`: `ollama`
    /// for a harness that runs local models only through Ollama. The kit's
    /// model answers chat completions at its address either way.
    pub fn provider(mut self, provider: &str) -> Self {
        self.provider = provider.into();
        self
    }

    /// Panics, naming what failed, unless the adapter answers the
    /// contract.
    pub fn run(self) {
        let adapter = self.adapter.clone();
        if let Err(err) = self.try_run() {
            panic!("{} doesn't conform: {err:#}", adapter.display());
        }
    }

    /// [`Check::run`], as a result.
    pub fn try_run(self) -> anyhow::Result<()> {
        let kit = Kit::new(self)?;
        let contract = kit.contract()?;
        kit.stand_in()?;
        kit.run("install", None)?;
        kit.configure()?;
        let launch: Launch = serde_json::from_str(&kit.run("command", None)?)
            .context("command didn't print a Launch")?;
        sandbox::proxy_checks(&launch)?;
        let mut running = kit.launch(&launch)?;
        kit.wait_healthy(&launch)?;
        let connections = kit.run("connections", None)?;
        serde_json::from_str::<Vec<String>>(&connections)
            .with_context(|| format!("connections didn't print a list of names: {connections}"))?;
        let traces = kit.run("traces", None)?;
        serde_json::from_str::<Vec<harness::traces::Trace>>(&traces)
            .with_context(|| format!("traces didn't print a list of traces: {traces}"))?;
        let state = kit.run("state", None)?;
        serde_json::from_str::<harness::State>(&state)
            .with_context(|| format!("state didn't print a State: {state}"))?;
        // Outside its proxy and behind it (`proxied`), as Link asks.
        for proxied in [false, true] {
            let mut settings: Settings = serde_json::from_str(&kit.settings(false))?;
            settings.proxied = proxied;
            let needs = kit.run("needs", Some(&serde_json::to_string(&settings)?))?;
            serde_json::from_str::<harness::Needs>(&needs)
                .with_context(|| format!("needs didn't print Needs: {needs}"))?
                .checked()
                .with_context(|| format!("needs named what Link refuses: {needs}"))?;
        }
        if let Some(endpoint) = kit.endpoint()? {
            kit.endpoint_checks(&endpoint, &contract)
                .map_err(|err| anyhow::anyhow!("{err:#}\n{}", kit.log()))?;
            kit.state_checks(&endpoint)
                .map_err(|err| anyhow::anyhow!("{err:#}\n{}", kit.log()))?;
            if contract.proof {
                kit.nodelay_checks(&endpoint, running.child.id())
                    .map_err(|err| anyhow::anyhow!("{err:#}\n{}", kit.log()))?;
            }
        }
        running.close()?;
        #[cfg(target_os = "linux")]
        ensure!(
            link_test_kit::left_under(&kit.home).is_empty(),
            "native processes survived adapter shutdown"
        );
        Ok(())
    }
}

/// What a check's message asks of the kit's model: to call the kit's
/// tool.
const TOOL_ASK: &str = "Stamp a note with the kit's tool";

/// What a check's message asks of the kit's model: to take its time, so
/// the caller can go before the answer.
const SLOW_ASK: &str = "Slowly";

/// How long the model takes over [`SLOW_ASK`].
const SLOW: Duration = Duration::from_secs(2);

/// The kit's tool, as the bridge names it (`$LNK_TOOL_KIT`), and its
/// one call.
const TOOL: &str = "kit";
const TOOL_CALL: &str = "stamp";

/// What the kit's tool answers: only a harness that called it, and sent
/// its answer to the model, has it in the model's answer.
const STAMP: &str = "stamped-4f1c9a";

/// How this machine reaches the internet, passed to `install` and
/// `configure`.
const NETWORK_ENV: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "PIP_CERT",
    "npm_config_https_proxy",
    "npm_config_noproxy",
];

/// What the kit's model and tool were sent.
#[derive(Default)]
struct Seen {
    /// Each request to the model.
    asked: Vec<Value>,
    /// The kit's message of each request the model has answered.
    answered: Vec<String>,
    /// Each call of the kit's tool's `tools/call`.
    called: Vec<Value>,
}

type Shared = Arc<Mutex<Seen>>;

/// One check's folders, ports and model. Its folder, with all the
/// harness installed in it, goes when it's dropped, the check passed or
/// not.
struct Kit {
    adapter: PathBuf,
    provider: String,
    stand_in: Option<StandIn>,
    root: TempDir,
    home: PathBuf,
    files: PathBuf,
    port: u16,
    /// The block from `port`, held across test programs while the check runs.
    _ports: link_test_kit::HeldNumber,
    model_port: u16,
    tool_port: u16,
    seen: Shared,
    runtime: tokio::runtime::Runtime,
}

/// The harness, stopped with all it started when dropped.
struct Running {
    child: link_test_kit::ChildGroup,
    closed: bool,
}

impl Running {
    fn close(&mut self) -> anyhow::Result<()> {
        self.child.interrupt();
        let status = link_test_kit::wait::within(
            "the adapter's graceful shutdown",
            Duration::from_secs(30),
            || self.child.try_status().ok().flatten(),
        );
        self.child.stop();
        self.closed = true;
        ensure!(status?.success(), "adapter did not shut down successfully");
        Ok(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.close();
        }
    }
}

/// The first port of a block of [`harness::PORT_BLOCK`] no one listens
/// on, below the ports the system hands out, held across test programs
/// until it's dropped: the harness binds the block itself, so seeing it
/// free doesn't keep another run of a test program from taking it first.
fn free_block() -> link_test_kit::HeldNumber {
    // From a random one of the 50, so runs at once seldom try the same.
    let random = (random() % 50) as u32;
    let bases = (0..50).map(|k| 21_000 + (random + k) % 50 * u32::from(harness::PORT_BLOCK));
    link_test_kit::HeldNumber::first("harness-ports", bases, |base| {
        (base..base + u32::from(harness::PORT_BLOCK))
            .all(|port| std::net::TcpListener::bind(("127.0.0.1", port as u16)).is_ok())
    })
}

/// A random number, a new one each call: `std`'s hasher, seeded at random
/// for each process and each new one after, of nothing.
pub(crate) fn random() -> u64 {
    use std::hash::BuildHasher;
    std::hash::RandomState::new().hash_one(())
}

#[cfg(test)]
link_test_kit::program!(unit);
