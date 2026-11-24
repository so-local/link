use super::*;

impl Store {
    /// Queue is delivered only at a new turn; Steer joins the next safe model boundary.
    pub fn waiting_inputs(&self, token: &str, new_turn: bool) -> anyhow::Result<Vec<UserInput>> {
        self.read(|s| {
            let b = bound(s, token)?;
            ensure!(progress(b), "work cannot take input");
            let mut inputs: Vec<_> = s
                .controls
                .inputs
                .values()
                .filter(|input| {
                    input.target.agent == b.state.work.agent
                        && input.target.session == b.state.work.session
                        && input.target.owner == b.state.work.owner
                        && input.target.kind == b.state.work.kind
                        && (queued(s, input) || input.target.attempt == b.state.work.attempt)
                        && (new_turn || !queued(s, input))
                        && !s.controls.runtime.delivered_inputs.contains_key(&input.id)
                        && !s.controls.receipts.iter().any(|r| {
                            r.correlation == input.id.as_str()
                                && matches!(
                                    r.stage,
                                    ReceiptStage::Included | ReceiptStage::RedirectApplied
                                )
                        })
                })
                .cloned()
                .collect();
            inputs.sort_by_key(|input| {
                s.controls
                    .receipts
                    .iter()
                    .find(|r| r.correlation == input.id.as_str())
                    .map(|r| r.sequence)
            });
            Ok(inputs)
        })
    }

    pub fn input_delivered(&self, token: &str, input: &InputId, event: u64) -> anyhow::Result<()> {
        self.change(|s| {
            let b = bound(s, token)?;
            let pending = s.controls.inputs.get(input).context("unknown input")?;
            ensure!(
                pending.target.agent == b.state.work.agent
                    && pending.target.session == b.state.work.session
                    && pending.actor == b.state.work.owner,
                "foreign input delivery"
            );
            if let Some(previous) = s
                .controls
                .runtime
                .delivered_inputs
                .insert(input.clone(), event)
            {
                ensure!(
                    previous == event,
                    "input already delivered to another event"
                );
            }
            let target = pending.target.clone();
            receipt(
                s,
                input.as_str(),
                &target,
                ReceiptStage::Delivered,
                "written to the host conversation",
            );
            Ok(())
        })
    }

    /// Report inclusion only after the provider actually answered a request containing the event.
    pub fn inputs_included(&self, token: &str, ids: &[InputId]) -> anyhow::Result<()> {
        self.change(|s| {
            let b = bound(s, token)?;
            let work = b.state.work.clone();
            let included: Vec<_> = s.controls.inputs.values().filter(|i| i.target.agent == work.agent
                && i.target.session == work.session && i.target.owner == work.owner
                && ids.contains(&i.id) && s.controls.runtime.delivered_inputs.contains_key(&i.id)
                && !s.controls.receipts.iter().any(|r| r.correlation == i.id.as_str() && r.stage == ReceiptStage::Included))
                .cloned().collect();
            for input in included {
                receipt(s, input.id.as_str(), &input.target, ReceiptStage::Included, "provider answered with this input in its context");
                let redirects: Vec<_> = s.controls.batches.values().flat_map(|b| &b.members)
                    .filter(|m| matches!(&m.operation, ControlOperation::Redirect {input: id} if *id == input.id)).cloned().collect();
                for control in redirects {
                    receipt(s, control.id.as_str(), &control.work, ReceiptStage::RedirectApplied, "redirect input included after the old path stopped");
                }
            }
            Ok(())
        })
    }
}

pub(super) fn queued(s: &State, input: &UserInput) -> bool {
    input.admission == Admission::Queue
        || s.controls
            .receipts
            .iter()
            .find(|r| r.correlation == input.id.as_str())
            .is_some_and(|r| r.stage == ReceiptStage::Queued)
}
