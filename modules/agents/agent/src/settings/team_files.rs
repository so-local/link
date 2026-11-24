//! Teams, a file each in `~/Link/Safe/Teams`: read, remembered while a
//! command runs, and what an agent reaches of their shared folders.

use super::*;
use link_format::WithinExt;

/// `~/Link/Safe/Teams`: a file per team (`<team>.toml`).
pub fn teams_dir() -> PathBuf {
    link_files::layout::Home::TeamSpecs.path()
}

/// The teams whose files list agent `id` as a member, by name. A team's
/// file that can't be read refuses: its opinions would be dropped
/// unseen.
pub fn teams_of(id: &str) -> anyhow::Result<Vec<link_agent_spec::Team>> {
    Ok(teams()?.into_iter().filter(|team| team.has(id)).collect())
}

/// An error, the whole team's move, when agent `id`, on a team kept on
/// this machine, goes `to` alone and that can't keep the team's
/// guarantees ([`link_agent_team::moving::check`]): asked before
/// anything of the move is made. `own` is its sandbox file's grants of
/// the teams' folders when it's here; none when it runs elsewhere. A
/// team whose file here is a copy of one a cloud keeps (its ID in
/// `kept_there`) is that cloud's to ask ([`crate::hosted::check_member_move`]).
pub fn check_member_move(
    id: &str,
    own: Option<&std::collections::BTreeMap<String, link_harness_contract::harness::TeamFolders>>,
    to: link_agent_team::moving::Toward,
    kept_there: &std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    use link_agent_team::moving::{Direct, Kept, Verdict, check};
    for team in teams_of(id)?
        .into_iter()
        .filter(|team| !kept_there.contains(&team.id))
    {
        let grant = own.map(|own| own.get(&team.name).cloned().unwrap_or_default());
        if let Verdict::Refuses(refusal) =
            check(&team, Kept::Machine, id, grant.as_ref(), to, &Direct)?
        {
            return Err(refusal.into());
        }
    }
    Ok(())
}

/// What agent `id`, arriving on this machine, keeps of team `name`'s
/// folder ([`crate::transfer::TeamGrant`]): derived again from the team's
/// file here, the team's home, and only for a member on its roster now.
/// An own member keeps `came` only where the machine running the move
/// vouches for it, an outside member what the roster grants it.
pub fn arriving_grant(
    id: &str,
    name: &str,
    came: &link_harness_contract::harness::TeamFolders,
    vouched: bool,
) -> Option<link_harness_contract::harness::TeamFolders> {
    use link_agent_team::moving::{Direct, Kept, Toward, Verdict, check};
    let team = teams().ok()?.into_iter().find(|team| team.name == name)?;
    let own = vouched.then_some(came);
    match check(&team, Kept::Machine, id, own, Toward::Home, &Direct).ok()? {
        Verdict::Keeps(stays) => Some(stays.folders),
        Verdict::Refuses(_) => None,
    }
}

/// Team `name`'s file (`~/Link/Safe/Teams/<name>.toml`).
pub fn team_path(name: &str) -> PathBuf {
    teams_dir().join(format!("{name}.toml"))
}

/// `~/Link/Teams/<name>`: what Link keeps for team `name`, its shared
/// folder (`shared`) by default and what its tools keep.
pub fn team_dir(name: &str) -> PathBuf {
    link_agent_contract::teams_dir().join(name)
}

/// Team `name`, from its file.
pub fn team(name: &str) -> anyhow::Result<link_agent_spec::Team> {
    link_machine::machine::check_name(name)?;
    let path = team_path(name);
    anyhow::ensure!(
        path.exists(),
        "there's no team {name}: lnk agent team create {name} makes it"
    );
    let text = read_spec_text(&path)?;
    let mut team = link_agent_spec::parse_team(&text).within(format!("in {}", path.display()))?;
    with_services(std::slice::from_mut(&mut team), &services_here());
    Ok(team)
}

/// The service each agent here acts as, by its ID.
fn services_here() -> std::collections::BTreeMap<String, String> {
    super::names()
        .unwrap_or_default()
        .iter()
        .filter_map(|name| Some((super::id_of(name)?, super::service_of(name)?)))
        .collect()
}

/// `teams` with each own member naming the service it acts as, as its
/// settings here say (`services`), over what its file says: another
/// account's, or an agent not here, names its own in the file.
fn with_services(
    teams: &mut [link_agent_spec::Team],
    services: &std::collections::BTreeMap<String, String>,
) {
    for member in teams.iter_mut().flat_map(|team| team.members.values_mut()) {
        if member.owner.is_none()
            && let Some(service) = services.get(&member.id)
        {
            member.service = Some(service.clone());
        }
    }
}

