//! The bridge's own side: the Homes it reaches ([`Homes`]), and each
//! proved call answered within them ([`serve`]).

use super::*;
use link_format::sentence;
#[cfg(test)]
mod approval_tests;

/// What an agent's calls reach: its Home's objects, how large its Home
/// and index may be together (none: no cap), and the folders of its
/// Home it may write (`.` for all of it).
#[cfg(feature = "files-bridge")]
pub struct Home {
    pub store: Arc<dyn Store>,
    pub cap: Option<u64>,
    pub write: Vec<String>,
}

/// A team's folder as a call of a member's reaches it: its objects
/// (Home's layout: files under [`HOME`]), the team's ID and its owner's,
/// and what of it the member may write and only read: its role on the
/// team ([`team_role`]). Its size is held by its store (the team owner's
/// storage), not counted here.
#[cfg(feature = "files-bridge")]
pub struct TeamFolder {
    pub store: Arc<dyn Store>,
    /// The team's ID: its folder is the bucket `team-<id>`.
    pub id: String,
    /// The team's owner, `acct-<id>` or a machine's own owner.
    pub owner: String,
    pub folders: crate::harness::TeamFolders,
    /// Whether the member may keep the team's memory's index, as whoever
    /// pays for it says: then, writing all of the folder, it writes the
    /// index; else it only reads it.
    pub keeps_index: bool,
}

