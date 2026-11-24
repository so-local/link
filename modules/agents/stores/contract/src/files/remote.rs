//! A team's folder answered by a folder service on the network: what a
//! bridge on a member's machine hands a call naming a team its folder
//! isn't on that machine ([`RemoteTeam`], behind [`Homes::remote_team`]).
//!
//! The member's own token goes with each call, made outside the sandbox
//! (`link_core::owner::TOKEN`, [`RemoteTeam::for_agent`])
//! and kept by the bridge, never by the agent. The service checks the
//! token, who it names and the team's roster at that call, and answers
//! from the team's folder as that member reaches it now: no folder secret
//! or storage credential ever reaches the member's machine.

#[cfg(feature = "files-bridge")]
use super::*;

/// The route of team `team`'s folder, owned by account `owner`, on its
/// folder service ([`crate::constants::TEAM_FILES_ROUTE`]): `POST` the
/// call, an [`Action`] naming no team, as JSON with the member's token
/// (`Authorization: Bearer`); an [`Answer`] back, or a refusal with its
/// status and a [`Failure`] saying why (409 for a conflict).
pub fn team_route(owner: &str, team: &str) -> String {
    crate::constants::TEAM_FILES_ROUTE
        .replace("{owner}", owner)
        .replace("{team}", team)
}

/// What a member's token for team `team`'s folder service grants
/// (`team.member@team:<id>`).
pub fn member_grants(team: &str) -> String {
    format!("{}{team}", crate::constants::TEAM_MEMBER_GRANT)
}

/// Where a member's token comes from, at each call: made and kept
/// outside the sandbox.
#[cfg(feature = "files-bridge")]
pub type TokenSource = Box<dyn Fn() -> anyhow::Result<link_secretive::Secret> + Send + Sync>;

/// Team `team`'s folder (account `owner`'s), as its folder service at
/// `url` answers a member's calls, with the member's own token.
#[cfg(feature = "files-bridge")]
pub struct RemoteTeam {
    url: String,
    owner: String,
    team: String,
    token: TokenSource,
    client: link_fetch::blocking::Client,
    /// What the member is granted of the folder where its bridge runs:
    /// checked there before a call goes ([`RemoteTeam::with_grant`]).
    grant: Option<crate::harness::TeamFolders>,
}

#[cfg(feature = "files-bridge")]
impl RemoteTeam {
    /// The folder of `owner`'s team `team` (its ID) at the folder service
    /// whose URL is `url` (before `/v1`), each call sent with what
    /// `token` gives then.
    pub fn new(url: &str, owner: &str, team: &str, token: TokenSource) -> anyhow::Result<Self> {
        crate::identity::check_owner(owner)?;
        ensure!(
            owner.starts_with("acct-") && !crate::identity::is_local(owner),
            "{:?} isn't an account's ID",
            link_format::clean(owner)
        );
        crate::machine::check_id(team).context("a team's ID")?;
        host_of(url)?;
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            owner: owner.into(),
            team: team.into(),
            token,
            client: link_fetch::blocking::Client::with_timeout(
                crate::constants::TEAM_FILES_TIMEOUT,
            )?,
            grant: None,
        })
    }

    /// The same, each call first held by the bridge to what `folders`
    /// grant the member where it runs (its sandbox file's, for the team
    /// owner's own agent): the service checks it again as the member its
    /// token names.
    pub fn with_grant(mut self, folders: crate::harness::TeamFolders) -> Self {
        self.grant = Some(folders);
        self
    }

    /// What it's granted where its bridge runs: none, the service's word
    /// alone.
    pub fn grant(&self) -> Option<&crate::harness::TeamFolders> {
        self.grant.as_ref()
    }

    /// The team's ID.
    pub fn team(&self) -> &str {
        &self.team
    }

    /// The team owner's account.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The same, with agent `agent`'s own token for the service's host,
    /// granted membership of the team ([`member_grants`]), as its machine
    /// signs it (`link_core::owner::TOKEN`): kept in this process, never
    /// in a file and never handed into the sandbox, until a minute before
    /// it ends.
    pub fn for_agent(url: &str, owner: &str, team: &str, agent: &str) -> anyhow::Result<Self> {
        let (host, grants, agent) = (host_of(url)?, member_grants(team), agent.to_string());
        let kept: std::sync::Mutex<Option<(link_secretive::Secret, u64)>> = Default::default();
        let token = move || {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if let Some((token, ends)) = kept.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                && *ends > now + crate::constants::TEAM_TOKEN_MARGIN_SECS
            {
                return Ok(token.duplicate());
            }
            // Made with the lock let go, so a slow token signer holds up
            // no call that finds a kept one; two at once may each make one,
            // the later kept.
            let (token, ends) = installation_signed(&agent, &host, &grants)?;
            *kept.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((token.duplicate(), ends.unwrap_or(now)));
            Ok(token)
        };
        Self::new(url, owner, team, Box::new(token))
    }

    /// `action` on the team's folder, whatever team it names: the
    /// service's answer, a [`Conflict`] for a write from a version that
    /// isn't the file's, or its refusal, saying why.
    pub fn act(&self, action: Action) -> anyhow::Result<Answer> {
        let token = (self.token)().context("making the member's token")?;
        let url = format!("{}{}", self.url, team_route(&self.owner, &self.team));
        let sent = self
            .client
            .post(&url)
            .bearer(&token)
            .json(&action.in_team(None))
            .limit(ANSWER_MAX)
            .answer::<Answer>();
        sent.map_err(|err| {
            let failure = err.refusal().and_then(|r| r.read::<Failure>().ok());
            match (err.status(), failure) {
                (Some(409), Some(failure)) => Conflict {
                    name: link_format::clean(&failure.error),
                }
                .into(),
                (_, Some(failure)) => anyhow::anyhow!(
                    "the team's folder service refused: {}",
                    link_format::clean(&failure.error)
                ),
                (_, None) => anyhow::Error::from(err).context("the team's folder service"),
            }
        })
    }
}

