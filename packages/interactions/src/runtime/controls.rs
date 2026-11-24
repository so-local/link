use super::*;

pub(crate) fn apply(
    s: &mut State,
    control: &AddressedControl,
    actor: &Actor,
) -> anyhow::Result<()> {
    let Some(binding) = s.controls.runtime.work.get(&control.work.attempt) else {
        ensure!(!super::replaced(s, &control.work), "stale host work");
        return Ok(());
    };
    ensure!(
        binding.state.work == control.work && !binding.retired,
        "stale host work"
    );
    let mut next = binding.clone();
    next.state = link_controls::transition(&binding.state, control)?;
    match &control.operation {
        ControlOperation::Stop
        | ControlOperation::Archive
        | ControlOperation::Redirect { .. }
        | ControlOperation::Pause {
            boundary: PauseBoundary::InterruptCurrentStep,
        } => next.interrupt = true,
        ControlOperation::Resume { .. } => {
            if next.state.pause_reasons.is_empty() {
                next.wake = true;
                next.interrupt = false;
                if !next.active && next.state.in_flight_effects.is_empty() {
                    next.state.observed = Execution::Prepared;
                }
            }
        }
        ControlOperation::StartPreparedWork => next.wake = true,
        ControlOperation::AddHold { id, terms } => {
            ensure!(
                !s.controls.runtime.holds.contains_key(id),
                "hold already exists"
            );
            s.controls.runtime.holds.insert(
                id.clone(),
                Hold {
                    id: id.clone(),
                    imposed_by: actor.clone(),
                    coordinator: control.work.clone(),
                    terms: terms.clone(),
                    released: false,
                },
            );
        }
        ControlOperation::ReleaseHold { hold } => {
            next.wake = true;
            let h = s.controls.runtime.holds.get(hold).context("unknown hold")?;
            ensure!(
                h.coordinator.agent == control.work.agent
                    && h.coordinator.session == control.work.session,
                "hold belongs to different work"
            );
            let released = link_controls::release(h, actor)?;
            s.controls.runtime.holds.insert(hold.clone(), released);
            for proposal in s.controls.runtime.proposals.values_mut() {
                if proposal.coordinator.agent == control.work.agent
                    && proposal.coordinator.session == control.work.session
                    && proposal.state == ProposalState::Held
                {
                    proposal.state = ProposalState::NeedsRevalidation;
                }
            }
        }
        _ => {}
    }
    if !next.active {
        if !next.state.in_flight_effects.is_empty() {
            next.state.observed = Execution::Unknown;
        } else {
            next.state.observed = match next.state.observed {
                Execution::Stopping => Execution::Stopped,
                Execution::Pausing => Execution::Paused,
                ref other => other.clone(),
            };
            if next.state.lifecycle == Lifecycle::Archiving {
                next.state.lifecycle = Lifecycle::Archived;
            }
            if matches!(control.operation, ControlOperation::Pause { .. }) {
                next.state.observed = Execution::Paused;
            }
        }
    }
    let stage = match &control.operation {
        ControlOperation::StartPreparedWork => ReceiptStage::Accepted,
        ControlOperation::Delete { .. } => ReceiptStage::Accepted,
        ControlOperation::Archive => {
            if next.active {
                ReceiptStage::StopRequested
            } else if next.state.lifecycle == Lifecycle::Archived {
                ReceiptStage::Applied
            } else {
                ReceiptStage::Unknown
            }
        }
        ControlOperation::Stop | ControlOperation::Redirect { .. } => {
            if next.active {
                ReceiptStage::StopRequested
            } else if next.state.observed == Execution::Unknown {
                ReceiptStage::Unknown
            } else {
                ReceiptStage::Stopped
            }
        }
        ControlOperation::Pause { .. } => {
            if next.active {
                ReceiptStage::Pausing
            } else if next.state.observed == Execution::Unknown {
                ReceiptStage::Unknown
            } else {
                ReceiptStage::Paused
            }
        }
        ControlOperation::AddHold { .. } => ReceiptStage::HoldActive,
        _ => ReceiptStage::Applied,
    };
    s.controls
        .runtime
        .work
        .insert(control.work.attempt.clone(), next);
    s.controls.runtime.revision += 1;
    receipt(
        s,
        control.id.as_str(),
        &control.work,
        stage,
        "host dispatch fence updated",
    );
    Ok(())
}

pub(crate) fn supersede(s: &mut State, work: &WorkRef) -> anyhow::Result<()> {
    ensure!(
        s.controls.runtime.work.contains_key(&work.attempt) || !super::replaced(s, work),
        "stale input work"
    );
    if let Some(b) = s.controls.runtime.work.get_mut(&work.attempt) {
        ensure!(!b.retired && b.state.work == *work, "stale input work");
        b.state.work.control_revision += 1;
        b.plan_revision += 1;
        s.controls.runtime.revision += 1;
    }
    for proposal in s.controls.runtime.proposals.values_mut() {
        if proposal.coordinator.agent == work.agent
            && proposal.coordinator.session == work.session
            && proposal.state == ProposalState::Ready
        {
            proposal.state = ProposalState::NeedsRevalidation;
        }
    }
    Ok(())
}

