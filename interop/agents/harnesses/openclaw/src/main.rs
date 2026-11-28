//! `lnk-openclaw`: the OpenClaw adapter. `lnk-agent` runs it; see
//! `link_harness_contract::harness` for the commands.
//!
//! OpenClaw is installed with its rootless installer into `~/.openclaw`
//! (its own Node included, no Homebrew), at a pinned version ([`VERSION`]), configured with its own `openclaw
//! config set`, and run as `openclaw gateway run` with `openclaw acp`
//! beside it, its ACP over stdio, behind the hub (`link_harness_hub`):
//! this program's `serve` is its endpoint, ACP at `/acp` on the port
//! after the gateway's. Keys go in `~/.openclaw/.env`, never on a command
//! line.

use link_openclaw::cli::Command as Given;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use link_format::{
    Done, Exit, Foreign, Note, Out, Path as Shown, Report, WithinExt, cmd, sentence,
};
use link_harness_contract::harness::traces::{self, Kind, Trace};
use link_harness_contract::harness::{self, Launch, Settings};
use serde_json::json;

mod configuring;
mod features;
mod identity;
mod installing;
mod native;
mod providers;
mod schedules;
mod state;
mod workarounds;
use configuring::{configure, configure_environment, forget_values, read_by_configure};
use installing::{bin, configured, doctored, install, prefix, uninstall};
use providers::{MODEL_URL, PROVIDERS_SET, model_id, providers, providers_gone, record_providers};

// The OpenClaw release Link installs. Changing it means updating
// [`INSTALLER`] to `scripts/install-cli.sh` at that release's tag
// (`git rev-parse v<version>^{commit}` in OpenClaw's repository) and its
// SHA-256 (`git show <commit>:scripts/install-cli.sh | sha256sum`), and
// its sourced install policy the same way.
include!(concat!(env!("OUT_DIR"), "/external-pins.rs"));
const VERSION: &str = OPENCLAW_VERSION;

/// OpenClaw's installer (what `openclaw.ai/install-cli.sh` serves), at
/// [`VERSION`]'s commit.
const INSTALLER: harness::Installer = harness::Installer {
    name: "OpenClaw",
    url: OPENCLAW_URL,
    sha256: OPENCLAW_SHA256,
    companions: &[harness::InstallerFile {
        name: "install-policy.sh",
        url: OPENCLAW_INSTALL_POLICY_URL,
        sha256: OPENCLAW_INSTALL_POLICY_SHA256,
    }],
};

/// The gateway's port: OpenClaw's default, unless Link gives the agent
/// another base port (`LNK_PORT`).
fn port() -> u16 {
    harness::base_port(18789)
}

/// The hub's port, its endpoint: the one after the gateway's, which
/// OpenClaw derives none of its own from.
fn endpoint_port() -> u16 {
    port() + 1
}

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[
        harness::saved_added_tools,
        harness::continuation::saved,
        harness::identity::memory::saved,
        link_harness_hub::SAVED[0],
        link_harness_hub::SAVED[1],
        providers::saved,
    ]);
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

/// `value` as one line of JSON on stdout: what `lnk-agent` reads.
fn answer(out: &mut Out, line: serde_json::Result<String>) -> Result<Done, Report> {
    let line = line.within("writing its answer")?;
    out.data(format!("{line}\n").as_bytes());
    Ok(Done::shown())
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    link_files::layout::check_exec_environment().within("checking the program directory")?;
    dispatch(out, link_openclaw::cli::parse())
}

