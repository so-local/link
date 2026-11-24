//! What a machine keeps of an agent that moved away (`lnk agent move`):
//! a note of where it went (`~/.lnk/safe/agents/transfers/moved/<id>.toml`), which
//! `start` refuses to run a second copy against, and its files for as
//! long as `agents.toml`'s `keep_moved` says, 30 days by default
//! (`~/.lnk/data/agents/move/<name>-<date>`). Its settings, state and keys
//! are deleted once it answers where it went.

use link_format::WithinExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use link_agent_identity::place::Place;
use serde::{Deserialize, Serialize};

use crate::service;
use crate::settings::{self, Settings};
pub mod models;
mod reading;
pub use reading::Read;
#[cfg(test)]
use reading::notes_in;
use reading::read_in;

/// How many days the files an agent left behind are kept: none, until
/// you delete them (`keep_moved = "never"`, or a `agents.toml` that
/// can't be read, since this deletes).
pub fn keep_days() -> Option<u64> {
    crate::machine_settings::keep_moved().unwrap_or(None)
}

/// Where an agent went from this machine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// None only for an agent on a box that didn't answer when this
    /// machine first ran a Link with notes (its ID is the box's to say).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The service it acts as, its ID, which its secrets are given to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    pub name: String,
    pub to: Place,
    /// When it left, in Unix seconds.
    pub at: u64,
    /// What its sandbox file here granted it of its teams' shared folders
    /// when it left, by the team's name: what this machine vouches for
    /// again when it comes back, never what the machine it comes from
    /// says.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub teams: std::collections::BTreeMap<String, link_harness_contract::harness::TeamFolders>,
}

link_files::specification!(Note, include_str!("moved/note.schema.toml"));

/// `~/.lnk/safe/agents/transfers/moved`: a note per agent that left this machine.
pub fn dir() -> PathBuf {
    link_files::layout::Hidden::Moved.path()
}

impl Note {
    /// A note that agent `id`, `name` left for machine `to`, now.
    pub fn new(id: Option<String>, name: &str, to: Place) -> Self {
        Self {
            id,
            service: None,
            name: name.to_string(),
            to,
            at: now(),
            teams: Default::default(),
        }
    }

    /// Its file: `<id>.toml`, or `<name>.toml` while its ID isn't known.
    pub fn path(&self) -> PathBuf {
        dir().join(format!("{}.toml", self.id.as_deref().unwrap_or(&self.name)))
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = self.path();
        link_files::create_private_dir(&dir())?;
        link_files::write_private(&path, link_files::saved::print(self)?.as_bytes())
            .within(format!("writing {}", path.display()))
    }

    /// Removes it (the agent came back, or is known to be elsewhere).
    pub fn remove(&self) {
        let _ = std::fs::remove_file(self.path());
    }

    /// Its ID, learned from the machine it went to: the note under its
    /// name gives way to one under its ID.
    pub fn learn_id(&mut self, id: &str) -> anyhow::Result<()> {
        link_machine::machine::check_id(id)?;
        self.remove();
        self.id = Some(id.to_string());
        self.save()
    }

    /// Where it went, for the user: "your box gcp-1 on 2026-09-27".
    pub fn place(&self) -> String {
        match self.to.cloud {
            true => format!(
                "your cloud account {} since {}",
                self.to.name,
                date(self.at)
            ),
            false => format!("your box {} since {}", self.to.name, date(self.at)),
        }
    }
}

/// Every note here, oldest first. A note this `lnk` can't read, as one
/// a newer release wrote, is refused, naming its file and the upgrade:
/// never taken for no note, which would lose where an agent went.
pub fn notes() -> anyhow::Result<Vec<Note>> {
    let read = read_in(&dir())?;
    match read.unreadable.into_iter().next() {
        Some(err) => Err(err),
        None => Ok(read.notes),
    }
}

/// Every note here, oldest first, as a list reads them: each note it
/// can't read said apart, by its file, never hiding the rest.
pub fn listed() -> anyhow::Result<Read> {
    read_in(&dir())
}

/// The note of agent `id`, if it left this machine.
pub fn of_id(id: &str) -> anyhow::Result<Option<Note>> {
    Ok(notes()?.into_iter().find(|n| n.id.as_deref() == Some(id)))
}

