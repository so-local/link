//! Authenticated owner responses, classified before ordinary turn admission.
use link_interactions::{sources, *};

pub fn urgent(text: Option<&str>, reply: bool) -> bool {
    reply || text.is_some_and(|t| reply::code_reply(t).is_some() || owner::is_command(t))
}

/// A reply is meaningful only after transport ownership and the stored delivery both match.
pub fn caught_reply(
    agent: &str,
    thread: &str,
    message: &str,
    text: &str,
    event: &str,
) -> Option<String> {
    let store = Store::machine().ok()?;
    caught_reply_at(&store, agent, thread, message, text, event)
}

pub fn caught_reply_at(
    store: &Store,
    agent: &str,
    thread: &str,
    message: &str,
    text: &str,
    event: &str,
) -> Option<String> {
    let surface = thread.split(':').next()?;
    let delivery = store.reply_delivery(thread, surface, message).ok()??;
    let snapshot = store.snapshot(&delivery.request, now()).ok()?;
    if !asked_of(store, &snapshot.question.requester, agent) {
        return None;
    }
    let result = (|| -> anyhow::Result<_> {
        let value = reply::parse(
            &snapshot.question,
            text,
            false,
            false,
            text.trim() == "/dismiss",
        )?;
        store.submit(
            &Ingress {
                actor: sources::owner(),
                route: thread.into(),
                surface: surface.into(),
                input_kind: InputKind::Typed,
            },
            ResponseCandidate {
                request: delivery.request.clone(),
                revision: snapshot.question.revision,
                delivery: delivery.id,
                value,
                transport_event: TransportEventId::new(format!("reply:{surface}:{event}"))
                    .map_err(anyhow::Error::msg)?,
            },
            now(),
        )
    })();
    Some(match result {
        Ok(SubmitDisposition::Accepted) => "Response accepted; the requester receives its exact result. Permission effects still require source checks.".into(),
        Ok(SubmitDisposition::AlreadyResolved) => "This question is already answered.".into(),
        Ok(SubmitDisposition::Expired) => "This question expired.".into(),
        Err(err) => format!("Response not accepted: {err:#}"),
    })
}

pub fn asked_of(store: &Store, requester: &Requester, agent: &str) -> bool {
    match &requester.work {
        Some(work) => store.is_agent_route(&work.agent, agent).unwrap_or(false),
        None => requester.routing_agent.as_deref() == Some(agent),
    }
}

/// Call only after transport authentication established the owner.
pub fn caught(agent: &str, thread: &str, text: &str) -> Option<String> {
    if owner::is_command(text) {
        if Store::machine()
            .is_ok_and(|store| owner::stops_nothing_retained(&store, agent, thread, text))
        {
            return None;
        }
        return Some(
            (|| {
                owner::command(
                    &Store::machine()?,
                    &sources::owner(),
                    agent,
                    thread,
                    text,
                    TransportEventId::new(link_files::random_token()?)
                        .map_err(anyhow::Error::msg)?,
                )
            })()
            .unwrap_or_else(|e| format!("Work control refused: {e:#}")),
        );
    }
    let parsed = reply::code_reply(text)?;
    caught_in(&Store::machine().ok()?, agent, thread, parsed)
}

pub fn caught_at(store: &Store, agent: &str, thread: &str, text: &str) -> Option<String> {
    caught_in(store, agent, thread, reply::code_reply(text)?)
}

fn caught_in(
    store: &Store,
    agent: &str,
    thread: &str,
    parsed: reply::CodeReply<'_>,
) -> Option<String> {
    let id = store.find_code(&parsed.code).ok()??;
    let snapshot = store.snapshot(&id, now()).ok()?;
    if !asked_of(store, &snapshot.question.requester, agent) {
        return None;
    }
    let surface = thread.split(':').next().unwrap_or("chat");
    let delivery = store
        .deliveries(&id)
        .ok()?
        .into_iter()
        .find(|d| d.route == thread && d.surface == surface)?;
    let result = (|| -> anyhow::Result<SubmitDisposition> {
        let value = reply::parse(
            &snapshot.question,
            parsed.body,
            parsed.body.is_empty() && !parsed.deny && !parsed.dismiss,
            parsed.deny,
            parsed.dismiss,
        )?;
        store.submit(
            &Ingress {
                actor: sources::owner(),
                route: thread.into(),
                surface: surface.into(),
                input_kind: InputKind::Typed,
            },
            ResponseCandidate {
                request: id.clone(),
                revision: snapshot.question.revision,
                delivery: delivery.id,
                value,
                transport_event: TransportEventId::new(link_files::random_token()?)
                    .map_err(anyhow::Error::msg)?,
            },
            now(),
        )
    })();
    Some(match result {
        Ok(SubmitDisposition::Accepted) => match store.snapshot(&id, now()).ok().map(|s| s.effect) {
            Some(approval::EffectState::Authorized) => "Allowed; awaiting source checks and application.".into(),
            Some(approval::EffectState::RequiresClarification) => "Your qualification was retained; this operation needs clarification before it can run.".into(),
            Some(approval::EffectState::Denied) => "Refused.".into(),
            _ => format!("Answer sent to {}.", link_format::clean(&snapshot.question.requester.label)),
        },
        Ok(SubmitDisposition::AlreadyResolved) => format!("Already answered{}.", snapshot.answered_on.map(|s| format!(" on {s}")).unwrap_or_default()),
        Ok(SubmitDisposition::Expired) => "No longer waiting: the question expired.".into(),
        Err(err) => format!("Response not accepted: {err:#}"),
    })
}