pub(super) fn finish(s: &mut State, token: &str, checkpoint: Option<String>) -> anyhow::Result<()> {
    if !bound(s, token)?.active {
        return Ok(());
    }
    observe_end(s, token, checkpoint)
}

pub(crate) fn reconciled(s: &mut State, attempt: &str) -> anyhow::Result<()> {
    let Some(b) = s.controls.runtime.work.get(attempt) else {
        return Ok(());
    };
    if b.active || !b.state.in_flight_effects.is_empty() || b.state.observed != Execution::Unknown {
        return Ok(());
    }
    let token = b.token.clone();
    observe_end(s, token.expose(), None)?;
    let redirected = s
        .controls
        .batches
        .values()
        .flat_map(|b| &b.members)
        .any(|m| {
            m.work.attempt == attempt && matches!(m.operation, ControlOperation::Redirect { .. })
        });
    let b = bound_mut(s, token.expose())?;
    if redirected
        && b.interrupt
        && b.state.pause_reasons.is_empty()
        && b.state.lifecycle == Lifecycle::Open
    {
        b.interrupt = false;
        b.wake = true;
        b.state.observed = Execution::Prepared;
    }
    Ok(())
}

fn observe_end(s: &mut State, token: &str, checkpoint: Option<String>) -> anyhow::Result<()> {
    let current = bound(s, token)?;
    let full_stop = s
        .controls
        .batches
        .values()
        .flat_map(|b| &b.members)
        .any(|m| {
            m.work.attempt == current.state.work.attempt
                && matches!(m.operation, ControlOperation::Stop)
                && current.state.pause_reasons.contains(&m.id)
        });
    let b = bound_mut(s, token)?;
    b.active = false;
    b.wake = false;
    if checkpoint.is_some() {
        b.state.checkpoint = checkpoint;
    }
    b.state.observed = if !b.state.in_flight_effects.is_empty() {
        Execution::Unknown
    } else if b.state.lifecycle == Lifecycle::Archiving
        || full_stop
        || (b.interrupt && b.state.pause_reasons.is_empty())
    {
        Execution::Stopped
    } else if !b.state.pause_reasons.is_empty() {
        Execution::Paused
    } else {
        Execution::Prepared
    };
    if b.state.lifecycle == Lifecycle::Archiving && b.state.in_flight_effects.is_empty() {
        b.state.lifecycle = Lifecycle::Archived;
    }
    let work = b.state.work.clone();
    let observed = b.state.observed.clone();
    invalidate(s, &work, "work reached its execution boundary", true);
    let pending: Vec<_> = s
        .controls
        .batches
        .values()
        .flat_map(|batch| &batch.members)
        .filter(|m| {
            m.work.attempt == work.attempt
                && matches!(
                    m.operation,
                    ControlOperation::Stop
                        | ControlOperation::Archive
                        | ControlOperation::Pause { .. }
                        | ControlOperation::Redirect { .. }
                )
        })
        .cloned()
        .collect();
    for control in pending {
        let stage = if observed == Execution::Unknown {
            ReceiptStage::Unknown
        } else {
            match control.operation {
                ControlOperation::Stop | ControlOperation::Redirect { .. } => ReceiptStage::Stopped,
                ControlOperation::Archive => ReceiptStage::Applied,
                ControlOperation::Pause { .. }
                    if observed == Execution::Paused || observed == Execution::Stopped =>
                {
                    ReceiptStage::Paused
                }
                _ => continue,
            }
        };
        if !s
            .controls
            .receipts
            .iter()
            .any(|r| r.correlation == control.id.as_str() && r.stage == stage)
        {
            receipt(
                s,
                control.id.as_str(),
                &control.work,
                stage,
                "runtime ended the active step and retained its checkpoint",
            );
        }
    }
    Ok(())
}

impl Store {
    /// Called after cancellation has settled. Redirect continues only its recorded input.
    pub fn reenter_redirect(&self, token: &str) -> anyhow::Result<bool> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            let b = bound(s, token)?;
            let work = b.state.work.clone();
            let redirect = s
                .controls
                .batches
                .values()
                .flat_map(|batch| &batch.members)
                .filter(|m| m.work.attempt == work.attempt)
                .filter_map(|m| {
                    if let ControlOperation::Redirect { input } = &m.operation {
                        Some(input)
                    } else {
                        None
                    }
                })
                .any(|input| {
                    !s.controls.receipts.iter().any(|r| {
                        r.correlation == input.as_str()
                            && matches!(
                                r.stage,
                                ReceiptStage::Included | ReceiptStage::RedirectApplied
                            )
                    })
                });
            if !redirect
                || !b.interrupt
                || b.state.observed != Execution::Stopped
                || !b.state.in_flight_effects.is_empty()
                || !b.state.pause_reasons.is_empty()
                || b.state.lifecycle != Lifecycle::Open
            {
                return Ok(false);
            }
            let b = bound_mut(s, token)?;
            b.interrupt = false;
            b.active = true;
            b.state.observed = Execution::Running;
            Ok(true)
        })
    }
}
