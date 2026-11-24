//! What each server last listed: in this process ([`Listed`]), and kept
//! beside the agent's settings ([`Lists`]).

use super::*;

/// Each server's tools as it last listed them, by agent and server, kept
/// between turns: a turn offers them without starting the server, and
/// a server that fails to start is still offered the same way, so the
/// prompt, and the provider's cache, stay the same. Derived from the
/// servers: a process without them lists them again.
///
/// What each agent's servers listed is kept beside its settings too
/// ([`Lists`]), so a new process whose server fails to list its tools
/// offers them as the last process did, not without them, until a turn
/// lists them again: a saved list never stands for a listing, so a tool
/// added since is offered once its server answers. A server that never
/// listed its tools and failed to, with none kept, is remembered, and
/// left out until [`RETRY_AFTER`] has passed.
///
/// A handle: its clones share what's listed, so a start a turn stopped
/// waiting for, still going ([`Listed::opening`]), lists its server for
/// the turns after.
#[derive(Clone, Default)]
pub struct Listed {
    listed: Arc<std::sync::Mutex<HashMap<(Agent, Server), Listing>>>,
    /// Each agent's lists as kept, read once a process, and whether this
    /// process listed any differently since.
    kept: Arc<std::sync::Mutex<HashMap<Agent, (Lists, bool)>>>,
    /// Each server this process asked for its tools, whatever it answered.
    asked: Arc<std::sync::Mutex<std::collections::HashSet<(Agent, Server)>>>,
    /// Each server's start under way, for every turn that waits for it.
    starting: Arc<std::sync::Mutex<HashMap<(Agent, Server), Opening>>>,
}

/// A server's start under way: how it ended, once it has.
pub(super) type Opening = tokio::sync::watch::Receiver<Option<Outcome>>;

/// How a server's start ended, as each turn waiting for it learns.
#[derive(Clone)]
pub(super) enum Outcome {
    /// It listed its tools: what it offers, and its session, for the
    /// first turn that takes it.
    Opened(Offered, Arc<std::sync::Mutex<Option<Arc<Session>>>>),
    /// It failed: why, and what's offered of it instead
    /// ([`Listed::failed`]).
    Failed(String, Option<Offered>),
}

enum Listing {
    /// As this process listed it.
    Offered(Offered),
    /// As the last process listed it ([`Lists`]), offered while this one
    /// hasn't: asked again at the next turn.
    Saved(Offered),
    /// It failed to start then.
    Failed(std::time::Instant),
}

impl Listed {
    /// What this process listed of `server`: none while only a saved
    /// list stands in for it.
    pub(super) fn get(&self, agent: &Agent, server: &Server) -> Option<Offered> {
        let listed = self.listed.lock().unwrap_or_else(|e| e.into_inner());
        match listed.get(&(agent.clone(), server.clone())) {
            Some(Listing::Offered(offered)) => Some(offered.clone()),
            _ => None,
        }
    }

    pub(super) fn put(&self, agent: &Agent, server: &Server, offered: Offered) {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((lists, changed)) = kept.get_mut(agent)
            && lists.offered(server).as_ref() != Some(&offered)
        {
            *changed = true;
        }
        let mut listed = self.listed.lock().unwrap_or_else(|e| e.into_inner());
        listed.insert((agent.clone(), server.clone()), Listing::Offered(offered));
    }

    /// Whether this ask is this process's first of `server` for its
    /// tools: the first waits for its grant ([`Grant`]), later ones don't,
    /// so a tool its sandbox keeps refusing never holds up every turn.
    pub(super) fn grant(&self, agent: &Agent, server: &Server) -> Grant {
        let mut asked = self.asked.lock().unwrap_or_else(|e| e.into_inner());
        match asked.insert((agent.clone(), server.clone())) {
            true => Grant::Awaited,
            false => Grant::Refused,
        }
    }

    /// The start of `agent`'s `server` under way, begun by `begin` when
    /// none is: a turn that stops waiting for it leaves it going, and the
    /// next turn waits for the same start, never another.
    pub(super) fn opening(
        &self,
        agent: &Agent,
        server: &Server,
        begin: impl FnOnce() -> Opening,
    ) -> Opening {
        let mut starting = self.starting.lock().unwrap_or_else(|e| e.into_inner());
        starting
            .entry((agent.clone(), server.clone()))
            .or_insert_with(begin)
            .clone()
    }

    /// The start of `agent`'s `server` ended: the next is a new one.
    pub(super) fn opened(&self, agent: &Agent, server: &Server) {
        let mut starting = self.starting.lock().unwrap_or_else(|e| e.into_inner());
        starting.remove(&(agent.clone(), server.clone()));
    }

