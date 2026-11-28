//! `lnk agent receive`: a stream unpacked into staging, then a switch
//! put in place, its connections' links and files with it.

use super::*;
use link_format::WithinExt;

/// Receives a stream from `send` into the agent's staging, its manifest
/// checked first, then by `check` (the caller's: whose it is, an agent of
/// that name here). A copy is put with what staging holds; a switch is
/// put there too, what it says was removed taken out first, and given back
/// to put in place. A switch that fails leaves nothing: its staging is
/// removed. A stream of another format is refused, by its name.
///
/// With `recipient` (a switch, whose receiver said what to seal to), its
/// whole stream is opened before its entries are read. Its final record
/// is authenticated before the staging can become a successful switch.
pub fn receive(
    input: impl Read,
    recipient: Option<&link_secretive::Recipient>,
    check: impl FnOnce(&Manifest) -> anyhow::Result<()>,
) -> anyhow::Result<Received> {
    receive_in(&moving_dir(), input, recipient, check, true)
}

/// [`receive`] into a staging folder in `moving`, a new one seeded if
/// `seeded`.
pub(super) fn receive_in(
    moving: &Path,
    input: impl Read,
    recipient: Option<&link_secretive::Recipient>,
    check: impl FnOnce(&Manifest) -> anyhow::Result<()>,
    seeded: bool,
) -> anyhow::Result<Received> {
    match recipient {
        Some(recipient) => {
            let input =
                link_secretive::stream::Receiver::new(input, recipient, &stream::stream_about())?;
            receive_tar(moving, input, check, seeded)
        }
        None => receive_tar(moving, input, check, seeded),
    }
}

fn receive_tar(
    moving: &Path,
    input: impl Read,
    check: impl FnOnce(&Manifest) -> anyhow::Result<()>,
    seeded: bool,
) -> anyhow::Result<Received> {
    let mut parsed: Option<(Manifest, PathBuf)> = None;
    let unpacked = stream::unpack(input, |value, head| {
        let manifest = read_manifest(value.clone())?;
        check(&manifest)?;
        let dir = staging_in(moving, manifest.agent.id(), seeded)?;
        let record = record_of(&dir);
        match head.phase {
            Phase::Copy => {
                let record = Record {
                    move_id: head.move_id.clone(),
                    at: crate::moved::now(),
                };
                link_files::write_private(
                    &dir.join(RECORD),
                    &link_files::saved::print_json(&record)?,
                )?;
            }
            Phase::Switch => {
                if let Some(record) = record
                    && record.move_id != head.move_id
                {
                    bail!(
                        "the switch is of another move than the copy here: move {} again",
                        manifest.agent.name
                    );
                }
                // What an earlier switch left, and what's gone since the
                // copy (of its harness's state too, which a copy carries).
                for folder in ["connections", "schedule"] {
                    let _ = std::fs::remove_dir_all(dir.join(folder));
                }
                for removed in &head.removed {
                    stream::remove_under(&dir, removed)?;
                }
            }
            Phase::Files => bail!("a stream of files alone isn't an agent's move"),
        }
        parsed = Some((manifest, dir.clone()));
        Ok(dir)
    });
    let (manifest, dir) = match (unpacked, parsed) {
        (Ok(_), Some(parsed)) => parsed,
        (Ok(_), None) => bail!("the stream had no manifest"),
        (Err(err), parsed) => {
            if let Some((manifest, dir)) = parsed
                && manifest.phase == Phase::Switch
            {
                let _ = std::fs::remove_dir_all(dir);
            }
            return Err(
                err.context("receiving the agent (the stream stopped, or held what it may not)")
            );
        }
    };
    match manifest.phase {
        Phase::Switch => Ok(Received::Switched(Incoming {
            manifest,
            staging: dir,
        })),
        _ => {
            let have = held(&dir)?;
            Ok(Received::Copied { manifest, have })
        }
    }
}

