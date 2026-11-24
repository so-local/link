//! A turn's tools: opened, offered and called ([`Toolbox`]).

use super::*;

mod calling;
mod compact;
mod reading;

/// A turn's tools: each server offered, its session once there is one,
/// and what each offers.
pub struct Toolbox<'a> {
    client: reqwest::Client,
    listed: &'a Listed,
    kept: &'a Kept,
    agent: Agent,
    /// How long a server has to start, or to list its tools.
    timeout: std::time::Duration,
    /// The agent's opinions: what's kept of each tool's answer, and how
    /// long a kept session lives.
    opinions: link_agent_spec::Spec,
    servers: Vec<(Server, tokio::sync::Mutex<Started>)>,
    /// What each of `servers` said of itself, in their order.
    said: Vec<String>,
    /// The model's name for each tool: the server it's in, and its own
    /// name there.
    tools: Vec<(Tool, usize, String)>,
    /// The model's names of the tools offered that say they only read.
    reads: std::collections::HashSet<String>,
    read_sources: Vec<link_harness_wire::evidence::ReadRegistration>,
}

/// A turn's session with a server.
enum Started {
    /// None yet: none kept, and the turn hasn't called it.
    Not,
    Open(Arc<Session>),
    /// It failed to start, why: its calls are answered so this turn.
    Failed(String),
}

