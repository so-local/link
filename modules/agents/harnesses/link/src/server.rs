//! The harness's port: its health, the proof that it's the agent's
//! harness, and ACP, where every client reaches it ([`crate::acp`]),
//! each request for the agent its `Link-Agent` names, with its key.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;

use crate::agent::Agent;
use crate::turn::Harness;

pub(crate) mod turning;

/// What every request is answered with.
pub struct Server {
    pub harness: Arc<Harness>,
    /// The agent of a request that names none: the one this process was
    /// started for.
    pub agent: Agent,
    /// Its ACP connections open now.
    pub(crate) connections: Arc<std::sync::atomic::AtomicUsize>,
}

pub fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(link_harness_contract::harness::PROOF_PATH, get(prove))
        .route(link_connection_contract::acp::ACP_PATH, get(acp))
        .with_state(server)
}

/// Proves to a connection that this is the agent's harness, before it sends
/// the key: the challenge signed with the key ([`link_harness_contract::harness::endpoint_proof`]).
async fn prove(State(server): State<Arc<Server>>, headers: HeaderMap) -> Response {
    let agent = match agent_of(&server, &headers) {
        Ok(agent) => agent,
        Err((status, why)) => return refuse(status, why),
    };
    let challenge = headers
        .get(link_harness_contract::harness::CHALLENGE_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|c| link_harness_contract::harness::is_challenge(c));
    let Some(challenge) = challenge else {
        return refuse(StatusCode::BAD_REQUEST, "no challenge");
    };
    let Some(key) = server.endpoint_key(&agent).ok().flatten() else {
        return refuse(StatusCode::NOT_FOUND, "no key to prove");
    };
    let proof = link_harness_contract::harness::endpoint_proof(&key, challenge);
    ([(link_harness_contract::harness::PROOF_HEADER, proof)], "").into_response()
}

/// An ACP connection ([`crate::acp`]), opened as any request is: for
/// the agent its headers name, with its endpoint key. Counted as
/// answering from here, while the request is still the server's, until
/// it closes: once upgraded the server no longer waits for it, and a
/// harness stopping lets the prompt it's about to be asked finish.
async fn acp(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    upgrade: axum::extract::ws::WebSocketUpgrade,
) -> Response {
    let agent = match agent_of(&server, &headers) {
        Ok(agent) => agent,
        Err((status, why)) => return refuse(status, why),
    };
    if let Err((status, why)) = keyed(&server, &agent, &headers) {
        return refuse(status, why);
    }
    let open = server.harness.answering();
    upgrade
        .max_message_size(BODY_MAX)
        .on_upgrade(move |socket| async move {
            crate::acp::serve(server, agent, socket).await;
            drop(open);
        })
}

/// The most a message holds: its images and documents, in base64, a
/// third larger than they are.
pub const BODY_MAX: usize = 48 << 20;

/// The most text a message holds, in bytes.
pub const TEXT_MAX: usize = 2 << 20;

fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(json!({"error": {"message": why}}))).into_response()
}

/// The agent a request is for: the one its `Link-Agent-Id` names, else
/// the one its `Link-Agent` names, or the process's own when it names
/// none. A name is only an owner's, so one two owners' agents here share
/// needs the ID; an ID two owners' agents share, the key of one. Refused
/// when it names one this process doesn't answer for, or the ID and the
/// name disagree, before anything else of it is read.
fn agent_of(server: &Server, headers: &HeaderMap) -> Result<Agent, (StatusCode, &'static str)> {
    let header = |name: &str| -> Result<Option<&str>, (StatusCode, &'static str)> {
        match headers.get(name) {
            None => Ok(None),
            Some(value) => value
                .to_str()
                .map(Some)
                .map_err(|_| (StatusCode::BAD_REQUEST, "that header isn't text")),
        }
    };
    let named = header(link_connection_contract::AGENT_HEADER)?;
    let id = header(link_connection_contract::AGENT_ID_HEADER)?;
    if let Some(name) = named {
        link_machine::machine::check_name(name)
            .map_err(|_| (StatusCode::BAD_REQUEST, "that Link-Agent isn't an agent"))?;
    }
    let agent = match (id, named) {
        (None, None) => return Ok(server.agent.clone()),
        (Some(id), _) => {
            let mut agents = server.harness.agents_with_id(id);
            agents.retain(|agent| named.is_none_or(|name| name == agent.name()));
            match agents.as_slice() {
                [one] => Some(one.clone()),
                [] => None,
                // Two owners' agents with one ID: the one whose key it
                // carries.
                several => match several.iter().find(|a| allowed(server, a, headers)) {
                    Some(agent) => Some(agent.clone()),
                    None if headers.contains_key(header::AUTHORIZATION) => {
                        return Err((StatusCode::UNAUTHORIZED, "wrong or missing key"));
                    }
                    None => {
                        return Err((
                            StatusCode::CONFLICT,
                            "more than one owner's agent here has that ID: send its key",
                        ));
                    }
                },
            }
        }
        (None, Some(name)) => match server.harness.agents_named(name).as_slice() {
            [one] => Some(one.clone()),
            [] => None,
            _ => {
                return Err((
                    StatusCode::CONFLICT,
                    "more than one agent here has that name: name it by Link-Agent-Id",
                ));
            }
        },
    };
    match agent {
        Some(agent) if named.is_none_or(|name| name == agent.name()) => Ok(agent),
        _ => Err((StatusCode::NOT_FOUND, "no such agent here")),
    }
}

/// Whether `headers` carry `agent`'s endpoint key; never when its
/// settings can't be read, or name no key ([`keyed`] says which).
fn allowed(server: &Server, agent: &Agent, headers: &HeaderMap) -> bool {
    let Ok(Some(key)) = server.endpoint_key(agent) else {
        return false;
    };
    let sent = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    same(sent.as_bytes(), key.expose().as_bytes())
}

/// Refused unless `headers` carry `agent`'s endpoint key: 503 while it
/// has none (Link hands every agent one), 401 for a wrong one.
fn keyed(
    server: &Server,
    agent: &Agent,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, &'static str)> {
    if !matches!(server.endpoint_key(agent), Ok(Some(_))) {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "this agent has no endpoint key yet",
        ));
    }
    match allowed(server, agent, headers) {
        true => Ok(()),
        false => Err((StatusCode::UNAUTHORIZED, "wrong or missing key")),
    }
}

use link_secretive::equal as same;

impl Server {
    pub fn new(harness: Harness, agent: Agent) -> Self {
        Self::sharing(Arc::new(harness), agent)
    }

    /// One of a process's endpoints, sharing its `harness` with the
    /// others: one for each agent it serves, on that agent's port, which
    /// answers a request naming none as `agent`.
    pub fn sharing(harness: Arc<Harness>, agent: Agent) -> Self {
        Self {
            harness,
            agent,
            connections: Arc::default(),
        }
    }

    /// How many ACP connections are open: one closed has been noticed,
    /// and what it was asked for finds its caller gone.
    pub fn connections(&self) -> usize {
        self.connections.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The key `agent`'s endpoint takes, as its settings say now (each
    /// start makes a new one).
    fn endpoint_key(&self, agent: &Agent) -> anyhow::Result<Option<link_secretive::Secret>> {
        Ok(self.harness.config(agent)?.endpoint_key)
    }
}