    /// Whether `server` failed to start less than [`RETRY_AFTER`] ago.
    pub(super) fn failed_lately(&self, agent: &Agent, server: &Server) -> bool {
        let listed = self.listed.lock().unwrap_or_else(|e| e.into_inner());
        matches!(
            listed.get(&(agent.clone(), server.clone())),
            Some(Listing::Failed(at)) if at.elapsed() < RETRY_AFTER
        )
    }

    /// `server` failed to list its tools: what's offered of it instead,
    /// what it listed before, in this process or the last. The last's
    /// stands in only until it's asked again, at the next turn.
    pub(super) fn failed(
        &self,
        agent: &Agent,
        server: &Server,
        temporary: bool,
    ) -> Option<Offered> {
        let standing = match self
            .listed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(agent.clone(), server.clone()))
        {
            Some(Listing::Offered(offered) | Listing::Saved(offered)) => Some(offered.clone()),
            _ => None,
        };
        if standing.is_some() {
            return standing;
        }
        let kept = self
            .kept
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(agent)
            .and_then(|(lists, _)| lists.offered(server));
        let mut listed = self.listed.lock().unwrap_or_else(|e| e.into_inner());
        let key = (agent.clone(), server.clone());
        match kept {
            Some(offered) => {
                listed.insert(key, Listing::Saved(offered.clone()));
                Some(offered)
            }
            None => {
                if !temporary {
                    listed.insert(key, Listing::Failed(std::time::Instant::now()));
                }
                None
            }
        }
    }

    /// What sums up a call of `agent`'s tool `name` (the model's name for
    /// it) in its title: the arguments its server named, in order, and
    /// its schema, which says which of them are sensitive. None while its
    /// server is offered only as a process kept it, or names none.
    pub fn summary(&self, agent: &Agent, name: &str) -> Option<(Vec<String>, Value)> {
        let listed = self.listed.lock().unwrap_or_else(|e| e.into_inner());
        listed
            .iter()
            .find_map(|((whose, _), listing)| match listing {
                Listing::Offered(offered) if whose == agent => {
                    let arguments = offered.summaries.get(name)?;
                    let (tool, _) = offered.tools.iter().find(|(tool, _)| tool.name == name)?;
                    Some((arguments.clone(), tool.schema.clone()))
                }
                _ => None,
            })
    }

    /// Whether `agent`'s kept lists were read in this process.
    pub fn read(&self, agent: &Agent) -> bool {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.contains_key(agent)
    }

    /// `agent`'s lists as kept (none: none were), whose servers are now
    /// `servers`: one of another server, or of one since moved, is never
    /// offered, and is dropped when they're kept again. Taken once: two
    /// turns that read them at once keep the first's.
    pub fn read_kept(&self, agent: &Agent, lists: Option<Lists>, servers: &[Server]) {
        let lists = lists.unwrap_or_default();
        let stale = lists
            .servers
            .iter()
            .any(|kept| !servers.iter().any(|server| kept.is_of(server)));
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.entry(agent.clone()).or_insert((lists, stale));
    }

    /// `agent`'s lists to keep, of `servers`, when this process listed any
    /// differently from what's kept: then they're taken as kept.
    pub fn to_keep(&self, agent: &Agent, servers: &[Server]) -> Option<Lists> {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        let (lists, changed) = kept.get_mut(agent).filter(|(_, changed)| *changed)?;
        let listed = self.listed.lock().unwrap_or_else(|e| e.into_inner());
        let now = Lists {
            servers: servers
                .iter()
                .filter_map(
                    |server| match listed.get(&(agent.clone(), server.clone())) {
                        Some(Listing::Offered(offered)) => Some(List::of(server, offered)),
                        _ => lists
                            .servers
                            .iter()
                            .find(|kept| kept.is_of(server))
                            .cloned(),
                    },
                )
                .collect(),
        };
        *lists = now.clone();
        *changed = false;
        Some(now)
    }
}

/// Each server's tools as a process last listed them, kept beside the
/// agent's settings: what a new process offers of a server that fails to
/// list its tools. Derived: when it's missing, or a server's isn't
/// there, the server's tools are only what it lists.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lists {
    pub servers: Vec<List>,
}

/// One server's tools, as [`Lists`] keeps them.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct List {
    /// The server's name.
    pub name: String,
    /// Its address's SHA-256: an address holds the proxy's secret. A
    /// server moved, or another of the name, isn't this one.
    pub address: String,
    pub tools: Vec<KeptTool>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instructions: String,
}

/// A tool, as [`Lists`] keeps it: as offered, and its server's name.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeptTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub own: String,
}

impl List {
    fn of(server: &Server, offered: &Offered) -> Self {
        Self {
            name: server.name.clone(),
            address: link_digest::sha256(server.url.as_bytes()),
            instructions: offered.instructions.clone(),
            tools: offered
                .tools
                .iter()
                .map(|(tool, own)| KeptTool {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    schema: tool.schema.clone(),
                    own: own.clone(),
                })
                .collect(),
        }
    }

