//! `lnk agent delete`, `lnk agent harness delete` and `lnk-agent
//! uninstall`: an agent, a harness or every agent gone.

use std::path::PathBuf;

use link_files::layout::Hidden;

use link_agent::{
    adapter::Adapter,
    grants, moved, service,
    settings::{self, Settings},
    traces,
};
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Path, Question, Report, Sentence, Table, Value, cmd,
    prose, sentence,
};

use crate::confine::forget_defined_tools;
use crate::inspecting::{guessed, link_files, outside};
use crate::services::on_stop;
use crate::{and_list, teams, tilde};

/// Link's hidden locations of agents that uninstalling deletes.
const HIDDEN: &[Hidden] = &[
    Hidden::Agents,
    Hidden::AgentCredentials,
    Hidden::AgentData,
    Hidden::HarnessHistory,
    Hidden::AgentMove,
    Hidden::AgentPolicy,
    Hidden::HarnessGrants,
    Hidden::AgentPlaces,
    Hidden::SharedMemory,
    Hidden::SharedHarnesses,
    Hidden::SharedBridges,
    Hidden::TeamMemory,
    Hidden::Teams,
    Hidden::Sending,
    Hidden::Receiving,
    Hidden::Restoring,
    Hidden::TeamMoving,
    Hidden::Moving,
    Hidden::Held,
    Hidden::StoreScratch,
    Hidden::Moved,
];

/// Of [`HIDDEN`], those holding an agent's keys, logins or conversations,
/// which an image of this machine must never carry: its settings, its
/// credentials, its harnesses' homes (with the `.env` a harness keeps
/// its keys in), shared or kept, and where it went.
const HIDDEN_SECRETS: &[Hidden] = &[
    Hidden::Agents,
    Hidden::AgentCredentials,
    Hidden::AgentData,
    Hidden::SharedHarnesses,
    Hidden::HarnessHistory,
    Hidden::Moved,
];

fn hidden(locations: &[Hidden]) -> anyhow::Result<Vec<PathBuf>> {
    locations.iter().map(|at| at.try_path()).collect()
}

/// `lnk-agent uninstall` (`link_core::uninstall`): every agent here
/// stops, since its programs are going; deleting `agents`, each harness
/// goes with everything of it outside its home, then Link's folders and
/// settings of every agent, and their Keychain items.
pub(crate) fn uninstall(args: &link_core::uninstall::Flags) -> anyhow::Result<()> {
    use link_core::uninstall as u;
    let names = settings::names().unwrap_or_default();
    let mut kept = u::Kept {
        parts: vec![u::Part {
            name: "agents".into(),
            about: "your agents: their harnesses, memory, conversations and settings".into(),
            paths: [
                u::files(link_files::layout::Home::Agents)?,
                link_files::layout::Runtime::AgentNetwork.try_path()?,
                u::files(link_files::layout::Home::AgentDefaults)?,
                u::files(link_files::layout::Home::TeamSpecs)?,
            ]
            .into_iter()
            .chain(hidden(HIDDEN)?)
            .collect(),
            keys: vec!["agent/".into()],
            first: String::new(),
            // The agents' files, settings, homes and notes; not this
            // machine's defaults for agents, or its harnesses' grants.
            secrets: Some(
                [
                    u::files(link_files::layout::Home::Agents)?,
                    link_files::layout::Runtime::AgentNetwork.try_path()?,
                ]
                .into_iter()
                .chain(hidden(HIDDEN_SECRETS)?)
                .collect(),
            ),
            credentials: names
                .iter()
                .filter_map(|name| Settings::load_without_keys(name).ok())
                .flat_map(|agent| agent.held())
                .collect(),
        }],
        ..u::Kept::default()
    };
    if !names.is_empty() {
        kept.parts[0].about += &format!(" ({})", names.join(", "));
        kept.stops = format!("Your agents stop: {}.", names.join(", "));
    }
    let stop = || {
        for name in &names {
            service::stop_foreground(name)?;
            service::stop(name)?;
        }
        Ok(())
    };
    let delete = |_: &[String]| {
        let mut failures = Vec::new();
        for name in &names {
            let agent = Settings::load(name)?;
            let harnesses = settings::harnesses()
                .into_iter()
                .map(|(harness, _)| harness)
                .chain([agent.harness().to_string()])
                .filter(|h| Adapter::find(h).is_ok());
            for harness in harnesses.collect::<std::collections::BTreeSet<_>>() {
                let gone =
                    plan(&agent, &harness, true).and_then(|plan| erase(&agent, &harness, &plan));
                match gone {
                    Ok(failed) => failures.extend(failed),
                    Err(err) => failures.push(format!("removing {harness} of {name}: {err:#}")),
                }
            }
            if let Err(err) = agent.forget_keys() {
                failures.push(format!("{name}'s keys stay in your secrets: {err:#}"));
            }
        }
        // Carried on past each, so all that could go has.
        anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
        Ok(())
    };
    u::run(args, &kept, stop, delete)
}

