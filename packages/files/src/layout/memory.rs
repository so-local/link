//! Memory's writable runtime and read-only handoff, apart from its records,
//! and the storage bridges' sockets it reaches.

use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct Locations {
    working: String,
    input_prefix: String,
    input_suffix: String,
}

impl Locations {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        for name in [&self.working, &self.input_prefix, &self.input_suffix] {
            one_name(name)?;
        }
        Ok(())
    }
}

/// How an owner's storage-bridge sockets are named in the runtime folder
/// (`[memory-bridges]`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct Bridges {
    owner_digest: usize,
    socket: String,
}

impl Bridges {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        one_name(&self.socket)?;
        ensure!(
            (8..=64).contains(&self.owner_digest),
            "an owner's digest is 8 to 64 hex characters"
        );
        Ok(())
    }
}

fn one_name(name: &str) -> anyhow::Result<()> {
    relative(Path::new(name))?;
    ensure!(
        Path::new(name).components().count() == 1,
        "one memory service component"
    );
    Ok(())
}

/// One owner's memory service. Its records and log remain outside these paths.
pub struct MemoryService<'a>(&'a Path);

impl<'a> MemoryService<'a> {
    pub fn at(folder: &'a Path) -> Self {
        Self(folder)
    }

    pub fn working(&self) -> PathBuf {
        self.0.join(&LAYOUT.memory_service.working)
    }

    pub fn input(&self, generation: &str) -> anyhow::Result<PathBuf> {
        ensure!(
            valid_generation(generation),
            "a hexadecimal memory generation is required"
        );
        Ok(self.0.join(format!(
            "{}{generation}{}",
            LAYOUT.memory_service.input_prefix, LAYOUT.memory_service.input_suffix
        )))
    }
}

/// Where one owner's memory service reaches its storage bridges, in the
/// runtime folder: each bridge's socket alone in a folder named by its
/// key (`<folder>/<key>/storage.sock`), which the bridge listens at, and
/// memory's service is told (`--bridges`) and checks a binding against. The owner is named by a short digest, so the socket's path
/// fits a socket's limit whatever the owner's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryBridges(PathBuf);

impl MemoryBridges {
    /// `owner`'s, in this process's runtime folder.
    pub fn of(owner: &str) -> Self {
        Self::in_runtime(&Root::Runtime.path(), owner)
    }

    /// `owner`'s, in the runtime folder `root`.
    pub fn in_runtime(root: &Path, owner: &str) -> Self {
        let digest = link_digest::sha256(owner.as_bytes());
        let short = &digest[..LAYOUT.memory_bridges.owner_digest];
        Self(super::Runtime::MemorySockets.at(root).join(short))
    }

    /// The folder memory's service is told (`--bridges`).
    pub fn at(folder: PathBuf) -> Self {
        Self(folder)
    }

    pub fn folder(&self) -> &Path {
        &self.0
    }

    /// The socket of the storage bridge keyed `key`.
    pub fn socket(&self, key: &str) -> PathBuf {
        self.0.join(key).join(&LAYOUT.memory_bridges.socket)
    }
}

/// Resolved paths only. A sandbox may write runtime data or read a generated
/// handoff; it never writes handoffs or agent records.
pub fn in_memory_runtime(path: &Path, hidden: &Path, write: bool) -> bool {
    let root = Hidden::SharedMemory.at(hidden);
    let Ok(rest) = path.strip_prefix(&root) else {
        return false;
    };
    let mut parts = rest.components();
    let (Some(Component::Normal(owner)), Some(Component::Normal(area))) =
        (parts.next(), parts.next())
    else {
        return false;
    };
    let tail: Vec<_> = parts.collect();
    if !tail.iter().all(|part| matches!(part, Component::Normal(_))) {
        return false;
    }
    let folder = root.join(owner);
    let service = MemoryService::at(&folder);
    if path.starts_with(service.working()) {
        return true;
    }
    if write {
        return false;
    }
    tail.is_empty()
        && area.to_str().is_some_and(|name| {
            name.strip_prefix(&LAYOUT.memory_service.input_prefix)
                .and_then(|name| name.strip_suffix(&LAYOUT.memory_service.input_suffix))
                .is_some_and(valid_generation)
        })
}

fn valid_generation(generation: &str) -> bool {
    !generation.is_empty() && generation.bytes().all(|byte| byte.is_ascii_hexdigit())
}