/// Removes agent `id`'s staging, and what it holds: a move that failed.
pub fn drop_staging(id: &str) {
    if let Ok(dir) = staging_of(id) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Unpacks a tar stream into `staging`, read to its end. The stream
/// was written by another machine from what a sandbox wrote, so it's
/// hostile: an entry whose path has `..` is skipped and a leading `/`
/// dropped; one reaching outside through a link it unpacked first is
/// refused, as is a hard link to a file outside. Links are unpacked as
/// links, as `links` says (kept, left out, or refused: nothing more
/// unpacked), and a file in a link's place replaces the link. Modes keep
/// their permission bits only (`0o777`): never setuid, setgid or sticky.
pub(super) fn unpack_in(input: impl Read, staging: &Path, links: Links) -> anyhow::Result<()> {
    let mut archive = tar::Archive::new(input);
    // Not preserved, tar-rs sets `mode & 0o777` as it is, without the
    // umask.
    archive.set_preserve_permissions(false);
    archive.set_overwrite(true);
    let staging = &staging.canonicalize().unwrap_or(staging.to_path_buf());
    // Folders last, as `tar::Archive::unpack` puts them, so a folder's
    // mode never keeps what's in it from being written.
    let mut folders = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        match entry.header().entry_type() {
            tar::EntryType::Symlink | tar::EntryType::Link => match links {
                Links::Keep => {}
                Links::LeaveOut => continue,
                Links::Refuse => bail!(
                    "the backup holds a link, {}: not restoring it",
                    entry.path()?.display()
                ),
            },
            tar::EntryType::Directory => {
                folders.push(entry);
                continue;
            }
            _ => {}
        }
        entry.unpack_in(staging)?;
    }
    folders.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
    for mut folder in folders {
        folder.unpack_in(staging)?;
    }
    // Read to the end: the archive stops at its first end block, and the
    // sender, still writing the rest, would find the pipe closed.
    std::io::copy(&mut archive.into_inner(), &mut std::io::sink())?;
    Ok(())
}

/// The manifest in `value`, if it names a harness and an agent, and
/// what it says is plain.
pub(super) fn read_manifest(value: serde_json::Value) -> anyhow::Result<Manifest> {
    link_agent_contract::moving::check_format(&value)?;
    let manifest: Manifest = serde_json::from_value(value).within(format!("reading {MANIFEST}"))?;
    stream::check_move_id(&manifest.move_id)?;
    for removed in &manifest.removed {
        stream::check_path(removed, &["files", "memory", "harness"])?;
    }
    for (connection, state) in &manifest.connections {
        in_box(Path::new("/"), &state.include)
            .and_then(|_| in_box(Path::new("/"), &state.exclude))
            .within(format!(
                "the stream names a path out of {connection:?}'s folder"
            ))?;
    }
    link_machine::machine::check_id(manifest.agent.id())?;
    crate::settings::check_new_name("agents", &manifest.agent.name)?;
    if let Some(owner) = &manifest.agent.owner {
        link_agent_contract::identity::check_owner(owner)?;
    }
    for team in &manifest.agent.teams {
        link_machine::machine::check_id(team)?;
    }
    if let Some(spec) = &manifest.spec {
        ensure!(
            spec.len() as u64 <= link_spec::MAX_BYTES,
            "the agent's spec is over a spec's size"
        );
        link_agent_spec::parse(spec).within("the agent's spec")?;
    }
    ensure!(
        !manifest.harness.is_empty()
            && manifest
                .harness
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "{:?} is not a harness name",
        manifest.harness
    );
    Ok(manifest)
}

