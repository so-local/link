use crate::{approval::*, delivery::*, *};

/// Full text fallback is also a capable form. Never truncate an authorization.
pub fn text_capabilities() -> SurfaceCapabilities {
    crate::capabilities::surface("cli", crate::capabilities::Host::Linux)
        .expect("CLI capability declaration")
        .form()
}

pub fn domains() -> &'static [DomainDescriptor] {
    crate::validation::domains()
}

pub fn question(q: &QuestionSet) -> String {
    let label = domains()
        .iter()
        .find(|d| d.id == q.domain)
        .map(|d| d.label.as_str())
        .unwrap_or("Unsupported domain");
    let mut text = format!(
        "{} · {}\n{}",
        escaped(label),
        escaped(&q.requester.label),
        escaped(&q.title)
    );
    if let Some(work) = &q.requester.work {
        let context = match &work.kind {
            WorkKind::Conversation => format!("Conversation {}", work.session),
            WorkKind::Child { id } => format!("Child {id} · {}", work.session),
            WorkKind::Schedule { id, revision } => {
                format!("Schedule {} · revision {revision}", id.0)
            }
            WorkKind::Occurrence { occurrence } => format!(
                "Schedule {} · occurrence {} · revision {}",
                occurrence.schedule.0, occurrence.scheduled_at, occurrence.revision
            ),
            WorkKind::Process { id } => format!("Process {id}"),
        };
        text.push_str(&format!("\n{}", escaped(&context)));
    }
    if let Some(review) = &q.review {
        text.push_str(&format!(
            "\n\n{}\nTargets: {}\nScope: {}",
            escaped(&review.capability),
            review
                .targets
                .iter()
                .map(|s| escaped(s))
                .collect::<Vec<_>>()
                .join(", "),
            escaped(&review.lifetime)
        ));
        for consequence in &review.consequences {
            text.push_str(&format!("\n{}", escaped(consequence)));
        }
        if let Some(reason) = &review.requester_reason {
            text.push_str(&format!("\nRequester's reason: {}", escaped(reason)));
        }
        text.push_str(&format!(
            "\nTechnical details:\n{}",
            escaped(&review.exact_operation)
        ));
    }
    for (n, item) in q.questions.iter().enumerate() {
        text.push_str(&format!("\n\nQ{}: {}", n + 1, escaped(&item.prompt)));
        if let AnswerShape::Choice {
            options,
            allow_other,
            ..
        } = &item.answer_shape
        {
            for (n, choice) in options.iter().enumerate() {
                text.push_str(&format!(
                    "\n{}. {}{}",
                    n + 1,
                    escaped(&choice.label),
                    if choice.recommended {
                        " [Recommended]"
                    } else {
                        ""
                    }
                ));
                if let Some(description) = &choice.description {
                    text.push_str(&format!(" — {}", escaped(description)));
                }
            }
            if *allow_other {
                text.push_str("\nOther: supply your own answer.");
            }
            text.push_str("\nAn optional note may accompany or replace selections.");
        }
    }
    text.push_str(&format!("\n\nDeadline (UTC seconds): {}. ", q.expires_at));
    if matches!(q.effect, Effect::Authorize(_)) {
        text.push_str("No answer: permission remains denied.");
    } else if q
        .timeout
        .item_fallbacks
        .values()
        .any(|f| *f != Fallback::Expire)
    {
        text.push_str("No answer: return configured automatic fallbacks.");
        for item in &q.questions {
            match q.timeout.item_fallbacks.get(&item.id) {
                Some(Fallback::UseDefault) => text.push_str(&format!(
                    "\n{} default: {:?}",
                    escaped(item.id.as_str()),
                    item.default_answer
                )),
                Some(Fallback::UseRecommended) => text.push_str(&format!(
                    "\n{}: use the marked recommendations automatically.",
                    escaped(item.id.as_str())
                )),
                _ => {}
            }
        }
    } else {
        text.push_str("No answer: expire without an answer.");
    }
    text
}

pub fn delivery(envelope: &DeliveryEnvelope) -> String {
    let q = &envelope.question;
    let code = &envelope.response_code;
    let instructions = if q.questions.len() == 1 {
        match q.questions[0].answer_shape {
            AnswerShape::Confirmation => format!(
                "Reply {code} for yes, no {code} for no, or {code} followed by a clarification."
            ),
            AnswerShape::Text { .. } => format!("Reply {code} followed by your answer."),
            AnswerShape::Choice { .. } => format!(
                "Reply {code} followed by option numbers (1 or 1,3), other <answer>, or free response."
            ),
        }
    } else {
        format!(
            "Reply {code} followed by free response, or lines such as q1: choice 1, q2: text <answer>, q3: other <answer>, q3: note <note>. A line -- starts overall clarification. Omitted items stay skipped."
        )
    };
    format!(
        "{}\n\n{instructions}\nUse cancel {code} to dismiss. Submit once, including any qualifications.",
        question(q)
    )
}

/// Render invisible/control characters visibly while retaining the frozen bytes in storage.
fn escaped(text: &str) -> String {
    text.chars().flat_map(|c| {
        if (c.is_control() && c != '\n' && c != '\t') || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}') {
            c.escape_unicode().collect::<Vec<_>>()
        } else { vec![c] }
    }).collect()
}