impl<'a> Toolbox<'a> {
    /// The tools of `servers`, `agent`'s, as each last listed them in
    /// `listed`, with its session if one is `kept`; a server not listed
    /// yet in this process is asked, all at once, and its session kept.
    /// One that doesn't answer is offered as the last process listed it
    /// ([`Lists`]), and asked again at the next turn, or else left out of
    /// this turn, and said so on stderr: the turn goes on without it.
    /// Each turn waits `timeout` for a server to start and list its
    /// tools (`OPEN_TIMEOUT`); a start still going then goes on, up to
    /// `limit` in all ([`super::start_limit`]), and the turns after wait
    /// for it, never a start of their own: on a busy machine a slow
    /// start is finished once, not begun again at every turn. A first
    /// ask waits for its sandbox's grant ([`Grant`]); `opinions` are the
    /// agent's.
    pub async fn open(
        client: &reqwest::Client,
        (listed, kept): (&'a Listed, &'a Kept),
        agent: &Agent,
        servers: &[Server],
        (timeout, limit): (std::time::Duration, std::time::Duration),
        opinions: link_agent_spec::Spec,
    ) -> Toolbox<'a> {
        let life = life(&opinions, servers);
        let opened = futures_util::future::join_all(servers.iter().map(|server| async move {
            if let Some(offered) = listed.get(agent, server) {
                let started = match kept.get(agent, server).filter(|_| server.kept) {
                    Some(session) => Started::Open(session),
                    None => Started::Not,
                };
                return Ok((started, offered));
            }
            if listed.failed_lately(agent, server) {
                anyhow::bail!(
                    "it failed to start less than {} minutes ago",
                    RETRY_AFTER.as_secs() / 60
                );
            }
            let mut opening = listed.opening(agent, server, || {
                begin(client, (listed, kept), agent, server, limit, life)
            });
            let outcome = match tokio::time::timeout(timeout, opening.wait_for(Option::is_some))
                .await
            {
                Ok(Ok(outcome)) => outcome.clone(),
                Ok(Err(_)) => None,
                // Still starting: offered as it was listed before, if it
                // was, its calls answered as unavailable, and listed once
                // its start is done.
                Err(_) => {
                    let why = anyhow::anyhow!("it's still starting, and is offered once it has");
                    return match listed.failed(agent, server, true) {
                        Some(offered) => {
                            crate::warn!(crate::said::Ids::agent(agent.id()) => "tool {} offered as last listed: {why:#}", server.name);
                            Ok((Started::Not, offered))
                        }
                        None => Err(why),
                    };
                }
            };
            match outcome {
                Some(super::listing::Outcome::Opened(offered, session)) => {
                    let taken = session.lock().unwrap_or_else(|e| e.into_inner()).take();
                    let started = match taken.or_else(|| kept.get(agent, server).filter(|_| server.kept)) {
                        Some(session) => Started::Open(session),
                        None => Started::Not,
                    };
                    anyhow::Ok((started, offered))
                }
                // Offered as the last process listed it, if it did: its
                // calls answered as unavailable, as when it fails later.
                Some(super::listing::Outcome::Failed(why, Some(offered))) => {
                    crate::warn!(crate::said::Ids::agent(agent.id()) => "tool {} offered as last listed: {why}", server.name);
                    Ok((Started::Not, offered))
                }
                Some(super::listing::Outcome::Failed(why, None)) => Err(anyhow::anyhow!(why)),
                None => Err(anyhow::anyhow!("its start ended without an answer")),
            }
        }))
        .await;
        let mut toolbox = Toolbox {
            client: client.clone(),
            listed,
            kept,
            agent: agent.clone(),
            timeout,
            opinions,
            servers: Vec::new(),
            said: Vec::new(),
            tools: Vec::new(),
            reads: Default::default(),
            read_sources: Vec::new(),
        };
        let mut size = 0;
        for (server, opened) in servers.iter().zip(opened) {
            match opened {
                Ok((started, offered)) => {
                    let index = toolbox.servers.len();
                    toolbox
                        .servers
                        .push((server.clone(), tokio::sync::Mutex::new(started)));
                    toolbox.said.push(offered.instructions);
                    for (tool, own) in offered.tools {
                        if toolbox.tools.iter().any(|(t, _, _)| t.name == tool.name) {
                            crate::warn!(crate::said::Ids::agent(agent.id()) => "tool {} left out: another has its name", tool.name);
                            continue;
                        }
                        size += tool.size();
                        if size > OFFERED_MAX {
                            crate::warn!(
                                crate::said::Ids::agent(agent.id()) =>
                                "tool {} left out: the tools offered are over {OFFERED_MAX} bytes",
                                tool.name
                            );
                            continue;
                        }
                        if offered.reads.contains(&tool.name) {
                            toolbox.reads.insert(tool.name.clone());
                        }
                        toolbox.tools.push((tool, index, own));
                    }
                }
                Err(err) => crate::warn!(
                    crate::said::Ids::agent(agent.id()) =>
                    "tool {} left out of this turn: {err:#}", server.name
                ),
            }
        }
        toolbox
    }

    /// Whether a call of `name` only reads: its tool is offered and said
    /// so (MCP's `readOnlyHint`). Any other call is taken as one that
    /// changes something, so a step's calls run in order around it.
    pub fn reads_only(&self, name: &str) -> bool {
        self.reads.contains(name) && self.tools.iter().any(|(t, _, _)| t.name == name)
    }

    /// Only the tools named in `names`: a conversation narrowed to them.
    pub fn only(&mut self, names: &[String]) {
        self.tools.retain(|(tool, _, _)| names.contains(&tool.name));
    }

    /// What each server whose tools are offered says of itself, by the
    /// server's name, so the prompt's tools section, and the provider's
    /// cache, stay the same; one that says nothing is left out.
    pub fn instructions(&self) -> Vec<(String, String)> {
        let mut said: Vec<(String, String)> = self
            .servers
            .iter()
            .zip(&self.said)
            .enumerate()
            .filter(|(index, (_, said))| {
                !said.is_empty() && self.tools.iter().any(|(_, i, _)| i == index)
            })
            .map(|(_, ((server, _), said))| (server.name.clone(), said.clone()))
            .collect();
        said.sort();
        said
    }

    /// What the model is offered, in the same order each turn so the
    /// provider's cache holds.
    pub fn offered(&self) -> Vec<Tool> {
        self.tools.iter().map(|(t, _, _)| t.clone()).collect()
    }

    /// The most of server `name`'s answers kept, in bytes: its spec's
    /// (`[tool.<name>] answer`, else `[tools] answer`), so one answer
    /// can't fill the window.
    fn answer_max(&self, name: &str) -> usize {
        self.opinions
            .tool(name)
            .answer
            .and_then(|answer| link_agent_spec::size(&answer).ok())
            .map_or(ANSWER_FLOOR, |bytes| bytes as usize)
            .max(ANSWER_FLOOR)
    }

    /// How long a call to `server` may take: its spec's (`[tool.<name>]
    /// timeout`, else `[tools] timeout`), else Link's; and, for a tool not
    /// Link's own, beside it as long as it may wait for its owner to allow
    /// it (`[connections] ask_wait`), which its time doesn't count. The
    /// sandbox asks, so which of its calls do isn't known here; Link's own
    /// ([`Server::kept`]) never ask.
    fn call_timeout(&self, server: &Server) -> std::time::Duration {
        let name = server.name.as_str();
        let of = |spec: &link_agent_spec::Spec| {
            let timeout = spec.tool(name).timeout?;
            u64::try_from(timeout.get().as_millis()).ok()
        };
        let asking = |spec: &link_agent_spec::Spec| {
            let wait = spec.connections.as_ref()?.ask_wait?;
            u64::try_from(wait.get().as_millis()).ok()
        };
        let own = of(&self.opinions)
            .or_else(|| of(&link_agent_spec::link()))
            .map_or(CALL_TIMEOUT, std::time::Duration::from_millis);
        let mut wait = asking(&self.opinions).or_else(|| asking(&link_agent_spec::link()));
        if let Some(approvals) = &self.opinions.approvals {
            for domain in approvals.domains.values() {
                let duration = if domain.allow_request_overrides == Some(true) {
                    Some(super::INTERACTION_MAX_WAIT_MS)
                } else {
                    domain.wait_secs.map(|s| s.saturating_mul(1000))
                };
                if let Some(duration) = duration {
                    wait = Some(wait.unwrap_or_default().max(duration));
                }
            }
        }
        own + wait.map_or(std::time::Duration::ZERO, std::time::Duration::from_millis)
    }

    /// Lets each session go: the turn is over. A kept one stays for the
    /// next turn, and is ended once none has used it for as long as its
    /// [`Life`] says; the rest end now, and so does a kept one no longer
    /// kept that no other turn holds.
    pub async fn close(self) {
        let mut ending = Vec::new();
        for (server, started) in self.servers {
            if let Started::Open(session) = started.into_inner() {
                let still = server.kept && self.kept.touch(&self.agent, &server, &session);
                if !still && (!server.kept || Arc::strong_count(&session) == 1) {
                    ending.push(session);
                }
            }
        }
        futures_util::future::join_all(ending.iter().map(|session| session.end())).await;
    }
}

