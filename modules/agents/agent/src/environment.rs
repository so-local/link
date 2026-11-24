//! An agent's environment on this machine, set up natively as `lnk env
//! setup --here` does: no VM box is made for it. Its stages run each in
//! its sandbox, or directly while the agent runs with no sandbox, and its
//! services start with the agent and stop with it.
//!
//! The environment is captured once, when the agent is created (`lnk box
//! machine`, Box's), and kept in `~/.lnk/safe/agents/environments`: what
//! it runs is never read from its source again.

use link_agent::cli::Starting;
use link_box_contract::{EnvironmentInput, HereOperation, HereResponse, InputReferences};
use link_format::{Report, WithinExt};
use serde::{Deserialize, Serialize};

/// What an agent keeps of its environment.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    environment: EnvironmentInput,
    /// Whether the stages an image would save ran: once, at its first
    /// start, and again only if that didn't finish.
    set_up: bool,
}

link_files::specification!(Kept, include_str!("environment.schema.toml"));

fn path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    link_machine::machine::check_name(name)?;
    Ok(link_files::layout::Hidden::AgentEnvironments
        .try_path()?
        .join(format!("{name}.json")))
}

fn save(name: &str, kept: &Kept) -> anyhow::Result<()> {
    let path = path(name)?;
    if let Some(dir) = path.parent() {
        link_files::create_private_dir(dir)?;
    }
    Ok(link_files::write_private(
        &path,
        &link_files::saved::print_json(kept)?,
    )?)
}

fn load(name: &str) -> anyhow::Result<Option<Kept>> {
    let path = path(name)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(
            link_files::saved::parse_json(&bytes).within(format!("reading {}", path.display()))?,
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).within(format!("reading {}", path.display())),
    }
}

/// The environment `starting` names (its flag, or its spec's), captured
/// by Box: none when it names none.
pub(crate) fn capture(starting: &Starting) -> anyhow::Result<Option<EnvironmentInput>> {
    let refs = crate::machine::refs(starting)?;
    if refs.environment.is_none() {
        return Ok(None);
    }
    let HereResponse::Inputs(inputs) = crate::machine::call(
        None,
        HereOperation::Capture(InputReferences {
            vm: None,
            environment: refs.environment,
        }),
    )?
    else {
        anyhow::bail!("box bridge returned no captured inputs")
    };
    Ok(inputs.environment)
}

/// A new agent `name`'s environment kept, then set up and started.
pub(crate) fn create(
    name: &str,
    environment: EnvironmentInput,
    direct: bool,
) -> Result<(), Report> {
    save(
        name,
        &Kept {
            environment,
            set_up: false,
        },
    )?;
    start(name, direct)
}

/// Agent `name`'s environment, when it has one, started with it: set up
/// first if it isn't yet, then its stages at every start and its
/// services. `direct`: in no sandbox, as the agent runs.
pub(crate) fn start(name: &str, direct: bool) -> Result<(), Report> {
    let Some(mut kept) = load(name)? else {
        return Ok(());
    };
    let (_staged, read) = link_env_contract::environment::staged(&kept.environment)?;
    if !kept.set_up {
        link_env_contract::remote::set_up_here(&read, direct)?;
        kept.set_up = true;
        save(name, &kept)?;
    }
    link_env_contract::remote::start_here(&read, direct)?;
    Ok(())
}

/// Agent `name`'s environment's services, stopped with it.
pub(crate) fn stop(name: &str) -> Result<(), Report> {
    let Some(kept) = load(name)? else {
        return Ok(());
    };
    let (_staged, read) = link_env_contract::environment::staged(&kept.environment)?;
    link_env_contract::remote::stop_here(&read)?;
    Ok(())
}

/// Agent `name`'s environment stopped and forgotten, with the agent.
pub(crate) fn remove(name: &str) -> Result<(), Report> {
    stop(name)?;
    match std::fs::remove_file(path(name)?) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(anyhow::Error::from(err).into())
        }
        _ => Ok(()),
    }
}