/// Every team on this machine, by name: read once while a
/// [`RememberTeams`] of this thread lives, else every time.
pub fn teams() -> anyhow::Result<Vec<link_agent_spec::Team>> {
    if let Some(teams) = REMEMBERED.with_borrow(|kept| kept.clone().flatten()) {
        return Ok(teams);
    }
    let teams = teams_in(&teams_dir())?;
    REMEMBERED.with_borrow_mut(|kept| {
        if let Some(slot) = kept {
            *slot = Some(teams.clone());
        }
    });
    Ok(teams)
}

thread_local! {
    /// The teams as [`teams`] last read them, while remembered: none when
    /// they aren't; not read yet when they are.
    static REMEMBERED: std::cell::RefCell<Option<Option<Vec<link_agent_spec::Team>>>> =
        const { std::cell::RefCell::new(None) };
    /// The teams' folders kept by accounts hosting agents, as
    /// [`hosted_folders`] last asked, while the teams are remembered.
    static HOSTED: std::cell::RefCell<Option<Vec<crate::hosted::TeamFolderAt>>> =
        const { std::cell::RefCell::new(None) };
}

/// The teams' folders the accounts hosting agents keep
/// ([`crate::hosted::team_folders`]): asked once while a [`RememberTeams`]
/// of this thread lives, else every time.
fn hosted_folders() -> Vec<crate::hosted::TeamFolderAt> {
    let remembering = REMEMBERED.with_borrow(Option::is_some);
    if remembering && let Some(kept) = HOSTED.with_borrow(Clone::clone) {
        return kept;
    }
    let asked = crate::hosted::team_folders();
    if remembering {
        HOSTED.with_borrow_mut(|kept| *kept = Some(asked.clone()));
    }
    asked
}

/// While it lives, [`teams`] reads the teams folder once on this
/// thread: for a start, whose every part (its spec, its tools, its
/// sandbox) asks for its teams, which don't change meanwhile. Never
/// for a process that runs on (a bridge), whose teams do.
pub struct RememberTeams {
    began: bool,
}

impl RememberTeams {
    pub fn begin() -> Self {
        let began = REMEMBERED.with_borrow_mut(|kept| match kept {
            Some(_) => false,
            None => {
                *kept = Some(None);
                true
            }
        });
        Self { began }
    }
}

impl Drop for RememberTeams {
    fn drop(&mut self) {
        if self.began {
            REMEMBERED.with_borrow_mut(|kept| *kept = None);
            HOSTED.with_borrow_mut(|kept| *kept = None);
        }
    }
}

/// Every team whose file is in `dir`, by name.
pub fn teams_in(dir: &Path) -> anyhow::Result<Vec<link_agent_spec::Team>> {
    teams_except_in(dir, None)
}

/// Other teams during an explicit replacement of one document. A
/// desired roster in the source still supplies no membership authority.
pub fn teams_except(name: &str) -> anyhow::Result<Vec<link_agent_spec::Team>> {
    teams_except_in(&teams_dir(), Some(name))
}

fn teams_except_in(dir: &Path, except: Option<&str>) -> anyhow::Result<Vec<link_agent_spec::Team>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).within(format!("reading {}", dir.display())),
    };
    let mut teams = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        if except.is_some_and(|name| path.file_stem().is_some_and(|stem| stem == name)) {
            continue;
        }
        let text = read_spec_text(&path)?;
        let team = link_agent_spec::parse_team(&text).within(format!("in {}", path.display()))?;
        ensure!(
            path.file_stem().is_some_and(|stem| *stem == *team.name),
            "{} names team {}: a team's file is named after it",
            path.display(),
            team.name
        );
        teams.push(team);
    }
    teams.sort_by(|a, b| a.name.cmp(&b.name));
    with_services(&mut teams, &services_here());
    Ok(teams)
}

/// What an agent reaches of its teams' shared folders, as its sandbox
/// file's `[teams]` grants them: the folders it writes, those it
/// only reads, and what every sandbox of its hides (each team's folder),
/// so it reaches nothing else of a team's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamReach {
    pub write: Vec<PathBuf>,
    pub read: Vec<PathBuf>,
    pub hide: Vec<PathBuf>,
    /// Each of its teams with a shared folder: its name, the folder, and
    /// what it's granted of it.
    pub teams: Vec<(String, PathBuf, link_harness_contract::harness::TeamFolders)>,
    /// Each of its teams whose folder an account hosting agents keeps:
    /// reached through the agent's files bridge, never as a folder here.
    pub hosted: Vec<HostedReach>,
}

/// A team whose folder an account hosting agents keeps, as an agent
/// here reaches it: through its files bridge, which hands each call to
/// the folder's service with the agent's own token
/// (`link_store_contract::files::RemoteTeam`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HostedReach {
    /// The team's name in its file here, as the agent's paths name it.
    pub name: String,
    pub id: String,
    /// The team owner's account.
    pub owner: String,
    /// Where its folder service is.
    pub files: String,
    /// What the agent is granted of it here: its sandbox file's.
    pub folders: link_harness_contract::harness::TeamFolders,
}

