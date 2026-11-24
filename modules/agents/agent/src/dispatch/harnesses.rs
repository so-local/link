//! Harness management commands.
use super::*;

/// Whether `command` is one of a harness's own commands: the harness's,
/// not one agent's. Setting the harness changes one agent, as setting its
/// model does, so it isn't one.
pub(crate) fn harness_own(command: &Command) -> bool {
    matches!(
        command,
        Command::HarnessList(_)
            | Command::HarnessPermissionList(_)
            | Command::HarnessPermissionAdd(_)
            | Command::HarnessPermissionRemove(_)
            | Command::HarnessReset(_)
            | Command::HarnessCheck(_)
            | Command::HarnessRun(_)
            | Command::HarnessShow(_)
            | Command::HarnessDelete(_)
    )
}

/// `lnk agent harness ...`: what a harness may do here, and its home.
/// The harness is `--harness`, else the agent's (`delete`: its object);
/// `named`, the agent `--agent` names.
pub(crate) async fn harness_command(
    out: &mut Out,
    named: Option<&str>,
    command: Command,
    fields: &link_spec::FieldFlags,
) -> Result<Done, Report> {
    let harness = match &command {
        Command::HarnessList(a) => {
            let _ = &a.harness;
            return list(out, &Settings::resolve_without_keys(named)?);
        }
        Command::HarnessPermissionList(a) => a.harness.clone(),
        Command::HarnessPermissionAdd(a) => a.harness.clone(),
        Command::HarnessPermissionRemove(a) => a.harness.clone(),
        Command::HarnessReset(a) => a.harness.clone(),
        Command::HarnessCheck(a) => a.harness.clone(),
        Command::HarnessRun(a) => a.harness.clone(),
        Command::HarnessShow(a) => a.harness.clone(),
        Command::HarnessDelete(a) => a.harness.clone(),
        _ => unreachable!("only a harness's own commands"),
    };
    let name = match (&command, harness) {
        (Command::HarnessDelete(_), None) => {
            return Err(Report::error(sentence!(
                "{} names the harness it removes.",
                cmd!("lnk agent harness delete --harness <harness>")
            ))
            .exit(Exit::Usage));
        }
        (_, Some(harness)) => harness,
        (_, None) => Settings::resolve_without_keys(named)?.harness().to_string(),
    };
    let name = name.as_str();
    // The agent whose home of it a command acts on.
    let agent = |keys: bool| -> Result<Settings, Report> {
        let agent = match keys {
            true => Settings::resolve(named)?,
            false => Settings::resolve_without_keys(named)?,
        };
        known_harness(name, Some(&agent))?;
        Ok(agent)
    };
    match command {
        Command::HarnessPermissionList(_) => {
            known_harness(name, None)?;
            harness_permissions(out, name)
        }
        Command::HarnessPermissionAdd(a) => {
            // A command bound to a spec checks the spec on its reads, so
            // its own arguments are checked here.
            link_agent::checks::harness_name(&a)
                .and_then(|()| link_agent::checks::risk_name(&a))
                .map_err(anyhow::Error::msg)?;
            known_harness(name, None)?;
            allow_need(out, name, Given::of(a.risk, fields.clone(), "add")?).await
        }
        Command::HarnessPermissionRemove(a) => {
            // A command bound to a spec checks the spec on its reads, so
            // its own arguments are checked here.
            link_agent::checks::harness_name(&a)
                .and_then(|()| link_agent::checks::risk_name(&a))
                .map_err(anyhow::Error::msg)?;
            known_harness(name, None)?;
            deny_need(out, name, Given::of(a.risk, fields.clone(), "remove")?).await
        }
        Command::HarnessReset(_) => {
            known_harness(name, None)?;
            reset(name)
        }
        Command::HarnessCheck(_) => check_sandbox(out, &agent(true)?, name),
        Command::HarnessRun(a) => {
            out.hand_over();
            Ok(Done::passed(run_inside(&agent(true)?, name, &a.command)?))
        }
        Command::HarnessShow(_) => show(out, &agent(false)?, name),
        Command::HarnessDelete(a) => remove(out, &agent(true)?, name, a.yes),
        _ => unreachable!("only a harness's own commands"),
    }
}

/// An error unless `name` is a harness here: Link's, one whose adapter is
/// on the PATH, one with grants here, or `agent`'s, or one it has a home
/// of.
pub(crate) fn known_harness(name: &str, agent: Option<&Settings>) -> Result<(), Report> {
    let known = settings::KNOWN.iter().any(|(k, _, _)| *k == name)
        || grants::exists(name)
        || agent.is_some_and(|a| a.harness() == name || a.box_of(name).is_dir())
        || Adapter::find(name).is_ok();
    if !known {
        return Err(Report::error(sentence!(
            "There is no command or harness {} here.",
            Name::typed("harness", name)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent --help"), "list the commands")
        .next(cmd!("lnk agent harness list"), "list the harnesses"));
    }
    Ok(())
}
