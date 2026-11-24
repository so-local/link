//! Authorization rules. Only a serialized engine commit can consume a claim.

use anyhow::ensure;
use link_interactions_contract::{approval::*, *};

pub fn review_digest(review: &PermissionReview) -> anyhow::Result<String> {
    Ok(link_digest::sha256(&serde_json::to_vec(review)?))
}

pub fn validate(question: &QuestionSet) -> anyhow::Result<()> {
    let Effect::Authorize(operation) = &question.effect else {
        ensure!(
            question.review.is_none(),
            "information questions have no permission review"
        );
        return Ok(());
    };
    ensure!(
        operation.source == question.requester.source,
        "wrong operation source"
    );
    ensure!(
        operation.work == question.requester.work,
        "wrong operation continuation"
    );
    ensure!(
        operation.schema == crate::constants::OPERATION_SCHEMA,
        "unsupported operation schema"
    );
    ensure!(
        operation.payload_digest.len() == crate::constants::DIGEST_LENGTH
            && operation
                .payload_digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "invalid payload digest"
    );
    ensure!(
        question.questions.len() == 1
            && matches!(
                question.questions[0].answer_shape,
                AnswerShape::Confirmation
            ),
        "permissions require one confirmation"
    );
    ensure!(
        question
            .timeout
            .item_fallbacks
            .values()
            .all(|f| *f == Fallback::Expire),
        "permissions cannot have automatic answers"
    );
    ensure!(
        question
            .questions
            .iter()
            .all(|q| q.default_answer.is_none()),
        "permissions cannot have defaults"
    );
    let review = question
        .review
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing full review"))?;
    ensure!(
        !review.capability.trim().is_empty()
            && !review.lifetime.trim().is_empty()
            && !review.exact_operation.trim().is_empty()
            && !review.targets.is_empty()
            && review.targets.iter().all(|t| !t.trim().is_empty()),
        "incomplete permission review"
    );
    ensure!(
        review_digest(review)? == operation.review_digest,
        "changed permission review"
    );
    Ok(())
}

pub fn decision(question: &QuestionSet, response: &ResponseValue) -> EffectState {
    if matches!(question.effect, Effect::ReplyOnly) {
        return EffectState::None;
    }
    let ResponseValue::Submit(submission) = response else {
        return EffectState::Denied;
    };
    let yes = submission
        .answers
        .values()
        .any(|a| matches!(a, ItemAnswer::Confirmation { yes: true }));
    if !yes {
        return EffectState::Denied;
    }
    if submission
        .comment
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return EffectState::RequiresClarification;
    }
    EffectState::Authorized
}

/// The source must hold its own execution fence until actual handoff.
pub fn eligible(
    question: &QuestionSet,
    state: &EffectState,
    checks: &SourceChecks,
    now: u64,
) -> anyhow::Result<()> {
    ensure!(
        *state == EffectState::Authorized,
        "effect is not authorized and unclaimed"
    );
    ensure!(now < question.expires_at, "permission expired before claim");
    let Effect::Authorize(operation) = &question.effect else {
        anyhow::bail!("reply-only question");
    };
    ensure!(
        operation.as_ref() == &checks.operation,
        "operation or execution binding changed"
    );
    ensure!(
        checks.authority_current && checks.generation_current && checks.control_fence_clear,
        "source authority, generation or control fence refused the claim"
    );
    validate(question)
}

pub fn outcome(
    state: &EffectState,
    claim: &ClaimId,
    outcome: EffectOutcome,
) -> anyhow::Result<EffectState> {
    let next = match outcome {
        EffectOutcome::Applied { detail } => EffectState::Applied {
            claim: claim.clone(),
            detail,
        },
        EffectOutcome::Failed { reason } => EffectState::Failed {
            claim: claim.clone(),
            reason,
        },
        EffectOutcome::Unknown { reason } => EffectState::Unknown {
            claim: claim.clone(),
            reason,
        },
    };
    if matches!(state, EffectState::Unknown {claim:current, ..} if current == claim)
        && matches!(next, EffectState::Unknown { .. })
    {
        return Ok(state.clone());
    }
    ensure!(
        matches!(state, EffectState::Applying { claim: current } if current == claim)
            || (matches!(state, EffectState::Unknown {claim:current, ..} if current == claim)
                && matches!(
                    next,
                    EffectState::Applied { .. } | EffectState::Failed { .. }
                ))
            || *state == next,
        "effect claim or recorded outcome does not match"
    );
    Ok(next)
}

mod constants;
/// The operation schema a bound operation names, which [`validate`]
/// reads.
pub use constants::OPERATION_SCHEMA;

#[cfg(test)]
link_test_kit::program!(unit);
