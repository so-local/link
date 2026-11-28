//! `lnk agent migrate`: finds an install of OpenClaw, Hermes or DeepSeek
//! made outside Link, says the whole plan, and on a yes runs it through
//! its journal, saying each step as it ends.

use link_agent::adapter::Adapter;
use link_agent::journal::Journal;
use link_agent::journal::Step;
use link_agent::migrating::{self, Agent, Found, HARNESSES};
use link_agent::settings::{self, Settings};
use link_format::{
    Count, Done, Exit, Name, Note, Out, Path, Product, Question, Report, Sentence, Table, Value,
    Version, WithinExt, cmd, prose, sentence,
};

/// `lnk agent migrate [--harness <name>] [--yes]`, the install's own
/// agent `named`; its other agents keep their names.
pub(crate) fn run(
    out: &mut Out,
    named: Option<&str>,
    harness: Option<&str>,
    yes: bool,
) -> Result<Done, Report> {
    let home = link_files::try_home_dir()?;
    link_files::try_installation_dir()?;
    let found = found(harness, &home)?;
    let adapter = Adapter::find_or_add(&found.harness)?;
    let name = name_of(&found.harness);
    let own_home = found
        .own()
        .home()
        .map(|p| p.at.under(&home))
        .unwrap_or_default();
    // A state Link can't read is refused before anything is copied.
    let mut versions = Vec::new();
    for agent in &found.agents {
        let Some(at) = agent.home().map(|p| p.at.under(&home)) else {
            continue;
        };
        let written = adapter.written(&at)?;
        if let Some(why) = written.refused {
            return Err(Report::error(sentence!(
                "{} at {} can't be migrated.",
                Product::new(name),
                Path::new(&at)
            ))
            .why(Sentence::adopt(&why)));
        }
        versions.extend(written.version);
    }
    let members: Vec<&str> = found
        .agents
        .iter()
        .filter_map(|a| a.name.as_deref())
        .collect();
    let begun =
        Journal::of_harness(&found.harness)?.filter(|j| !members.contains(&j.agent.as_str()));
    let own = match (named, &begun) {
        (Some(name), _) => name.to_string(),
        (None, Some(journal)) => journal.agent.clone(),
        (None, None) => found.harness.clone(),
    };
    let names: Vec<String> = found
        .agents
        .iter()
        .map(|a| a.name.clone().unwrap_or_else(|| own.clone()))
        .collect();
    out.block(Note(sentence!(
        "Found {} at {}.",
        Product::new(name),
        Path::new(&own_home)
    )));
    if let Some(version) = versions.first() {
        out.block(Note(sentence!(
            "Its state was written by version {}, which Link reads.",
            Version::new(version)
        )));
    }
    // Several agents: each named, any left out.
    let mut chosen = vec![true; names.len()];
    if names.len() > 1 {
        out.block(Note(sentence!(
            "It holds {} agents, each one of Link's: {}.",
            names.len(),
            Value::List(names.iter().map(|n| Name::new("agent", n).into()).collect())
        )));
        for (at, agent) in names.iter().enumerate() {
            chosen[at] = out.ask(
                Question::confirm(sentence!("Bring in {}?", Name::new("agent", agent)), true)
                    .yes(yes),
            )?;
        }
    }
    let mut runs: Vec<(usize, Journal, Vec<String>)> = Vec::new();
    let mut service = true;
    for (at, agent) in names.iter().enumerate().filter(|(at, _)| chosen[*at]) {
        link_machine::machine::check_name(agent)?;
        let shown = Name::new("agent", agent);
        let from = &found.agents[at];
        // Begun before: resumed, the plan it wrote.
        let (journal, resumed) = match Journal::load(agent)? {
            Some(journal) => (journal, true),
            None if Settings::path_of(agent).exists() => {
                if Settings::load_without_keys(agent).is_ok_and(|s| s.harness() == found.harness)
                    && from.name.is_some()
                {
                    out.block(Note(sentence!(
                        "{} is under Link already: left as it is.",
                        &shown
                    )));
                    continue;
                }
                return Err(Report::error(sentence!(
                    "There is an agent named {} here already.",
                    &shown
                ))
                .exit(Exit::Usage)
                .next(cmd!("lnk agent migrate --agent <name>"), "name another"));
            }
            None => {
                let mut steps = migrating::plan(from, &home, service)?;
                let adapter = link_agent::adapter::Adapter::find(&found.harness)?;
                if adapter
                    .contract()?
                    .state
                    .export
                    .contains(&link_harness_contract::asp::Kind::Schedules)
                    && let Some(place) = from.home()
                {
                    let schedules = adapter.native_schedules(&place.at.under(&home))?;
                    if schedules.source_revision.is_some() {
                        let before_secrets = steps
                            .iter()
                            .position(|step| {
                                matches!(
                                    step,
                                    Step::Secret { .. } | Step::Model { .. } | Step::Sealed { .. }
                                )
                            })
                            .unwrap_or(steps.len());
                        steps.insert(
                            before_secrets,
                            Step::Schedules {
                                at: place.lands.clone(),
                                names: schedules
                                    .carried
                                    .iter()
                                    .map(link_agent::schedules::words)
                                    .collect(),
                                not_carried: schedules
                                    .not_carried
                                    .iter()
                                    .map(|s| format!("{}: {}", s.name, s.reason))
                                    .collect(),
                            },
                        );
                    }
                }
                (Journal::plan(agent, &found.harness, steps), false)
            }
        };
        service &= !journal
            .entries
            .iter()
            .any(|e| matches!(e.step, Step::Service { .. }));
        let harness_home = settings::boxes(agent).join(&journal.harness);
        if resumed {
            let done = journal.entries.iter().filter(|e| e.done).count();
            out.block(Note(sentence!(
                "Its migration stopped partway ({} of {} steps done): carrying on.",
                done,
                journal.entries.len()
            )));
        }
        let steps: Vec<_> = journal.entries.iter().map(|e| e.step.clone()).collect();
        let words = migrating::words(&found.harness, from, &steps, agent, &harness_home, &home);
        for line in &words {
            out.block(Note(Sentence::adopt(line)));
        }
        // Each variable whole, where it goes: a name may say less than its use.
        let handed = migrating::handed(from, &home, agent);
        if !handed.is_empty() {
            out.block(Note(sentence!(
                "Its variables, what Link makes of each, and where it's sent: check each goes where you expect."
            )));
            let mut table = Table::new(
                ["Variable", "Link makes it", "Sent to"],
                sentence!("It has no variables."),
            );
            for row in handed {
                table.row(row);
            }
            out.now(table);
        }
        runs.push((at, journal, words));
    }
    if runs.is_empty() {
        out.block(Note(sentence!("Nothing to bring in.")));
        return Ok(Done::shown());
    }
    if !out.ask(Question::confirm(sentence!("Go ahead?"), false).yes(yes))? {
        out.block(Note(sentence!("Nothing changed.")));
        return Ok(Done::shown());
    }
    let stop = link_secretive::test_var("LNK_TEST_MIGRATE_STOP")
        .and_then(|v| v.to_str().and_then(|v| v.parse::<usize>().ok()));
    let mut said_all = 0;
    for (_, journal, words) in &mut runs {
        let plan = journal.clone();
        let agent = journal.agent.clone();
        let mut said = journal.entries.iter().filter(|e| e.done).count();
        let ran = journal.run(|step| {
            if stop == Some(said_all) {
                anyhow::bail!("stopped before step {said_all}, as LNK_TEST_MIGRATE_STOP says");
            }
            migrating::run(step, &plan, &home)?;
            out.block(Note(Sentence::adopt(&format!("Done: {}", words[said]))));
            said += 1;
            said_all += 1;
            Ok(())
        });
        if let Err(err) = ran {
            let agent = Name::new("agent", &agent);
            let again = match named {
                Some(n) => cmd!("lnk agent migrate --agent {}", Name::new("agent", n)),
                None => cmd!("lnk agent migrate"),
            };
            return Err(
                Report::error(sentence!("The migration of {} stopped.", &agent))
                    .cause(err)
                    .next(again, "carry on")
                    .next(cmd!("lnk agent delete {}", &agent), "clear what it did"),
            );
        }
    }
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let mut done = Done::success(sentence!(
        "{} under Link, run by {}.",
        Count::new(runs.len() as u64, "agent is", "agents are"),
        Product::new(name)
    ));
    for (at, journal, _) in &runs {
        let agent = &journal.agent;
        let shown = Name::new("agent", agent);
        done = done.changed("Under Link", &shown);
        hand_over(
            out,
            agent,
            &found.harness,
            &found.agents[*at],
            &home,
            terminal && !yes,
        );
        // Started under Link, in a terminal, on a yes: its harness
        // installed in its home, the model asked for if Link has none for
        // it yet.
        let wanted = terminal
            && !yes
            && out.ask(Question::confirm(sentence!("Start {} now?", &shown), true))?;
        let started = wanted && {
            out.hand_over();
            link_core::lnk()
                .args(["agent", "-a", agent, "start"])
                .status()
                .is_ok_and(|s| s.success())
        };
        if !started {
            done = done.next(cmd!("lnk agent start --agent {}", &shown), "start it");
        }
    }
    let first = Name::new("agent", &runs[0].1.agent);
    Ok(done
        .next(
            cmd!("lnk agent migrate --clean"),
            "remove the original install once it answers, naming every file first",
        )
        .next(
            cmd!("lnk agent harness set link --agent {}", &first),
            "switch it to Link Harness",
        ))
}

