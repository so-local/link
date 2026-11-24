//! `lnk sandbox stop`, `start` and `list`: the kill switch, one
//! sandbox's way out, and the sandboxes they reach.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Report, Table, Text, Value, WithinExt, cmd, phrase,
    prose, sentence,
};
use link_sandbox::cli::{self};
use link_sandbox_proxy::wire;

/// `lnk sandbox stop <name>`: one sandbox's wire closed, its program
/// left running.
fn pause(name: String) -> Result<Done, Report> {
    let dir = wire::dir()?;
    // A sandbox running now, or one that has run: never a name no
    // sandbox has, which would be paused for nothing.
    let known = wire::traffic(&dir).iter().any(|t| t.sandbox == name)
        || wire::read(&dir).iter().any(|e| e.sandbox == name);
    if !known {
        return Err(Report::error(sentence!(
            "No sandbox named {} runs or has run here.",
            Name::typed("sandbox", &name)
        ))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk sandbox list"),
            "see the sandboxes that run or have run",
        ));
    }
    // Kept as written: `lnk agent show` and `lnk sandbox list --json` show it, backticks and all.
    wire::pause(&dir, &name, "by \u{60}lnk sandbox stop\u{60}")?;
    Ok(Done::success(sentence!(
        "{}'s wire refuses everything now.",
        Name::new("sandbox", &name)
    ))
    .next(
        cmd!("lnk sandbox start {}", Name::new("sandbox", &name)),
        "open it again",
    ))
}

/// `lnk sandbox list`: every sandbox this machine knows, its proxy
/// running or its calls only in the log, and why each paused one is;
/// with `paused`, only those paused.
pub(crate) fn list(
    out: &mut Out,
    cli::SandboxListArgs { json, paused }: cli::SandboxListArgs,
) -> Result<Done, Report> {
    let dir = wire::dir()?;
    let state = link_sandbox_client::Wires::read(&dir).within("reading the wires")?;
    let running: BTreeSet<String> = wire::traffic(&dir).into_iter().map(|t| t.sandbox).collect();
    let why: BTreeMap<&str, &str> = state
        .paused
        .iter()
        .map(|p| (p.sandbox.as_str(), p.why.as_str()))
        .collect();
    let mut names: BTreeSet<String> = running.clone();
    names.extend(wire::read(&dir).into_iter().map(|e| e.sandbox));
    names.extend(state.paused.iter().map(|p| p.sandbox.clone()));
    let known: Vec<(String, &str, Option<&str>)> = names
        .into_iter()
        .map(|name| {
            let kind = match running.contains(&name) {
                true => "running",
                false => "ended",
            };
            let reason = why.get(name.as_str()).copied();
            (name, kind, reason)
        })
        .filter(|(_, _, reason)| !paused || reason.is_some())
        .collect();
    if json {
        let sandboxes: Vec<serde_json::Value> = known
            .iter()
            .map(|(name, kind, reason)| {
                let mut row = serde_json::json!({"sandbox": name, "state": kind});
                if let Some(reason) = reason {
                    row["paused"] = serde_json::Value::from(*reason);
                }
                row
            })
            .collect();
        link_actions::answer::reply::<link_sandbox_contract::spec::Sandboxes>(
            out,
            &serde_json::json!({"stopped": state.stopped, "sandboxes": sandboxes}),
        )?;
        return Ok(Done::shown());
    }
    let mut done = Done::shown();
    if state.stopped {
        out.block(Text(prose!("Every wire is stopped.")));
        done = done.next(cmd!("lnk sandbox start"), "open them again");
    }
    let none = match paused {
        true => sentence!("Every sandbox's way out is open."),
        false => sentence!("No sandbox runs or has run here."),
    };
    let mut table = Table::new(["SANDBOX", "STATE", "PAUSED"], none);
    for (name, kind, reason) in &known {
        table.row([
            Value::from(Name::new("sandbox", name)),
            Value::from(*kind),
            Value::from(Foreign::new(reason.unwrap_or("-"))),
        ]);
        if reason.is_some() {
            done = done.next(
                cmd!("lnk sandbox start {}", Name::new("sandbox", name)),
                phrase!("open {}'s wire again", Name::new("sandbox", name)),
            );
        }
    }
    out.block(table);
    Ok(done)
}

/// `lnk sandbox start <name>`: one sandbox's wire opened again.
fn resume(out: &mut Out, name: String) -> Result<Done, Report> {
    match wire::resume(&wire::dir()?, &name)? {
        true => Ok(Done::success(sentence!(
            "{}'s wire is open again.",
            Name::new("sandbox", &name)
        ))),
        false => {
            out.block(Note(sentence!(
                "{}'s wire isn't paused.",
                Name::new("sandbox", &name)
            )));
            Ok(Done::shown())
        }
    }
}

