//! Carrying your agent from one machine to another (`lnk agent move`):
//! `lnk agent state-send` writes it as a tar stream, `lnk agent receive` reads
//! it on the other machine, in two phases (`link_agent_contract::moving`, format
//! 4, each file with its SHA-256):
//!
//! - The copy, while the agent answers: `files/...`, its files folder,
//!   conversations included, and `memory/<engine>/index.sqlite`, memory's
//!   index, as SQLite's online backup of it, with the parts beside it
//!   (`index.sqlite.<part>`, its words and vectors). It lands in staging there,
//!   `~/.lnk/safe/agents/transfers/receiving/<agent id>`, which says what it holds
//!   ([`have`]), so a copy tried again sends only what's new.
//! - The switch, the agent paused: what differs from what staging holds,
//!   the paths gone since (`removed`), and everything else of it:
//!   - `lnk-move.json`: the agent's ID and name, its harness, what of
//!     its settings move (model, phone), what you granted it (its
//!     sandbox file), its spec, what of its home is its state, and its
//!     buckets and their password.
//!   - `harness/...`: that state (`lnk-<name> state`), never its install:
//!     the harness is installed fresh on the other side.
//!   - `connections/<connection>/...`: for a harness Link carries its connections
//!     for, what links each linked connection (Signal's account), as
//!     `lnk-<connection> state` says, from the agent's folder for the connection. Not what the connection's module installed there:
//!     the other machine installs its own.
//!
//! Nothing goes through the relay: `lnk agent move` pipes one machine's
//! `send` into the other's `receive`, over SSH for a box, or through a
//! cloud's adapter for one that hosts agents.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_harness_contract::harness::{Model, State};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

use crate::sandbox_file::Sandbox;
use crate::settings::Settings;
use link_files::nofollow::{self, Copying, Dir, Kind, Links};

use link_agent_contract::moving as stream;
pub use link_agent_contract::moving::Have;
use link_agent_contract::moving::MANIFEST;
pub use link_agent_contract::moving::PROTOCOL;
pub use link_agent_contract::moving::Phase;
mod archiving;
mod indexes;
pub(crate) mod landing;
mod receiving;
mod restoring;
mod settling;
mod stores;
pub use archiving::archive_state;
use archiving::{add_folder, add_state, holding, in_box, in_home, left_out, seen_files};
pub use indexes::INDEX;
use indexes::{add_index, index_of, put_index};
pub use landing::{RECORD, Record, SEED_DAYS, have, moving_dir, record_of, staging_of};
use landing::{held, staging_in};
use link_agent_contract::moving::collections;
pub use receiving::{drop_staging, receive};
use receiving::{gone, remove_at, remove_paths, unpack_in};
use restoring::{move_in, put_state};
pub use restoring::{move_state, replace_state};
pub use settling::{Role, TeamGrant, arrived, settle_model, settle_sandbox};

/// The agent a stream carries: the workload it is, `agent:<id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub workload: link_id::Workload,
    /// The service it acts as, its ID, minted with it and carried with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    pub name: String,
    /// Whose it is ([`Settings::owner`]): a machine signed in as another
    /// account refuses it. None from an lnk before owners.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The teams it's on, by ID.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<String>,
}

impl Agent {
    /// Its ID, bare: its workload's after its kind.
    pub fn id(&self) -> &str {
        self.workload.id()
    }
}

