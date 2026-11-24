//! What every bridge shares is [`link_sandbox_bridge`], here as it is: who's
//! calling ([`Identity`]), its proof ([`proof`]), the one error envelope
//! ([`Failure`]), the server every bridge runs on ([`serve`]) and a call's
//! two sides ([`take`], [`call`]). What this adds is where the keys are.
//!
//! **The key** is the agent's, while it's bound ([`Binding`]): made at
//! each start on someone's own machine (the endpoint's key, which the
//! harness already holds, the harness endpoint key),
//! and per binding on a machine that runs an agent only while it answers
//! (the bound unit). A bridge looks the key up in the [`Keyring`], where the
//! start, or the machine binding a unit, puts it, outside every sandbox; a
//! key taken out of it proves nothing any more. The endpoint's own proof (the harness endpoint proof)
//! is the same thing the other way, the harness proving itself to a
//! bridge, under a label of its own, so neither is ever the other.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// A binding as its file keeps it (`<agent id>.json`), naming its schema.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Kept(Binding);

link_files::specification!(Kept, include_str!("binding.schema.toml"));

pub use link_core::*;
pub use link_sandbox_bridge::serve::serve_streams_from;

pub use link_sandbox_bridge::{
    BUSY_TRIES, Binding, Both, CALL_MAX, CALL_WAIT, CHALLENGE_WAIT, Challenge, Failure, Identity,
    Keys, Limits, PEER_PROVING, PEERS, PROOF_WAIT, PROVEN_MAX, PROVING, Proven, Why, answer_of,
    call, call_async, call_at, check_owner, proof, proof_matches, read_answer, read_answer_async,
    read_line_async, retrying, same_agent, serve, serve_streams, serve_with, stream, take,
    take_async, write_line_async,
};

/// The agents bound on this machine and their keys, a file each
/// (`<agent ID>.json`, a [`Binding`]) in a folder only the user reaches,
/// outside every sandbox (`~/.lnk/safe/agents/credentials`): what every
/// bridge here checks a call's proof against. Binding writes an agent's
/// file; unbinding removes it, and a key no longer in it proves nothing.
///
/// A [`Keyring::passable`] one is for a machine whose helpers each read
/// one agent's key alone: its folder `0711`, so no one but its user lists
/// it, and an agent bound with [`Keyring::bind_shared`] has its file
/// readable by one group too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyring {
    dir: PathBuf,
    /// Its folder `0711`, not `0700`.
    passable: bool,
}

impl Keyring {
    /// This machine's: `~/.lnk/safe/agents/credentials`.
    pub fn here() -> Self {
        Self::at(&link_files::layout::Hidden::AgentCredentials.path())
    }