/// What a deletion couldn't remove, by its path and why, for the user to
/// remove by hand: the agent is gone all the same.
pub(crate) fn left_behind(out: &mut Out, agent: &Name, left: &[String]) {
    for left in left {
        out.block(
            Report::warning(sentence!(
                "Some of {}'s files couldn't be removed: remove them yourself.",
                agent
            ))
            .why(prose!(
                "{}. A folder it can't write may need {} first.",
                Foreign::new(left),
                cmd!("chmod -R u+w")
            )),
        );
    }
}

/// `lnk agent delete <name>`: everything of agent `name` here goes, after
/// asking: its service, settings, harness homes, log, keys, sandbox
/// rules and tools (`moved::delete`); its own files folder after asking
/// again, else put aside. Its buckets stay in your clouds.
pub(crate) async fn delete(
    out: &mut Out,
    name: &str,
    yes: bool,
    files: bool,
) -> Result<Done, Report> {
    // Never relative to wherever this runs.
    link_files::try_home_dir()?;
    link_files::try_installation_dir()?;
    let agent = Settings::resolve_without_keys(Some(name))?;
    if service::foreground_running(name) {
        return Err(foreground_refusal(name));
    }
    let shown = Name::new("agent", name);
    let own_files = agent.files().starts_with(agent.dir());
    let own = agent.folder().agent;
    // Its files: its Home where it's its own, and its own folder.
    let files_text = match own_files {
        true => format!("{} and {}", tilde(&agent.files()), tilde(&own)),
        false => tilde(&own),
    };
    let has_files = (own_files && agent.files().exists()) || own.exists();
    let mut goes = Table::new(["Goes"], sentence!("Nothing of it is here."));
    for path in [agent.path(), agent.dir(), settings::config_of(name)] {
        if path.exists() {
            goes.row([Path::new(path)]);
        }
    }
    out.now(goes);
    let why = match link_core::find_module("storage").is_some() {
        true => prose!(
            "That deletes its settings, its harnesses with their memory and conversations, \
             its log and its keys. It stops first. Its buckets stay in your clouds."
        ),
        false => prose!(
            "That deletes its settings, its harnesses with their memory and conversations, \
             its log and its keys. It stops first."
        ),
    };
    let sure = Question::confirm(sentence!("Remove {} for good?", &shown), false)
        .why(why)
        .yes(yes);
    if !out.ask(sure)? {
        out.block(Note(sentence!("Nothing removed.")));
        return Ok(Done::shown());
    }
    let files = files
        || has_files
            && !yes
            && out.ask(
                Question::confirm(sentence!("Delete its files too, {}?", files_text), false).why(
                    prose!(
                        "No keeps them, with its settings, in one archive in {}, for lnk archive \
                         import to bring back.",
                        Path::new(link_files::layout::Home::Archive.path())
                    ),
                ),
            )?;
    let kept = match files {
        true => moved::Kept::Nothing,
        false => moved::Kept::Files,
    };
    // The calendar stops waking its scheduler before anything of it goes:
    // left, it would start a runner for an agent that's gone.
    service::take_calendar(name).map_err(|err| {
        Report::error(sentence!("Couldn't delete {}.", Name::new("agent", name)))
            .why(Sentence::adopt(&format!(
                "The calendar still wakes its scheduler, and would start it once it's gone: {err:#}"
            )))
            .next(
                cmd!("lnk agent delete {}", Name::new("agent", name)),
                "try again",
            )
    })?;
    // What it keeps is kept before anything of it goes.
    let kept = link_agent::deleted::preserve(&agent, kept)?;
    // Its environment's services stop, and what it kept of it goes.
    crate::environment::remove(name)?;
    let cleared = moved::delete(&agent, kept)?;
    // Its ID is never given again: no grant, log or measure naming it
    // comes to mean another agent.
    if let Some(id) = agent.agent_id() {
        link_agent::settings::ids::retire(id)?;
    }
    // Nor its service: a grant to it never reaches another's.
    if let Some(service) = &agent.service {
        link_agent::settings::ids::SERVICES.retire(service)?;
    }
    // The install it was brought in from runs again, as it did before.
    let home = link_files::try_home_dir()?;
    for at in &agent.stopped_services {
        let at = link_files::relative::Relative::new(at)?;
        let path = at.under(&home);
        if link_agent::migrating::turn_back_on(&home, &at)? {
            out.block(Note(sentence!(
                "Turned its install's own service back on, {}.",
                Path::new(&path)
            )));
        }
    }
    on_stop(name, false);
    link_sandbox_binding::Keyring::here()
        .unbind(&agent.key(), &link_secret_contract::keeper::RouteRevoker);
    // What its model calls were spent on goes with it.
    link_model_contract::context::forget(&link_files::installation_dir(), &agent.key())?;
    forget_defined_tools();
    // Off every team it was on.
    let left = agent
        .agent_id()
        .map(|id| teams::leave_all(out, id))
        .unwrap_or_default();
    // Its buckets stay, but this machine lets go of their encryption, as
    // when it moves away: its password is asked again to restore it.
    if let Some(id) = agent.agent_id()
        && link_core::find_module("storage").is_some()
        && let Ok(owner) = link_agent::owners::agent_flag(id)
    {
        let _ = link_core::lnk()
            .args(["storage", "leave"])
            .args(owner)
            .stdin(std::process::Stdio::null())
            .status();
    }
    if !left.is_empty() {
        out.block(Note(sentence!(
            "{} is off team {} now.",
            &shown,
            Value::List(left.iter().map(|t| Name::new("team", t).into()).collect())
        )));
    }
    for note in cleared.notes {
        out.block(Note(Sentence::adopt(&note)));
    }
    left_behind(out, &shown, &cleared.left);
    // Its teammates running take its leaving at a start of their own.
    teams::restart_after_leaving(out, &left).await?;
    Ok(Done::success(sentence!("Removed {}.", &shown))
        .next(cmd!("lnk agent create <name>"), "make another"))
}