/// The latest note of an agent named `name`.
pub fn of_name(name: &str) -> anyhow::Result<Option<Note>> {
    Ok(notes()?.into_iter().rev().find(|n| n.name == name))
}

/// `~/.lnk/data/agents/move`: the files agents left behind when they
/// moved, a folder each, apart from `~/Link/Archive`, which is the
/// archive's.
pub fn files_dir() -> PathBuf {
    link_files::layout::Hidden::AgentMove.path()
}

/// Where agent `name`'s folder is put aside today:
/// `~/.lnk/data/agents/move/<name>-<date>` (`-2`, `-3`, ... when that's
/// taken), where it's kept [`keep_days`].
fn aside(name: &str) -> anyhow::Result<PathBuf> {
    aside_in(&files_dir(), name)
}

fn aside_in(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    link_files::create_private_dir(root)?;
    let base = format!("{name}-{}", date(now()));
    let mut to = root.join(&base);
    let mut n = 2;
    while to.symlink_metadata().is_ok() {
        to = root.join(format!("{base}-{n}"));
        n += 1;
    }
    Ok(to)
}

/// Moves agent `name`'s folder `dir` whole to where it's put aside
/// ([`aside`]); returns where.
pub fn keep_files(name: &str, dir: &Path) -> anyhow::Result<PathBuf> {
    let to = aside(name)?;
    std::fs::rename(dir, &to).within(format!("moving {} to {}", dir.display(), to.display()))?;
    Ok(to)
}

/// Puts aside `folder`'s own folder, and its Home where `home` (it's the
/// agent's, not one you chose), each as it's named in an agent's folder
/// (`home`, `agent`), in one folder ([`aside`]); returns where, or none
/// when neither is there.
fn keep_folder(
    name: &str,
    folder: &link_agent_contract::folder::Folder,
    home: bool,
) -> anyhow::Result<Option<PathBuf>> {
    let parts: Vec<(&Path, &str)> = [
        (
            folder.home.as_path(),
            link_agent_contract::folder::HOME,
            home,
        ),
        (
            folder.agent.as_path(),
            link_agent_contract::folder::AGENT,
            true,
        ),
    ]
    .into_iter()
    .filter(|(path, _, keep)| *keep && path.exists())
    .map(|(path, as_named, _)| (path, as_named))
    .collect();
    if parts.is_empty() {
        return Ok(None);
    }
    let to = aside(name)?;
    link_files::create_private_dir(&to)?;
    for (path, as_named) in parts {
        std::fs::rename(path, to.join(as_named)).within(format!(
            "moving {} to {}",
            path.display(),
            to.display()
        ))?;
    }
    Ok(Some(to))
}

/// The folder agent `note` left here, if it's kept: as [`clear`] put it
/// aside the day it left (the latest of that day's), its Home and its
/// own folder in it.
pub fn kept_files(note: &Note) -> Option<link_agent_contract::folder::Folder> {
    let base = format!("{}-{}", note.name, date(note.at));
    let kept = std::fs::read_dir(files_dir())
        .ok()?
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name == base
                || name
                    .strip_prefix(&format!("{base}-"))
                    .is_some_and(|n| n.parse::<u32>().is_ok())
        })
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?
        .path();
    Some(link_agent_contract::folder::Folder::new(&kept, None))
}

/// Deletes the keys each harness home in `homes` keeps: a `.env` at its
/// top or in a folder at its top (`.heron/.env`), never through a link.
fn strip_keys(homes: &Path) {
    let Ok(harnesses) = std::fs::read_dir(homes) else {
        return;
    };
    for home in harnesses.flatten() {
        if !home.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let home = home.path();
        let _ = std::fs::remove_file(home.join(".env"));
        for folder in std::fs::read_dir(&home).into_iter().flatten().flatten() {
            if folder.file_type().is_ok_and(|t| t.is_dir()) {
                let _ = std::fs::remove_file(folder.path().join(".env"));
            }
        }
    }
}

/// What of an agent's own files folder [`clear`] keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kept {
    /// Its whole folder: Home, its own and its harness homes (an agent
    /// not seen running where it went).
    All,
    /// Its Home, where it's its own, and its own folder.
    Files,
    /// Nothing: its files go with it.
    Nothing,
}