/// A member's role on the team `id`'s folder, from what of it the team's
/// file grants it: `bucket.read` and `bucket.write` in each folder it
/// writes, `bucket.read` in each it only reads, `.` the whole folder.
/// `owner` names the team's account where it isn't the member's own.
#[cfg(feature = "files-bridge")]
pub fn team_role(
    id: &str,
    owner: Option<&link_policy::AccountId>,
    folders: &crate::harness::TeamFolders,
) -> anyhow::Result<link_policy::Policy> {
    let account = owner.map(|a| format!("{a}/")).unwrap_or_default();
    let on = |name: &String| match name.as_str() {
        crate::harness::WHOLE => format!("{account}{}*", crate::harness::team_resource(id, None)),
        name => format!(
            "{account}{}*",
            crate::harness::team_resource(id, Some(name))
        ),
    };
    let grant = |actions: &[&str], names: &[String]| {
        let on = names
            .iter()
            .map(|name| {
                on(name)
                    .parse::<link_policy::Resources>()
                    .map_err(anyhow::Error::msg)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let actions = actions
            .iter()
            .map(|a| {
                a.parse::<link_policy::Actions>()
                    .map_err(anyhow::Error::msg)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        anyhow::Ok(link_policy::Grant::new(&actions, &on))
    };
    let mut grants = Vec::new();
    if !folders.written().is_empty() {
        grants.push(grant(&["bucket.read", "bucket.write"], folders.written())?);
    }
    if !folders.only_read().is_empty() {
        grants.push(grant(&["bucket.read"], folders.only_read())?);
    }
    let role = link_policy::Policy {
        name: format!("team-{}-folder", id.to_ascii_lowercase()),
        version: link_policy::VERSION,
        allow: grants,
        limits: None,
    };
    role.check()?;
    Ok(role)
}

/// What a member's call reaches of a team's folder, as `allows` checks
/// it: the member, holding its role on the team, and the folder `path`
/// is in as a resource, `bucket:team-<id>/<folder>/`, or
/// `bucket:team-<id>/` for a file at its top. Another account's team
/// lets its member in by its `[access]`, as the team's file does.
#[cfg(feature = "files-bridge")]
struct InTeam {
    member: link_policy::Principal,
    owner: Option<link_policy::AccountId>,
    id: String,
}

#[cfg(feature = "files-bridge")]
impl InTeam {
    fn new(who: &Identity, folder: &TeamFolder) -> anyhow::Result<InTeam> {
        Self::of(who, &folder.id, &folder.owner, &folder.folders)
    }

    /// `who` holding its role on team `id`, owned by `owner`, as `folders`
    /// grant it.
    fn of(
        who: &Identity,
        id: &str,
        owner: &str,
        folders: &crate::harness::TeamFolders,
    ) -> anyhow::Result<InTeam> {
        let owner = (owner != who.owner).then(|| link_policy::AccountId::of_owner(owner));
        let role = team_role(id, owner.as_ref(), folders)?;
        let member = link_sandbox_bridge::grants::principal(who, [role])?;
        Ok(InTeam {
            member,
            owner,
            id: id.to_string(),
        })
    }

    /// Whether the member may `verb` (`read`, `write`) `path`.
    fn may(&self, verb: &str, path: &str) -> bool {
        let top = path.split_once('/').map(|(top, _)| top);
        let folder = top.filter(|top| crate::harness::TeamFolders::is_folder_name(top));
        let account = self
            .owner
            .as_ref()
            .map(|a| format!("{a}/"))
            .unwrap_or_default();
        let name = format!(
            "{account}{}",
            crate::harness::team_resource(&self.id, folder)
        );
        let Some(action) = link_policy::Action::of(link_policy::Kind::Bucket, verb) else {
            return false;
        };
        let mut access = link_policy::Access::default();
        if self.owner.is_some() {
            for verb in ["read", "write"] {
                let member = link_policy::Who::Principal(self.member.id().clone());
                access.0.insert(verb.into(), vec![member]);
            }
        }
        link_sandbox_bridge::grants::may(&self.member, action, &name, access)
    }
}

/// Where a bridge finds the Home of the agent a call is proved as: never
/// another's. And what it's told once a file of a Home changed.
#[cfg(feature = "files-bridge")]
pub trait Homes: Send + Sync {
    /// The Home of `who`, an error for one it doesn't serve.
    fn home(&self, who: &Identity) -> anyhow::Result<Home>;
    /// Existing allowed writes retain their policy. A source explicitly enables reviewed paths.
    fn reviews_writes(&self, _who: &Identity, _team: Option<&TeamRef>, _path: &str) -> bool {
        false
    }
    /// Source policy decides applicability. Waiting holds neither quota nor storage locks.
    fn review_write(
        &self,
        _who: &Identity,
        _write: &PreparedWrite,
    ) -> anyhow::Result<Option<link_interactions::sources::ReviewedEffect>> {
        Ok(None)
    }
    /// The folder of the team `team` names, as `who` may reach it now:
    /// asked at every call naming it, so what was true at bind is never
    /// trusted. None, or an error, for a team it isn't on, or none of
    /// that name. Left out, no team is served.
    fn team(&self, _who: &Identity, _team: &TeamRef) -> anyhow::Result<Option<TeamFolder>> {
        Ok(None)
    }
    /// The folder service that answers the team `team` names, for a team
    /// whose folder this bridge neither holds nor checks: each call is
    /// handed on as `who`'s own, and the service checks it there
    /// ([`RemoteTeam`]). Asked at every call naming a team, before
    /// [`Homes::team`]; none, this bridge reaches the folder itself.
    fn remote_team(
        &self,
        _who: &Identity,
        _team: &TeamRef,
    ) -> anyhow::Result<Option<Arc<RemoteTeam>>> {
        Ok(None)
    }
    /// `path` of `who`'s Home was written (memory reads `memory/` again).
    fn changed(&self, _who: &Identity, _path: &str) {}
    /// `path` of the folder of the team `team` names was written by
    /// `who`: what keeps the team's memory current is told.
    fn team_changed(&self, _who: &Identity, _team: &TeamRef, _path: &str) {}
}

/// A folder of Home's, as `list` takes one: empty for all of it, else a
/// path ending `/`.
#[cfg(feature = "files-bridge")]
fn check_folder(folder: &str) -> anyhow::Result<()> {
    if folder.is_empty() {
        return Ok(());
    }
    ensure!(
        folder.ends_with('/'),
        "{:?} isn't a folder: end it with /",
        link_format::clean(folder)
    );
    check_path(folder.trim_end_matches('/'))
}

/// An index object's name: `<engine>/<file>`, letters, digits, `.`, `-`
/// and `_`.
#[cfg(feature = "files-bridge")]
fn check_index_name(name: &str) -> anyhow::Result<()> {
    let parts: Vec<&str> = name.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|p| {
                !p.is_empty()
                    && !p.starts_with('.')
                    && p.len() <= 128
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            }),
        "{:?} isn't an object of the index (<engine>/<file>)",
        link_format::clean(name)
    );
    Ok(())
}

/// A bridge serving Homes from what `homes` gives each agent proved by
/// `keys`, on `listener`, for good, on the server every bridge runs on
/// ([`crate::bridge::serve`], within [`LIMITS`]): at most [`CONNECTIONS`]
/// calls at once and [`AGENT_CONNECTIONS`] of one agent's.
#[cfg(feature = "files-bridge")]
pub fn serve(listener: UnixListener, keys: Arc<dyn Keys>, homes: Arc<dyn Homes>) {
    let stop = std::sync::atomic::AtomicBool::new(false);
    serve_until(listener, keys, homes, &stop);
}

/// [`serve`], until `stop` is set: for a bridge that ends with the
/// process serving it, as an agent's own bridge does.
#[cfg(feature = "files-bridge")]
pub fn serve_until(
    listener: UnixListener,
    keys: Arc<dyn Keys>,
    homes: Arc<dyn Homes>,
    stop: &std::sync::atomic::AtomicBool,
) {
    serve_bridge_until(listener, Arc::new(Bridge::new(keys, homes)), &LIMITS, stop);
}

#[cfg(all(test, feature = "files-bridge"))]
pub(super) fn serve_bridge(
    listener: UnixListener,
    bridge: Arc<Bridge>,
    limits: &crate::bridge::Limits,
) {
    let stop = std::sync::atomic::AtomicBool::new(false);
    serve_bridge_until(listener, bridge, limits, &stop);
}

#[cfg(feature = "files-bridge")]
fn serve_bridge_until(
    listener: UnixListener,
    bridge: Arc<Bridge>,
    limits: &crate::bridge::Limits,
    stop: &std::sync::atomic::AtomicBool,
) {
    let keys = bridge.keys.clone();
    let served = crate::bridge::serve(listener, keys, limits, stop, move |who, action: Action| {
        bridge
            .act(who, action)
            .map_err(|err| match is_conflict(&err) {
                true => Failure::new(Why::Conflict, format!("{err:#}")).into(),
                false => err,
            })
    });
    if let Err(err) = served {
        crate::warned(sentence!("The files bridge stopped."), &err);
    }
}

/// An agent, as the bridge keeps count of it: its ID and owner.
#[cfg(feature = "files-bridge")]
type Agent = (String, String);

/// What an agent's Home and index hold, and the key (its SHA-256) it
/// was bound with when they were counted: none until counted.
#[cfg(feature = "files-bridge")]
type Count = Option<(String, u64)>;

/// A files bridge's state.
#[cfg(feature = "files-bridge")]
pub(super) struct Bridge {
    keys: Arc<dyn Keys>,
    pub(super) homes: Arc<dyn Homes>,
    /// Each agent's count, locked from its check against the cap to its
    /// write. Counted again once it's bound anew, so what another bridge
    /// wrote meanwhile is in it.
    used: Mutex<BTreeMap<Agent, Arc<Mutex<Count>>>>,
}

#[cfg(feature = "files-bridge")]
impl Bridge {
    pub(super) fn new(keys: Arc<dyn Keys>, homes: Arc<dyn Homes>) -> Self {
        Self {
            keys,
            homes,
            used: Mutex::new(BTreeMap::new()),
        }
    }

    /// `who`'s count, to lock.
    fn count(&self, who: &Identity) -> Arc<Mutex<Count>> {
        self.used
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry((who.agent.clone(), who.owner.clone()))
            .or_default()
            .clone()
    }

    /// The folder of team `team` as `who` reaches it now: refused for one
    /// it isn't on.
    fn team(&self, who: &Identity, team: &TeamRef) -> anyhow::Result<TeamFolder> {
        self.homes
            .team(who, team)?
            .ok_or_else(|| anyhow::anyhow!("you aren't on a team {team} that has a folder"))
    }

    /// A call naming a team: its folder's, as its grants let it, each
    /// checked by [`team_allows`] before anything of the folder is reached.
    fn act_in_team(
        &self,
        who: &Identity,
        team: &TeamRef,
        action: Action,
    ) -> anyhow::Result<Answer> {
        let (folder, member) = self.admit(who, team, &action)?;
        self.serve_in_team(who, team, folder, member, action)
    }

    /// The folder of `team` as `who` reaches it now, and `who` holding
    /// its role on it, once [`team_allows`] lets `action` in: an error,
    /// nothing of the folder reached, for a team it isn't on or a call
    /// its grants don't let it make.
    fn admit(
        &self,
        who: &Identity,
        team: &TeamRef,
        action: &Action,
    ) -> anyhow::Result<(TeamFolder, InTeam)> {
        let folder = self.team(who, team)?;
        let member = InTeam::new(who, &folder)?;
        team_allows(&folder.folders, folder.keeps_index, &member, action)
            .with_context(|| format!("team {team}"))?;
        Ok((folder, member))
    }

    /// `action`, let in by [`Bridge::admit`], answered from `folder`.
    fn serve_in_team(
        &self,
        who: &Identity,
        team: &TeamRef,
        folder: TeamFolder,
        member: InTeam,
        action: Action,
    ) -> anyhow::Result<Answer> {
        match action {
            Action::Read { path, .. } => get_from(&*folder.store, &format!("{HOME}{path}"), &path),
            Action::Write {
                path, data, expect, ..
            } => {
                let data = decode(&data, WRITE_MAX, &path)?;
                if !self.homes.reviews_writes(who, Some(team), &path) {
                    let version = folder.store.put(&format!("{HOME}{path}"), &data, &expect)?;
                    self.homes.team_changed(who, team, &path);
                    return Ok(Answer::Written { version });
                }
                let prepared = super::approvals::freeze(
                    &*folder.store,
                    who,
                    Some(team),
                    &path,
                    &data,
                    &expect,
                )?;
                let before = self.keys.key_of(who);
                let reviewed = self
                    .homes
                    .review_write(who, &prepared)?
                    .context("source requires a reviewed write but supplied no authorization")?;
                let _fence = reviewed.dispatch_fence()?;
                let folder = self.team(who, team)?;
                ensure!(
                    InTeam::new(who, &folder)?.may("write", &path),
                    "team grant changed during review"
                );
                ensure!(
                    before.is_some() && before == self.keys.key_of(who),
                    "source binding changed during review"
                );
                let application = Some(prepared.claim(reviewed)?);
                let result = folder
                    .store
                    .put(&format!("{HOME}{path}"), &data, &prepared.expect);
                super::approvals::report(application, &result)?;
                let version = result?;
                self.homes.team_changed(who, team, &path);
                Ok(Answer::Written { version })
            }
            Action::List { folder: under, .. } => {
                let files = folder
                    .store
                    .list(&format!("{HOME}{under}"))?
                    .into_iter()
                    .filter_map(|o| {
                        let path = o.path.strip_prefix(HOME)?.to_string();
                        (check_path(&path).is_ok() && member.may("read", &path))
                            .then_some(Object { path, ..o })
                    })
                    .collect();
                Ok(Answer::Listed { files })
            }
            index => index_of(&*folder.store, index),
        }
    }

    fn act(&self, who: &Identity, action: Action) -> anyhow::Result<Answer> {
        if let Some(team) = action.team() {
            let team = team.clone();
            if let Some(remote) = self.homes.remote_team(who, &team)? {
                // What it's granted here narrows what it asks there: the
                // service checks the call again, as the member it proves.
                if let Some(folders) = remote.grant() {
                    let member = InTeam::of(who, remote.team(), remote.owner(), folders)?;
                    team_allows(folders, false, &member, &action)
                        .with_context(|| format!("team {team}"))?;
                }
                return remote.act(action);
            }
            return self.act_in_team(who, &team, action);
        }
        let home = self.homes.home(who)?;
        match action {
            Action::Read { path, .. } => {
                check_path(&path)?;
                get(&home, &format!("{HOME}{path}"), &path)
            }
            Action::IndexGet { name, .. } => {
                check_index_name(&name)?;
                get(&home, &format!("{INDEX}{name}"), &name)
            }
            Action::List { folder, .. } => {
                check_folder(&folder)?;
                let files = home
                    .store
                    .list(&format!("{HOME}{folder}"))?
                    .into_iter()
                    .filter_map(|o| {
                        let path = o.path.strip_prefix(HOME)?.to_string();
                        check_path(&path).is_ok().then_some(Object { path, ..o })
                    })
                    .collect();
                Ok(Answer::Listed { files })
            }
            Action::Write {
                path, data, expect, ..
            } => {
                check_path(&path)?;
                ensure!(
                    writes(&home.write, &path),
                    "you may write only in {}: {path} isn't",
                    granted(&home.write)
                );
                let data = decode(&data, WRITE_MAX, &path)?;
                if !self.homes.reviews_writes(who, None, &path) {
                    let version = self.put(who, &home, &format!("{HOME}{path}"), &data, &expect)?;
                    self.homes.changed(who, &path);
                    return Ok(Answer::Written { version });
                }
                let prepared =
                    super::approvals::freeze(&*home.store, who, None, &path, &data, &expect)?;
                let before = self.keys.key_of(who);
                let reviewed = self
                    .homes
                    .review_write(who, &prepared)?
                    .context("source requires a reviewed write but supplied no authorization")?;
                let _fence = reviewed.dispatch_fence()?;
                let home = self.homes.home(who)?;
                ensure!(
                    writes(&home.write, &path),
                    "Home grant changed during review"
                );
                ensure!(
                    before.is_some() && before == self.keys.key_of(who),
                    "source binding changed during review"
                );
                let application = Some(prepared.claim(reviewed)?);
                let result = self.put(
                    who,
                    &home,
                    &format!("{HOME}{path}"),
                    &data,
                    &prepared.expect,
                );
                super::approvals::report(application, &result)?;
                let version = result?;
                self.homes.changed(who, &path);
                Ok(Answer::Written { version })
            }
            Action::IndexPut {
                name, data, expect, ..
            } => {
                check_index_name(&name)?;
                let data = decode(&data, READ_MAX, &name)?;
                let version = self.put(who, &home, &format!("{INDEX}{name}"), &data, &expect)?;
                Ok(Answer::Written { version })
            }
            Action::IndexList { engine, .. } => {
                check_index_name(&format!("{engine}/x"))?;
                let prefix = format!("{INDEX}{engine}/");
                let files = home
                    .store
                    .list(&prefix)?
                    .into_iter()
                    .filter_map(|o| {
                        let path = o.path.strip_prefix(INDEX)?.to_string();
                        check_index_name(&path)
                            .is_ok()
                            .then_some(Object { path, ..o })
                    })
                    .collect();
                Ok(Answer::Listed { files })
            }
            Action::IndexDelete { name, expect, .. } => {
                check_index_name(&name)?;
                let name = format!("{INDEX}{name}");
                let count = self.count(who);
                let mut count = count.lock().unwrap_or_else(|e| e.into_inner());
                // Counted again at the next write.
                *count = None;
                home.store.delete(&name, &expect)?;
                Ok(Answer::Deleted { deleted: true })
            }
        }
    }

    /// Writes `name`, within the cap: one write of an agent's at a time,
    /// so two can't both pass the cap.
    pub(super) fn put(
        &self,
        who: &Identity,
        home: &Home,
        name: &str,
        data: &[u8],
        expect: &Expect,
    ) -> anyhow::Result<Version> {
        let Some(cap) = home.cap else {
            return home.store.put(name, data, expect);
        };
        let count = self.count(who);
        let mut count = count.lock().unwrap_or_else(|e| e.into_inner());
        let bound = self
            .keys
            .key_of(who)
            .map(|key| sha256(key.expose().as_bytes()));
        let used = match &*count {
            Some((key, used)) if Some(key) == bound.as_ref() => *used,
            _ => {
                *count = None;
                let objects = home.store.list(HOME)?.into_iter();
                objects.chain(home.store.list(INDEX)?).map(|o| o.size).sum()
            }
        };
        let before = home
            .store
            .list(name)?
            .into_iter()
            .find(|o| o.path == name)
            .map_or(0, |o| o.size);
        let after = used - before.min(used) + data.len() as u64;
        if after > cap && data.len() as u64 > before {
            bail!(
                "Home is full: it holds {} of its {}, and this needs {} more. Remove files \
                 from Home to make room.",
                size(used),
                size(cap),
                size(after - cap)
            );
        }
        let version = home.store.put(name, data, expect)?;
        *count = bound.map(|key| (key, after));
        Ok(version)
    }
}

/// A team's folder answered for members a service proves itself, never
/// with this bridge's keyring: a folder service on the network that
/// verified each call's token and says whose it is ([`Identity`]), then
/// asks [`Homes::team`] for the folder as that member reaches it now. The
/// same checks as the bridge's own ([`team_allows`], with the member's
/// role on the team): only the identity's source differs. A write its
/// source reviews is refused: a review needs an agent bound here.
#[cfg(feature = "files-bridge")]
pub struct TeamCalls {
    bridge: Bridge,
}

#[cfg(feature = "files-bridge")]
impl TeamCalls {
    /// Answering from what `homes` gives each member at each call.
    pub fn new(homes: Arc<dyn Homes>) -> Self {
        Self {
            bridge: Bridge::new(Arc::new(NoKeys), homes),
        }
    }

    /// `action` of `who`, on the folder of `team` whatever team `action`
    /// names: refused ([`Why::Refused`]) when `who` isn't on it now or its
    /// grants don't let it, a [`Why::Conflict`] for a write from a version
    /// that isn't the file's, else what it asked, or its failure.
    pub fn act(&self, who: &Identity, team: &TeamRef, action: Action) -> Result<Answer, Failure> {
        let action = action.in_team(Some(team.clone()));
        let refused = |err: anyhow::Error| Failure::new(Why::Refused, format!("{err:#}"));
        if let Action::Write { path, .. } = &action
            && self.bridge.homes.reviews_writes(who, Some(team), path)
        {
            return Err(refused(anyhow::anyhow!(
                "a reviewed write to team {team} is made by an agent bound to its bridge"
            )));
        }
        let (folder, member) = self.bridge.admit(who, team, &action).map_err(refused)?;
        self.bridge
            .serve_in_team(who, team, folder, member, action)
            .map_err(|err| match is_conflict(&err) {
                true => Failure::new(Why::Conflict, format!("{err:#}")),
                false => Failure::new(Why::Failed, format!("{err:#}")),
            })
    }
}

/// No agent bound: what [`TeamCalls`] proves its callers with.
#[cfg(feature = "files-bridge")]
struct NoKeys;

#[cfg(feature = "files-bridge")]
impl Keys for NoKeys {
    fn key_of(&self, _: &Identity) -> Option<link_secretive::Secret> {
        None
    }
}

/// The most a team's index holds beside its folder, in bytes: a copy at
/// its largest and the next written beside it before the old one's
/// segments go.
pub const TEAM_INDEX_MAX: u64 = 2 * index::COPY_MAX;

pub fn team_index_capacity() -> Capacity {
    Capacity {
        prefix: INDEX.into(),
        bytes: TEAM_INDEX_MAX,
    }
}

/// The one check of a call on a team's folder, against the member's
/// grant, before anything of the folder is reached: a read, a folder it
/// reads or writes; a write, one it writes; a listing, a folder's shape
/// (what's listed is what it reads); the index, which tells what's
/// anywhere in the folder, read by a member granted all of it and
/// written only by one writing all of it that may keep it
/// ([`TeamFolder::keeps_index`]), and only as the index's own objects
/// ([`check_team_index_name`]).
#[cfg(feature = "files-bridge")]
fn team_allows(
    folders: &crate::harness::TeamFolders,
    keeps_index: bool,
    member: &InTeam,
    action: &Action,
) -> anyhow::Result<()> {
    match action {
        Action::Read { path, .. } => {
            check_path(path)?;
            ensure!(
                member.may("read", path),
                "you may read only {}: {path} isn't",
                granted_in_team(folders, false)
            );
        }
        Action::Write { path, .. } => {
            check_path(path)?;
            ensure!(
                member.may("write", path),
                "you may write only in {}: {path} isn't",
                granted_in_team(folders, true)
            );
        }
        Action::List { folder, .. } => check_folder(folder)?,
        Action::IndexGet { name, .. } => {
            check_team_index_name(name)?;
            ensure!(
                folders.whole(),
                "its memory's index is read only by a member granted all of its \
                 folder; you're granted {}",
                granted_in_team(folders, false)
            );
        }
        Action::IndexList { engine, .. } => {
            check_index_name(&format!("{engine}/x"))?;
            ensure!(
                folders.whole(),
                "its memory's index is read only by a member granted all of its \
                 folder; you're granted {}",
                granted_in_team(folders, false)
            );
        }
        Action::IndexPut { name, .. } | Action::IndexDelete { name, .. } => {
            check_team_index_name(name)?;
            ensure!(
                folders.writes_whole(),
                "its memory's index is written only by a member writing all of \
                 its folder; you write {}",
                granted_in_team(folders, true)
            );
            ensure!(
                keeps_index,
                "its memory's index is kept by a member its owner's account pays for"
            );
        }
    }
    Ok(())
}

/// An object of a team's index, as memory writes one: `<engine>/manifest.json`,
/// or a segment, `<engine>/seg-<sha256>`; never another name.
#[cfg(feature = "files-bridge")]
fn check_team_index_name(name: &str) -> anyhow::Result<()> {
    check_index_name(name)?;
    let file = name.split_once('/').map_or("", |(_, file)| file);
    let segment = file
        .strip_prefix("seg-")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
    ensure!(
        file == index::MANIFEST || segment,
        "{:?} isn't an object of a team's index (<engine>/manifest.json or a segment)",
        link_format::clean(name)
    );
    Ok(())
}

/// An index call answered from `store`, a team's folder's, checked by
/// [`team_allows`]: held by the team owner's storage, within
/// [`TEAM_INDEX_MAX`].
#[cfg(feature = "files-bridge")]
pub(super) fn index_of(store: &dyn Store, action: Action) -> anyhow::Result<Answer> {
    match action {
        Action::IndexGet { name, .. } => get_from(store, &format!("{INDEX}{name}"), &name),
        Action::IndexPut {
            name, data, expect, ..
        } => {
            let data = decode(&data, READ_MAX, &name)?;
            let name = format!("{INDEX}{name}");
            let version = store.put_bounded(&name, &data, &expect, &team_index_capacity())?;
            Ok(Answer::Written { version })
        }
        Action::IndexList { engine, .. } => {
            let files = store
                .list(&format!("{INDEX}{engine}/"))?
                .into_iter()
                .filter_map(|o| {
                    let path = o.path.strip_prefix(INDEX)?.to_string();
                    check_index_name(&path)
                        .is_ok()
                        .then_some(Object { path, ..o })
                })
                .collect();
            Ok(Answer::Listed { files })
        }
        Action::IndexDelete { name, expect, .. } => {
            store.delete_bounded(&format!("{INDEX}{name}"), &expect, &team_index_capacity())?;
            Ok(Answer::Deleted { deleted: true })
        }
        _ => bail!("not a call of the index"),
    }
}

pub(super) fn get(home: &Home, name: &str, shown: &str) -> anyhow::Result<Answer> {
    get_from(&*home.store, name, shown)
}

#[cfg(feature = "files-bridge")]
fn get_from(store: &dyn Store, name: &str, shown: &str) -> anyhow::Result<Answer> {
    // Its own size, never its folder's listing: a read costs the same in
    // a folder of five files as of five thousand.
    if let Some(size) = store.size(name)? {
        ensure!(
            size <= READ_MAX,
            "{shown} is over {} MiB, larger than the bridge reads",
            READ_MAX >> 20
        );
    }
    Ok(match store.get(name)? {
        Some((data, version)) => Answer::Got {
            data: base64::engine::general_purpose::STANDARD.encode(data),
            version,
        },
        None => Answer::Missing { missing: true },
    })
}

#[cfg(feature = "files-bridge")]
pub(super) fn decode(data: &str, max: u64, shown: &str) -> anyhow::Result<Vec<u8>> {
    ensure!(
        data.len() as u64 <= max / 3 * 4 + 4,
        "{shown} is over {} MiB, larger than a file written",
        max >> 20
    );
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .with_context(|| format!("{shown}'s contents aren't base64"))
}

/// Whether the folders `write` grants hold `path`.
#[cfg(feature = "files-bridge")]
pub(super) fn writes(write: &[String], path: &str) -> bool {
    let top = path.split('/').next().unwrap_or("");
    path.contains('/')
        && write
            .iter()
            .any(|w| w == top || (w == "." && top != crate::harness::layout::CONVERSATIONS))
}

#[cfg(feature = "files-bridge")]
fn granted_in_team(folders: &crate::harness::TeamFolders, write: bool) -> String {
    let names: Vec<&String> = match write {
        true => folders.written().iter().collect(),
        false => folders
            .written()
            .iter()
            .chain(folders.only_read())
            .collect(),
    };
    match names.is_empty() {
        true => "none of its folder".into(),
        false => names
            .iter()
            .map(|w| match w.as_str() {
                crate::harness::WHOLE => "all of its folder".to_string(),
                w => format!("{w}/"),
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(feature = "files-bridge")]
fn granted(write: &[String]) -> String {
    match write.is_empty() {
        true => "none of Home".into(),
        false => write
            .iter()
            .map(|w| match w.as_str() {
                "." => "all of Home".to_string(),
                w => format!("{w}/"),
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}
