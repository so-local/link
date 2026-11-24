//! `lnk-harness`: the adapter for Link Harness, and the harness
//! itself (`serve`). `lnk-agent` runs it; see `link_harness_contract::harness`
//! for the commands.
//!
//! It runs from where it's installed, that file alone read into the
//! sandbox, so every agent shares one copy of it; `configure` keeps its
//! settings through its store (`config.json` in its home); the
//! harness serves chat completions on its port, for Link's connections
//! (`endpoint`), and speaks no connection itself. Its tools are the MCP
//! servers in its environment (`$LNK_MEMORY`, `$LNK_TOOL_<NAME>`).

use std::{io::Read, path::PathBuf, sync::Arc};

use anyhow::bail;
use link_agent_spec as agent_spec;
use link_format::{Address, Done, Out, Report, WithinExt, cmd, sentence};
use link_harness::cli::{
    Command, ConversationsForkArgs, ConversationsMoveArgs, HarnessAskArgs, HarnessConversationArgs,
    HarnessServeArgs,
};
use link_harness::{
    agent::{Agent, Agents, Folder},
    config::Config,
    prompt,
    server::{self, Server},
    store::{Files, Store, Way},
    tools,
    turn::Harness,
};
use link_harness_contract::declared;
use link_harness_contract::harness::{self, Launch, Settings};

/// Its port: the first of the block Link gives its agent (`LNK_PORT`),
/// else its own ([`Port`]).
fn port() -> u16 {
    harness::base_port(PORT.default)
}

/// Its port outside Link, as `port.toml` says.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Port {
    default: u16,
}

static PORT: std::sync::LazyLock<Port> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("port.toml")).expect("Link Harness's port")
});

/// This program, as it runs in the sandbox: where it's installed.
fn program() -> anyhow::Result<PathBuf> {
    let me = std::env::current_exe().within("finding this program")?;
    Ok(me.canonicalize().unwrap_or(me))
}

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    let code = link_settings::output::run(try_main);
    // A compact conversation's folds handed on, done before it ends.
    link_store_contract::store::compact::settle();
    code
}

/// A protocol answer for the program that runs this one, as `T`, its
/// declared output: the JSON of `value` and a newline. With `--json` it is
/// the reply.
fn answer_with<T>(out: &mut Out, value: &impl serde::Serialize) -> Result<Done, Report>
where
    T: serde::de::DeserializeOwned + serde::Serialize + link_actions::value::Validate,
{
    if out.is_json() {
        link_actions::answer::reply::<T>(out, value)?;
    } else {
        let line = link_actions::answer::json_line::<T>(value)?;
        out.data(&line);
    }
    Ok(Done::shown())
}

