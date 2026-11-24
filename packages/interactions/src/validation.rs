use crate::{approval::*, constants::*, delivery::*, *};
use anyhow::{Context, ensure};
use std::collections::BTreeSet;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Domains {
    domain: Vec<DomainDescriptor>,
}

pub fn domains() -> &'static [DomainDescriptor] {
    static DOMAINS_PARSED: std::sync::OnceLock<Vec<DomainDescriptor>> = std::sync::OnceLock::new();
    DOMAINS_PARSED.get_or_init(|| {
        toml::from_str::<Domains>(DOMAINS)
            .expect("domain declarations")
            .domain
    })
}

pub fn question(q: &QuestionSet) -> anyhow::Result<()> {
    PROTOCOL.read("the question", q.version)?;
    ensure!(q.revision == 1, "unsupported question revision");
    let domain = domains()
        .iter()
        .find(|d| d.id == q.domain)
        .context("unregistered domain")?;
    ensure!(
        domain.sources.contains(&q.requester.source),
        "source cannot ask in this domain"
    );
    ensure!(
        domain.authorization == matches!(q.effect, Effect::Authorize(_)),
        "unsupported domain effect"
    );
    text(&q.title)?;
    ensure!(
        !q.questions.is_empty() && q.questions.len() <= MAX_ITEMS,
        "invalid question count"
    );
    ensure!(
        q.timeout.after_secs > 0 && q.timeout.after_secs <= MAX_WAIT_SECS,
        "invalid deadline"
    );
    ensure!(
        q.expires_at
            == q.created_at
                .checked_add(q.timeout.after_secs)
                .context("deadline overflow")?,
        "deadline does not match frozen timeout"
    );
    let mut ids = BTreeSet::new();
    for item in &q.questions {
        ensure!(
            crate::capabilities::accepts(&q.requester.source, &item.answer_shape),
            "source does not support this answer shape"
        );
        ensure!(ids.insert(&item.id), "duplicate question ID");
        text(&item.prompt)?;
        match &item.answer_shape {
            AnswerShape::Confirmation => {}
            AnswerShape::Text { max_bytes, .. } => ensure!(
                *max_bytes > 0 && *max_bytes <= MAX_TEXT_BYTES,
                "invalid text limit"
            ),
            AnswerShape::Choice {
                options,
                min_selected,
                max_selected,
                allow_other,
            } => {
                ensure!(
                    !options.is_empty() && options.len() <= MAX_OPTIONS,
                    "invalid option count"
                );
                ensure!(
                    *max_selected > 0
                        && min_selected <= max_selected
                        && *max_selected <= options.len() + usize::from(*allow_other),
                    "unsatisfiable choice bounds"
                );
                let mut choices = BTreeSet::new();
                for option in options {
                    ensure!(choices.insert(&option.id), "duplicate choice ID");
                    text(&option.label)?;
                    if let Some(description) = &option.description {
                        text(description)?;
                    }
                }
                let recommended = options.iter().filter(|o| o.recommended).count();
                ensure!(
                    recommended == 0
                        || (recommended >= *min_selected && recommended <= *max_selected),
                    "recommendations violate cardinality"
                );
            }
        }
        if let Some(default) = &item.default_answer {
            answer(item, default)?;
        }
        if let Some(Fallback::UseRecommended) = q.timeout.item_fallbacks.get(&item.id) {
            ensure!(
                matches!(item.answer_shape, AnswerShape::Choice { .. }),
                "recommendation requires choice"
            );
        }
    }
    ensure!(
        q.timeout.item_fallbacks.keys().all(|id| ids.contains(id)),
        "fallback for unknown item"
    );
    link_approvals::validate(q)
}

pub fn text(value: &str) -> anyhow::Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= MAX_TEXT_BYTES,
        "empty or oversized text"
    );
    Ok(())
}

pub fn optional(value: &Option<String>) -> anyhow::Result<bool> {
    if let Some(value) = value {
        ensure!(value.len() <= MAX_TEXT_BYTES, "oversized commentary");
    }
    Ok(value.as_ref().is_some_and(|v| !v.trim().is_empty()))
}

pub fn answer(item: &QuestionItem, value: &ItemAnswer) -> anyhow::Result<()> {
    match (&item.answer_shape, value) {
        (AnswerShape::Confirmation, ItemAnswer::Confirmation { .. }) => {}
        (
            AnswerShape::Text {
                max_bytes,
                allow_empty,
            },
            ItemAnswer::Text { text },
        ) => {
            ensure!(
                text.len() <= *max_bytes && (*allow_empty || !text.trim().is_empty()),
                "invalid text answer"
            );
        }
        (
            AnswerShape::Choice {
                options,
                min_selected,
                max_selected,
                allow_other,
            },
            ItemAnswer::Choice {
                selected,
                other_text,
                comment,
            },
        ) => {
            let mut seen = BTreeSet::new();
            for id in selected {
                ensure!(
                    seen.insert(id) && options.iter().any(|o| o.id == *id),
                    "unknown or duplicate choice"
                );
            }
            if let Some(other) = other_text {
                ensure!(*allow_other, "Other is unsupported");
                text(other)?;
            }
            let count = selected.len() + usize::from(other_text.is_some());
            let comment_only = count == 0 && optional(comment)?;
            ensure!(
                comment_only || (count >= *min_selected && count <= *max_selected && count > 0),
                "invalid selection count"
            );
            optional(comment)?;
        }
        _ => anyhow::bail!("wrong answer shape"),
    }
    Ok(())
}

pub fn response(q: &QuestionSet, response: &ResponseValue) -> anyhow::Result<()> {
    match response {
        ResponseValue::Dismiss { comment } => {
            optional(comment)?;
        }
        ResponseValue::Submit(submission) => {
            ensure!(
                !submission.answers.is_empty() || optional(&submission.comment)?,
                "empty submission"
            );
            optional(&submission.comment)?;
            for (id, value) in &submission.answers {
                answer(
                    q.questions
                        .iter()
                        .find(|item| item.id == *id)
                        .context("unknown item ID")?,
                    value,
                )?;
            }
        }
    }
    Ok(())
}

pub fn delivery(q: &QuestionSet, caps: &SurfaceCapabilities) -> anyhow::Result<()> {
    ensure!(
        caps.free_response && (q.questions.len() == 1 || caps.sets),
        "surface lacks question forms"
    );
    for item in &q.questions {
        ensure!(
            match item.answer_shape {
                AnswerShape::Confirmation => caps.confirmation,
                AnswerShape::Text { .. } => caps.text,
                AnswerShape::Choice { .. } => caps.choice,
            },
            "unsupported surface answer shape"
        );
    }
    ensure!(
        serde_json::to_vec(q)?.len() <= caps.max_review_bytes,
        "surface cannot show full review"
    );
    Ok(())
}