/// What every sandbox hides of teams': `~/Link/Teams`, and each team's
/// shared folder kept elsewhere. A team's file that can't be read hides
/// nothing of its own here, so no sandbox starts while one can't
/// ([`check_teams`]).
pub fn teams_hidden() -> Vec<PathBuf> {
    let root = link_agent_contract::teams_dir();
    let mut hide = vec![root.clone()];
    for team in teams().unwrap_or_default() {
        if let Some(shared) = team.shared
            && !shared.starts_with(&root)
            && !hide.contains(&shared)
        {
            hide.push(shared);
        }
    }
    hide
}

/// An error while a team's file can't be read: a sandbox started then
/// wouldn't hide that team's shared folder kept outside `~/Link/Teams`.
pub fn check_teams() -> anyhow::Result<()> {
    teams()
        .map(drop)
        .within("reading the teams' files, which say what every sandbox hides")
}

impl Settings {
    /// What it reaches of its teams' shared folders ([`TeamReach`]).
    pub fn team_reach(&self) -> TeamReach {
        let Some(id) = self.agent_id() else {
            return TeamReach {
                hide: teams_hidden(),
                ..Default::default()
            };
        };
        let granted = crate::sandbox_file::of(&self.name).spec.teams;
        let teams = teams().unwrap_or_default();
        // Asked only for an agent on a team: each account answers over the
        // network.
        let hosted = match teams.iter().any(|team| team.has(id)) {
            true => hosted_folders(),
            false => Vec::new(),
        };
        reach_of(id, teams, &hosted, &granted)
    }

    /// The teams it's on, as its harness is told ([`link_harness_contract::harness::TeamPlace`]).
    pub fn team_places(&self) -> anyhow::Result<Vec<link_harness_contract::harness::TeamPlace>> {
        let Some(id) = self.agent_id() else {
            return Ok(Vec::new());
        };
        let granted = crate::sandbox_file::of(&self.name).spec.teams;
        Ok(teams_of(id)?
            .into_iter()
            .filter_map(|team| {
                let (member, own) = team.member(id)?;
                Some(link_harness_contract::harness::TeamPlace {
                    name: team.name.clone(),
                    id: team.id.clone(),
                    member: member.clone(),
                    purpose: own.purpose.clone(),
                    teammates: team
                        .members
                        .iter()
                        .filter(|(_, m)| m.id != *id)
                        .map(|(name, m)| link_harness_contract::harness::Teammate {
                            name: name.clone(),
                            id: m.id.clone(),
                            purpose: m.purpose.clone(),
                        })
                        .collect(),
                    // Its folder only where it may reach some of it.
                    shared: team
                        .shared
                        .clone()
                        .filter(|_| granted.contains_key(&team.name)),
                })
            })
            .collect())
    }
}

/// What agent `id` reaches of `teams`: those whose folder an account
/// hosting agents keeps (`hosted`) through its files bridge, the others'
/// shared folders here, each as its sandbox file (`granted`) grants it,
/// and every team's folder hidden besides.
fn reach_of(
    id: &str,
    teams: Vec<link_agent_spec::Team>,
    hosted: &[crate::hosted::TeamFolderAt],
    granted: &std::collections::BTreeMap<String, link_harness_contract::harness::TeamFolders>,
) -> TeamReach {
    let mut reach = TeamReach {
        hide: teams_hidden(),
        ..Default::default()
    };
    for team in teams {
        if let Some(at) = hosted.iter().find(|at| at.id == team.id) {
            if !team.has(id) {
                continue;
            }
            if let Some(folders) = granted.get(&team.name) {
                reach.hosted.push(HostedReach {
                    name: team.name.clone(),
                    id: team.id.clone(),
                    owner: at.owner.clone(),
                    files: at.files.clone(),
                    folders: folders.clone(),
                });
            }
            continue;
        }
        let Some(shared) = team.shared.clone() else {
            continue;
        };
        let Some(folders) = granted.get(&team.name).filter(|_| team.has(id)) else {
            continue;
        };
        // Checked as a files folder is: a hand-written team's file
        // naming `~/.local/bin`, or an agent's folder, grants nothing.
        let agents = agents();
        if unsafe_files(&shared).is_some()
            || shared.starts_with(&agents)
            || agents.starts_with(&shared)
        {
            continue;
        }
        let (write, read) = folders.paths(&shared);
        reach.write.extend(write);
        reach.read.extend(read);
        reach
            .teams
            .push((team.name.clone(), shared, folders.clone()));
    }
    reach
}

#[cfg(test)]
mod reach_tests;