/// The one install found here of harness `harness`, or of any Link
/// migrates when it's none.
fn found(harness: Option<&str>, home: &std::path::Path) -> Result<Found, Report> {
    let uid = link_service::user_id().unwrap_or_default();
    let wanted = wanted(harness)?;
    let mut found: Vec<Found> = Vec::new();
    for name in &wanted {
        let adapter = Adapter::find_or_add(name)?;
        found.extend(migrating::find(name, &adapter.traces(), home, &uid));
    }
    match found.as_slice() {
        [] => Err(Report::error(sentence!(
            "Found no install of {} here outside Link.",
            and_or(&wanted)
        ))),
        [one] => Ok(one.clone()),
        many => Err(several(
            &many.iter().map(|f| f.harness.as_str()).collect::<Vec<_>>(),
        )),
    }
}

/// The harnesses `--harness` allows: the one named, else every one Link
/// migrates.
fn wanted(harness: Option<&str>) -> Result<Vec<&str>, Report> {
    match harness {
        Some(h) if HARNESSES.contains(&h) => Ok(vec![h]),
        Some(h) => Err(Report::error(sentence!(
            "Link migrates OpenClaw, Hermes or DeepSeek, not {}.",
            h.to_string()
        ))
        .exit(Exit::Usage)),
        None => Ok(HARNESSES.to_vec()),
    }
}