/// What a command that failed says to try: Link starts the harness.
fn failed(why: anyhow::Error) -> Report {
    Report::from(why).next(
        cmd!("lnk agent start"),
        "start the agent this harness runs for",
    )
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    match link_harness::cli::parse() {
        // Nothing to install: it runs from where it is.
        Command::HarnessInstall(_) => Ok(Done::shown()),
        Command::HarnessConfigure(_) => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .within("reading its settings on stdin")?;
            let settings: Settings =
                serde_json::from_str(&input).within("reading settings on stdin")?;
            // Its settings are in its home: the files folder, unset, is
            // the one the settings name.
            let (agent, agents) = agents(Some(settings.files.clone())).map_err(failed)?;
            Files::new(agents)
                .set_config(&agent, &Config::from_settings(&settings))
                .map_err(failed)?;
            Ok(Done::shown())
        }
        // It reaches its model at the proxy's address, holding no key; its
        // endpoint proves it's the harness before it's sent its key, and
        // answers only the agents it runs; it takes one port.
        Command::HarnessContract(_) => {
            let contract = harness::Contract {
                session_content: link_harness::acp::session_content(),
                proof: true,
                // It's an ACP agent: every client reaches it so.
                agent: true,
                // Its one port: its endpoint.
                ports: Some(1),
                // Its default instructions are the spec's.
                instructions: true,
                // It reads who a message is from, and passes its hops on.
                from: true,
                recover: true,
                // It keeps a conversation's tools, narrowed by a list.
                tools: true,
                // It takes a message's images and documents.
                parts: true,
                // It writes only chats/ of the agent's files.
                folders: true,
                // It prints the prompt it sends its model.
                prompt: true,
                // One process may serve several agents of one owner.
                shared: true,
                // It prints a conversation, and moves its conversations to
                // the way its spec keeps them.
                conversations: true,
                conversation_history: true,
                conversation_fork: true,
                // It can start blank, and be bound to one agent later.
                blank: true,
                // Its conversations, given and taken over ASP; its notes
                // the agent's own, where a switch writes what it remembered
                // elsewhere.
                state: link_harness::acp::state_kinds(),
                offline_state: true,
                notes: true,
                // Every feature, read from the agent's spec at each turn.
                features: Some(
                    harness::features::Feature::ALL
                        .into_iter()
                        .map(|f| (f, harness::features::Honor::Spec))
                        .collect(),
                ),
                ..harness::Contract::new(true)
            };
            answer_with::<declared::Contract>(out, &contract)
        }
        Command::HarnessCommand(_) => {
            answer_with::<declared::Launch>(out, &launch(program().map_err(failed)?, drivers()))
        }
        Command::HarnessAsk(given) => ask(out, given),
        Command::HarnessHealth(_) => match harness::http_ok(port(), "/health") {
            true => Ok(Done::shown()),
            false => Err(Report::error(sentence!(
                "The harness doesn't answer on port {}.",
                Address::local(port())
            ))
            .next(
                cmd!("lnk agent start"),
                "start the agent this harness runs for",
            )),
        },
        // Where its ACP is reached: a WebSocket at this address.
        Command::HarnessEndpoint(_) => {
            let url = format!(
                "http://127.0.0.1:{}{}",
                port(),
                link_connection_contract::acp::ACP_PATH
            );
            answer_with::<declared::Endpoint>(out, &url)
        }
        // Nothing beyond Link's defaults, whatever the settings.
        Command::HarnessNeeds(_) => answer_with::<declared::Needs>(out, &harness::Needs::default()),
        // It speaks no connection itself: Link carries them.
        Command::HarnessConnections(_) => {
            answer_with::<declared::Connections>(out, &Vec::<()>::new())
        }
        Command::HarnessTraces(_) => answer_with::<declared::Traces>(out, &Vec::<()>::new()),
        // Its conversations and instructions are in the agent's files,
        // which move anyway; its home holds what `configure` writes, and
        // the tools' lists, derived.
        Command::StateShow(_) => answer_with::<declared::State>(out, &harness::State::default()),
        Command::StateExport(_) => {
            let (agent, agents) = agents(None).map_err(failed)?;
            let store = Files::new(agents).without_bridge();
            let state = link_harness::acp::exported_state(
                &store,
                &agent,
                &[link_agent_state_protocol::Kind::Sessions],
            )
            .map_err(failed)?;
            answer_with::<declared::Exported>(out, &state)
        }
        Command::HarnessServe(given) => serve(given).map(|()| Done::shown()).map_err(failed),
        // The prompt its model is sent now, from the store as a turn reads
        // it: exactly, or as JSON with its sections.
        Command::HarnessInstructions(_) => instructions(out),
        // A conversation as a person reads it, wherever it's kept.
        Command::HarnessConversation(given) => conversation(out, given),
        Command::ConversationsList(_) => conversations(out, Conversations::List),
        Command::ConversationsFork(given) => conversations(out, Conversations::Fork(given)),
        // Each conversation kept another way than the spec says, moved to
        // its way, the agent stopped.
        Command::ConversationsMove(given) => conversations(out, Conversations::Move(given)),
    }
}