impl Incoming {
    /// Puts the agent in place here: its state replaces the one in its home (its
    /// install, if any, stays), its files overwrite the files folder's
    /// (nothing there is deleted), each linked connection's link replaces the
    /// one in its folder, and `settings` take what moves.
    ///
    /// What of its home comes is what `state`, this machine's adapter for
    /// its harness, names, as a restore does ([`put_state`]): never the
    /// state the stream carries, nor anything else that came for the home
    /// (an install, a `.env`).
    ///
    /// What of a connection's folder comes is what `state_of(connection)`, this
    /// machine's own module, names: never the state the stream carries,
    /// which the machine it came from wrote, while the connection's module
    /// here runs what's in that folder outside any sandbox. What each
    /// connection is (its title, secret fields, whether it's linked) is what
    /// `describe(connection)`, this machine's module, says, never what came:
    /// a connection whose name isn't one, or whose module here says nothing,
    /// is left out, and its name returned. The agent's permissions are
    /// the caller's to settle ([`settle_permissions`]). What came is gone
    /// from staging after, whether or not it was put in place: a failure
    /// never leaves a linked device's keys there for a prune.
    pub fn apply(
        self,
        settings: &mut Settings,
        state: &State,
        state_of: impl Fn(&str) -> Option<State>,
        describe: impl Fn(&str) -> Option<link_connection_contract::Describe>,
    ) -> anyhow::Result<Vec<String>> {
        let applied = self.apply_staged(settings, state, state_of, describe);
        remove_staging(&self.staging);
        applied
    }

    fn apply_staged(
        &self,
        settings: &mut Settings,
        state: &State,
        state_of: impl Fn(&str) -> Option<State>,
        describe: impl Fn(&str) -> Option<link_connection_contract::Describe>,
    ) -> anyhow::Result<Vec<String>> {
        collections::check(&stores::Folder(&self.staging))?;
        // Its ID, taken here before anything of it is written: refused
        // when this machine retired it.
        crate::settings::ids::arrive(&settings.name, self.manifest.agent.id())?;
        if let Some(service) = &self.manifest.agent.service {
            crate::settings::ids::SERVICES.arrive(&settings.name, service)?;
        }
        let name = self.manifest.harness.clone();
        put_state(
            &self.staging.join("harness"),
            &settings.box_of(&name),
            state,
        )?;
        // Its state names where its home was on the machine it came from:
        // where it is here.
        let tail = format!(
            "/{}",
            link_files::layout::harness_homes_at(Path::new(""), &settings.name)
                .join(&name)
                .display()
        );
        link_settings::folders::rewrite_arrived(
            &settings.box_of(&name),
            &tail,
            &settings.box_of(&name),
        )?;
        let files = settings.files();
        crate::layout::lay_out(settings)?;
        let incoming_home = self.staging.join("files");
        std::fs::create_dir_all(&incoming_home)?;
        if incoming_home.is_dir() {
            settings.id = Some(crate::settings::ids::workload(self.manifest.agent.id())?);
            settings.service =
                service_after(self.manifest.agent.service.as_deref(), &settings.service);
            if let Some(owner) = &self.manifest.agent.owner {
                settings.owner = Some(owner.clone());
            }
            if let Some(spec) = &self.manifest.spec {
                link_files::create_private_dir(&crate::settings::config_of(&settings.name))?;
                crate::settings::configuration::write_behavior(settings, spec)?;
                crate::schedules::put_record(&settings.name, &self.manifest.schedules)?;
            }
            let stores = stores::Stores::of(settings)?;
            collections::put(&stores.kept(), &stores::Folder(&self.staging))?;
            for collection in link_agent_contract::folder::SEEN {
                let incoming = incoming_home.join(collection.directory());
                if incoming.exists() {
                    std::fs::remove_dir_all(incoming)?;
                }
            }
            merge(&incoming_home, &files)?;
        }
        put_index(&self.staging.join("memory"), |engine| {
            settings.memory_state(engine)
        })?;
        let agent = self.manifest.agent.name.clone();
        let came = self.manifest.connected.duplicate();
        let (connected, left_out) = described_here(came, describe);
        let linked = |c: &str| connected.get(c).is_some_and(|c| c.about.linked);
        put_connections(
            &self.staging,
            self.manifest.connections.keys(),
            &linked,
            &state_of,
            |connection| crate::bridge::state(&agent, connection),
        )?;
        settings.id = Some(crate::settings::ids::workload(self.manifest.agent.id())?);
        settings.service = service_after(self.manifest.agent.service.as_deref(), &settings.service);
        if let Some(owner) = &self.manifest.agent.owner {
            settings.owner = Some(owner.clone());
        }
        settings.harness = Some(name);
        settings.connections = connected;
        if let Some(spec) = &self.manifest.spec {
            link_files::create_private_dir(&crate::settings::config_of(&agent))?;
            crate::settings::configuration::write_behavior(settings, spec)?;
            crate::schedules::put_record(&agent, &self.manifest.schedules)?;
        }
        settings.sandbox = true;
        if settings.files.is_none() {
            settings.files = Some(files);
        }
        Ok(left_out)
    }

