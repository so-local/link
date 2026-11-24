//! What this process last asked a model in each conversation, so the
//! next request's evidence lists only what changed since
//! ([`link_harness_wire::evidence::request::Continued`]). Derived: a
//! process without it writes the next request whole.

use std::collections::HashMap;
use std::sync::Mutex;

use link_harness_wire::evidence::request::{Projected, RequestProjection};
use link_harness_wire::evidence::{Captured, Record};

use crate::agent::{Agent, Thread};
use crate::event::Event;

mod digested;

/// The most conversations remembered: past it, the one asked longest ago
/// is forgotten, and its next request written whole.
const KEPT_MAX: usize = 64;

/// Each conversation's last model request this process recorded.
#[derive(Default)]
pub(super) struct Requested {
    held: Mutex<(u64, HashMap<Key, Last>)>,
    /// Each conversation's digests of what its requests send, each made
    /// once ([`digested`]).
    digested: digested::Digested,
}

/// A conversation, by its agent's owner and ID and its thread.
type Key = (String, String, Thread);

fn key(agent: &Agent, thread: &Thread) -> Key {
    (
        agent.owner().to_string(),
        agent.id().to_string(),
        thread.clone(),
    )
}

/// A model request recorded: its sequence, every message it sent, and how
/// many requests since the last recorded whole.
struct Last {
    seq: u64,
    projected: Projected,
    chain: u32,
    used: u64,
}

impl Requested {
    /// Each conversation's digests of what its requests send.
    pub(super) fn digests(&self) -> &digested::Digested {
        &self.digested
    }

    /// The projection to record for `projected`, the next request of
    /// `thread`, whose events so far are `events`, and how many requests
    /// since the last whole one: what changed since the request before it
    /// when this process recorded that one, it's the last in `events`,
    /// and fewer than `whole_every` went since the last whole one; else
    /// whole.
    pub(super) fn projection(
        &self,
        agent: &Agent,
        thread: &Thread,
        events: &[Event],
        projected: &Projected,
        whole_every: u32,
    ) -> (RequestProjection, u32) {
        let before = events.iter().rev().find_map(|event| {
            let evidence = event.evidence.as_ref()?;
            matches!(evidence.record, Record::ModelRequested { .. }).then_some(event.seq)
        });
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let continued = held
            .1
            .get(&key(agent, thread))
            .filter(|last| Some(last.seq) == before && last.chain + 1 < whole_every)
            .and_then(|last| {
                let continued = last.projected.continue_to(last.seq, projected)?;
                Some((continued, last.chain + 1))
            });
        match continued {
            Some((continued, chain)) => (RequestProjection::Continued(continued), chain),
            None => (RequestProjection::Whole(projected.clone()), 0),
        }
    }

    /// Keeps `projected` as `thread`'s last request, recorded as `seq`
    /// with `captured`: forgotten when it was omitted, so the next is whole.
    pub(super) fn recorded(
        &self,
        agent: &Agent,
        thread: &Thread,
        seq: u64,
        projected: Projected,
        captured: &Captured<RequestProjection>,
        chain: u32,
    ) {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let (tick, conversations) = &mut *held;
        let key = key(agent, thread);
        if captured.retained().is_none() {
            conversations.remove(&key);
            return;
        }
        *tick += 1;
        conversations.insert(
            key,
            Last {
                seq,
                projected,
                chain,
                used: *tick,
            },
        );
        while conversations.len() > KEPT_MAX {
            let Some(oldest) = conversations
                .iter()
                .min_by_key(|(_, last)| last.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            conversations.remove(&oldest);
        }
    }
}
