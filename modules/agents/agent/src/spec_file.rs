//! `lnk agent spec show`: an agent's spec as it runs, each key's owner
//! in a comment, or the effective spec of a file or field flags alone,
//! printed before any agent is started.

use link_agent::cli::Command;
use link_agent::settings::Settings;
use link_format::{Done, Exit, Out, Report, Verbatim, cmd, sentence};
use link_spec::{FieldFlags, Show};

/// A spec printed before any agent is read: done, when it's printed.
pub(crate) fn route(
    out: &mut Out,
    named: Option<&str>,
    command: &Command,
    fields: &FieldFlags,
) -> Result<Option<Done>, Report> {
    match command {
        Command::SpecShow(given) => show(out, named, given, fields).map(Some),
        Command::TeamSpecShow(given) => {
            let shown = link_spec::show::<link_agent_spec::TeamFile>(given, fields, Some(&team))?;
            printed(out, &shown, given.output.json).map(Some)
        }
        _ => Ok(None),
    }
}

/// Team `name`'s file, as the team's spec: what it was made from.
fn team(name: &str) -> anyhow::Result<link_agent_spec::TeamFile> {
    link_machine::machine::check_name(name)?;
    let path = link_agent::settings::team_path(name);
    anyhow::ensure!(
        path.exists(),
        "there's no team {name}: lnk agent team create {name} makes it"
    );
    let text = std::fs::read_to_string(&path)?;
    link_spec::parse_spec::<link_agent_spec::TeamFile>(&text)
}

/// A spec `link_spec::show` printed: its text, or its JSON as the
/// command's answer.
fn printed(out: &mut Out, shown: &str, json: bool) -> Result<Done, Report> {
    match json {
        true => out
            .reply(&serde_json::from_str::<serde_json::Value>(shown).map_err(anyhow::Error::from)?),
        false => out.data(shown.as_bytes()),
    }
    Ok(Done::shown())
}

pub(crate) fn show(
    out: &mut Out,
    named: Option<&str>,
    show: &Show,
    fields: &FieldFlags,
) -> Result<Done, Report> {
    if named.is_some() && show.thing.is_some() {
        return Err(Report::error(sentence!(
            "Name the agent once: as the argument, or with {}, not both.",
            cmd!("--agent <name>")
        ))
        .exit(Exit::Usage));
    }
    let thing = show.thing.as_deref().or(named);
    if thing.is_some() && show.spec.is_some() {
        return Err(Report::error(sentence!(
            "Name an agent or give {}, not both: an agent's spec is the one it runs with.",
            cmd!("--spec FILE")
        ))
        .exit(Exit::Usage));
    }
    let alone = thing.is_none() && show.spec.is_none() && fields.is_empty();
    // A file or flags: the spec they make, as any kind's.
    if thing.is_none() && !alone {
        let shown = link_spec::show::<link_agent_spec::Spec>(show, fields, None)?;
        return printed(out, &shown, show.output.json);
    }
    let agent = Settings::resolve_without_keys(thing)?;
    if !agent.path().exists() {
        let said = match thing {
            Some(_) => sentence!("There is no agent here by that name."),
            None => sentence!("There is no agent yet."),
        };
        return Err(Report::error(said)
            .exit(Exit::Usage)
            .next(cmd!("lnk agent list"), "see your agents"));
    }
    let resolved = agent.spec()?;
    // As it runs here: each collection where this machine keeps it, what
    // the machine overrode marked.
    let places = link_agent::machine_settings::storage()?
        .resolve(&resolved.spec.storage.clone().unwrap_or_default());
    let resolved = resolved.on_machine(&places);
    // Field flags over it: the spec it would run with them, every key
    // the union's, so none is anyone's alone.
    if !fields.is_empty() {
        let spec = resolved.spec.clone();
        let args = Show {
            thing: Some(agent.name.clone()),
            ..show.clone()
        };
        let shown =
            link_spec::show::<link_agent_spec::Spec>(&args, fields, Some(&|_| Ok(spec.clone())))?;
        return printed(out, &shown, show.output.json);
    }
    // JSON is the spec itself, as a file or flags print it; whose each key
    // is shows in the text alone.
    match show.output.json {
        true => {
            use link_spec::Specification;
            let json = link_agent_spec::Spec::format().print_json(&resolved.spec)?;
            return printed(out, &String::from_utf8_lossy(&json), true);
        }
        false => {
            let mut lines = Verbatim::new();
            for (line, whose) in resolved.lines()? {
                lines = match whose {
                    Some(whose) => lines.commented(line, whose),
                    None => lines.line(line),
                };
            }
            out.block(lines);
        }
    }
    Ok(Done::shown())
}
