//! `lnk agent show`'s whole readout: the agent's state and whether it
//! answers, then who it is, what it was told to prefer, what it costs,
//! its skills and memory, and where each of its things is kept.

use link_agent::settings::{self, Settings, ids};
use link_format::{
    Count, Done, Fields, Foreign, Money, Name, Out, Path, Phrase, Report, Section, Size, Value,
    phrase,
};

use crate::status;

/// `lnk agent show`, read by a person: its name and state, then one
/// section for each part of it. `reported`: an agent that doesn't answer
/// is reported once shown; a diagnosis (an agent named) shows it alone.
pub(crate) fn show(
    out: &mut Out,
    agent: &Settings,
    on: Option<&str>,
    reported: bool,
) -> Result<Done, Report> {
    let seen = status::seen(agent)?;
    out.block(status::machine_row(
        status::state_rows(Fields::new(), agent, &seen),
        on,
    ));
    let model = status::model_rows(out, Fields::new(), agent)?;
    out.block(Section::new("Model").with(opinions(model, agent)));
    let harness = Fields::new().row("Harness", Name::new("harness", agent.harness()));
    let harness = status::tools_row(harness, agent).row("Skills", skills(agent));
    out.block(Section::new("Harness").with(harness));
    out.block(Section::new("Connections").with(status::phone_rows(Fields::new(), agent)));
    out.block(Section::new("Memory").with(memory(agent)));
    let sandbox = status::sandbox_rows(Fields::new(), agent, |fields| grants(fields, agent));
    out.block(Section::new("Sandbox").with(sandbox));
    out.block(Section::new("Who").with(who(agent)));
    if let Some(usage) = usage(agent) {
        out.block(Section::new("Today").with(usage));
    }
    out.block(Section::new("Where").with(places(agent)));
    // An agent that doesn't answer is shown all the same, then reported.
    match status::said(out, agent, seen.state) {
        (_, Some(report)) if reported => Err(report),
        (done, _) => Ok(done),
    }
}