    /// One in `dir`.
    pub fn at(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            passable: false,
        }
    }

    /// One in `dir`, its folder `0711`: its user lists it and no one else
    /// does, but anyone may open a file in it that its mode lets them,
    /// one [`Keyring::bind_shared`] made for their group.
    pub fn passable(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            passable: true,
        }
    }

    /// Its folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Binds `identity` with `secret`, in place of whatever key it had.
    pub fn bind(&self, identity: &Identity, secret: &Secret) -> anyhow::Result<Binding> {
        self.bind_for(identity, secret, None)
    }

    /// Holds `identity`'s binding for one run, as long as the [`Held`]
    /// lives. One something else bound (a start) is used as it is, and
    /// never unbound by a run. With none, it's bound with `secret` for
    /// runs, and every run held meanwhile shares that binding, each
    /// keeping a shared lock on its file of holders (`<ID>.held`): the
    /// last to let go unbinds it, through `revoker`, unless a bind since
    /// replaced it. A run killed before it lets go leaves the binding to
    /// the next run, which shares it and unbinds it as the last. Each step
    /// is under the keyring's lock.
    pub fn hold(
        &self,
        identity: &Identity,
        secret: &Secret,
        revoker: impl Revoker + Send + 'static,
    ) -> anyhow::Result<Held> {
        let binding = Self::binding(identity, secret)?;
        let (path, holders) = self.paths(&binding);
        let _locked = self.lock()?;
        let shared = match Self::kept_at(&path) {
            Some(now) => match std::fs::File::open(&holders) {
                // Bound for runs: shared with them.
                Ok(file) => Some((now, share(file)?)),
                // Bound by something else: used as it is.
                Err(_) => None,
            },
            None => {
                let _ = std::fs::remove_file(&holders);
                let file = {
                    use std::os::unix::fs::OpenOptionsExt;
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&holders)
                        .with_context(|| format!("writing {}", holders.display()))?
                };
                let file = share(file)?;
                if let Err(err) = self.write(&path, &binding, None) {
                    let _ = std::fs::remove_file(&holders);
                    return Err(err);
                }
                Some((binding, file))
            }
        };
        Ok(Held {
            keyring: self.clone(),
            shared,
            revoker: Box::new(revoker),
        })
    }

    /// Binds `identity` with `secret`, as [`Keyring::bind`] does, its
    /// file readable by group `gid` too (`0640`), and by no one else: a
    /// helper of the agent's, in that group alone, reads its key. A bind
    /// again sets its group again, so a file moves whole from one group to
    /// another; unbinding removes it. Its mode and group are the file's
    /// before its key is in it.
    pub fn bind_shared(
        &self,
        identity: &Identity,
        secret: &Secret,
        gid: u32,
    ) -> anyhow::Result<Binding> {
        self.bind_for(identity, secret, Some(gid))
    }

    /// Binds `identity` with `secret`, its file of group `group` when it
    /// has one, in place of whatever binding it had: one runs held
    /// ([`Keyring::hold`]) too, which they never unbind from now on.
    fn bind_for(
        &self,
        identity: &Identity,
        secret: &Secret,
        group: Option<u32>,
    ) -> anyhow::Result<Binding> {
        let binding = Self::binding(identity, secret)?;
        let (path, holders) = self.paths(&binding);
        let _locked = self.lock()?;
        let _ = std::fs::remove_file(&holders);
        self.write(&path, &binding, group)?;
        Ok(binding)
    }

    /// `identity` bound with `secret`, checked: its ID, owner and key.
    fn binding(identity: &Identity, secret: &Secret) -> anyhow::Result<Binding> {
        check_agent(&identity.agent)?;
        anyhow::ensure!(
            !identity.owner.is_empty(),
            "agent {} has no owner, so it can't be bound: lnk agent start settles one",
            identity.agent
        );
        link_sandbox_bridge::check_owner(&identity.owner)?;
        anyhow::ensure!(
            secret.expose().len() >= 32,
            "a bridge key is at least 32 characters"
        );
        Ok(Binding {
            identity: Identity {
                teams: Vec::new(),
                ..identity.clone()
            },
            key: secret.duplicate(),
        })
    }

    /// The files of `binding`'s ID: its binding, and its runs' holders.
    fn paths(&self, binding: &Binding) -> (PathBuf, PathBuf) {
        let path = self.path(&binding.identity.agent);
        (path.clone(), path.with_extension("held"))
    }

    /// `binding` written to `path`, its file of group `group` when it has
    /// one, with the keyring's lock held.
    fn write(&self, path: &Path, binding: &Binding, group: Option<u32>) -> anyhow::Result<()> {
        let bytes = link_files::saved::print_json(&Kept(binding.duplicate()))?;
        match group {
            None => link_files::write_private(path, &bytes),
            Some(gid) => write_shared(path, &bytes, gid),
        }
        .with_context(|| format!("writing {}", path.display()))
    }

    /// Unbinds agent `agent` (its ID), whatever its key: it proves nothing
    /// from now on (its start stopped, or it was removed). Nothing for one
    /// not bound.
    pub fn unbind(&self, agent: &str, revoker: &impl Revoker) {
        if check_agent(agent).is_ok() {
            revoker.unbind(agent, None);
            let _locked = self.lock();
            let _ = std::fs::remove_file(self.path(agent));
        }
    }

    /// Unbinds `binding`, only while the agent is bound with its key: a
    /// binding made since, with a key of its own, stays.
    pub fn unbind_key(&self, binding: &Binding, revoker: &impl Revoker) {
        if check_agent(&binding.identity.agent).is_err() {
            return;
        }
        let Ok(_locked) = self.lock() else {
            return;
        };
        self.unbind_locked(binding, revoker);
    }

    /// Unbinds `binding` as [`Keyring::unbind_key`] does, with the
    /// keyring's lock held.
    fn unbind_locked(&self, binding: &Binding, revoker: &dyn Revoker) {
        let (path, _) = self.paths(binding);
        if Self::kept_at(&path).is_some_and(|now| now.key == binding.key) {
            revoker.unbind(&binding.identity.agent, Some(&binding.key));
            let _ = std::fs::remove_file(&path);
        }
    }

    /// The keyring's lock, held while a binding is written or removed.
    fn lock(&self) -> anyhow::Result<std::fs::File> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        self.folder()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(self.dir.join(".lock"))?;
        // SAFETY: flock takes only the open file's descriptor.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error()).context("locking the keyring");
        }
        Ok(file)
    }

    /// Agent `agent`'s binding (its ID), if it's bound.
    pub fn bound(&self, agent: &str) -> Option<Binding> {
        check_agent(agent).ok()?;
        Self::kept_at(&self.path(agent)).filter(|binding| binding.identity.agent == agent)
    }

    /// The binding the file at `path` keeps, if it keeps one.
    fn kept_at(path: &Path) -> Option<Binding> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(64 << 10)
            .read_to_end(&mut bytes)
            .ok()?;
        link_files::saved::parse_json::<Kept>(&bytes)
            .ok()
            .map(|kept| kept.0)
    }

    /// Whether it binds no one: then no call can be proved against it.
    pub fn binds_no_one(&self) -> bool {
        std::fs::read_dir(&self.dir).map_or(true, |entries| {
            !entries
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|e| e == "json"))
        })
    }

    fn path(&self, agent: &str) -> PathBuf {
        self.dir.join(format!("{agent}.json"))
    }

    /// Its folder, made if it's missing, its user's alone ([`crate::create_private_dir`]):
    /// `0700`, or `0711` when it's [`Keyring::passable`], set only when
    /// it's another, so a reader never meets it shut.
    fn folder(&self) -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        if !self.passable {
            return Ok(link_files::create_private_dir(&self.dir)?);
        }
        if !self.dir.exists() {
            link_files::create_private_dir(&self.dir)?;
        }
        link_files::check_own_dir(&self.dir)?;
        let mode = std::fs::metadata(&self.dir)?.permissions().mode() & 0o7777;
        if mode != 0o711 {
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o711))?;
        }
        Ok(())
    }
}

