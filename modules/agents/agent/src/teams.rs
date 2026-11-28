//! `lnk agent team`: teams of your agents. A team is a file of its own
//! (`~/Link/Safe/Teams/<team>.toml`): its members by ID, each with a
//! name in it and a purpose, a folder they share, and the opinions they
//! share beneath each agent's spec (`link_agent_spec::Team`).
//!
//! What a member reaches of the shared folder is its own grant, in its
//! sandbox file (`[teams.<team>]`). Whom it may send to is read
//! from the teams' files: by its scheduler at its start, and by each
//! bridge again as a message comes. A change takes effect at each
//! member's next start, which these commands make for the members that
//! run, so nothing changes inside a turn.

mod constants;
pub(crate) mod defining;
mod grants;
mod lifecycle;
mod memory;
pub(crate) mod moving;
mod remote;
mod specifying;
mod talking;

use grants::{grant_of, no_grant};
pub(crate) use moving::finishing;
pub(crate) use remote::hosted_teams;

use link_format::WithinExt;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use link_agent::cli;
use link_agent::sandbox_file;
use link_agent::service;
use link_agent::settings::{self, Settings};
use link_agent_spec::Team;
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Path as Where, Question, Report, Section, Sentence,
    Table, Value, cmd, phrase, prose, sentence, states,
};
use link_harness_contract::harness::{TeamFolders, WHOLE};

states! {
    /// Whether a team member's agent runs.
    enum Member {
        Running("running") => Good,
        Stopped("stopped") => Quiet,
        Away("not on this machine") => Quiet,
    }
}

/// Whether `command` is a team's: its agents are named in the command.
pub(crate) fn owns(command: &cli::Command) -> bool {
    use cli::Command as C;
    matches!(
        command,
        C::TeamStart(_)
            | C::TeamStop(_)
            | C::TeamCreate(_)
            | C::TeamAdd(_)
            | C::TeamRemove(_)
            | C::TeamMemberAdd(_)
            | C::TeamBackup(_)
            | C::TeamRestore(_)
            | C::TeamJoin(_)
            | C::TeamLeave(_)
            | C::TeamMove(_)
            | C::TeamList(_)
            | C::TeamDelete(_)
            | C::TeamShow(_)
            | C::TeamMemoryStart(_)
            | C::TeamMemoryStop(_)
    ) || matches!(command, C::ChatSend(send) if send.team.is_some())
}

