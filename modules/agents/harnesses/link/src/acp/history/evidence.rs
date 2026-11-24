//! An exporter's native observations remain outside imported updates.

use link_agent_state_protocol::forms::Session;
use link_harness_wire::evidence::{
    Version,
    export::{NativeEvent, Omission, SessionEvidence, UpdateRange},
};
use link_harness_wire::runtime::{AgentKey, Conversation};

use crate::{
    agent::{Agent, Thread},
    event,
    store::Snapshot,
};

pub(crate) fn session(
    agent: &Agent,
    thread: &Thread,
    snapshot: Snapshot,
    mut part: impl FnMut(&event::Part) -> anyhow::Result<Vec<u8>>,
) -> anyhow::Result<Session> {
    let mut updates = Vec::new();
    let mut manifest = SessionEvidence {
        version: Version::V1,
        conversation: Conversation {
            agent: AgentKey {
                owner: agent.owner().into(),
                id: agent.id().into(),
            },
            thread: thread.name().into(),
        },
        events: Vec::new(),
        imported: Vec::new(),
        read: snapshot.read,
    };
    for event in &snapshot.events {
        let start = updates.len();
        updates.extend(super::archived(std::slice::from_ref(event), &mut part)?);
        let range = UpdateRange {
            start: start.try_into()?,
            len: (updates.len() - start).try_into()?,
        };
        if event.portable.is_some() {
            if range.len != 0 {
                manifest.imported.push(range);
            }
            continue;
        }
        manifest.events.push(NativeEvent {
            seq: event.seq,
            role: event.role.clone(),
            at: event.at.clone(),
            updates: range,
            request: event.request,
            model: event.model.clone(),
            provider: event.provider.clone(),
            response_id: event.response_id.clone(),
            tokens: event.tokens.clone(),
            latency_ms: event.latency_ms,
            cost: event.cost,
            parts: event.parts.clone(),
            omissions: if super::internal(event) && !event.parts.is_empty() {
                vec![Omission::InternalMedia]
            } else {
                Vec::new()
            },
            evidence: event.evidence.as_deref().cloned(),
        });
    }
    manifest.validate(updates.len())?;
    Ok(Session {
        session_id: thread.name().to_string().into(),
        title: None,
        harness: None,
        updates,
        evidence: Some(manifest),
    })
}

#[cfg(test)]
mod tests;
