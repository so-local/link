//! `lnk agent memory`: a memory engine over the agent's files, served to
//! whichever harness runs it as a tool outside its sandbox (`lnk sandbox
//! tool add`), through its proxy, at `$LNK_MEMORY`.
//!
//! The engine is an adapter answering `link_memory_tool_contract` (Link Memory
//! by default, [`settings::MEMORIES`]). Its tool is the agent's
//! (`memory-<agent>`), defined again at a start when the settings, the
//! engine or Link changed, or the tool is gone, so it follows the agent
//! to another machine and a new release of the engine. The engine runs
//! confined too: it reads Home, the conversations and skills of the
//! agent's own folder, and its own program, and writes only its state
//! folder ([`Settings::memory_state`]), its HOME.
//! The embedding model it asks for is served for it outside, at each
//! start of its tool ([`serve`]), and its sandbox is given the address.
//! Where the agent may write its instructions, the engine writes them
//! too, for its notes alone ([`notes_of`]).
//!
//! A team with a shared folder may have a memory of its own over it,
//! once its owner turns it on (`lnk agent team-memory start`,
//! [`define_team`]): the same engine, one server for every member
//! (`lnk sandbox tool add --shared`), offered to members only.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{bail, ensure};
use link_agent::kept::{Kept, Restart, Watch};
use link_agent::settings::{self, Settings};
use link_format::{
    Done, Fields, Foreign, Name, Note, Out, Path as Where, Product, Question, Report, Value,
    WithinExt, cmd, prose, sentence,
};
use link_harness_contract::harness::layout;
use link_memory_tool_contract::{FROM_ENV, MODEL_ENV, NOTES_ENV};
mod connecting;
pub(crate) mod defining;
mod embedding;
mod engine;
pub(crate) mod recording;
pub use connecting::{connect, service};
pub(crate) use defining::read_fingerprint;
use defining::{Defining, Store, define_tool, own_reads};
pub use defining::{define, notes_of, store_drivers};
pub use embedding::serve;
use embedding::{Served, model_placeholder};
pub use engine::Engine;
pub(crate) use engine::confined;
use engine::{Serving, confined_writing};
use recording::{
    Record, Whose, current_generation, generation, records, socket_name, stamps, working,
};

/// The tool team `id`'s memory is served as.
pub fn team_tool_of(id: &str) -> String {
    format!("team-{id}-memory")
}

/// Its record, beside the team's file: there while it's defined.
fn team_record(team: &str) -> PathBuf {
    link_files::layout::Hidden::TeamMemory
        .path()
        .join(format!("{team}.json"))
}

/// The tool of `team`'s memory, when it's defined.
pub fn team_tool(team: &link_agent_spec::Team) -> Option<String> {
    team_record(&team.name)
        .exists()
        .then(|| team_tool_of(&team.id))
}

/// Defines `team`'s memory as its tool, as it should be now: Link Memory
/// over its shared folder, its state with the team's
/// (`~/Link/Teams/<team>/memory`), its opinions the team's spec's over
/// Link's, confined, one server for every member. None, and forgotten,
/// for a team with no shared folder or its memory off (its spec's
/// `[settings.memory] enabled`).
pub fn define_team(out: &mut Out, team: &link_agent_spec::Team) -> Result<Option<String>, Report> {
    let Some(folder) = team
        .shared
        .as_ref()
        .filter(|_| team.settings.memory.enabled)
    else {
        forget_team(team);
        return Ok(None);
    };
    let known = link_agent_spec::Known {
        name: team.name.clone(),
    };
    let spec = link_agent_spec::resolve(None, std::slice::from_ref(team), &known)?;
    let whose = format!("Team {}'s memory", team.name);
    let engine = Engine::find_or_add(out, settings::DEFAULT_MEMORY, &whose, false)?;
    let defining = Defining {
        folder: folder.clone(),
        own: None,
        state: settings::team_dir(&team.name)
            .join("memory")
            .join(&engine.name),
        notes: None,
        serving: Serving {
            spec: serde_json::to_string(&spec.tool("memory")).within("writing its memory spec")?,
            sandbox: true,
            // A team's writes no notes.
            from: false,
        },
        record: team_record(&team.name),
        shared: true,
        owner: None,
        agent: None,
        store: Store::default(),
        engine,
        tool: team_tool_of(&team.id),
    };
    Ok(define_tool(&defining).map(Some)?)
}

