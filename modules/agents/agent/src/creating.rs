//! Agents made and started: `lnk agent start`, `create` (here or on a
//! box) and `route`.

use link_agent::cli::Starting;
use link_agent::{
    adapter::Adapter,
    sandbox_file, service,
    settings::{self, Settings},
};
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Prose, Report, Sentence, WithinExt, cmd, prose, sentence,
};

use crate::granting::restart_with;
use crate::launching::up;
use crate::model::{first_run, set_fallback};
use crate::{absolute, moving, tools};

/// `lnk agent start`, for `agent`.
pub(crate) async fn start(
    out: &mut Out,
    agent: &mut Settings,
    starting: &Starting,
) -> Result<Done, Report> {
    // Never started here: no settings saved yet.
    let new = !agent.path().exists();
    if new && let Some(guest) = crate::machine::create(&agent.name, None, starting)? {
        return new_in_guest(
            out,
            &guest,
            &agent.name,
            agent.harness.as_deref(),
            starting,
            false,
        );
    }
    if !new
        && (starting.fields.get(&["isolation", "vm"]).is_some()
            || starting.fields.get(&["env"]).is_some())
    {
        return Err(Report::error(sentence!(
            "Couldn't give {} a VM or an environment.",
            Name::new("agent", &agent.name)
        ))
        .why(prose!(
            "An existing agent keeps the inputs it was created with; create another agent with them."
        ))
        .next(
            cmd!("lnk agent create <name> --env <file>"),
            "create an agent with an environment",
        )
        .exit(Exit::Usage));
    }
    // A new agent's environment, set up here: captured before anything
    // is asked, so a file it can't take refuses the agent too.
    let environment = match new {
        true => crate::environment::capture(starting)?,
        false => None,
    };
    // A supplied spec must be installed before choosing any start settings.
    let had_model = agent.model.is_some();
    let supplied = supplied(agent, starting)?;
    match &supplied {
        Some((spec, origin)) => keep(agent, spec, origin)?,
        None if new && !Settings::path_of(&agent.name).exists() => keep_defaults(agent)?,
        None => {}
    }
    let spec = agent.spec()?;
    let run = spec.spec.run.clone().unwrap_or_default();
    agent.awake = !(starting.let_sleep || run.let_sleep.unwrap_or(false));
    if let Some(files) = run.files {
        agent.files = Some(absolute(files)?);
    }
    // Its programs found where yours are: the folders of your `PATH`.
    agent.programs = link_agent::programs::caller();
    // No sandbox from `--no-sandbox` until `--sandbox`: neither given, as
    // it was.
    if starting.no_sandbox {
        link_agent::machine_settings::check_unsandboxed().within("starting without a sandbox")?;
        agent.sandbox = false;
    }
    if starting.sandbox.is_some() {
        agent.sandbox = true;
    }
    // A sandbox this machine can't make, said before anything is asked:
    // a model's questions and its key answered for nothing otherwise.
    if agent.sandbox {
        crate::confine::sandbox_can_run()?;
    }
    let running = service::running(&agent.name);
    let model_of = |agent: &Settings| {
        agent
            .model
            .as_ref()
            .map(|m| m.name.clone().unwrap_or_else(|| m.provider.clone()))
    };
    let before = model_of(agent);
    let first = agent.model.is_none();
    // The model it thinks with: the one `--model.default` names, or the
    // supplied spec's for an agent that has none yet.
    let wanted = starting.given(&["model", "default"]).or_else(|| {
        supplied
            .as_ref()
            .filter(|_| !had_model)
            .and_then(|(own, _)| own.model.as_ref()?.default.clone())
    });
    first_run(out, agent, wanted, Some(&spec)).await?;
    set_fallback(out, agent)?;
    if let Some(note) = agent.check_files()? {
        out.block(Note(Sentence::adopt(&note)));
    }
    // Saved, so a later default doesn't switch it.
    agent
        .harness
        .get_or_insert_with(|| settings::DEFAULT_HARNESS.into());
    agent.settle_owner()?;
    agent.save()?;
    if let Some(path) = starting.sandbox_file() {
        let sandbox = sandbox_file::keep(&agent.name, path)?;
        out.block(Note(sentence!(
            "Its grants: {}.",
            Foreign::new(&sandbox_file::summary(&sandbox))
        )));
    }
    // Which of Link's tools it gets, asked once rather than left to be
    // found in the docs.
    if new {
        sandbox_file::initialize(&agent.name)?;
    }
    tools::offer(out, agent, first)?;
    agent.save()?;
    // Its environment, set up natively the first time, and its services
    // started with it: in the agent's sandboxes, or directly as it runs.
    match environment {
        Some(environment) => crate::environment::create(&agent.name, environment, !agent.sandbox),
        None => crate::environment::start(&agent.name, !agent.sandbox),
    }?;
    // Started again while it runs: say so, and what changed.
    let after = model_of(agent);
    if let (Some(was), Some(now)) = (&before, &after)
        && was != now
    {
        out.block(Note(sentence!(
            "Its model is {} now, and was {}.",
            Name::new("model", now),
            Name::new("model", was)
        )));
    }
    if running && !starting.foreground {
        out.block(Note(sentence!(
            "{} is running, so it restarts.",
            Name::new("agent", &agent.name)
        )));
    }
    let name = agent.harness().to_string();
    up(out, agent, &name, starting.foreground).await
}

