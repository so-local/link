//! The agent's `run`, `file` and `code` (Link run, `lnk-run-tool`, Link
//! file, `lnk-file-tool`, and Link code, `lnk-code-tool`): programs it
//! runs, files it reads and writes, and the language servers it asks
//! about its code, for a harness that keeps to the agent's folder's
//! layout (`Contract::folders`).
//!
//! Each is a tool defined at a start like memory's (`run-<agent>`,
//! `file-<agent>`, `code-<agent>`, `lnk sandbox tool add`), which the
//! harness calls through its proxy at `$LNK_RUN`, `$LNK_FILE` and
//! `$LNK_CODE`. None runs a call itself: each runs its calls under `lnk
//! sandbox run`, made from the agent's sandbox file. `run` starts one
//! for each call, ended with the call, which reaches what the file grants
//! and runs only its `[commands]`; `file` one for a session's calls,
//! which reaches no network and runs only Link file; `code` one for each
//! language server, which reaches no network and runs only its
//! `[commands]`, its servers among them. Each reads Home and the agent's
//! instructions, skills and conversations in its own folder, and writes
//! only the folders its `[files]` names.
//!
//! With the agent's sandbox off none is defined: code runs only in a
//! sandbox.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::bail;
use link_agent::background;
use link_agent::bridge;
use link_agent::programs;
use link_agent::sandbox_file::{self, Sandbox};
use link_agent::settings::{self, Settings};
use link_agent_spec::CodeServer;
mod context;

/// Link run's module.
pub const RUN: &str = "run-tool";

/// Link file's module.
pub const FILE: &str = "file-tool";

/// Link code's module.
pub const CODE: &str = "code-tool";

/// The tool `which` (`run`, `file` or `code`) of the agent keyed `key`
/// ([`Settings::key`]) is served as.
pub fn tool_of(which: &str, key: &str) -> String {
    format!("{which}-{key}")
}

/// Its record, beside the agent's settings: there while it's defined.
fn record(agent: &Settings, which: &str) -> PathBuf {
    settings::config_of(&agent.name).join(format!("{which}.json"))
}

/// Tool `which` (`run`, `file` or `code`), when it's defined for the agent.
pub fn tool(agent: &Settings, which: &str) -> Option<String> {
    record(agent, which)
        .exists()
        .then(|| tool_of(which, &agent.key()))
}

/// A File definition still running the exact managed native program.
pub(crate) fn read_fingerprint(agent: &Settings, actual: &[String]) -> Option<String> {
    let kept = crate::defined::kept::<crate::defined::Tool>(&record(agent, "file"))?;
    let program = program(FILE).ok()?;
    current_read_fingerprint(&kept, &program, actual)
}

fn current_read_fingerprint(
    kept: &crate::defined::Tool,
    program: &Path,
    actual: &[String],
) -> Option<String> {
    let executable = Path::new(kept.argv.first()?);
    if kept.argv != actual
        || executable.canonicalize().ok()? != program.canonicalize().ok()?
        || kept.modified.is_none()
        || kept.modified != crate::defined::modified(program)
        || kept.lnk != env!("CARGO_PKG_VERSION")
    {
        return None;
    }
    Some(link_digest::sha256(&serde_json::to_vec(actual).ok()?))
}