fn dispatch(out: &mut Out, command: Given) -> Result<Done, Report> {
    match command {
        Given::Uninstall(given) => {
            let args = &given;
            uninstall(args)
        }

        // It reaches a hosted model at the proxy's address
        // (`Model.base_url`), so the proxy can hold the key.
        Given::OpenclawContract(_) => {
            let mut contract = harness::Contract::new(true);
            contract.features = Some(features::honors());
            // What its hub's agent state gives and takes, which `lnk agent
            // harness set` carries; its own memory files (MEMORY.md and USER.md)
            // among them, exported into Link's folder at a switch away.
            contract.state = state::kinds();
            contract.offline_state = true;
            contract.native_memory = true;
            // Who the agent is, written into SOUL.md and AGENTS.md and
            // read back out.
            contract.identity = true;
            // Its auth profiles may keep a key Link takes as it brings an install in.
            contract.forget = true;
            // It takes the spec's [instructions] it carries (persona, owner,
            // standing instructions, skills); its features say which.
            contract.instructions = true;
            // Link's bridge carries every connection to its gateway's chat
            // completions: each chat a session by its key, its default agent.
            contract.proof = true;
            (contract.agent, contract.from) = (true, true);
            answer(out, serde_json::to_string(&contract))
        }
        Given::OpenclawInstall(_) => install(out),
        Given::OpenclawConfigure(_) => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .within("reading settings on stdin")?;
            let settings: Settings =
                serde_json::from_str(&input).within("reading the settings on stdin")?;
            harness::continuation::Context::configure(
                &link_files::home_dir(),
                "openclaw",
                &settings,
            )?;
            identity::configure(&link_files::home_dir(), &settings)?;
            if let Some(notes) = &settings.remembered {
                identity::remember(&link_files::home_dir(), notes)?;
            }
            // Endpoint and model keys can rotate without changing any
            // OpenClaw JSON. Refresh them before the structural cache so
            // an ordinary restart does not spawn its CLI for every key.
            configure_environment(&settings)?;
            // Nothing changed since the last one, and its doctor has run
            // for this release: OpenClaw is as it left it. A doctor that
            // failed runs again, however long the settings stay the same.
            let configured = configured();
            let doctored = std::fs::read_to_string(doctored()).ok().as_deref() == Some(VERSION);
            let read = read_by_configure(&input)?;
            if doctored && configured.unchanged(&read) {
                configuring::configure_endpoints(&settings)?;
                configured
                    .done(&read)
                    .within("recording the settings it configured")?;
                return Ok(Done::shown());
            }
            configured.forget();
            configure(&settings)?;
            configuring::configure_endpoints(&settings)?;
            configured
                .done(&read)
                .within("recording the settings it configured")?;
            Ok(Done::shown())
        }
        Given::OpenclawCommand(_) => answer(out, serde_json::to_string(&launch())),
        Given::OpenclawServe(_) => {
            // The hub's logs and the gateway's own lines are on this terminal.
            out.hand_over();
            serve()
        }
        Given::OpenclawHealth(_) => Ok(if harness::http_ok(endpoint_port(), "/health") {
            Done::shown()
        } else {
            Done::passed(1)
        }),
        // For `lnk module add`, which checks it is the release it asked for.
        Given::OpenclawWritten(given) => {
            let home = PathBuf::from(given.home);
            answer(out, serde_json::to_string(&written(&home)))
        }
        // Secrets Link took from a copy of an install: its profiles keep
        // none of them.
        Given::OpenclawClean(given) => {
            let home = PathBuf::from(given.home);
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .within("reading the secrets on stdin")?;
            let values: Vec<link_secretive::Secret> =
                serde_json::from_str(&input).within("reading the secrets on stdin")?;
            forget_values(&home, &values)?;
            Ok(Done::shown())
        }
        Given::OpenclawTraces(_) => answer(out, serde_json::to_string(TRACES)),
        // `state schedules`: its own scheduled jobs, and their revision.
        Given::StateSchedules(given) => {
            let schedules = schedules::export_root(&PathBuf::from(given.root))?;
            answer(out, serde_json::to_string(&schedules))
        }
        Given::StateClearSchedules(given) => {
            schedules::clear_root(&PathBuf::from(given.root), &given.revision)?;
            Ok(Done::shown())
        }
        // `state export`: who the agent is and what it remembers, in
        // ASP's portable forms, for a switch away from it.
        Given::StateExport(_) => {
            let home = link_files::home_dir();
            let state = link_harness_hub::Store::export(
                &state::OpenClaw { home: home.clone() },
                &link_harness_contract::asp::Kind::ALL,
            )?;
            let state = link_harness_hub::export_personal(
                &home,
                "openclaw",
                prefix().join("link-threads.json"),
                state,
            )?;
            answer(out, serde_json::to_string(&state))
        }
        Given::StateShow(_) => answer(out, serde_json::to_string(&state())),
        Given::OpenclawEndpoint(_) => {
            let url = format!(
                "http://127.0.0.1:{}{}",
                endpoint_port(),
                link_connection_contract::acp::ACP_PATH
            );
            answer(out, serde_json::to_string(&url))
        }
        // It speaks no connection itself: Link's bridge carries them.
        Given::OpenclawConnections(_) => {
            out.data(b"[]\n");
            Ok(Done::shown())
        }
        Given::OpenclawInternetHosts(_) => answer(out, serde_json::to_string(HOSTS)),
        Given::OpenclawNeeds(_) => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .within("reading settings on stdin")?;
            let settings: Settings =
                serde_json::from_str(&input).within("reading the settings on stdin")?;
            answer(
                out,
                serde_json::to_string(&needs(&settings, cfg!(target_os = "macos"))),
            )
        }
        // Nothing it links itself: Signal's account is `lnk-signal`'s.
        Given::OpenclawLink(request) => Err(harness::unsupported_link(&request.connection)),
    }
}