/// Its saved grants, a row for each kind: what it writes, reads, reaches
/// and runs, then each team's folders it reaches.
fn grants(mut fields: Fields, agent: &Settings) -> Fields {
    let sandbox = link_agent::sandbox_file::of(&agent.name);
    let mut kinds: Vec<(Phrase, Vec<Value>)> = Vec::new();
    for item in sandbox
        .spec
        .on(link_sandbox_contract::spec::Os::here())
        .unwrap_or_default()
        .items()
    {
        let shown = link_sandbox_client::flags::shown(&item);
        let (flag, value) = shown.split_once(' ').unwrap_or((&shown, ""));
        let label = match flag {
            "--files.write" => Phrase::from("Writes"),
            "--files.read" => Phrase::from("Reads"),
            "--internet.hosts" => Phrase::from("Internet"),
            "--commands.allow" => Phrase::from("Commands"),
            other => phrase!("{}", Foreign::new(other)),
        };
        let value = value.trim_matches('\'');
        let value = Value::from(Foreign::new(if value.is_empty() { "yes" } else { value }));
        match kinds.iter_mut().find(|(kind, _)| *kind == label) {
            Some((_, values)) => values.push(value),
            None => kinds.push((label, vec![value])),
        }
    }
    if kinds.is_empty() && sandbox.spec.teams.is_empty() {
        return fields.row("Grants", "nothing beyond its harness's home");
    }
    for (kind, values) in kinds {
        fields = fields.row(kind, Value::List(values));
    }
    for (team, folders) in &sandbox.spec.teams {
        let names = |names: &[String]| {
            names
                .iter()
                .map(|n| match n.as_str() {
                    link_harness_contract::harness::WHOLE => "all".to_string(),
                    n => format!("{n}/"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut reach = Vec::new();
        if !folders.write.is_empty() {
            reach.push(format!("writes {}", names(&folders.write)));
        }
        if !folders.read.is_empty() {
            reach.push(format!("reads {}", names(&folders.read)));
        }
        if reach.is_empty() {
            reach.push("reaches none".into());
        }
        fields = fields.row(
            phrase!("Team {}", Name::new("team", team)),
            Foreign::new(&reach.join(", ")),
        );
    }
    fields
}

/// Its IDs, owner and teams.
fn who(agent: &Settings) -> Fields {
    let or_none = |value: Option<String>| match value {
        Some(value) => Value::from(Foreign::new(&value)),
        None => Value::from("none yet"),
    };
    let teams: Vec<Value> = agent
        .agent_id()
        .and_then(|id| settings::teams_of(id).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|team| Value::from(Name::new("team", team.name)))
        .collect();
    let mut fields = Fields::new()
        .row("ID", or_none(agent.id.as_ref().map(ToString::to_string)))
        .row(
            "Runs as",
            or_none(agent.service.as_deref().map(|s| ids::SERVICES.written(s))),
        )
        .row("Owner", or_none(agent.owner.clone()))
        .row(
            "Teams",
            match teams.is_empty() {
                true => Value::from("none"),
                false => Value::List(teams),
            },
        );
    if let Some(port) = agent.port {
        fields = fields.row("Port", Value::from(port.to_string()));
    }
    fields
}

/// `fields`, then what its spec prefers of its model, when it says.
fn opinions(mut fields: Fields, agent: &Settings) -> Fields {
    let Ok(shown) = crate::context_mcp::shown(agent, false) else {
        return fields;
    };
    if let Some(window) = shown.window {
        fields = fields.row(
            "Context window",
            Count::new(u64::from(window), "token", "tokens"),
        );
    }
    if let Some(answer) = shown.answer {
        fields = fields.row(
            "Longest answer",
            Count::new(u64::from(answer), "token", "tokens"),
        );
    }
    fields
}

/// Its memory and spend from `lnk measure`, when it measured them.
fn usage(agent: &Settings) -> Option<Fields> {
    let id = agent.id.as_ref()?.to_string();
    let (memory, spend) = crate::services::measured().get(&id).copied()?;
    let mut fields = Fields::new();
    if let Some(bytes) = memory {
        fields = fields.row("Memory", Size(bytes as u64));
    }
    if let Some(micros) = spend {
        fields = fields.row(
            "Spend",
            Money {
                cents: (micros / 1e4).round() as u64,
                per: None,
            },
        );
    }
    (memory.is_some() || spend.is_some()).then_some(fields)
}

/// The skills in its own folder, by name.
fn skills(agent: &Settings) -> Value {
    let names: Vec<Value> = link_agent::skills::list(&agent.folder().agent)
        .unwrap_or_default()
        .into_iter()
        .map(|skill| Value::from(Foreign::new(&skill.name)))
        .collect();
    match names.is_empty() {
        true => Value::from("none yet"),
        false => Value::List(names),
    }
}

/// Its memory's engine and how many notes it keeps.
fn memory(agent: &Settings) -> Fields {
    let notes = link_agent::identity::remembered(&agent.folder().agent)
        .map(|notes| notes.into_iter().filter(|note| !note.deleted).count())
        .unwrap_or_default();
    Fields::new()
        .row(
            "Engine",
            match &agent.memory {
                Some(engine) => Value::from(Foreign::new(engine)),
                None => Value::from("off"),
            },
        )
        .row("Notes", Value::from(notes))
}

/// Where each of its things is kept on this machine.
fn places(agent: &Settings) -> Fields {
    let harness = agent.harness();
    let folder = agent.folder();
    Fields::new()
        .row("Its folder", Path::new(agent.dir()))
        .row("Its own", Path::new(folder.agent))
        .row("Files", Path::new(agent.files()))
        .row("Settings and spec", Path::new(agent.path()))
        .row(
            "Sandbox file",
            Path::new(link_agent::sandbox_file::path(&agent.name)),
        )
        .row("Harness home", Path::new(agent.box_of(harness)))
        .row("Log", Path::new(agent.log_path(harness)))
}
