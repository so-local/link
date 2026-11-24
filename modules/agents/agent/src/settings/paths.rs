//! Where Link keeps what's not one agent's settings: `~/Link`, its
//! agents' folders and services, and `~/.lnk`.

use super::*;
use link_format::WithinExt;

/// The tool the memory of the agent keyed `key` ([`Settings::key`]) is
/// served as.
pub fn memory_tool_of(key: &str) -> String {
    format!("memory-{key}")
}

/// Where the memory service of `owner`'s agents on this machine keeps
/// its socket and what it serves (`~/.lnk/data/agents/memory/<owner>`): in
/// the agents' folder, which every sandbox hides, so only the service's
/// own sandbox and what runs outside reach it. One owner's, never two's.
pub fn memory_service_of(owner: &str) -> PathBuf {
    link_files::layout::Hidden::SharedMemory.path().join(owner)
}

/// What the memory service of `owner`'s agents serves the agent keyed
/// `key` with, when its engine serves many agents from one process.
pub fn memory_record_of(owner: &str, key: &str) -> PathBuf {
    memory_service_of(owner)
        .join("agents")
        .join(format!("{key}.json"))
}

/// Where the one harness of `owner`'s agents on this machine keeps what
/// it serves, when they share one (`agents/policy.toml`'s `shared_harness`):
/// `~/.lnk/data/agents/shared-harnesses/<owner>`, in the agents' folder, which every
/// sandbox hides.
pub fn shared_harness_of(owner: &str) -> PathBuf {
    link_files::layout::Hidden::SharedHarnesses
        .path()
        .join(owner)
}

/// What the shared harness of `owner`'s agents serves the agent keyed
/// `key` with, while it does.
pub fn shared_record_of(owner: &str, key: &str) -> PathBuf {
    shared_harness_of(owner)
        .join("agents")
        .join(format!("{key}.json"))
}

/// Whether `key` is what an agent's services are keyed by
/// ([`Settings::key`]): its ID (which may start with a digit), or its
/// name where it has none.
pub fn is_key(key: &str) -> bool {
    link_machine::machine::check_id(key).is_ok() || link_machine::machine::check_name(key).is_ok()
}

/// Whose agent `name` is, as its identity here says, read alone as
/// [`key_of`] reads its ID: the local owner when they say nobody.
pub fn owner_of(name: &str) -> anyhow::Result<String> {
    let said = link_machine::machine::check_name(name)
        .ok()
        .and_then(|()| super::records::identity(name).ok().flatten())
        .and_then(|identity| identity.owner);
    match said {
        Some(owner) => Ok(owner.text()),
        None => link_agent_contract::identity::local_owner(),
    }
}

/// The ID agent `name`'s settings here say, read alone: none when it
/// has none, or its settings can't be read.
pub fn id_of(name: &str) -> Option<String> {
    super::ids::AGENTS.of(name)
}

/// The service agent `name` acts as, as its settings here say, read
/// alone: its ID, none when it has none or its settings can't be read.
pub fn service_of(name: &str) -> Option<String> {
    super::ids::SERVICES.of(name)
}

/// What agent `name`'s services are keyed by ([`Settings::key`]): the ID
/// its settings here say, read alone (a load reads the secrets too); its name when it has none here. Read once a
/// process while its settings file stays the same file, unchanged: each
/// of its services' names asks.
pub fn key_of(name: &str) -> String {
    use std::os::unix::fs::MetadataExt;
    type Seen = (u64, u64, u64, i64, i64);
    static KEYS: std::sync::Mutex<BTreeMap<String, (Seen, String)>> =
        std::sync::Mutex::new(BTreeMap::new());
    if link_machine::machine::check_name(name).is_err() {
        return name.to_string();
    }
    let path = super::records::identity_path(name);
    let seen = std::fs::metadata(&path)
        .ok()
        .map(|m| (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec()));
    if let Some(seen) = seen
        && let Some((was, key)) = KEYS.lock().ok().and_then(|k| k.get(name).cloned())
        && was == seen
    {
        return key;
    }
    let key = id_of(name).unwrap_or_else(|| name.to_string());
    if let Some(seen) = seen
        && let Ok(mut keys) = KEYS.lock()
    {
        keys.insert(name.to_string(), (seen, key.clone()));
    }
    key
}

/// The Link folder, `~/Link` unless it moved: everything Link keeps for
/// you and your agents.
pub fn root() -> PathBuf {
    link_settings::link_dir()
}

/// `~/Link/Agents`: a folder per agent.
pub fn agents() -> PathBuf {
    link_agent_contract::agents_dir()
}

/// `~/Link/Agents/<name>`: agent `name`, whichever harness runs it.
pub fn agent_dir(name: &str) -> PathBuf {
    agents().join(name)
}

/// `~/Link/Agents/<name>/home`: agent `name`'s default files folder, the
/// one each of its harnesses works in.
pub fn home(name: &str) -> PathBuf {
    agent_dir(name).join("home")
}

/// `~/.lnk/data/agents/<name>/harnesses`: one home per harness of agent `name`.
pub fn boxes(name: &str) -> PathBuf {
    link_harness_contract::harness::homes_of(name)
}

/// `~/Link/Safe/Agents/defaults.toml`: what every agent made on this
/// machine starts with, unless it's given otherwise. A box's setup sets
/// the route (`lnk agent route direct --default`), so its agents are
/// behind their proxy from the start.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Defaults {
    /// The route of a new agent here: `direct`, or a SOCKS upstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
}

impl Defaults {
    pub fn path() -> PathBuf {
        link_files::layout::Home::AgentDefaults.path()
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                link_files::saved::parse(&text).within(format!("reading {}", path.display()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err).within(format!("reading {}", path.display())),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        link_files::saved::write(&path, self).within(format!("writing {}", path.display()))
    }
}

link_files::specification!(Defaults, include_str!("defaults.schema.toml"));

/// The name of an agent's settings file in its folder.
pub const SETTINGS_FILE: &str = "agent.toml";

pub fn configuration_of(name: &str) -> PathBuf {
    link_files::layout::Home::AgentSettings.path().join(name)
}

/// `~/.lnk/safe/agents`: what this machine keeps for its agents while they
/// run (logs, sockets), a folder each.
pub fn agents_config() -> PathBuf {
    link_files::layout::Hidden::Agents.path()
}

/// `~/.lnk/safe/agents/<name>`: what this machine keeps for agent `name`
/// while it runs (its log, its proxy's sockets), where no harness reaches.
pub fn config_of(name: &str) -> PathBuf {
    agents_config().join(name)
}
