//! A harness set up and started for an agent, until it answers.

use std::time::{Duration, Instant};

use link_agent::{
    adapter::{Adapter, Boxed},
    bridge, bridges, moved, sandbox_file,
    service::{self, Job},
    settings::{self, Settings},
};
use link_format::{
    Done, Exit, Name, Note, Out, Path, Report, Sentence, Steps, WithinExt, cmd, phrase, prose,
    sentence,
};
use link_secretive::Secret;

use crate::confine::{
    connection_programs, connection_rules, hand_grants, isolated, link_on, on_loopback, ports_free,
    runs_here, sandbox_can_run, sealed_need_proxy, sleep_of, unwalled,
};
use crate::connecting::connection_module;
use crate::model::model_ready;
use crate::permissions::{YES, answer_needs, beyond_lines, no_sandbox_warning};
use crate::services::{bridge_job, on_stop, unsandboxed_argv, watch_measures};
use crate::status::{AgentState, fields, ports_speak, small_model_warning};
use crate::{memory, run_file, scheduler, screens, shared, stops};

/// Installs, configures and starts harness `name` in its home, then waits
/// until it answers.
pub(crate) async fn up(
    out: &mut Out,
    settings: &Settings,
    name: &str,
    foreground: bool,
) -> Result<Done, Report> {
    match launch(out, settings, name, foreground).await? {
        Some(started) => started.answer(out, settings, name),
        None => Ok(Done::shown()),
    }
}

/// [`up`] in the background, for an agent `stopped` stopped: that stop
/// is `up`'s to finish from when it starts the service again, and
/// `stopped`'s if it fails before (its reload and calendar, then, not
/// left for a service that isn't running).
pub(crate) async fn up_after(
    out: &mut Out,
    settings: &Settings,
    name: &str,
    stopped: &mut StoppedUnlessStarted,
) -> Result<Done, Report> {
    match launch_after(out, settings, name, false, Some(stopped)).await? {
        Some(started) => started.answer(out, settings, name),
        None => Ok(Done::shown()),
    }
}

/// A harness whose service started, until it answers its health check:
/// what's left of [`up`].
pub(crate) struct Started {
    adapter: Adapter,
    health: link_agent::adapter::Step,
    ports: Vec<u16>,
    wait: Duration,
    timed: link_agent::times::Starting,
}

impl Started {
    /// Waits for it to answer, as [`up`] does: then the table `lnk agent
    /// status` shows, and what's next.
    fn answer(self, out: &mut Out, settings: &Settings, name: &str) -> Result<Done, Report> {
        let agent = Name::new("agent", &settings.name);
        let waiting = out.activity(phrase!("Waiting for {} to answer", &agent));
        let answered = answers_within(&self.health, &self.ports, self.wait, |_| {});
        drop(waiting);
        if !answered {
            return Err(
                Report::error(sentence!("{} started but isn't answering yet.", &agent))
                    .why(prose!("Its harness is {}.", Name::new("harness", name)))
                    .next(cmd!("lnk agent log show"), "see why"),
            );
        }
        self.timed.answered(settings);
        // Its harness answers; whether its model takes the key shows at
        // its first message, which a start doesn't send.
        let state = fields(out, settings, None, AgentState::Running)?;
        out.block(state);
        if let Some(small) = small_model_warning(settings) {
            out.block(small);
        }
        let speaks = self.adapter.speaks(settings);
        let on = settings
            .connections()
            .iter()
            .any(|c| speaks.speaks(settings, c));
        let done = Done::success(sentence!("{} is running.", &agent));
        Ok(running_next(
            done,
            on,
            crate::tools::has(settings, "run"),
            crate::tools::has(settings, "mcp"),
        ))
    }

    /// Waits for it to answer, saying nothing: whether it did. For
    /// agents restarted together, each waited for at once.
    pub(crate) fn answered(self, settings: &Settings) -> bool {
        let answered = answers_within(&self.health, &self.ports, self.wait, |_| {});
        if answered {
            self.timed.answered(settings);
        }
        answered
    }
}

/// How long a start waits for its harness to answer: as long as its
/// adapter says it may take (`Launch::wait_secs`), up to ten minutes.
fn start_wait(launch: &link_harness_contract::harness::Launch) -> Duration {
    launch.wait().min(Duration::from_secs(600))
}

