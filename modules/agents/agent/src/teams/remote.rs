//! Team control through a hosting cloud's optional typed contract.
use anyhow::ensure;
use link_agent::{cli, hosted, settings};
use link_agent_place::{
    Account,
    teams::{Action, HostedTeam, Reply, TeamFile},
};
use link_format::{Done, Exit, Name, Out, Report, Table, Value, WithinExt, cmd, sentence};

mod inspecting;
pub(super) use inspecting::{delete, show};

pub(super) fn call(account: &Account, action: Action) -> anyhow::Result<Reply> {
    hosted::call(account, "agent-team", |r| r.team = Some(action))?
        .team
        .within("this cloud doesn't answer team commands")
}

pub(super) fn find(named: &str) -> anyhow::Result<(Account, HostedTeam)> {
    let (name, account) = named
        .rsplit_once('@')
        .map_or((named, None), |(n, a)| (n, Some(a)));
    let accounts = match account {
        Some(account) => {
            vec![link_agent_place::account(account).within("no such cloud account")?]
        }
        None => hosted::hosting(),
    };
    let mut found = Vec::new();
    for account in accounts {
        let reply = match call(
            &account,
            Action::List(link_agent_place::teams::TeamActionList {}),
        ) {
            Ok(reply) => reply,
            Err(e) if named.contains('@') => return Err(e),
            Err(_) => continue,
        };
        for team in reply.teams.unwrap_or_default() {
            link_machine::machine::check_id(&team.id)?;
            link_machine::machine::check_name(&team.name)?;
            if team.name == name || team.id == name {
                found.push((account.duplicate(), team));
            }
        }
    }
    ensure!(
        found.len() <= 1,
        "more than one cloud has this team: name team@account"
    );
    found
        .pop()
        .within("no team of that name here or in a cloud (lnk agent team list shows them)")
}
use link_secretive::Duplicate;

pub(super) fn read(account: &Account, team: &str) -> anyhow::Result<(TeamFile, toml::Table)> {
    let kept = call(
        account,
        Action::Get(link_agent_place::teams::TeamActionGet { team: team.into() }),
    )?
    .file
    .within("the cloud gave no team's file")?;
    let parsed = link_agent_spec::parse_team(&kept.file)?;
    ensure!(parsed.id == team, "the cloud returned another team's file");
    let table = toml::from_str(&kept.file)?;
    Ok((kept, table))
}
pub(super) fn write(
    account: &Account,
    team: &str,
    kept: &TeamFile,
    table: toml::Table,
) -> anyhow::Result<()> {
    let file = link_agent_spec::team_text(&table)?;
    link_agent_spec::parse_team(&file)?;
    call(
        account,
        Action::Put(link_agent_place::teams::TeamActionPut {
            team: team.into(),
            file,
            version: kept.version,
        }),
    )?;
    Ok(())
}

pub(super) fn apply(cloud: &str, file: &str) -> anyhow::Result<TeamFile> {
    let desired = link_agent_spec::parse_team_definition(file)?;
    let account = link_agent_place::account(cloud).within("no such cloud account")?;
    let listed = call(
        &account,
        Action::List(link_agent_place::teams::TeamActionList {}),
    )?
    .teams
    .within("the cloud gave no team inventory")?;
    let mut selected = listed
        .iter()
        .filter(|team| team.name == desired.name || desired.id.as_ref() == Some(&team.id));
    let before = selected.next();
    ensure!(
        selected.next().is_none(),
        "the document's name and ID select different teams"
    );
    let version = match before {
        Some(team) => read(&account, &team.id)?.0.version,
        None => 0,
    };
    let kept = call(
        &account,
        Action::Apply(link_agent_place::teams::TeamActionApply {
            file: file.into(),
            version,
        }),
    )?
    .file
    .within("the cloud gave no applied team document")?;
    let team = link_agent_spec::parse_team(&kept.file)?;
    ensure!(
        team.name == desired.name && desired.id.as_ref().is_none_or(|id| *id == team.id),
        "the cloud returned another team"
    );
    for (name, member) in &desired.members {
        ensure!(
            team.members
                .get(name)
                .is_some_and(|actual| member.id.as_ref().is_none_or(|id| *id == actual.id)),
            "the cloud changed a member's identity"
        );
    }
    Ok(kept)
}