/// What `lnk agent harness delete` deletes of harness `name`: everything
/// it surely is, and what is found by its name alone.
struct Plan {
    /// Goes now.
    doomed: Vec<traces::Trace>,
    /// Found by its name alone: only after asking, never with `--yes`.
    by_name: Vec<traces::Trace>,
    /// The other agents here that run it, which keep its grants.
    others: Vec<String>,
}

fn plan(settings: &Settings, name: &str, yes: bool) -> anyhow::Result<Plan> {
    let adapter = Adapter::find(name)?;
    // Never relative to wherever this runs.
    link_files::try_home_dir()?;
    link_files::try_installation_dir()?;
    let dir = settings.box_of(name);
    let mut doomed: Vec<traces::Trace> = dir
        .exists()
        .then(|| dir.clone())
        .into_iter()
        .chain(
            link_files(settings, name)
                .into_iter()
                .filter(|p| p.exists()),
        )
        .map(|path| traces::Trace {
            path,
            known: true,
            kind: None,
        })
        .collect();
    // Its grants go with it, unless another agent here runs it.
    let others: Vec<String> = settings::names()?
        .into_iter()
        .filter(|n| *n != settings.name)
        .filter(|n| Settings::load_without_keys(n).is_ok_and(|s| s.harness() == name))
        .collect();
    if others.is_empty() && grants::exists(name) {
        doomed.push(traces::Trace {
            path: grants::path(name)?,
            known: true,
            kind: None,
        });
    }
    let (known, by_name): (Vec<_>, Vec<_>) =
        outside(&adapter, name).into_iter().partition(|t| t.known);
    doomed.extend(known);
    if !yes {
        doomed.extend(by_name.iter().cloned());
    }
    Ok(Plan {
        doomed,
        by_name,
        others,
    })
}

/// `plan` carried out; what couldn't be removed, said.
fn erase(settings: &Settings, name: &str, plan: &Plan) -> anyhow::Result<Vec<String>> {
    let doomed: Vec<PathBuf> = plan.doomed.iter().map(|t| t.path.clone()).collect();
    if settings.harness() == name {
        service::stop(&settings.name)?;
    }
    let mut failed = Vec::new();
    for path in &doomed {
        // A background service of its own is stopped before its file goes.
        let service_file = path
            .parent()
            .is_some_and(|p| p.ends_with("Library/LaunchAgents") || p.ends_with("systemd/user"));
        if service_file && let Some(label) = path.file_name().and_then(|n| n.to_str()) {
            let label = label.trim_end_matches(".plist");
            service::unload_other(label);
        }
        let result = match path.symlink_metadata() {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
            Ok(_) => std::fs::remove_file(path),
            Err(_) => Ok(()),
        };
        if let Err(err) = result {
            failed.push(format!("{}: {err}", tilde(path)));
        }
    }
    Ok(failed)
}

