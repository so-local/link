//! Root-owned settings, state and installed programs (`exec`) share the same path catalogue.

use super::Kind;
use anyhow::ensure;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
}

impl Os {
    pub fn here() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Root {
    Settings,
    State,
    Exec,
}

impl Root {
    pub fn path_for(self, os: Os) -> PathBuf {
        let settings = &super::LAYOUT.system;
        match self {
            Self::Settings => settings.settings.clone(),
            Self::Exec => settings.exec.clone(),
            Self::State => match os {
                Os::Linux => settings.state.linux.clone(),
                Os::Macos => settings.state.macos.clone(),
            },
        }
    }

    pub fn path(self) -> PathBuf {
        self.path_for(Os::here())
    }

    /// Prepare this trusted shared root without making it private.
    pub fn prepare(self) -> std::io::Result<()> {
        crate::system_dirs::prepare(&self.path())
    }

    /// Additional root-owned names the OS gives the same state.
    pub fn aliases_for(self, os: Os) -> Vec<PathBuf> {
        match (self, os) {
            (Self::State, Os::Linux) => vec![super::LAYOUT.system.state.linux_private.clone()],
            _ => Vec::new(),
        }
    }
}

/// Root-owned control is closed to runs, including control kept in state.
/// State is control unless declared data: besides each control location,
/// every folder already in a state root that no data location is in, so a
/// module's state this catalogue doesn't name is closed too.
pub fn safes() -> Vec<PathBuf> {
    let data: Vec<PathBuf> = System::ALL
        .iter()
        .filter(|location| location.root() == Root::State && location.kind() != Kind::Control)
        .flat_map(|location| location.paths_for(Os::here()))
        .collect();
    let roots: Vec<PathBuf> = std::iter::once(Root::State.path())
        .chain(Root::State.aliases_for(Os::here()))
        .collect();
    let mut safes: Vec<PathBuf> = std::iter::once(Root::Settings.path())
        .chain(
            System::ALL
                .iter()
                .filter(|location| {
                    location.root() == Root::State && location.kind() == Kind::Control
                })
                .flat_map(|location| location.paths_for(Os::here())),
        )
        .collect();
    for path in unnamed(&roots, &data) {
        if !safes.contains(&path) {
            safes.push(path);
        }
    }
    safes
}

/// Each folder or file already in `roots` that none of `data` is in.
pub(super) fn unnamed(roots: &[PathBuf], data: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| std::fs::read_dir(root).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| !data.iter().any(|data| data.starts_with(path)))
        .collect();
    found.sort();
    found
}

/// A path under one of the system roots that its owner declares in a file
/// of its own (`root` and `path`), as a module released apart from Link
/// does: this catalogue names only Link's own. A path that leaves its root
/// is refused as it's read.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "Declared")]
pub struct Place {
    pub root: Root,
    pub path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declared {
    root: Root,
    path: PathBuf,
}

impl TryFrom<Declared> for Place {
    type Error = anyhow::Error;

    fn try_from(declared: Declared) -> anyhow::Result<Self> {
        super::relative(&declared.path)?;
        Ok(Place {
            root: declared.root,
            path: declared.path,
        })
    }
}

/// An owner's own file of places, `text`, read into `T`: its [`Place`]s
/// under Link's system roots and its [`super::HiddenPlace`]s in `~/.lnk`.
/// Built into the owner, so one that doesn't read is a build's mistake.
pub fn places<T: serde::de::DeserializeOwned>(text: &str) -> T {
    toml::from_str(text).unwrap_or_else(|err| panic!("an owner's system places: {err}"))
}

impl Place {
    /// Its path on `os`.
    pub fn path_for(&self, os: Os) -> PathBuf {
        self.root.path_for(os).join(&self.path)
    }

    /// Its path here.
    pub fn path(&self) -> PathBuf {
        self.path_for(Os::here())
    }

    /// The other names the OS gives it (systemd's private state).
    pub fn aliases_for(&self, os: Os) -> Vec<PathBuf> {
        let aliases = self.root.aliases_for(os).into_iter();
        aliases.map(|root| root.join(&self.path)).collect()
    }

