//! Cloud accounts that run agents: an account whose adapter answers
//! the `agent-*` commands of `link_agent_place` runs agents itself,
//! with no box. `lnk agent move <account>` moves one there, `lnk agent
//! list` finds those it hosts (`agent-list`), and `lnk storage` reaches
//! their Home through it. What an adapter answers is untrusted: every
//! name, ID and time is checked before it's shown or used.

use link_format::WithinExt;
use std::process::Stdio;

use anyhow::bail;
use link_agent_place::{self as cloud, Account, HostedAgent, Refused, Reply, Request};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

/// Your cloud accounts (`lnk cloud list --json`), by their names; none
/// when that can't say.
pub fn accounts() -> Vec<String> {
    #[derive(Deserialize)]
    struct Listed {
        name: String,
    }
    link_core::lnk()
        .args(["cloud", "list", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Vec<Listed>>(&o.stdout).ok())
        .map_or_else(Vec::new, |a| a.into_iter().map(|a| a.name).collect())
}

/// Whether `account` runs agents: its adapter answers `agent-runs`
/// with `runs_agents: true`. One with no such command, or that fails,
/// runs none.
pub fn runs_agents(account: &Account) -> bool {
    let request = Request {
        account: account.duplicate(),
        ..Request::default()
    };
    cloud::call_quietly(&account.kind, "agent-runs", &request)
        .is_ok_and(|reply| reply.runs_agents == Some(true))
}

/// Each of your cloud accounts that runs agents, as each answered
/// `agent-runs` once ([`RunsAgents`]), so an account that doesn't (aws,
/// gcp) is asked once, never again.
pub fn hosting() -> Vec<Account> {
    let mut kept = RunsAgents::load();
    let accounts: Vec<Account> = accounts()
        .iter()
        .filter_map(|name| cloud::account(name))
        .collect();
    let learned = accounts
        .iter()
        .fold(false, |learned, account| kept.learn(account) | learned);
    if learned {
        let _ = kept.save();
    }
    accounts
        .into_iter()
        .filter(|account| kept.of(account))
        .collect()
}

/// `~/.lnk/safe/agents/places/runs-agents.toml`: whether each account
/// runs agents, as its adapter answered `agent-runs` once
/// ([`runs_agents`]), by name, with the kind and identity it was asked
/// as: an account connected again as another is asked again.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunsAgents {
    #[serde(default)]
    pub accounts: std::collections::BTreeMap<String, RunsAgentsAnswer>,
}

/// What an account answered `agent-runs`, and as which account.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunsAgentsAnswer {
    pub kind: String,
    pub identity: String,
    pub runs_agents: bool,
}

link_files::specification!(RunsAgents, include_str!("runs_agents.schema.toml"));

/// The agents run elsewhere, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<RunsAgents>()
}

impl RunsAgents {
    /// Its file, where its schema's `[[files]]` says, so the upgrade
    /// that converts it finds it where it's written.
    pub fn path() -> anyhow::Result<std::path::PathBuf> {
        use link_files::schema::Specification;
        let name = Self::format().files[0]
            .path
            .clone()
            .within("the accounts' answers' schema names its file")?;
        Ok(link_files::layout::Hidden::AgentPlaces
            .try_path()?
            .join(name))
    }

    /// What's kept; none when it can't be read, so each is asked again.
    pub fn load() -> Self {
        Self::path()
            .ok()
            .and_then(|path| link_files::saved::read(&path).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            link_files::create_private_dir(dir)?;
        }
        link_files::saved::write(&path, self).within(format!("writing {}", path.display()))
    }

    /// Whether `account` runs agents: as kept for it, else asked of its
    /// adapter now and kept. True when it changed what's kept.
    pub fn learn(&mut self, account: &Account) -> bool {
        if self
            .accounts
            .get(&account.name)
            .is_some_and(|kept| kept.is(account))
        {
            return false;
        }
        let answer = RunsAgentsAnswer {
            kind: account.kind.clone(),
            identity: identity(account).to_string(),
            runs_agents: runs_agents(account),
        };
        self.accounts.insert(account.name.clone(), answer);
        true
    }

