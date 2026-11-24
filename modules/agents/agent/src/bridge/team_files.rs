//! The files bridge an agent's bridge runs for its teams whose folders
//! an account hosting agents keeps ([`TeamFilesPlan`]).

use super::*;
use link_store_contract::files::{RemoteTeam, RemoteTeams};

/// The files bridge's socket, in the agent's own settings folder, which
/// no sandbox writes: its tools, outside the sandbox, reach it there.
pub const FILES_SOCKET: &str = "team-files.sock";

/// The files bridge an agent's bridge runs: its socket, and each of the
/// agent's teams whose folder an account hosting agents keeps
/// ([`settings::TeamReach::hosted`]). A call naming one is handed to the
/// folder's service with the agent's own token, made and kept here,
/// outside every sandbox, never in it: the service checks the call again
/// as the agent its token names, at each call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TeamFilesPlan {
    pub socket: PathBuf,
    pub teams: Vec<settings::HostedReach>,
}

impl TeamFilesPlan {
    /// Agent `agent`'s, with the teams `reach` keeps elsewhere: none for
    /// an agent with none.
    pub fn of(agent: &Settings, reach: &settings::TeamReach) -> Option<Self> {
        (!reach.hosted.is_empty()).then(|| Self {
            socket: Self::socket_of(agent),
            teams: reach.hosted.clone(),
        })
    }

    /// Where agent `agent`'s files bridge listens.
    pub fn socket_of(agent: &Settings) -> PathBuf {
        settings::config_of(&agent.name).join(FILES_SOCKET)
    }

    /// What the bridge serves as agent `who`: each team as `open` reaches
    /// its service, held first to what the agent is granted here, and
    /// only while the team's file here still lists the agent.
    pub fn homes(
        &self,
        who: link_sandbox_binding::Identity,
        open: impl Fn(&settings::HostedReach) -> anyhow::Result<RemoteTeam>,
    ) -> anyhow::Result<RemoteTeams> {
        let mut teams = std::collections::BTreeMap::new();
        for team in &self.teams {
            let remote = open(team)?.with_grant(team.folders.clone());
            teams.insert(team.name.clone(), remote);
        }
        let ids: std::collections::BTreeMap<String, String> = self
            .teams
            .iter()
            .map(|t| (t.name.clone(), t.id.clone()))
            .collect();
        let agent = who.agent.clone();
        // The team's file here at each call: an agent taken off the team
        // reaches its folder no more, whatever the service would say.
        let on = move |name: &str| {
            settings::team(name)
                .is_ok_and(|team| ids.get(name) == Some(&team.id) && team.has(&agent))
        };
        Ok(RemoteTeams::new(who, teams, Box::new(on)))
    }
}

/// Runs agent `agent`'s files bridge as `files` says, until `stop`: each
/// call proved as this agent's, by its key in this machine's keyring,
/// and each team's call handed to its service with the agent's own token
/// ([`RemoteTeam::for_agent`]); any other agent's refused.
pub(super) fn serve_team_files(
    agent: &Settings,
    files: &TeamFilesPlan,
    stop: Arc<AtomicBool>,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    let me = agent.identity();
    let id = me.agent.clone();
    let homes = files.homes(me, |team| {
        RemoteTeam::for_agent(&team.files, &team.owner, &team.id, &id)
    })?;
    // In a folder of the agent's settings, which no sandbox writes: what's
    // there first (a stale socket) is replaced, never followed.
    let listener = link_sandbox_bridge::socket::listen_private(&files.socket)?;
    let keys: Arc<dyn link_sandbox_binding::Keys> = Arc::new(link_sandbox_binding::Keyring::here());
    let homes = Arc::new(homes);
    Ok(std::thread::spawn(move || {
        link_store_contract::files::serve_until(listener, keys, homes, &stop)
    }))
}
