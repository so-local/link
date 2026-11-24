//! `lnk-scheduler-tool`: Link scheduler, subagents and schedules for any
//! harness that reads who a message is from (`link_scheduler_tool`):
//!
//! ```sh
//! lnk-scheduler-tool serve --state <dir> [caps] [teams] [--driver address --socket <socket> --as <agent id>] [--store-bridge <socket>]  # the tool, MCP over stdio
//! lnk-scheduler-tool run --state <dir> --way-in <socket> --as <agent id> [--keyring <dir>] [caps] [teams] [--way-in-of <id>=<socket>]... [--team-relay <url> --team-owner <team>=<account>...] [--driver service|cron|address] [--socket <socket>] [--store-bridge <socket>]
//! lnk-scheduler-tool run --agents <file> --time-zone <zone>      # one runner for many agents
//! lnk-scheduler-tool cron install --label <name> <run's flags>   # the calendar wakes `run --driver cron`
//! lnk-scheduler-tool cron uninstall --label <name>
//! ```
//!
//! The runner's driver is `service` (it runs for good, from the agent's
//! bridge, and ends once asked by a SIGTERM or once the process that
//! started it is gone, as `run --agents` does) unless `--driver cron` says the machine's calendar wakes it
//! once a minute (`cron install`): it fires what's due and stops. On a
//! machine that runs the agent only while it answers, `--driver address`
//! with the machine's `--socket`: `serve`, after each
//! call that changes the jobs, and `run`, after it fired what's due and
//! stopped, tell the machine when they're next due, proved as the agent
//! (`--as`), and the machine runs `run` again then. There the jobs are
//! kept off the machine, through its store bridge (`--store-bridge`, a
//! Unix socket), each call proved as the agent, so a sleeping agent's
//! jobs outlive it, and whichever machine it's bound to next reads them;
//! `--state` keeps only what's the machine's (the runners' and the
//! posts' locks). With
//! `--agents`, one runner fires for every agent its file lists, each by
//! the flags it would be run with alone, all in one time zone (Link's
//! bridge for all of a machine's agents runs one a zone).
//!
//! The runner fires as its agent alone (`--as <agent id>`): each firing
//! is a bridge's call, proved with the agent's key from the machine's
//! keyring (`--keyring`, else Link's), read at each firing.
//!
//! An agent on a team is given its ID (`--agent-id`), its teams
//! (`--team-id <name>=<id>`) and their other members
//! (`--teammate <team>/<name>=<agent id>`), and the runner each
//! teammate's way in (`--way-in-of`): whom it may send to is Link's to
//! say, never this program's. A teammate that isn't on this machine, on
//! a machine that carries no message itself, is reached through the team
//! relay (`--team-relay <url>`) of each team whose owner it's given
//! (`--team-owner <team>=<account>`), signed in as the agent with its own
//! token; what the relay kept for the agent comes in at each look.
//!
//! The caps are the agent's spec's (`[tool.scheduler]`): `--per-day`,
//! `--subagents`, `--hops`, `--late run|skip` and `--time-zone local|<zone>`
//! (its times' time zone), each required, and the runner's `--timeout`
//! (how long it waits on a firing), so the numbers are never this
//! program's.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use link_format::{Done, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence};
use link_scheduler_tool::cli::{self, Command, Flags, FlagsDriver, FlagsLate};
use link_scheduler_tool::{
    Address, Addressed, Caller, Caps, Cron, Driver, Relayed, Store, TeamRelay, Teams, WayIn, Ways,
    calendar::Calendar, mcp, run, run_while, set_zone, store::Bridge,
};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

/// What wakes the runner (`--driver`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wakes {
    /// It runs for good (`service`).
    Service,
    /// The machine's calendar, once a minute (`cron`).
    Cron,
    /// The machine, told at its address when the jobs are next due
    /// (`address`).
    Address,
}