/// `instructions [--json]`: the prompt its model is sent now.
fn instructions(out: &mut Out) -> Result<Done, Report> {
    let (agent, agents) = agents(None).map_err(failed)?;
    let folder = agents.paths(&agent).map_err(failed)?.folder();
    let store = Files::new(agents);
    let config = store.config(&agent).map_err(failed)?;
    // Its tools as a turn last listed them: this process starts none.
    let mut tools: Vec<(String, String)> = store
        .tool_lists(&agent)
        .map(|lists| lists.servers)
        .unwrap_or_default()
        .into_iter()
        .filter(|list| !list.instructions.is_empty() && !list.tools.is_empty())
        .map(|list| (list.name, list.instructions))
        .collect();
    tools.sort();
    // As the owner's conversations are sent it.
    let given = prompt::Given {
        model: Some(&config.model),
        tools: &tools,
        others: false,
    };
    let prompt = prompt::compose(&store, &agent, &config, &folder, given).map_err(failed)?;
    match out.is_json() {
        true => link_actions::answer::reply::<declared::Instructions>(out, &prompt)?,
        false => out.data(prompt.system.as_bytes()),
    }
    Ok(Done::shown())
}

/// `conversation <thread> [--json]`.
fn conversation(out: &mut Out, given: HarnessConversationArgs) -> Result<Done, Report> {
    let thread = given.thread;
    let thread = link_harness::agent::Thread::new(&thread).map_err(failed)?;
    let (agent, agents) = agents(None).map_err(failed)?;
    let store = Files::new(agents).without_bridge();
    if out.is_json() {
        let history = store.history(&agent, &thread).map_err(failed)?;
        link_actions::answer::reply::<declared::History>(out, &history)?;
    } else {
        let text = store.conversation(&agent, &thread).map_err(failed)?;
        out.data(text.as_bytes());
    }
    Ok(Done::shown())
}

/// Which of `conversations list | fork | move` it was given.
enum Conversations {
    List,
    Fork(ConversationsForkArgs),
    Move(ConversationsMoveArgs),
}

/// `conversations list | fork | move`.
fn conversations(out: &mut Out, given: Conversations) -> Result<Done, Report> {
    let (agent, agents) = agents(None).map_err(failed)?;
    let store = Files::new(agents).without_bridge();
    match given {
        Conversations::List => answer_with::<declared::Conversations>(
            out,
            &store.list_conversations(&agent).map_err(failed)?,
        ),
        Conversations::Fork(given) => {
            let thread = link_harness::agent::Thread::new(&given.thread).map_err(failed)?;
            let forked = store
                .fork_conversation(&agent, &thread, given.before)
                .map_err(failed)?;
            answer_with::<declared::History>(out, &forked)
        }
        Conversations::Move(given) => {
            let to = match given.way {
                Some(name) => {
                    if ![agent_spec::LOG, agent_spec::COMPACT].contains(&name.as_str()) {
                        link_store_contract::store::check_name(&name).map_err(failed)?;
                    }
                    Way::named(&name)
                }
                None => store.configured(&agent),
            };
            let moved = store.move_conversations(&agent, &to).map_err(failed)?;
            let mut text = String::new();
            for (thread, from) in &moved {
                text += &format!("{}: {} to {}\n", thread.name(), from.name(), to.name());
            }
            if moved.is_empty() {
                text = format!("Every conversation is kept {} already.\n", to.name());
            }
            out.data(text.as_bytes());
            Ok(Done::shown())
        }
    }
}

/// The programs of the store drivers the agent's conversations are kept
/// by, or its spec names, that are installed: its sandbox reads them, to
/// run them.
fn drivers() -> Vec<PathBuf> {
    let Ok((agent, agents)) = agents(None) else {
        return Vec::new();
    };
    Files::new(agents)
        .drivers(&agent)
        .iter()
        .filter_map(|name| link_core::find_module(link_store_contract::store::module_for(name)))
        .map(|program| program.canonicalize().unwrap_or(program))
        .collect()
}

/// Runs `program`, which its sandbox reads, and nothing beside it: every
/// agent shares the one file, and its pages in memory. It runs `drivers`
/// too, which its sandbox reads.
fn launch(program: PathBuf, drivers: Vec<PathBuf>) -> Launch {
    Launch {
        argv: vec![program.display().to_string(), "serve".into()],
        read: std::iter::once(program).chain(drivers).collect(),
        ports: vec![port()],
        serve: Some(vec![port()]),
        ..Launch::default()
    }
}

