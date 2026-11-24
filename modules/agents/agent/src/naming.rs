//! People name agents; actions take their IDs. Every agent action names
//! its agent by its ID, `agent:<id>`, so what may act on it is checked by
//! the agent itself, whatever it's called. On the command line a person
//! types the agent's name, and [`lookup`], which this program gives the
//! command-line parser for the kind `agent` before it reads a command,
//! finds that name in the person's agents and gives the parser the ID.
//! [`name_of`] goes back: the agent an ID names, by the name this
//! machine's commands keep its files under.

use link_agent::moved;
use link_agent::settings::{self, Settings, ids};

/// The ID of the agent named `name`, written `agent:<id>`: an agent here
/// by its settings, else one that moved from here by its note, else one
/// of the person's agents anywhere they list (`lnk agent list`).
pub(crate) fn lookup(name: &str) -> Result<String, String> {
    let id = id_of(name).map_err(|why| format!("{why:#}"))?;
    TYPED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((id.clone(), name.to_string()));
    Ok(id)
}

/// Each ID [`lookup`] found this run, by the name the person typed for it
/// (`main`, `main@host`): what this machine's commands take it by.
static TYPED: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

fn id_of(named: &str) -> anyhow::Result<String> {
    let (name, place) = match named.split_once('@') {
        Some((name, place)) => (name, Some(place)),
        None => (named, None),
    };
    if place.is_none() {
        if let Some(id) = settings::id_of(name) {
            return Ok(ids::written(&id));
        }
        if Settings::path_of(name).exists() {
            // Settings that don't read say why first; an agent made by
            // `lnk agent create` has its ID from then on.
            let settings = Settings::load_without_keys(name)?;
            anyhow::bail!(
                "agent {} has no ID: its settings were never saved by Link; lnk agent create makes an agent with one",
                settings.name
            );
        }
    }
    // One that moved from here, by its note, which keeps its ID while
    // where it went doesn't answer.
    if let Some(id) = moved::of_name(name)?
        .filter(|note| place.is_none_or(|place| note.to.name == place))
        .and_then(|note| note.id)
    {
        return Ok(ids::written(&id));
    }
    // One of the person's agents on another machine, as it answers.
    let found = crate::moving::find(Some(named))?;
    let id = found
        .id
        .ok_or_else(|| anyhow::anyhow!("agent {named} has no ID yet"))?;
    Ok(ids::written(&id))
}

/// The name of the agent `id` names, `agent:<id>`: as the person typed it
/// for this run's lookup, else the agent here that has it, or that moved
/// from here with it (`Settings::name_of`), else
/// the one of the person's agents on another machine that answers with
/// it, never a note of where one went.
pub(crate) fn name_of(id: &str) -> anyhow::Result<String> {
    let bare = ids::parse(id)?;
    if let Some((_, typed)) = TYPED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(found, _)| found == id)
    {
        return Ok(typed.clone());
    }
    let here = Settings::name_of(id);
    if here.is_ok() || moved::of_id(&bare)?.is_some() {
        return here;
    }
    let gathered = crate::listing::gather(false, true)?;
    let found = gathered
        .agents
        .iter()
        .filter(|agent| agent["location"].is_null())
        .find(|agent| crate::listing::workload_of(agent).as_deref() == Some(bare.as_str()));
    // Named with the machine it runs on, so no agent of its name anywhere
    // else is taken for it.
    let named = found.and_then(|agent| {
        let name = agent["name"].as_str()?;
        let machine = agent["machine"]["name"]
            .as_str()
            .map(str::to_string)
            .or_else(|| crate::listing::machine_of(agent))?;
        Some(format!("{name}@{machine}"))
    });
    match named {
        Some(named) => Ok(named),
        None => here,
    }
}