/// The programs its `run` may start here, as the environment names them:
/// those of its sandbox file's `[commands]` this machine has, or `*`;
/// none when it has no `run`, or they lack the shell its call runs in.
pub fn commands(agent: &Settings) -> Vec<String> {
    match tool(agent, "run") {
        Some(_) => sandbox_file::of(&agent.name)
            .commands()
            .map(|c| c.present(&programs::path(agent)))
            .filter(|present| with_shell(present))
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Whether `present` (the programs a command may start here) hold the
/// shell `run`'s call runs in: without it, `run` offers nothing.
fn with_shell(present: &[String]) -> bool {
    let any = link_harness_contract::harness::Commands {
        allow: present.to_vec(),
        toolchains: Vec::new(),
    }
    .any();
    link_run_tool_contract::shell(present, any).is_some()
}

/// Module `module`'s program, installed first if it's missing (`lnk
/// module add`); for a `lnk` built from source, only one built beside
/// it, never a release's of another version.
pub fn program(module: &str) -> anyhow::Result<PathBuf> {
    if let Some(program) = link_core::find_module(module) {
        return Ok(program);
    }
    if !link_core::RELEASED {
        bail!(
            "lnk-{module} isn't beside lnk: this lnk is built from source, so build Link's tools \
             too (docs/more/developing/install.md in its source)"
        );
    }
    link_core::modules::add(module, "Starting your agent")
}

/// `lnk sandbox run`'s flags for one call over the agent's `folder`:
/// Home and its own folder's instructions, skills and conversations
/// read, the folders of them `sandbox` lets the agent write written (but
/// its conversations, its harness's alone), what it reaches of its teams'
/// folders (`teams`), every other agent's folder and team's hidden; and
/// for `run` ([`Reach::Network`]), what the file grants beyond that: your
/// home read, and its network as the file says.
pub fn sandbox_flags(
    sandbox: &Sandbox,
    folder: &link_agent_contract::folder::Folder,
    teams: &settings::TeamReach,
    reach: Reach,
) -> Vec<String> {
    let mut flags = Vec::new();
    let home = folder.home.display().to_string();
    let folders = sandbox.folders();
    match folders.whole() {
        true => flags.extend(["--files.write".into(), home.clone()]),
        false => flags.extend(["--files.read".into(), home.clone()]),
    }
    // A program `run` or `code` starts reaches no conversation's folder
    // but, per `run` call, its own (`lnk-run-tool serve --conversations`):
    // `chats/` hidden, whatever else grants it. Only `file` reads it all,
    // its server holding each call to its own conversation.
    let conversations = folder.of(link_store_contract::storage::Collection::Conversations);
    let all_chats = matches!(reach, Reach::Files);
    for name in &folders.write {
        if name != link_harness_contract::harness::WHOLE {
            flags.extend([
                "--files.write".into(),
                folder.seen(name).display().to_string(),
            ]);
        }
    }
    // The dot files of those it writes that its sandbox file grants, so
    // its `run` commits as its harness does; never `file`'s, which has no
    // need to write `.git/hooks` or `.envrc`.
    let dot_files = match reach {
        Reach::Files => Vec::new(),
        Reach::Folders | Reach::Network { .. } => sandbox.write_dot_files(),
    };
    for name in dot_files {
        let path = match name == link_harness_contract::harness::WHOLE {
            true => home.clone(),
            false => folder.seen(&name).display().to_string(),
        };
        flags.extend(["--files.write_dot_files".into(), path]);
    }
    for collection in link_agent_contract::folder::SEEN {
        let chats = collection == link_store_contract::storage::Collection::Conversations;
        if chats && !all_chats {
            flags.extend(["--files.hide".into(), conversations.display().to_string()]);
        } else if !folders.write.iter().any(|n| n == collection.directory()) {
            let path = folder.of(collection);
            flags.extend(["--files.read".into(), path.display().to_string()]);
        }
    }
    for folder in &teams.write {
        flags.extend(["--files.write".into(), folder.display().to_string()]);
    }
    for folder in &teams.read {
        flags.extend(["--files.read".into(), folder.display().to_string()]);
    }
    for folder in &teams.hide {
        flags.extend(["--files.hide".into(), folder.display().to_string()]);
    }
    let Reach::Network { upstream } = reach else {
        return flags;
    };
    flags.extend(home_flags(sandbox));
    // Its network as its spec says it: through its proxy, or as it is;
    // of the loopback, the ports it names. With a route, as the agent's
    // own start has it: always behind its proxy, out through the route,
    // the network as it is and the LAN reaching nothing past it.
    let spec = match upstream {
        Some(_) => sandbox.resolve(
            &Default::default(),
            folder,
            link_agent_contract::agent_sandbox::Route::Proxied,
        ),
        None => sandbox.spec.clone(),
    }
    .on(link_sandbox_contract::spec::Os::here())
    .unwrap_or_default();
    if let Some(upstream) = upstream {
        flags.extend(["--upstream".into(), upstream.to_string()]);
    }
    let localhost = spec.localhost.and_then(|l| l.ports).map(|ports| {
        let ports: Vec<_> = ports
            .into_iter()
            .filter(|p| matches!(p, link_sandbox_contract::spec::Port::Number(_)))
            .collect();
        link_sandbox_contract::spec::Localhost {
            ports: (!ports.is_empty()).then_some(ports),
            ..Default::default()
        }
    });
    let reach = link_sandbox_client::Spec {
        network: spec.network,
        internet: spec.internet,
        lan: spec.lan,
        localhost: localhost.filter(|l| l.ports.is_some()),
        // Never its tools: a tool's address is its harness's alone, and a
        // program `run` starts would hold it.
        ..Default::default()
    };
    if let Ok(given) = link_spec::flags_of(&reach) {
        flags.extend(given);
    }
    flags
}

/// What a call of [`sandbox_flags`] reaches beyond the agent's folders.
#[derive(Clone, Copy)]
pub enum Reach<'a> {
    /// Its folders alone, none of their dot files written: `file`.
    Files,
    /// Its folders alone, with the dot files its sandbox file grants:
    /// `code`.
    Folders,
    /// The network as its sandbox file says, out through `upstream`, the
    /// agent's route, when it has one: `run`.
    Network { upstream: Option<&'a str> },
}

impl<'a> Reach<'a> {
    /// `run`'s, out through `agent`'s route when its traffic has one
    /// (an upstream but `direct`).
    pub fn of(agent: &'a Settings) -> Self {
        Reach::Network {
            upstream: route(agent),
        }
    }
}

/// `agent`'s route, when its traffic has one: an upstream but `direct`.
pub fn route(agent: &Settings) -> Option<&str> {
    agent.upstream.as_deref().filter(|u| *u != "direct")
}

/// Your home read, when the sandbox file reads it, every agent's folder
/// hidden in it: where a program `run` or `code` starts may be installed.
fn home_flags(sandbox: &Sandbox) -> Vec<String> {
    match sandbox.reads_home() {
        true => vec![
            "--files.read".into(),
            "~".into(),
            "--files.hide".into(),
            settings::agents().display().to_string(),
        ],
        false => Vec::new(),
    }
}

/// The milliseconds tool `which` (`run`, `code`) gives a command or a
/// server: less than the agent's spec gives the call (`[tool.<which>]
/// timeout`) by its margin ([`margin_ms`]), so what it has comes back
/// before the harness stops waiting.
fn command_ms(agent: &Settings, which: &str) -> anyhow::Result<u64> {
    let spec = agent.spec()?;
    let all = spec.spec.tools.as_ref().and_then(|tools| tools.timeout);
    command_ms_of(which, spec.tool(which).timeout, all)
}

/// [`command_ms`] of tool `which`'s timeout (`own`, its `[tool.<which>]`
/// over `[tools]`) and `[tools] timeout` (`all`). Refused above `all`:
/// its call goes through a namespace that stops waiting at `all`, while
/// the command would run on.
fn command_ms_of(
    which: &str,
    own: Option<link_actions::value::Duration>,
    all: Option<link_actions::value::Duration>,
) -> anyhow::Result<u64> {
    let timeout = own.within(format!(
        "its spec sets no [tool.{which}] timeout or [tools] timeout"
    ))?;
    let ms = link_agent_spec::millis(&timeout);
    if let Some(all) = all {
        let most = link_agent_spec::millis(&all);
        if ms > most {
            bail!(
                "its spec's [tool.{which}] timeout \"{timeout}\" is above its [tools] timeout \
                 \"{all}\", where every call stops waiting: raise [tools] timeout to at least it"
            );
        }
    }
    Ok(ms - margin_ms(ms))
}

/// What `run_file.toml` sets.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RunFile {
    margin: Margin,
}

/// How much sooner than its call's timeout a command or server is
/// stopped: `percent` of it, at least `least_ms`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Margin {
    percent: u64,
    least_ms: u64,
}