/// The agent this process runs for (`LNK_AGENT`, with its ID, owner and
/// service in `LNK_AGENT_ID`, `LNK_OWNER` and `LNK_AGENT_SERVICE` when
/// Link gives them), with its
/// home and its folder: its Home (`LNK_FILES`) and its own
/// (`LNK_AGENT_FOLDER`; unset, its Home holds everything).
fn agents(files: Option<PathBuf>) -> anyhow::Result<(Agent, Agents)> {
    let name = std::env::var("LNK_AGENT").unwrap_or_else(|_| "main".into());
    let id = std::env::var(link_agent_contract::identity::AGENT_ID_ENV);
    let owner = std::env::var(link_agent_contract::identity::OWNER_ENV);
    let agent = match (id, owner) {
        (Ok(id), Ok(owner)) => Agent::with_identity(&name, &id, &owner)?,
        _ => Agent::new(&name)?,
    };
    let agent = match std::env::var(link_agent_contract::identity::SERVICE_ENV) {
        Ok(service) => agent.acting_as(&service)?,
        Err(_) => agent,
    };
    let files = std::env::var_os("LNK_FILES")
        .map(PathBuf::from)
        .or(files)
        .within("LNK_FILES isn't set: lnk agent start runs it")?;
    let own = std::env::var_os(link_harness_contract::harness::AGENT_FOLDER_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| files.clone());
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        link_files::home_dir(),
        Folder {
            home: files,
            agent: own,
        },
    );
    Ok((agent, agents))
}

/// Async workers: a turn waits on its model and its tools, not on these,
/// and its file reads and writes leave them (`block_in_place`), so two
/// serve every thread of an agent; one per core would cost an idle agent
/// a thread and its stack each.
const WORKERS: usize = 2;

/// The harness's runtime: [`WORKERS`] workers, whatever the cores.
/// `ask <thread> [--from <sender>]`: one message, on stdin after the
/// endpoint key's line, asked of the harness on this machine's port
/// over ACP, as a bridge asks it; its answer printed.
fn ask(out: &mut Out, given: HarnessAskArgs) -> Result<Done, Report> {
    let (thread, from) = (given.thread, given.from);
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .within("reading the message on stdin")?;
    let (key, message) = input
        .split_once('\n')
        .within("the endpoint key's line, then the message, on stdin")?;
    let bridge = link_connection_contract::Bridge {
        agent: std::env::var("LNK_AGENT").unwrap_or_default(),
        endpoint: format!(
            "http://127.0.0.1:{}{}",
            port(),
            link_connection_contract::acp::ACP_PATH
        ),
        key: key.trim().to_string().into(),
        proof: true,
        ..Default::default()
    };
    let speaker = from
        .as_deref()
        .map(|from| link_connection_contract::Speaker {
            from,
            name: None,
            hops: 0,
        });
    let (progress, _) = tokio::sync::watch::channel(String::new());
    let asked =
        runtime()
            .within("starting its runtime")?
            .block_on(link_connection_contract::ask_from(
                &bridge,
                &thread,
                message.trim_end_matches('\n'),
                speaker,
                &progress,
            ))?;
    match asked {
        link_connection_contract::Asked::Answer(answer) => {
            out.data(format!("{answer}\n").as_bytes())
        }
        link_connection_contract::Asked::Steered => out.data(b"\n"),
    }
    Ok(Done::shown())
}

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKERS)
        .enable_all()
        .build()
}