/// The complete agent command, including hooks and harness installation, runs in the guest.
pub(crate) fn new_in_guest(
    out: &mut Out,
    on: &str,
    name: &str,
    harness: Option<&str>,
    starting: &Starting,
    yes: bool,
) -> Result<Done, Report> {
    let args = guest_args(on, name, harness, starting, yes)?;
    Ok(Done::passed(crate::machine::forward(out, on, args)?))
}

/// What `lnk agent create` is run with in the guest `on`, its inputs copied there.
fn guest_args(
    on: &str,
    name: &str,
    harness: Option<&str>,
    starting: &Starting,
    yes: bool,
) -> anyhow::Result<Vec<std::ffi::OsString>> {
    moving::machines::ready_box(on, false, harness)?;
    let mut args: Vec<std::ffi::OsString> = ["agent", "create", name]
        .into_iter()
        .map(Into::into)
        .collect();
    if let Some(harness) = harness {
        args.extend(["--harness".into(), harness.into()]);
    }
    guest_inputs(on, name, starting, yes, args)
}

/// What `lnk agent start` of agent `name`, which lives in the guest `on`,
/// is run with there: its spec as here, a file with its flags over it,
/// copied as `new_in_guest` copies a new agent's, since the guest has
/// neither the file nor the folder its path fields were given from.
/// Flags alone are forwarded: they're over the agent's own spec, which
/// is the guest's.
pub(crate) fn start_in_guest(
    out: &mut Out,
    on: &str,
    name: &str,
    starting: &Starting,
    yes: bool,
    arguments: &[std::ffi::OsString],
) -> Result<Done, Report> {
    let args: Vec<std::ffi::OsString> = match starting.spec {
        Some(_) => {
            let start = ["agent", "start", "--agent", name].map(Into::into).to_vec();
            guest_inputs(on, name, starting, yes, start)?
        }
        // This program's arguments never hold its namespace: `lnk agent`
        // runs it again there.
        None => std::iter::once("agent".into())
            .chain(arguments.iter().cloned())
            .collect(),
    };
    Ok(Done::passed(crate::machine::forward(out, on, args)?))
}

/// `args` with a start's own flags, and its spec and sandbox file copied
/// into the guest `on` and named there.
fn guest_inputs(
    on: &str,
    name: &str,
    starting: &Starting,
    yes: bool,
    mut args: Vec<std::ffi::OsString>,
) -> anyhow::Result<Vec<std::ffi::OsString>> {
    if starting.no_sandbox {
        args.push("--no-sandbox".into());
    }
    if starting.foreground {
        args.push("--foreground".into());
    }
    if starting.let_sleep {
        args.push("--let-sleep".into());
    }
    if yes {
        args.push("--yes".into());
    }
    // The spec it starts with, as here: its file with its flags over it,
    // or the flags over Link's defaults. A copied agent spec
    // cannot recursively select another VM: inputs were sealed here.
    let spec = supplied(&Settings::new(name), starting)?
        .map(|(mut spec, _)| {
            spec.isolation = Default::default();
            spec.env = None;
            link_spec::print_spec(&spec)
        })
        .transpose()?;
    let sandbox = starting
        .sandbox_file()
        .map(std::fs::read_to_string)
        .transpose()?;
    // A folder of its own there, made new (`mktemp -d`), never one named
    // after the agent, which anyone on the guest could make first.
    let mut staged = None;
    for (flag, text, basename) in [
        ("--spec", spec, "agent.toml"),
        ("--sandbox", sandbox, "sandbox.toml"),
    ] {
        if let Some(text) = text {
            let dir = match &staged {
                Some(dir) => dir,
                None => staged.insert(staging_dir(on)?),
            };
            let dest = format!("{dir}/{basename}");
            let mut copy = link_core::lnk();
            copy.args(["box", "ssh", on, "--"])
                .arg(format!("umask 077; cat > '{dest}'"))
                .stdin(std::process::Stdio::piped());
            let mut child = copy.spawn()?;
            std::io::Write::write_all(
                &mut child.stdin.take().within("the guest spec input")?,
                text.as_bytes(),
            )?;
            anyhow::ensure!(
                child.wait()?.success(),
                "could not copy the agent's inputs into the guest {on}"
            );
            args.extend([flag.into(), dest.into()]);
        }
    }
    Ok(args)
}