/// Whether health check `health` succeeds within `wait`: every 2 s, and
/// within half a second of one of `ports` answering. `polled` hears each
/// poll's count, a poll every 250 ms.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a starting agent until it answers"
)]
pub(crate) fn answers_within(
    health: &link_agent::adapter::Step,
    ports: &[u16],
    wait: Duration,
    mut polled: impl FnMut(u32),
) -> bool {
    let start = Instant::now();
    let mut checked: Option<Instant> = None;
    let mut polls = 0u32;
    while start.elapsed() < wait {
        let due = checked.is_none_or(|at| {
            at.elapsed() >= Duration::from_secs(2)
                || (at.elapsed() >= Duration::from_millis(500) && ports_speak(ports))
        });
        if due {
            checked = Some(Instant::now());
        }
        if due && health.succeeds() {
            return true;
        }
        polls += 1;
        polled(polls);
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

/// Sets agent `settings` up on harness `name` and starts it: in the
/// foreground, until it stops (none); else its service, started (what's
/// left: [`Started`]).
pub(crate) async fn launch(
    out: &mut Out,
    settings: &Settings,
    name: &str,
    foreground: bool,
) -> Result<Option<Started>, Report> {
    launch_after(out, settings, name, foreground, None).await
}

/// [`launch`], disarming `stopped` as it starts the service: from there
/// systemd is reloaded by the start, and the calendar is the running
/// agent's, kept even if it then doesn't answer.
#[expect(
    clippy::disallowed_methods,
    reason = "starts the agent with the environment its spec builds"
)]
pub(crate) async fn launch_after(
    out: &mut Out,
    settings: &Settings,
    name: &str,
    foreground: bool,
    stopped: Option<&mut StoppedUnlessStarted>,
) -> Result<Option<Started>, Report> {
    let agent = Name::new("agent", &settings.name);
    let harness = Name::new("harness", name);
    // A move of it from this computer that ended before it finished,
    // finished or taken back first: it may answer elsewhere now.
    if let Some(id) = settings.agent_id() {
        crate::moving::journal::recover_before_start(out, id)?;
    }
    // A start takes messages again: a pause for a move is over.
    link_agent::bridge::unpause(&settings.name);
    // Its teams, which every part of a start asks for, read once.
    let _teams = settings::RememberTeams::begin();
    // One place runs the agent at a time: two would answer the same bot.
    if let Some(note) = settings.agent_id().map(moved::of_id).transpose()?.flatten() {
        return Err(
            Report::error(sentence!("{} runs on {}.", &agent, note.place()))
                .why(prose!("One place runs the agent at a time."))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk agent move here --agent {}", &agent),
                    "bring it back",
                )
                .next(
                    cmd!(
                        "lnk box run {} -- agent <command>",
                        Name::new("box", &note.to.name)
                    ),
                    "act on it there",
                ),
        );
    }
    // One running in a terminal holds its ports and runs with what it
    // was set up with: refused before anything of it is set up again.
    if service::foreground_running(&settings.name) {
        return Err(Report::error(sentence!("{} runs in a terminal.", &agent))
            .why(prose!("Ctrl-C there stops it first."))
            .exit(Exit::Usage)
            .next(cmd!("lnk agent stop --agent {}", &agent), "stop it"));
    }
    // A harness with no model to think with starts, then only fails.
    if let Some(model) = &settings.model {
        model_ready(model)?;
    }
    // Its URLs' secret new for this start: its proxy and its harness's
    // configuration are both made from it below, and a tool's address a
    // program read in an earlier start is refused from here on.
    settings
        .new_url_secret()
        .within("making its URLs' secret")?;
    // Setting up and starting; an adapter that installs its harness first
    // says so itself.
    let mut steps = Steps::new(2);
    let adapter = Adapter::find_or_add(name)?;
    let contract = adapter.contract()?;
    // Refuse customized instructions a harness cannot take. Link's,
    // written out, do not claim the harness supports them.
    if !contract.instructions
        && settings.own_spec()?.is_some_and(|spec| {
            spec.instructions.is_some() && !settings::configuration::shipped_instructions(&spec)
        })
    {
        return Err(Report::error(sentence!(
            "Its spec sets [instructions], which {} doesn't take.",
            &harness
        ))
        .why(prose!(
            "Remove them from {}, or run a harness that takes them.",
            Path::new(Settings::path_of(&settings.name))
        ))
        .next(cmd!("lnk agent spec show"), "see the spec"));
    }
    // A feature its spec turns on that this harness doesn't honor.
    if let Some(feature) = contract.refused(&settings.spec()?) {
        return Err(Report::error(sentence!(
            "Its spec turns on {}, which {} doesn't honor.",
            feature.id().to_string(),
            &harness
        ))
        .why(prose!(
            "Turn it off ({} in {}), or run a harness that honors it.",
            feature.table().to_string(),
            Path::new(Settings::path_of(&settings.name))
        ))
        .next(cmd!("lnk agent spec show"), "see the spec"));
    }
    let refit;
    // Its block of ports as long as this harness takes: Link Harness
    // takes one, not the two hundred an adapter that doesn't say gets.
    let size = settings::block_for(&contract);
    // A port of its block another program listens on, while it doesn't
    // run here (another user's agent on this machine, say): moved to a
    // block free here.
    let taken = settings
        .port
        .filter(|_| !runs_here(&settings.name))
        .and_then(|base| {
            (base..base.saturating_add(settings.block())).find(|&p| settings::listened_on(p))
        });
    let settings = match settings.port.is_none() || settings.block() != size || taken.is_some() {
        true => {
            let mut fresh = Settings::load(&settings.name)?;
            // One with no block here yet takes one as long as its harness's.
            if fresh.port.is_none() {
                fresh.block = Some(size);
                fresh.take_ports_here()?;
            }
            fresh.fit_block(size)?;
            if let Some(port) = taken {
                let base = settings::free_block_here(&fresh.name, size)?;
                fresh.port = Some(base);
                fresh.block = Some(size);
                out.block(Note(sentence!(
                    "Port {} is another program's, so your agent moves to ports {} to {}.",
                    port.to_string(),
                    base.to_string(),
                    (base + size - 1).to_string()
                )));
            }
            fresh.save()?;
            refit = fresh;
            &refit
        }
        false => settings,
    };
    // Without a sandbox already (`--no-sandbox`, or allowed before), said
    // first: what it's asked next is asked knowing it.
    let warned = !settings.sandbox;
    if warned {
        no_sandbox_warning(out, name, &settings.box_of(name));
    }
    // What it needs beyond Link's defaults, answered before anything of it
    // changes; then, every start, what it runs with beyond them.
    let needs = answer_needs(out, settings, name, &adapter)?;
    for line in beyond_lines(settings, name)? {
        out.block(Note(Sentence::adopt(&line)));
    }
    // Its folders.
    let files = settings.files();
    link_agent::layout::lay_out(settings)?;
    let mut boxed = Boxed::new(settings, name)?;
    boxed.ports.extend(settings.model_port());
    // A harness that keeps to the layout writes only what the agent's
    // sandbox file lets it; any other writes the whole folder, so a
    // folder it only reads there isn't, and that's said.
    let folders = sandbox_file::of(&settings.name).folders();
    match contract.folders {
        true => boxed.folders = Some(folders),
        false if folders != link_agent::settings::default_files() => out.block(Note(sentence!(
            "{} writes the whole of your agent's files, so its sandbox file's [files] \
                 holds only for Link's tools there.",
            &harness
        ))),
        false => {}
    }
    if !boxed.sandbox && !warned {
        no_sandbox_warning(out, name, &boxed.dir);
    }
    if boxed.sandbox {
        sandbox_can_run()?;
    }
    adapter.install(
        &boxed,
        &needs,
        YES.load(std::sync::atomic::Ordering::Relaxed),
    )?;
    // Its memory, served again as its settings say (the engine may be new,
    // or the agent new to this machine); without it, the agent still runs.
    if let Err(err) = memory::define(out, settings) {
        out.block(
            Report::warning(sentence!("Your agent's memory isn't served."))
                .why(Sentence::adopt(&err.headline.to_string()))
                .next(cmd!("lnk agent memory start"), "try again"),
        );
    }
    // Its teams' memories, each over the team's shared folder, served
    // once for every member, where the team's turned on; without one, the
    // agent still runs.
    if let Some(id) = settings.agent_id() {
        for team in settings::teams_of(id).unwrap_or_default() {
            if let Err(err) = memory::define_team(out, &team) {
                out.block(memory::team_warning(&team.name, &err));
            }
        }
    }
    // A count past what Link is tested with: the spec's to say, but said.
    if let Ok(spec) = settings.spec() {
        for unusual in link_agent_spec::unusual(&spec.spec) {
            out.block(Note(Sentence::adopt(&format!("Its spec's {unusual}."))));
        }
    }
    // Its scheduler, for a harness that reads who a message is from;
    // without it, the agent still runs.
    if let Err(err) = scheduler::define(settings, contract.from) {
        out.block(
            Report::warning(sentence!("Your agent's scheduler isn't served."))
                .why(Sentence::adopt(&format!("{err:#}")))
                .next(cmd!("lnk agent start"), "try again"),
        );
    }
    // Its screens, defined again for a moved program; without them, the
    // agent still runs.
    if let Err(err) = screens::define_all(settings) {
        out.block(
            Report::warning(sentence!("Your agent's browser isn't served."))
                .why(Sentence::adopt(&format!("{err:#}")))
                .next(cmd!("lnk agent start"), "try again"),
        );
    }
    // Stopped first: a harness may refuse to change its state while its
    // own gateway runs (a harness's own checks may need its database to itself).
    // systemd reloads once, when it starts again; or, should the start
    // fail before that, when this does.
    service::stop_before_start(&settings.name)?;
    let mut binding_lifetime = BindingUntilStarted {
        agent: settings.key(),
        armed: true,
    };
    let mut own = StoppedUnlessStarted {
        agent: settings.name.clone(),
        armed: stopped.is_none(),
    };
    // Link's own lines are the same for every harness; what a harness
    // prints comes under them (installing), or only when it fails.
    out.step(&mut steps, phrase!("Setting up {}", &harness));
    // Placeholders of this start's own: what its harness was given at the
    // last is no use from now on.
    settings.new_placeholders()?;
    // Its `run`, `file` and `code`, for a harness that keeps to the
    // files folder's layout, in its sandbox; without them, the agent
    // still runs.
    if let Err(err) = run_file::define(settings, contract.folders, boxed.sandbox, contract.from) {
        out.block(
            Report::warning(sentence!("Your agent's run, file and code aren't served."))
                .why(Sentence::adopt(&format!("{err:#}")))
                .next(cmd!("lnk agent start"), "try again"),
        );
    }
    // Its namespaces, made from the tools just defined: `task run` is
    // offered only once `run` is.
    crate::mcp_tool::define(settings)?;
    crate::namespaces::define(settings)?;

    // A harness that speaks no connection: Link carries them to its endpoint,
    // and it's handed the endpoint's key rather than their tokens.
    let mut handed = settings.for_harness(&connection_rules(settings)?)?;
    handed.actions = crate::action_routes::endpoints(settings)?;
    hand_grants(&mut handed, settings, name, boxed.sandbox);
    // The programs its `run` may start here, named to the harness.
    if let Some(environment) = &mut handed.environment {
        environment.commands = run_file::commands(settings);
    }
    let endpoint = adapter.endpoint(settings.port);
    let plan = match endpoint {
        Some(endpoint) => {
            let key = Secret::new(
                link_harness_contract::harness::random_token().within("making its key")?,
            );
            let connections = bridge::carried(settings, &adapter.connections());
            handed.endpoint_key = Some(key.duplicate());
            bridge::keep_out(&mut handed, &connections)?;
            Some(bridge::Plan {
                endpoint,
                key,
                // Its endpoint proves it's the harness before the bridges
                // hand it the key, when its adapter says it can.
                proof: contract.proof,
                // Messages from others, and its scheduler, when its
                // adapter reads who a message is from.
                from: contract.from,
                // The answers of turns it finished after a crash, sent to
                // the chat, when its adapter says it hands them over.
                recover: contract.recover,
                // A subagent's fewer tools, when its adapter says it keeps
                // a conversation's.
                tools: contract.tools,
                // A message's images and documents, downloaded by its
                // connection, when its adapter says it takes them.
                parts: contract.parts,
                // Its storage bridge, each collection's driver run outside
                // its sandbox, for a harness keeping Link's collections.
                storage: if contract.conversations {
                    bridge::StoragePlan::of(settings, &boxed)?
                } else {
                    None
                },
                // Its files bridge, for its teams whose folders are kept
                // by an account hosting agents.
                files: bridge::TeamFilesPlan::of(settings, &settings.team_reach()),
                connections,
            })
        }
        None => None,
    };
    handed.storage = plan
        .as_ref()
        .and_then(|plan| plan.storage.as_ref())
        .map(bridge::StoragePlan::at);
    // Each place's driver, installed as a module a feature needs is when
    // it's missing (`lnk-file-store` comes with Link).
    if let Some(storage) = plan.as_ref().and_then(|plan| plan.storage.as_ref()) {
        let mut places: Vec<&str> = storage
            .places
            .0
            .values()
            .map(|p| p.place.as_str())
            .collect();
        places.sort_unstable();
        places.dedup();
        // Local file collections stay in the harness's declared layout.
        places.retain(|place| *place != link_store_contract::storage::FILE);
        for place in places {
            link_core::modules::ensure(
                link_store_contract::store::module_for(place),
                &format!("keeping {}'s collections at {place}", settings.name),
            )?;
        }
    }
    connection_programs(&adapter, &boxed, settings, &mut handed)?;
    // One of the owner's agents at a time configures and joins their
    // shared harness: held until it's joined, or the start fails.
    let configuring = match shared::sharing(&contract, &boxed, true) {
        true => Some(shared::configuring(&settings::shared_harness_of(
            &settings.owner()?,
        ))?),
        false => None,
    };
    // Its skills where a harness keeping who it is in its own files reads them.
    if let (true, Some(who)) = (contract.identity, &handed.identity) {
        link_agent::identity::copy_skills(&boxed.folder.agent, &boxed.dir, &who.skills)?;
    }
    adapter
        .configure(&boxed, &handed)
        .map_err(|err| configure_failed(&harness, &err))?;
    // What the harness links itself (Signal), linked on this terminal if
    // it isn't (after switching harness, say).
    let speaks = adapter.connections();
    let linked = |c: &String| settings.connections.get(c).is_some_and(|c| c.about.linked);
    for connection in settings
        .connections()
        .iter()
        .filter(|c| linked(c) && speaks.contains(c))
    {
        if !link_on(&adapter, &boxed, settings, connection, false)? {
            out.block(Note(sentence!(
                "{} isn't linked on {}, and {} in a terminal links it.",
                settings.title(connection),
                &harness,
                cmd!("lnk agent connect {}", connection.to_string())
            )));
        }
    }
    // What Link carries and links (Signal on Link Harness), the
    // same way.
    for connection in plan
        .iter()
        .flat_map(|p| &p.connections)
        .filter(|c| linked(c))
    {
        if !bridge::link(settings, connection, false)? {
            out.block(Note(sentence!(
                "{} isn't linked for {}, and {} in a terminal links it.",
                settings.title(connection),
                &harness,
                cmd!("lnk agent connect {}", connection.to_string())
            )));
        }
    }
    let launch = adapter.command(&boxed)?;
    boxed.ports.extend(&launch.ports);
    settings::save_ports(&settings.name, name, &launch.ports)?;
    service::disable_foreign(&launch.services);
    match &plan {
        Some(plan) => {
            if !plan.endpoint.is_empty() {
                let port = link_harness_contract::harness::endpoint_port(&plan.endpoint)?;
                if !launch.ports.contains(&port) {
                    return Err(Report::error(sentence!(
                        "The endpoint of {} names port {}, which isn't one of its own.",
                        Name::new("module", format!("lnk-{name}")),
                        u64::from(port)
                    )));
                }
            }
            for connection in &plan.connections {
                connection_module(connection)?;
            }
            plan.save(&settings.name)?;
            // Its key for this start is what it proves its bridge calls
            // with: bound here, where every bridge checks them.
            if settings.id.is_none() {
                return Err(Report::error(sentence!("{} isn't started.", &agent))
                    .why(prose!("It has no ID yet, which its first save reserves.")));
            }
            link_sandbox_binding::Keyring::here().bind(&settings.identity(), &plan.key)?;
        }
        None => {
            bridge::Plan::remove(&settings.name);
            binding::bind_routes(&link_sandbox_binding::Keyring::here(), settings)?;
        }
    }
    // A bridge for the connections Link carries, and for the scheduler's
    // runner and the way in, connections or not, and for its webhooks.
    let plan = plan.filter(|plan| {
        !plan.connections.is_empty()
            || (plan.from && scheduler::tool(settings).is_some())
            || (plan.from && plan.tools && !settings.webhooks.is_empty())
            || plan.storage.is_some()
    });

    let mut env = boxed.env();
    env.extend(launch.env.clone());
    let proxy = settings.proxy_of(name);
    if boxed.sandbox && proxy.is_none() && !settings.hosts_of(name).is_empty() {
        out.block(Note(sentence!(
            "The list of hosts of {} applies only behind its proxy, and {}.",
            &harness,
            match settings
                .spec_of(name, link_agent_contract::agent_sandbox::Route::Direct)
                .internet
                .is_some()
            {
                true => "on the network as it is it runs without one",
                false => "without the internet it reaches none",
            }
        )));
    }
    // A model that searches the web by nature reaches past a list of
    // hosts: the proxy refuses it when it adds the key, and can't see it
    // when the harness holds the key.
    let searching = settings.model.as_ref().filter(|model| {
        model
            .name
            .as_deref()
            .is_some_and(|m| link_model_contract::route::searches_by_nature(&model.provider, m))
    });
    if let Some(model) =
        searching.filter(|_| proxy.is_some() && !settings.hosts_of(name).is_empty())
    {
        let model = Name::new("model", model.name.as_deref().unwrap_or_default());
        out.block(Note(match settings.wired_model()? {
            Some(_) => sentence!(
                "{} searches the web by nature, past the list of hosts of {}, so its proxy \
                 refuses it: pick another model to keep the list.",
                model,
                &harness
            ),
            None => sentence!(
                "{} searches the web by nature, past the list of hosts of {}: pick another \
                 model to keep the list.",
                model,
                &harness
            ),
        }));
    }
    // Its ports on this machine's loopback must be free: a program
    // already there would be taken for its connection's daemon, or be handed
    // the bridge's messages.
    ports_free(
        name,
        &on_loopback(
            &launch,
            proxy.is_some() && boxed.sandbox,
            cfg!(target_os = "macos"),
        ),
    )?;
    let mut sharing = None;
    let shares = proxy.is_some() && shared::sharing(&contract, &boxed, true);
    // Stopped when idle, where this machine's one bridge can wake it: the
    // bridge serves its endpoint's port, in its proxy's place.
    let sleep = sleep_of(
        settings,
        plan.as_ref(),
        &adapter.connections(),
        proxy.is_some(),
        &boxed,
        foreground,
        shares,
    )?;
    let front = sleep
        .as_ref()
        .filter(|sleep| !sleep.handoff)
        .map(|sleep| sleep.port);
    let mut argv = match &proxy {
        None => {
            sealed_need_proxy(settings, "it runs without its proxy here")?;
            boxed.wrap(
                &launch.argv,
                &boxed.profile,
                &launch.write,
                &launch.read,
                true,
                &[],
            )?
        }
        // No sandbox: its proxy beside it, which it may ignore.
        Some(upstream) if !boxed.sandbox => {
            unwalled(&boxed, settings, &launch, upstream, &mut env)?
        }
        // Its owner's one harness here serves it: its service runs its
        // proxy alone.
        Some(upstream) if shares => {
            sealed_need_proxy(
                settings,
                "its harness is shared with your other agents here",
            )?;
            let (argv, confinement) =
                isolated(&boxed, settings, &launch, upstream, &mut env, true, front)?;
            sharing = Some(confinement);
            argv
        }
        Some(upstream) => isolated(&boxed, settings, &launch, upstream, &mut env, false, front)?.0,
    };
    if cfg!(target_os = "macos") && settings.awake {
        // No idle sleep while the harness runs, so it keeps answering like
        // a machine in the cloud would. Outside the sandbox: it only holds
        // the power assertion and waits for the harness.
        argv.splice(0..0, ["/usr/bin/caffeinate".to_string(), "-i".into()]);
    }

    // The kill switch stops it, in this terminal or in the background.
    on_stop(&settings.name, true);
    if foreground {
        service::stop(&settings.name)?;
        let _running = service::foreground_lock(&settings.name)?;
        // Beside the harness, until it ends, then stopped, its scheduler's
        // runner ended with it; or this machine's one bridge carries it
        // meanwhile, and lets it go as it ends. None where the calendar
        // wakes the runner, as for a start in the background.
        let carrying = match plan {
            Some(plan) => {
                let agent = Settings::load(&settings.name)?;
                let runner = scheduler::bridge_runner(&agent);
                match bridges::sharing() {
                    true => {
                        bridges::join(&agent, runner, true, sleep.clone())?;
                        bridges::watch();
                        None
                    }
                    false => Some(bridge::carry(&agent, &plan, runner, true)),
                }
            }
            None => None,
        };
        if let Some(confinement) = &sharing {
            shared::join(settings, &boxed, &launch, confinement, &env, true)?;
        }
        drop(configuring);
        wake_by_calendar(out, settings);
        out.step(
            &mut steps,
            phrase!("Running {} here. Ctrl-C stops it", &harness),
        );
        out.hand_over();
        // A plain SIGTERM or SIGHUP stops it as Ctrl-C does: passed on,
        // and waited for, so nothing it started keeps the agent's ports.
        // Caught before it starts: one sent as it starts is passed on
        // once it has.
        #[cfg(unix)]
        let stopped = stops::catch();
        // Timed as a service's start is, until its health check answers,
        // beside it: nothing here waits for that.
        let health = adapter.health(&boxed)?;
        let (ports, wait) = (launch.ports.clone(), start_wait(&launch));
        let timed = link_agent::times::Starting::begin(settings);
        let agent = settings.name.clone();
        std::thread::spawn(move || {
            if answers_within(&health, &ports, wait, |_| {})
                && let Ok(agent) = Settings::load(&agent)
            {
                timed.answered(&agent);
            }
        });
        // Shown here, and in its log, as a service's is.
        let log = service::Log {
            file: service::open_log(&settings.log_path(name))?,
            agent: settings.agent_id().map(str::to_string),
        };
        let status = match sleep.as_ref().map(|sleep| &sleep.job) {
            // Stopped and started again as the bridge says; a stop while
            // it's asleep ends it.
            Some(link_agent::idle::Job::Terminal(control)) => {
                link_format::daemon::logs();
                link_agent::idle::run_in_terminal(
                    &argv,
                    &env,
                    &files,
                    &log,
                    control,
                    stops::pass_to,
                    stopped,
                )
            }
            // Ended with this start, however it ends: killed outright, it
            // can pass nothing on, and what it started would keep the
            // agent's ports.
            _ => service::spawn_logged(
                link_secretive::process::ends_with_parent(
                    std::process::Command::new(&argv[0])
                        .args(&argv[1..])
                        .env_clear()
                        .envs(&env)
                        .current_dir(&files),
                    libc::SIGTERM,
                ),
                &log,
            )
            .and_then(|mut child| {
                #[cfg(unix)]
                stops::pass_to(Some(child.id()));
                child.wait()
            })
            .map_err(anyhow::Error::from),
        };
        // Its last lines shown and logged before this ends.
        service::copied(std::time::Duration::from_secs(1));
        if let Some(carrying) = &carrying {
            carrying.stop();
        }
        // Stopped, as `lnk agent stop` stops one in the background: the
        // calendar no longer wakes its scheduler.
        service::remove_calendar(&settings.name);
        // What its proxy was given goes with it.
        crate::confine::forget_routes(settings);
        let status = status.within(format!("running {}", argv[0]))?;
        bridges::leave(&settings.name);
        // Its owner's harness serves the rest without it.
        if sharing.is_some() {
            let _ = std::fs::remove_file(settings::shared_record_of(
                &settings.owner()?,
                &settings.key(),
            ));
        }
        #[cfg(unix)]
        if stopped.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(None);
        }
        if !status.success() {
            return Err(Report::error(sentence!("{} stopped.", &harness))
                .why(prose!("It ended with {}.", status.to_string()))
                .next(cmd!("lnk agent log show"), "see why"));
        }
        return Ok(None);
    }

    if !boxed.sandbox {
        argv = unsandboxed_argv(&settings.name, argv)?;
    }
    watch_measures();
    // Its own bridge service, unless this machine's one bridge carries
    // it: it joins once its service runs.
    let shared_bridge = plan.is_some() && bridges::sharing();
    let bridge = match plan {
        Some(_) if !shared_bridge => Some(bridge_job(&settings.name)?),
        _ => None,
    };
    let timed = link_agent::times::Starting::begin(settings);
    own.armed = false;
    if let Some(stopped) = stopped {
        stopped.armed = false;
    }
    service::start(
        &settings.name,
        &Job {
            argv,
            env: env.clone(),
            workdir: files.clone(),
            log: settings.log_path(name),
        },
        bridge.as_ref(),
    )?;
    if let Some(confinement) = &sharing {
        shared::join(settings, &boxed, &launch, confinement, &env, false)?;
    }
    drop(configuring);
    if shared_bridge {
        let agent = Settings::load(&settings.name)?;
        bridges::join(&agent, scheduler::bridge_runner(&agent), false, sleep)?;
    }
    wake_by_calendar(out, settings);
    // With services off (`LNK_SERVICES=off`) none was started: nothing to
    // wait for.
    if !link_service::managed() {
        out.block(Note(sentence!(
            "LNK_SERVICES=off: no service was started, so it isn't running, and {} runs it in \
             this terminal.",
            cmd!("lnk agent start --foreground")
        )));
        return Ok(None);
    }
    out.step(&mut steps, phrase!("Starting {}", &harness));
    // Its health check, wrapped once.
    let health = adapter.health(&boxed)?;
    let wait = start_wait(&launch);
    // A real background service now owns this start. A foreground exit,
    // failed start or services-off return leaves the guard armed.
    binding_lifetime.armed = false;
    Ok(Some(Started {
        adapter,
        health,
        ports: launch.ports,
        wait,
        timed,
    }))
}