pub(super) fn lifecycle(named: &str, start: bool) -> Result<Done, Report> {
    let (account, team) = find(named)?;
    call(
        &account,
        if start {
            Action::Start(link_agent_place::teams::TeamActionStart { team: team.id })
        } else {
            Action::Stop(link_agent_place::teams::TeamActionStop { team: team.id })
        },
    )?;
    Ok(Done::success(match start {
        true => sentence!(
            "Your members of team {} are started.",
            Name::new("team", &team.name)
        ),
        false => sentence!(
            "Your members of team {} are stopped; their agents and files are kept.",
            Name::new("team", &team.name)
        ),
    }))
}
fn access(table: &mut toml::Table) {
    let principals = table
        .get("member")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|members| members.values())
        .filter_map(|m| {
            m.get("owner")?;
            Some(toml::Value::String(format!(
                "service:{}",
                m.get("id")?.as_str()?
            )))
        })
        .collect();
    let mut access = toml::Table::new();
    access.insert("member".into(), toml::Value::Array(principals));
    table.insert("access".into(), toml::Value::Table(access));
}

pub(super) fn invite(_out: &mut Out, args: cli::TeamMemberAddArgs) -> Result<Done, Report> {
    let (account, team) = find(&args.team)?;
    let (owner, id) = args
        .member
        .split_once('/')
        .within("name the invited agent as <account owner>/<agent ID>")?;
    link_agent_contract::identity::check_owner(owner)?;
    link_machine::machine::check_id(id)?;
    let name = args.r#as.as_deref().unwrap_or(id);
    link_machine::machine::check_name(name)?;
    let (kept, mut table) = read(&account, &team.id)?;
    let grant = super::grant_of(&args.write, &args.read)?.unwrap_or_else(super::no_grant);
    let member = link_agent_spec::Member {
        id: id.into(),
        service: None,
        agent: None,
        owner: Some(owner.into()),
        purpose: args.purpose,
        write: grant.write,
        read: grant.read,
    };
    let members = table
        .entry("member")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .within("the members aren't a table")?;
    if members
        .values()
        .any(|m| m.get("id").and_then(toml::Value::as_str) == Some(id))
    {
        return Err(Report::error(sentence!("This agent is already invited.")).exit(Exit::Usage));
    }
    if members.contains_key(name) {
        return Err(Report::error(sentence!(
            "The team already has a member named {}.",
            Name::new("member", name)
        ))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk agent team-member add --as <name>"),
            "give the invited agent another name",
        ));
    }
    members.insert(
        name.into(),
        toml::Value::try_from(member).within("writing the member")?,
    );
    access(&mut table);
    write(&account, &team.id, &kept, table)?;
    Ok(Done::success(sentence!(
        "Invited {} to {}.",
        Name::new("agent", format!("{owner}/{id}")),
        Name::new("team", format!("{}@{}", team.name, account.name))
    ))
    .next(
        cmd!("lnk agent team join"),
        "its owner must join before it can participate",
    ))
}

pub(super) fn remove(_out: &mut Out, named: &str, agent: &str) -> Result<Done, Report> {
    let (account, team) = find(named)?;
    let (kept, mut table) = read(&account, &team.id)?;
    let members = table
        .get_mut("member")
        .and_then(toml::Value::as_table_mut)
        .within("no members")?;
    let name = members
        .iter()
        .find(|(name, m)| {
            name.as_str() == agent
                || m.get("id").and_then(toml::Value::as_str) == Some(agent)
                || m.get("owner")
                    .and_then(toml::Value::as_str)
                    .zip(m.get("id").and_then(toml::Value::as_str))
                    .is_some_and(|(o, id)| agent == format!("{o}/{id}"))
        })
        .map(|(name, _)| name.clone())
        .within("no such member")?;
    members.remove(&name);
    if table.get("lead").and_then(toml::Value::as_str) == Some(name.as_str()) {
        table.remove("lead");
    }
    access(&mut table);
    write(&account, &team.id, &kept, table)?;
    Ok(Done::success(sentence!(
        "Removed {} from {}.",
        Name::new("member", &name),
        Name::new("team", format!("{}@{}", team.name, account.name))
    )))
}