/// Agent `agent` left this machine for `to`: its service, its owner
/// delivery's reader of the interaction store, settings, harness homes (state, installs, keys), log, sockets, sandbox rules and
/// its memory's and scheduler's tools go, its files folder is kept [`keep_days`] when it's its own (a folder
/// you chose stays where it is), and a note says where it went. With
/// `unconfirmed` (it wasn't seen running there), its harness homes are
/// kept with its files. Returns what to tell the user.
pub fn leave(agent: &Settings, to: Place, unconfirmed: bool) -> anyhow::Result<Cleared> {
    // What it was granted of its teams' folders, read before its sandbox
    // file goes.
    let teams = crate::sandbox_file::of(&agent.name).spec.teams;
    // Its keys first: while they stay here it hasn't left, so nothing of
    // it goes, the move that called keeps its note of the copy it took,
    // and `left` is tried again.
    agent
        .forget_keys()
        .within(format!("{}'s keys stay in your secrets", agent.name))?;
    let mut cleared = clear(
        agent,
        match unconfirmed {
            true => Kept::All,
            false => Kept::Files,
        },
    )?;
    Note {
        teams,
        service: agent.service.clone(),
        ..Note::new(agent.agent_id().map(str::to_string), &agent.name, to)
    }
    .save()?;
    cleared.left.extend(remove_settings(agent)?);
    Ok(cleared)
}

/// Deletes agent `agent` here (`lnk agent delete`): all [`leave`] does,
/// but no note says where it went, and its own files folder goes too
/// unless `kept`. Once its files start going, its keys and settings go
/// whatever a folder of its files does: what can't be removed is left
/// and said, for the user to finish, never a delete stopped halfway.
pub fn delete(agent: &Settings, kept: crate::deleted::Preserved) -> anyhow::Result<Cleared> {
    if let Some(id) = agent.agent_id() {
        models::release(id)?;
    }
    let folder = matches!(kept, crate::deleted::Preserved::Folder(_));
    let mut cleared = clear(
        agent,
        match folder {
            true => Kept::Files,
            false => Kept::Nothing,
        },
    )?;
    let days = match keep_days() {
        Some(days) => format!("for {days} days"),
        None => "until you delete it".into(),
    };
    match kept {
        crate::deleted::Preserved::Nothing => {}
        crate::deleted::Preserved::Archive(archive)
            if agent.harness() == settings::DEFAULT_HARNESS =>
        {
            cleared.notes.push(format!(
                "Its settings, files, chats and memory are in {} {days}: lnk archive import brings \
                 it back as a new agent.",
                tilde(&archive)
            ))
        }
        crate::deleted::Preserved::Archive(archive) => cleared.notes.push(format!(
            "Its settings and files are in {} {days}. It ran on the {} harness, whose chats and \
             memory were in its home and are gone, so its archive holds its files only.",
            tilde(&archive),
            agent.harness()
        )),
        crate::deleted::Preserved::Folder(why) => cleared.notes.push(format!(
            "It couldn't be kept as one archive ({why}), so its files are kept as a folder instead."
        )),
    }
    cleared.left.extend(remove_settings(agent)?);
    Ok(cleared)
}

/// What [`delete`] and [`leave`] did.
#[derive(Debug, Default)]
pub struct Cleared {
    /// What to tell the user.
    pub notes: Vec<String>,
    /// What couldn't be removed, each by its path and why, for the user
    /// to remove by hand.
    pub left: Vec<String>,
}

/// Removes agent `agent`'s settings: an error if a grant can't be
/// revoked; what of its control folder can't go is returned.
fn remove_settings(agent: &Settings) -> anyhow::Result<Vec<String>> {
    // Revoke grants before removing the file that says the agent is here.
    remove_control_files(&[crate::sandbox_file::path(&agent.name), agent.path()])?;
    // Then what remains of its control folder, with nothing left to grant.
    remove_owned(
        &[link_files::layout::Home::AgentSettings.path()],
        &agent.name,
    )
}