mod binding;
mod guards;
mod next;
use guards::BindingUntilStarted;
pub(crate) use guards::StoppedUnlessStarted;
use next::running_next;

/// Has the calendar wake the agent's scheduler when its spec says `cron`
/// ([`scheduler::calendar`]), in this terminal or in the background;
/// what fails is said, and the start goes on.
fn wake_by_calendar(out: &mut Out, settings: &Settings) {
    if let Err(err) = scheduler::calendar(settings) {
        out.block(
            Report::warning(sentence!(
                "Your agent's scheduler isn't woken by the calendar."
            ))
            .why(Sentence::adopt(&format!("{err:#}")))
            .next(cmd!("lnk agent start"), "try again"),
        );
    }
}

/// What a harness that couldn't configure itself says: its own program
/// failing (OpenClaw exiting 2) is the harness's, refused as such, never a
/// fault of Link's to report.
fn configure_failed(harness: &Name, err: &anyhow::Error) -> Report {
    Report::error(sentence!("{} couldn't configure itself.", harness))
        .why(Sentence::adopt(&format!("{err:#}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A harness's configure failing is refused as its own, without the
    /// line asking to report a fault of Link's.
    #[test]
    fn a_failed_configure_is_the_harnesss() {
        let err = anyhow::anyhow!("configuring openclaw failed (exit status: 2)");
        let report = configure_failed(&Name::new("harness", "openclaw"), &err);
        assert!(!report.is_bug());
        assert!(report.headline.to_string().contains("openclaw"));
    }

    /// One running in a terminal is refused before anything is set up
    /// again: what it reaches its model with stays what it was given.
    #[test]
    fn a_start_refuses_one_in_a_terminal_before_setting_up() {
        let _config = crate::tests::own_config();
        let mut settings = Settings::default();
        settings.name = "in-terminal".into();
        let _running = service::foreground_lock(&settings.name).unwrap();
        let (mut out, _) = Out::capture(false, &["lnk-agent", "start"]);
        let refused = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(launch_after(
                &mut out,
                &settings,
                "absent-harness",
                false,
                None,
            ));
        let refused = refused.err().expect("it started");
        let err = refused.headline.to_string();
        assert!(err.contains("runs in a terminal"), "{err}");
        let actions: toml::Value = crate::navigation::navigation();
        for action in [
            "agent start",
            "agent create",
            "agent restart",
            "agent harness set",
        ] {
            let fail: Vec<String> = actions[action]["fail"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect();
            let typed = link_format::Command::fill(&format!("lnk {action}"), vec![]);
            for next in &refused.next.0 {
                assert!(
                    link_format::nexts::declares(&fail, &next.command, &typed),
                    "{action} must declare its foreground refusal hint: {}",
                    next.command.text()
                );
            }
        }
    }
}
