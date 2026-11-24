//! Which agent a turn is for, and where everything of it is. A turn is
//! always handed its agent: nothing reaches an agent's folders but
//! through [`Agents::paths`], the one place to audit when many agents
//! share a process.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{bail, ensure};

use crate::tools::Server;
pub use link_agent_contract::folder::Folder;

/// An agent: who it is (its ID), whose (its owner), and its name, for
/// people. Two agents are the same only when their IDs and owners are:
/// every owner has a `main`, so nothing is keyed by the name.
#[derive(Clone, Debug)]
pub struct Agent {
    id: String,
    owner: String,
    name: String,
    /// The service it acts as, its bare ID: empty, none was given.
    service: String,
}

impl Agent {
    /// Agent `name`, standing in for its own ID and owner: for a harness
    /// started without them (`LNK_AGENT_ID`, `LNK_OWNER`), which runs
    /// one agent, and tests.
    pub fn new(name: &str) -> anyhow::Result<Self> {
        link_machine::machine::check_name(name)?;
        Ok(Self {
            id: name.to_string(),
            owner: String::new(),
            name: name.to_string(),
            service: String::new(),
        })
    }

    /// Agent `name`, with ID `id`, owned by `owner`.
    pub fn with_identity(name: &str, id: &str, owner: &str) -> anyhow::Result<Self> {
        link_machine::machine::check_name(name)?;
        link_machine::machine::check_id(id)?;
        link_agent_contract::identity::check_owner(owner)?;
        Ok(Self {
            id: id.to_string(),
            owner: owner.to_string(),
            name: name.to_string(),
            service: String::new(),
        })
    }

    /// It, acting as service `service` (its bare ID).
    pub fn acting_as(self, service: &str) -> anyhow::Result<Self> {
        link_machine::machine::check_id(service)?;
        Ok(Self {
            service: service.to_string(),
            ..self
        })
    }

    /// The service it acts as, its bare ID: empty, none was given.
    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// What it's keyed by: its owner and its ID.
    fn key(&self) -> (&str, &str) {
        (&self.owner, &self.id)
    }
}

impl PartialEq for Agent {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for Agent {}

impl std::hash::Hash for Agent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state)
    }
}

impl PartialOrd for Agent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Agent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

/// A conversation: its connection and chat (`telegram:123456789`), as the
/// ACP `Said::thread` metadata names it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Thread(String);

/// The thread of a message that names none.
pub const DEFAULT_THREAD: &str = link_connection_contract::acp::DEFAULT_THREAD;

impl Thread {
    /// `name`, if it's a thread's: letters, digits, `-`, `_`, `.` and `~`
    /// (a copy's, `link:main~hermes`), with one `:` between the connection
    /// and its chat, starting with a
    /// letter or digit, at most 128 long. Never a path.
    pub fn new(name: &str) -> anyhow::Result<Self> {
        let plain = (1..=128).contains(&name.len())
            && name.starts_with(|c: char| c.is_ascii_alphanumeric())
            && name.matches(':').count() <= 1
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:~".contains(&b));
        ensure!(
            plain,
            "{name:?} isn't a thread (such as telegram:123456789)"
        );
        Ok(Self(name.to_string()))
    }

    pub fn name(&self) -> &str {
        &self.0
    }

    /// Its folder's name: the thread with `-` for `:`
    /// (`telegram-123456789`).
    pub fn folder(&self) -> String {
        self.0.replace(':', "-")
    }
}

/// Where an agent's things are. Only its store
/// ([`crate::store::Files`]) reads and writes them.
#[derive(Clone, Debug, PartialEq)]
pub struct Paths {
    /// Its workspace: the folder its tools work in, the agent's Home
    /// (`~/Link/Agents/<agent>/home`).
    pub workspace: PathBuf,
    /// Its own folder (`~/Link/Agents/<agent>/agent`): each of its
    /// collections kept at `file`, the files its prompt is made of among
    /// them (`instructions/instruction.md`).
    pub own: PathBuf,
    /// A folder per conversation (`<own>/chats`).
    pub conversations: PathBuf,
    /// Its settings, read at each turn (`<home>/config.json`).
    pub config: PathBuf,
    /// Each tool's list of calls as the harness last listed it, derived
    /// (`<home>/tool-lists.json`).
    pub tool_lists: PathBuf,
}