    pub fn of(&self, account: &Account) -> bool {
        self.accounts
            .get(&account.name)
            .is_some_and(|kept| kept.is(account) && kept.runs_agents)
    }
}

impl RunsAgentsAnswer {
    /// Whether it was asked of `account` as it's connected now.
    fn is(&self, account: &Account) -> bool {
        self.kind == account.kind && self.identity == identity(account)
    }
}

/// Who `account` signs in as: its identity, else its login.
fn identity(account: &Account) -> &str {
    match account.identity.is_empty() {
        true => &account.login,
        false => &account.identity,
    }
}

/// Account `account`'s adapter command `command`, with what `more` puts
/// in the request: its reply, what it said on stderr kept for the error.
pub fn call(
    account: &Account,
    command: &str,
    more: impl FnOnce(&mut Request),
) -> anyhow::Result<Reply> {
    let mut request = Request {
        account: account.duplicate(),
        ..Request::default()
    };
    more(&mut request);
    cloud::call_quietly(&account.kind, command, &request)
}

/// What [`check_member_move`] found of the teams accounts hosting agents
/// keep.
#[derive(Debug, Default)]
pub struct MemberTeams {
    /// Every such team, by ID: a copy of one's file here isn't a team kept
    /// here.
    pub kept_there: std::collections::BTreeSet<String>,
    /// Each of them the agent is on that keeps it where it goes, a
    /// machine, to carry there ([`crate::hosted_teams::keep`]).
    pub carried: Vec<crate::hosted_teams::Carried>,
}

/// An error, the whole team's move, when agent `id`, a member of a team
/// an account hosting agents keeps, goes alone where that team's
/// guarantees can't hold ([`link_agent_team::moving::check`]): `to` says
/// where it goes from each account. The ways back are the ones its
/// `agent-team list` says reach each team from a machine (its relay, its
/// folder service), the roster is its team's file there now, and `own`
/// is the agent's sandbox file's grants of the teams' folders when it's
/// here. A team a move brought word of here, the agent on it, that no
/// account here lists now while one doesn't answer, refuses too: the
/// cloud that keeps it can't be asked, unless the agent goes to it. An
/// account that answers no team commands keeps no team of its.
pub fn check_member_move(
    id: &str,
    own: Option<&std::collections::BTreeMap<String, link_harness_contract::harness::TeamFolders>>,
    to: impl Fn(&Account) -> link_agent_team::moving::Toward,
) -> anyhow::Result<MemberTeams> {
    use link_agent_place::teams::Action;
    use link_agent_team::moving::{Kept, Toward, Verdict, Ways, check, keeps_every};
    let mut found = MemberTeams::default();
    let mut unanswered = Vec::new();
    for account in hosting() {
        let to = to(&account);
        let reply = match call(&account, "agent-team", |r| {
            r.team = Some(Action::List(link_agent_place::teams::TeamActionList {}))
        }) {
            Ok(reply) => reply,
            Err(err) => {
                if !keeps_every(to) {
                    unanswered.push((account.name.clone(), err));
                }
                continue;
            }
        };
        for listed in reply.team.and_then(|t| t.teams).unwrap_or_default() {
            link_machine::machine::check_id(&listed.id)?;
            found.kept_there.insert(listed.id.clone());
            if keeps_every(to) || !listed.members.iter().any(|member| member == id) {
                continue;
            }
            let mut ways = Ways::default();
            if listed.relay.is_some() {
                ways.conversation.insert(listed.id.clone());
            }
            if listed.files.is_some() {
                ways.folder.insert(listed.id.clone());
            }
            let carried = crate::hosted_teams::Carried::read(&account, &listed)?;
            let team = carried.team()?;
            let grant = own.map(|own| own.get(&team.name).cloned().unwrap_or_default());
            if let Verdict::Refuses(refusal) =
                check(&team, Kept::Hosted, id, grant.as_ref(), to, &ways)?
            {
                return Err(refusal.into());
            }
            if to == Toward::Machine {
                found.carried.push(carried);
            }
        }
    }
    for (team, known) in crate::hosted_teams::Known::load().teams {
        if found.kept_there.contains(&team) || !known.members.iter().any(|m| m == id) {
            found.kept_there.insert(team);
            continue;
        }
        found.kept_there.insert(team);
        if let Some((account, err)) = unanswered.first() {
            anyhow::bail!(
                "this agent is on team {}, which a cloud hosting agents keeps, and {account} \
                 didn't answer whether it keeps the team's guarantees there ({err:#}): try again \
                 once it answers",
                known.name
            );
        }
    }
    Ok(found)
}