/// Removes agent `name`'s own folder in each of `roots`, the managed
/// locations the catalogue (`layout.toml`) gives it: its harness homes
/// in `Hidden::AgentData`, its control in `Home::AgentSettings`. Each
/// root is opened refusing a link, and what's in the agent's folder is
/// removed through handles, a link removed itself, never followed. The
/// name is an agent's (lowercase, no dots), so it can't name a root's
/// shared entries, such as `Teams` or `defaults.toml`. Work files are
/// never in these roots. An error only for a name that isn't an agent's;
/// otherwise each root is tried, and what couldn't go is returned, by
/// its path and why.
fn remove_owned(roots: &[PathBuf], name: &str) -> anyhow::Result<Vec<String>> {
    link_machine::machine::check_name(name)?;
    let mut left = Vec::new();
    for root in roots {
        let dir = match link_files::nofollow::Dir::open_unlinked(root) {
            Ok(dir) => dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                left.push(format!("{}: {err}", root.join(name).display()));
                continue;
            }
        };
        match dir.remove_all(std::ffi::OsStr::new(name)) {
            // The error names what it left from `name` down.
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                left.push(format!("{}/{err}", root.display()));
            }
            _ => {}
        }
    }
    Ok(left)
}

fn remove_control_files(paths: &[PathBuf]) -> anyhow::Result<()> {
    for path in paths {
        let parent = path
            .parent()
            .within("an agent's control file has a folder")?;
        let dir = match link_files::nofollow::Dir::open_unlinked(parent) {
            Ok(dir) => dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).within(format!("opening {}", parent.display())),
        };
        match dir.remove_file(
            path.file_name()
                .within("an agent's control file has a name")?,
        ) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                return Err(err).within(format!("removing {}", path.display()));
            }
            _ => {}
        }
    }
    Ok(())
}

/// The sandbox tools the agent keyed `key` has of its own, which go with
/// it: its memory's, scheduler's, run's, file's, context's and Link MCP's, and
/// each namespace's server, every one a harness may be handed.
fn own_tools(key: &str) -> Vec<String> {
    let namespaced = link_harness_contract::harness::NAMESPACES
        .iter()
        .map(|namespace| format!("namespace-{namespace}-{key}"));
    [
        settings::memory_tool_of(key),
        format!("scheduler-{key}"),
        format!("run-{key}"),
        format!("file-{key}"),
        format!("context-{key}"),
        format!("mcp-{key}"),
    ]
    .into_iter()
    .chain(namespaced)
    .collect()
}