    /// Drops what was unpacked, changing nothing: the copy staged here
    /// goes too.
    pub fn discard(self) {
        remove_staging(&self.staging);
    }
}

/// Removes `staging` and what's in it, a folder a stream unpacked
/// without write permission (`0o500`) made writable first, never through
/// a link.
pub(super) fn remove_staging(staging: &Path) {
    fn writable(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::symlink_metadata(dir) else {
            return;
        };
        if !meta.is_dir() {
            return;
        }
        let mode = meta.permissions().mode();
        if mode & 0o700 != 0o700 {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode | 0o700));
        }
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            writable(&entry.path());
        }
    }
    writable(staging);
    let _ = std::fs::remove_dir_all(staging);
}

/// The service an arriving agent acts as: the one its stream names, or,
/// naming none, the one `here` has, whose secrets and model keys are
/// kept by it, never lost to nothing.
pub(super) fn service_after(arriving: Option<&str>, here: &Option<String>) -> Option<String> {
    arriving.map(str::to_string).or_else(|| here.clone())
}

/// The connections that `came`, each as this machine's module describes it
/// (`describe`), and the names of those left out: a name that can't be a
/// connection's, or whose module here says nothing of it.
pub(super) fn described_here(
    came: BTreeMap<String, crate::connections::Connection>,
    describe: impl Fn(&str) -> Option<link_connection_contract::Describe>,
) -> (
    BTreeMap<String, crate::connections::Connection>,
    Vec<String>,
) {
    let mut kept = BTreeMap::new();
    let mut left_out = Vec::new();
    for (name, mut connection) in came {
        let about = crate::connections::check_name(&name)
            .ok()
            .and_then(|_| describe(&name));
        match about {
            Some(about) => {
                connection.about = about;
                kept.insert(name, connection);
            }
            None => left_out.push(name),
        }
    }
    (kept, left_out)
}

/// Puts each linked connection that came (`connections/<connection>` in `staging`)
/// in its folder here, `folder(connection)`: what its state here
/// (`state_of`) names there is replaced, the rest (what its module
/// installed) stays. Only what that state names comes, and no link: the
/// connection's module uses the folder outside any sandbox. A name that
/// isn't a linked connection of the agent's (`linked`), or whose module here
/// doesn't say its state, is ignored.
pub(super) fn put_connections<'a>(
    staging: &Path,
    connections: impl IntoIterator<Item = &'a String>,
    linked: &dyn Fn(&str) -> bool,
    state_of: &dyn Fn(&str) -> Option<State>,
    folder: impl Fn(&str) -> PathBuf,
) -> anyhow::Result<()> {
    for connection in connections {
        if crate::connections::check_name(connection).is_err() || !linked(connection) {
            continue;
        }
        let Some(state) = state_of(connection) else {
            continue;
        };
        let state = &state;
        let dir = folder(connection);
        link_files::create_private_dir(&dir)?;
        let excluded = in_home(&dir, &state.exclude)?;
        remove_paths(&dir, &in_home(&dir, &state.include)?, &excluded)?;
        let incoming = staging.join("connections").join(connection);
        if incoming.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            remove_links(&incoming)?;
            move_state(&incoming, &dir, state)?;
        }
    }
    Ok(())
}

