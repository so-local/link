//! Host-issued work bindings and execution fences. Transcripts and checkpoints stay with the host.
use crate::{
    control::{invalidate, receipt},
    store::State,
    *,
};
use anyhow::{Context, ensure};
use link_interactions_contract::control::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod children;
mod controls;
mod credential;
mod deletion;
mod effects;
mod holds;
mod inputs;
mod schedules;
#[cfg(test)]
pub(crate) mod tests;
pub use holds::{DelegationProposal, ProposalState};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub owner_route: Option<String>,
    #[serde(with = "credential")]
    pub token: std::sync::Arc<link_secretive::Secret>,
    pub state: WorkState,
    pub routing_agent: String,
    pub parent_attempt: Option<String>,
    pub active: bool,
    pub retired: bool,
    pub interrupt: bool,
    pub plan_revision: u64,
    pub wake: bool,
}

pub struct WorkAdmission {
    pub owner_route: Option<String>,
    pub kind: WorkKind,
    pub owner: Actor,
    pub agent: String,
    pub routing_agent: String,
    pub session: String,
    pub attempt: String,
    pub parent_attempt: Option<String>,
    pub checkpoint: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeState {
    #[serde(default)]
    pub effects: BTreeMap<OperationId, effects::Record>,
    pub revision: u64,
    pub work: BTreeMap<String, Binding>,
    pub holds: BTreeMap<HoldId, Hold>,
    pub proposals: BTreeMap<String, DelegationProposal>,
    pub delivered_inputs: BTreeMap<InputId, u64>,
    pub children: BTreeMap<String, String>,
    #[serde(default)]
    pub deletion_sources: BTreeMap<ControlId, BTreeSet<String>>,
    #[serde(default)]
    pub routes: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub delivered_children: BTreeSet<String>,
}

#[derive(Clone, Copy)]
pub enum Step {
    Model,
    Tool,
    Delegation,
}

fn route(s: &mut State, agent: &str, name: &str) {
    for ids in s.controls.runtime.routes.values_mut() {
        ids.remove(agent);
    }
    s.controls
        .runtime
        .routes
        .insert(name.into(), BTreeSet::from([agent.into()]));
}

fn routed(s: &State, binding: &Binding, agent: &str) -> bool {
    binding.state.work.agent == agent
        || match s.controls.runtime.routes.get(agent) {
            Some(ids) => ids.contains(&binding.state.work.agent),
            None => binding.routing_agent == agent,
        }
}

impl Store {
    pub fn result_delivered(&self, agent: &str, session: &str) -> anyhow::Result<()> {
        self.change(|s| {
            let keys: Vec<_> = s
                .controls
                .runtime
                .work
                .values()
                .filter(|b| {
                    b.state.work.session == session
                        && (b.routing_agent == agent || b.state.work.agent == agent)
                        && matches!(b.state.work.kind, WorkKind::Child { .. })
                })
                .map(|b| child_key(&b.state.work.agent, session))
                .collect();
            s.controls.runtime.delivered_children.extend(keys);
            Ok(())
        })
    }

    pub fn child_result_delivered(&self, agent: &str, session: &str) -> anyhow::Result<bool> {
        self.read(|s| {
            Ok(s.controls
                .runtime
                .delivered_children
                .contains(&child_key(agent, session)))
        })
    }
    pub fn register_agent_route(&self, agent: &str, routing_agent: &str) -> anyhow::Result<()> {
        self.change(|s| {
            crate::validation::text(agent)?;
            crate::validation::text(routing_agent)?;
            route(s, agent, routing_agent);
            Ok(())
        })
    }

    pub fn work_status(&self, agent: &str, session: &str) -> anyhow::Result<Option<WorkState>> {
        self.read(|s| {
            Ok(s.controls
                .runtime
                .work
                .values()
                .filter(|b| b.state.work.session == session && routed(s, b, agent))
                .max_by_key(|b| b.state.work.generation)
                .map(|b| b.state.clone()))
        })
    }
    /// Shared with control admission. Hold only through dispatch, never an owner wait.
    pub fn dispatch_fence(&self) -> anyhow::Result<std::fs::File> {
        link_files::create_private_dir(self.dir())?;
        link_files::lock_file(&self.dir().join(super::constants::DISPATCH_LOCK))
    }

