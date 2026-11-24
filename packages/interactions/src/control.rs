use crate::{
    store::{State, close, event},
    *,
};
use anyhow::{Context, ensure};
pub use link_interactions_contract::control::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod batches;
pub use batches::BatchStatus;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlState {
    pub inputs: BTreeMap<InputId, UserInput>,
    pub batches: BTreeMap<ControlId, ResolvedControlBatch>,
    pub receipts: Vec<Receipt>,
    #[serde(default)]
    pub runtime: crate::runtime::RuntimeState,
}

impl Store {
    /// The host holds its task/lineage dispatch fence across this admission.
    pub fn control_work(
        &self,
        control: WorkControl,
        lineage: &LineageSnapshot,
        now: u64,
    ) -> anyhow::Result<ResolvedControlBatch> {
        let _fence = self.dispatch_fence()?;
        self.change(|state| {
            if let Some(batch) = state.controls.batches.get(&control.id) {
                ensure!(
                    batch.control == control,
                    "control ID reused with different content"
                );
                return Ok(batch.clone());
            }
            ensure!(
                state.controls.runtime.work.is_empty()
                    || lineage.revision == state.controls.runtime.revision,
                "lineage changed before control admission; resolve the selection again"
            );
            ensure!(
                !state
                    .controls
                    .batches
                    .values()
                    .any(|b| b.control.transport_event == control.transport_event),
                "transport event already admitted under another control ID"
            );
            let targets = link_controls::select(&control.actor, &control.selector, lineage)?;
            ensure!(
                targets.len() <= 1
                    || !matches!(control.operation, ControlOperation::Redirect { .. }),
                "Redirect requires one exact input target"
            );
            if let ControlOperation::Redirect { input } = &control.operation {
                let pending = state
                    .controls
                    .inputs
                    .get(input)
                    .context("unknown redirect input")?;
                ensure!(
                    targets.contains(&pending.target) && pending.actor == control.actor,
                    "wrong redirect target"
                );
                ensure!(
                    !state
                        .controls
                        .receipts
                        .iter()
                        .any(|r| r.correlation == input.as_str()
                            && matches!(
                                r.stage,
                                ReceiptStage::Included | ReceiptStage::RedirectApplied
                            )),
                    "input already included"
                );
            }
            let id = BatchId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?;
            let multiple = targets.len() > 1;
            let mut members = Vec::new();
            for target in targets {
                let mut operation = control.operation.clone();
                if multiple && let ControlOperation::AddHold { id, .. } = &mut operation {
                    *id = HoldId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?;
                }
                let member = AddressedControl {
                    id: ControlId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?,
                    batch: id.clone(),
                    work: target.clone(),
                    operation,
                };
                receipt(
                    state,
                    member.id.as_str(),
                    &target,
                    ReceiptStage::Accepted,
                    "owner control recorded",
                );
                match crate::runtime::apply(state, &member, &control.actor) {
                    Ok(()) => {
                        if !matches!(
                            control.operation,
                            ControlOperation::Resume { .. }
                                | ControlOperation::StartPreparedWork
                                | ControlOperation::ReleaseHold { .. }
                                | ControlOperation::Unarchive
                        ) {
                            invalidate(state, &target, "work control superseded permission", true);
                        }
                    }
                    Err(err) => {
                        receipt(
                            state,
                            member.id.as_str(),
                            &target,
                            ReceiptStage::Denied,
                            &err.to_string(),
                        );
                    }
                }
                members.push(member);
            }
            let batch = ResolvedControlBatch {
                id,
                control: control.clone(),
                lineage_revision: lineage.revision,
                members,
            };
            state.controls.batches.insert(control.id, batch.clone());
            event(state, None, "control_accepted", now)?;
            Ok(batch)
        })
    }