/// The first entry of the stream. A copy's says the agent and its
/// harness; everything else of it, a switch's.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub phase: Phase,
    /// The move's ID: the same in its copy and its switch.
    #[serde(rename = "move")]
    pub move_id: String,
    /// In a switch: the paths in `files/` and `memory/` gone since the
    /// copy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    /// The sender's lnk.
    pub version: String,
    pub agent: Agent,
    pub harness: String,
    /// What moves of the settings. Not the files folder (each machine has
    /// its own), the sandbox (on, wherever it goes) or where it runs.
    #[serde(default)]
    pub model: Option<Model>,
    /// Its fallback model (`lnk agent start --model.fallback_model`), its key with it,
    /// when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Model>,
    /// Its connections, as its settings keep them.
    #[serde(default)]
    pub connected: BTreeMap<String, crate::connections::Connection>,
    /// What you granted the agent where it came from (its sandbox file),
    /// when it has one: what of it opens the machine it lands on is
    /// settled there ([`settle_sandbox`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<crate::sandbox_file::Sandbox>,
    /// Its own spec's text (`agent.toml`), when it has one: read strictly
    /// where it lands, as `--spec` reads one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Its schedules' identities (`schedules.json`), which its spec never
    /// says: carried with the spec so a schedule stays the one its runner
    /// knows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<crate::schedules::Kept>,
    /// What of its home was sent, and what of it the install is, as the
    /// sender's adapter says. `receive` goes by its own adapter's.
    #[serde(default)]
    pub state: State,
    /// What of each linked connection's folder was sent, by connection
    /// (`signal`), when Link carries it.
    #[serde(default)]
    pub connections: BTreeMap<String, State>,
    /// Its buckets and their password, as `lnk storage export` says
    /// (`{"password", "buckets"}`); none for an agent never backed up.
    #[serde(default)]
    pub bucket: Option<serde_json::Value>,
}

/// Which part of a move `send` writes, and what the other end holds.
pub struct Sending<'a> {
    pub phase: Phase,
    /// The move's ID.
    pub move_id: &'a str,
    /// What the other end holds, by path and hash: each file it holds the
    /// same isn't sent, and in a switch, each it holds that's gone here
    /// is `removed`.
    pub have: &'a Have,
}

/// What a write of the stream skips (files the other end holds the
/// same) and where a large file is copied aside to be hashed.
struct Out<'a> {
    have: &'a Have,
    spool: PathBuf,
}

impl Out<'_> {
    fn none() -> Out<'static> {
        static NONE: std::sync::OnceLock<Have> = std::sync::OnceLock::new();
        Out {
            have: NONE.get_or_init(Have::new),
            spool: spool(),
        }
    }
}

/// `~/.lnk/safe/agents/transfers/sending`: where a large file is copied aside, unnamed,
/// to be hashed and sent as it was hashed.
fn spool() -> PathBuf {
    link_files::layout::Hidden::Sending.path()
}

/// Writes the agent of `settings` to `out` as a tar stream, the part of
/// the move `sending` says: a copy, its files and memory's index; a
/// switch, what of them differs from what the other end holds, and
/// everything else (harness `state` says what of its home, and
/// `connections` what of each linked connection's folder).
pub fn send(
    out: impl Write,
    settings: &Settings,
    sending: &Sending,
    state: &State,
    connections: &BTreeMap<String, State>,
    bucket: Option<serde_json::Value>,
    seal_to: Option<&link_secretive::SealTo>,
) -> anyhow::Result<()> {
    let name = settings.harness();
    let id = settings
        .agent_id()
        .map(str::to_string)
        .within("this agent has no ID: only the machine it runs on moves it")?;
    stream::check_move_id(sending.move_id)?;
    let switch = sending.phase == Phase::Switch;
    let manifest = Manifest {
        format: PROTOCOL.current,
        phase: sending.phase,
        move_id: sending.move_id.to_string(),
        removed: Vec::new(),
        version: env!("CARGO_PKG_VERSION").into(),
        agent: Agent {
            workload: crate::settings::ids::workload(&id)?,
            service: settings.service.clone(),
            name: settings.name.clone(),
            owner: settings.owner.clone(),
            teams: Vec::new(),
        },
        harness: name.into(),
        model: switch.then(|| settings.model.duplicate()).flatten(),
        fallback: switch.then(|| settings.fallback.duplicate()).flatten(),
        connected: match switch {
            true => settings.connections.duplicate(),
            false => BTreeMap::new(),
        },
        // What you granted it; its harness's needs are the harness's, and
        // asked again where it lands.
        sandbox: (switch && crate::sandbox_file::path(&settings.name).exists())
            .then(|| crate::sandbox_file::of(&settings.name)),
        spec: if switch {
            settings
                .own_spec()?
                .map(|spec| link_spec::print_spec(&spec))
                .transpose()?
        } else {
            None
        },
        schedules: match switch {
            true => crate::schedules::record(settings)?,
            false => Vec::new(),
        },
        // A copy's, when it carries the harness's state.
        state: state.clone(),
        connections: match switch {
            true => connections.clone(),
            false => BTreeMap::new(),
        },
        bucket: bucket.filter(|_| switch),
    };
    let parts = Parts {
        harness: settings.box_of(name),
        connections: connections
            .keys()
            .map(|connection| {
                (
                    connection.clone(),
                    crate::bridge::state(&settings.name, connection),
                )
            })
            .collect(),
        folder: settings.folder(),
        index: index_of(settings)?,
        spool: spool(),
    };
    // Between machines, what holds its secrets is sealed to the receiver.
    let stores = stores::Stores::of(settings)?;
    // Memory's copy goes with `parts`.
    write(out, manifest, &parts, sending.have, seal_to, Some(&stores))
}