    /// Its path on `os`, then its other names.
    pub fn paths_for(&self, os: Os) -> Vec<PathBuf> {
        std::iter::once(self.path_for(os))
            .chain(self.aliases_for(os))
            .collect()
    }
}

macro_rules! locations {
    ($($variant:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
        #[serde(rename_all = "kebab-case")]
        pub enum System { $($variant),+ }

        impl System {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn root(self) -> Root { super::LAYOUT.system.locations[&self].root }
            pub fn kind(self) -> Kind { super::LAYOUT.system.locations[&self].kind }
            pub fn relative(self) -> &'static str { &super::LAYOUT.system.locations[&self].path }
            pub fn at(self, root: &Path) -> PathBuf { root.join(self.relative()) }
            pub fn path_for(self, os: Os) -> PathBuf { self.at(&self.root().path_for(os)) }
            pub fn path(self) -> PathBuf { self.path_for(Os::here()) }
            pub fn paths_for(self, os: Os) -> Vec<PathBuf> {
                std::iter::once(self.path_for(os))
                    .chain(self.aliases_for(os))
                    .collect()
            }
            pub fn aliases_for(self, os: Os) -> Vec<PathBuf> {
                self.root().aliases_for(os).into_iter().map(|root| self.at(&root)).collect()
            }
        }
    };
}

locations!(
    MachineKind,
    WorkerSettings,
    WorkerJournal,
    GuestService,
    BoxEnd,
    BoxUsers,
    CoreProgram,
    SecretProgram,
    SandboxProgram,
    SecretStore,
    Data,
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    settings: PathBuf,
    exec: PathBuf,
    pub(crate) shared_mode: u32,
    state: State,
    locations: BTreeMap<System, Location>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    linux: PathBuf,
    #[serde(rename = "linux-private")]
    linux_private: PathBuf,
    macos: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    root: Root,
    path: String,
    kind: Kind,
}

impl Settings {
    pub fn shared_mode(&self) -> u32 {
        self.shared_mode
    }
    pub(super) fn validate(&self, core: &str) -> anyhow::Result<()> {
        ensure!(
            self.shared_mode & !0o777 == 0
                && self.shared_mode & 0o022 == 0
                && self.shared_mode & 0o711 == 0o711,
            "shared system roots are owner-writable and searchable by each service user, never writable by others"
        );
        for (state, alias) in [
            (&self.state.linux, Some(&self.state.linux_private)),
            (&self.state.macos, None),
        ] {
            let roots: Vec<_> = [&self.settings, &self.exec, state]
                .into_iter()
                .chain(alias)
                .collect();
            for root in &roots {
                ensure!(
                    root.is_absolute()
                        && root.components().all(|part| matches!(
                            part,
                            std::path::Component::RootDir | std::path::Component::Normal(_)
                        )),
                    "a system root is absolute without traversal"
                );
            }
            for (index, root) in roots.iter().enumerate() {
                for other in &roots[index + 1..] {
                    ensure!(
                        !root.starts_with(other) && !other.starts_with(root),
                        "system settings, state and exec have separate roots"
                    );
                }
            }
        }
        ensure!(
            self.locations.len() == System::ALL.len(),
            "every system location is registered"
        );
        for location in self.locations.values() {
            super::relative(Path::new(&location.path))?;
            match location.root {
                Root::Settings => ensure!(
                    location.kind == Kind::Control,
                    "system settings are control"
                ),
                Root::Exec => {
                    ensure!(location.kind == Kind::Exec, "system exec holds code")
                }
                Root::State => {
                    ensure!(location.kind != Kind::Exec, "system code belongs in exec");
                    let owner = Path::new(&location.path)
                        .components()
                        .next()
                        .expect("a relative path");
                    super::check_owner_name(
                        owner.as_os_str().to_str().expect("a TOML owner"),
                        core,
                    )?;
                }
            }
        }
        Ok(())
    }
}
