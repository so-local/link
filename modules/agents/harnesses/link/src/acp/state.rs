//! ASP's agent state, as Link Harness keeps it: its sessions, each
//! conversation of its store, both ways. Who the agent is, its skills
//! and its memory are the agent's spec and Link's memory, which Link
//! carries to any harness itself.

use link_agent_state_protocol::acp::schema::v1::SessionId;
use link_agent_state_protocol::methods::{Described, Imported, NotTaken};
use link_agent_state_protocol::{AgentState, Kind, State};

use super::{Connection, thread_of};

/// What it gives and takes.
pub fn kinds() -> AgentState {
    AgentState {
        export: vec![Kind::Sessions],
        import: vec![Kind::Sessions],
    }
}

pub fn content() -> Vec<link_harness_contract::harness::switching::SessionContent> {
    use link_harness_contract::harness::switching::SessionContent;
    vec![
        SessionContent::Tools,
        SessionContent::Images,
        SessionContent::Resources,
    ]
}

pub(super) fn describe(connection: &Connection) -> anyhow::Result<Described> {
    let threads = connection.server.harness.threads(&connection.agent)?;
    Ok(Described {
        kinds: [(Kind::Sessions, threads.len())].into(),
    })
}

/// Each conversation whole, as `session/load` replays it.
pub(super) fn export(connection: &Connection, kinds: &[Kind]) -> anyhow::Result<State> {
    connection
        .server
        .harness
        .exported_state(&connection.agent, kinds)
}

/// The shared native export path, whether the harness runs or is stopped.
pub fn exported_state(
    store: &dyn crate::store::Store,
    agent: &crate::agent::Agent,
    kinds: &[Kind],
) -> anyhow::Result<State> {
    if !kinds.contains(&Kind::Sessions) {
        return Ok(State::default());
    }
    let mut sessions = Vec::new();
    for thread in store.threads(agent)? {
        let snapshot = store.snapshot(agent, &thread)?;
        sessions.push(super::history::session(agent, &thread, snapshot, |part| {
            store.part(agent, &thread, &part.file)
        })?);
    }
    Ok(State {
        sessions: Some(sessions),
        ..State::default()
    })
}

/// Each session a conversation: its thread's, where it's one that holds
/// nothing yet or that the session goes on from; else, one that parted,
/// `<thread>~<harness>` for the harness it came from; else a new one.
pub(super) fn import(connection: &Connection, state: State) -> anyhow::Result<Imported> {
    let harness = &connection.server.harness;
    let mut imported = Imported::default();
    imported.not_taken.extend(state.omissions.iter().cloned());
    let mut contract = link_harness_contract::harness::Contract::new(true);
    contract.session_content = content();
    let omitted = contract.session_omissions(&state);
    if !omitted.is_empty() {
        imported.not_taken.push(NotTaken {
            kind: Kind::Sessions,
            reason: format!(
                "{} stay in the portable archive and are not used in native context",
                omitted
                    .into_iter()
                    .map(|kind| kind.words())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    for kind in state.kinds() {
        let reason = match kind {
            Kind::Sessions => continue,
            // Each named: its owner adds those they want to the agent's spec.
            Kind::Tools => format!(
                "Link Harness reads its tools from the agent's spec, which doesn't hold {}",
                servers(state.tools.as_deref().unwrap_or_default())
            ),
            _ => "Link Harness reads it from the agent's spec, which Link writes".into(),
        };
        imported.not_taken.push(NotTaken { kind, reason });
    }
    let Some(sessions) = state.sessions else {
        return Ok(imported);
    };
    for session in sessions {
        // Its thread's, else a copy beside it named for the harness it
        // came from, else one of its own.
        let copy = session
            .harness
            .as_ref()
            .map(|from| SessionId::new(format!("{}~{from}", session.session_id.0)));
        let mut thread = None;
        for id in [Some(&session.session_id), copy.as_ref()]
            .into_iter()
            .flatten()
        {
            let Ok(named) = thread_of(id) else { continue };
            if harness.bring(&connection.agent, &named, &session.updates)? {
                thread = Some(named);
                break;
            }
        }
        let thread = match thread {
            Some(thread) => thread,
            None => {
                let thread = super::fresh();
                harness.bring(&connection.agent, &thread, &session.updates)?;
                thread
            }
        };
        imported
            .sessions
            .insert(session.session_id, SessionId::new(thread.name()));
    }
    imported.taken.push(Kind::Sessions);
    Ok(imported)
}

/// The names of MCP servers `servers`, as a sentence lists them.
fn servers(servers: &[link_agent_state_protocol::acp::schema::v1::McpServer]) -> String {
    use link_agent_state_protocol::acp::schema::v1::McpServer;
    let names: Vec<&str> = servers
        .iter()
        .map(|server| match server {
            McpServer::Http(server) => server.name.as_str(),
            McpServer::Sse(server) => server.name.as_str(),
            McpServer::Stdio(server) => server.name.as_str(),
            _ => "an MCP server",
        })
        .collect();
    match names.split_last() {
        None => "its MCP servers".into(),
        Some((last, [])) => format!("MCP server {last}"),
        Some((last, rest)) => format!("MCP servers {} and {last}", rest.join(", ")),
    }
}