pub(super) fn joining(
    _out: &mut Out,
    team: String,
    agent: &str,
    join: bool,
) -> Result<Done, Report> {
    let (named, account) = team
        .rsplit_once('@')
        .within("name <team owner>/<team>@<cloud account>")?;
    let (owner, name) = named
        .split_once('/')
        .within("name <team owner>/<team>@<cloud account>")?;
    link_agent_contract::identity::check_owner(owner)?;
    let account = link_agent_place::account(account).within("no such cloud account")?;
    let agents = link_agent_place::hosted_agents(&account, None)?
        .within("this cloud doesn't host agents")?;
    let agent = agents
        .iter()
        .find(|a| a.id == agent || a.name == agent)
        .within("no such hosted agent")?;
    let invitations = call(
        &account,
        Action::Invites(link_agent_place::teams::TeamActionInvites {
            agent: agent.id.clone(),
        }),
    )?
    .invites
    .within("no invitations answer")?;
    let invitation = invitations
        .into_iter()
        .find(|i| i.owner == owner && (i.name == name || i.team == name))
        .within("this team hasn't invited this agent")?;
    let action = if join {
        Action::Join(link_agent_place::teams::TeamActionJoin {
            owner: owner.into(),
            team: invitation.team,
            agent: agent.id.clone(),
        })
    } else {
        Action::Leave(link_agent_place::teams::TeamActionLeave {
            owner: owner.into(),
            team: invitation.team,
            agent: agent.id.clone(),
        })
    };
    call(&account, action)?;
    let who = Name::new("agent", &agent.name);
    let team = Name::new("team", &team);
    Ok(Done::success(match join {
        true => sentence!("{} joined the team {}.", &who, &team),
        false => sentence!("{} left the team {}.", &who, &team),
    }))
}

/// Each team on a cloud account that hosts agents, by its shared folder
/// here, owned by its group (`group:<its ID>`), for `lnk storage`
/// ([`link_storage_contract::FOLDERS`]); an account that doesn't answer,
/// or a team that isn't plain, says none.
pub(crate) fn hosted_teams() -> Vec<link_storage_contract::Owned> {
    let mut teams = Vec::new();
    for account in hosted::hosting() {
        let Ok(reply) = call(
            &account,
            Action::List(link_agent_place::teams::TeamActionList {}),
        ) else {
            continue;
        };
        for team in reply.teams.unwrap_or_default() {
            let Ok(owner) = link_agent::owners::group(&team.id) else {
                continue;
            };
            if link_machine::machine::check_name(&team.name).is_ok() {
                teams.push(link_storage_contract::Owned {
                    owner,
                    root: shared(&team.name),
                    name: team.name,
                    running: None,
                    account: Some(account.name.clone()),
                });
            }
        }
    }
    teams
}

/// Hosted team `name`'s shared folder here.
fn shared(name: &str) -> std::path::PathBuf {
    link_files::home_dir()
        .join("Link/Teams")
        .join(name)
        .join("shared")
}

pub(super) fn listed(out: &mut Out, json: bool) -> Result<(), Report> {
    let mut hosted = Table::new(["Team", "Members"], sentence!("No hosted teams."));
    for account in hosted::hosting() {
        let Ok(reply) = call(
            &account,
            Action::List(link_agent_place::teams::TeamActionList {}),
        ) else {
            continue;
        };
        for team in reply.teams.unwrap_or_default() {
            link_machine::machine::check_id(&team.id)?;
            link_machine::machine::check_name(&team.name)?;
            if json {
                link_actions::answer::line::<link_agent_spec::declared::TeamsHostedListed>(
                    out,
                    &serde_json::json!({"name":team.name,"resource":link_agent_team::resource(&team.id),"account":account.name,"members":team.members,"shared":shared(&team.name)}),
                )?;
            }
            hosted.row([
                Name::new("team", format!("{}@{}", team.name, account.name)).into(),
                Value::from(team.members.len() as u64),
            ]);
        }
    }
    if !hosted.is_empty() {
        out.block(hosted);
    }
    Ok(())
}

pub(super) fn is_remote(team: &str) -> bool {
    team.contains('@') || !settings::team_path(team).exists()
}

pub(super) fn snapshot(_out: &mut Out, team: &str, restore: bool) -> Result<Done, Report> {
    let (account, team) = find(team)?;
    let action = if restore {
        Action::Restore(link_agent_place::teams::TeamActionRestore { team: team.id })
    } else {
        Action::Backup(link_agent_place::teams::TeamActionBackup { team: team.id })
    };
    let at = call(&account, action)?
        .at
        .within("the cloud gave no snapshot time")?;
    let team = Name::new("team", &team.name);
    Ok(Done::success(match restore {
        true => sentence!(
            "Team {} is restored from the backup at {}.",
            &team,
            Value::from(at.to_string())
        ),
        false => sentence!(
            "Team {} is backed up, at {}.",
            &team,
            Value::from(at.to_string())
        ),
    }))
}