/// What `serve` and `run` take.
struct Args {
    state: PathBuf,
    definitions: Option<PathBuf>,
    way_in: Option<PathBuf>,
    caps: Caps,
    teams: Teams,
    /// Each teammate's way in, by its agent's ID.
    ways: std::collections::BTreeMap<String, PathBuf>,
    /// The `--driver`.
    driver: Wakes,
    /// The machine's socket (`--socket`), on the address driver.
    address: Option<PathBuf>,
    /// The team relay's address (`--team-relay`), for teammates off this
    /// machine, and each team's owner by its name (`--team-owner`).
    relay: Option<String>,
    owners: std::collections::BTreeMap<String, String>,
    /// Its times' time zone (`--time-zone`), set for the process by the
    /// command ([`set_zone`]).
    zone: String,
    /// The agent it fires as (`--as`), and the keyring its key is in
    /// (`--keyring`).
    caller: Option<String>,
    keyring: Option<PathBuf>,
    /// The store bridge its jobs are kept through (`--store-bridge`), in
    /// place of the folder.
    store_bridge: Option<PathBuf>,
}

fn args(given: &Flags) -> anyhow::Result<Args> {
    let driver = given.driver();
    let given = given.clone();
    let mut teams = Teams {
        agent: given.agent_id,
        ..Teams::default()
    };
    for team in &given.team_id {
        teams.add_team(team)?;
    }
    for teammate in &given.teammate {
        teams.add_teammate(teammate)?;
    }
    let mut ways = std::collections::BTreeMap::new();
    for arg in &given.way_in_of {
        let (agent, socket) = arg
            .split_once('=')
            .context("--way-in-of is <agent id>=<socket>")?;
        link_machine::machine::check_id(agent)?;
        ways.insert(agent.to_string(), PathBuf::from(socket));
    }
    let mut owners = std::collections::BTreeMap::new();
    for arg in &given.team_owner {
        let (team, owner) = arg
            .split_once('=')
            .context("--team-owner is <team name>=<its owner's account>")?;
        link_agent_place::relay::check_account(owner)?;
        owners.insert(team.to_string(), owner.to_string());
    }
    let driver = match driver {
        FlagsDriver::Service => Wakes::Service,
        FlagsDriver::Cron => Wakes::Cron,
        FlagsDriver::Address => Wakes::Address,
    };
    let timeout = match &given.timeout {
        Some(given) => Some(Some(given.get()).filter(|t| !t.is_zero()).context(Usage)?),
        None => None,
    };
    let late = given.late.map(|late| matches!(late, FlagsLate::Run));
    let (state, definitions, way_in) = (given.state, given.definitions, given.way_in);
    let (per_day, subagents, hops, zone) =
        (given.per_day, given.subagents, given.hops, given.time_zone);
    let (address, relay, store_bridge) = (given.socket, given.team_relay, given.store_bridge);
    let (caller, keyring) = (given.r#as, given.keyring);
    let zone = zone.context(Usage)?;
    teams.check()?;
    anyhow::ensure!(
        (driver == Wakes::Address) == address.is_some(),
        "--driver address and --socket <socket>, the machine's, go together"
    );
    anyhow::ensure!(
        relay.is_none() || (driver != Wakes::Address && teams.agent.is_some()),
        "--team-relay is for an agent with --agent-id whose machine carries no message itself \
         (no --driver address)"
    );
    for team in owners.keys() {
        anyhow::ensure!(
            teams.teams.contains_key(team),
            "--team-owner names team {}, which no --team-id names",
            link_format::clean(team)
        );
    }
    anyhow::ensure!(
        store_bridge.is_none() || caller.is_some(),
        "--store-bridge is called as the agent --as names"
    );
    let caps = Caps {
        per_day: per_day.context(Usage)?,
        subagents: subagents.context(Usage)?,
        hops: hops.context(Usage)?,
        late_runs: late.context(Usage)?,
        interval: given.interval.map(|interval| interval.get()),
        timeout,
    };
    Ok(Args {
        definitions,
        state: state.context(Usage)?,
        way_in,
        caps,
        teams,
        ways,
        driver,
        address,
        relay,
        owners,
        zone,
        caller,
        keyring,
        store_bridge,
    })
}

/// The store: its jobs through the store bridge with `--store-bridge`,
/// proved as agent `--as` with its key from `--keyring`'s; else in
/// `--state`, which holds the rest either way.
fn project(args: &Args, store: &Store) -> anyhow::Result<()> {
    if let Some(path) = &args.definitions {
        let definitions: link_scheduler_tool_contract::definitions::Definitions =
            link_files::saved::parse_json(&std::fs::read(path)?)?;
        link_scheduler_tool::portable::install(
            store,
            &definitions.schedules,
            link_scheduler_tool::now(),
        )?;
    }
    Ok(())
}

fn store(args: &Args) -> anyhow::Result<Store> {
    let store = match &args.store_bridge {
        Some(socket) => Store::with(&args.state, Arc::new(Bridge::new(socket, caller(args)?))),
        None => Store::new(&args.state),
    }?;
    Ok(store
        .with_interactions(link_interactions::Store::machine()?)
        .with_interaction_agent(args.teams.agent.clone().or_else(|| args.caller.clone())))
}

/// Who it acts as: the agent `--as` names, its key in `--keyring`'s.
fn caller(args: &Args) -> anyhow::Result<Caller> {
    Ok(Caller {
        agent: args.caller.clone().context(Usage)?,
        keyring: match &args.keyring {
            Some(dir) => link_sandbox_binding::Keyring::at(dir),
            None => link_sandbox_binding::Keyring::here(),
        },
    })
}

/// The machine's address, on the address driver, posted to as the agent.
fn address(args: &Args) -> anyhow::Result<Option<Address>> {
    match &args.address {
        Some(socket) => Ok(Some(Address {
            socket: socket.clone(),
            caller: caller(args)?,
        })),
        None => Ok(None),
    }
}

/// The runner's driver: the agent's bridge's way in, and each
/// teammate's; woken by the calendar with `--driver cron`, by the
/// machine with `--driver address`.
fn driver(args: &Args) -> anyhow::Result<Arc<dyn Driver>> {
    let way_in = args.way_in.clone().context(Usage)?;
    args.caps.timeout.context(Usage)?;
    let caller = caller(args)?;
    let way = |socket: &PathBuf| WayIn {
        socket: socket.clone(),
        caller: caller.clone(),
    };
    let ways: Arc<dyn Driver> = Arc::new(Ways {
        own: way(&way_in),
        teammates: args
            .ways
            .iter()
            .map(|(agent, socket)| (agent.clone(), way(socket)))
            .collect(),
    });
    let ways = match &args.relay {
        Some(url) => {
            let agent = args.teams.agent.clone().context(Usage)?;
            let tokens = Arc::new(link_scheduler_tool::InstallationTokens::new(
                &agent,
                &TeamRelay::host(url)?,
            ));
            let teams =
                args.owners
                    .iter()
                    .map(|(name, owner)| link_agent_place::relay::TeamIndex {
                        owner: owner.clone(),
                        team: args.teams.teams[name].clone(),
                    });
            Arc::new(Relayed(ways, TeamRelay::new(url, &agent, teams, tokens)?))
        }
        None => ways,
    };
    Ok(match (args.driver, address(args)?) {
        (Wakes::Cron, _) => Arc::new(Cron(ways)),
        (Wakes::Address, Some(address)) => Arc::new(Addressed(ways, address)),
        _ => ways,
    })
}

/// `cron install`: the machine's calendar starts `run --driver cron`
/// with `flags` once a minute, as `label`, its output in `cron.log` of
/// the state; `cron uninstall --label <name>` stops it.
fn cron_install(label: String, flags: Flags) -> anyhow::Result<()> {
    let calendar = Calendar::of_machine();
    let args = args(&flags)?;
    set_zone(&args.zone)?;
    driver(&args)?;
    anyhow::ensure!(
        args.driver == Wakes::Service,
        "cron install starts 'run --driver cron' itself"
    );
    // Cron starts it from the user's home.
    let paths = [
        &args.state,
        args.way_in.as_ref().unwrap_or(&args.state),
        args.keyring.as_ref().unwrap_or(&args.state),
    ];
    for path in paths.into_iter().chain(args.ways.values()) {
        anyhow::ensure!(
            path.is_absolute(),
            "{} isn't an absolute path: the calendar starts the runner from your home",
            path.display()
        );
    }
    store(&args)?;
    let program = std::env::current_exe().context("finding lnk-scheduler-tool")?;
    let mut argv = vec![program.display().to_string()];
    argv.extend(
        cli::SchedulerToolRunArgs {
            agents: None,
            flags: Flags {
                driver: Some(FlagsDriver::Cron),
                ..flags
            },
        }
        .argv(),
    );
    calendar.install(&label, &argv, &args.state.join("cron.log"))
}

/// Set once a SIGTERM asks this runner to end.
static ASKED_TO_END: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn asked_to_end(_: libc::c_int) {
    ASKED_TO_END.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a runner for good is still wanted, as asked now: it ends once
/// a SIGTERM asks it to (its agent's bridge stopping it), or once the
/// process that started it is gone (that bridge killed, and this left
/// behind), so none outlives its agent and the next bridge's is the only
/// one. Either way in order: its firings ended, its mark on the store let
/// go ([`run_while`]).
fn wanted() -> impl Fn() -> bool {
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(
            libc::SIGTERM,
            asked_to_end as extern "C" fn(libc::c_int) as libc::sighandler_t,
        )
    };
    let parent = std::os::unix::process::parent_id();
    move || {
        std::os::unix::process::parent_id() == parent
            && !ASKED_TO_END.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// An agent's runner in [`run_many`]: told to stop, and its thread.
struct Running {
    keep: Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

/// `run --agents <file> --time-zone <zone>`: one runner for many agents, all
/// in time zone `zone`, each listed in `file` ([`link_scheduler_tool_contract::listed::Listed`]) by the flags
/// `run` takes for it alone. The file is read again whenever it changes:
/// an agent added starts, and one gone, or listed with other flags,
/// stops once its firings have ended ([`run_while`]), so none is fired
/// twice. It ends once the file lists none and every agent's ended, or,
/// the same way, once the process that started it is gone.
#[expect(clippy::disallowed_methods, reason = "the scheduler's own period")]
fn run_many(file: &std::path::Path, zone: &str) -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    // Each agent running, by its flags as the line spells them: a key,
    // never read back.
    let mut running: std::collections::BTreeMap<Vec<String>, Running> = Default::default();
    let mut stopping: Vec<std::thread::JoinHandle<()>> = Vec::new();
    let mut read: Option<Vec<Flags>> = None;
    // Asked to end, or its bridge gone (killed, and this left behind):
    // none listed, so the runner the next bridge starts is the only one.
    let wanted = wanted();
    loop {
        let listed: Option<Vec<Flags>> = match std::fs::read(file) {
            _ if !wanted() => Some(Vec::new()),
            // One being written is read at the next look.
            Ok(bytes) => link_files::saved::parse_json::<
                link_scheduler_tool_contract::listed::Listed,
            >(&bytes)
            .ok()
            .map(|listed| listed.agents),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
            Err(err) => return Err(err).with_context(|| format!("reading {}", file.display())),
        };
        if let Some(listed) = listed.filter(|listed| read.as_ref() != Some(listed)) {
            read = Some(listed.clone());
            // Each runner by its run line, as the contract writes it.
            let keys: Vec<Vec<String>> = listed
                .iter()
                .map(|flags| {
                    cli::SchedulerToolRunArgs {
                        agents: None,
                        flags: flags.clone(),
                    }
                    .argv()
                })
                .collect();
            let gone: Vec<Vec<String>> = running
                .keys()
                .filter(|key| !keys.contains(key))
                .cloned()
                .collect();
            for flags in gone {
                if let Some(agent) = running.remove(&flags) {
                    agent.keep.store(false, Ordering::Relaxed);
                    stopping.push(agent.thread);
                }
            }
            for (flags, key) in listed.iter().zip(keys) {
                if running.contains_key(&key) {
                    continue;
                }
                // One that can't run is said, and the rest run.
                let started = args(flags).and_then(|args| {
                    anyhow::ensure!(
                        args.zone == zone,
                        "its zone, {}, isn't this runner's, {}",
                        link_format::clean(&args.zone),
                        link_format::clean(zone)
                    );
                    anyhow::ensure!(
                        args.driver == Wakes::Service,
                        "a runner with --driver {} is woken, never run for good",
                        match args.driver {
                            Wakes::Cron => "cron",
                            _ => "address",
                        }
                    );
                    let driver = driver(&args)?;
                    let store = store(&args)?;
                    project(&args, &store)?;
                    let keep = Arc::new(AtomicBool::new(true));
                    let kept = keep.clone();
                    let thread = std::thread::spawn(move || {
                        let keep = || kept.load(Ordering::Relaxed);
                        if let Err(err) = run_while(&store, driver, &args.caps, &keep) {
                            link_scheduler_tool::warn_why(
                                sentence!(
                                    "The scheduler of {} stopped.",
                                    link_format::Path::new(&args.state)
                                ),
                                &err,
                            );
                        }
                    });
                    Ok(Running { keep, thread })
                });
                match started {
                    Ok(agent) => {
                        running.insert(key, agent);
                    }
                    Err(err) => link_scheduler_tool::warn_why(
                        sentence!("An agent's scheduler isn't run."),
                        &err,
                    ),
                }
            }
        }
        stopping.retain(|thread| !thread.is_finished());
        running.retain(|_, agent| !agent.thread.is_finished());
        let none = read.as_ref().is_some_and(Vec::is_empty);
        if none && running.is_empty() && stopping.is_empty() {
            return Ok(());
        }
        std::thread::sleep(link_scheduler_tool::limits().look_every());
    }
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    if err.is::<Usage>() {
        return Report::usage("lnk-scheduler-tool");
    }
    let program = Product::new("lnk-scheduler-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(_out: &mut Out) -> anyhow::Result<Done> {
    // Its warnings are log lines: it runs with no one to read them.
    link_format::daemon::logs();
    let ended: anyhow::Result<()> = match cli::parse().command.command {
        Command::SchedulerToolServe(cli::SchedulerToolServeArgs { flags }) => {
            // Link says the agent's floor, from its spec and its teams.
            let floor = match flags.floor.context("serve takes the agent's --floor")? {
                cli::FlagsFloor::Owner => link_connection_contract::access::Standing::Owner,
                cli::FlagsFloor::Member => link_connection_contract::access::Standing::Member,
            };
            let args = args(&flags)?;
            set_zone(&args.zone)?;
            anyhow::ensure!(args.driver != Wakes::Cron, "--driver cron is the runner's");
            let tool = mcp::Tool {
                store: store(&args)?,
                caps: args.caps,
                address: address(&args)?,
                teams: args.teams,
                floor,
                connections: flags.connection.clone(),
            };
            mcp::serve(
                &tool,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        Command::SchedulerToolRun(cli::SchedulerToolRunArgs {
            agents: Some(file),
            flags,
        }) => {
            // Its check hook holds `--agents` to `--time-zone`; nothing
            // else is taken with it.
            anyhow::ensure!(
                flags
                    == Flags {
                        time_zone: flags.time_zone.clone(),
                        ..Flags::default()
                    },
                Usage
            );
            let zone = flags.time_zone.context(Usage)?;
            set_zone(&zone)?;
            run_many(&file, &zone)
        }
        Command::SchedulerToolRun(cli::SchedulerToolRunArgs {
            agents: None,
            flags,
        }) => {
            let args = args(&flags)?;
            set_zone(&args.zone)?;
            let driver = driver(&args)?;
            let store = store(&args)?;
            project(&args, &store)?;
            match driver.resident() {
                // For good, while what started it is there.
                true => run_while(&store, driver, &args.caps, &wanted()),
                false => run(&store, driver, &args.caps),
            }
        }
        Command::CronInstall(cli::CronInstallArgs { label, flags }) => cron_install(label, flags),
        Command::CronUninstall(cli::CronUninstallArgs { label }) => {
            Calendar::of_machine().remove(&label)
        }
        // Nothing to install: this program is the scheduler.
        Command::SchedulerToolInstall(_) => Ok(()),
        // A model's calls, made over MCP through serve: no command line
        // reads them.
        Command::SchedulerToolMessage(_)
        | Command::SchedulerToolTask(_)
        | Command::SchedulerToolSchedule(_)
        | Command::SchedulerToolSend(_)
        | Command::SchedulerToolCompact(_)
        | Command::JobList(_)
        | Command::JobShow(_)
        | Command::JobStop(_)
        | Command::ScheduledJobList(_)
        | Command::ScheduledJobDelete(_) => Err(anyhow::anyhow!(
            "the scheduler's calls are a model's, made through serve"
        )),
    };
    ended?;
    Ok(Done::shown())
}

#[cfg(test)]
link_test_kit::program!(unit);