/// `lnk agent team ...`.
pub async fn run(
    out: &mut Out,
    command: cli::Command,
    fields: &link_spec::FieldFlags,
) -> Result<Done, Report> {
    use cli::Command as C;
    let _control = match &command {
        C::ChatSend(_) | C::TeamList(_) | C::TeamShow(_) | C::TeamBackup(_) | C::TeamRestore(_) => {
            None
        }
        _ => Some(control()?),
    };
    match command {
        C::TeamStart(a) => {
            lifecycle::run(out, cli::hosted_name(&a.team, a.cloud.as_deref()), true).await
        }
        C::TeamStop(a) => {
            lifecycle::run(out, cli::hosted_name(&a.team, a.cloud.as_deref()), false).await
        }
        C::ChatSend(args) => talking::run(out, args).await,
        C::TeamMemoryStart(args) => memory::run(out, &args.team, true).await,
        C::TeamMemoryStop(args) => memory::run(out, &args.team, false).await,
        C::TeamCreate(cli::TeamCreateArgs {
            team,
            spec: Some(spec),
            cloud,
            ..
        }) => specifying::apply(out, team.as_deref(), &spec, fields, cloud.as_deref()).await,
        C::TeamCreate(cli::TeamCreateArgs {
            team,
            with,
            no_folder,
            cloud,
            write,
            read,
            ..
        }) => {
            if cloud.is_some() {
                return Err(Report::error(sentence!(
                    "A team a cloud hosts is made from its spec: give it with {}.",
                    cmd!("--spec <file>")
                ))
                .exit(Exit::Usage));
            }
            // Field flags are lines of a team spec: without one, they'd
            // be dropped, so they're refused, but `--lead`, which names
            // one of the agents given.
            let lead = fields.get(&["lead"]).and_then(toml::Value::as_str);
            if fields.table().keys().any(|key| key != "lead") {
                return Err(Report::error(sentence!(
                    "A team spec's field flags go over its file: give it with {}.",
                    cmd!("--spec <file>")
                ))
                .exit(Exit::Usage));
            }
            let Some(team) = team else {
                return Err(Report::error(sentence!(
                    "Name the team, or give its spec with {}.",
                    cmd!("--spec <file>")
                ))
                .exit(Exit::Usage));
            };
            let grant = grant_of(&write, &read)?;
            new(out, &team, &with, no_folder, lead, grant).await
        }
        C::TeamAdd(cli::TeamAddArgs {
            team,
            member,
            purpose,
            r#as,
            write,
            read,
            ..
        }) => {
            let grant = grant_of(&write, &read)?;
            let added = add(&team, &member, r#as.as_deref(), purpose.as_deref(), grant).await?;
            let restarted = match added.roster {
                true => names_of(&team_now(&team)?, &by_id()),
                false => added.changed.then(|| member.clone()).into_iter().collect(),
            };
            restart(out, &restarted).await?;
            Ok(Done::success(added.said).changed(
                "Kept in the team's file",
                Where::new(settings::team_path(&team)),
            ))
        }
        C::TeamRemove(cli::TeamRemoveArgs {
            team,
            member,
            cloud,
            ..
        }) => {
            let team = cli::hosted_name(&team, cloud.as_deref());
            if remote::is_remote(&team) {
                remote::remove(out, &team, &member)
            } else {
                remove(out, &team, &member).await
            }
        }
        C::TeamMemberAdd(mut args) => {
            args.team = cli::hosted_name(&args.team, args.cloud.as_deref());
            remote::invite(out, args)
        }
        C::TeamBackup(a) => {
            remote::snapshot(out, &cli::hosted_name(&a.team, a.cloud.as_deref()), false)
        }
        C::TeamRestore(a) => {
            remote::snapshot(out, &cli::hosted_name(&a.team, a.cloud.as_deref()), true)
        }
        C::TeamJoin(a) => {
            remote::joining(out, joined(&a.team, &a.account, &a.cloud)?, &a.member, true)
        }
        C::TeamLeave(a) => remote::joining(
            out,
            joined(&a.team, &a.account, &a.cloud)?,
            &a.member,
            false,
        ),
        C::TeamMove(args) => moving::run(out, args),
        C::TeamList(args) => {
            let done = list(out, args.json)?;
            remote::listed(out, args.json)?;
            Ok(done)
        }
        C::TeamDelete(cli::TeamDeleteArgs {
            team, cloud, yes, ..
        }) => {
            let team = cli::hosted_name(&team, cloud.as_deref());
            if remote::is_remote(&team) {
                remote::delete(out, &team, yes)
            } else {
                delete(out, &team, yes).await
            }
        }
        C::TeamShow(cli::TeamShowArgs {
            team,
            cloud,
            member,
            json,
            ..
        }) => {
            let team = cli::hosted_name(&team, cloud.as_deref());
            if remote::is_remote(&team) {
                remote::show(out, &team, member.as_deref(), json)
            } else {
                show(out, &team, member.as_deref())
            }
        }
        _ => unreachable!("only a team's commands"),
    }
}

/// A team joined or left by another account's agent:
/// `<account>/<team>@<cloud>`.
fn joined(team: &str, account: &Option<String>, cloud: &Option<String>) -> Result<String, Report> {
    match (account, cloud) {
        (Some(account), Some(cloud)) => Ok(format!("{account}/{team}@{cloud}")),
        _ => Err(Report::error(sentence!(
            "Name the team's account and cloud: {} and {}.",
            cmd!("--account <account>"),
            cmd!("--cloud <cloud>")
        ))
        .exit(Exit::Usage)),
    }
}

/// The shared folder a new team is made with, as its file says it.
fn default_shared(team: &str) -> String {
    format!("~/Link/Teams/{team}/shared")
}

/// `lnk agent team create <team> [--with a --with b] [--write f]
/// [--read f] [--no-folder]`: each member named is granted `grant` of
/// the shared folder, or none of it.
async fn new(
    out: &mut Out,
    team: &str,
    agents: &[String],
    no_folder: bool,
    lead: Option<&str>,
    grant: Option<TeamFolders>,
) -> Result<Done, Report> {
    link_agent::settings::check_new_name("teams", team)?;
    let shown = Name::new("team", team);
    let path = settings::team_path(team);
    if path.exists() {
        return Err(
            Report::error(sentence!("There is a team {} already.", &shown))
                .exit(Exit::Usage)
                .next(cmd!("lnk agent team list"), "see it"),
        );
    }
    // Each agent there, before anything is written.
    for agent in agents {
        here(agent)?;
    }
    if let Some(lead) = lead {
        agents
            .iter()
            .any(|name| name == lead)
            .then_some(())
            .within("the lead must be one of the agents --with names")?;
    }
    let mut table = toml::Table::new();
    table.insert("id".into(), settings::ids::TEAMS.reserve()?.into());
    table.insert("name".into(), team.into());
    if !no_folder {
        let mut files = toml::Table::new();
        files.insert("shared".into(), default_shared(team).into());
        table.insert("files".into(), files.into());
    }
    defining::forget_identity(team)?;
    write_team(team, &table)?;
    let made = team_now(team)?;
    if let Some(shared) = &made.shared {
        make_shared(team, shared)?;
    }
    for agent in agents {
        out.block(Note(
            add(team, agent, None, None, grant.clone()).await?.said,
        ));
    }
    if let Some(lead) = lead {
        let mut table = read_team(team)?;
        table.insert("lead".into(), lead.into());
        write_team(team, &table)?;
    }
    restart(out, agents).await?;
    let mut done = Done::success(match &made.shared {
        Some(shared) => sentence!("Team {} is made, sharing {}.", &shown, Where::new(shared)),
        None => sentence!("Team {} is made.", &shown),
    })
    .changed("Kept in", Where::new(&path));
    if agents.is_empty() {
        done = done.next(
            cmd!(
                "lnk agent team add {} --member <agent> --purpose <purpose>",
                &shown
            ),
            "put an agent on it",
        );
    }
    Ok(done)
}

/// Puts agent `agent` on team `team` as `member` (its own name, else),
/// with `purpose`, granted `grant` of the shared folder; or, on it already,
/// changes those. With no `grant`, a new member reaches none of the
/// folder and one already on it keeps what it had.
async fn add(
    team: &str,
    agent: &str,
    member: Option<&str>,
    purpose: Option<&str>,
    grant: Option<TeamFolders>,
) -> Result<Added, Report> {
    let before = team_now(team)?;
    let settings = here(agent)?;
    let id = settings
        .agent_id()
        .map(str::to_string)
        .within("the agent has no ID")?;
    let member = member.unwrap_or(agent).to_string();
    link_machine::machine::check_name(&member).within("--as")?;
    if let Some(purpose) = purpose
        && (purpose.chars().count() > link_agent_spec::PURPOSE_MAX
            || purpose.chars().any(char::is_control))
    {
        return Err(Report::error(sentence!(
            "{} is one line of at most {} characters.",
            Foreign::new("--purpose"),
            link_agent_spec::PURPOSE_MAX as u64
        ))
        .exit(Exit::Usage));
    }
    if let Some(other) = before.members.get(&member)
        && other.id != id
    {
        return Err(Report::error(sentence!(
            "Team {} has a {} already.",
            Name::new("team", team),
            Name::new("member", &member)
        ))
        .exit(Exit::Usage)
        .next(
            cmd!(
                "lnk agent team add {} --member {} --as <name>",
                Name::new("team", team),
                Name::new("agent", agent)
            ),
            "give the agent another name there",
        ));
    }
    let mut table = read_team(team)?;
    let members = table
        .entry("member")
        .or_insert_with(|| toml::Table::new().into())
        .as_table_mut()
        .within("its [member] isn't a table")?;
    // Under another name before: renamed, its purpose kept unless given.
    let old_purpose = before.member(&id).and_then(|(_, m)| m.purpose.clone());
    if let Some((old, _)) = before.member(&id) {
        members.remove(old);
    }
    let mut entry = toml::Table::new();
    entry.insert("id".into(), id.clone().into());
    if let Some(purpose) = purpose.map(str::to_string).or(old_purpose) {
        entry.insert("purpose".into(), purpose.into());
    }
    members.insert(member.clone(), entry.into());
    if before
        .member(&id)
        .is_some_and(|(old, _)| before.lead.as_ref() == Some(old))
    {
        table.insert("lead".into(), member.clone().into());
    }
    let was = std::fs::read_to_string(settings::team_path(team)).within("reading the team")?;
    write_team(team, &table)?;
    // A team whose opinions its own can't take: refused, as it was.
    if let Err(err) = settings.spec() {
        link_files::write_private(&settings::team_path(team), was.as_bytes())
            .within("putting the team back")?;
        return Err(Report::error(sentence!(
            "{} can't be on team {}.",
            Name::new("agent", agent),
            Name::new("team", team)
        ))
        .cause(err));
    }
    let mut sandbox = sandbox_file::load(agent)?;
    let kept = before
        .member(&id)
        .and_then(|_| sandbox.spec.teams.get(team).cloned());
    let joined = before.member(&id).is_none();
    let renamed = before.member(&id).is_some_and(|(old, _)| *old != member);
    let had = kept.clone();
    let grant = grant.or(kept).unwrap_or_else(no_grant);
    let regranted = had.unwrap_or_else(no_grant) != grant;
    match grant.is_empty() {
        true => sandbox.spec.teams.remove(team),
        false => sandbox.spec.teams.insert(team.to_string(), grant.clone()),
    };
    sandbox.check()?;
    sandbox_file::save(agent, &sandbox)?;
    let now = team_now(team)?;
    if let Some(shared) = &now.shared {
        make_shared(team, shared)?;
        for folder in grant.write.iter().chain(&grant.read) {
            if folder != WHOLE {
                std::fs::create_dir_all(shared.join(folder))
                    .within(format!("creating {}", shared.join(folder).display()))?;
            }
        }
    }
    let purpose = match now.member(&id).and_then(|(_, m)| m.purpose.clone()) {
        Some(purpose) => Value::from(format!(" ({purpose})")),
        None => Value::from(""),
    };
    let reaches = match &now.shared {
        Some(_) => Value::from(format!(": {}", reach(&grant))),
        None => Value::from(""),
    };
    // A member messages its teammates through its scheduler, whose caps
    // stop two that keep answering each other: on a team, it's on, for a
    // harness that reads who a message is from, the one that honors it.
    let honors = link_agent::adapter::Adapter::find(settings.harness())
        .is_ok_and(|adapter| adapter.contract().is_ok_and(|c| c.from));
    let scheduled = honors
        && settings
            .set_tool_enabled("scheduler", true)
            .within("turning its scheduler on")?;
    let scheduler = match scheduled {
        true => Value::from(", its scheduler on for messages to its teammates"),
        false => Value::from(""),
    };
    Ok(Added {
        said: sentence!(
            "{} is on team {} as {}{}{}{}.",
            Name::new("agent", agent),
            Name::new("team", team),
            Name::new("member", &member),
            purpose,
            reaches,
            scheduler
        ),
        changed: joined || regranted || scheduled,
        roster: joined || renamed,
    })
}

/// What [`add`] did: the sentence that says so, whether it changed the
/// agent's grant or settings, and whether it changed the team's roster.
/// A purpose alone changes none of them.
struct Added {
    said: Sentence,
    /// Its grant or settings changed: it takes them at a start of its own.
    changed: bool,
    /// It joined, or took another name: each member's scheduler reads whom
    /// it may send to at its start, so every member takes it.
    roster: bool,
}

/// `lnk agent team remove <team> --member <agent>`.
async fn remove(out: &mut Out, team: &str, agent: &str) -> Result<Done, Report> {
    let before = team_now(team)?;
    let id = settings::key_of(agent);
    let Some((member, _)) = before.member(&id) else {
        return Err(Report::error(sentence!(
            "{} isn't on team {}.",
            Name::new("agent", agent),
            Name::new("team", team)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent team list"), "see who is"));
    };
    let member = member.clone();
    let mut table = read_team(team)?;
    if let Some(members) = table.get_mut("member").and_then(toml::Value::as_table_mut) {
        members.remove(&member);
    }
    if before.lead.as_ref() == Some(&member) {
        table.remove("lead");
    }
    write_team(team, &table)?;
    drop_grant(out, agent, team);
    // It lost its grant and every other member a teammate.
    let mut restarted = names_of(&before, &by_id());
    restarted.push(agent.to_string());
    restart(out, &restarted).await?;
    Ok(Done::success(sentence!(
        "{} is off team {}.",
        Name::new("agent", agent),
        Name::new("team", team)
    )))
}

/// Takes agent `id` off every team it's on (`lnk agent delete`): the
/// teams' names.
pub fn leave_all(out: &mut Out, id: &str) -> Vec<String> {
    let _control = match control() {
        Ok(lock) => lock,
        Err(err) => {
            out.block(
                Report::warning(sentence!("The agent's teams could not be changed.")).cause(err),
            );
            return Vec::new();
        }
    };
    let mut left = Vec::new();
    for team in settings::teams_of(id).unwrap_or_default() {
        let Some((member, _)) = team.member(id) else {
            continue;
        };
        let Ok(mut table) = read_team(&team.name) else {
            continue;
        };
        if let Some(members) = table.get_mut("member").and_then(toml::Value::as_table_mut) {
            members.remove(member);
        }
        if team.lead.as_ref() == Some(member) {
            table.remove("lead");
        }
        match write_team(&team.name, &table) {
            Ok(()) => left.push(team.name.clone()),
            Err(err) => out.block(
                Report::warning(sentence!(
                    "Team {} still lists the agent.",
                    Name::new("team", &team.name)
                ))
                .why(Sentence::adopt(&format!("{err:#}"))),
            ),
        }
    }
    left
}

/// Restarts the running members of `teams`, each once, after an agent
/// left them all (`lnk agent delete`), so each takes the change.
pub async fn restart_after_leaving(out: &mut Out, teams: &[String]) -> Result<(), Report> {
    let ids = by_id();
    let names: Vec<String> = teams
        .iter()
        .filter_map(|t| team_now(t).ok())
        .flat_map(|t| names_of(&t, &ids))
        .collect();
    restart(out, &names).await
}

/// `lnk agent team delete <team> [--yes]`.
async fn delete(out: &mut Out, team: &str, yes: bool) -> Result<Done, Report> {
    let gone = team_now(team)?;
    let shown = Name::new("team", team);
    let sure = Question::confirm(sentence!("Delete team {}?", &shown), false)
        .why(prose!(
            "Its {} members leave it; its folder stays.",
            Value::from(gone.members.len() as u64)
        ))
        .yes(yes);
    if !out.ask(sure)? {
        out.block(Note(sentence!("Nothing deleted.")));
        return Ok(Done::shown());
    }
    crate::memory::forget_team(&gone);
    defining::forget_identity(team)?;
    settings::ids::TEAMS.retire(&gone.id)?;
    std::fs::remove_file(settings::team_path(team))
        .within(format!("removing {}", settings::team_path(team).display()))?;
    let members = names_of(&gone, &by_id());
    for agent in &members {
        drop_grant(out, agent, team);
    }
    restart(out, &members).await?;
    let mut done = Done::success(sentence!("Team {} is deleted.", &shown));
    if let Some(shared) = &gone.shared {
        done = done.changed(
            "Its folder stays; delete it when you're done with it",
            Where::new(shared),
        );
    }
    Ok(done)
}

/// `lnk agent team list [--json]`.
fn list(out: &mut Out, json: bool) -> Result<Done, Report> {
    let teams = settings::teams()?;
    // Every agent's ID read once, and every member's service asked at
    // once, however many teams and members.
    let ids = by_id();
    let members: Vec<String> = teams
        .iter()
        .flat_map(|team| names_of(team, &ids))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let running: BTreeSet<String> = members
        .iter()
        .zip(service::running_each(&members))
        .filter(|(_, running)| *running)
        .map(|(name, _)| name.clone())
        .collect();
    if json {
        // One line each, and nothing else on stdout.
        for team in &teams {
            link_actions::answer::line::<link_agent_spec::declared::TeamsListed>(
                out,
                &team_json(team, &ids, &running),
            )?;
        }
        return Ok(Done::shown());
    }
    if teams.is_empty() {
        out.block(Note(sentence!("There are no teams.")));
        return Ok(Done::shown().next(
            cmd!("lnk agent team create <team> --with <a> --with <b>"),
            "make one",
        ));
    }
    for team in &teams {
        team_blocks(out, team, &ids, &running, None);
    }
    Ok(Done::shown())
}

/// `lnk agent team show <team>`: the team, or with `--member` one of its
/// members, as typed blocks (`--json`): the diagnosis of a `member`.
fn show(out: &mut Out, team: &str, member: Option<&str>) -> Result<Done, Report> {
    let teams = settings::teams()?;
    let Some(found) = teams.iter().find(|t| t.name == team) else {
        return Err(Report::error(sentence!(
            "There is no team {} here.",
            Name::new("team", team)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent team list"), "see them"));
    };
    if let Some(member) = member
        && !found.members.contains_key(member)
    {
        return Err(Report::error(sentence!(
            "{} has no member {}.",
            Name::new("team", team),
            Name::new("member", member)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent team list"), "see the members"));
    }
    let ids = by_id();
    let names = names_of(found, &ids);
    let running: BTreeSet<String> = names
        .iter()
        .zip(service::running_each(&names))
        .filter(|(_, running)| *running)
        .map(|(name, _)| name.clone())
        .collect();
    team_blocks(out, found, &ids, &running, member);
    Ok(Done::shown().next(cmd!("lnk agent team list"), "see every team"))
}

/// `team`'s members as a table (only `only`, if named) under its heading.
fn team_blocks(
    out: &mut Out,
    team: &Team,
    ids: &HashMap<String, String>,
    running: &BTreeSet<String>,
    only: Option<&str>,
) {
    let mut table = Table::new(
        ["Member", "Agent", "Purpose", "State", "Reach", "Lead"],
        sentence!("No one is on it yet."),
    );
    for (member, entry) in team
        .members
        .iter()
        .filter(|(m, _)| only.is_none_or(|o| o == m.as_str()))
    {
        let agent = ids.get(&entry.id).cloned();
        let running = agent.as_ref().is_some_and(|a| running.contains(a));
        let reach = agent
            .as_deref()
            .and_then(|a| sandbox_file::of(a).spec.teams.get(&team.name).cloned())
            .map(|grant| reach(&grant))
            .unwrap_or_else(|| "none of the folder".into());
        table.row([
            Name::new("member", member).into(),
            match &agent {
                Some(agent) if agent != member => Name::new("agent", agent).into(),
                Some(_) => Value::from(""),
                None => Value::from(format!("agent {}", entry.id)),
            },
            Value::from(entry.purpose.clone().unwrap_or_default()),
            match (&agent, running) {
                (None, _) => Member::Away.into(),
                (Some(_), true) => Member::Running.into(),
                (Some(_), false) => Member::Stopped.into(),
            },
            Value::from(reach),
            Value::from(if team.lead.as_ref() == Some(member) {
                "yes"
            } else {
                ""
            }),
        ]);
    }
    let name = Name::new("team", &team.name);
    let heading = match &team.shared {
        Some(shared) => phrase!("{}, sharing {}", &name, Where::new(shared)),
        None => phrase!("{}", &name),
    };
    out.block(Section::new(heading).with(table));
}

/// A team as `list --json` prints it, its members found in `ids`
/// ([`by_id`]), those `running` running.
fn team_json(
    team: &Team,
    ids: &HashMap<String, String>,
    running: &BTreeSet<String>,
) -> serde_json::Value {
    let members: Vec<serde_json::Value> = team
        .members
        .iter()
        .map(|(member, entry)| {
            let agent = ids.get(&entry.id).cloned();
            let grant = agent
                .as_deref()
                .and_then(|a| sandbox_file::of(a).spec.teams.get(&team.name).cloned());
            serde_json::json!({
                "name": member,
                "workload": link_agent::settings::ids::written(&entry.id),
                "agent": agent,
                "purpose": entry.purpose,
                "running": agent.as_ref().is_some_and(|a| running.contains(a)),
                "write": grant.as_ref().map(|g| g.write.clone()).unwrap_or_default(),
                "read": grant.map(|g| g.read).unwrap_or_default(),
            })
        })
        .collect();
    serde_json::json!({
        "name": team.name,
        "resource": link_agent_team::resource(&team.id),
        "lead": team.lead,
        "shared": team.shared,
        "members": members,
        "memory": crate::memory::team_tool(team),
    })
}

/// What `grant` reaches, in words.
fn reach(grant: &TeamFolders) -> String {
    let names = |names: &[String]| {
        names
            .iter()
            .map(|n| match n.as_str() {
                WHOLE => "all of it".to_string(),
                n => format!("{n}/"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (grant.write.is_empty(), grant.read.is_empty()) {
        (true, true) => "none of the folder".into(),
        (false, true) => format!("writes {}", names(&grant.write)),
        (true, false) => format!("reads {}", names(&grant.read)),
        (false, false) => format!(
            "writes {}, reads {}",
            names(&grant.write),
            names(&grant.read)
        ),
    }
}

/// Team `team` as its file says now.
fn team_now(team: &str) -> anyhow::Result<Team> {
    settings::team(team)
}

/// Every team CLI writer, including removal during agent deletion,
/// serializes on this stable sidecar rather than a replaceable document.
fn control() -> anyhow::Result<std::fs::File> {
    link_files::lock_file(&settings::teams_dir().join(constants::CONTROL_LOCK))
}

/// Team `team`'s file as a table, to change.
fn read_team(team: &str) -> anyhow::Result<toml::Table> {
    let path = settings::team_path(team);
    let text = std::fs::read_to_string(&path).within(format!("reading {}", path.display()))?;
    toml::from_str(&text).within(format!("reading {}", path.display()))
}

/// Writes team `team`'s file from `table`, once it reads as a team's.
fn write_team(team: &str, table: &toml::Table) -> anyhow::Result<()> {
    let mut table = table.clone();
    if table
        .get("member")
        .and_then(toml::Value::as_table)
        .is_some_and(toml::Table::is_empty)
    {
        table.remove("member");
    }
    let text = format!(
        "# Team {team}: its members, the folder they share, and the opinions they\n\
         # share, as an agent spec's tables. lnk agent team add and remove\n\
         # change it, and restart the members that run.\n\n{}",
        link_agent_spec::team_text(&table).within("the team's file")?
    );
    link_agent_spec::parse_team(&text).within("the team's file")?;
    let path = settings::team_path(team);
    link_files::create_private_dir(&settings::teams_dir())
        .within(format!("creating {}", settings::teams_dir().display()))?;
    link_files::write_private(&path, text.as_bytes()).within(format!("writing {}", path.display()))
}

/// The team's own folder and its shared folder, made: its own only the
/// user's, as what its tools keep is there.
fn make_shared(team: &str, shared: &Path) -> anyhow::Result<()> {
    let own = settings::team_dir(team);
    link_files::create_private_dir(&own).within(format!("creating {}", own.display()))?;
    std::fs::create_dir_all(shared).within(format!("creating {}", shared.display()))
}

/// Takes team `team`'s folders off agent `agent`'s grants.
fn drop_grant(out: &mut Out, agent: &str, team: &str) {
    let Ok(mut sandbox) = sandbox_file::load(agent) else {
        return;
    };
    if sandbox.spec.teams.remove(team).is_some()
        && let Err(err) = sandbox_file::save(agent, &sandbox)
    {
        out.block(
            Report::warning(sentence!(
                "The grants of {} still hold the team's folders.",
                Name::new("agent", agent)
            ))
            .why(Sentence::adopt(&format!("{err:#}"))),
        );
    }
}

/// Agent `name`'s settings, when it's here.
fn here(name: &str) -> Result<Settings, Report> {
    link_machine::machine::check_name(name)?;
    if !Settings::path_of(name).exists() {
        return Err(Report::error(sentence!(
            "There is no agent named {} here.",
            Name::new("agent", name)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk agent list"), "see them"));
    }
    Ok(Settings::load(name)?)
}

/// Every agent here, by its ID: each agent's settings read once, for a
/// command, not once for each member it looks for.
fn by_id() -> HashMap<String, String> {
    let mut ids = HashMap::new();
    for name in settings::names().unwrap_or_default() {
        ids.entry(settings::key_of(&name)).or_insert(name);
    }
    ids
}

/// The names of `team`'s members here, found in `ids` ([`by_id`]).
fn names_of(team: &Team, ids: &HashMap<String, String>) -> Vec<String> {
    team.members
        .values()
        .filter_map(|m| ids.get(&m.id).cloned())
        .collect()
}

/// Restarts each of `agents` that runs, once however often it's named:
/// the members whose grant, settings or teammates a team change changed,
/// so each takes it at a start of its own. A member the change didn't
/// touch keeps running.
async fn restart(out: &mut Out, agents: &[String]) -> Result<(), Report> {
    let names: Vec<String> = agents
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // One in a terminal can't be restarted from here: it takes the
    // change only when started again, so it's said.
    let foreground: Vec<&String> = names
        .iter()
        .filter(|name| !service::running(name) && service::foreground_running(name))
        .collect();
    for name in foreground {
        out.block(Report::warning(sentence!(
            "{} runs in a terminal, so it takes the team's change when you stop it and start it again.",
            Name::new("agent", name)
        )));
    }
    let running: Vec<String> = names
        .iter()
        .zip(service::running_each(&names))
        .filter(|(_, running)| *running)
        .map(|(name, _)| name.clone())
        .collect();
    if running.is_empty() {
        return Ok(());
    }
    out.block(Note(sentence!(
        "Restarting {} with it.",
        Value::List(
            running
                .iter()
                .map(|n| Name::new("agent", n).into())
                .collect()
        )
    )));
    for name in running {
        let mut agent = Settings::load(&name)?;
        crate::granting::restart_with(out, &mut agent).await?;
    }
    Ok(())
}