/// Everything of agent `agent` here but its settings file, which says
/// it's here until the caller removes it: its service, forgotten by the
/// service manager, its owner delivery's reader of the interaction
/// store, harness homes, log, sockets, sandbox rules, its
/// memory's and scheduler's tools and its keys; of its folder, what `kept` says, put aside for
/// [`keep_days`]: its Home, when it's its own, and its own folder
/// (`agent/`) with it.
fn clear(agent: &Settings, kept: Kept) -> anyhow::Result<Cleared> {
    service::stop(&agent.name)?;
    service::forget(&agent.name);
    // Stopped now, its owner delivery never reads the outbox here again,
    // so nothing waits for it there. Before anything of it goes: a store
    // that can't be written leaves the agent whole, to delete again.
    link_interactions::Store::machine()
        .and_then(|store| crate::bridge::forget_owner_reader(&store, &agent.name))
        .within(format!(
            "its reader of the interaction store's outbox, {}, stays",
            crate::bridge::owner_reader(&agent.name)
        ))?;
    let dir = agent.dir();
    let files = agent.files();
    let own_files = files.starts_with(&dir);
    let mut notes = Vec::new();
    // Its background commands: none goes with it, and none stays running.
    let stopped = crate::background::stop_background(agent, true);
    if !stopped.is_empty() {
        notes.push(format!(
            "Its background commands were stopped, and don't move: {}.",
            stopped.join("; ")
        ));
    }
    if let Ok(harnesses) = std::fs::read_dir(agent.boxes()) {
        for harness in harnesses.flatten() {
            let Some(harness) = harness.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let profile = agent.profile(&harness);
            for rules in [
                profile.clone(),
                format!("{profile}-setup"),
                format!("{profile}-check"),
            ] {
                link_sandbox_client::forget(&rules);
            }
        }
    }
    // Its own tools, which no other agent may be granted, and what its
    // owner's memory service would serve it with.
    let key = agent.key();
    if let Ok(owner) = agent.owner() {
        let _ = std::fs::remove_file(settings::memory_record_of(&owner, &key));
    }
    for tool in own_tools(&key) {
        let _ = link_core::lnk()
            .args(["sandbox", "tool", "remove", &tool])
            .stdin(std::process::Stdio::null())
            .output();
    }
    if matches!(kept, Kept::All) && agent.boxes().is_dir() {
        let root = link_files::layout::Hidden::HarnessHistory.path();
        let saved = aside_in(&root, &agent.name)?;
        std::fs::rename(agent.boxes(), &saved)?;
        strip_keys(&saved);
        notes.push(format!(
            "Its harness homes, without their keys, stay in {}.",
            tilde(&saved)
        ));
    }
    let put_aside = match kept {
        Kept::All if dir.exists() => {
            let put_aside = keep_files(&agent.name, &dir)?;
            Some(put_aside)
        }
        Kept::All | Kept::Files => keep_folder(&agent.name, &agent.folder(), own_files)?,
        Kept::Nothing => None,
    };
    if let Some(put_aside) = &put_aside {
        notes.push(format!(
            "Its {} stay in {} {}.",
            match kept {
                Kept::All =>
                    "files and harnesses, without the keys a harness keeps in its home (its .env),",
                _ => "files",
            },
            tilde(put_aside),
            match keep_days() {
                Some(days) => format!("for {days} days"),
                None => "until you delete them".into(),
            }
        ));
    }
    if !own_files {
        notes.push(format!("Its files folder, {}, stays.", tilde(&files)));
    }
    // From here on, what can't be removed is left and said: its keys
    // and settings go all the same.
    let mut left = Vec::new();
    if dir.exists()
        && let Err(err) = std::fs::remove_dir_all(&dir)
    {
        left.push(format!("{}: {err}", dir.display()));
    }
    let _ = std::fs::remove_dir_all(settings::config_of(&agent.name));
    let _ = std::fs::remove_dir_all(agent.wires());
    // Its harness homes: memory, conversations, settings and keys, unless
    // `Kept::All` put them aside above.
    match remove_owned(&[link_files::layout::Hidden::AgentData.path()], &agent.name) {
        Ok(homes) => left.extend(homes),
        Err(err) => left.push(format!("{err:#}")),
    }
    if let Err(err) = agent.forget_keys() {
        left.push(format!("its keys, still in your secrets: {err:#}"));
    }
    Ok(Cleared { notes, left })
}

/// `path` under `~` as `~/...`.
fn tilde(path: &Path) -> String {
    match path.strip_prefix(link_files::home_dir()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Deletes what agents left behind more than [`keep_days`] ago (by the
/// date in the folder's name), and deleted agents' archives as old;
/// returns what went.
pub fn prune() -> Vec<PathBuf> {
    prune_staging(&crate::transfer::moving_dir(), now());
    prune_spool(&link_files::layout::Hidden::Sending.path(), now());
    match keep_days() {
        Some(days) => {
            let mut removed = prune_in(&files_dir(), now() / 86400, days);
            // Deleted agents' archives: only those a deletion made.
            if let Ok(roots) = link_archive::Roots::here() {
                for archive in link_archive::expired(&roots.archive(), days, &date(now())) {
                    if std::fs::remove_file(&archive).is_ok() {
                        removed.push(archive);
                    }
                }
            }
            removed.extend(prune_in(
                &link_files::layout::Hidden::HarnessHistory.path(),
                now() / 86400,
                days,
            ));
            removed
        }
        None => Vec::new(),
    }
}

/// How many days a move's copy waits for its switch before it's removed.
pub const STAGING_DAYS: u64 = 7;

/// Removes what moves to this machine copied into `dir`
/// (`~/.lnk/safe/agents/transfers/receiving`) and never switched, after [`STAGING_DAYS`]
/// from their copy (`now`, in Unix seconds); returns what went.
pub fn prune_staging(dir: &Path, now: u64) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut gone = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let at = crate::transfer::record_of(&path).map(|r| r.at).or_else(|| {
            let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
            Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs())
        });
        if at.is_some_and(|at| now.saturating_sub(at) > STAGING_DAYS * 86400)
            && std::fs::remove_dir_all(&path).is_ok()
        {
            gone.push(path);
        }
    }
    gone
}

