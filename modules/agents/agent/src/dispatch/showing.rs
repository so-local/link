//! Selecting what `lnk agent show` displays, with incompatible flags refused.

use link_agent::{cli, service, settings::Settings};
use link_format::{Done, Exit, Out, Report, cmd, sentence};

use crate::conversations::instructions;
use crate::listing::agent_json;
use crate::permissions::permissions;

/// A name given to `show` is the whole question: no flag beside it.
fn ensure_alone(flagged: bool) -> Result<(), Report> {
    match flagged {
        true => Err(Report::error(sentence!(
            "A name to {} goes without {}, {} or {}.",
            cmd!("lnk agent show"),
            cmd!("--instructions"),
            cmd!("--permissions"),
            cmd!("--harness")
        ))
        .exit(Exit::Usage)),
        false => Ok(()),
    }
}

/// `lnk agent show`: what the agent runs and whether it answers, or with a flag its prompt
/// (`--instructions`) or what it may do
/// (`--permissions`, of `--harness` or the agent's own).
pub(super) fn show_agent(
    out: &mut Out,
    agent: &Settings,
    show: cli::AgentShowArgs,
) -> Result<Done, Report> {
    let cli::AgentShowArgs {
        name,
        json,
        instructions: prompt,
        permissions: may,
        harness,
        on,
        ..
    } = show;
    if prompt && may {
        return Err(Report::error(sentence!(
            "Give one of {} or {}.",
            cmd!("--instructions"),
            cmd!("--permissions")
        ))
        .exit(Exit::Usage));
    }
    if harness.is_some() && !may {
        return Err(Report::error(sentence!(
            "{} goes with {}.",
            cmd!("--harness"),
            cmd!("--permissions")
        ))
        .exit(Exit::Usage));
    }
    // An agent named: its typed blocks, the diagnosis `lnk issue create`
    // reads, which shows a stopped agent rather than fails on it.
    if name.is_some() {
        ensure_alone(prompt || may || harness.is_some())?;
        return crate::readout::show(out, agent, on.as_deref(), false);
    }
    if prompt {
        return instructions(out, agent, json);
    }
    if may {
        let name = harness.as_deref().unwrap_or(agent.harness()).to_string();
        return permissions(out, agent, &name);
    }
    if !json {
        return crate::readout::show(out, agent, on.as_deref(), true);
    }
    // Its entry as the list gives it, its wire, and its window and budget.
    let running = service::running_anywhere(&agent.name).then(|| service::processes(&agent.name));
    let mut json = agent_json(
        agent,
        link_machine::installation::installation().ok(),
        running,
    );
    json["wire"] = serde_json::json!(
        link_sandbox_client::wires()
            .filter(|_| agent.proxy_of(agent.harness()).is_some())
            .and_then(|w| w.closed(&agent.profile(agent.harness())))
    );
    let shown = crate::context_mcp::shown(agent, false)?;
    for (key, value) in [("window", shown.window), ("answer", shown.answer)] {
        if let Some(value) = value {
            json[key] = serde_json::json!(value);
        }
    }
    link_actions::answer::reply::<link_agent_spec::declared::LifeAgentEntry>(out, &json)?;
    Ok(Done::shown())
}