/// Whether a turn still holds a live lease, at the conversation's resolved place.
pub fn turn_running(settings: &Settings) -> anyhow::Result<bool> {
    stores::Stores::of(settings)?.turn_running()
}

/// The size of the collections carried through their stores, before a move.
pub fn collection_bytes(settings: &Settings) -> anyhow::Result<u64> {
    let stores = stores::Stores::of(settings)?;
    let mut size = 0;
    collections::read(&stores.kept(), true, |_, bytes| {
        size += bytes.len() as u64;
        Ok(())
    })?;
    Ok(size)
}

/// Where what a stream carries of an agent is on this machine.
struct Parts {
    /// Its harness's home.
    harness: PathBuf,
    /// Each linked connection's folder.
    connections: BTreeMap<String, PathBuf>,
    /// Its Home and its own folder, carried as the agent sees them
    /// (`files/`, [`link_agent_contract::folder::Folder::seen`]).
    folder: link_agent_contract::folder::Folder,
    /// Memory's index: each of its entries' path in the stream, and a
    /// copy of it.
    index: Option<indexes::Carried>,
    /// Where a large file is copied aside to be hashed.
    spool: PathBuf,
}

/// `manifest`, then what of `parts` its phase carries, to `out`: a
/// copy, the files folder and memory's index; a switch, the harness's
/// state and each linked connection's (as `manifest` says what of each),
/// and the files and index, with the paths `have` holds that are gone
/// here as `removed`. A file `have` holds the same isn't sent.
fn write(
    out: impl Write,
    manifest: Manifest,
    parts: &Parts,
    have: &Have,
    seal_to: Option<&link_secretive::SealTo>,
    stores: Option<&stores::Stores>,
) -> anyhow::Result<()> {
    match (manifest.phase, seal_to) {
        (Phase::Switch, Some(to)) => {
            let mut sealed = link_secretive::stream::Sender::new(out, to, stream::stream_about())?;
            write_tar(&mut sealed, manifest, parts, have, stores)?;
            sealed.finish()?;
            Ok(())
        }
        // A switch carries the agent's secrets: never written unsealed.
        (Phase::Switch, None) => anyhow::bail!("a switch needs its receiver's public key"),
        _ => write_tar(out, manifest, parts, have, stores),
    }
}