/// Removes every link in the folder `dir`, never following one.
fn remove_links(dir: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            std::fs::remove_file(entry.path())?;
        } else if kind.is_dir() {
            remove_links(&entry.path())?;
        }
    }
    Ok(())
}

/// Removes `includes`, paths in the folder `dir`, and what's under them,
/// but not what's in `keep` (nor the folders holding it). Each name is
/// opened and removed (`unlinkat`) from the folder holding it, never
/// through a link: one on the way to a path, whenever it came, is
/// refused, and one in a path's place, or anywhere under it, is removed,
/// never followed.
pub(super) fn remove_paths(
    dir: &Path,
    includes: &[PathBuf],
    keep: &[PathBuf],
) -> anyhow::Result<()> {
    if includes.is_empty() {
        return Ok(());
    }
    let home = Dir::open(dir).within(format!("opening {}", dir.display()))?;
    for include in includes {
        if include == dir {
            remove_in(&home, dir, keep)?;
        } else if let Some((parent, name)) = holding(&home, dir, include)? {
            remove_at(&parent, name, include, keep)?;
        }
    }
    Ok(())
}

/// Removes what's in the open folder `folder` (at `path`), but not what's
/// in `keep`.
fn remove_in(folder: &Dir, path: &Path, keep: &[PathBuf]) -> anyhow::Result<()> {
    let names = folder
        .names()
        .within(format!("reading {}", path.display()))?;
    for name in names {
        remove_at(folder, &name, &path.join(&name), keep)?;
    }
    Ok(())
}

/// Removes `name` from the folder `parent` (at `path`), and what's under
/// it, but not what's in `keep` (nor the folders holding it).
pub(super) fn remove_at(
    parent: &Dir,
    name: &OsStr,
    path: &Path,
    keep: &[PathBuf],
) -> anyhow::Result<()> {
    if keep.iter().any(|k| k == path) {
        return Ok(());
    }
    let removing = || format!("removing {}", path.display());
    match parent.kind(name) {
        Ok(Kind::Folder) => {}
        // A file or a link: removed, never followed.
        Ok(_) => return gone(parent.remove_file(name), path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).within(removing()),
    }
    let folder = match parent.folder(name) {
        Ok(folder) => folder,
        // No longer a folder (a link since): removed as it is.
        Err(err) if nofollow::is_link(&err) || err.raw_os_error() == Some(libc::ENOTDIR) => {
            return gone(parent.remove_file(name), path);
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).within(removing()),
    };
    remove_in(&folder, path, keep)?;
    if keep.iter().any(|k| k.starts_with(path)) {
        return Ok(());
    }
    gone(parent.remove_folder(name), path)
}

/// A removal of `path`, done or already gone.
pub(super) fn gone(removed: std::io::Result<()>, path: &Path) -> anyhow::Result<()> {
    match removed {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        removed => removed.within(format!("removing {}", path.display())),
    }
}

/// Moves what's in `from`, a folder of Link's own, into `to`, the files
/// folder, folder by folder, replacing what's in the way. The harness
/// writes `to`, and a process of it may outlive `lnk agent stop`: it's
/// walked from folder handles, as
/// [`move_paths`](super::restoring::move_paths) walks a home, so a link
/// in `to`, whenever it came, is replaced, never followed.
pub(super) fn merge(from: &Path, to: &Path) -> anyhow::Result<()> {
    let source = Dir::open(from).within(format!("opening {}", from.display()))?;
    let target = Dir::open(to).within(format!("opening {}", to.display()))?;
    move_in(&source, from, &target, to, &[])
}
