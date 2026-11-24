//! What the store stops keeping at each write, by who still reads it.
//! Its numbers are `retention.toml`'s.

use crate::{control::ReceiptStage, store::State};
use link_interactions_contract::control::ControlOperation;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// The limits of what the store keeps.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Retention {
    /// The most settled inputs and controls kept, newest first.
    pub settled: usize,
    /// The days a resolved question stays after its resolution.
    pub resolved_days: u64,
}

/// Seconds in a day, which [`Retention::resolved_days`] counts.
const DAY_SECS: u64 = 24 * 60 * 60;

/// The limits, from `retention.toml`.
pub(crate) fn retention() -> &'static Retention {
    static RETENTION: std::sync::LazyLock<Retention> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("retention.toml"))
            .expect("retention.toml is the interaction store's retention")
    });
    &RETENTION
}

/// Drops what no later call reads: the events every subscriber has
/// passed, the retired work nothing names and the policy calls made
/// under it, the settled inputs and controls past the newest
/// [`Retention::settled`], and the questions resolved more than
/// [`Retention::resolved_days`] before `now`. A question resolved in
/// this write is stamped with `now` first.
pub(crate) fn apply(state: &mut State, now: u64) {
    let retention = retention();
    stamp(state, now);
    events(state);
    resolved(state, now, retention.resolved_days.saturating_mul(DAY_SECS));
    bindings(state);
    effects(state);
    settled(state, retention.settled);
}

/// Stamps each question resolved since the last write with `now`, the
/// clock its retention counts by, however it was resolved: answered,
/// expired, cancelled or closed.
fn stamp(state: &mut State, now: u64) {
    for record in state.records.values_mut() {
        if record.snapshot.resolution.is_some() && record.resolved_at.is_none() {
            record.resolved_at = Some(now);
        }
    }
}

/// Removes each question resolved at least `kept` seconds before `now`,
/// with its response code, deliveries, their messages and drafts. A
/// question stays while it isn't resolved, its effect waits or is under
/// way, or a kept event names it: a subscriber starting at the first
/// event kept reads that event's question. A delivery that never reached
/// its owner keeps nothing once the question is resolved, since nothing
/// delivers a resolved question. Its code is free again once it's gone.
/// Runs after [`events`], so the events left are the kept ones.
fn resolved(state: &mut State, now: u64, kept: u64) {
    let named: BTreeSet<&crate::RequestId> = state
        .events
        .iter()
        .filter_map(|e| e.request.as_ref())
        .collect();
    let gone: BTreeSet<crate::RequestId> = state
        .records
        .iter()
        .filter(|(id, r)| {
            r.resolved_at
                .is_some_and(|at| now.saturating_sub(at) >= kept)
                && !matches!(
                    r.snapshot.effect,
                    crate::approval::EffectState::Awaiting
                        | crate::approval::EffectState::Authorized
                        | crate::approval::EffectState::RequiresClarification
                        | crate::approval::EffectState::Applying { .. }
                        | crate::approval::EffectState::Unknown { .. }
                )
                && !named.contains(id)
        })
        .map(|(id, _)| id.clone())
        .collect();
    if gone.is_empty() {
        return;
    }
    for id in &gone {
        state.records.remove(id);
    }
    state.forget_codes(&gone);
    let deliveries: BTreeSet<crate::DeliveryId> = state
        .deliveries
        .values()
        .filter(|d| gone.contains(&d.request))
        .map(|d| d.id.clone())
        .collect();
    state.deliveries.retain(|id, _| !deliveries.contains(id));
    state.drafts.retain(|id, _| !deliveries.contains(id));
    state.messages.retain(|_, id| !deliveries.contains(id));
}

/// The lowest subscriber cursor: every event at or before it has been
/// read by each subscriber. With none subscribed, every event has.
fn floor(state: &State) -> u64 {
    state.cursors.values().min().copied().unwrap_or(u64::MAX)
}

/// Keeps the events after the lowest cursor, and the newest always: the
/// next event's number follows it, and a cursor is acknowledged against
/// it.
fn events(state: &mut State) {
    let floor = floor(state);
    let newest = state.events.last().map(|e| e.id);
    state
        .events
        .retain(|e| e.id > floor || Some(e.id) == newest);
}

