//! `lnk agent ...`, each command sent to what runs it.

use std::io::Write;

use link_agent::cli::{self, Chat, Command, Starting};
use link_agent::{
    adapter::Adapter,
    bridge, connections, grants, migrating, moved,
    settings::{self, Settings},
};
use link_agent_contract::moving::Phase;
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Path, Report, Table, Text, Value, WithinExt, cmd, prose,
    sentence,
};

use crate::confine::run_inside;
use crate::connecting::{connect, disconnect, list_shares, share, speaks, unshare, webhook};
use crate::conversations::{conversation, conversation_command, conversations_move, skill_add};
use crate::creating::{new, new_on, route, route_default, start};
use crate::granting::{
    Given, allow_need, allow_or_deny, deny_need, reset, restart_agent, restart_with,
};
use crate::inspecting::{check_sandbox, show};
use crate::launching::up;
use crate::listing::{list_agents, moved_json};
use crate::permissions::{
    Asks, YES, agents_running, grant_needs, harness_permissions, keyed, pending_needs,
};
use crate::removing::{delete, remove, uninstall};
use crate::services::unsandboxed;
use crate::status::{list, logs};
use crate::{live, memory, moving, scheduler, shared, stopping, teams, tools, using};

/// A command that prints for itself: it printed what it prints.
fn shown(result: anyhow::Result<()>) -> Result<Done, Report> {
    result?;
    Ok(Done::shown())
}