    pub fn register_work(&self, admission: WorkAdmission) -> anyhow::Result<Binding> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            for value in [
                &admission.agent,
                &admission.routing_agent,
                &admission.session,
                &admission.attempt,
            ] {
                crate::validation::text(value)?;
            }
            ensure!(
                !s.controls.runtime.work.contains_key(&admission.attempt),
                "attempt already admitted"
            );
            route(s, &admission.agent, &admission.routing_agent);
            let previous = s
                .controls
                .runtime
                .work
                .values()
                .find(|b| {
                    !b.retired
                        && b.state.work.owner == admission.owner
                        && b.state.work.agent == admission.agent
                        && b.state.work.session == admission.session
                })
                .cloned();
            if let Some(previous) = &previous {
                ensure!(previous.state.work.kind == admission.kind, "work kind changed across attempts");
                ensure!(!previous.active, "another attempt still owns this work");
                ensure!(
                    previous.state.lifecycle == Lifecycle::Open
                        && previous.state.pause_reasons.is_empty()
                        && !matches!(
                            previous.state.observed,
                            Execution::Stopped | Execution::Unknown
                        ),
                    "work is paused, stopped or archived; explicitly resume it first"
                );
            }
            if let WorkKind::Occurrence {occurrence} = &admission.kind {
                ensure!(schedules::schedule_allows(s, &admission.agent, occurrence, previous.is_none()),
                    "schedule was paused, archived or replaced before occurrence admission");
            }
            let child_key = child_key(&admission.agent, &admission.session);
            let parent = admission
                .parent_attempt
                .or_else(|| s.controls.runtime.children.get(&child_key).cloned())
                .or_else(|| previous.as_ref().and_then(|b| b.parent_attempt.clone()));
            let parent = parent.or_else(|| if let WorkKind::Occurrence {occurrence} = &admission.kind {
                s.controls.runtime.work.values().find(|b| !b.retired && b.state.work.agent == admission.agent
                    && matches!(&b.state.work.kind, WorkKind::Schedule {id, ..} if *id == occurrence.schedule))
                    .map(|b| b.state.work.attempt.clone())
            } else {None});
            if let Some(parent) = &parent {
                let p = s
                    .controls
                    .runtime
                    .work
                    .get(parent)
                    .context("unknown parent work")?;
                ensure!(
                    !p.retired && p.state.work.owner == admission.owner,
                    "stale or foreign parent work"
                );
                if let WorkKind::Child {id} = &admission.kind
                    && previous.as_ref().is_none_or(|b| b.state.work.generation == 0) {
                    ensure!(previous.as_ref().is_some_and(|c| c.wake && progress(c)) || children::allows_start(s, p, id), "child start is held or its coordinator is fenced");
                }
            }
            ensure!(
                !admission.session.starts_with("task-") || parent.is_some(),
                "child has no host-issued lineage"
            );
            let generation = s.controls.runtime.work.values().filter(|b|
                b.state.work.owner == admission.owner && b.state.work.agent == admission.agent && b.state.work.session == admission.session
            ).map(|b| b.state.work.generation).max().unwrap_or(0) + 1;
            let plan_revision = previous.as_ref().map_or(0, |p| p.plan_revision);
            let starts: Vec<_> = previous.as_ref().into_iter().flat_map(|old| s.controls.batches.values().flat_map(|b| &b.members)
                .filter(|m| m.work.attempt == old.state.work.attempt && matches!(m.operation, ControlOperation::StartPreparedWork)))
                .filter(|m| !s.controls.receipts.iter().any(|r| r.correlation == m.id.as_str()
                    && matches!(r.stage, ReceiptStage::Applied | ReceiptStage::Denied | ReceiptStage::Stale)))
                .cloned().collect();
            if let Some(previous) = previous {
                let old = &previous.state.work;
                invalidate(s, old, "attempt replaced", true);
                s.controls
                    .runtime
                    .work
                    .get_mut(&old.attempt)
                    .expect("previous work")
                    .retired = true;
                for child in s.controls.runtime.work.values_mut() {
                    if child.parent_attempt.as_deref() == Some(&old.attempt) {
                        child.parent_attempt = Some(admission.attempt.clone());
                    }
                }
                for parent in s.controls.runtime.children.values_mut() {
                    if *parent == old.attempt {
                        *parent = admission.attempt.clone();
                    }
                }
            }
            let binding = Binding {
                owner_route: admission.owner_route.or_else(|| parent.as_ref().and_then(|p| s.controls.runtime.work.get(p)).and_then(|b| b.owner_route.clone())),
                token: std::sync::Arc::new(link_secretive::Secret::new(link_files::random_token()?)),
                state: WorkState {
                    work: WorkRef {
                        kind: admission.kind,
                        owner: admission.owner,
                        agent: admission.agent,
                        session: admission.session.clone(),
                        task: admission.session.strip_prefix("task-").map(str::to_owned),
                        attempt: admission.attempt,
                        generation,
                        control_revision: 0,
                    },
                    lifecycle: Lifecycle::Open,
                    pause_reasons: BTreeSet::new(),
                    observed: Execution::Running,
                    checkpoint: admission.checkpoint,
                    in_flight_effects: BTreeSet::new(),
                },
                routing_agent: admission.routing_agent,
                parent_attempt: parent,
                active: true,
                retired: false,
                interrupt: false,
                plan_revision,
                wake: false,
            };
            s.controls.runtime.revision += 1;
            s.controls.runtime.delivered_children.remove(&child_key);
            s.controls
                .runtime
                .work
                .insert(binding.state.work.attempt.clone(), binding.clone());
            for start in starts {
                receipt(s, start.id.as_str(), &start.work, ReceiptStage::Applied, "host admitted the prepared work under its exclusive lease");
            }
            Ok(binding)
        })
    }

    /// Only the host passes this unguessable binding in tool metadata, outside model arguments.
    pub fn authenticate_work(
        &self,
        token: &str,
        agent: &str,
        session: &str,
        attempt: &str,
    ) -> anyhow::Result<WorkRef> {
        self.read(|s| {
            let b = bound(s, token)?;
            ensure!(
                b.active
                    && !b.retired
                    && b.state.work.agent == agent
                    && b.state.work.session == session
                    && b.state.work.attempt == attempt,
                "stale or foreign work binding"
            );
            ensure!(progress(b), "work is fenced");
            Ok(b.state.work.clone())
        })
    }

    pub fn source_work(
        &self,
        token: &str,
        routing_agent: Option<&str>,
        session: &str,
        attempt: &str,
        revision: u64,
    ) -> anyhow::Result<WorkRef> {
        self.read(|s| {
            let b = bound(s, token)?;
            ensure!(
                b.active
                    && progress(b)
                    && b.state.work.session == session
                    && b.state.work.attempt == attempt
                    && b.state.work.control_revision == revision
                    && routing_agent.is_none_or(|a| b.routing_agent == a),
                "source has a stale or foreign work binding"
            );
            Ok(b.state.work.clone())
        })
    }

    pub fn works(&self, routing_agent: &str) -> anyhow::Result<Vec<Binding>> {
        self.read(|s| {
            Ok(s.controls
                .runtime
                .work
                .values()
                .filter(|b| {
                    !b.retired
                        && routed(s, b, routing_agent)
                        && b.state.lifecycle != Lifecycle::Deleted
                })
                .cloned()
                .collect())
        })
    }

    pub fn has_agent_work(&self, agent: &str) -> anyhow::Result<bool> {
        self.read(|s| {
            Ok(s.controls.runtime.work.values().any(|b| {
                routed(s, b, agent) && !matches!(b.state.work.kind, WorkKind::Schedule { .. })
            }))
        })
    }

    /// Question delivery follows the stable identity currently assigned to a transport name.
    pub fn is_agent_route(&self, agent: &str, name: &str) -> anyhow::Result<bool> {
        self.read(|s| {
            Ok(s.controls
                .runtime
                .routes
                .get(name)
                .is_some_and(|ids| ids.contains(agent)))
        })
    }

    pub fn owner_route(&self, work: &WorkRef) -> anyhow::Result<Option<String>> {
        self.read(|s| {
            Ok(s.controls
                .runtime
                .work
                .get(&work.attempt)
                .and_then(|b| b.owner_route.clone()))
        })
    }

    pub fn lineage(&self) -> anyhow::Result<LineageSnapshot> {
        self.read(|s| {
            Ok(LineageSnapshot {
                revision: s.controls.runtime.revision,
                members: s
                    .controls
                    .runtime
                    .work
                    .values()
                    .filter(|b| !b.retired && b.state.lifecycle != Lifecycle::Deleted)
                    .map(|b| LineageMember {
                        work: b.state.work.clone(),
                        parent_attempt: b.parent_attempt.clone(),
                    })
                    .collect(),
            })
        })
    }

    pub fn ready_work(&self, routing_agent: &str) -> anyhow::Result<Vec<WorkRef>> {
        self.read(|s| {
            Ok(s.controls
                .runtime
                .work
                .values()
                .filter(|b| {
                    if matches!(b.state.work.kind, WorkKind::Schedule { .. })
                        || b.state.checkpoint.is_none()
                    {
                        return false;
                    }
                    !b.active
                        && !b.retired
                        && b.routing_agent == routing_agent
                        && progress(b)
                        && (b.wake
                            || s.controls.inputs.values().any(|i| {
                                i.target.agent == b.state.work.agent
                                    && i.target.session == b.state.work.session
                                    && i.target.owner == b.state.work.owner
                                    && i.target.kind == b.state.work.kind
                                    && (inputs::queued(s, i)
                                        || i.target.attempt == b.state.work.attempt)
                                    && !s.controls.receipts.iter().any(|r| {
                                        r.correlation == i.id.as_str()
                                            && r.stage == ReceiptStage::Included
                                    })
                            }))
                })
                .map(|b| b.state.work.clone())
                .collect())
        })
    }

    /// Caller holds the dispatch fence through the actual provider/tool admission.
    pub fn work_step(&self, token: &str, _step: Step) -> anyhow::Result<WorkRef> {
        self.read(|s| {
            let b = bound(s, token)?;
            ensure!(
                b.active && progress(b),
                "work is paused, stopped, redirected or archived"
            );
            Ok(b.state.work.clone())
        })
    }

    pub fn interrupted_work(&self, token: &str) -> anyhow::Result<bool> {
        self.read(|s| {
            let b = bound(s, token)?;
            Ok(b.interrupt || b.retired || !b.active)
        })
    }

    pub fn finish_work(&self, token: &str, checkpoint: Option<String>) -> anyhow::Result<()> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| controls::finish(s, token, checkpoint))
    }

    /// The host calls this only after taking the conversation's exclusive lease.
    pub fn recover_work(&self, agent: &str, session: &str) -> anyhow::Result<()> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            let old = s
                .controls
                .runtime
                .work
                .values()
                .find(|b| {
                    !b.retired
                        && b.active
                        && b.state.work.agent == agent
                        && b.state.work.session == session
                })
                .cloned();
            if let Some(old) = old {
                invalidate(
                    s,
                    &old.state.work,
                    "host recovered a lost attempt lease",
                    true,
                );
                for record in s.records.values_mut() {
                    if record
                        .snapshot
                        .question
                        .requester
                        .work
                        .as_ref()
                        .is_some_and(|w| w.attempt == old.state.work.attempt)
                        && let approval::EffectState::Applying { claim } = &record.snapshot.effect
                    {
                        record.snapshot.effect = approval::EffectState::Unknown {
                            claim: claim.clone(),
                            reason: "host lost the source attempt; never replay".into(),
                        };
                    }
                }
                effects::lost(s, &old.state.work);
                controls::finish(s, old.token.expose(), None)?;
            }
            Ok(())
        })
    }

    pub fn continue_work(&self, token: &str) -> anyhow::Result<()> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            let b = bound_mut(s, token)?;
            ensure!(progress(b), "work remains fenced");
            b.active = true;
            b.state.observed = Execution::Running;
            Ok(())
        })
    }
}

