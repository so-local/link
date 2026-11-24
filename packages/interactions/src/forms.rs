//! Delivery-local drafts and typed controls submit through the same atomic response boundary.
use crate::{
    delivery::*,
    store::{State, event, expire},
    *,
};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub revision: u64,
    pub owner: Actor,
    pub value: SetSubmission,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Edit {
        expected_revision: u64,
        value: SetSubmission,
    },
    Submit {
        draft_revision: u64,
    },
    Respond {
        value: ResponseValue,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub delivery: DeliveryId,
    pub question_revision: u64,
    pub anchor: Option<ContextAnchor>,
    pub transport_event: TransportEventId,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    pub envelope: DeliveryEnvelope,
    pub draft: Draft,
    pub resolution: Option<Resolution>,
    pub effect: approval::EffectState,
}

impl Store {
    /// The renderer reports a changed immutable question element, independently of surrounding streaming text.
    pub fn invalidate_anchor(
        &self,
        id: &DeliveryId,
        anchor: &ContextAnchor,
        now: u64,
    ) -> anyhow::Result<()> {
        self.change(|s| {
            let delivery = s.deliveries.get(id).context("unknown delivery")?;
            ensure!(
                delivery.anchor.as_ref() == Some(anchor),
                "wrong anchor revision"
            );
            let request = delivery.request.clone();
            crate::store::close(
                s.records.get_mut(&request).context("unknown question")?,
                "review context changed; request a new review",
            );
            event(s, Some(request), "context_changed", now)
        })
    }
    pub fn form(&self, ingress: &Ingress, id: &DeliveryId, now: u64) -> anyhow::Result<Form> {
        self.change(|s| {
            let delivery = authorized(s, ingress, id)?.clone();
            expire(s, &delivery.request, now)?;
            let record = s
                .records
                .get(&delivery.request)
                .context("unknown request")?;
            let draft = s.drafts.get(id).cloned().unwrap_or(Draft {
                revision: 0,
                owner: ingress.actor.clone(),
                value: SetSubmission::default(),
            });
            Ok(Form {
                envelope: DeliveryEnvelope {
                    question: record.snapshot.question.clone(),
                    delivery,
                    response_code: record.code.clone(),
                },
                draft,
                resolution: record.snapshot.resolution.clone(),
                effect: record.snapshot.effect.clone(),
            })
        })
    }

    pub fn form_control(
        &self,
        ingress: &Ingress,
        control: Control,
        now: u64,
    ) -> anyhow::Result<Option<SubmitDisposition>> {
        self.change(|s| {
            let delivery = authorized(s, ingress, &control.delivery)?.clone();
            ensure!(
                delivery.anchor == control.anchor,
                "stale or wrong inline anchor"
            );
            expire(s, &delivery.request, now)?;
            let record = s
                .records
                .get(&delivery.request)
                .context("unknown request")?;
            ensure!(
                record.snapshot.question.revision == control.question_revision,
                "stale question control"
            );
            if let Action::Edit {
                expected_revision,
                value,
            } = control.action
            {
                ensure!(record.snapshot.resolution.is_none(), "question is closed");
                let revision = s.drafts.get(&delivery.id).map_or(0, |d| d.revision);
                ensure!(
                    revision == expected_revision,
                    "draft changed on this delivery"
                );
                let mut shape = record.snapshot.question.clone();
                for item in &mut shape.questions {
                    match &mut item.answer_shape {
                        AnswerShape::Text { allow_empty, .. } => *allow_empty = true,
                        AnswerShape::Choice { min_selected, .. } => *min_selected = 0,
                        _ => {}
                    }
                }
                validation::response(&shape, &ResponseValue::Submit(value.clone()))?;
                s.drafts.insert(
                    delivery.id,
                    Draft {
                        revision: revision + 1,
                        owner: ingress.actor.clone(),
                        value,
                    },
                );
                return Ok(None);
            }
            let value = match control.action {
                Action::Submit { draft_revision } => {
                    let draft = s.drafts.get(&delivery.id).context("no draft to submit")?;
                    ensure!(
                        draft.revision == draft_revision && draft.owner == ingress.actor,
                        "stale or foreign draft"
                    );
                    ResponseValue::Submit(draft.value.clone())
                }
                Action::Respond { value } => value,
                Action::Edit { .. } => unreachable!(),
            };
            submit(
                s,
                ingress,
                ResponseCandidate {
                    request: delivery.request,
                    revision: control.question_revision,
                    delivery: delivery.id,
                    value,
                    transport_event: control.transport_event,
                },
                now,
            )
            .map(Some)
        })
    }
}

#[cfg(test)]
mod tests;

fn authorized<'a>(
    s: &'a State,
    ingress: &Ingress,
    id: &DeliveryId,
) -> anyhow::Result<&'a Delivery> {
    let delivery = s.deliveries.get(id).context("unknown delivery")?;
    let q = &s
        .records
        .get(&delivery.request)
        .context("unknown question")?
        .snapshot
        .question;
    ensure!(
        delivery.route == ingress.route
            && delivery.surface == ingress.surface
            && q.recipient == ingress.actor,
        "control is not from this delivery's owner"
    );
    Ok(delivery)
}

pub(crate) fn submit(
    s: &mut State,
    ingress: &Ingress,
    candidate: ResponseCandidate,
    now: u64,
) -> anyhow::Result<SubmitDisposition> {
    let delivery = authorized(s, ingress, &candidate.delivery)?;
    ensure!(
        delivery.request == candidate.request,
        "wrong question delivery"
    );
    let record = s
        .records
        .get(&candidate.request)
        .context("unknown request")?;
    ensure!(
        candidate.revision == record.snapshot.question.revision,
        "stale question revision"
    );
    if let Some((by, accepted)) = &record.accepted {
        if accepted.transport_event == candidate.transport_event {
            ensure!(
                *by == *ingress && *accepted == candidate,
                "transport event reused with different response"
            );
            return Ok(SubmitDisposition::Accepted);
        }
        return Ok(SubmitDisposition::AlreadyResolved);
    }
    expire(s, &candidate.request, now)?;
    let record = s
        .records
        .get_mut(&candidate.request)
        .context("unknown request")?;
    if record.snapshot.resolution.is_some() {
        return Ok(if now >= record.snapshot.question.expires_at {
            SubmitDisposition::Expired
        } else {
            SubmitDisposition::AlreadyResolved
        });
    }
    validation::response(&record.snapshot.question, &candidate.value)?;
    record.snapshot.effect = link_approvals::decision(&record.snapshot.question, &candidate.value);
    record.snapshot.resolution = Some(lifecycle::submitted(
        &record.snapshot.question,
        &candidate.value,
        ingress,
        now,
    ));
    record.snapshot.answered_on = Some(ingress.surface.clone());
    record.accepted = Some((ingress.clone(), candidate.clone()));
    event(s, Some(candidate.request), "resolved", now)?;
    Ok(SubmitDisposition::Accepted)
}