    fn is_of(&self, server: &Server) -> bool {
        self.name == server.name && self.address == link_digest::sha256(server.url.as_bytes())
    }
}

impl Lists {
    /// What `server` offered as kept: none when it isn't kept, or isn't
    /// what a listing of `server` could have made.
    fn offered(&self, server: &Server) -> Option<Offered> {
        let list = self.servers.iter().find(|list| list.is_of(server))?;
        if list.tools.len() > TOOLS_MAX || list.instructions.len() > INSTRUCTIONS_MAX + 32 {
            return None;
        }
        list.tools
            .iter()
            .map(|kept| {
                let full = format!("{}{SEPARATOR}{}", server.name, kept.own);
                let fits = kept.name == plain_name(&full)
                    && full.chars().count() <= NAME_MAX
                    && kept.schema.is_object()
                    && kept.schema.to_string().len() <= SCHEMA_MAX
                    && kept.description.len() <= DESCRIPTION_MAX + 32;
                fits.then(|| {
                    let tool = Tool {
                        name: kept.name.clone(),
                        description: kept.description.clone(),
                        schema: kept.schema.clone(),
                    };
                    (tool, kept.own.clone())
                })
            })
            .collect::<Option<Vec<_>>>()
            // By name, as a listing offers them, whatever order an older
            // harness kept them in.
            .map(|mut tools| {
                tools.sort_by(|a, b| a.0.name.cmp(&b.0.name));
                Offered {
                    tools,
                    instructions: list.instructions.clone(),
                    summaries: HashMap::new(),
                    reads: Default::default(),
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kept list is offered only for the server it was listed from,
    /// and only as a listing could have made it.
    #[test]
    fn a_kept_list_is_only_its_servers() {
        let gh = Server {
            name: "gh".into(),
            url: "http://127.0.0.1:1/_link/url/secret/mcp/gh".into(),
            kept: false,
        };
        let tool = |name: &str, own: &str| {
            (
                Tool {
                    name: name.into(),
                    description: "Lists issues.".into(),
                    schema: json!({"type": "object"}),
                },
                own.to_string(),
            )
        };
        let offered = Offered {
            tools: vec![tool("gh__list_issues", "list_issues")],
            instructions: "List issues before you open one.".into(),
            ..Offered::default()
        };
        let lists = Lists {
            servers: vec![List::of(&gh, &offered)],
        };
        assert_eq!(lists.offered(&gh), Some(offered));
        let moved = Server {
            url: "http://127.0.0.1:2/_link/url/secret/mcp/gh".into(),
            ..gh.clone()
        };
        assert_eq!(lists.offered(&moved), None);
        // A name that isn't `<server>__<its own>`.
        let lists = Lists {
            servers: vec![List::of(
                &gh,
                &Offered {
                    tools: vec![tool("gh__list_issues", "delete_repo")],
                    ..Offered::default()
                },
            )],
        };
        assert_eq!(lists.offered(&gh), None);
    }

    /// Two agents of one process each keep their own lists: one's kept
    /// list is never offered for the other's server of the same name.
    #[test]
    fn kept_lists_are_each_agents() {
        let gh = Server {
            name: "gh".into(),
            url: "http://127.0.0.1:1/_link/url/secret/mcp/gh".into(),
            kept: false,
        };
        let offered = Offered {
            tools: vec![(
                Tool {
                    name: "gh__list_issues".into(),
                    description: "Lists issues.".into(),
                    schema: json!({"type": "object"}),
                },
                "list_issues".to_string(),
            )],
            ..Offered::default()
        };
        let (one, two) = (Agent::new("one").unwrap(), Agent::new("two").unwrap());
        let listed = Listed::default();
        let lists = Lists {
            servers: vec![List::of(&gh, &offered)],
        };
        listed.read_kept(&one, Some(lists), std::slice::from_ref(&gh));
        listed.read_kept(&two, None, std::slice::from_ref(&gh));
        assert!(listed.read(&one) && listed.read(&two));
        assert_eq!(listed.failed(&two, &gh, true), None);
        assert!(!listed.failed_lately(&two, &gh));
        assert_eq!(listed.failed(&two, &gh, false), None);
        assert!(listed.failed_lately(&two, &gh));
        // Only a process's first ask waits for the grant, whatever came
        // of it.
        assert_eq!(listed.grant(&one, &gh), Grant::Awaited);
        assert_eq!(listed.grant(&one, &gh), Grant::Refused);
        assert_eq!(listed.grant(&two, &gh), Grant::Awaited);
        assert_eq!(listed.failed(&one, &gh, false), Some(offered.clone()));
        // The saved list stands in, but never for a listing: the next turn
        // asks again.
        assert_eq!(listed.get(&one, &gh), None);
        assert_eq!(listed.failed(&one, &gh, true), Some(offered));
        assert_eq!(listed.to_keep(&two, std::slice::from_ref(&gh)), None);
    }
}