/// `run_file.toml`, read once.
fn run_file() -> &'static RunFile {
    static READ: std::sync::LazyLock<RunFile> = std::sync::LazyLock::new(|| {
        let read: RunFile = toml::from_str(include_str!("run_file.toml")).expect("run_file.toml");
        assert!(
            read.margin.percent < 100,
            "run_file.toml's margin is a share"
        );
        read
    });
    &READ
}

/// The milliseconds kept back of a call's `ms` ([`Margin`]): at most
/// half of it, so a command always has the rest.
fn margin_ms(ms: u64) -> u64 {
    let margin = &run_file().margin;
    (ms * margin.percent / 100).max(margin.least_ms).min(ms / 2)
}

/// What `run` keeps of a command's output, in bytes: the agent's spec's
/// (`[tool.run] output`), clamped so both of a command's streams fit the
/// answer its harness keeps of a `task` call (`[tools] answer`) and the
/// tools bridge's message ([`link_run_tool_contract::output_within`]).
fn output_bytes(agent: &Settings) -> anyhow::Result<u64> {
    let spec = agent.spec()?;
    let output = spec
        .tool("run")
        .output
        .within("its spec sets no [tool.run] output")?;
    let asked = link_agent_spec::size(&output)?;
    // Run's calls reach the harness through the `task` namespace, whose
    // answer its spec caps; set nowhere, the harness keeps its least.
    let answer = match spec.tool("task").answer {
        Some(answer) => link_agent_spec::size(&answer)?,
        None => link_agent_spec::ANSWER_LEAST,
    };
    Ok(link_run_tool_contract::output_within(asked, answer))
}

