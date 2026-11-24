//! Persistent locations in Home and Hidden, and ephemeral runtime locations.

mod exec;
mod kind;
mod locations;
pub mod map;
mod memory;
mod outside;
mod owners;
mod runtime;
mod system;
pub use exec::{check_exec_environment, exec_in, exec_variable, harness_exec};
pub use kind::{Kind, Location};
pub use locations::{Hidden, Home, Runtime};
pub use memory::{MemoryBridges, MemoryService, in_memory_runtime};
pub use outside::HiddenPlace;
pub use owners::{
    Owner, Part, owner_folder, owner_of_kind, owners, part_root, reserved, state_locations,
    thing_folders,
};
pub use runtime::Settings as RuntimeSettings;
#[cfg(unix)]
pub(crate) use runtime::guard as guard_runtime;
pub use system::{
    Os as SystemOs, Place as SystemPlace, Root as SystemRoot, Settings as SystemLayout, System,
    places as system_places, safes as system_safes,
};
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

/// The person's folder, implementation state and ephemeral runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    Home,
    Hidden,
    Runtime,
}

impl Root {
    /// A default root beneath an explicit parent, independent of process
    /// overrides: the user home for Home/Hidden, the XDG base for Runtime.
    pub fn default_in(self, parent: &Path) -> PathBuf {
        parent.join(match self {
            Self::Home => &LAYOUT.roots.home,
            Self::Hidden => &LAYOUT.roots.hidden,
            Self::Runtime => &LAYOUT.runtime_root.folder,
        })
    }

    pub fn try_path(self) -> anyhow::Result<PathBuf> {
        match self {
            Self::Home => crate::try_link_dir(),
            Self::Hidden => crate::try_installation_dir(),
            Self::Runtime => runtime::path(),
        }
    }