/// The harness: answers on its port until stopped. With `--agents
/// <file>`, every agent the file names ([`harness::SharedAgents`]), each
/// on its own port, from this one process; with `--control <control>`, no
/// agent until one is bound ([`blank`]).
fn serve(given: HarnessServeArgs) -> anyhow::Result<()> {
    let mut until_gone = None;
    let (agents, served) = match (
        given.agents.map(PathBuf::from),
        given.control.map(PathBuf::from),
    ) {
        (None, None) => {
            let (agent, mut agents) = agents(None)?;
            link_harness::said::to_log(Some(agent.id()));
            // The tools Link's bridge runs for it, outside its sandbox.
            let servers = tools::from_env(link_secretive::process::variables(tools::names_a_tool));
            // By name: its address holds the path that is its secret.
            for server in &servers {
                link_harness::note!("Tool {}", server.name);
            }
            agents.allow_tools(&agent, servers);
            (agents, vec![(agent, port())])
        }
        (Some(file), None) => {
            // Its agents' lines each name their own.
            link_harness::said::to_log(None);
            until_gone = Some(file.clone());
            shared(&file)?
        }
        (None, Some(control)) => {
            link_harness::said::to_log(None);
            return blank(&control);
        }
        _ => bail!("serve [--agents <file> | --control <control>]"),
    };
    let store = Arc::new(Files::new(agents.clone()));
    let harness = Arc::new(
        Harness::new(agents, store)?.with_interactions(link_interactions::Host::of_harness()?),
    );
    let runtime = runtime()?;
    runtime.block_on(async {
        // Each port taken at once, so one held already fails the start
        // now; listened on only once its tools are listed, so nothing is
        // let in that it wouldn't answer: until then a connection is
        // refused, and its health check, and `lnk agent show`, learn at
        // once that it isn't answering yet.
        let mut claimed = Vec::new();
        for (agent, port) in served {
            claimed.push((agent, claim(port)?, port));
        }
        // Its tools listed before it answers, its health check too: the
        // prompt `instructions` prints is the one its first turn sends.
        futures_util::future::join_all(
            claimed
                .iter()
                .map(|(agent, _, _)| harness.list_tools(agent)),
        )
        .await;
        let mut listening = Vec::new();
        for (agent, socket, port) in claimed {
            listening.push((agent, open(socket, port)?));
        }
        answer(harness, listening, until_gone).await
    })
}

/// A listener on `port` of this machine's loopback (inside its sandbox's
/// own network, when it has one).
async fn listen(port: u16) -> anyhow::Result<tokio::net::TcpListener> {
    open(claim(port)?, port)
}

/// `port` of this machine's loopback taken, nothing let in yet: a
/// connection to it is refused until it's [`open`]ed.
fn claim(port: u16) -> anyhow::Result<tokio::net::TcpSocket> {
    let socket = tokio::net::TcpSocket::new_v4().within("making a socket")?;
    // As `TcpListener::bind` does: a port its last run left in TIME_WAIT
    // is taken again.
    socket.set_reuseaddr(true).within("making a socket")?;
    socket
        .bind(std::net::SocketAddr::from(([127, 0, 0, 1], port)))
        .within(format!("listening on 127.0.0.1:{port}"))?;
    Ok(socket)
}

/// The port [`claim`]ed, listened on: answering from now.
fn open(socket: tokio::net::TcpSocket, port: u16) -> anyhow::Result<tokio::net::TcpListener> {
    let listener = socket
        .listen(1024)
        .within(format!("listening on 127.0.0.1:{port}"))?;
    link_harness::note!("Link Harness answering on 127.0.0.1:{port}");
    Ok(listener)
}

