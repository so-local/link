//! The teams accounts hosting agents keep, as this machine knows them:
//! what its own accounts list, and what a move brought word of for a
//! machine with no account of its own to ask (a box). A move to a machine
//! carries, from the machine running it, each such team the agent is on
//! ([`Carried`]): a read-only copy of the team's file, written to
//! `~/Link/Safe/Teams/<team>.toml` where the agent's tools look for
//! its teams, and where its relay and folder service are, kept in
//! `~/.lnk/safe/agents/places/cloud-teams.toml`. Its grants are derived from
//! that file here, as every team's are; none is taken from the stream.

use std::collections::{BTreeMap, BTreeSet};

use link_agent_place::teams::{Action, HostedTeam};
use link_format::WithinExt;
use serde::{Deserialize, Serialize};

use crate::hosted::{call, hosting};
use crate::settings;

/// A team a cloud hosting agents keeps, as the machine running a move
/// read it from that cloud: its listing and its file there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Carried {
    pub listed: HostedTeam,
    pub file: String,
}

impl Carried {
    /// `listed` and its team's file, as its account `account` answers now.
    pub fn read(account: &link_agent_place::Account, listed: &HostedTeam) -> anyhow::Result<Self> {
        let file = call(account, "agent-team", |r| {
            r.team = Some(Action::Get(link_agent_place::teams::TeamActionGet {
                team: listed.id.clone(),
            }))
        })?
        .team
        .and_then(|t| t.file)
        .within("the cloud gave no team's file")?
        .file;
        let carried = Self {
            listed: listed.clone(),
            file,
        };
        carried.team()?;
        Ok(carried)
    }

    /// Its team's file, checked as the team its listing names.
    pub fn team(&self) -> anyhow::Result<link_agent_spec::Team> {
        link_machine::machine::check_id(&self.listed.id)?;
        link_machine::machine::check_name(&self.listed.name)?;
        let team = link_agent_spec::parse_team(&self.file)?;
        anyhow::ensure!(
            team.id == self.listed.id && team.name == self.listed.name,
            "the cloud returned another team's file"
        );
        Ok(team)
    }
}

/// `~/.lnk/safe/agents/places/cloud-teams.toml`: where each team a move brought
/// word of is kept, by its ID.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Known {
    #[serde(default)]
    pub teams: BTreeMap<String, HostedTeam>,
}

link_files::specification!(Known, include_str!("hosted_teams.schema.toml"));

/// The cloud teams' file, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Known>()
}

impl Known {
    /// Its file, where its schema's `[[files]]` says, so the upgrade
    /// that converts it finds it where it's written.
    pub fn path() -> anyhow::Result<std::path::PathBuf> {
        use link_files::schema::Specification;
        let name = Self::format().files[0]
            .path
            .clone()
            .within("cloud teams' schema names its file")?;
        Ok(link_files::layout::Hidden::AgentPlaces
            .try_path()?
            .join(name))
    }

    /// What's kept; none when it can't be read.
    pub fn load() -> Self {
        Self::path()
            .ok()
            .and_then(|path| link_files::saved::read(&path).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> anyhow::Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            link_files::create_private_dir(dir)?;
        }
        link_files::saved::write(&path, self).within(format!("writing {}", path.display()))
    }
}

/// `carried`, which agent `id` arrived here with from the machine running
/// its move, kept here: each team the agent is on, its file written to
/// `~/Link/Safe/Teams/` and where it's kept recorded. A team whose name
/// here is another team's is left alone, by name in what's returned.
pub fn keep(id: &str, carried: &[Carried]) -> anyhow::Result<Vec<String>> {
    let mut known = Known::load();
    let mut left_out = Vec::new();
    for team in carried {
        let parsed = team.team()?;
        if !parsed.has(id) {
            continue;
        }
        let path = settings::team_path(&parsed.name);
        let here = path
            .exists()
            .then(|| settings::team(&parsed.name))
            .transpose()?;
        if here.is_some_and(|here| here.id != parsed.id) {
            left_out.push(parsed.name.clone());
            continue;
        }
        // Its ID taken in the teams' ledger here, as a team moved here is:
        // refused once retired, or while another team here has it.
        settings::ids::TEAMS.arrive(&parsed.name, &parsed.id)?;
        link_files::create_private_dir(&settings::teams_dir())?;
        link_files::write_private(&path, team.file.as_bytes())
            .within(format!("writing {}", path.display()))?;
        known.teams.insert(parsed.id.clone(), team.listed.clone());
    }
    if !carried.is_empty() {
        known.save()?;
    }
    Ok(left_out)
}

/// Every team the accounts hosting agents here list, and each a move
/// brought word of that none of them lists: an account that doesn't
/// answer is said in the log, and lists none.
pub fn listed() -> Vec<HostedTeam> {
    let mut teams = Vec::new();
    for account in hosting() {
        match call(&account, "agent-team", |r| {
            r.team = Some(Action::List(link_agent_place::teams::TeamActionList {}))
        }) {
            Ok(reply) => teams.extend(reply.team.and_then(|t| t.teams).unwrap_or_default()),
            Err(err) => link_format::daemon::warn!(
                "{} didn't list the teams it keeps: {err:#}",
                account.name
            ),
        }
    }
    let seen: BTreeSet<String> = teams.iter().map(|t| t.id.clone()).collect();
    teams.extend(
        Known::load()
            .teams
            .into_values()
            .filter(|team| !seen.contains(&team.id)),
    );
    teams
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file is where its schema says, so `lnk upgrade` finds it.
    #[test]
    fn the_file_is_where_its_schema_says() {
        use link_files::schema::Specification;
        let place = &Known::format().files[0];
        assert_eq!(place.location, "lnk.agent-places");
        assert!(Known::path().unwrap().ends_with("cloud-teams.toml"));
    }
}