/// A new folder for an agent's inputs in the guest `on`, only its user's:
/// `mktemp -d` there, its path read back and checked.
fn staging_dir(on: &str) -> anyhow::Result<String> {
    let made = link_core::lnk()
        .args(["box", "ssh", on, "--"])
        .arg(STAGING)
        .stdin(std::process::Stdio::null())
        .output()?;
    anyhow::ensure!(
        made.status.success(),
        "could not make a folder for the agent's inputs in the guest {on}"
    );
    staged_path(&String::from_utf8_lossy(&made.stdout))
}

/// What makes [`staging_dir`]'s folder in the guest, under [`STAGED`].
const STAGING: &str = "umask 077; mktemp -d /tmp/lnk-agent.XXXXXXXXXX";

/// What every folder [`STAGING`] makes starts with.
const STAGED: &str = "/tmp/lnk-agent.";

/// The folder `said`, what [`STAGING`] printed, if it's one `mktemp`
/// makes: `/tmp/lnk-agent.` and letters and digits, nothing a shell's
/// quotes would end at.
fn staged_path(said: &str) -> anyhow::Result<String> {
    let path = said.trim();
    let plain = path
        .strip_prefix(STAGED)
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric()));
    anyhow::ensure!(plain, "the guest made no folder for the agent's inputs");
    Ok(path.to_string())
}

/// The spec a start supplies, and where it's from: its `--spec` file
/// with its field flags over it, or the flags over the agent's own spec
/// (Link's personal profile for a new agent), read strictly once. None
/// when it's given neither.
pub(crate) fn supplied(
    agent: &Settings,
    starting: &Starting,
) -> anyhow::Result<Option<(link_agent_spec::Spec, String)>> {
    if let Some(path) = &starting.spec {
        let made = link_spec::read_with::<link_agent_spec::Spec>(Some(path), &starting.fields)?;
        let origin = match starting.fields.is_empty() {
            true => path.display().to_string(),
            false => format!("{} with its field flags", path.display()),
        };
        return Ok(Some((made.spec, origin)));
    }
    if starting.fields.is_empty() {
        return Ok(None);
    }
    let own = agent.own_spec()?.unwrap_or_default();
    let spec = link_spec::over(&own, &starting.fields)?;
    Ok(Some((spec, "its field flags".into())))
}

/// A new agent's own spec: Link's defaults, which its owner edits from
/// there (`lnk agent spec show`).
fn keep_defaults(agent: &mut Settings) -> anyhow::Result<()> {
    keep(agent, &link_agent_spec::Spec::default(), "Link's defaults")
}

/// Keeps `spec`, read as `text` from `origin`, as `agent`'s own: checked
/// first, and the one it ran before kept when the whole fails.
fn keep(agent: &mut Settings, spec: &link_agent_spec::Spec, origin: &str) -> anyhow::Result<()> {
    link_agent_spec::check(spec).within(format!("in {origin}"))?;
    let mut behavior = agent
        .resolve_spec(Some(spec))
        .within(format!("in {origin}"))?
        .spec;
    behavior.extends = None;
    behavior.name = None;
    behavior.harness = None;
    let existing = agent.path().exists();
    if existing {
        agent.save()?;
        settings::configuration::write_behavior(agent, &link_spec::print_spec(&behavior)?)?;
    } else {
        // A new agent stays in memory until model selection has succeeded.
        // Saving its defaults here would leave an agent after a refused start.
        agent.behavior = Some(behavior);
    }
    if let Some(harness) = &spec.harness {
        Adapter::find_or_add(&harness.name)?;
        agent.harness = Some(harness.name.clone());
        if existing {
            agent.save()?;
        }
    }
    Ok(())
}