/// Forgets `team`'s memory, its tool too, if it was defined.
pub fn forget_team(team: &link_agent_spec::Team) {
    if std::fs::remove_file(team_record(&team.name)).is_ok() {
        let _ = link_core::lnk()
            .args(["sandbox", "tool", "remove", &team_tool_of(&team.id)])
            .output();
        crate::confine::forget_defined_tools();
    }
}

/// The warning for team `team`'s memory, unserved for `err`. A start
/// again meets the same engine, so its next steps are what helps: the
/// engine from the build `lnk` came from, or the team's memory off.
pub fn team_warning(team: &str, err: &Report) -> Report {
    let shown = Name::new("team", team);
    let engine = Name::new(
        "module",
        settings::memory_module_of(settings::DEFAULT_MEMORY),
    );
    Report::warning(sentence!("The memory of team {} isn't served.", &shown))
        .why(link_format::Sentence::adopt(&err.headline.to_string()))
        .next(
            cmd!("lnk up --source <checkout> --module {}", &engine),
            "install its engine from the build lnk came from",
        )
        .next(
            cmd!("lnk agent team-memory stop {}", &shown),
            "turn the team's memory off",
        )
}

/// `lnk agent memory start`: `engine` (else Link Memory) for `agent`,
/// installed and defined. Whether it changed, and how it ended.
pub fn start_engine(
    out: &mut Out,
    agent: &mut Settings,
    engine: Option<&str>,
) -> Result<crate::tools::Changed, Report> {
    let engine = engine.unwrap_or(settings::DEFAULT_MEMORY).to_string();
    let before = agent.memory.replace(engine.clone());
    match define(out, agent) {
        Ok(tool) => tool.within("no memory")?,
        Err(err) => {
            agent.memory = before;
            return Err(err);
        }
    };
    agent.save()?;
    let mut done = Done::success(sentence!(
        "Your agent's memory is on: {}, over {}.",
        Product::new(about(&engine)),
        Where::new(agent.files())
    ));
    // Where what it remembers is, to read or change.
    if let Some(notes) = notes_of(agent) {
        done = done.changed("The notes it keeps", Where::new(notes));
    }
    reach_note(out, agent);
    offer_meaning(out, agent)?;
    Ok((before.as_deref() != Some(engine.as_str()), done))
}

/// Memory's embedding model (its spec's `[tool.memory] model`), when it
/// isn't here: asked for once, in a terminal, and installed (`lnk model
/// install`) on a yes; without a terminal, or refused, it says memory
/// searches by words only, and what adds meaning.
fn offer_meaning(out: &mut Out, agent: &mut Settings) -> Result<(), Report> {
    let model = agent
        .spec()?
        .tool("memory")
        .model
        .clone()
        .unwrap_or_else(|| "qwen3-embedding".into());
    if model == "none" || agent.meaning_offered {
        return Ok(());
    }
    let here = link_core::find_module("model").is_some()
        && crate::model::local_runtimes()
            .ok()
            .and_then(|runtimes| crate::model::find_local(&runtimes, &model))
            .is_some();
    if here {
        return Ok(());
    }
    let shown = Name::new("model", &model);
    let words = Note(sentence!(
        "Your agent's memory searches by words only, and {} adds search by meaning.",
        cmd!("lnk model install {}", &shown)
    ));
    if !crate::tools::may_ask() {
        out.block(words);
        return Ok(());
    }
    let install = out.ask(
        Question::confirm(sentence!("Install {} for memory?", &shown), true).why(prose!(
            "Memory searches by meaning with it, a model on this computer: a download, with the \
             models module."
        )),
    )?;
    agent.meaning_offered = true;
    agent.save()?;
    if !install {
        out.block(words);
        return Ok(());
    }
    link_core::ensure("model", "Searching memory by meaning")?;
    out.hand_over();
    let installed = link_core::lnk()
        .args(["model", "install", &model, "--yes"])
        .status()
        .within("running lnk model install")?;
    if !installed.success() {
        out.block(
            Report::warning(sentence!("{} didn't install.", &shown))
                .why(prose!("Memory searches by words until it is."))
                .next(cmd!("lnk model install {}", &shown), "try again"),
        );
    }
    Ok(())
}

