//! Pure control transitions over supplied host snapshots, without an executor.

use anyhow::ensure;
use link_interactions_contract::{control::*, *};
use std::collections::BTreeSet;

/// Resolve once under the host's lineage/spawn fence; persist the exact list.
pub fn select(
    actor: &Actor,
    selector: &ControlSelector,
    lineage: &LineageSnapshot,
) -> anyhow::Result<Vec<WorkRef>> {
    let members = match selector {
        ControlSelector::Exact { work } => vec![work.clone()],
        ControlSelector::Selected { members } => {
            ensure!(!members.is_empty(), "empty control selection");
            members.clone()
        }
        ControlSelector::Tree { root, scope } => {
            ensure!(
                lineage.members.iter().any(|m| m.work == *root),
                "stale tree root"
            );
            let mut attempts = BTreeSet::from([root.attempt.clone()]);
            loop {
                let old = attempts.len();
                for member in &lineage.members {
                    if member
                        .parent_attempt
                        .as_ref()
                        .is_some_and(|p| attempts.contains(p))
                    {
                        attempts.insert(member.work.attempt.clone());
                    }
                }
                if old == attempts.len() {
                    break;
                }
            }
            lineage
                .members
                .iter()
                .filter(|m| match scope {
                    TreeScope::SelfOnly => m.work == *root,
                    TreeScope::Descendants => m.work != *root && attempts.contains(&m.work.attempt),
                    TreeScope::Subtree => attempts.contains(&m.work.attempt),
                })
                .map(|m| m.work.clone())
                .collect()
        }
    };
    let mut unique = BTreeSet::new();
    for work in members {
        ensure!(work.owner == *actor, "control belongs to another owner");
        ensure!(
            lineage.members.iter().any(|m| m.work == work),
            "stale control member"
        );
        unique.insert(work);
    }
    Ok(unique.into_iter().collect())
}

pub fn admit(input: &UserInput, state: &WorkState) -> anyhow::Result<ReceiptStage> {
    ensure!(
        input.actor == state.work.owner && input.content.supplied_by == input.actor,
        "input is not authenticated owner text"
    );
    ensure!(input.target == state.work, "stale input target");
    ensure!(state.lifecycle != Lifecycle::Deleted, "work is deleted");
    Ok(
        if input.admission == Admission::Queue || !can_progress(state) {
            ReceiptStage::Queued
        } else {
            ReceiptStage::Accepted
        },
    )
}

pub fn can_progress(state: &WorkState) -> bool {
    state.lifecycle == Lifecycle::Open
        && state.pause_reasons.is_empty()
        && !matches!(
            state.observed,
            Execution::Prepared | Execution::Stopped | Execution::Unknown
        )
}