/// What OpenClaw needs beyond Link's defaults with `settings`, on macOS
/// (`macos`) or not: any program, as it runs what it installed in its
/// home (Link's bridge carries its connections, so it opens no connection's
/// socket of its own); no sandbox with OpenAI on macOS, where it runs Codex's
/// runtime, which runs `ps`, a setuid program no sandbox runs there; and
/// where its browser's ports are on the machine's loopback
/// ([`browser_open`]), its managed browser, which it runs without unless
/// allowed ([`browser`]).
fn needs(settings: &Settings, macos: bool) -> harness::Needs {
    // Its installer unpacks Node, then runs it.
    let mut needs = harness::Needs {
        install_runs_downloads: true,
        ..Default::default()
    };
    needs
        .needs
        .push(harness::Need::runs(&installing::programs_root(), PROGRAMS));
    if macos && settings.model.provider == "openai" {
        needs.unsandboxed = Some(harness::Why {
            why: "with OpenAI it runs Codex's runtime, which runs ps (setuid)".into(),
        });
    }
    if browser_open(settings, macos) {
        needs.risks.push(harness::Risk {
            name: BROWSER_RISK.into(),
            why: "outside its proxy, its managed browser opens Chrome's debugging port on this \
                  machine's loopback, with no credential: any user or program here could drive it"
                .into(),
            without: Some("OpenClaw runs with its browser off".into()),
        });
    }
    needs
}

/// Why it runs any program: what it installed in its home.
const PROGRAMS: &str = "it runs the Node it installed in its home, and the commands its tools run";

/// The risk OpenClaw's managed browser is, by the name `lnk agent
/// openclaw allow` takes it.
const BROWSER_RISK: &str = "browser";

/// Whether OpenClaw runs with no sandbox with `settings`, on macOS
/// (`macos`) or not: as the user chose, or as it needs with OpenAI on a
/// Mac ([`needs`]).
fn unsandboxed(settings: &Settings, macos: bool) -> bool {
    settings.unsandboxed || (macos && settings.model.provider == "openai")
}

/// Whether, with `settings`, on macOS (`macos`) or not, its managed
/// browser's ports (its extension relay's, and the browsers' debugging
/// ports, which ask for no credential) would be on the machine's
/// loopback: on a Mac, unsandboxed (in the sandbox there the relay's
/// port is refused, and OpenClaw finds no Chrome to start); on Linux,
/// wherever it runs without its proxy: unsandboxed, or with `lan`. Behind
/// its proxy on Linux those ports stay in the sandbox's own network.
fn browser_open(settings: &Settings, macos: bool) -> bool {
    match macos {
        true => unsandboxed(settings, macos),
        false => !settings.proxied,
    }
}

/// OpenClaw's `browser` section, on macOS (`macos`) or not: its managed
/// browser off where its ports would be on the machine's loopback
/// ([`browser_open`]), unless the user allowed it, and on elsewhere.
fn browser(settings: &Settings, macos: bool) -> serde_json::Value {
    let allowed = settings.allowed.iter().any(|a| a == BROWSER_RISK);
    json!({"enabled": !browser_open(settings, macos) || allowed})
}

/// What of `~/.openclaw` is the agent: all of it but its programs (its
/// launcher, its own Node and packages), logs and caches, and `.env`,
/// which `configure` writes from Link's settings.
fn state() -> harness::State {
    let path = |p: &str| format!("~/.openclaw{p}");
    harness::State {
        // signal-cli's account is lnk-signal's to say: `lnk agent move`
        // adds it.
        include: vec![path(""), "~/.lnk/data/agents/context".into()],
        exclude: [
            "/bin",
            "/tools",
            "/lib",
            "/node_modules",
            "/logs",
            "/cache",
            "/.env",
        ]
        .into_iter()
        .map(path)
        .collect(),
    }
}