/// Begins `agent`'s `server`'s start, apart from the turn that asks, so a
/// turn that stops waiting for it never ends it: within `limit`, its
/// tools listed and its session kept as a turn's start does, and how it
/// ended told to each turn waiting. A session of a server not kept that
/// no turn waits for any more is ended.
fn begin(
    client: &reqwest::Client,
    (listed, kept): (&Listed, &Kept),
    agent: &Agent,
    server: &Server,
    limit: std::time::Duration,
    life: Life,
) -> super::listing::Opening {
    use super::listing::Outcome;
    let (told, opening) = tokio::sync::watch::channel(None);
    let grant = listed.grant(agent, server);
    let (client, listed, kept) = (client.clone(), listed.clone(), kept.clone());
    let (agent, server) = (agent.clone(), server.clone());
    tokio::spawn(async move {
        let outcome = match Session::open(&client, &server, limit, grant).await {
            Ok((session, offered)) => {
                listed.put(&agent, &server, offered.clone());
                let session = Arc::new(session);
                if server.kept {
                    kept.put(&agent, &server, session.clone(), life);
                }
                Outcome::Opened(offered, Arc::new(std::sync::Mutex::new(Some(session))))
            }
            Err(err) => {
                let passing = super::sessions::is_passing(&err);
                Outcome::Failed(format!("{err:#}"), listed.failed(&agent, &server, passing))
            }
        };
        listed.opened(&agent, &server);
        if let Outcome::Opened(_, session) = &outcome
            && !server.kept
            && told.receiver_count() == 0
        {
            let unclaimed = session.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(session) = unclaimed {
                session.end().await;
            }
        }
        let _ = told.send(Some(outcome));
    });
    opening
}

/// The least of a tool's answer kept, in bytes, whatever a spec says: a
/// spec's own floor ([`link_agent_spec`] refuses less).
const ANSWER_FLOOR: usize = link_agent_spec::ANSWER_LEAST as usize;

/// How long a kept server's sessions live, as the agent's spec says of
/// memory (`[tool.memory] keep`, `max_age`), run's and file's alike.
pub(super) fn life(opinions: &link_agent_spec::Spec, _servers: &[Server]) -> Life {
    let memory = opinions.tool("memory");
    let link = link_agent_spec::link().tool("memory");
    let of = |mine: Option<link_actions::value::Duration>,
              links: Option<link_actions::value::Duration>| {
        mine.or(links).map(|d| d.get()).unwrap_or_default()
    };
    (of(memory.keep, link.keep), of(memory.max_age, link.max_age))
}