/// Several installs found, where one is wanted.
fn several(harnesses: &[&str]) -> Report {
    Report::error(sentence!("Found {} here.", and_or(harnesses)))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent migrate --harness <name>"), "pick one")
}

/// Agent `agent`'s bots, from agent `from` of an install of `harness`,
/// handed over: when `connect`, the connection's connect with the token
/// given, so only confirming the bot is left to the owner; else said.
fn hand_over(
    out: &mut Out,
    agent: &str,
    harness: &str,
    from: &Agent,
    home: &std::path::Path,
    connect: bool,
) {
    for connection in migrating::connections(from, home) {
        if connect {
            let mut command = link_core::lnk();
            command.args(["agent", "-a", agent, "connect", connection]);
            for (var, value) in migrating::tokens_for(connection, from, home) {
                link_secretive::process::give(&mut command, var, &value);
            }
            out.hand_over();
            if command.status().is_ok_and(|s| s.success()) {
                continue;
            }
        }
        out.block(Note(sentence!(
            "Its {} bot isn't handed to Link yet: {} does it, with the token in {}'s settings.",
            connection.to_string(),
            cmd!(
                "lnk agent connect {} --agent {}",
                connection.to_string(),
                Name::new("agent", agent)
            ),
            Product::new(name_of(harness))
        )));
    }
}

