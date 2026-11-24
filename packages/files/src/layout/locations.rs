//! Typed locations in Home and Hidden. Identifiers stay stable when paths move.

use super::*;

macro_rules! locations {
    ($kind:ident, $field:ident, $root:ident, $($variant:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $kind { $($variant),+ }

        impl $kind {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn kind(self) -> Kind { LAYOUT.$field[&self].kind }

            /// The word in `[owners]` that keeps it, or `link` for Link's own.
            pub fn owner(self) -> &'static str { &LAYOUT.$field[&self].owner }

            /// The layout's default path relative to this location's root.
            pub fn relative(self) -> &'static str { &LAYOUT.$field[&self].path }

            /// Its stable ID in `layout.toml`, by which the map pins it.
            pub fn id(self) -> String { kebab(&format!("{self:?}")) }

            /// The layout's default under an explicit root, as a fresh
            /// machine has it, without consulting the map.
            pub fn at_default(self, root: &Path) -> PathBuf { root.join(self.relative()) }

            /// This process's location, refusing an unavailable root.
            pub fn try_path(self) -> anyhow::Result<PathBuf> { Ok(self.at(&Root::$root.try_path()?)) }

            /// This process's location, with the same missing-HOME fallback as its root.
            pub fn path(self) -> PathBuf { self.at(&Root::$root.path()) }
        }
    }
}

locations!(
    Home,
    home,
    Home,
    Agents,
    Teams,
    Boxes,
    Files,
    Archive,
    Logs,
    Safe,
    Marker,
    Settings,
    AgentSettings,
    AgentDefaults,
    TeamSpecs,
    Models,
    SandboxPresets
);

locations!(
    Hidden,
    hidden,
    Hidden,
    Bin,
    Map,
    Settings,
    Installation,
    Maintenance,
    AgentPolicy,
    SecretPreferences,
    MigrationJournal,
    StoppedServices,
    AgentPlaces,
    Agents,
    AgentData,
    AgentMachines,
    AgentEnvironments,
    AgentIds,
    AgentServices,
    Context,
    ModelContext,
    Shims,
    Mktemp,
    Configured,
    SharedMemory,
    SharedHarnesses,
    TeamMemory,
    Sending,
    Receiving,
    Restoring,
    TeamMoving,
    Moved,
    Moving,
    Held,
    TransferScratch,
    HarnessHistory,
    AgentMove,
    HarnessGrants,
    Teams,
    Claude,
    Codex,
    OpenclawDoctorRelease,
    ClaudePrograms,
    CodexPrograms,
    DeepseekPrograms,
    OpenclawPrograms,
    HermesPrograms,
    Installations,
    SharedBridges,
    Jobs,
    MeasureLog,
    Boxes,
    BoxMachines,
    Storage,
    Clouds,
    Envs,
    Gcp,
    BundleSigners,
    Bundles,
    Measures,
    ModelFacts,
    ModuleOffers,
    Approvals,
    Sandboxes,
    SandboxGrants,
    Secrets,
    Update,
    StoreScratch,
    Prices,
    Aws,
    AgentCredentials,
    MeasureCredentials,
    Releases,
    Artifacts,
    HarnessMeasureLog,
    CoreProgram,
    Modules,
    Services,
    ArchiveStaging,
    Upgrades
);

fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if index > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl Home {
    /// This location in the Link folder `home`, where this machine's map
    /// puts it when `home` is the Link folder the map names, else the
    /// layout's default.
    pub fn at(self, home: &Path) -> PathBuf {
        let mapped = crate::try_home_dir()
            .ok()
            .zip(crate::try_installation_dir().ok())
            .and_then(|(user_home, hidden)| {
                let map = super::map::cached(&hidden);
                (map.link_folder(&user_home) == home).then_some(map)
            });
        match mapped {
            Some(map) => resolved(home, map.relative(super::map::Entry::Home(self))),
            None => self.at_default(home),
        }
    }
}

impl Hidden {
    /// This location in installation folder `hidden`, where the map there puts
    /// it. The map itself is at its fixed path.
    pub fn at(self, hidden: &Path) -> PathBuf {
        if self == Hidden::Map {
            return self.at_default(hidden);
        }
        let map = super::map::cached(hidden);
        resolved(hidden, map.relative(super::map::Entry::Hidden(self)))
    }

    /// A path relative to an explicit user's home, for remote recipes and traces.
    pub fn relative_to_home(self) -> PathBuf {
        self.in_home(Path::new(""))
    }

    /// State in an explicit isolated home, independent of LNK_INSTALLATION_DIR.
    pub fn in_home(self, home: &Path) -> PathBuf {
        self.at(&home.join(&LAYOUT.roots.hidden))
    }
}

fn resolved(root: &Path, entry: &str) -> PathBuf {
    if Path::new(entry).is_absolute() {
        PathBuf::from(entry)
    } else {
        root.join(entry)
    }
}

impl Runtime {
    /// This location under the runtime folder `root`.
    pub fn at(self, root: &Path) -> PathBuf {
        self.at_default(root)
    }
}

locations!(
    Runtime,
    runtime,
    Runtime,
    ProgramStaging,
    AgentNetwork,
    SharedTools,
    Bridges,
    Checks,
    Sandbox,
    MemorySockets,
    Displays
);