/// Answers each agent of `listening` on its listener, from `harness`,
/// until stopped: a turn a crash left running is finished first. It
/// stops on SIGINT, SIGTERM or SIGHUP, which `lnk` passes on to it, and,
/// given `until_gone`, the agents file a shared harness serves, when that
/// file is removed, as its owner's harness does for a change of its
/// agents. Stopping, it takes no new connection, and those it has are
/// answered until their clients close them, and their turns finish, for
/// at most [`DRAIN`]; then it exits successfully.
#[expect(clippy::disallowed_methods, reason = "the idle sweep's own period")]
async fn answer(
    harness: Arc<Harness>,
    listening: Vec<(Agent, tokio::net::TcpListener)>,
    until_gone: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    // A turn a crash left running is finished: its calls answered as
    // not made again, and the model asked to go on.
    for (agent, _) in &listening {
        let (recovering, agent) = (harness.clone(), agent.clone());
        tokio::spawn(async move { recovering.recover(&agent).await });
    }
    // Tools' sessions no turn has used for a while, or started long
    // ago, end, and their processes with them.
    let idle = harness.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            idle.end_idle_tools().await;
        }
    });
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let mut serving = tokio::task::JoinSet::new();
    for (agent, listener) in listening {
        let server = Arc::new(Server::sharing(harness.clone(), agent));
        // Each piece of an answer goes out at once, not after the
        // connection's delayed ACK: every message opens a new connection.
        let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
            let _ = tcp.set_nodelay(true);
        });
        let mut stopping = stopping.clone();
        serving.spawn(async move {
            axum::serve(listener, server::router(server))
                .with_graceful_shutdown(async move {
                    let _ = stopping.wait_for(|stop| *stop).await;
                })
                .await
        });
    }
    use tokio::signal::unix::{SignalKind, signal};
    let (mut interrupt, mut term, mut hangup) = (
        signal(SignalKind::interrupt()).within("catching SIGINT")?,
        signal(SignalKind::terminate()).within("catching SIGTERM")?,
        signal(SignalKind::hangup()).within("catching SIGHUP")?,
    );
    tokio::select! {
        ended = serving.join_next() => {
            if let Some(ended) = ended {
                ended??;
            }
            return Ok(());
        }
        () = gone(until_gone) => {}
        _ = interrupt.recv() => {}
        _ = term.recv() => {}
        _ = hangup.recv() => {}
    }
    link_harness::note!("Stopping: finishing the turns being answered.");
    let _ = stop.send(true);
    let drained = tokio::time::timeout(DRAIN, async {
        while let Some(ended) = serving.join_next().await {
            ended??;
        }
        // An ACP connection is a WebSocket, which no longer counts as the
        // server's once upgraded: each closed by its client, and each
        // prompt answered, too. A bridge asks on a connection it opened
        // just before, so one opened before the stop is asked and
        // answered here, never cut between its opening and its prompt.
        while harness.answers() > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        anyhow::Ok(())
    })
    .await;
    // A turn still running, such as a summary after its answer, is cut
    // off: its lease let go, so the next process takes the conversation
    // at once and finishes it, never waiting out the lease's lapse.
    let cut = harness.let_go();
    if cut > 0 {
        link_harness::note!(
            "Stopping: {cut} turn(s) still running end here, their conversations let go."
        );
    }
    match drained {
        Ok(ended) => ended,
        Err(_) => {
            link_harness::note!(
                "Stopping: a turn still running after {}s ends here.",
                DRAIN.as_secs()
            );
            Ok(())
        }
    }
}

/// Returns once `file` is removed; never without one.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a file's removal, which nothing announces"
)]
async fn gone(file: Option<std::path::PathBuf>) {
    let Some(file) = file else {
        return std::future::pending().await;
    };
    while file.exists() {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// How long a stopping harness lets the turns it's answering finish.
const DRAIN: std::time::Duration = std::time::Duration::from_secs(30);

/// Started blank, for a machine that runs an agent only while it
/// answers: its runtime up, nothing of any agent's loaded and no port
/// answering, it listens on the Unix socket `control` (mode 0600) for one
/// [`harness::Bind`] line, answered with [`harness::Bound`]. A line it
/// can't bind with is refused, and it stays blank; once bound, it answers
/// that agent alone, as `serve` does, and refuses any other `Bind`. There
/// is no unbinding: the process ends when the agent sleeps.
fn blank(control: &std::path::Path) -> anyhow::Result<()> {
    let runtime = runtime()?;
    runtime.block_on(async {
        let listener = link_sandbox_bridge::socket::listen_async(
            control,
            link_sandbox_bridge::socket::Given::default(),
        )?;
        link_harness::note!("Link Harness started blank, bound at {}", control.display());
        let (harness, agent, port, mut stream) = loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let bound = match read_bind(&mut stream).await {
                Ok(bind) => bound(bind),
                Err(err) => Err(err),
            };
            match bound {
                Ok((harness, agent, port)) => break (harness, agent, port, stream),
                Err(err) => {
                    let refused = harness::Bound {
                        error: Some(format!("{err:#}")),
                    };
                    let _ = write_bound(&mut stream, &refused).await;
                }
            }
        };
        let listening = match listen(port).await {
            Ok(listening) => listening,
            Err(err) => {
                let refused = harness::Bound {
                    error: Some(format!("{err:#}")),
                };
                let _ = write_bound(&mut stream, &refused).await;
                return Err(err);
            }
        };
        // Answered once its port is: the agent can be sent its first
        // message at once.
        write_bound(&mut stream, &harness::Bound::default()).await?;
        link_format::log::set_agent(agent.id());
        link_harness::note!("Link Harness bound to agent {}", agent.name());
        let name = agent.name().to_string();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_bind(&mut stream).await;
                let refused = harness::Bound {
                    error: Some(format!(
                        "this harness is bound to agent {name}, once: start another for another"
                    )),
                };
                let _ = write_bound(&mut stream, &refused).await;
            }
        });
        answer(harness, vec![(agent, listening)], None).await
    })
}