    pub fn path(self) -> PathBuf {
        match self {
            Self::Home => self
                .try_path()
                .unwrap_or_else(|_| PathBuf::from(&LAYOUT.roots.home)),
            Self::Hidden => crate::installation_dir(),
            Self::Runtime => self.try_path().expect("the user's runtime folder"),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roots {
    pub home: String,
    pub hidden: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub roots: Roots,
    pub boundaries: Boundaries,
    pub temporary: Temporary,
    agents: Agents,
    #[serde(rename = "runtime-root")]
    pub runtime_root: RuntimeSettings,
    installation: exec::Settings,
    #[serde(rename = "owner-files")]
    owner_files: OwnerFiles,
    #[serde(rename = "trusted-read")]
    pub trusted_read: TrustedRead,
    #[serde(rename = "memory-service")]
    memory_service: memory::Locations,
    #[serde(rename = "memory-bridges")]
    memory_bridges: memory::Bridges,
    pub system: SystemLayout,
    pub owners: BTreeMap<String, Owner>,
    pub home: BTreeMap<Home, Location>,
    pub hidden: BTreeMap<Hidden, Location>,
    pub runtime: BTreeMap<Runtime, Location>,
}

/// The protected roots, declared once beside the locations they contain.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Boundaries {
    pub hidden_safe: String,
    pub home_safe: String,
    pub exec: String,
    pub data: String,
    pub core: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Temporary {
    prefix: String,
    pub max_age_seconds: u64,
    pub sweep_period_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct OwnerFiles {
    seen_specs: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TrustedRead {
    pub system_links: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Agents {
    harness_homes: String,
    browser: String,
    desktop: String,
    background: String,
    caches: String,
}

/// One of an agent's own folders in `Hidden::AgentData`, beside its
/// harness homes: runtime, sign-ins and records, never its state.
#[derive(Clone, Copy, Debug)]
pub enum AgentData {
    /// Its browser's profile, with its sign-ins.
    Browser,
    /// Its desktop's display number and cookie.
    Desktop,
    /// Its background commands' records.
    Background,
    /// Its programs' package caches (cargo's, npm's, pip's, go's): its
    /// own, so nothing it downloads reaches what you build.
    Caches,
}

impl AgentData {
    /// Its folder's name.
    pub fn folder(self) -> &'static str {
        match self {
            Self::Browser => &LAYOUT.agents.browser,
            Self::Desktop => &LAYOUT.agents.desktop,
            Self::Background => &LAYOUT.agents.background,
            Self::Caches => &LAYOUT.agents.caches,
        }
    }

    /// Agent `agent`'s folder, Hidden at `hidden`:
    /// `<hidden>/data/agents/<agent>/<folder>`.
    pub fn at(self, hidden: &Path, agent: &str) -> PathBuf {
        Hidden::AgentData.at(hidden).join(agent).join(self.folder())
    }
}

/// The folder of an agent's harness homes in `Hidden::AgentData`.
pub fn harness_homes_folder() -> &'static str {
    &LAYOUT.agents.harness_homes
}

/// `<hidden>/data/agents/<agent>/harnesses`, Hidden at `hidden`: the
/// homes of agent `agent`'s harnesses, one each, named by its harness.
pub fn harness_homes_at(hidden: &Path, agent: &str) -> PathBuf {
    Hidden::AgentData
        .at(hidden)
        .join(agent)
        .join(harness_homes_folder())
}

/// Whether `path` is in a harness's home, or is one, under Hidden at
/// `hidden`: an agent's own ([`harness_homes_at`]), or the one its owner's
/// agents share (`Hidden::SharedHarnesses`, a folder each owner).
/// Its components are compared as they
/// are: resolve both first.
pub fn in_harness_home(path: &Path, hidden: &Path) -> bool {
    let plain = |rest: std::path::Components| {
        rest.into_iter()
            .all(|part| matches!(part, Component::Normal(_)))
    };
    if let Ok(rest) = path.strip_prefix(Hidden::SharedHarnesses.at(hidden)) {
        let mut parts = rest.components();
        return matches!(parts.next(), Some(Component::Normal(_))) && plain(parts);
    }
    let Ok(rest) = path.strip_prefix(Hidden::AgentData.at(hidden)) else {
        return false;
    };
    let mut parts = rest.components();
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(Component::Normal(_)), Some(Component::Normal(homes)), Some(Component::Normal(_)))
            if homes == LAYOUT.agents.harness_homes.as_str()
    ) && plain(parts)
}

/// A unique, visible sibling name for an atomic write. A destination may be
/// on another filesystem, so staging remains beside it and keeps this name.
/// Drawn at random, since a process ID repeats in every PID namespace: two
/// sandboxes writing one folder would otherwise share a name, and one's
/// write land in the other's file. Panics only if the system has no
/// randomness, which leaves no name safe to use.
pub fn temporary_name() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let random = link_id::candidate().expect("the system's randomness, for a temporary name");
    format!(
        "{}{random}-{}-{}",
        LAYOUT.temporary.prefix,
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

pub fn is_temporary(name: &str) -> bool {
    name.starts_with(&LAYOUT.temporary.prefix)
}

/// Every fixed location has one built-in home.
pub static LAYOUT: LazyLock<Layout> = LazyLock::new(|| {
    let layout: Layout =
        toml::from_str(include_str!("../../layout.toml")).expect("the built-in file layout");
    visible(Path::new(&layout.temporary.prefix)).expect("a visible temporary prefix");
    relative(Path::new(&layout.roots.hidden)).expect("a relative Hidden root");
    visible(Path::new(&layout.roots.home)).expect("a visible Home root");
    assert!(
        layout.roots.hidden.starts_with('.')
            && Path::new(&layout.roots.hidden).components().count() == 1,
        "Hidden is one hidden root in the user's home"
    );
    assert_eq!(
        layout.home.len(),
        Home::ALL.len(),
        "every Home location is registered"
    );
    assert_eq!(
        layout.hidden.len(),
        Hidden::ALL.len(),
        "every Hidden location is registered"
    );
    for area in Home::ALL {
        visible(Path::new(&layout.home[area].path)).expect("a visible Home location");
    }
    for area in Hidden::ALL {
        relative(Path::new(&layout.hidden[area].path)).expect("a relative Hidden location");
    }
    layout.validate().expect("the built-in trust boundaries");
    assert_eq!(
        Path::new(&layout.agents.harness_homes).components().count(),
        1,
        "the harnesses' homes are one folder in an agent's"
    );
    visible(Path::new(&layout.agents.harness_homes)).expect("a visible harness homes folder");
    layout
        .memory_service
        .validate()
        .expect("memory service locations");
    layout.memory_bridges.validate().expect("memory bridges");
    layout
});

impl Layout {
    /// Refuse misplaced control or code before a location is ever used.
    pub fn validate(&self) -> anyhow::Result<()> {
        for alias in &self.trusted_read.system_links {
            ensure!(alias.is_absolute(), "a system alias is absolute");
            ensure!(
                alias
                    .components()
                    .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
                "a system alias contains no traversal"
            );
        }
        ensure!(
            self.temporary.max_age_seconds > 0 && self.temporary.sweep_period_seconds > 0,
            "temporary-file age and sweep period are positive"
        );
        for boundary in [
            &self.boundaries.hidden_safe,
            &self.boundaries.home_safe,
            &self.boundaries.exec,
            &self.boundaries.data,
            &self.boundaries.core,
        ] {
            relative(Path::new(boundary))?;
            ensure!(
                Path::new(boundary).components().count() == 1,
                "a boundary is one folder"
            );
        }
        let hidden_roots = [
            &self.boundaries.hidden_safe,
            &self.boundaries.exec,
            &self.boundaries.data,
        ];
        ensure!(
            hidden_roots
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == hidden_roots.len(),
            "Safe, exec and data are separate roots"
        );
        for location in self.home.values() {
            visible(Path::new(&location.path))?;
            let in_safe = Path::new(&location.path).starts_with(&self.boundaries.home_safe);
            ensure!(
                (location.kind == Kind::Control) == in_safe,
                "{} has the wrong Home trust boundary",
                location.path
            );
            ensure!(
                location.kind != Kind::Exec,
                "programs belong in Hidden's exec"
            );
        }
        for location in self.hidden.values() {
            relative(Path::new(&location.path))?;
            let expected = match location.kind {
                Kind::Control => &self.boundaries.hidden_safe,
                Kind::Exec => &self.boundaries.exec,
                Kind::Data => &self.boundaries.data,
            };
            ensure!(
                Path::new(&location.path).starts_with(expected),
                "{} is outside its {} boundary",
                location.path,
                expected
            );
            ensure!(
                Path::new(&location.path).components().count() >= 2,
                "a Hidden location has an owner"
            );
        }
        ensure!(
            self.runtime.len() == Runtime::ALL.len(),
            "every runtime location is registered"
        );
        for location in self.runtime.values() {
            relative(Path::new(&location.path))?;
            ensure!(
                location.kind == Kind::Data,
                "runtime holds no control or program"
            );
        }
        self.runtime_root.validate()?;
        self.installation.validate()?;
        self.system.validate(&self.boundaries.core)?;
        self.check_owners()?;
        relative(Path::new(&self.owner_files.seen_specs))?;
        ensure!(
            Path::new(&self.owner_files.seen_specs).components().count() == 1,
            "an owner's specs file is one name"
        );
        Ok(())
    }
}

/// Safes and `exec` exist before an untrusted child can plant them, and every
/// location is pinned in the map.
pub fn prepare() -> anyhow::Result<()> {
    let hidden = crate::try_installation_dir()?;
    let safe = hidden.join(&LAYOUT.boundaries.hidden_safe);
    if let Ok(metadata) = std::fs::symlink_metadata(&safe) {
        ensure!(
            !metadata.file_type().is_symlink(),
            "{} must be a real folder",
            safe.display()
        );
    }
    crate::create_private_dir(&safe)?;
    map::pin(&crate::try_home_dir()?, &hidden)?;
    prepare_at(&hidden, &crate::try_link_dir()?)?;
    Ok(())
}

/// Prepare explicit roots without reading or modifying process inputs.
pub fn prepare_at(hidden: &Path, home: &Path) -> anyhow::Result<()> {
    // Home may have been moved by its owner. Startup must not recreate it
    // before the folder command can adopt the moved tree.
    let home_safe = home.try_exists()?.then(|| Home::Safe.at(home));
    for path in [
        hidden.join(&LAYOUT.boundaries.hidden_safe),
        hidden.join(&LAYOUT.boundaries.exec),
    ]
    .into_iter()
    .chain(home_safe)
    {
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            ensure!(
                !metadata.file_type().is_symlink(),
                "{} must be a real folder",
                path.display()
            );
        }
        crate::create_private_dir(&path)
            .with_context(|| format!("protecting {}", path.display()))?;
        ensure!(
            !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "{} must be a real folder",
            path.display()
        );
    }
    Ok(())
}

/// One owner's protected control folder, `~/.lnk/safe/<owner>`, by the
/// owner's word in `[owners]` (`sandboxes`, `boxes`).
pub fn control(owner: &str) -> anyhow::Result<PathBuf> {
    Ok(crate::try_installation_dir()?
        .join(&LAYOUT.boundaries.hidden_safe)
        .join(owner_folder(owner, Part::Safe)?))
}

/// The specs observed by their owner, kept in its Safe.
pub fn seen_specs(owner: &str) -> anyhow::Result<PathBuf> {
    Ok(control(owner)?.join(&LAYOUT.owner_files.seen_specs))
}

pub fn check_owner(owner: &str) -> anyhow::Result<()> {
    check_owner_name(owner, &LAYOUT.boundaries.core)
}

fn check_owner_name(owner: &str, core: &str) -> anyhow::Result<()> {
    ensure!(
        !owner.is_empty()
            && owner != core
            && owner
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && owner.as_bytes()[0].is_ascii_lowercase(),
        "{owner:?} is not a module owner"
    );
    Ok(())
}

fn relative(path: &Path) -> anyhow::Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "{} is not a relative layout path",
        path.display()
    );
    Ok(())
}

/// A visible relative path, refusing traversal and hidden components.
pub fn visible(path: &Path) -> anyhow::Result<()> {
    relative(path)?;
    ensure!(
        path.components()
            .all(|part| !part.as_os_str().as_encoded_bytes().starts_with(b".")),
        "{} is a hidden path in Home",
        path.display()
    );
    Ok(())
}

/// Find the Link folder through the map in `hidden`, relative to the user's
/// home folder, the map read and checked once while its file is unchanged.
pub fn link_in(user_home: &Path, hidden: &Path) -> anyhow::Result<PathBuf> {
    Ok(map::load_cached(user_home, hidden)?.link_folder(user_home))
}

/// The marker persists with Home when it moves.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub id: String,
}

crate::specification!(Marker, include_str!("marker.schema.toml"));

/// Read Home's marker without changing the locator.
pub fn read_marker(home: &Path) -> anyhow::Result<Option<Marker>> {
    let current = Home::Marker.at(home);
    crate::saved::read_optional(&current)
}