/// One run's hold on a binding ([`Keyring::hold`]), let go as it's
/// dropped.
pub struct Held {
    keyring: Keyring,
    /// The binding runs made, and this run's shared lock on its holders'
    /// file; None for one something else bound, used as it is.
    shared: Option<(Binding, std::fs::File)>,
    revoker: Box<dyn Revoker + Send>,
}

impl Drop for Held {
    /// The last run to let go of the binding runs made unbinds it, unless
    /// a bind since replaced it: the one that holds its lock alone.
    fn drop(&mut self) {
        let Some((binding, file)) = self.shared.take() else {
            return;
        };
        let Ok(_locked) = self.keyring.lock() else {
            return;
        };
        if !alone(&file) {
            return;
        }
        let (_, holders) = self.keyring.paths(&binding);
        if same_file(&file, &holders) {
            let _ = std::fs::remove_file(&holders);
        }
        self.keyring.unbind_locked(&binding, self.revoker.as_ref());
    }
}

/// `file` with a shared lock on it, as one of its holders.
fn share(file: std::fs::File) -> anyhow::Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    // SAFETY: flock takes only the open file's descriptor.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) } != 0 {
        return Err(std::io::Error::last_os_error()).context("holding a binding");
    }
    Ok(file)
}

/// Whether `file`'s holder is its only one: its shared lock made
/// exclusive at once, which no other holder's allows.
fn alone(file: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: flock takes only the open file's descriptor.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// Whether `file` is the file at `path` now, not one removed since.
fn same_file(file: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(open), Ok(now)) => open.dev() == now.dev() && open.ino() == now.ino(),
        _ => false,
    }
}

/// `contents` written to `path` by a rename, its file `0640` and of group
/// `gid` before anything's written to it.
fn write_shared(path: &Path, contents: &[u8], gid: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let tmp = path.with_file_name(link_files::layout::temporary_name());
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        std::os::unix::fs::fchown(&file, None, Some(gid))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
        file.write_all(contents)?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

impl Keys for Keyring {
    fn key_of(&self, identity: &Identity) -> Option<Secret> {
        self.bound(&identity.agent)
            .filter(|binding| link_sandbox_bridge::same_agent(&binding.identity, identity))
            .map(|binding| binding.key)
    }
}

/// An agent as a keyring names it: letters, digits, `-` and `_`, at most
/// 64.
pub fn check_agent(agent: &str) -> anyhow::Result<()> {
    let plain = (1..=64).contains(&agent.len())
        && agent.starts_with(|c: char| c.is_ascii_alphanumeric())
        && agent
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    anyhow::ensure!(plain, "{:?} isn't an agent's ID", link_format::clean(agent));
    Ok(())
}

/// Requests grant revocation before removing a binding. This interface
/// returns no result; implementations must report failures through their
/// own diagnostics and cannot certify remote revocation to the keyring.
pub trait Revoker {
    fn unbind(&self, agent: &str, key: Option<&Secret>);
}

use link_secretive::Secret;

#[cfg(test)]
link_test_kit::program!(unit);