/// The hosts `install` and `configure` reach, behind their proxy
/// (`lnk-openclaw hosts`): the installer and Node, and npm for OpenClaw.
const HOSTS: &[&str] = &[
    // Its installer.
    "github.com",
    "*.githubusercontent.com",
    // Node, and OpenClaw's own packages.
    "nodejs.org",
    "registry.npmjs.org",
];

/// Where OpenClaw writes when it isn't boxed: its folder, with its
/// settings and secrets, the installer's launcher and checkout, its own
/// gateway service, its locks and logs.
/// The OpenClaw that last wrote the install whose home is `home`
/// (`meta.lastTouchedVersion` in its `openclaw.json`), refused when it's
/// newer than [`VERSION`]: Link would run its state on an older OpenClaw.
fn written(home: &std::path::Path) -> traces::Written {
    let text = std::fs::read_to_string(home.join("openclaw.json")).unwrap_or_default();
    // JSON5 in places, so the key and the string after it, not a parse.
    let version = text.split_once("lastTouchedVersion").and_then(|(_, rest)| {
        // The key's closing quote, if it has one, then its colon.
        let rest = rest.strip_prefix(['"', '\'']).unwrap_or(rest).trim_start();
        let rest = rest.strip_prefix(':')?.trim_start();
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        Some(rest[1..].split_once(quote)?.0.to_string())
    });
    let refused = version
        .as_deref()
        .filter(|v| traces::newer(v, VERSION))
        .map(|v| {
            format!(
                "OpenClaw {v} wrote it, newer than the {VERSION} Link runs, which can't be \
                 sure of reading it: update Link, or run OpenClaw {VERSION}"
            )
        });
    traces::Written { version, refused }
}

const TRACES: &[Trace] = &[
    Trace::new(Kind::Home, "~/.openclaw"),
    Trace::new(Kind::Config, "~/.openclaw/openclaw.json"),
    Trace::new(Kind::Secrets, "~/.openclaw/.env"),
    Trace::new(Kind::Agent, "~/.openclaw/agents/{agent}"),
    Trace::new(Kind::Agent, "~/.openclaw/workspace{-agent}"),
    Trace::new(Kind::Package, "~/openclaw"),
    Trace::new(Kind::Program, "~/.local/bin/openclaw"),
    Trace::new(
        Kind::Service,
        "~/Library/LaunchAgents/ai.openclaw.gateway.plist",
    ),
    Trace::new(
        Kind::Service,
        "~/.config/systemd/user/openclaw-gateway.service",
    ),
    Trace::new(Kind::Log, "/tmp/openclaw"),
    Trace::new(Kind::Lock, "/tmp/openclaw-state-locks-{uid}"),
];

/// Its gateway's port, the hub's after it, and the ones OpenClaw derives
/// from the gateway's: +2 to +4
/// (browser control on +2, when it has a port of its own; the canvas is
/// on the gateway's) and the managed browsers' debugging ports (+11 to
/// +110).
fn ports() -> Vec<u16> {
    let port = port();
    [port, endpoint_port(), port + 2, port + 3, port + 4]
        .into_iter()
        .chain(port + 11..=port + 110)
        .collect()
}

/// OpenClaw's file helper (`@openclaw/fs-safe`) on its own fallbacks:
/// its native one opens files with `openat2`, which the sandbox answers
/// as unsupported (ENOSYS), and fails rather than falling back.
const FS_SAFE: (&str, &str) = ("FS_SAFE_NATIVE_MODE", "off");

/// Its programs' environment, as the gateway and `openclaw acp` run.
fn env() -> std::collections::BTreeMap<String, String> {
    let prefix = prefix();
    let path = format!(
        "{}:{}:/usr/bin:/bin:/usr/sbin:/sbin",
        prefix.join("bin").display(),
        prefix.join("tools/node/bin").display()
    );
    [
        ("PATH".to_string(), path),
        // No announcing itself on the local network (Bonjour/mDNS): the
        // sandbox blocks what that needs anyway, and it warned every 15 s.
        ("OPENCLAW_DISABLE_BONJOUR".into(), "1".into()),
        (FS_SAFE.0.into(), FS_SAFE.1.into()),
    ]
    .into()
}