/// Each of your accounts that hosts agents, with what `agent-list`
/// says of `owner`'s there (`None`: an answer without them), or why it
/// didn't answer.
pub fn listed(owner: Option<&str>) -> Vec<(Account, anyhow::Result<Option<Vec<HostedAgent>>>)> {
    hosting()
        .into_iter()
        .map(|account| {
            let agents = cloud::hosted_agents(&account, owner);
            (account, agents)
        })
        .collect()
}

/// A team whose folder an account hosting agents keeps, as its
/// `agent-team list` says: the team's ID, its owner's account, and where
/// its folder service is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamFolderAt {
    pub id: String,
    pub owner: String,
    pub files: String,
}

/// Every team the accounts hosting agents keep a folder of and name a
/// folder service for, each checked: as they list them, else as a move
/// brought word of them ([`crate::hosted_teams::listed`]). An account
/// that doesn't answer is said in the log and keeps none, so its teams'
/// folders are reached as folders here until it answers again.
pub fn team_folders() -> Vec<TeamFolderAt> {
    crate::hosted_teams::listed()
        .into_iter()
        .filter_map(|team| {
            let (owner, files) = (team.owner?, team.files?);
            let checked = link_machine::machine::check_id(&team.id).is_ok()
                && link_agent_contract::identity::check_owner(&owner).is_ok()
                && owner.starts_with("acct-");
            checked.then_some(TeamFolderAt {
                id: team.id,
                owner,
                files,
            })
        })
        .collect()
}

/// A move's ID from an adapter, checked.
pub fn move_of(reply: &Reply, command: &str) -> anyhow::Result<String> {
    let id = reply
        .r#move
        .clone()
        .within(format!("{command} named no move"))?;
    link_agent_contract::moving::check_move_id(&id)?;
    Ok(id)
}

/// A refusal as it's shown: why, as the cloud says it (cleaned), and
/// when the next move is allowed, as this machine's time ("the next move
/// is allowed at 14:05").
pub fn refused_text(account: &str, refused: &Refused) -> String {
    let why = link_format::clean(refused.why.trim());
    let why = match why.is_empty() {
        true => "no reason given".to_string(),
        false => why.chars().take(500).collect(),
    };
    let when = match refused.retry_at {
        Some(at) => format!(": the next move is allowed at {}", local_time(at)),
        None => String::new(),
    };
    format!(
        "{} refused the move ({why}){when}",
        link_format::clean(account)
    )
}

/// Unix seconds `at` as this machine's time of day, `14:05`, with its
/// date when it isn't today (`14:05 on 2026-10-12`).
pub fn local_time(at: u64) -> String {
    let (day, clock) = local(at);
    let (today, _) = local(crate::moved::now());
    match day == today {
        true => clock,
        false => format!("{clock} on {day}"),
    }
}

/// `YYYY-MM-DD` and `HH:MM` of Unix seconds `at`, this machine's time.
fn local(at: u64) -> (String, String) {
    let Ok(at) = libc::time_t::try_from(at) else {
        return ("a far date".into(), "--:--".into());
    };
    // SAFETY: `tm` is written by localtime_r, which reads only `at`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&at, &mut tm).is_null() };
    match ok {
        true => (
            format!(
                "{:04}-{:02}-{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday
            ),
            format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
        ),
        false => ("a far date".into(), "--:--".into()),
    }
}

