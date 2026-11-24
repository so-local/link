use crate::*;
use std::collections::BTreeMap;

fn result(
    q: &QuestionSet,
    item: &QuestionItem,
    answer: ItemAnswer,
    provenance: Provenance,
    ingress: Option<&Ingress>,
) -> ItemResult {
    let actor = ingress.map(|i| i.actor.clone());
    let mut selections = Vec::new();
    let mut text = Vec::new();
    if let (
        AnswerShape::Choice { options, .. },
        ItemAnswer::Choice {
            selected,
            other_text,
            comment,
        },
    ) = (&item.answer_shape, &answer)
    {
        for choice in options.iter().filter(|o| selected.contains(&o.id)) {
            selections.push(SelectionEvidence {
                choice: choice.clone(),
                supplied_by: q.requester.actor.clone(),
                selected_by: actor.clone(),
            });
        }
        for value in [other_text, comment].into_iter().flatten() {
            if let Some(ingress) = ingress
                && !value.trim().is_empty()
            {
                text.push(TextEvidence {
                    text: value.clone(),
                    supplied_by: ingress.actor.clone(),
                    input_kind: ingress.input_kind.clone(),
                });
            }
        }
    }
    if let (ItemAnswer::Text { text: value }, Some(ingress)) = (&answer, ingress) {
        text.push(TextEvidence {
            text: value.clone(),
            supplied_by: ingress.actor.clone(),
            input_kind: ingress.input_kind.clone(),
        });
    }
    ItemResult::Answered {
        answer,
        provenance,
        selections,
        text,
    }
}

pub fn submitted(
    q: &QuestionSet,
    value: &ResponseValue,
    ingress: &Ingress,
    now: u64,
) -> Resolution {
    let mut resolution = base(q, Trigger::UserSubmission);
    match value {
        ResponseValue::Dismiss { comment } => {
            resolution.trigger = Trigger::Closed;
            resolution.closed_reason = Some("dismissed".into());
            resolution.comment = evidence(comment, ingress);
        }
        ResponseValue::Submit(submission) => {
            resolution.comment = evidence(&submission.comment, ingress);
            for item in &q.questions {
                if let Some(answer) = submission.answers.get(&item.id) {
                    resolution.items.insert(
                        item.id.clone(),
                        result(
                            q,
                            item,
                            answer.clone(),
                            Provenance::User {
                                actor: ingress.actor.clone(),
                                accepted_at: now,
                            },
                            Some(ingress),
                        ),
                    );
                }
            }
        }
    }
    resolution
}

fn evidence(value: &Option<String>, ingress: &Ingress) -> Option<TextEvidence> {
    value
        .as_ref()
        .filter(|v| !v.trim().is_empty())
        .map(|value| TextEvidence {
            text: value.clone(),
            supplied_by: ingress.actor.clone(),
            input_kind: ingress.input_kind.clone(),
        })
}

pub fn timeout(q: &QuestionSet) -> Resolution {
    let mut resolution = base(q, Trigger::Timeout);
    if matches!(q.effect, approval::Effect::ReplyOnly) {
        for item in &q.questions {
            let fallback = q.timeout.item_fallbacks.get(&item.id);
            let (answer, provenance) = match fallback {
                Some(Fallback::UseDefault) => (
                    item.default_answer.clone(),
                    Provenance::TimeoutDefault {
                        policy_revision: q.timeout.policy_revision.clone(),
                    },
                ),
                Some(Fallback::UseRecommended) => {
                    let answer = match &item.answer_shape {
                        AnswerShape::Choice { options, .. } => {
                            let selected: Vec<_> = options
                                .iter()
                                .filter(|o| o.recommended)
                                .map(|o| o.id.clone())
                                .collect();
                            (!selected.is_empty()).then_some(ItemAnswer::Choice {
                                selected,
                                other_text: None,
                                comment: None,
                            })
                        }
                        _ => None,
                    };
                    (
                        answer,
                        Provenance::TimeoutRecommended {
                            policy_revision: q.timeout.policy_revision.clone(),
                        },
                    )
                }
                _ => (
                    None,
                    Provenance::TimeoutDefault {
                        policy_revision: q.timeout.policy_revision.clone(),
                    },
                ),
            };
            if let Some(answer) = answer {
                resolution
                    .items
                    .insert(item.id.clone(), result(q, item, answer, provenance, None));
            } else {
                resolution.items.insert(
                    item.id.clone(),
                    ItemResult::Skipped {
                        reason: "timeout: fallback unavailable".into(),
                    },
                );
            }
        }
    }
    if resolution
        .items
        .values()
        .all(|r| matches!(r, ItemResult::Skipped { .. }))
    {
        resolution.closed_reason = Some("expired".into());
    }
    resolution
}

pub fn closed(q: &QuestionSet, reason: &str) -> Resolution {
    let mut resolution = base(q, Trigger::Closed);
    resolution.closed_reason = Some(reason.into());
    resolution
}

fn base(q: &QuestionSet, trigger: Trigger) -> Resolution {
    Resolution {
        request: q.id.clone(),
        requester: q.requester.clone(),
        domain: q.domain.clone(),
        trigger,
        items: q
            .questions
            .iter()
            .map(|item| {
                (
                    item.id.clone(),
                    ItemResult::Skipped {
                        reason: "not answered".into(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
        comment: None,
        closed_reason: None,
    }
}