fn launch() -> Launch {
    Launch {
        argv: vec![installed().display().to_string(), "serve".into()],
        // HOME is its home, so ~/.openclaw is in it. Its locks go to
        // /tmp/openclaw-state-locks-<uid> (a fixed path), which the sandbox
        // allows as /tmp/openclaw*.
        env: env(),
        write: vec![],
        read: vec![],
        ports: ports(),
        // Only its endpoint outside: the gateway, its managed browsers'
        // debugging ports among the rest, is reached only from inside.
        serve: Some(vec![endpoint_port()]),
        services: vec![
            "ai.openclaw.gateway".into(),
            "openclaw-gateway.service".into(),
        ],
        // Its gateway takes about a minute and a half to answer on a Mac
        // mini.
        wait_secs: Some(180),
    }
}

/// Where `install` puts this program, in its home: the sandbox runs
/// programs only from there.
pub(crate) fn installed() -> PathBuf {
    installing::programs_root().join("bin/lnk-openclaw")
}

/// The gateway `serve` started, stopped with it.
struct Gateway(Option<std::process::Child>);

impl Drop for Gateway {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            // OpenClaw owns a SQLite state database. Give its foreground
            // gateway the same Ctrl-C a terminal does so it can release
            // leases and write its clean-close receipt. An immediate
            // `Child::kill` is SIGKILL on Unix; repeated benchmark starts
            // then pay for integrity recovery and can trip its restart-loop
            // breaker.
            // SAFETY: this is the child this adapter started.
            let _ = unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
            let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while std::time::Instant::now() < until
                && child.try_wait().is_ok_and(|status| status.is_none())
            {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "waits for graceful gateway shutdown"
                )]
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            if child.try_wait().is_ok_and(|status| status.is_none()) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

/// Whether the gateway listens on its port: one started here, or one
/// still running since an earlier start, which a second would find the port taken by.
fn gateway_listening() -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port())),
        std::time::Duration::from_secs(1),
    )
    .is_ok()
}

/// Whether this gateway boot reached OpenClaw's usable readiness. Its
/// listener and ACP process can come up while a recovered agent database
/// still refuses every new session. The pinned release records both the
/// early refusal and the later admission in its configured log.
fn gateway_log_usable(text: &str) -> bool {
    // Its terminal renderer writes `[gateway] ready`; the configured
    // JSON log keeps the message itself as `gateway ready`.
    let ready = text.rfind("gateway ready");
    let listening = text.rfind("http server listening");
    let refused = text.rfind("has not completed startup inspection and preparation");
    let recovered = text.rfind("agent database recovered after background inspection");
    match refused {
        None => ready.is_some(),
        Some(refused) => {
            listening.is_some() && recovered.is_some_and(|recovered| recovered > refused)
        }
    }
}

fn gateway_usable_since(path: &Path, from: u64) -> bool {
    use std::io::{Read, Seek};
    let Ok(mut log) = std::fs::File::open(path) else {
        return false;
    };
    if log.seek(std::io::SeekFrom::Start(from)).is_err() {
        return false;
    }
    let mut text = String::new();
    log.read_to_string(&mut text).is_ok() && gateway_log_usable(&text)
}