/// `ms` milliseconds, as a tool's flag takes a duration.
fn millis(ms: u64) -> link_actions::value::Duration {
    link_actions::value::Duration::from_std(std::time::Duration::from_millis(ms))
}

/// `run`'s background commands, for a harness that takes messages from
/// others (`from`), laid on `serve`: where they're kept, the bridge's way
/// in that tells the agent each one's end, and the caps, its spec's;
/// none when its spec allows none at once.
fn background(
    agent: &Settings,
    serve: &mut link_run_tool_contract::cli::RunToolServeArgs,
) -> anyhow::Result<()> {
    let spec = agent.spec()?;
    let run = spec.tool("run");
    let missing = |key: &str| format!("its spec sets no [tool.run] {key}");
    let at_once = run.background.within(missing("background"))?;
    // None at once: no background commands offered at all.
    if at_once == 0 {
        return Ok(());
    }
    let timeout = run
        .background_timeout
        .within(missing("background_timeout"))?;
    let output = run.background_output.within(missing("background_output"))?;
    let keep = run.background_keep.within(missing("background_keep"))?;
    let per_day = run
        .background_per_day
        .within(missing("background_per_day"))?;
    let hops = spec
        .tool("scheduler")
        .hops
        .within("its spec sets no [tool.scheduler] hops")?;
    serve.state = Some(background::state(agent));
    serve.way_in = Some(link_connection_contract::way_in(&agent.name));
    // Each ending is told as this agent alone, proved with its key.
    serve.r#as = Some(agent.key());
    serve.background = Some(at_once);
    serve.background_timeout = Some(timeout);
    serve.background_output = Some(link_agent_spec::size(&output)?);
    serve.background_keep = Some(keep);
    serve.background_per_day = Some(per_day);
    serve.hops = Some(hops);
    Ok(())
}

