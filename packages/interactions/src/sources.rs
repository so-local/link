//! Trusted source constructors. Model drafts cannot supply authority or recipients.
use crate::{approval::*, constants::*, *};
use std::collections::BTreeMap;
mod effects;
pub use effects::{Application, ReviewedEffect};

pub fn owner() -> Actor {
    Actor::Owner
}

pub fn requester(source: &str, principal: &str, label: &str) -> anyhow::Result<Requester> {
    Ok(Requester {
        source: SourceId::new(source).map_err(anyhow::Error::msg)?,
        actor: principal.parse()?,
        label: label.into(),
        routing_agent: None,
        work: None,
        tool_call: None,
    })
}

pub fn timeout(wait: std::time::Duration) -> EffectiveTimeout {
    EffectiveTimeout {
        after_secs: wait.as_secs().clamp(1, MAX_WAIT_SECS),
        item_fallbacks: BTreeMap::new(),
        policy_revision: POLICY_REVISION.into(),
    }
}

pub fn confirmation(
    title: String,
    prompt: String,
    key: String,
    wait: std::time::Duration,
) -> QuestionDraft {
    QuestionDraft {
        title,
        questions: vec![QuestionItem {
            id: ItemId::new(CONFIRM_ITEM).expect("item ID"),
            prompt,
            answer_shape: AnswerShape::Confirmation,
            default_answer: None,
        }],
        caller_idempotency_key: key,
        requested_timeout: Some(timeout(wait)),
    }
}

pub fn confirmed(snapshot: &crate::Snapshot) -> Option<bool> {
    snapshot
        .resolution
        .as_ref()?
        .items
        .values()
        .find_map(|item| match item {
            ItemResult::Answered {
                answer: ItemAnswer::Confirmation { yes },
                provenance: Provenance::User { .. },
                ..
            } => Some(*yes),
            _ => None,
        })
}

pub fn operation(
    source: SourceId,
    payload: &[u8],
    review: &PermissionReview,
    scope: Scope,
    work: Option<WorkRef>,
) -> anyhow::Result<BoundOperation> {
    Ok(BoundOperation {
        source,
        schema: link_approvals::OPERATION_SCHEMA,
        id: OperationId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?,
        payload_digest: link_digest::sha256(payload),
        review_digest: link_approvals::review_digest(review)?,
        scope,
        work,
    })
}

pub struct QuestionCaller<'a> {
    pub revision: u64,
    pub token: &'a link_secretive::Secret,
    pub agent: &'a str,
    /// The service the agent acts as, its bare ID: the question's
    /// requester.
    pub service: &'a str,
    pub label: &'a str,
    pub session: &'a str,
    pub attempt: &'a str,
    pub call: &'a str,
}

/// The source authenticates the continuation before accepting any model-authored question.
pub fn create_question(
    store: &Store,
    caller: QuestionCaller<'_>,
    proposal: QuestionProposal,
    opinions: &InteractionOpinions,
    wait: std::time::Duration,
    now: u64,
) -> anyhow::Result<RequestId> {
    let work = store.authenticate_work(
        caller.token.expose(),
        caller.agent,
        caller.session,
        caller.attempt,
    )?;
    anyhow::ensure!(
        work.control_revision == caller.revision,
        "work changed before this question call"
    );
    let mut requester = requester(
        "agent",
        &format!("service:{}", caller.service),
        caller.label,
    )?;
    requester.routing_agent = Some(caller.label.into());
    requester.tool_call = Some(caller.call.into());
    requester.work = Some(work);
    let domain = DomainId::new("question").map_err(anyhow::Error::msg)?;
    let timeout = effective_timeout(
        opinions,
        &domain,
        &proposal.questions,
        proposal.requested_timeout.as_ref(),
        wait,
    )?;
    store.create(
        requester,
        owner(),
        QuestionDraft {
            title: proposal.title,
            questions: proposal.questions,
            caller_idempotency_key: caller.call.into(),
            requested_timeout: Some(timeout),
        },
        now,
    )
}

/// Follow registered parentage, then freeze the owner's effective opinion.
pub fn effective_timeout(
    opinions: &InteractionOpinions,
    domain: &DomainId,
    items: &[QuestionItem],
    requested: Option<&EffectiveTimeout>,
    fallback_wait: std::time::Duration,
) -> anyhow::Result<EffectiveTimeout> {
    let descriptors = crate::render::domains();
    let mut chain = Vec::new();
    let mut current = Some(domain);
    while let Some(id) = current {
        let descriptor = descriptors
            .iter()
            .find(|d| d.id == *id)
            .ok_or_else(|| anyhow::anyhow!("unregistered interaction domain"))?;
        chain.push(descriptor);
        current = descriptor.parent.as_ref();
    }
    let mut opinion = DomainOpinion::default();
    for descriptor in chain.iter().rev() {
        if let Some(next) = opinions.domains.get(&descriptor.id) {
            if next.wait_secs.is_some() {
                opinion.wait_secs = next.wait_secs;
            }
            if next.confirmation.is_some() {
                opinion.confirmation = next.confirmation;
            }
            if next.text.is_some() {
                opinion.text = next.text;
            }
            if next.choice.is_some() {
                opinion.choice = next.choice;
            }
            if next.allow_request_overrides.is_some() {
                opinion.allow_request_overrides = next.allow_request_overrides;
            }
        }
    }
    let mut timeout = timeout(fallback_wait);
    if let Some(wait) = opinion.wait_secs {
        timeout.after_secs = wait;
    }
    for item in items {
        let fallback = match item.answer_shape {
            AnswerShape::Confirmation => &opinion.confirmation,
            AnswerShape::Text { .. } => &opinion.text,
            AnswerShape::Choice { .. } => &opinion.choice,
        };
        if let Some(fallback) = fallback {
            timeout.item_fallbacks.insert(item.id.clone(), *fallback);
        }
    }
    if opinion.allow_request_overrides == Some(true)
        && let Some(requested) = requested
    {
        timeout.after_secs = requested.after_secs;
        timeout.item_fallbacks = requested.item_fallbacks.clone();
    }
    anyhow::ensure!(
        timeout.after_secs > 0 && timeout.after_secs <= MAX_WAIT_SECS,
        "invalid interaction wait"
    );
    if chain.iter().any(|d| d.authorization) {
        anyhow::ensure!(
            timeout
                .item_fallbacks
                .values()
                .all(|f| *f == Fallback::Expire),
            "permissions cannot have automatic answers"
        );
    }
    timeout.policy_revision =
        link_digest::sha256(&serde_json::to_vec(&(opinions, domain, &timeout))?);
    Ok(timeout)
}