/// Desired restrictions change first. Observed pause/shutdown requires a host receipt.
pub fn transition(state: &WorkState, control: &AddressedControl) -> anyhow::Result<WorkState> {
    ensure!(state.work == control.work, "stale work revision");
    ensure!(state.lifecycle != Lifecycle::Deleted, "work is deleted");
    let mut next = state.clone();
    match &control.operation {
        ControlOperation::Stop => {
            next.pause_reasons.insert(control.id.clone());
            next.observed = Execution::Stopping;
        }
        ControlOperation::Pause { boundary } => {
            next.pause_reasons.insert(control.id.clone());
            next.observed = match boundary {
                PauseBoundary::FinishModelResponse => Execution::Pausing,
                PauseBoundary::InterruptCurrentStep => Execution::Stopping,
            };
        }
        ControlOperation::Resume { reasons } => {
            ensure!(state.lifecycle == Lifecycle::Open, "work is not open");
            ensure!(
                state.in_flight_effects.is_empty(),
                "reconcile effects before resumption"
            );
            for reason in reasons {
                next.pause_reasons.remove(reason);
            }
            // Permitting execution is not evidence that a model has restarted.
        }
        ControlOperation::StartPreparedWork => {
            ensure!(
                state.lifecycle == Lifecycle::Open
                    && state.observed == Execution::Prepared
                    && state.pause_reasons.is_empty(),
                "work cannot start"
            );
        }
        ControlOperation::Archive => {
            next.lifecycle = Lifecycle::Archiving;
            next.pause_reasons.insert(control.id.clone());
            next.observed = Execution::Stopping;
        }
        ControlOperation::Unarchive => {
            ensure!(
                state.lifecycle == Lifecycle::Archived,
                "work is not archived"
            );
            next.lifecycle = Lifecycle::Open;
            next.pause_reasons.insert(control.id.clone());
            next.observed = Execution::Paused;
        }
        ControlOperation::Delete { scope } => {
            ensure!(
                !matches!(scope, DeleteScope::TaskRecord)
                    || matches!(state.work.kind, WorkKind::Child { .. }),
                "task-record deletion needs a retained child"
            );
            ensure!(
                matches!(
                    state.observed,
                    Execution::Paused | Execution::Stopped | Execution::Prepared
                ) && state.in_flight_effects.is_empty(),
                "stop and reconcile work before deletion"
            );
            next.lifecycle = Lifecycle::Deleting;
        }
        ControlOperation::Redirect { .. } => {
            ensure!(state.lifecycle == Lifecycle::Open, "work is not open");
            next.observed = Execution::Stopping;
        }
        ControlOperation::AddHold { terms, .. } => {
            ensure!(!terms.blocked_actions.is_empty(), "empty delegation hold");
        }
        ControlOperation::ReleaseHold { .. } => {}
    }
    next.work.control_revision = next
        .work
        .control_revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("control revision exhausted"))?;
    Ok(next)
}

pub fn blocked_by(
    holds: &[Hold],
    coordinator: &WorkRef,
    child: Option<&str>,
    action: &DelegationAction,
) -> Vec<HoldId> {
    holds
        .iter()
        .filter(|hold| {
            !hold.released
                && hold.coordinator.owner == coordinator.owner
                && hold.coordinator.agent == coordinator.agent
                && hold.coordinator.session == coordinator.session
                && hold.coordinator.task == coordinator.task
                && hold.terms.blocked_actions.contains(action)
                && (*action != DelegationAction::DispatchChildMessage || child.is_some())
                && hold
                    .terms
                    .children
                    .as_ref()
                    .is_none_or(|set| child.is_some_and(|c| set.contains(c)))
        })
        .map(|h| h.id.clone())
        .collect()
}

pub fn release(hold: &Hold, actor: &Actor) -> anyhow::Result<Hold> {
    ensure!(
        *actor == hold.imposed_by && *actor == hold.coordinator.owner,
        "only the imposing owner may release this hold"
    );
    let mut next = hold.clone();
    next.released = true;
    Ok(next)
}

pub fn receipt_progress(previous: &ReceiptStage, next: &ReceiptStage) -> bool {
    use ReceiptStage::*;
    previous == next
        || match previous {
            Accepted => matches!(
                next,
                Queued
                    | Delivered
                    | StopRequested
                    | HoldRequested
                    | HoldActive
                    | Pausing
                    | Applied
                    | Stale
                    | Denied
                    | Unsupported
                    | Partial
                    | Unknown
            ),
            Queued => matches!(next, Delivered | Stale | Denied | Unsupported | Unknown),
            Delivered => matches!(next, Included | Stale | Denied | Unknown),
            StopRequested => matches!(next, Stopped | Partial | Denied | Unsupported | Unknown),
            Stopped => matches!(next, RedirectApplied | Applied),
            HoldRequested => matches!(next, HoldActive | Denied | Unsupported | Unknown),
            Pausing => matches!(next, Paused | Denied | Unsupported | Unknown),
            Unknown | Partial => matches!(
                next,
                Delivered
                    | Included
                    | Stopped
                    | Paused
                    | Applied
                    | RedirectApplied
                    | Denied
                    | Unsupported
                    | Partial
                    | Unknown
            ),
            _ => false,
        }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);