/// Whether agent `agent` is on the team named `name` where its bridge
/// runs, asked at each call.
#[cfg(feature = "files-bridge")]
pub type OnTeam = Box<dyn Fn(&str) -> bool + Send + Sync>;

/// One agent's teams whose folders their services answer, by the names it
/// gives them (`research`): what a files bridge on the agent's own machine
/// serves, where its Home is a folder of that machine's and never served.
/// Each call is that agent's alone, and reaches a team only while `on`
/// says it's on it at that call; any other agent's call, or a team
/// named as another account's, reaches none.
#[cfg(feature = "files-bridge")]
pub struct RemoteTeams {
    who: Identity,
    teams: BTreeMap<String, Arc<RemoteTeam>>,
    on: OnTeam,
}

#[cfg(feature = "files-bridge")]
impl RemoteTeams {
    pub fn new(who: Identity, teams: BTreeMap<String, RemoteTeam>, on: OnTeam) -> Self {
        Self {
            who,
            teams: teams.into_iter().map(|(n, t)| (n, Arc::new(t))).collect(),
            on,
        }
    }
}

#[cfg(feature = "files-bridge")]
impl Homes for RemoteTeams {
    fn home(&self, _who: &Identity) -> anyhow::Result<Home> {
        bail!("this bridge serves an agent's teams' folders alone: its Home is a folder here")
    }

    fn remote_team(
        &self,
        who: &Identity,
        team: &TeamRef,
    ) -> anyhow::Result<Option<Arc<RemoteTeam>>> {
        ensure!(
            who.agent == self.who.agent && who.owner == self.who.owner,
            "this is agent {}'s files bridge: agent {}'s calls aren't taken",
            link_format::clean(&self.who.agent),
            link_format::clean(&who.agent)
        );
        if team.owner.is_some() || !(self.on)(&team.name) {
            return Ok(None);
        }
        Ok(self.teams.get(&team.name).cloned())
    }
}

/// The host of `url`, an `https://` URL (or `http://` on this machine),
/// as a token is made for it.
#[cfg(feature = "files-bridge")]
fn host_of(url: &str) -> anyhow::Result<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .with_context(|| format!("{:?} isn't an https:// URL", link_format::clean(url)))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    ensure!(
        !host.is_empty() && !authority.contains('@'),
        "{:?} names no host",
        link_format::clean(url)
    );
    Ok(host.to_ascii_lowercase())
}

/// Agent `agent`'s own token for `host`, granted `grants`, as its machine
/// signs it, and when it ends.
#[cfg(feature = "files-bridge")]
fn installation_signed(
    agent: &str,
    host: &str,
    grants: &str,
) -> anyhow::Result<(link_secretive::Secret, Option<u64>)> {
    let made = link_core::owner::signed_token(&link_core::owner::TokenAsk {
        audience: host.into(),
        workload: Some(agent.into()),
        grants: Some(grants.into()),
    })
    .map_err(|err| {
        anyhow::anyhow!(
            "its machine made it no token for {}: {err:#}",
            link_format::clean(host)
        )
    })?;
    Ok((made.token, made.expires_at))
}