impl Paths {
    /// Its folder: Home, its workspace, and its own.
    pub fn folder(&self) -> Folder {
        Folder {
            home: self.workspace.clone(),
            agent: self.own.clone(),
        }
    }

    /// Conversation `thread`'s folder.
    pub fn conversation(&self, thread: &Thread) -> PathBuf {
        self.conversations.join(thread.folder())
    }
}

/// The agents this process answers for, each with its harness's home,
/// its folder (Home and its own) and the tools it may call.
#[derive(Clone, Debug, Default)]
pub struct Agents {
    places: BTreeMap<Agent, (PathBuf, Folder)>,
    tools: BTreeMap<Agent, Vec<Server>>,
}

impl Agents {
    /// Adds `agent`, whose harness's home is `home` and folder `folder`.
    pub fn add(&mut self, agent: Agent, home: PathBuf, folder: Folder) {
        self.places.insert(agent, (home, folder));
    }

    /// Lets `agent` call the tools of `servers`.
    pub fn allow_tools(&mut self, agent: &Agent, servers: Vec<Server>) {
        self.tools.insert(agent.clone(), servers);
    }

    /// The tools' servers `agent` may call: none for an agent this
    /// process doesn't answer for.
    pub fn tools(&self, agent: &Agent) -> &[Server] {
        match self.places.contains_key(agent) {
            true => self.tools.get(agent).map_or(&[], Vec::as_slice),
            false => &[],
        }
    }

    /// `agent`'s workspace, its Home: none for one this process doesn't
    /// answer for.
    pub fn workspace(&self, agent: &Agent) -> Option<PathBuf> {
        self.places
            .get(agent)
            .map(|(_, folder)| folder.home.clone())
    }

    /// The agents this process answers for named `name`: one per owner.
    pub fn named(&self, name: &str) -> Vec<&Agent> {
        self.places.keys().filter(|a| a.name() == name).collect()
    }

    /// The agents this process answers for whose ID is `id`: one per
    /// owner, never the first found for another owner's.
    pub fn with_id(&self, id: &str) -> Vec<&Agent> {
        self.places.keys().filter(|a| a.id() == id).collect()
    }

    /// Where `agent`'s things are; an error for an agent this process
    /// doesn't answer for.
    pub fn paths(&self, agent: &Agent) -> anyhow::Result<Paths> {
        let Some((home, folder)) = self.places.get(agent) else {
            bail!("no agent {} here", agent.name());
        };
        Ok(Paths {
            workspace: folder.home.clone(),
            own: folder.agent.clone(),
            conversations: folder.of(link_store_contract::storage::Collection::Conversations),
            config: home.join("config.json"),
            tool_lists: home.join("tool-lists.json"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_are_never_paths() {
        let thread = Thread::new("telegram:-100123").unwrap();
        assert_eq!(thread.folder(), "telegram--100123");
        for bad in [
            "",
            "..",
            ".hidden",
            "../main",
            "a/b",
            "telegram:1:2",
            "a b",
            "a\0",
            &"x".repeat(129),
        ] {
            assert!(Thread::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_its_own_agents() {
        let mut agents = Agents::default();
        let main = Agent::new("main").unwrap();
        agents.add(
            main.clone(),
            "/h/main".into(),
            Folder::new("/f/main".as_ref(), None),
        );
        let paths = agents.paths(&main).unwrap();
        assert_eq!(paths.workspace, PathBuf::from("/f/main/home"));
        assert_eq!(paths.own, PathBuf::from("/f/main/agent"));
        assert_eq!(paths.conversations, PathBuf::from("/f/main/agent/chats"));
        assert_eq!(paths.config, PathBuf::from("/h/main/config.json"));
        assert_eq!(paths.tool_lists, PathBuf::from("/h/main/tool-lists.json"));
        assert!(agents.paths(&Agent::new("other").unwrap()).is_err());
        assert!(Agent::new("../main").is_err());
        // Tools are an agent's own too.
        let memory = Server {
            name: "memory".into(),
            url: "http://127.0.0.1:1/_link/mcp/memory-main".into(),
            kept: true,
        };
        agents.allow_tools(&main, vec![memory.clone()]);
        agents.allow_tools(&Agent::new("other").unwrap(), vec![memory.clone()]);
        assert_eq!(agents.tools(&main), [memory]);
        assert!(agents.tools(&Agent::new("other").unwrap()).is_empty());
    }
}