/// Removes what a move that stopped left in `dir`
/// (`~/.lnk/safe/agents/transfers/sending`), a copy of memory's index set aside to
/// hash: anything there a day old at `now`.
fn prune_spool(dir: &Path, now: u64) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let at = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        if at.is_some_and(|at| now.saturating_sub(at) > 86400)
            && entry.file_type().is_ok_and(|t| t.is_file())
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// [`prune`] in folder `dir` on day `today`, keeping `days`.
fn prune_in(dir: &Path, today: u64, days: u64) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut gone = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(day) = path.file_name().and_then(|n| n.to_str()).and_then(day_of) else {
            continue;
        };
        if today.saturating_sub(day) > days && std::fs::remove_dir_all(&path).is_ok() {
            gone.push(path);
        }
    }
    gone
}

/// The day (since 1970) in a [`files_dir`] folder's name: `<name>-<date>`,
/// or `<name>-<date>-<n>`.
fn day_of(folder: &str) -> Option<u64> {
    let parts: Vec<&str> = folder.split('-').collect();
    [3, 4].iter().find_map(|&skip| {
        let end = parts.len().checked_sub(skip - 3)?;
        let start = end.checked_sub(3)?;
        (start > 0).then_some(())?;
        let date = parts[start..end].join("-");
        days_from_date(&date)
    })
}