/// `lnk sandbox start`: the wires stopped opened again, or with `name`,
/// one sandbox's.
pub(crate) fn start(out: &mut Out, name: Option<String>) -> Result<Done, Report> {
    if let Some(name) = name {
        return resume(out, name);
    }
    wire::stop(&wire::dir()?, false)?;
    Ok(Done::success(sentence!("The wires carry calls again."))
        .next(cmd!("lnk agent start"), "start what stopped"))
}

/// `lnk sandbox stop`: every wire stopped, then every agent running
/// here; or with `name`, one sandbox's wire. The flag comes first and
/// needs nothing from any sandbox, so a misbehaving agent can't hold it
/// up.
pub(crate) fn stop(out: &mut Out, name: Option<String>) -> Result<Done, Report> {
    if let Some(name) = name {
        return pause(name);
    }
    let dir = wire::dir()?;
    wire::stop(&dir, true)?;
    out.block(Note(sentence!(
        "Every sandbox's wire refuses everything now; calls in progress end within a second."
    )));
    run_on_stop(out)?;
    Ok(Done::success(sentence!("Every wire is stopped."))
        .next(cmd!("lnk sandbox log show"), "see the log, which stays")
        .next(cmd!("lnk sandbox start"), "open the wires again"))
}

/// Where what runs on `stop` is kept, one file per name: in Link's
/// settings, which no sandbox reaches.
/// What runs on `stop` for one name: `lnk <args>`.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OnStop {
    args: Vec<String>,
}

link_files::specification!(OnStop, include_str!("stop/on_stop.schema.toml"));

pub(crate) fn on_stop_dir() -> anyhow::Result<PathBuf> {
    let dir = wire::dir()?.join("on-stop");
    link_files::create_private_dir(&dir)?;
    Ok(dir)
}

/// `lnk sandbox on-stop <name> -- <args>`: `lnk <args>` runs on `stop`
/// for `name`; no args, nothing does anymore.
pub(crate) fn on_stop(name: &str, args: &[String]) -> Result<Done, Report> {
    let plain = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !plain {
        return Err(
            Report::error(sentence!("{} isn't a name.", Name::typed("on-stop", name)))
                .why(prose!("A name is letters, digits and dashes."))
                .exit(Exit::Usage),
        );
    }
    let file = on_stop_dir()?.join(format!("{name}.json"));
    if args.is_empty() {
        let _ = std::fs::remove_file(&file);
        return Ok(Done::success(sentence!(
            "Nothing runs on stop for {} anymore.",
            Name::new("on-stop", name)
        )));
    }
    // Never the kill switch itself, run again.
    if args[0] == "sandbox" {
        return Err(
            Report::error(sentence!("Sandbox doesn't run itself on stop."))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk sandbox on-stop <name> -- <command>"),
                    "give another command",
                ),
        );
    }
    link_files::write_private(
        &file,
        &link_files::saved::print_json(&OnStop {
            args: args.to_vec(),
        })
        .within("writing it")?,
    )
    .within("keeping it")?;
    Ok(Done::success(sentence!(
        "{} runs on stop.",
        Name::new("on-stop", name)
    )))
}

/// Runs what asked to stop with the wires (`on-stop`), each `lnk <args>`,
/// saying which didn't.
pub(crate) fn run_on_stop(out: &mut Out) -> anyhow::Result<()> {
    let dir = on_stop_dir()?;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    entries.sort();
    let (mut stopped, mut failed) = (Vec::new(), Vec::new());
    for file in entries {
        let name = file
            .file_stem()
            .map(|s| link_format::clean(&s.to_string_lossy()))
            .unwrap_or_default();
        let args: Vec<String> = match std::fs::read(&file)
            .ok()
            .and_then(|b| link_files::saved::parse_json::<OnStop>(&b).ok())
        {
            Some(on_stop) => on_stop.args,
            None => {
                failed.push(name);
                continue;
            }
        };
        if args.first().is_none_or(|a| a == "sandbox") {
            failed.push(name);
            continue;
        }
        let ran = link_core::lnk()
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        match ran {
            true => stopped.push(name),
            false => failed.push(name),
        }
    }
    if !stopped.is_empty() {
        out.block(Note(sentence!(
            "Stopped {}.",
            Value::List(
                stopped
                    .iter()
                    .map(|n| Name::new("on-stop", n).into())
                    .collect()
            )
        )));
    }
    if !failed.is_empty() {
        out.block(
            Report::warning(sentence!(
                "Couldn't stop {}.",
                Value::List(
                    failed
                        .iter()
                        .map(|n| Name::new("on-stop", n).into())
                        .collect()
                )
            ))
            .why(prose!("Their wires refuse everything; stop each yourself.")),
        );
    }
    Ok(())
}