pub(crate) async fn try_main(
    out: &mut Out,
    parsed: link_spec::Parsed<cli::Cli>,
) -> Result<Done, Report> {
    let mut command = parsed.command.command;
    let fields = parsed.fields;
    let arguments = parsed.arguments;
    if let Command::Uninstall(given) = &command {
        return shown(uninstall(given));
    }
    // The agent's ID, which the command line gave for its name.
    let named = command.agent().map(|id| id.as_str().to_string());
    if let Some(done) = crate::spec_file::route(out, named.as_deref(), &command, &fields)? {
        return Ok(done);
    }
    // `show <name>` names its agent itself.
    if let (Some(_), Command::AgentShow(cli::AgentShowArgs { name: Some(_), .. })) =
        (&named, &command)
    {
        return Err(Report::error(sentence!(
            "Name the agent once: as the argument, or with {}, not both.",
            cmd!("--agent <name>")
        ))
        .exit(Exit::Usage));
    }
    let named = match &command {
        Command::AgentShow(cli::AgentShowArgs {
            name: Some(name), ..
        }) => Some(name.clone()),
        _ => named,
    };
    // The agent an ID names, by its name from here on: the name this
    // machine's commands keep its files under.
    let named = named.map(|named| by_id(&named)).transpose()?;
    if let Command::AgentDelete(delete) = &mut command {
        delete.name = by_id(&delete.name)?;
    }
    // Route before native harness lookup, setup, hooks or tool dispatch.
    if let Some(done) =
        crate::machine::routed(out, &command, &fields, named.as_deref(), &arguments)?
    {
        return Ok(done);
    }
    if let Command::ChatSet(set) = &command {
        set.check().map_err(|err| {
            Report::error(link_format::Sentence::adopt(&err.to_string())).exit(Exit::Usage)
        })?;
    }
    let chat = Chat::of(&command);
    match &command {
        Command::AgentActionBridge(given) => {
            let started = link_sandbox_route::bridge::Started {
                socket: given.socket.clone().into(),
                keys: given.keys.clone().into(),
                settings: given.settings.clone().into(),
                runtime: given.runtime.clone().into(),
            };
            return shown(crate::action_routes::serve(&started).await);
        }
        Command::AgentContextMcp(given) => {
            return shown(crate::context_mcp::serve(named.as_deref(), &given.agent_id).await);
        }
        Command::QuestionList(args) => {
            return crate::questions_cli::run(out, named.as_deref(), args);
        }
        Command::ModelSet(args) if chat.is_none() => {
            return crate::model::set(out, named.as_deref(), args).await;
        }
        _ => {}
    }
    if let Some((path, request)) = crate::namespace_cli::command(&command) {
        let agent = Settings::resolve_without_keys(named.as_deref())?;
        let input = crate::namespace_cli::read(request)?;
        let answer = if path.starts_with("model ") {
            let endpoints = crate::action_routes::endpoints(&agent)?;
            if agent.proxy_is_here() {
                link_agent_contract::namespace::client::call(&endpoints, &path, input).await?
            } else {
                link_agent_contract::namespace::client::call_via_socket(
                    &endpoints,
                    &path,
                    input,
                    &agent.net_dir(agent.harness()).join("out.sock"),
                )
                .await?
            }
        } else {
            // Every conversation's work: the owner's view, said only here.
            let meta = match &command {
                Command::TaskList(args) if args.all => serde_json::json!({"link.local/all": true}),
                _ => serde_json::json!({}),
            };
            crate::action_routes::execute_with(
                &agent,
                &path,
                input,
                meta,
                link_agent_contract::action_registry::Caller::Owner,
            )
            .await?
        };
        // `model status` is the endpoint `model show` answers with.
        let answer = match &command {
            Command::ModelStatus(_) => answer["endpoint"].clone(),
            _ => answer,
        };
        out.answer(&answer);
        return Ok(Done::shown());
    }
    match &command {
        // A send as the agent: its own tool's, on the command line.
        Command::ChatSend(message) if message.to.is_some() => {
            let agent = Settings::resolve_without_keys(named.as_deref())?;
            let mut input = serde_json::json!({
                "to": cli::send_to(message)?,
                "message": message.message,
                "thread": cli::send_thread(message)?,
            });
            if let Some(connection) = &message.connection {
                input["connection"] = serde_json::json!(connection);
            }
            let answer = crate::action_routes::execute(&agent, "agent send", input).await?;
            out.answer(&answer);
            return Ok(Done::shown());
        }
        // The service carrying the agent's connections: nothing else to do.
        Command::AgentBridge(_) => {
            link_format::daemon::logs();
            let agent = Settings::resolve(named.as_deref())?;
            let plan = bridge::Plan::load(&agent.name)?;
            return shown(bridge::run(&agent, &plan, scheduler::bridge_runner(&agent)));
        }
        // The one bridge of this machine's agents, where they share one.
        Command::AgentBridges(args) => {
            link_format::daemon::logs();
            return shown(link_agent::bridges::serve(&args.dir));
        }
        // The one harness of an owner's agents here, where they share one.
        Command::AgentShared(args) => {
            link_format::daemon::logs();
            return shown(shared::serve(&args.dir));
        }
        // A service running its harness with no sandbox: the machine's
        // switch read first, at every restart and boot.
        Command::AgentUnsandboxed(guard) => {
            link_format::daemon::logs();
            return shown(unsandboxed(guard));
        }
        // An agent's memory tool, starting: its model, then its engine.
        Command::MemoryServe(serve) => return shown(memory::serve(&serve.model, &serve.command)),
        // A session of an agent's memory, with its owner's memory service;
        // and that service, started by the first session needing it.
        Command::MemoryConnect(connect) => {
            return shown(memory::connect(&connect.dir, &connect.agent_id));
        }
        Command::MemoryService(service) => {
            return shown(memory::service(&service.dir, &service.generation));
        }
        _ => {}
    }
    // `--yes`: what a harness asks for beyond Link's defaults is allowed.
    let yes = match &command {
        Command::AgentStart(a) => a.yes,
        Command::AgentCreate(a) => a.yes,
        Command::AgentRestore(a) => a.yes,
        Command::HarnessSet(a) => a.yes,
        Command::AgentMove(a) => a.yes,
        Command::ToolAdd(a) => a.yes,
        _ => false,
    };
    YES.store(yes, std::sync::atomic::Ordering::Relaxed);
    // What agents that left were kept of, deleted after 30 days.
    moved::prune();
    // A harness's own commands: the harness's, not one agent's.
    if harnesses::harness_own(&command) {
        return harness_command(out, named.as_deref(), command, &fields).await;
    }
    // A team's: its agents are named in the command.
    if teams::owns(&command) {
        return teams::run(out, command, &fields).await;
    }
    // The agent named, when a cloud that hosts agents runs it: its note
    // here says where, and it has no settings here.
    let hosted = match named.as_deref() {
        Some(name) if !Settings::path_of(name).exists() => {
            moved::of_name(name)?.filter(|note| note.to.cloud)
        }
        _ => None,
    };
    // `create` makes an agent and `list` shows them all; every other command
    // acts on one: the one -a names, else the only one here.
    let mut agent = match &command {
        Command::AgentConnect(args)
            if named
                .as_deref()
                .is_none_or(|name| !Settings::path_of(name).exists())
                && let Ok(found) = moving::find(named.as_deref())
                && found.runs_agents =>
        {
            return crate::connecting::connect_hosted(out, found, &args.connection);
        }
        Command::AgentCreate(args) => {
            // The harness is the spec's field, `--harness <name>` its
            // shorthand for `--harness.name`.
            let harness = fields
                .get(&["harness", "name"])
                .and_then(toml::Value::as_str)
                .map(str::to_string);
            if let Some(harness) = &harness {
                link_agent::checks::harness(harness)
                    .map_err(|why| Report::error(sentence!("{}", why)).exit(Exit::Usage))?;
            }
            let starting = Starting::create(args, &fields);
            let name = &args.name;
            let harness = harness.as_deref();
            let slots = cli::slots(&args.on, &args.cloud, &args.zone, args.here, &args.place);
            let target = crate::slots::target(&slots)?;
            if let Some(link_workplace::Target::Cloud { cloud, zone }) = &target {
                if zone.is_some() {
                    return Err(Report::error(sentence!(
                        "A cloud that hosts agents has no zones to choose."
                    ))
                    .exit(Exit::Usage));
                }
                let from = args.from.as_deref();
                return crate::creating::new_hosted(
                    out, cloud, name, from, harness, &starting, args.yes,
                );
            }
            // A ready-made agent here is its archive, imported.
            if let Some(from) = &args.from {
                return Err(Report::error(sentence!(
                    "Only a cloud that hosts agents makes one from a ready-made agent."
                ))
                .why(prose!(
                    "Here, import {}'s archive, then start it.",
                    Name::new("agent", from)
                ))
                .exit(Exit::Usage)
                .next(cmd!("lnk archive import"), "bring a ready-made agent in"));
            }
            let on = match &target {
                Some(link_workplace::Target::On(machine)) => Some(machine.as_str()),
                _ => None,
            };
            if let Some(guest) = crate::machine::create(name, on, &starting)? {
                return crate::creating::new_in_guest(
                    out, &guest, name, harness, &starting, args.yes,
                );
            }
            return match on {
                Some(on) => new_on(out, on, name, harness, &starting, args.yes),
                None => new(out, name, harness.map(str::to_string), &starting).await,
            };
        }
        Command::AgentList(args) => return list_agents(out, args.json, args.here, args.refresh),
        Command::AgentDelete(args) => {
            // A partial migration's steps undone first: what's left of it
            // is an agent, removed as any is.
            if migrating::clear_partial(&args.name)? {
                out.block(Note(sentence!(
                    "Cleared the migration of {}, stopped partway.",
                    Name::new("agent", &args.name)
                )));
                if !Settings::path_of(&args.name).exists() {
                    return Ok(Done::shown());
                }
            }
            return delete(out, &args.name, args.yes, args.delete_files).await;
        }
        Command::AgentMigrate(args) if args.clean => {
            return crate::migrate::clean(out, args.harness.as_deref(), args.yes);
        }
        Command::AgentMigrate(args) => {
            return crate::migrate::run(out, named.as_deref(), args.harness.as_deref(), args.yes);
        }
        // The machine's, not one agent's: made before any agent is.
        Command::AgentRoute(given) if given.default => {
            return shown(route_default(out, &cli::upstream(given)?).await);
        }
        // Wherever the agent is.
        Command::AgentMove(args) => {
            let slots = cli::slots(&args.on, &args.cloud, &args.zone, args.here, &args.place);
            let Some(to) = crate::slots::target(&slots)? else {
                return Err(Report::error(sentence!(
                    "Where to? Give one of {}, {} or {}.",
                    cmd!("--on <box>"),
                    cmd!("--cloud <cloud>"),
                    cmd!("--here")
                ))
                .exit(Exit::Usage));
            };
            let done = moving::away::move_to(
                out,
                named.as_deref(),
                &to,
                args.image.as_deref(),
                args.no_start,
                args.yes,
            )?;
            crate::machine::left_guests()?;
            return Ok(done);
        }
        Command::AgentRestore(args) => {
            return crate::restoring::restore(out, named.as_deref(), args, &fields).await;
        }
        // The agent it is, made here if it isn't.
        Command::AgentReceive(args) => {
            if args.have {
                let id = args
                    .agent_id
                    .as_deref()
                    .within("--have needs --agent-id <agent id>")?;
                link_machine::machine::check_id(id)?;
                return shown(moving::sending::print_have(out, id));
            }
            let phase = Phase::of(args.phase.as_deref().unwrap_or("switch"))?;
            if let Some(id) = args.agent_id.as_deref() {
                link_machine::machine::check_id(id)?;
            }
            let Some((agent, harness)) =
                moving::receiving::receive(out, named.as_deref(), phase, args.agent_id.as_deref())?
            else {
                return Ok(Done::shown());
            };
            if !args.no_start {
                up(out, &agent, &harness, false).await?;
            }
            return Ok(Done::shown());
        }
        // A cloud that hosts agents runs them with no programs of their
        // own: `run` is for a computer or a box.
        Command::ToolAdd(args)
            if args.tool == "run"
                && let Some(name) = named.as_deref()
                && let Some(note) = &hosted =>
        {
            return Err(Report::error(sentence!(
                "{} runs on {}, which hosts agents without running programs for them.",
                Name::new("agent", name),
                note.place()
            ))
            .why(prose!(
                "Run works on your computer or a box, and keeps the agent's commands."
            ))
            .exit(Exit::Usage)
            .next(
                cmd!("lnk agent move here --agent {}", Name::new("agent", name)),
                "bring it back with its commands",
            ));
        }
        // A hosted agent's backup is its host's, where it runs, and
        // billed by that cloud: made once you say yes.
        Command::AgentBackup(args)
            if let Some(name) = named.as_deref()
                && let Some(note) = &hosted =>
        {
            link_env_contract::remote::billed(
                out,
                &note.to.name,
                sentence!(
                    "Make a backup of {} in {}, billed by that cloud?",
                    Name::new("agent", name),
                    Name::new("cloud", &note.to.name)
                ),
                prose!("The cloud keeps the backup, and bills its storage at its rates."),
                args.yes,
            )?;
            return Ok(link_agent::hosted::backup(note, false)?);
        }
        // A chat with an agent a cloud hosts goes through that cloud's
        // adapter, where it runs; one message follows it as it moves.
        _ if let Some(chat) = chat.as_ref()
            && let Some(name) = named.as_deref()
            && hosted.is_some() =>
        {
            return crate::following::send(out, name, chat).await;
        }
        // Show and the log read no key: no Keychain read on a Mac. They
        // never save, so the keys they lack are never written back.
        Command::AgentShow(_) | Command::LogShow(_) | Command::StateExport(_) => {
            Settings::resolve_without_keys(named.as_deref())?
        }
        _ => Settings::resolve(named.as_deref())?,
    };
    // An agent that moved away: its note says where.
    if !agent.path().exists()
        && let Some(note) = moved::of_name(&agent.name)?
    {
        match &command {
            // Its entry, where it went: as the list gives it.
            Command::AgentShow(args) if args.json => {
                link_actions::answer::reply::<link_agent_spec::declared::LifeAgentEntry>(
                    out,
                    &moved_json(&note),
                )?;
                return Ok(Done::shown());
            }
            Command::AgentShow(_) if note.to.cloud => {
                out.block(Text(prose!(
                    "{} runs on {}.",
                    Name::new("agent", &agent.name),
                    note.place()
                )));
                return Ok(Done::shown());
            }
            Command::AgentShow(_) => {
                // What it shows where it runs, its place as this computer
                // says it.
                let shown = ["show".to_string(), "--on".into(), note.place()];
                out.hand_over();
                let status = link_core::lnk()
                    .args(["box", "run", &note.to.name, "--", "agent"])
                    .args(moving::naming(&agent.name))
                    .args(&shown)
                    .status()
                    .within("running lnk box run")?;
                return Ok(Done::passed(status.code().unwrap_or(1)));
            }
            // A chat goes where the agent is: nothing new on the box.
            // The message, if given, on its stdin.
            _ if let Some(chat) = chat.as_ref()
                && !note.to.cloud =>
            {
                let mut there = link_core::lnk();
                there
                    .args(["box", "run", &note.to.name, "--", "agent"])
                    .args(moving::naming(&agent.name))
                    .args(chat.args());
                if chat.piped() {
                    there.stdin(std::process::Stdio::piped());
                }
                let mut child = there.spawn().within("running lnk box run")?;
                if let (true, Some(message), Some(mut stdin)) =
                    (chat.piped(), chat.message(), child.stdin.take())
                {
                    stdin
                        .write_all(message.as_bytes())
                        .within("handing the message to lnk box run")?;
                }
                let status = child.wait().within("running lnk box run")?;
                return Ok(Done::passed(status.code().unwrap_or(1)));
            }
            _ => {
                return Err(Report::error(sentence!(
                    "{} runs on {}.",
                    Name::new("agent", &agent.name),
                    note.place()
                ))
                .next(
                    cmd!(
                        "lnk agent move here --agent {}",
                        Name::new("agent", &agent.name)
                    ),
                    "bring it back",
                )
                .next(cmd!("lnk agent create <name>"), "make another here"));
            }
        }
    }
    // An agent that isn't there yet: said, rather than a new one acted
    // on or shown.
    let needs_one = chat.is_some()
        || matches!(
            command,
            Command::AgentStop(_)
                | Command::AgentRestart(_)
                | Command::LogShow(_)
                | Command::StateExport(_)
                | Command::ChatShow(_)
                | Command::ChatExport(cli::ChatExportArgs {
                    format: Some(_),
                    ..
                })
                | Command::ChatList(_)
                | Command::ChatFork(_)
                | Command::AgentConnect(_)
                | Command::AgentDisconnect(_)
                | Command::ShareAdd(_)
                | Command::ShareList(_)
                | Command::ShareRemove(_)
                | Command::WebhookCreate(_)
                | Command::WebhookList(_)
                | Command::WebhookRotate(_)
                | Command::WebhookDelete(_)
                | Command::AgentShow(_)
                | Command::PermissionAdd(_)
                | Command::PermissionRemove(_)
                | Command::AgentBackup(_)
                | Command::AgentSecret(_)
                | Command::ToolAdd(_)
                | Command::ToolList(_)
                | Command::ToolRemove(_)
                | Command::MemoryStart(_)
                | Command::MemoryStop(_)
                | Command::MemoryShow(_)
                | Command::SkillAdd(_)
                | Command::SkillList(_)
                | Command::SkillRemove(_)
        );
    if needs_one && !agent.path().exists() {
        return Err(Report::error(sentence!("There is no agent yet."))
            .next(cmd!("lnk agent start"), "set one up"));
    }
    if let Some(chat) = &chat {
        return crate::following::send(out, &agent.name, chat).await;
    }
    match command {
        Command::AgentStart(args) => {
            return start(out, &mut agent, &Starting::start(&args, &fields)).await;
        }
        Command::HarnessSet(args) => {
            return using::switch(out, &mut agent, args.harness, args.foreground).await;
        }
        Command::AgentStop(_) => return stopping::stop(&agent),
        Command::StateExport(_) => return crate::state_export::run(out, &agent).await,
        Command::AgentRestart(_) => return stopping::restart(out, &mut agent).await,
        Command::LogShow(args) if args.live => return live::run(out, &agent),
        Command::LogShow(args) => return logs(out, &agent, args.follow),
        Command::ChatShow(args) => return conversation(out, &agent, &args.chat, args.json),
        Command::ChatExport(cli::ChatExportArgs {
            format: Some(format),
            ..
        }) => return conversations_move(out, &agent, &format),
        Command::ChatList(_) => {
            return conversation_command(out, &agent, &["conversations", "list", "--json"], false);
        }
        Command::ChatFork(args) => {
            return conversation_command(
                out,
                &agent,
                &[
                    "conversations",
                    "fork",
                    &args.chat,
                    "--before",
                    &args.before.to_string(),
                ],
                true,
            );
        }
        Command::AgentConnect(args) => {
            connections::check_name(&args.connection)?;
            // A secret by its ID is passed on an agent a cloud hosts;
            // here the connection reads its token as it says.
            if args.secret.is_some() {
                return Err(Report::error(sentence!(
                    "Here {} reads its token as its help says; a secret by its ID is for an agent a cloud hosts.",
                    &args.connection
                ))
                .next(cmd!("lnk agent connect --help"), "see how it reads its token")
                .exit(Exit::Usage));
            }
            speaks(&agent, &args.connection)?;
            return connect(out, &mut agent, &args.connection).await;
        }
        Command::AgentDisconnect(args) => {
            return disconnect(out, &mut agent, &args.connection).await;
        }
        Command::ShareAdd(args) => {
            connections::check_name(&args.connection)?;
            return share(out, &mut agent, &args.connection, args.json).await;
        }
        Command::ShareList(args) => return list_shares(out, &agent, args.json),
        Command::ShareRemove(args) => {
            connections::check_name(&args.connection)?;
            return unshare(out, &mut agent, &args.connection, &args.chat, args.json).await;
        }
        command @ (Command::WebhookCreate(_)
        | Command::WebhookList(_)
        | Command::WebhookRotate(_)
        | Command::WebhookDelete(_)) => return webhook(out, &mut agent, command).await,
        Command::AgentShow(args) => return show_agent(out, &agent, args),
        command @ (Command::MemoryStart(_) | Command::MemoryStop(_) | Command::MemoryShow(_)) => {
            let (changed, done) = match command {
                Command::MemoryStart(args) => {
                    memory::start_engine(out, &mut agent, args.engine.as_deref())?
                }
                Command::MemoryStop(_) => memory::off(out, &mut agent)?,
                _ => {
                    let harness = agent.harness().to_string();
                    (false, memory::status(out, &agent, &harness)?)
                }
            };
            // Restarted only once you say so; one in a terminal keeps what
            // it had until it's started again there, and is told so (X7).
            if changed {
                restart_agent(out, &mut agent).await?;
            }
            return Ok(done);
        }
        command @ (Command::ToolList(_) | Command::ToolAdd(_) | Command::ToolRemove(_)) => {
            let (changed, done) = match command {
                // Each of Link's tools, for a person; the granted
                // namespaces, typed, with `--json`.
                Command::ToolList(args) if !args.json => (false, tools::list(out, &agent)?),
                Command::ToolList(args) => {
                    let input = crate::namespace_cli::read(&args.request)?;
                    let answer = crate::action_routes::execute(&agent, "tool list", input).await?;
                    out.reply(&answer);
                    (false, Done::shown())
                }
                Command::ToolAdd(args) => tools::add(out, &mut agent, &args.tool, args.yes)?,
                Command::ToolRemove(args) => tools::remove(out, &mut agent, &args.tool)?,
                _ => unreachable!("a tool command"),
            };
            if changed {
                restart_agent(out, &mut agent).await?;
            }
            return Ok(done);
        }
        Command::SkillAdd(args) => {
            let source = args.path.or(args.url).unwrap_or_default();
            return skill_add(out, &agent, &source, args.replace);
        }
        Command::SkillList(_) => {
            let found = link_agent::skills::list(&agent.folder().agent)?;
            let mut skills = Table::new(["SKILL", "WHAT"], sentence!("There are no skills yet."));
            for skill in &found {
                skills.row([
                    Value::from(Foreign::new(&skill.name)),
                    Value::from(Foreign::new(&skill.description)),
                ]);
            }
            out.block(skills);
            return Ok(match found.is_empty() {
                true => Done::shown().next(cmd!("lnk agent skill add <folder or URL>"), "add one"),
                false => Done::shown(),
            });
        }
        Command::SkillRemove(args) => {
            let at = link_agent::skills::remove(&agent.folder().agent, &args.name)?;
            return Ok(Done::success(sentence!(
                "Removed skill {} from {}.",
                Foreign::new(&args.name),
                Path::new(&at)
            )));
        }
        Command::AgentBackup(args) => return moving::backups::backup(out, &agent, args.yes),
        Command::AgentStateSend(args) => {
            let phase = Phase::of(args.phase.as_deref().unwrap_or("switch"))?;
            if let Some(move_id) = &args.r#move {
                link_agent_contract::moving::check_move_id(move_id)?;
            }
            let seal_to = args
                .seal_to
                .as_deref()
                .map(str::parse::<link_secretive::SealTo>)
                .transpose()
                .within("reading --seal-to")?;
            let have = moving::sending::read_have(args.held.as_deref())?;
            if args.without_model_credentials {
                moved::models::keep(&agent)?;
                agent.model = None;
                agent.fallback = None;
            } else if phase == Phase::Switch
                && let Some(id) = agent.agent_id()
            {
                moved::models::release(id)?;
            }
            moving::sending::send(
                &agent,
                phase,
                args.r#move.clone(),
                have,
                seal_to.as_ref(),
                args.harness_state,
            )?
        }
        Command::AgentPause(_) => moving::sending::pause(&agent)?,
        Command::AgentResume(_) => moving::sending::resume(out, &agent),
        Command::AgentDeliver(_) => moving::sending::deliver(out, &agent).await?,
        Command::AgentNeeds(args) => {
            let allow = &args.add;
            let harness = agent.harness().to_string();
            let adapter = Adapter::find(&harness)?;
            // What it runs without (a risk with `without`) is left to its
            // start there, which says how to allow it.
            let (needs, _, grants) = pending_needs(&agent, &harness, &adapter)?;
            // What the machine running the move asked about and was
            // allowed, granted; only what the harness asks for now.
            let allowed: Asks = needs
                .iter()
                .filter(|need| allow.contains(&need.key))
                .cloned()
                .collect();
            if let Some(other) = allow
                .iter()
                .find(|a| !needs.iter().any(|need| need.key == **a))
            {
                return Err(Report::error(sentence!(
                    "{} doesn't ask for {} here, so nothing was granted.",
                    Name::new("harness", &harness),
                    other.to_string()
                ))
                .exit(Exit::Usage));
            }
            // What it has already is remembered, as a start would.
            grant_needs(&harness, grants, &allowed)?;
            let needs: Asks = needs
                .into_iter()
                .filter(|need| !allow.contains(&need.key))
                .collect();
            let needs = keyed(&needs);
            // The machine running the move reads this line.
            out.data(
                format!(
                    "{}\n",
                    serde_json::json!({
                        "harness": harness,
                        "needs": needs,
                        "running": agents_running(&harness),
                    })
                )
                .as_bytes(),
            );
        }
        Command::AgentLeft(args) => {
            if let Some(id) = &args.machine_id {
                link_machine::machine::check_id(id)?;
            }
            let slots = cli::slots(&args.on, &args.cloud, &args.zone, args.here, &args.place);
            crate::slots::no_zone(&slots)?;
            let (name, runs_agents) = match slots.target().map_err(crate::slots::refused)? {
                Some(link_workplace::Target::On(machine)) => (machine, false),
                Some(link_workplace::Target::Cloud { cloud, .. }) => (cloud, true),
                _ => {
                    return Err(Report::error(sentence!(
                        "Where it went is --on <machine> or --cloud <cloud>."
                    ))
                    .exit(Exit::Usage));
                }
            };
            return Ok(moving::sending::left(
                out,
                &agent,
                moved::Place {
                    id: args.machine_id,
                    name,
                    cloud: runs_agents,
                    owner: None,
                },
            )?);
        }
        Command::AgentRoute(given) => {
            let upstream = cli::upstream(&given)?;
            agent.upstream = route(&upstream)?;
            restart_with(out, &mut agent).await?;
        }
        Command::AgentSecret(args) => {
            let wired = match args.at.as_str() {
                "wire" => true,
                "agent" => false,
                other => {
                    return Err(Report::error(sentence!(
                        "{} isn't a place for keys: wire or agent.",
                        other.to_string()
                    ))
                    .exit(Exit::Usage));
                }
            };
            agent.wire_keys = Some(wired);
            if wired && agent.wired_model()?.is_none() && agent.address_keys()?.is_empty() {
                out.block(Note(sentence!(
                    "Kept for when it applies: the proxy adds the key for a hosted model \
                     (Anthropic, OpenAI, OpenRouter, or one at an HTTPS address of its own), \
                     with the agent behind its proxy (not on the network as it is)."
                )));
            }
            restart_with(out, &mut agent).await?;
        }
        Command::PermissionAdd(_) => {
            return allow_or_deny(out, &mut agent, fields, true).await;
        }
        Command::PermissionRemove(_) => {
            return allow_or_deny(out, &mut agent, fields, false).await;
        }
        _ => unreachable!("handled above"),
    }
    Ok(Done::shown())
}

/// `lnk agent harness ...`: what a harness may do here, and its home.
pub(crate) use harnesses::harness_command;
mod harnesses;

mod showing;
use showing::show_agent;

/// The name of the agent `named` names: by its ID, `agent:<id>`, the agent
/// here that has it; else `named`, a name.
fn by_id(named: &str) -> anyhow::Result<String> {
    match named.starts_with(&format!("{}:", settings::ids::KIND)) {
        true => crate::naming::name_of(named),
        false => Ok(named.to_string()),
    }
}