/// `lnk agent migrate --clean`: the original install of a harness an
/// agent here was migrated from, every file of it named, then removed on
/// a yes. Refused while its migration is unfinished, or when no agent
/// here runs that harness.
pub(crate) fn clean(out: &mut Out, harness: Option<&str>, yes: bool) -> Result<Done, Report> {
    let home = link_files::try_home_dir()?;
    let uid = link_service::user_id().unwrap_or_default();
    let wanted = wanted(harness)?;
    let mut installs = Vec::new();
    for name in &wanted {
        let adapter = Adapter::find_or_add(name)?;
        let traces = adapter.traces();
        if migrating::find(name, &traces, &home, &uid).is_some() {
            installs.push((name.to_string(), traces));
        }
    }
    let (harness, traces) = match installs.as_slice() {
        [] => {
            return Err(Report::error(sentence!(
                "Found no install of {} here outside Link to clean.",
                and_or(&wanted)
            )));
        }
        [one] => one.clone(),
        many => {
            return Err(several(
                &many.iter().map(|(h, _)| h.as_str()).collect::<Vec<_>>(),
            ));
        }
    };
    if let Some(journal) = Journal::of_harness(&harness)? {
        return Err(Report::error(sentence!(
            "The migration of {} into {} isn't finished.",
            Product::new(name_of(&harness)),
            Name::new("agent", &journal.agent)
        ))
        .next(cmd!("lnk agent migrate"), "carry on first"));
    }
    let runs: Vec<String> = settings::names()?
        .into_iter()
        .filter(|n| Settings::load_without_keys(n).is_ok_and(|s| s.harness() == harness))
        .collect();
    if runs.is_empty() {
        return Err(Report::error(sentence!(
            "No agent here runs {}.",
            Product::new(name_of(&harness))
        ))
        .next(cmd!("lnk agent migrate"), "bring it under Link first"));
    }
    let mut doomed: Vec<std::path::PathBuf> = Vec::new();
    for trace in &traces {
        let at = trace.at(&home, &uid);
        let kept = at
            .strip_prefix(&home)
            .ok()
            .and_then(|rest| link_files::relative::Relative::new(&rest.to_string_lossy()).ok())
            .map(|rel| migrating::stopped_places(&home, &rel))
            .transpose()?
            .unwrap_or_default();
        for path in std::iter::once(at).chain(kept) {
            if path.symlink_metadata().is_ok() && !doomed.iter().any(|d| path.starts_with(d)) {
                doomed.retain(|d| !d.starts_with(&path));
                doomed.push(path);
            }
        }
    }
    let mut goes = Table::new(["Goes"], sentence!("Nothing is left of it."));
    for path in &doomed {
        goes.row([Path::new(path)]);
    }
    out.now(goes);
    let sure = Question::confirm(sentence!("Remove them for good?"), false)
        .why(prose!(
            "{} runs under Link now, as {}. This removes the original install.",
            Product::new(name_of(&harness)),
            Value::List(runs.iter().map(|n| Name::new("agent", n).into()).collect())
        ))
        .yes(yes);
    if !out.ask(sure)? {
        out.block(Note(sentence!("Nothing removed.")));
        return Ok(Done::shown());
    }
    for path in &doomed {
        let removed = match path.symlink_metadata() {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
            Ok(_) => std::fs::remove_file(path),
            Err(_) => Ok(()),
        };
        removed.within(format!("removing {}", path.display()))?;
    }
    Ok(Done::success(sentence!(
        "Removed the original {}.",
        Product::new(name_of(&harness))
    )))
}

/// How a harness is named to a person.
fn name_of(harness: &str) -> &str {
    match harness {
        "openclaw" => "OpenClaw",
        "hermes" => "Hermes",
        "deepseek" => "DeepSeek",
        other => other,
    }
}

fn and_or(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => name_of(one).to_string(),
        [rest @ .., last] => format!(
            "{} or {}",
            rest.iter()
                .map(|n| name_of(n))
                .collect::<Vec<_>>()
                .join(", "),
            name_of(last)
        ),
    }
}