/// `lnk agent create`: another agent here, made and started.
pub(crate) async fn new(
    out: &mut Out,
    name: &str,
    harness: Option<String>,
    starting: &Starting,
) -> Result<Done, Report> {
    link_agent::settings::check_new_name("agents", name)?;
    let shown = Name::new("agent", name);
    if Settings::path_of(name).exists() {
        return Err(Report::error(sentence!(
            "There is an agent named {} here already.",
            &shown
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent start --agent {}", &shown), "start it"));
    }
    let mut agent = Settings::new(name);
    if let Some(harness) = harness {
        Adapter::find_or_add(&harness)?;
        agent.harness = Some(harness);
    }
    // Its ID, reserved before anything of it is written, and retired
    // when nothing of it was.
    let id = link_agent::settings::ids::reserve()?;
    agent.id = Some(link_agent::settings::ids::workload(&id)?);
    out.block(Note(sentence!(
        "A new agent, {}, with its own settings, files and harness.",
        &shown
    )));
    let started = start(out, &mut agent, starting).await;
    // A refused create's own error is the one it reports.
    let retired = unsaved(&agent, &id);
    let done = started?;
    retired?;
    Ok(done)
}

/// Retires the IDs a create reserved for `agent` when its settings were
/// never saved here: a create refused before then, or one made in a
/// guest, which mints its own. A saved agent keeps them.
fn unsaved(agent: &Settings, id: &str) -> anyhow::Result<()> {
    if Settings::path_of(&agent.name).exists() {
        return Ok(());
    }
    link_agent::settings::ids::retire(id)?;
    if let Some(service) = &agent.service {
        link_agent::settings::ids::SERVICES.retire(service)?;
    }
    Ok(())
}

/// A route as `lnk agent route` takes it: `none` (its own network), else
/// its proxy's upstream (`direct` or a SOCKS address).
pub(crate) fn route(upstream: &str) -> anyhow::Result<Option<String>> {
    Ok(match upstream {
        "none" => None,
        other => {
            link_sandbox_contract::net::Upstream::parse(other)?;
            Some(other.to_string())
        }
    })
}

/// `lnk agent route <upstream> --default`: this machine's default route,
/// for every agent made here from now on, and for each agent here with
/// none yet (restarted if it runs). No agent is made for it.
pub(crate) async fn route_default(out: &mut Out, upstream: &str) -> anyhow::Result<()> {
    let upstream = route(upstream)?;
    settings::Defaults {
        upstream: upstream.clone(),
    }
    .save()?;
    let Some(upstream) = upstream else {
        return Ok(());
    };
    for name in settings::names()? {
        let mut agent = Settings::load(&name)?;
        if agent.upstream.is_some() {
            continue;
        }
        agent.upstream = Some(upstream.clone());
        crate::lossy(restart_with(out, &mut agent).await)?;
    }
    Ok(())
}

/// `lnk agent create --cloud <cloud>`: made by a cloud account that hosts
/// agents, with its defaults or from the agent `from` it lists, and billed by
/// that cloud: made once you say yes (`yes`, `--yes`).
pub(crate) fn new_hosted(
    out: &mut Out,
    cloud: &str,
    name: &str,
    from: Option<&str>,
    harness: Option<&str>,
    starting: &Starting,
    yes: bool,
) -> Result<Done, Report> {
    link_machine::machine::check_name(name)?;
    let usage = |headline: Sentence, why: Prose| Report::error(headline).why(why).exit(Exit::Usage);
    let Some(account) = link_agent_place::account(cloud).filter(link_agent::hosted::runs_agents)
    else {
        return Err(usage(
            sentence!("{} doesn't host agents.", Name::new("cloud", cloud)),
            prose!("Make the agent on a box of it with --on, or here."),
        ));
    };
    if harness.is_some_and(|h| h != "link") {
        return Err(usage(
            sentence!("The hosting account creates its own harness."),
            prose!("Leave out --harness."),
        ));
    }
    if starting.supplied()
        || starting.sandbox.is_some()
        || starting.foreground
        || starting.no_sandbox
        || starting.let_sleep
    {
        return Err(usage(
            sentence!("A hosting account creates an agent with its defaults."),
            prose!("Leave out the local start settings."),
        ));
    }
    crate::moving::hosted_billed(out, cloud, name, yes)?;
    let reply = link_agent::hosted::call(&account, "agent-create", |request| {
        request.create = Some(link_agent_place::Create {
            name: name.into(),
            from: from.map(str::to_string),
        });
    })?;
    link_agent::hosted::unless_refused(&account.name, &reply)?;
    let agent = reply
        .agent
        .within("the hosting account named no created agent")?;
    link_agent_place::check_hosted(std::slice::from_ref(&agent))?;
    if agent.name != name {
        return Err(Report::error(sentence!(
            "The hosting account created another agent's name."
        )));
    }
    crate::moved::Note::new(
        Some(agent.id),
        name,
        crate::moved::Place::of_account(&account.name, agent.owner),
    )
    .save()?;
    Ok(Done::success(sentence!(
        "Created {} on {}.",
        Name::new("agent", name),
        Name::new("account", &account.name)
    ))
    .next(cmd!("lnk agent list"), "see it"))
}

/// `lnk agent create --on <box>`: the same, run on the box (`lnk box run`),
/// its questions asked there. It runs `create` there.
pub(crate) fn new_on(
    out: &mut Out,
    on: &str,
    name: &str,
    harness: Option<&str>,
    starting: &Starting,
    yes: bool,
) -> Result<Done, Report> {
    link_machine::machine::check_name(name)?;
    let usage = |headline: Sentence, why: Prose| Report::error(headline).why(why).exit(Exit::Usage);
    if let Some(selector) = crate::machine::borrow(name, on)? {
        return new_in_guest(out, &selector, name, harness, starting, yes);
    }
    if starting.foreground {
        return Err(usage(
            sentence!("An agent on a box runs in the background."),
            prose!("Leave out --foreground."),
        ));
    }
    if starting.spec.is_some() || starting.sandbox_file().is_some() {
        return Err(usage(
            sentence!("--spec and --sandbox read a file on this machine, not the box's."),
            prose!(
                "Make the agent here with them, then move it to {}, which carries both.",
                Name::new("box", on)
            ),
        )
        .next(
            cmd!(
                "lnk agent move --on {} --agent {}",
                Name::new("box", on),
                Name::new("agent", name)
            ),
            "move it once made",
        ));
    }
    moving::machines::ready_box(on, false, harness)?;
    let mut args: Vec<std::ffi::OsString> = ["box", "run", on, "--", "agent", "create", name]
        .map(Into::into)
        .to_vec();
    if let Some(harness) = harness {
        args.extend(["--harness".into(), harness.into()]);
    }
    // The spec's field flags, given there as here: a spec file is this
    // machine's, refused above.
    args.extend(starting.fields.args());
    if starting.no_sandbox {
        args.push("--no-sandbox".into());
    }
    if starting.let_sleep {
        args.push("--let-sleep".into());
    }
    if yes {
        args.push("--yes".into());
    }
    let mut run = link_core::lnk();
    run.args(&args);
    // Never this machine's key: the box asks for its own.
    for var in settings::key_vars() {
        run.env_remove(var);
    }
    out.hand_over();
    let status = run.status().within("running lnk box run")?;
    Ok(Done::passed(status.code().unwrap_or(1)))
}

#[cfg(test)]
mod tests {
    /// An agent's inputs in a guest go in a folder `mktemp -d` made new,
    /// read back as one: never a path named after the agent, nor one
    /// that ends a shell's quotes.
    #[test]
    fn an_agent_made_on_a_box_stages_in_a_folder_of_its_own() {
        assert!(super::STAGING.contains("mktemp -d"));
        let made = format!("{}a1B2c3D4e5", super::STAGED);
        assert_eq!(super::staged_path(&format!("{made}\n")).unwrap(), made);
        let named = super::STAGED.replace('.', "-");
        for bad in [
            String::new(),
            format!("{named}main"),
            format!("{}x'y", super::STAGED),
            super::STAGED.to_string(),
        ] {
            assert!(super::staged_path(&bad).is_err(), "{bad:?}");
        }
    }
}