/// Now, in Unix seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `YYYY-MM-DD` of Unix seconds `at`, in UTC.
pub fn date(at: u64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = (at / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The day since 1970 of a `YYYY-MM-DD` date.
fn days_from_date(date: &str) -> Option<u64> {
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 || parts.next().is_some() {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    u64::try_from(era * 146_097 + doe - 719_468).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every namespace server an agent is handed goes with it, the
    /// question's and cron's among them, so none outlives a delete.
    #[test]
    fn own_tools_name_every_namespace() {
        let tools = own_tools("k1");
        for namespace in link_harness_contract::harness::NAMESPACES {
            let tool = format!("namespace-{namespace}-k1");
            assert!(tools.contains(&tool), "{tool} isn't among {tools:?}");
        }
    }

    /// A note a newer release wrote is refused, naming the upgrade, never
    /// read as no note: an agent that moved away isn't forgotten after a
    /// rollback. One this release wrote reads back.
    #[test]
    fn notes_refuse_a_newer_schema() {
        let fixture = link_test_kit::TempDir::new("moved-newer");
        let dir = fixture.canonicalize().unwrap();
        assert!(notes_in(&dir.join("absent")).unwrap().is_empty());
        let note = Note::new(None, "main", Place::of_account("acme", None));
        std::fs::write(
            dir.join("main.toml"),
            link_files::saved::print(&note).unwrap(),
        )
        .unwrap();
        let read = notes_in(&dir).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].name, "main");
        let newer = link_files::saved::print(&note)
            .unwrap()
            .replace("schema = 1", "schema = 2");
        assert!(newer.contains("schema = 2"));
        std::fs::write(dir.join("main.toml"), newer).unwrap();
        let refused = format!("{:#}", notes_in(&dir).unwrap_err());
        assert!(refused.contains("lnk upgrade"), "{refused}");
        assert!(refused.contains("main.toml"), "{refused}");
    }

    #[test]
    fn removing_control_revokes_grants_without_removing_other_data() {
        let fixture = link_test_kit::TempDir::new("revoke-agent");
        let root = fixture.canonicalize().unwrap();
        let settings = root.join("safe");
        let other = root.join("safe/other");
        let data = root.join("homes/main");
        for dir in [&settings, &other, &data] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let grants = settings.join("sandbox.toml");
        let agent = settings.join("agent.toml");
        let others_grants = other.join("sandbox.toml");
        std::fs::write(&grants, "broad old grants").unwrap();
        std::fs::write(&agent, "old agent identity").unwrap();
        std::fs::write(&others_grants, "another agent's grants").unwrap();
        std::fs::write(data.join("conversation"), "kept data").unwrap();
        let controls = [grants.clone(), agent.clone()];
        remove_control_files(&controls).unwrap();
        remove_control_files(&controls).unwrap();
        assert!(!grants.exists() && !agent.exists());
        assert_eq!(
            std::fs::read_to_string(&others_grants).unwrap(),
            "another agent's grants"
        );
        assert_eq!(
            std::fs::read_to_string(data.join("conversation")).unwrap(),
            "kept data"
        );
    }

    #[test]
    fn removing_control_refuses_a_linked_agent_folder() {
        let fixture = link_test_kit::TempDir::new("revoke-linked");
        let root = fixture.canonicalize().unwrap();
        let elsewhere = root.join("other");
        std::fs::create_dir(&elsewhere).unwrap();
        let grants = elsewhere.join("sandbox.toml");
        std::fs::write(&grants, "never removed").unwrap();
        let linked = root.join("agent");
        std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
        assert!(remove_control_files(&[linked.join("sandbox.toml")]).is_err());
        assert_eq!(std::fs::read_to_string(grants).unwrap(), "never removed");
    }

    /// A deleted agent's managed folders go, in each root, while another
    /// agent's, the roots' shared entries and its work files stay.
    #[test]
    fn removing_owned_folders_keeps_other_agents_and_work_files() {
        let fixture = link_test_kit::TempDir::new("remove-owned");
        let root = fixture.canonicalize().unwrap();
        let homes = root.join("data/agents");
        let control = root.join("Safe/Agents");
        let work = root.join("Agents/main/home");
        for dir in [
            homes.join("main/harnesses/hermes/.hermes"),
            homes.join("work/harnesses/hermes"),
            control.join("main"),
            control.join("work"),
            control.join("Teams"),
            work.clone(),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(homes.join("main/harnesses/hermes/.env"), "KEY=1").unwrap();
        std::fs::write(homes.join("main/harnesses/hermes/.hermes/state.db"), "x").unwrap();
        std::fs::write(control.join("main/notes.toml"), "x").unwrap();
        std::fs::write(homes.join("work/harnesses/hermes/.env"), "kept").unwrap();
        std::fs::write(control.join("work/agent.toml"), "kept").unwrap();
        std::fs::write(control.join("Teams/research.toml"), "kept").unwrap();
        std::fs::write(control.join("defaults.toml"), "kept").unwrap();
        std::fs::write(work.join("draft.md"), "kept").unwrap();
        // A link in the agent's folder is removed, never its target.
        std::os::unix::fs::symlink(&work, homes.join("main/harnesses/files")).unwrap();
        let roots = [homes.clone(), control.clone()];
        remove_owned(&roots, "main").unwrap();
        remove_owned(&roots, "main").unwrap();
        assert!(!homes.join("main").exists() && !control.join("main").exists());
        for kept in [
            homes.join("work/harnesses/hermes/.env"),
            control.join("work/agent.toml"),
            control.join("Teams/research.toml"),
            control.join("defaults.toml"),
            work.join("draft.md"),
        ] {
            assert_eq!(
                std::fs::read_to_string(&kept).unwrap(),
                "kept",
                "{}",
                kept.display()
            );
        }
        // No agent's name reaches a root's shared entries.
        for name in ["Teams", "defaults.toml", "..", "main/../work", ""] {
            assert!(remove_owned(&roots, name).is_err(), "{name}");
        }
        assert!(control.join("Teams/research.toml").exists());
    }

    /// A part of an agent's folder that can't go (here, deeper than a
    /// removal walks) is left and named by its path; the rest of it, and
    /// its folder in every other root, still go.
    #[test]
    fn removing_owned_folders_carries_on_past_what_it_leaves() {
        let fixture = link_test_kit::TempDir::new("remove-owned-left");
        let root = fixture.canonicalize().unwrap();
        let homes = root.join("data/agents");
        let control = root.join("Safe/Agents");
        let mut deep = homes.join("main/harnesses/go");
        for _ in 0..link_files::nofollow::DEEPEST {
            deep.push("d");
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(control.join("main")).unwrap();
        std::fs::write(homes.join("main/harnesses/.env"), "KEY=1").unwrap();
        std::fs::write(control.join("main/agent.toml"), "x").unwrap();
        let left = remove_owned(&[homes.clone(), control.clone()], "main").unwrap();
        assert_eq!(left.len(), 1, "{left:?}");
        let named = homes.join("main/harnesses/go").display().to_string();
        assert!(left[0].starts_with(&named), "{left:?}");
        assert!(!homes.join("main/harnesses/.env").exists());
        assert!(!control.join("main").exists());
    }

    /// A root that's a link is refused, and nothing it leads to goes.
    #[test]
    fn removing_owned_folders_refuses_a_linked_root() {
        let fixture = link_test_kit::TempDir::new("remove-owned-linked");
        let root = fixture.canonicalize().unwrap();
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("main")).unwrap();
        std::fs::write(elsewhere.join("main/conversation"), "never removed").unwrap();
        let linked = root.join("homes");
        std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
        let left = remove_owned(std::slice::from_ref(&linked), "main").unwrap();
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(left[0].starts_with(&linked.join("main").display().to_string()));
        assert_eq!(
            std::fs::read_to_string(elsewhere.join("main/conversation")).unwrap(),
            "never removed"
        );
    }

    /// A kept agent's harness homes keep their files but not their keys,
    /// and a `.env` that's a link is removed, never followed.
    #[test]
    fn the_kept_homes_lose_their_keys() {
        let dir = link_test_kit::TempDir::new("strip-keys");
        let homes = dir.join("Harnesses");
        let heron = homes.join("heron");
        std::fs::create_dir_all(heron.join(".heron")).unwrap();
        std::fs::write(heron.join(".env"), "KEY=1").unwrap();
        std::fs::write(heron.join(".heron/.env"), "KEY=2").unwrap();
        std::fs::write(heron.join(".heron/config.yaml"), "kept").unwrap();
        let outside = dir.join("outside.env");
        std::fs::write(&outside, "not the harness's").unwrap();
        std::fs::create_dir_all(homes.join("otter/.otter")).unwrap();
        std::os::unix::fs::symlink(&outside, homes.join("otter/.otter/.env")).unwrap();
        strip_keys(&homes);
        assert!(!heron.join(".env").exists());
        assert!(!heron.join(".heron/.env").exists());
        assert!(heron.join(".heron/config.yaml").exists());
        assert!(homes.join("otter/.otter/.env").symlink_metadata().is_err());
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "not the harness's"
        );
    }

    #[test]
    fn dates_both_ways() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_790_000_000), "2026-09-21");
        assert_eq!(date(951_782_400), "2000-02-29");
        for at in [0, 951_782_400, 1_790_000_000, 4_102_444_800] {
            assert_eq!(days_from_date(&date(at)), Some(at / 86400), "{at}");
        }
        assert_eq!(days_from_date("2026-13-01"), None);
        assert_eq!(days_from_date("26-09-01"), None);
    }

    #[test]
    fn what_was_left_goes_after_its_days() {
        let dir = link_test_kit::TempDir::new("moved");
        for name in ["main-2026-09-01", "main-2026-09-21", "notes"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        let today = days_from_date("2026-10-07").unwrap();
        assert_eq!(prune_in(&dir, today, 30), [dir.join("main-2026-09-01")]);
        assert_eq!(prune_in(&dir, today, 10), [dir.join("main-2026-09-21")]);
        assert!(dir.join("notes").exists());
    }

    /// A copy that never switched goes after a week; one newer stays.
    #[test]
    fn a_copy_never_switched_goes_after_a_week() {
        let dir = link_test_kit::TempDir::new("staging");
        let now = 1_800_000_000;
        for (id, at) in [
            ("a1b2c3d4e5f6", now - 8 * 86400),
            ("b1b2c3d4e5f6", now - 86400),
        ] {
            std::fs::create_dir_all(dir.join(id).join("files")).unwrap();
            let record = crate::transfer::Record {
                move_id: "0123456789abcdef0123456789abcdef".into(),
                at,
            };
            std::fs::write(
                dir.join(id).join(crate::transfer::RECORD),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(prune_staging(&dir, now), [dir.join("a1b2c3d4e5f6")]);
        assert!(dir.join("b1b2c3d4e5f6/files").exists());
    }

    #[test]
    fn a_left_folder_knows_its_day() {
        let day = days_from_date("2026-09-21");
        assert_eq!(day_of("main-2026-09-21"), day);
        assert_eq!(day_of("moltbot-2-2026-09-21"), day);
        assert_eq!(day_of("moltbot-2-2026-09-21-3"), day);
        assert_eq!(day_of("2026-09-21"), None);
        assert_eq!(day_of("main"), None);
        assert_eq!(day_of("notes"), None);
    }
}