/// The [`harness::Bind`] line on `stream`, at most
/// [`harness::BIND_MAX`] of it.
async fn read_bind(stream: &mut tokio::net::UnixStream) -> anyhow::Result<harness::Bind> {
    use tokio::io::AsyncBufReadExt;
    let mut line = String::new();
    tokio::io::BufReader::new(tokio::io::AsyncReadExt::take(stream, harness::BIND_MAX))
        .read_line(&mut line)
        .await
        .within("reading its binding")?;
    anyhow::ensure!(line.ends_with('\n'), "its binding isn't a line of JSON");
    serde_json::from_str(&line).within("reading its binding")
}

async fn write_bound(
    stream: &mut tokio::net::UnixStream,
    bound: &harness::Bound,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    stream
        .write_all(format!("{}\n", serde_json::to_string(bound)?).as_bytes())
        .await?;
    Ok(())
}

/// The harness for the agent `bind` names, and its port: its identity
/// from the binding, its endpoint's key the binding's, its settings those
/// it was bound with, held in memory (no settings file is read or
/// written), its conversations' driver called through the store bridge
/// at `bind.store`, and its tools those its settings name.
fn bound(bind: harness::Bind) -> anyhow::Result<(Arc<Harness>, Agent, u16)> {
    let agent = Agent::with_identity(&bind.agent.name, &bind.agent.id, &bind.agent.owner)?;
    anyhow::ensure!(
        !bind.key.is_empty() && !bind.key.expose().chars().any(char::is_control),
        "its binding has no key"
    );
    let settings = &bind.settings;
    anyhow::ensure!(
        settings.files.is_absolute(),
        "its binding's files folder isn't an absolute path"
    );
    let mut config = Config::from_settings(settings);
    config.endpoint_key = Some(bind.key.duplicate());
    // Its conversations' driver: their place, where its machine keeps
    // them off the machine; else as its settings or its spec name it.
    // Called through the bridge it was bound with; none bound, none.
    let placed = settings.storage.as_ref().and_then(|storage| {
        let places = &storage.places;
        places
            .elsewhere(link_store_contract::storage::Collection::Conversations)
            .then(|| {
                places
                    .of(link_store_contract::storage::Collection::Conversations)
                    .to_string()
            })
    });
    let driver = placed
        .or_else(|| settings.store.as_ref().map(|at| at.driver.clone()))
        .or_else(|| {
            let way = config
                .opinions()
                .storage
                .unwrap_or_default()
                .conversations()
                .to_string();
            (![agent_spec::LOG, agent_spec::COMPACT].contains(&way.as_str())).then_some(way)
        });
    // The storage bridge: the socket its settings name, else the one
    // it was bound with.
    if let Some(storage) = config.storage.as_mut()
        && storage.socket.is_none()
    {
        storage.socket = bind.store.clone();
    }
    let socket = config
        .storage
        .as_ref()
        .and_then(|s| s.socket.clone())
        .or_else(|| bind.store.clone());
    config.store = match (socket, driver) {
        (Some(socket), Some(driver)) => {
            link_store_contract::store::check_name(&driver)?;
            Some(link_store_contract::store::BridgeAt { driver, socket })
        }
        _ => None,
    };
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        link_files::home_dir(),
        Folder {
            home: settings.files.clone(),
            agent: settings
                .agent
                .clone()
                .unwrap_or_else(|| settings.files.clone()),
        },
    );
    let servers = tools::from_env(tool_env(settings));
    for server in &servers {
        link_harness::note!("Tool {}", server.name);
    }
    agents.allow_tools(&agent, servers);
    let store = Arc::new(Files::new(agents.clone()).holding(&agent, config));
    Ok((
        Arc::new(
            Harness::new(agents, store)?.with_interactions(link_interactions::Host::of_harness()?),
        ),
        agent,
        port(),
    ))
}