/// `lnk agent memory stop`. Whether it had one, and how it ended.
pub fn off(out: &mut Out, agent: &mut Settings) -> Result<crate::tools::Changed, Report> {
    let Some(engine) = agent.memory.take() else {
        out.block(Note(sentence!("Your agent has no memory on.")));
        return Ok((
            false,
            Done::shown().next(cmd!("lnk agent memory start"), "set one up"),
        ));
    };
    agent.save()?;
    let tool = settings::memory_tool_of(&agent.key());
    let _ = std::fs::remove_file(settings::config_of(&agent.name).join("memory.json"));
    if let Ok(owner) = agent.owner() {
        let _ = std::fs::remove_file(settings::memory_record_of(&owner, &agent.key()));
    }
    let _ = link_core::lnk()
        .args(["sandbox", "tool", "remove", &tool])
        .output();
    crate::confine::forget_defined_tools();
    Ok((
        true,
        Done::success(sentence!(
            "Your agent's memory is off, and its files stay as they are."
        ))
        .changed(
            "What it kept is in",
            Where::new(agent.memory_state(&engine)),
        )
        .next(
            cmd!("lnk agent memory start {}", Name::new("engine", &engine)),
            "use it again",
        ),
    ))
}

/// `lnk agent memory show`.
pub fn status(out: &mut Out, agent: &Settings, harness: &str) -> Result<Done, Report> {
    let Some(engine) = &agent.memory else {
        out.block(Note(sentence!("There is no memory.")));
        return Ok(Done::shown().next(
            cmd!("lnk agent memory start"),
            "give your agent Link Memory over its files",
        ));
    };
    let tool = settings::memory_tool_of(&agent.key());
    let reach = match agent.proxy_of(harness) {
        Some(_) => format!(
            "at ${} in {harness}'s environment (MCP over HTTP, tool {tool})",
            link_memory_tool_contract::MEMORY_ENV
        ),
        None => format!(
            "not from {harness}: it runs without its proxy (on the network as it is, or no \
             internet)"
        ),
    };
    out.block(
        Fields::new()
            .row("Engine", Product::new(about(engine)))
            .row("Over", Where::new(agent.files()))
            .row("Kept in", Where::new(agent.memory_state(engine)))
            .row("Reached", reach),
    );
    Ok(Done::shown())
}

/// Where the harness reaches its memory, or why it can't.
fn reach_note(out: &mut Out, agent: &Settings) {
    let harness = agent.harness();
    // Reached as it should be, through its proxy: nothing to say
    // (`lnk agent memory show` says how).
    if agent.proxy_of(harness).is_none() {
        out.block(
            Report::warning(sentence!(
                "{} runs without its proxy, so it can't reach its memory.",
                Name::new("harness", harness)
            ))
            .why(prose!(
                "It runs on the network as it is, or with no internet."
            ))
            .next(
                cmd!(
                    "lnk agent harness-permission list --harness {}",
                    Name::new("harness", harness)
                ),
                "see what it may do",
            ),
        );
    }
}

/// Engine `name` as people read it.
fn about(name: &str) -> String {
    settings::MEMORIES
        .iter()
        .find(|(k, _, _)| *k == name)
        .map(|(_, _, about)| about.split(':').next().unwrap_or(name).to_string())
        .unwrap_or_else(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A team memory that isn't served names what helps, never a start
    /// again, which meets the same engine.
    #[test]
    fn a_team_memory_warning_names_what_helps() {
        let err = Report::error(sentence!("Couldn't install the memory-tool module."));
        let warned = team_warning("research", &err);
        let next = serde_json::to_string(&warned.next).unwrap();
        assert!(
            next.contains("team-memory") && next.contains("research"),
            "{next}"
        );
        assert!(
            next.contains("--module") && next.contains("memory-tool"),
            "{next}"
        );
        assert!(!next.contains("try again"), "{next}");
    }
}