/// The endpoint: the hub on its port until stopped, with `openclaw acp`
/// behind it, and the gateway it reaches started first whenever it isn't
/// running.
fn serve() -> Result<Done, Report> {
    let files = std::env::var("LNK_FILES").map_err(|_| {
        Report::error(sentence!("The variable LNK_FILES isn't set."))
            .why(sentence!("The program lnk agent start runs this command."))
            .next(cmd!("lnk agent start"), "start the agent")
            .exit(Exit::Usage)
    })?;
    let key = harness::read_env_file(&prefix().join(".env"), "LNK_OPENCLAW_GATEWAY_TOKEN")
        .filter(|k| !k.is_empty());
    let gateway = std::sync::Mutex::new(Gateway(None));
    let acp = link_harness_hub::spawning({
        let files = files.clone();
        move || {
            let mut gateway = gateway.lock().unwrap_or_else(|e| e.into_inner());
            if !gateway_listening() {
                let log = prefix().join("logs/openclaw.log");
                let log_from = std::fs::metadata(&log).map_or(0, |meta| meta.len());
                let running = gateway
                    .0
                    .as_mut()
                    .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
                if !running {
                    let mut run = link_secretive::process::command(bin());
                    run.args(["gateway", "run"]).envs(env()).current_dir(&files);
                    match run.spawn() {
                        Ok(child) => gateway.0 = Some(child),
                        Err(err) => link_format::aside(
                            Report::warning(sentence!("OpenClaw's gateway didn't start.")).why(
                                sentence!("The system said: {}.", Foreign::new(&err.to_string())),
                            ),
                        ),
                    }
                }
                // `openclaw acp` hangs up at once on a gateway not yet
                // listening. A listener alone is not usable after an unclean
                // boot either: background database admission can still
                // refuse sessions. Wait for this boot's readiness record and,
                // when needed, its later recovery record.
                let until = std::time::Instant::now() + launch().wait();
                while (!gateway_listening() || !gateway_usable_since(&log, log_from))
                    && std::time::Instant::now() < until
                {
                    let alive = gateway
                        .0
                        .as_mut()
                        .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
                    if !alive {
                        break;
                    }
                    #[expect(clippy::disallowed_methods, reason = "polls for the gateway's port")]
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
            let mut acp = link_secretive::process::command(bin());
            acp.arg("acp").envs(env()).current_dir(&files);
            workarounds::acp_startup::prepare(&mut acp);
            acp
        }
    });
    link_harness_hub::Hub::new(link_harness_hub::Config {
        // OpenClaw's own bug, worked around until its fix is pinned.
        start: workarounds::acp_session_ids::translated(acp),
        cwd: files,
        key: key.map(link_secretive::Secret::from),
        agent: std::env::var("LNK_AGENT").ok().filter(|a| !a.is_empty()),
        sessions: prefix().join("link-threads.json"),
        state: Some(std::sync::Arc::new(state::OpenClaw {
            home: link_files::home_dir(),
        })),
        // `openclaw acp` refuses a session's MCP servers: Link's tools are
        // the gateway's, in `openclaw.json`.
        tools: link_harness_hub::Tools::default(),
    })
    .with_continuity(&link_files::home_dir())?
    .serve(endpoint_port())?;
    Ok(Done::shown())
}

#[cfg(test)]
mod tests {
    use link_secretive::Duplicate;

    use super::*;

    #[test]
    fn an_unadvertised_native_connection_is_refused_by_the_real_handler() {
        use clap::FromArgMatches;
        let parser = link_openclaw::cli::command();
        let matches = parser
            .clone()
            .try_get_matches_from(["lnk-openclaw", "connections"])
            .unwrap();
        let parsed = link_openclaw::cli::Cli::from_arg_matches(&matches).unwrap();
        let (mut out, captured) = Out::capture(false, &["lnk-openclaw", "connections"]);
        assert!(dispatch(&mut out, parsed.command).is_ok());
        assert_eq!(captured.stdout(), "[]\n");
        for again in [false, true] {
            let mut argv = vec!["lnk-openclaw", "link", "signal"];
            if again {
                argv.push("--again");
            }
            let matches = parser.clone().try_get_matches_from(&argv).unwrap();
            let parsed = link_openclaw::cli::Cli::from_arg_matches(&matches).unwrap();
            let (mut out, captured) = Out::capture(false, &argv);
            let report =
                dispatch(&mut out, parsed.command).expect_err("unsupported native link succeeded");
            assert_eq!(report.exit, Exit::Usage);
            assert!(captured.stdout().is_empty());
        }
    }

    #[test]
    fn a_gateway_is_ready_only_after_its_database_is_admitted() {
        assert!(!gateway_log_usable("gateway http server listening"));
        assert!(gateway_log_usable("gateway ready"));
        assert!(!gateway_log_usable(
            "has not completed startup inspection and preparation\ngateway ready"
        ));
        assert!(gateway_log_usable(
            "has not completed startup inspection and preparation\n\
             http server listening\ngateway ready\n\
             agent database recovered after background inspection"
        ));
        // A recovered restart can omit the one-time `gateway ready`
        // record, but its admitted database is usable once the listener
        // for that boot is present.
        assert!(gateway_log_usable(
            "has not completed startup inspection and preparation\n\
             http server listening\n\
             agent database recovered after background inspection"
        ));
    }

    /// The version that last wrote an install is read from its settings,
    /// JSON or JSON5, and a newer one than Link runs is refused.
    #[test]
    fn its_version_is_read_and_a_newer_refused() {
        let dir = link_test_kit::TempDir::new("written");
        assert_eq!(written(&dir), traces::Written::default());
        for (text, version) in [
            (
                r#"{"meta": {"lastTouchedVersion": "2026.1.2"}}"#,
                "2026.1.2",
            ),
            ("{ meta: { lastTouchedVersion: '2099.1.1' } }", "2099.1.1"),
        ] {
            std::fs::write(dir.join("openclaw.json"), text).unwrap();
            let said = written(&dir);
            assert_eq!(said.version.as_deref(), Some(version));
            assert_eq!(said.refused.is_some(), version == "2099.1.1", "{said:?}");
        }
    }

    /// Its file helper runs on its own fallbacks, in the gateway and in
    /// each `openclaw` command Link runs: natively it opens with
    /// `openat2`, which the sandbox answers as unsupported, and fails.
    #[test]
    fn its_file_helper_never_needs_openat2() {
        let given = |command: &std::process::Command| {
            command
                .get_envs()
                .find(|(k, _)| *k == "FS_SAFE_NATIVE_MODE")
                .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(
            launch().env.get("FS_SAFE_NATIVE_MODE").map(String::as_str),
            Some("off")
        );
        assert_eq!(given(&configuring::openclaw()).as_deref(), Some("off"));
    }

    #[test]
    fn it_needs_its_programs_and_no_sandbox_for_openai_on_a_mac() {
        let mut settings = Settings::default();
        settings.model.provider = "anthropic".into();
        // Any program, whatever else.
        let only_programs = harness::Needs {
            needs: vec![harness::Need::runs(&installing::programs_root(), PROGRAMS)],
            install_runs_downloads: true,
            ..Default::default()
        };
        assert_eq!(needs(&settings, true), only_programs);
        // Slack and Discord ask nothing: Link's bridge carries them.
        settings.slack = Some(harness::Slack::default());
        settings.discord = Some(harness::Discord::default());
        settings.proxied = true;
        assert_eq!(needs(&settings, false), only_programs);
        settings.proxied = false;
        settings.model.provider = "openai".into();
        assert!(needs(&settings, true).unsandboxed.is_some());
        // Linux runs `ps` in its sandbox.
        settings.proxied = true;
        assert_eq!(needs(&settings, false), only_programs);
    }

    /// Outside its proxy, its managed browser's ports ask for no
    /// credential on the machine's loopback: it's off unless the user
    /// allowed it.
    #[test]
    fn outside_its_proxy_its_browser_is_off_unless_allowed() {
        let mut settings = Settings::default();
        settings.model.provider = "anthropic".into();
        settings.proxied = true;
        // Sandboxed, behind its proxy: on, and nothing asked.
        for macos in [true, false] {
            assert!(needs(&settings, macos).risks.is_empty());
            assert_eq!(browser(&settings, macos), json!({"enabled": true}));
        }
        // On a Mac in the sandbox, behind its proxy or not: on.
        let mut mac = settings.duplicate();
        mac.proxied = false;
        assert!(needs(&mac, true).risks.is_empty());
        assert_eq!(browser(&mac, true), json!({"enabled": true}));
        // Unsandboxed as the user chose, anywhere, with OpenAI on a Mac,
        // and on Linux with `lan` (granted already, or from a Link that
        // doesn't say): asked, as one it runs without, and off until allowed.
        let mut openai = settings.duplicate();
        openai.model.provider = "openai".into();
        let mut lan = settings.duplicate();
        lan.proxied = false;
        settings.unsandboxed = true;
        settings.proxied = false;
        for (settings, macos) in [
            (&mut settings, false),
            (&mut openai, true),
            (&mut lan, false),
        ] {
            let asked = needs(settings, macos);
            asked.checked().unwrap();
            assert_eq!(asked.risks.len(), 1);
            assert_eq!(asked.risks[0].name, BROWSER_RISK);
            assert!(asked.risks[0].without.is_some());
            assert_eq!(browser(settings, macos), json!({"enabled": false}));
            settings.allowed = vec![BROWSER_RISK.into()];
            assert_eq!(browser(settings, macos), json!({"enabled": true}));
        }
        // OpenAI on Linux runs in its sandbox.
        openai.allowed.clear();
        assert!(needs(&openai, false).risks.is_empty());
        assert_eq!(browser(&openai, false), json!({"enabled": true}));
    }
}

#[cfg(test)]
mod pin_tests;

#[cfg(test)]
link_test_kit::program!(unit);