/// Code's servers, as the agent's spec names them (`[tool.code.servers]`):
/// those it may start, each allowed by `allow` (its sandbox file's
/// `[commands]`) and found on `path`, and why each other may not.
pub fn code_servers(
    servers: &BTreeMap<String, CodeServer>,
    allow: &[String],
    path: &str,
) -> (BTreeMap<String, CodeServer>, BTreeMap<String, String>) {
    let mut unavailable = BTreeMap::new();
    for (language, server) in servers {
        let Some(program) = server.command().first() else {
            continue;
        };
        if !allows(allow, program, path) {
            unavailable.insert(
                language.clone(),
                "your sandbox file's [commands] doesn't allow it".to_string(),
            );
        } else if link_harness_contract::harness::find_program(program, path).is_none() {
            unavailable.insert(language.clone(), "this machine doesn't have it".to_string());
        }
    }
    let usable = servers
        .iter()
        .filter(|(language, server)| {
            !server.command().is_empty() && !unavailable.contains_key(*language)
        })
        .map(|(l, s)| (l.clone(), s.clone()))
        .collect();
    (usable, unavailable)
}

/// A sandbox's flags for `toolchains`, the sandbox file's.
fn toolchain_flags(toolchains: &[link_sandbox_contract::toolchains::Toolchain]) -> Vec<String> {
    use link_actions::value::Words;
    toolchains
        .iter()
        .flat_map(|t| ["--commands.toolchains".to_string(), t.word().to_string()])
        .collect()
}

/// A sandbox's flag for the developer tools, when the sandbox file grants
/// them here (`[commands.macos] developer_tools`): every program of the
/// agent's runs with them, a `run` command's and a `code` server's as its
/// harness's, so `/usr/bin/git` and `/usr/bin/clangd` reach the real ones
/// through `xcrun`.
fn developer_tools_flags(sandbox: &Sandbox) -> Vec<String> {
    match sandbox.developer_tools() {
        true => vec!["--commands.developer_tools".to_string()],
        false => Vec::new(),
    }
}

/// Whether `allow` (its sandbox file's `[commands]`) allows `program`,
/// each looked up on `path`: the entry as written, or an entry that
/// is the same file, so a name and its absolute path are one program;
/// any, when `allow` is `["*"]`.
fn allows(allow: &[String], program: &str, path: &str) -> bool {
    if (link_harness_contract::harness::Commands {
        allow: allow.to_vec(),
        toolchains: Vec::new(),
    })
    .any()
    {
        return true;
    }
    let file = |entry: &str| {
        link_harness_contract::harness::find_program(entry, path)
            .and_then(|found| found.canonicalize().ok())
    };
    let wanted = file(program);
    allow
        .iter()
        .any(|entry| entry == program || (wanted.is_some() && file(entry) == wanted))
}

/// The folders `sandbox` lets the agent write, as `lnk-file-tool apply`
/// takes them: `--write <name>` under its files folder, and `--whole` for
/// all of it (`.`), since a bare `.` reads as a path from here, which
/// `lnk sandbox tool add` refuses in a server's command.
fn write_names(sandbox: &Sandbox) -> (bool, Vec<String>) {
    let write = &sandbox.folders().write;
    let whole = write
        .iter()
        .any(|n| n == link_harness_contract::harness::WHOLE);
    let names = write
        .iter()
        .filter(|n| *n != link_harness_contract::harness::WHOLE)
        .cloned()
        .collect();
    (whole, names)
}

/// `file`'s `apply` for the agent's files folder `files` and folder
/// `folder`: the folders its sandbox file lets it write (the tool's own
/// `--write` and `--whole`, no sandbox flag), and its teams' folders at
/// `teams/<team>/` of its paths; each grant laid on `serve` too, which
/// says them to the model in its instructions.
fn file_grants(
    sandbox: &Sandbox,
    teams: &settings::TeamReach,
    files: &Path,
    folder: &link_agent_contract::folder::Folder,
    serve: &mut link_file_tool_contract::cli::FileToolServeArgs,
) -> link_file_tool_contract::cli::FileToolApplyArgs {
    let (whole, write) = write_names(sandbox);
    let mut apply = link_file_tool_contract::cli::FileToolApplyArgs {
        files: files.to_path_buf(),
        agent_folder: folder.agent.clone(),
        write,
        whole,
        team_folder: Vec::new(),
        team_write: Vec::new(),
        team_read: Vec::new(),
    };
    for (team, shared, folders) in &teams.teams {
        apply
            .team_folder
            .push(format!("{team}={}", shared.display()));
        for folder in &folders.write {
            apply.team_write.push(format!("{team}/{folder}"));
        }
        for folder in &folders.read {
            apply.team_read.push(format!("{team}/{folder}"));
        }
    }
    serve.write = apply.write.clone();
    serve.whole = apply.whole;
    serve.team_write = apply.team_write.clone();
    serve.team_read = apply.team_read.clone();
    apply
}

