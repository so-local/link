//! Source-owned handles consume a bound operation once and preserve uncertain outcomes on drop.
use super::*;

pub struct ReviewedEffect {
    store: Store,
    request: RequestId,
    operation: BoundOperation,
    claimed: bool,
}

impl Store {
    pub fn reviewed_effect(
        &self,
        request: &RequestId,
        source: &SourceId,
    ) -> anyhow::Result<ReviewedEffect> {
        let snapshot = self.snapshot(request, crate::now())?;
        anyhow::ensure!(
            snapshot.question.requester.source == *source,
            "effect belongs to another source"
        );
        let Effect::Authorize(operation) = snapshot.question.effect else {
            anyhow::bail!("question authorizes no effect")
        };
        anyhow::ensure!(
            snapshot.effect == EffectState::Authorized,
            "effect was not authorized"
        );
        Ok(ReviewedEffect {
            store: self.clone(),
            request: request.clone(),
            operation: *operation,
            claimed: false,
        })
    }
}

impl ReviewedEffect {
    pub fn dispatch_fence(&self) -> anyhow::Result<std::fs::File> {
        self.store.dispatch_fence()
    }
    pub fn operation(&self) -> &BoundOperation {
        &self.operation
    }

    /// Caller holds its source and work dispatch fences, and has freshly checked source authority.
    pub fn claim(mut self, payload: &[u8], authority_current: bool) -> anyhow::Result<Application> {
        anyhow::ensure!(
            self.operation.payload_digest == link_digest::sha256(payload),
            "reviewed payload changed"
        );
        let claim = self.store.claim(
            &self.request,
            &SourceChecks {
                operation: self.operation.clone(),
                authority_current,
                generation_current: true,
                control_fence_clear: true,
            },
            crate::now(),
        )?;
        self.claimed = true;
        Ok(Application {
            store: self.store.clone(),
            target: Target::Question(self.request.clone()),
            source: self.operation.source.clone(),
            claim,
            reported: false,
        })
    }
}

impl Drop for ReviewedEffect {
    fn drop(&mut self) {
        if !self.claimed
            && let Ok(snapshot) = self.store.snapshot(&self.request, crate::now())
        {
            let _ = self.store.cancel(
                &self.request,
                &snapshot.question.requester,
                "source abandoned the reviewed operation",
                crate::now(),
            );
        }
    }
}

pub struct Application {
    store: Store,
    target: Target,
    source: SourceId,
    claim: ClaimId,
    reported: bool,
}
enum Target {
    Question(RequestId),
    Policy(OperationId),
}
impl Store {
    pub fn policy_call(
        &self,
        work: &WorkRef,
        source: &SourceId,
        target: &str,
        call: &str,
        payload: &[u8],
    ) -> anyhow::Result<Application> {
        let (id, claim) = self.claim_policy_call(work, source, target, call, payload)?;
        Ok(Application {
            store: self.clone(),
            target: Target::Policy(id),
            source: source.clone(),
            claim,
            reported: false,
        })
    }
}
impl Application {
    pub fn report(mut self, outcome: EffectOutcome) -> anyhow::Result<()> {
        self.record(outcome)?;
        self.reported = true;
        Ok(())
    }
    fn record(&self, outcome: EffectOutcome) -> anyhow::Result<()> {
        match &self.target {
            Target::Question(request) => {
                self.store
                    .report(request, &self.source, &self.claim, outcome, crate::now())
            }
            Target::Policy(id) => {
                self.store
                    .report_policy_call(id, &self.source, &self.claim, outcome)
            }
        }
    }
}
impl Drop for Application {
    fn drop(&mut self) {
        if !self.reported {
            let _ = self.record(EffectOutcome::Unknown {
                reason: "source ended before reporting the operation".into(),
            });
        }
    }
}