fn write_tar(
    out: impl Write,
    mut manifest: Manifest,
    parts: &Parts,
    have: &Have,
    stores: Option<&stores::Stores>,
) -> anyhow::Result<()> {
    let mut entries = BTreeMap::new();
    let switch = manifest.phase == Phase::Switch;
    let file;
    let kept;
    let kept = match stores {
        Some(stores) => stores.kept(),
        None => {
            let root = &parts.folder.agent;
            file = link_store_contract::store::file::Folder::at(root, manifest.agent.id())?;
            kept = link_store_contract::store::Agent {
                id: manifest.agent.id().to_string(),
                owner: manifest.agent.owner.clone().unwrap_or_default(),
            };
            collections::Kept::new(kept, |_| &file)
        }
    };
    collections::read(&kept, switch, |path, bytes| {
        entries.insert(path.to_string(), bytes.to_vec());
        Ok(())
    })?;
    // What the other end holds that's gone here: a file of the files
    // folder, or the index.
    if switch {
        let mut here = seen_files(&parts.folder)?;
        here.extend(entries.keys().cloned());
        here.extend(
            parts
                .index
                .iter()
                .flat_map(|carried| carried.entries.iter().map(|(entry, _)| entry.clone())),
        );
        here.extend(still_state(&parts.harness, &manifest.state, have)?);
        manifest.removed = have
            .keys()
            .filter(|path| !here.contains(*path))
            .cloned()
            .collect();
    }
    let ctx = Out {
        have,
        spool: parts.spool.clone(),
    };
    let mut tar = tar::Builder::new(out);
    tar.follow_symlinks(false);
    let json = serde_json::to_value(&manifest)?;
    stream::append_manifest(&mut tar, &serde_json::to_vec(&json)?)?;
    // A switch's, and a copy's that carries it.
    add_state(
        &mut tar,
        &parts.harness,
        &manifest.state,
        Path::new("harness"),
        &ctx,
    )?;
    if switch {
        for (connection, state) in &manifest.connections {
            let Some(dir) = parts.connections.get(connection) else {
                continue;
            };
            add_state(
                &mut tar,
                dir,
                state,
                &Path::new("connections").join(connection),
                &ctx,
            )?;
        }
    }
    // Home, but a folder of it named as one of the agent's own (left
    // there before, or a repository's), which comes from its own folder
    // in its place: said, once a move, so it's never left unknown.
    let home = &parts.folder.home;
    if switch {
        for name in left_out(&parts.folder) {
            link_format::aside(
                link_format::Report::warning(link_format::sentence!(
                    "{} isn't carried: a move carries the agent's own {}/ in its place.",
                    link_format::Path::new(home.join(&name)),
                    link_format::Foreign::new(&name)
                ))
                .why(link_format::prose!("Rename it to take it along.")),
            );
        }
    }
    if home.is_dir() {
        let folder = Dir::open(home).within(format!("opening {}", home.display()))?;
        let own: Vec<PathBuf> = link_agent_contract::folder::SEEN
            .iter()
            .map(|c| home.join(c.directory()))
            .collect();
        add_folder(&mut tar, folder, home, Path::new("files"), &own, &ctx)?;
    }
    for (path, bytes) in entries {
        if ctx.have.get(&path) != Some(&link_digest::sha256(&bytes)) {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            tar.append_pax_extensions([(stream::SHA256, link_digest::sha256(&bytes).as_bytes())])?;
            tar.append_data(&mut header, path, &bytes[..])?;
        }
    }
    for (entry, copy) in parts.index.iter().flat_map(|carried| &carried.entries) {
        add_index(&mut tar, entry, copy, &ctx)?;
    }
    stream::append_end(&mut tar)?;
    tar.into_inner()?.flush()?;
    Ok(())
}

/// The paths of `have` under `harness/` that are still of the harness's
/// state in `dir`, as `state` says: those a copy carried and the switch
/// doesn't remove.
fn still_state(dir: &Path, state: &State, have: &Have) -> anyhow::Result<Vec<String>> {
    let includes = archiving::in_home(dir, &state.include)?;
    let excluded = archiving::in_home(dir, &state.exclude)?;
    Ok(have
        .keys()
        .filter(|path| {
            let Some(rel) = path.strip_prefix("harness/") else {
                return false;
            };
            let path = dir.join(rel);
            includes.iter().any(|i| path.starts_with(i))
                && !excluded.iter().any(|e| path.starts_with(e))
                && std::fs::symlink_metadata(&path).is_ok()
        })
        .cloned()
        .collect())
}

/// A switch received, not yet put in place: [`Incoming::apply`] does,
/// once the caller has settled the model.
pub struct Incoming {
    pub manifest: Manifest,
    staging: PathBuf,
}

/// What `receive` made of a stream.
pub enum Received {
    /// A copy, in staging: what staging holds now.
    Copied { manifest: Manifest, have: Have },
    /// A switch, ready to be put in place.
    Switched(Incoming),
}

#[cfg(test)]
mod tests;