/// `lnk agent harness delete --harness <harness>`: the home and every trace, after asking.
pub(crate) fn remove(
    out: &mut Out,
    settings: &Settings,
    name: &str,
    yes: bool,
) -> Result<Done, Report> {
    let harness = Name::new("harness", name);
    let plan = plan(settings, name, yes)?;
    if plan.doomed.is_empty() && plan.by_name.is_empty() {
        return Ok(Done::success(sentence!(
            "Nothing of {} is on this machine.",
            &harness
        )));
    }
    let mut goes = Table::new(["Goes", "Note"], sentence!("Nothing is kept of it."));
    for trace in &plan.doomed {
        goes.row([
            Value::from(Path::new(&trace.path)),
            Value::from(guessed(trace)),
        ]);
    }
    out.now(goes);
    out.block(Note(sentence!(
        "Your files ({}) and your agent's settings stay.",
        Path::new(settings.files())
    )));
    if !plan.others.is_empty() && grants::exists(name) {
        out.block(Note(sentence!(
            "Its grants stay: {} run{} it too.",
            Foreign::new(&and_list(&plan.others)),
            if plan.others.len() == 1 { "s" } else { "" }
        )));
    }
    if yes && !plan.by_name.is_empty() {
        let mut by_name = Table::new(["Not removed"], sentence!("Nothing."));
        for trace in &plan.by_name {
            by_name.row([Path::new(&trace.path)]);
        }
        out.now(by_name);
        out.block(Note(sentence!(
            "These were found by their name only, so --yes leaves them: check, and remove them \
             by hand."
        )));
    }
    let sure = Question::confirm(
        sentence!(
            "Remove {} for good, with its memory and conversations?",
            &harness
        ),
        false,
    )
    .yes(yes);
    if !out.ask(sure)? {
        out.block(Note(sentence!("Nothing removed.")));
        return Ok(Done::shown());
    }
    for failed in erase(settings, name, &plan)? {
        out.block(Report::warning(sentence!(
            "Couldn't remove {}.",
            Foreign::new(&failed)
        )));
    }
    Ok(Done::success(sentence!("Removed {}.", &harness)).next(
        cmd!("lnk agent harness set {}", &harness),
        "install it again, boxed, asking again for what it needs beyond Link's defaults",
    ))
}

fn foreground_refusal(name: &str) -> Report {
    let shown = Name::new("agent", name);
    Report::error(sentence!("{} runs in a terminal.", &shown))
        .why(prose!("Stop it there with Ctrl-C, then remove it."))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent stop --agent {}", &shown), "stop it")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_delete_refusal_has_declared_navigation() {
        let refusal = foreground_refusal("terminal-main");
        let actions: toml::Value = crate::navigation::navigation();
        let fail: Vec<String> = actions["agent delete"]["fail"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect();
        let typed = cmd!("lnk agent delete terminal-main --yes --delete-files");
        assert!(!refusal.next.0.is_empty());
        for suggestion in &refusal.next.0 {
            assert!(
                link_format::nexts::declares(&fail, &suggestion.command, &typed),
                "the foreground refusal offers an undeclared failure hint: {}",
                suggestion.command.text()
            );
        }
    }

    /// Every location that holds a harness's home, with the keys a
    /// harness keeps there, goes with the agents and is never carried in
    /// an image.
    #[test]
    fn uninstalling_takes_every_harness_home_as_a_secret() {
        let hidden = std::path::Path::new("/hidden");
        for at in Hidden::ALL {
            let home = at.at(hidden).join("main/harnesses/hermes");
            if link_files::layout::in_harness_home(&home, hidden) {
                assert!(HIDDEN.contains(at), "{at:?} isn't uninstalled");
                assert!(HIDDEN_SECRETS.contains(at), "{at:?} isn't a secret");
            }
        }
        assert!(HIDDEN_SECRETS.iter().all(|at| HIDDEN.contains(at)));
    }
}