/// Removes a retired binding once a newer generation of its work holds
/// its place and nothing reads it by its attempt: no effect in flight or
/// unknown under it, no unread event naming it (the owner's notice of a
/// receipt finds its route there), no work naming it as parent, and no
/// question still open or applying under it. Its work then reads as
/// stale by that newer binding ([`crate::runtime::claim_current`]), as it
/// did while kept.
fn bindings(state: &mut State) {
    let floor = floor(state);
    let runtime = &state.controls.runtime;
    let mut newest = BTreeMap::new();
    for b in runtime.work.values() {
        let work = &b.state.work;
        let generation = newest
            .entry(crate::runtime::same_work(work))
            .or_insert(work.generation);
        *generation = (*generation).max(work.generation);
    }
    let named: BTreeSet<&str> = state
        .events
        .iter()
        .filter(|e| e.id > floor)
        .filter_map(|e| e.target.as_ref())
        .map(|w| w.attempt.as_str())
        .chain(
            runtime
                .work
                .values()
                .filter_map(|b| b.parent_attempt.as_deref()),
        )
        .chain(runtime.children.values().map(String::as_str))
        .chain(
            state
                .records
                .values()
                .filter(|r| {
                    r.snapshot.resolution.is_none()
                        || matches!(
                            r.snapshot.effect,
                            crate::approval::EffectState::Awaiting
                                | crate::approval::EffectState::Authorized
                                | crate::approval::EffectState::Applying { .. }
                                | crate::approval::EffectState::Unknown { .. }
                        )
                })
                .filter_map(|r| r.snapshot.question.requester.work.as_ref())
                .map(|w| w.attempt.as_str()),
        )
        .collect();
    let gone: Vec<String> = runtime
        .work
        .values()
        .filter(|b| {
            let work = &b.state.work;
            b.retired
                && !b.active
                && b.state.in_flight_effects.is_empty()
                && newest[&crate::runtime::same_work(work)] > work.generation
                && !named.contains(work.attempt.as_str())
        })
        .map(|b| b.state.work.attempt.clone())
        .collect();
    for attempt in gone {
        state.controls.runtime.work.remove(&attempt);
    }
}

/// Removes the record of a policy call whose work's binding is gone: it
/// guarded the call against a replay, which that work's staleness now
/// refuses first. A call in flight or unknown keeps its binding, so its
/// record stays.
fn effects(state: &mut State) {
    let runtime = &mut state.controls.runtime;
    let work = &runtime.work;
    runtime
        .effects
        .retain(|_, record| work.contains_key(&record.work.attempt));
}

/// A settled input or control batch, by its ID.
enum Settled {
    Input(crate::InputId),
    Batch(crate::ControlId),
}

/// Keeps the newest `kept` settled inputs and control batches, with
/// their receipts. An input is settled once the model has read it (its
/// receipt says included, or its redirect applied). A batch is settled
/// once no member addresses a binding that's still kept, waits on a
/// deletion, or pauses a binding. One that isn't settled stays however
/// old.
fn settled(state: &mut State, kept: usize) {
    let controls = &state.controls;
    let mut first: BTreeMap<&str, u64> = BTreeMap::new();
    let mut read: BTreeSet<&str> = BTreeSet::new();
    let mut ended: BTreeSet<&str> = BTreeSet::new();
    for r in &controls.receipts {
        first.entry(&r.correlation).or_insert(r.sequence);
        if matches!(
            r.stage,
            ReceiptStage::Included | ReceiptStage::RedirectApplied
        ) {
            read.insert(&r.correlation);
        }
        if matches!(
            r.stage,
            ReceiptStage::Applied | ReceiptStage::Unsupported | ReceiptStage::Denied
        ) {
            ended.insert(&r.correlation);
        }
    }
    let pausing: BTreeSet<&crate::ControlId> = controls
        .runtime
        .work
        .values()
        .flat_map(|b| &b.state.pause_reasons)
        .collect();
    let mut candidates: Vec<(u64, Settled)> = controls
        .inputs
        .keys()
        .filter(|id| read.contains(id.as_str()))
        .map(|id| (first[id.as_str()], Settled::Input(id.clone())))
        .collect();
    candidates.extend(
        controls
            .batches
            .iter()
            .filter(|(_, batch)| {
                batch.members.iter().all(|m| {
                    !controls.runtime.work.contains_key(&m.work.attempt)
                        && !pausing.contains(&m.id)
                        && (!matches!(m.operation, ControlOperation::Delete { .. })
                            || ended.contains(m.id.as_str()))
                })
            })
            .map(|(id, batch)| {
                let admitted = batch
                    .members
                    .iter()
                    .filter_map(|m| first.get(m.id.as_str()).copied())
                    .min()
                    .unwrap_or_default();
                (admitted, Settled::Batch(id.clone()))
            }),
    );
    if candidates.len() <= kept {
        return;
    }
    candidates.sort_by_key(|(admitted, _)| std::cmp::Reverse(*admitted));
    let mut gone: BTreeSet<String> = BTreeSet::new();
    for (_, settled) in candidates.into_iter().skip(kept) {
        match settled {
            Settled::Input(id) => {
                state.controls.inputs.remove(&id);
                state.controls.runtime.delivered_inputs.remove(&id);
                gone.insert(id.as_str().into());
            }
            Settled::Batch(id) => {
                let batch = state.controls.batches.remove(&id).expect("listed batch");
                for member in batch.members {
                    state.controls.runtime.deletion_sources.remove(&member.id);
                    gone.insert(member.id.as_str().into());
                }
            }
        }
    }
    state
        .controls
        .receipts
        .retain(|r| !gone.contains(&r.correlation));
}

#[cfg(test)]
mod tests;