pub(super) fn turn(agent: &str, thread: &str) -> Option<presence::Presence> {
    Store::machine()
        .ok()?
        .presence(&format!("turn:{agent}"), Some(thread))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_agents_question_follows_its_identity_and_rejects_a_reused_display_name() {
        let root = link_test_kit::TempDir::new("question-agent-route");
        let store = Store::at(&*root);
        let binding = store
            .register_work(link_interactions::runtime::WorkAdmission {
                owner_route: Some("telegram:42".into()),
                kind: WorkKind::Conversation,
                owner: sources::owner(),
                agent: "aaaaaaaaaaa1".into(),
                routing_agent: "main".into(),
                session: "telegram:42".into(),
                attempt: "one".into(),
                parent_attempt: None,
                checkpoint: Some("prompt".into()),
            })
            .unwrap();
        let id = sources::create_question(&store, sources::QuestionCaller {
            token:&binding.token, revision:0, agent:"aaaaaaaaaaa1", service:"s1s2s3s4s5s6", label:"main", session:"telegram:42", attempt:"one", call:"call-one"
        }, serde_json::from_value(serde_json::json!({"title":"Report", "questions":[{"id":"focus", "prompt":"What should it emphasize?",
            "answer_shape":{"kind":"text", "max_bytes":128, "allow_empty":false}}]})).unwrap(),
            &InteractionOpinions::default(), std::time::Duration::from_secs(30), now()).unwrap();
        let envelope = store
            .plan_delivery(
                &id,
                "telegram:42",
                "telegram",
                &render::text_capabilities(),
                None,
                now(),
            )
            .unwrap();
        store
            .delivered(&envelope.delivery.id, Some("platform-one".into()))
            .unwrap();
        store
            .register_agent_route("aaaaaaaaaaa1", "renamed")
            .unwrap();
        store.register_agent_route("bbbbbbbbbbb2", "main").unwrap();
        assert!(
            caught_reply_at(
                &store,
                "main",
                "telegram:42",
                "platform-one",
                "The exact cost",
                "wrong-agent"
            )
            .is_none()
        );
        assert!(store.snapshot(&id, now()).unwrap().resolution.is_none());
        assert!(
            caught_reply_at(
                &store,
                "renamed",
                "telegram:42",
                "platform-one",
                "The exact cost",
                "right-agent"
            )
            .is_some()
        );
        let result = store.snapshot(&id, now()).unwrap().resolution.unwrap();
        assert_eq!(result.requester.work.unwrap().agent, "aaaaaaaaaaa1");
        assert!(
            serde_json::to_string(&result.items)
                .unwrap()
                .contains("The exact cost")
        );
    }
    #[test]
    fn explicit_codes_require_the_original_agents_owner_route() {
        let dir = link_test_kit::TempDir::new("interaction-chat");
        let store = Store::at(&*dir);
        let mut requester = sources::requester("agent", "service:m1a2i3n4s5v6", "main").unwrap();
        requester.routing_agent = Some("main".into());
        let id = store
            .create(
                requester,
                sources::owner(),
                sources::confirmation(
                    "Format".into(),
                    "Use brief format?".into(),
                    "one".into(),
                    std::time::Duration::from_secs(60),
                ),
                now(),
            )
            .unwrap();
        let e = store
            .plan_delivery(
                &id,
                "telegram:42",
                "telegram",
                &render::text_capabilities(),
                None,
                now(),
            )
            .unwrap();
        let code = &e.response_code;
        let catch = |agent, thread, text: &str| {
            caught_in(&store, agent, thread, reply::code_reply(text).unwrap())
        };
        assert!(catch("other", "telegram:42", code).is_none());
        assert!(catch("main", "telegram:other", code).is_none());
        assert!(
            caught_in(
                &store,
                "main",
                "telegram:42",
                reply::code_reply(&code.to_uppercase()).unwrap()
            )
            .unwrap()
            .starts_with("Answer sent")
        );
        assert!(
            catch("main", "telegram:42", &format!("no {code}"))
                .unwrap()
                .starts_with("Already answered")
        );
        assert!(sources::confirmed(&store.snapshot(&id, now()).unwrap()).unwrap());
        assert!(reply::code_reply("yes").is_none());
    }
}