/// `lnk agent backup` (or, with `restore`, `lnk agent restore`) of agent
/// `note.name`, hosted by the account its note names: the host backs it
/// up where it runs, never stopping it, or restores its last backup.
pub fn backup(note: &crate::moved::Note, restore: bool) -> anyhow::Result<link_format::Done> {
    let name = &note.name;
    let place = note.place();
    let account = cloud::account(&note.to.name).within(format!(
        "{name} runs on {place}, which isn't a cloud account here: lnk cloud list"
    ))?;
    let id = note
        .id
        .clone()
        .within(format!("the note of where {name} went has no ID"))?;
    let command = match restore {
        true => "agent-restore",
        false => "agent-backup",
    };
    let reply = call(&account, command, |r| {
        r.agent = Some(HostedAgent {
            id,
            name: name.clone(),
            ..HostedAgent::default()
        });
    })
    .map_err(|err| match err.is::<cloud::NoCommand>() {
        true => anyhow::anyhow!(
            "{place} keeps no backups of the agents it hosts: lnk agent move here --agent \
             {name} brings {name} back here"
        ),
        false => err,
    })?;
    let when = reply
        .at
        .map(local_time)
        .unwrap_or_else(|| "now".to_string());
    use link_format::{Done, Foreign, Name, sentence};
    let agent = Name::new("agent", name);
    Ok(match restore {
        true => Done::success(sentence!(
            "Restored {} on {} to its backup of {}.",
            &agent,
            Foreign::new(&place),
            Foreign::new(&when)
        )),
        false => Done::success(sentence!(
            "Backed up {} on {}, at {}.",
            &agent,
            Foreign::new(&place),
            Foreign::new(&when)
        )),
    })
}

/// Fails with `account`'s refusal, if `reply` is one.
pub fn unless_refused(account: &str, reply: &Reply) -> anyhow::Result<()> {
    if let Some(refused) = &reply.refused {
        bail!("{}", refused_text(account, refused));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file is where its schema says, so `lnk upgrade` finds it.
    #[test]
    fn the_answers_file_is_where_its_schema_says() {
        use link_files::schema::Specification;
        let place = &RunsAgents::format().files[0];
        assert_eq!(place.location, "lnk.agent-places");
        assert!(RunsAgents::path().unwrap().ends_with("runs-agents.toml"));
    }

    #[test]
    fn which_accounts_run_agents_is_kept_with_its_schema() {
        let mut kept = RunsAgents::default();
        kept.accounts.insert(
            "host".into(),
            RunsAgentsAnswer {
                kind: "link".into(),
                identity: "acct".into(),
                runs_agents: true,
            },
        );
        let text = link_files::saved::print(&kept).unwrap();
        assert!(text.starts_with("schema = 1\n"), "{text}");
        assert_eq!(link_files::saved::parse::<RunsAgents>(&text).unwrap(), kept);
    }

    #[test]
    fn a_refusal_says_why_and_when() {
        let refused = Refused {
            why: "no more moves today\u{1b}[2J".into(),
            retry_at: Some(crate::moved::now() + 60),
        };
        let text = refused_text("host", &refused);
        assert!(
            text.starts_with("host refused the move (no more moves today"),
            "{text}"
        );
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(text.contains(": the next move is allowed at "), "{text}");
        let (_, clock) = local(crate::moved::now() + 60);
        assert!(
            text.ends_with(&clock) || text.contains(&format!("{clock} on ")),
            "{text}"
        );
        let far = local_time(crate::moved::now() + 3 * 86400);
        assert!(far.contains(" on "), "{far}");
        let none = refused_text(
            "host",
            &Refused {
                why: String::new(),
                retry_at: None,
            },
        );
        assert_eq!(none, "host refused the move (no reason given)");
    }
}