/// Defines the agent's `run`, `file` and `code`, for a harness that keeps
/// to the files folder's layout (`folders`) running `sandboxed`; forgets
/// them otherwise, `run` when its sandbox file allows no shell here,
/// and `code` when it may start no language server here.
/// For a harness that takes messages from others (`from`), `run` offers
/// background commands too. The tools defined.
pub fn define(
    agent: &Settings,
    folders: bool,
    sandboxed: bool,
    from: bool,
) -> anyhow::Result<Vec<String>> {
    let wanted = folders && sandboxed;
    // Each as its spec says: off unless `[tool.<which>] enabled`.
    let spec = agent.spec()?;
    let on = |which: &str| wanted && spec.tool(which).enabled == Some(true);
    let sandbox = sandbox_file::load(&agent.name)?;
    let path = programs::path(agent);
    // Its programs by name, a toolchain's among them, and its toolchains,
    // which each call's sandbox runs whole: their builds' programs too.
    let commands = sandbox
        .commands()
        .map(|c| c.present(&path))
        .unwrap_or_default();
    let toolchains = sandbox
        .commands()
        .map(|c| c.toolchains.clone())
        .unwrap_or_default();
    let files = agent.files();
    let folder = agent.folder();
    crate::settings::check_teams()?;
    let teams = agent.team_reach();
    let lnk = link_core::lnk()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let mut defined = Vec::new();
    // Run: `lnk-run-tool serve`, each call `lnk sandbox run <flags> --
    // <program> <args>`.
    match on("run") && with_shell(&commands) {
        false => forget(agent, "run"),
        true => {
            let program = program(RUN)?;
            let mut serve = link_run_tool_contract::cli::RunToolServeArgs {
                files: files.clone(),
                programs_path: path.clone(),
                timeout: millis(command_ms(agent, "run")?),
                output_bytes: output_bytes(agent)?,
                program: commands.clone(),
                var: Vec::new(),
                with_env: false,
                state: None,
                way_in: None,
                r#as: None,
                keyring: None,
                background: None,
                background_timeout: None,
                background_output: None,
                background_keep: None,
                background_per_day: None,
                hops: None,
                conversations: Some(
                    folder.of(link_store_contract::storage::Collection::Conversations),
                ),
                sandbox: Vec::new(),
            };
            if from {
                background(agent, &mut serve)?;
            }
            // Its desktop's: the display named to each program, and its
            // socket and cookie alone read, never another display.
            let desktop = crate::screens::desktop(agent);
            if let Some((display, auth)) = &desktop {
                serve.var.extend([
                    format!("DISPLAY=:{display}"),
                    format!("XAUTHORITY={}", auth.display()),
                ]);
            }
            serve.var.extend(programs::vars(agent));
            let context = context::of(agent)?;
            let mut environment = BTreeMap::new();
            if let Some(context) = context {
                context.check_variables(
                    &serve
                        .var
                        .iter()
                        .map(|value| {
                            value
                                .split_once('=')
                                .map(|(name, value)| (name.to_string(), value.to_string()))
                                .unwrap()
                        })
                        .collect::<Vec<_>>(),
                )?;
                let name = link_run_tool_contract::context::ENV;
                serve.with_env = true;
                environment.insert(name.into(), serde_json::to_string(&context)?);
            }
            let mut sandboxed = vec![lnk.clone(), "sandbox".into(), "run".into()];
            sandboxed.extend(sandbox_flags(&sandbox, &folder, &teams, Reach::of(agent)));
            sandboxed.extend(programs::flags(agent)?);
            for command in &commands {
                sandboxed.extend(["--commands.allow".into(), command.clone()]);
            }
            sandboxed.extend(toolchain_flags(&toolchains));
            sandboxed.extend(developer_tools_flags(&sandbox));
            // Its variables reach the sandbox on a descriptor, checked
            // there as a caller's: no program but `[commands]`' runs.
            if let Some((display, auth)) = &desktop {
                sandboxed.extend([
                    "--files.read".into(),
                    format!("/tmp/.X11-unix/X{display}"),
                    "--files.read".into(),
                    auth.display().to_string(),
                ]);
            }
            serve.sandbox = sandboxed;
            let argv = std::iter::once(program.display().to_string())
                .chain(serve.argv())
                .collect();
            defined.push(add_with_env(
                agent,
                "run",
                &program,
                &folder,
                &sandbox,
                argv,
                environment,
            )?);
        }
    }
    // File: `lnk-file-tool serve`, its calls in one `lnk sandbox run
    // <flags> -- lnk-file-tool apply`, a call a line on its stdin.
    match on("file") {
        false => forget(agent, "file"),
        true => {
            let program = program(FILE)?;
            let program_arg = program.display().to_string();
            // What its spec leaves out of it: `find`, and diffs.
            let file = spec.tool("file");
            let mut serve = link_file_tool_contract::cli::FileToolServeArgs {
                no_find: file.find != Some(true),
                no_diffs: file.diffs != Some(true),
                team_bridge: Vec::new(),
                socket: None,
                r#as: None,
                account: None,
                keyring: None,
                command: Vec::new(),
                write: Vec::new(),
                whole: false,
                team_write: Vec::new(),
                team_read: Vec::new(),
            };
            // Its teams whose folders an account hosting agents keeps: at
            // `teams/<team>/` of its paths too, through its bridge's files
            // bridge, which holds the agent's token for their service.
            if !teams.hosted.is_empty() {
                let who = agent.identity();
                serve.team_bridge = teams.hosted.iter().map(|t| t.name.clone()).collect();
                serve.socket = Some(bridge::TeamFilesPlan::socket_of(agent));
                serve.r#as = Some(who.agent);
                serve.account = Some(who.owner);
            }
            let apply = file_grants(&sandbox, &teams, &files, &folder, &mut serve);
            let mut command = vec![lnk.clone(), "sandbox".into(), "run".into()];
            command.extend(sandbox_flags(&sandbox, &folder, &teams, Reach::Files));
            command.extend(["--files.read".into(), program_arg.clone()]);
            command.extend(["--commands.allow".into(), program_arg.clone()]);
            command.push("--".into());
            command.push(program_arg.clone());
            command.extend(apply.argv());
            serve.command = command;
            let argv = std::iter::once(program_arg.clone())
                .chain(serve.argv())
                .collect();
            defined.push(add(agent, "file", &program, &folder, &sandbox, argv)?);
        }
    }
    // Code: `lnk-code-tool serve`, each language server `lnk sandbox run
    // <flags> -- <server>`, no network, the programs `run` may start its
    // to start (a server runs the project's builds).
    let servers = spec.tool("code").servers;
    let (usable, unavailable) = code_servers(&servers, &commands, &path);
    match on("code") && !usable.is_empty() {
        false => forget(agent, "code"),
        true => {
            let program = program(CODE)?;
            let mut sandboxed = vec![lnk, "sandbox".into(), "run".into()];
            sandboxed.extend(sandbox_flags(&sandbox, &folder, &teams, Reach::Folders));
            sandboxed.extend(home_flags(&sandbox));
            sandboxed.extend(programs::flags(agent)?);
            for command in &commands {
                sandboxed.extend(["--commands.allow".into(), command.clone()]);
            }
            sandboxed.extend(toolchain_flags(&toolchains));
            sandboxed.extend(developer_tools_flags(&sandbox));
            // The cache variables `run` gives its commands, so a server sees
            // the dependencies `run` fetched: set inside the
            // sandbox by `env`, which then runs where only servers may.
            let var = programs::vars(agent);
            if !var.is_empty() && !allows(&commands, link_code_tool_contract::env_program(), &path)
            {
                sandboxed.extend([
                    "--commands.allow".into(),
                    link_code_tool_contract::env_program().into(),
                ]);
            }
            let serve = link_code_tool_contract::cli::CodeToolServeArgs {
                files: files.clone(),
                programs_path: path.clone(),
                timeout: millis(command_ms(agent, "code")?),
                servers: serde_json::to_string(&servers)?,
                unavailable: unavailable
                    .iter()
                    .map(|(language, why)| format!("{language}={why}"))
                    .collect(),
                var,
                sandbox: sandboxed,
            };
            let argv = std::iter::once(program.display().to_string())
                .chain(serve.argv())
                .collect();
            defined.push(add(agent, "code", &program, &folder, &sandbox, argv)?);
        }
    }
    Ok(defined)
}