fn bound<'a>(s: &'a State, token: &str) -> anyhow::Result<&'a Binding> {
    let candidate = link_secretive::Secret::new(token.to_owned());
    s.controls
        .runtime
        .work
        .values()
        .find(|b| b.token.as_ref() == &candidate && !token.is_empty())
        .context("unknown work binding")
}
fn bound_mut<'a>(s: &'a mut State, token: &str) -> anyhow::Result<&'a mut Binding> {
    let candidate = link_secretive::Secret::new(token.to_owned());
    s.controls
        .runtime
        .work
        .values_mut()
        .find(|b| b.token.as_ref() == &candidate && !token.is_empty())
        .context("unknown work binding")
}
fn progress(b: &Binding) -> bool {
    !b.retired
        && !b.interrupt
        && b.state.lifecycle == Lifecycle::Open
        && b.state.pause_reasons.is_empty()
        && b.state.observed != Execution::Unknown
}
fn child_key(agent: &str, session: &str) -> String {
    format!("{agent}/{session}")
}

pub(crate) use controls::reconciled;
pub(crate) use controls::{apply, supersede};

pub(crate) fn claim_current(s: &State, work: &WorkRef) -> bool {
    match s.controls.runtime.work.get(&work.attempt) {
        Some(b) => b.active && progress(b) && b.state.work == *work,
        None => !replaced(s, work),
    }
}

/// Whether `work`, whose attempt has no binding, is of work the host
/// registered under another attempt: an attempt retired and since
/// removed ([`crate::retention`]), stale as its retired binding was.
pub(crate) fn replaced(s: &State, work: &WorkRef) -> bool {
    let work = same_work(work);
    s.controls
        .runtime
        .work
        .values()
        .any(|b| same_work(&b.state.work) == work)
}

/// The work `work` is an attempt of: its reference with what each attempt
/// changes (its attempt, generation and control revision) left out.
pub(crate) fn same_work(work: &WorkRef) -> WorkRef {
    WorkRef {
        attempt: String::new(),
        generation: 0,
        control_revision: 0,
        ..work.clone()
    }
}