/// The tools `settings` name, as the environment a start would have
/// given them in: memory at `$LNK_MEMORY`, `run`, `file` and `code` at
/// theirs, each other at `$LNK_TOOL_<NAME>`.
fn tool_env(settings: &Settings) -> Vec<(String, String)> {
    settings
        .mcp_servers()
        .into_iter()
        .map(|(name, url)| {
            let key = match name.as_str() {
                "memory" => link_memory_tool_contract::MEMORY_ENV.to_string(),
                "run" => link_harness_contract::harness::RUN_ENV.to_string(),
                "file" => link_harness_contract::harness::FILE_ENV.to_string(),
                "code" => link_harness_contract::harness::CODE_ENV.to_string(),
                _ => format!("LNK_TOOL_{}", name.to_ascii_uppercase().replace('-', "_")),
            };
            (key, url)
        })
        .collect()
}

/// The agents of `file` ([`harness::SharedAgents`]), each with its
/// folders and its tools from the environment it would have had on its
/// own, and the port it's answered on.
fn shared(file: &std::path::Path) -> anyhow::Result<(Agents, Vec<(Agent, u16)>)> {
    let read = link_files::saved::parse_json::<harness::SharedAgents>(
        &std::fs::read(file).within(format!("reading {}", file.display()))?,
    )
    .within(format!("reading {}", file.display()))?
    .agents;
    anyhow::ensure!(!read.is_empty(), "{} names no agent", file.display());
    let mut agents = Agents::default();
    let mut served = Vec::new();
    for one in read {
        let agent = Agent::with_identity(&one.name, &one.id, &one.owner)?;
        anyhow::ensure!(
            served
                .iter()
                .all(|(a, p): &(Agent, u16)| *a != agent && *p != one.port),
            "{} names agent {} or port {} twice",
            file.display(),
            one.name,
            one.port
        );
        let own = one.agent.unwrap_or_else(|| one.files.clone());
        agents.add(
            agent.clone(),
            one.home,
            Folder {
                home: one.files,
                agent: own,
            },
        );
        let servers = tools::from_env(one.env);
        for server in &servers {
            link_harness::note!(
                link_harness::said::Ids::agent(agent.id()) =>
                "{}'s tool {} at {}", one.name, server.name, server.url
            );
        }
        agents.allow_tools(&agent, servers);
        served.push((agent, one.port));
    }
    Ok((agents, served))
}

#[cfg(test)]
mod tests {
    /// Its own port, outside Link, is `port.toml`'s: an unprivileged one
    /// with room after it for the block an agent's harness takes.
    #[test]
    fn its_own_port_is_its_files() {
        let port = super::PORT.default;
        assert!(port >= 1024, "{port}");
        assert!(
            port <= u16::MAX - link_harness_contract::harness::PORT_BLOCK,
            "{port}"
        );
    }

    /// An idle agent's harness has its few workers, however many cores
    /// the machine has.
    #[test]
    fn the_runtime_has_its_workers_whatever_the_cores() {
        let runtime = super::runtime().unwrap();
        assert_eq!(runtime.metrics().num_workers(), super::WORKERS);
    }

    /// Every agent runs the program as installed, read-only in its
    /// sandbox, not a copy of its own.
    #[test]
    fn agents_run_the_one_program() {
        let program = std::path::PathBuf::from("/home/me/.lnk/exec/bin/lnk-harness");
        let launch = super::launch(program.clone(), Vec::new());
        assert_eq!(launch.argv[0], program.display().to_string());
        assert_eq!(launch.read, std::slice::from_ref(&program));
        // A store driver's program, read to be run.
        let driver = std::path::PathBuf::from("/usr/local/bin/lnk-sqlite-store");
        let launch = super::launch(program.clone(), vec![driver.clone()]);
        assert_eq!(launch.read, [program, driver]);
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