/// Forgets tool `which`, if it was defined.
pub fn forget(agent: &Settings, which: &str) {
    if std::fs::remove_file(record(agent, which)).is_ok() {
        let _ = link_core::lnk()
            .args(["sandbox", "tool", "remove", &tool_of(which, &agent.key())])
            .output();
        crate::confine::forget_defined_tools();
    }
}

/// Defines tool `which` as `argv`, its program `program`, unless it's
/// defined so already. Its name. A screen ([`crate::screens`]) is
/// shared by the agent's sessions, which share one browser.
pub fn add(
    agent: &Settings,
    which: &str,
    program: &Path,
    folder: &link_agent_contract::folder::Folder,
    sandbox: &Sandbox,
    argv: Vec<String>,
) -> anyhow::Result<String> {
    add_with_env(
        agent,
        which,
        program,
        folder,
        sandbox,
        argv,
        BTreeMap::new(),
    )
}

fn add_with_env(
    agent: &Settings,
    which: &str,
    program: &Path,
    folder: &link_agent_contract::folder::Folder,
    sandbox: &Sandbox,
    argv: Vec<String>,
    environment: BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let record = record(agent, which);
    let tool = tool_of(which, &agent.key());
    let mut defined = crate::defined::Tool::new(argv.clone(), program);
    defined.environment(&environment)?;
    // The folders each call writes, there before it runs.
    for name in &sandbox.folders().write {
        let path = folder.seen(name);
        std::fs::create_dir_all(&path).within(format!("creating {}", path.display()))?;
    }
    for folder in agent.team_reach().write {
        std::fs::create_dir_all(&folder).within(format!("creating {}", folder.display()))?;
    }
    if crate::defined::kept(&record).as_ref() == Some(&defined)
        && crate::confine::defined_tools().is_ok_and(|tools| tools.contains(&tool))
    {
        return Ok(tool);
    }
    let _ = std::fs::remove_file(&record);
    // Run's calls ask nothing (its `[commands]` allows or refuses),
    // file's are scoped by `[files]`, the browser's by its own sandbox,
    // and the desktop's touch only its own display: none waits for you.
    // Each call the tool declares, as its contract renders them.
    let calls = match which {
        "run" => link_run_tool_contract::calls(),
        "browser" => link_browser_tool_contract::calls(),
        "desktop" => link_desktop_tool_contract::calls(),
        "code" => link_code_tool_contract::calls(),
        _ => link_file_tool_contract::calls(),
    }?;
    let mut add: Vec<String> = vec!["sandbox".into(), "tool".into(), "add".into(), tool.clone()];
    if crate::screens::SCREENS.contains(&which) {
        add.push("--shared".into());
    }
    for call in calls {
        add.extend(["--add".into(), call]);
    }
    for name in environment.keys() {
        add.extend(["--env".into(), name.clone()]);
    }
    add.push("--".into());
    add.extend(argv);
    let mut command = link_core::lnk();
    command.envs(&environment);
    let output = command
        .args(&add)
        .current_dir(&folder.home)
        .output()
        .within("running lnk sandbox tool add")?;
    crate::confine::forget_defined_tools();
    if !output.status.success() {
        bail!(
            "lnk sandbox tool add {tool} failed: {}",
            link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    crate::defined::keep(&record, &defined)?;
    Ok(tool)
}

#[cfg(test)]
mod tests;
