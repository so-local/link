//! `lnk sandbox log show` and `show`: what sandboxes reached; and the
//! question of `lnk measure` this module answers, what each sandbox's
//! proxy carried.

use link_format::{
    Count, Done, Foreign, Name, Out, Report, Size, Table, Text, Value, cmd, phrase, prose, sentence,
};
use link_sandbox::bridges::asks;
use link_sandbox::cli::{self};
use link_sandbox_proxy::wire;

pub(crate) fn log_command(
    out: &mut Out,
    cli::LogShowArgs {
        lines,
        sandbox,
        refused,
        json,
    }: cli::LogShowArgs,
) -> Result<Done, Report> {
    let lines = match lines {
        // Past what this machine can count is every line.
        Some(lines) => usize::try_from(lines).unwrap_or(usize::MAX),
        None => link_sandbox::constants::LOG_LINES,
    };
    log(out, lines, sandbox.as_deref(), refused, json)
}

/// `lnk sandbox show`: each running sandbox's proxy and what it carried,
/// of sandbox `name` alone if given.
pub(crate) fn show(
    out: &mut Out,
    cli::SandboxShowArgs { json, name }: cli::SandboxShowArgs,
) -> Result<Done, Report> {
    let mut carried = wire::traffic(&wire::dir()?);
    if let Some(name) = &name {
        carried.retain(|t| &t.sandbox == name);
    }
    if json {
        link_actions::answer::reply::<link_sandbox_contract::spec::Carried>(out, &carried)?;
        return Ok(Done::shown());
    }
    // What a model's answers counted is the measure module's to show
    // (`lnk measure`), as the agent's.
    let none = match &name {
        Some(name) => sentence!(
            "No sandbox named {} runs here.",
            Name::typed("sandbox", name)
        ),
        None => sentence!("No sandbox's proxy runs here."),
    };
    let mut table = Table::new(["SANDBOX", "PROXY", "UP", "DOWN"], none);
    for t in &carried {
        table.row([
            Value::from(Name::new("sandbox", &t.sandbox)),
            Value::from(t.pid.to_string()),
            Value::from(Size(t.up)),
            Value::from(Size(t.down)),
        ]);
    }
    out.block(table);
    Ok(Done::shown().next(cmd!("lnk sandbox log show"), "see the calls they made"))
}

/// `lnk-sandbox --link-answer <question>`: [`TRAFFIC`], what each running
/// sandbox's proxy has carried, as `lnk sandbox show --json` says it,
/// the one question this module answers.
///
/// [`TRAFFIC`]: link_measure_contract::client::asked::TRAFFIC
pub(crate) fn answer(question: &str) -> anyhow::Result<()> {
    use link_measure_contract::client::asked::TRAFFIC;
    anyhow::ensure!(
        question == TRAFFIC.name,
        "{:?} is no question this module answers",
        link_format::clean(question)
    );
    TRAFFIC.input()?;
    TRAFFIC.answer(&wire::traffic(&wire::dir()?))
}

/// `lnk sandbox log show`: the latest `lines` calls on the wires, of
/// `sandbox` only if given, refused ones only if `refused`.
pub(crate) fn log(
    out: &mut Out,
    lines: usize,
    sandbox: Option<&str>,
    refused: bool,
    json: bool,
) -> Result<Done, Report> {
    let dir = wire::dir()?;
    let entries: Vec<_> = wire::read(&dir)
        .into_iter()
        .filter(|e| sandbox.is_none_or(|name| e.sandbox == name))
        .filter(|e| !refused || e.outcome == wire::Outcome::Refused)
        .collect();
    let latest = &entries[entries.len().saturating_sub(lines)..];
    if json {
        // A line each, as its readers parse it.
        for entry in latest {
            link_actions::answer::line::<link_sandbox_contract::spec::Entry>(out, entry)?;
        }
        return Ok(Done::shown());
    }
    let mut done = Done::shown();
    if wire::is_stopped(&dir) {
        out.block(Text(prose!("Every wire is stopped.")));
        done = done.next(cmd!("lnk sandbox start"), "open them again");
    }
    let waiting = asks::waiting(&dir)?;
    for (name, why) in &wire::paused(&dir) {
        out.block(Text(prose!(
            "Paused: {}, {}.",
            Name::new("sandbox", name),
            Foreign::new(why)
        )));
        done = done.next(
            cmd!("lnk sandbox start {}", Name::new("sandbox", name)),
            phrase!("open {}'s wire again", Name::new("sandbox", name)),
        );
    }
    let mut table = Table::new(
        wire::Entry::HEADER,
        sentence!("No calls are on the wire yet."),
    );
    for entry in latest {
        table.row(entry.cells());
    }
    out.block(table);
    if !waiting.is_empty() {
        done = done.next(
            cmd!("lnk sandbox question list"),
            phrase!(
                "{} waiting on your answer",
                Count::new(waiting.len() as u64, "question", "questions")
            ),
        );
    }
    Ok(done)
}
