//! Existing source policy can admit a call without a question; its execution still needs evidence.
use super::*;
use crate::approval::{EffectOutcome, EffectState};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub(crate) work: WorkRef,
    source: SourceId,
    target: String,
    payload_digest: String,
    outcome: EffectState,
}

impl Store {
    /// The trusted source holds the dispatch fence and has checked its current standing policy.
    /// This records execution, never a new permission or inferred owner answer.
    pub fn claim_policy_call(
        &self,
        work: &WorkRef,
        source: &SourceId,
        target: &str,
        call: &str,
        payload: &[u8],
    ) -> anyhow::Result<(OperationId, ClaimId)> {
        let id = OperationId::new(link_digest::sha256(&serde_json::to_vec(&(
            work, source, target, call,
        ))?))
        .map_err(anyhow::Error::msg)?;
        self.change(|s| {
            ensure!(
                claim_current(s, work),
                "work changed before policy call admission"
            );
            crate::validation::text(target)?;
            crate::validation::text(call)?;
            ensure!(
                !s.controls.runtime.effects.contains_key(&id),
                "this exact source call was already admitted; never replay it"
            );
            let claim = ClaimId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?;
            let binding = s
                .controls
                .runtime
                .work
                .get_mut(&work.attempt)
                .context("policy call needs registered host work")?;
            binding.state.in_flight_effects.insert(id.clone());
            s.controls.runtime.effects.insert(
                id.clone(),
                Record {
                    work: work.clone(),
                    source: source.clone(),
                    target: target.into(),
                    payload_digest: link_digest::sha256(payload),
                    outcome: EffectState::Applying {
                        claim: claim.clone(),
                    },
                },
            );
            Ok((id, claim))
        })
    }

    pub fn report_policy_call(
        &self,
        id: &OperationId,
        source: &SourceId,
        claim: &ClaimId,
        outcome: EffectOutcome,
    ) -> anyhow::Result<()> {
        self.change(|s| {
            let record = s
                .controls
                .runtime
                .effects
                .get_mut(id)
                .context("unknown policy call")?;
            ensure!(record.source == *source, "wrong policy call source");
            record.outcome = link_approvals::outcome(&record.outcome, claim, outcome)?;
            let attempt = record.work.attempt.clone();
            if !matches!(record.outcome, EffectState::Unknown { .. }) {
                s.controls
                    .runtime
                    .work
                    .get_mut(&attempt)
                    .context("unknown policy work")?
                    .state
                    .in_flight_effects
                    .remove(id);
                controls::reconciled(s, &attempt)?;
            }
            Ok(())
        })
    }
}

pub(super) fn lost(s: &mut State, work: &WorkRef) {
    for record in s
        .controls
        .runtime
        .effects
        .values_mut()
        .filter(|r| r.work.attempt == work.attempt)
    {
        if let EffectState::Applying { claim } = &record.outcome {
            record.outcome = EffectState::Unknown {
                claim: claim.clone(),
                reason: "source outcome unavailable after host recovery; never replay".into(),
            };
        }
    }
}