    pub fn admit_input(
        &self,
        input: UserInput,
        host: &WorkState,
        now: u64,
    ) -> anyhow::Result<Receipt> {
        let _fence = self.dispatch_fence()?;
        let stage = link_controls::admit(&input, host)?;
        self.change(|state| {
            if let Some(existing) = state.controls.inputs.get(&input.id) {
                ensure!(*existing == input, "input ID reused with changed content");
                return state
                    .controls
                    .receipts
                    .iter()
                    .find(|r| r.correlation == input.id.as_str())
                    .cloned()
                    .context("missing input receipt");
            }
            ensure!(
                !state
                    .controls
                    .inputs
                    .values()
                    .any(|i| i.transport_event == input.transport_event),
                "transport event already admitted under another input ID"
            );
            if input.admission == Admission::Steer && stage != ReceiptStage::Queued {
                invalidate(state, &input.target, "superseded by steering", true);
                crate::runtime::supersede(state, &input.target)?;
            }
            state
                .controls
                .inputs
                .insert(input.id.clone(), input.clone());
            let got = receipt(
                state,
                input.id.as_str(),
                &input.target,
                stage,
                "authenticated owner input recorded",
            );
            event(state, None, "input_accepted", now)?;
            Ok(got)
        })
    }

    /// Only the authenticated runtime adapter reports execution observations.
    pub fn record_receipt(
        &self,
        correlation: &str,
        target: &WorkRef,
        stage: ReceiptStage,
        evidence: &str,
        now: u64,
    ) -> anyhow::Result<Receipt> {
        self.change(|state| {
            let known = state
                .controls
                .inputs
                .values()
                .any(|i| i.id.as_str() == correlation && i.target == *target)
                || state
                    .controls
                    .batches
                    .values()
                    .flat_map(|b| &b.members)
                    .any(|m| m.id.as_str() == correlation && m.work == *target);
            ensure!(known, "unknown or stale receipt target");
            validation::text(evidence)?;
            let previous = state
                .controls
                .receipts
                .iter()
                .rev()
                .find(|r| r.correlation == correlation);
            if let Some(previous) = previous {
                ensure!(
                    link_controls::receipt_progress(&previous.stage, &stage),
                    "receipt cannot move backward or revise a terminal observation"
                );
            }
            if let Some(existing) = state
                .controls
                .receipts
                .iter()
                .find(|r| r.correlation == correlation && r.stage == stage)
            {
                ensure!(
                    existing.evidence == evidence && existing.target == *target,
                    "receipt changed"
                );
                return Ok(existing.clone());
            }
            let got = receipt(state, correlation, target, stage, evidence);
            event(state, None, "receipt", now)?;
            Ok(got)
        })
    }

    pub fn receipts(&self, correlation: &str) -> anyhow::Result<Vec<Receipt>> {
        self.read(|s| {
            Ok(s.controls
                .receipts
                .iter()
                .filter(|r| r.correlation == correlation)
                .cloned()
                .collect())
        })
    }
}

pub(crate) fn invalidate(state: &mut State, target: &WorkRef, reason: &str, information: bool) {
    for record in state.records.values_mut() {
        if record.snapshot.question.requester.work.as_ref() == Some(target)
            && (information
                || matches!(
                    record.snapshot.question.effect,
                    approval::Effect::Authorize(_)
                ))
        {
            close(record, reason);
        }
    }
}

pub(crate) fn receipt(
    state: &mut State,
    correlation: &str,
    target: &WorkRef,
    stage: ReceiptStage,
    evidence: &str,
) -> Receipt {
    let receipt = Receipt {
        // Numbered after the newest kept: older ones leave (crate::retention).
        sequence: state.controls.receipts.last().map_or(1, |r| r.sequence + 1),
        correlation: correlation.into(),
        target: target.clone(),
        stage,
        evidence: evidence.into(),
    };
    state.controls.receipts.push(receipt.clone());
    state.events.push(delivery::Event {
        id: state.events.last().map_or(1, |e| e.id + 1),
        request: None,
        target: Some(target.clone()),
        receipt: Some(receipt.clone()),
        kind: "receipt".into(),
        at: crate::now(),
    });
    receipt
}
