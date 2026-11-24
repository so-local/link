//! Observe each selected model attempt at its final canonical boundary.

use super::*;
use link_harness_contract::harness::Model;
use link_harness_wire::evidence::{Captured, Evidence, Phase, Record, request::*};

pub(super) struct Attempt<'a> {
    pub first: &'a Model,
    pub second: Option<&'a crate::fallback::Second>,
    pub prompt: &'a model::Prompt<'a>,
    pub sources: &'a [Contribution],
    pub work: Option<link_interactions::WorkRef>,
    pub phase: Phase,
    pub pieces: &'a mpsc::Sender<String>,
    pub separate: bool,
}

impl Harness {
    pub(super) async fn ask_model(
        &self,
        turn: &Turn<'_>,
        attempt: Attempt<'_>,
        using: &mut Option<crate::fallback::Why>,
        events: &mut Vec<Event>,
    ) -> anyhow::Result<Event> {
        let events = std::sync::Mutex::new(events);
        let observer = |model: &Model, prompt: &model::Prompt<'_>| {
            let mut held = events
                .lock()
                .map_err(|_| anyhow::anyhow!("model request recorder lock poisoned"))?;
            let projected = self
                .requested
                .digests()
                .with(turn.agent, turn.thread, |digests| {
                    projected(attempt.sources, prompt.messages, digests)
                });
            // What changed since the request before, when this process
            // recorded it: a conversation's evidence then grows with its
            // messages, not with its window at every request.
            let (projection, chain) = self.requested.projection(
                turn.agent,
                turn.thread,
                &held,
                &projected,
                crate::tools::whole_every(),
            );
            let captured = crate::tools::request_projection(projection);
            let request = self
                .requested
                .digests()
                .with(turn.agent, turn.thread, |digests| {
                    described(turn, &attempt, model, prompt, captured.clone(), digests)
                });
            let mut event = Event {
                role: event::EVIDENCE.into(),
                at: event::now(),
                evidence: Some(Evidence::new(Record::ModelRequested { request }).into()),
                ..Event::default()
            };
            off_runtime(|| self.append_next(turn.agent, turn.thread, &mut event, &mut held))?;
            self.requested.recorded(
                turn.agent,
                turn.thread,
                event.seq,
                projected,
                &captured,
                chain,
            );
            turn.wrote(&event);
            Ok(event.seq)
        };
        crate::fallback::answer_observed(
            &self.client,
            attempt.first,
            attempt.second,
            &self.off,
            attempt.prompt,
            using,
            attempt.pieces,
            attempt.separate,
            turn.cancelled.clone(),
            Some(&observer),
        )
        .await
    }
}

fn described(
    turn: &Turn<'_>,
    attempt: &Attempt<'_>,
    model: &Model,
    prompt: &model::Prompt<'_>,
    projection: Captured<RequestProjection>,
    digests: &mut model::Digests,
) -> ModelRequest {
    ModelRequest {
        phase: attempt.phase,
        attempt: turn.attempt.into(),
        work: attempt.work.clone(),
        model: model.named(),
        provider: model.provider.clone(),
        system: digests.of(prompt.system.as_bytes()),
        tools: prompt
            .tools
            .iter()
            .map(|tool| RequestTool {
                name: tool.name.clone(),
                declaration: digests.of(&serde_json::to_vec(tool).expect("tool declaration JSON")),
            })
            .collect(),
        no_calls: prompt.no_calls,
        projection,
        media_delivery: MediaDelivery::CanonicalOnly,
    }
}

/// Every message of `messages`, a request's, and their sources, each
/// digest the one `digests` keeps.
pub(super) fn projected(
    sources: &[Contribution],
    messages: &[model::Message],
    digests: &mut model::Digests,
) -> Projected {
    let mut sources = sources.to_vec();
    for source in &mut sources {
        if source.coverage != Coverage::Whole {
            continue;
        }
        let matched = messages
            .get(source.message as usize)
            .is_some_and(|message| match &source.slot {
                Slot::Text { start, end } => message
                    .text
                    .get(*start as usize..*end as usize)
                    .is_some_and(|text| digests.matches(&source.projected_text, text.as_bytes())),
                Slot::Answer { index, call } => {
                    message.answers.get(*index as usize).is_some_and(|answer| {
                        answer.call == *call
                            && digests.matches(&source.projected_text, answer.text.as_bytes())
                    })
                }
            });
        if !matched {
            source.coverage = Coverage::Unknown {
                reason: "final canonical request differs from the source projection".into(),
            };
        }
    }
    Projected {
        sources,
        messages: messages
            .iter()
            .map(|message| RequestMessage {
                role: message.role.clone(),
                text: digests.of(message.text.as_bytes()),
                media: message
                    .parts
                    .iter()
                    .map(|media| digests.of(&media.bytes))
                    .collect(),
                calls: message
                    .calls
                    .iter()
                    .map(|call| RequestCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: digests
                            .of(&serde_json::to_vec(&call.arguments).expect("JSON arguments")),
                    })
                    .collect(),
                answers: message
                    .answers
                    .iter()
                    .map(|answer| RequestAnswer {
                        call: answer.call.clone(),
                        text: digests.of(answer.text.as_bytes()),
                        failed: answer.failed,
                        media: answer
                            .parts
                            .iter()
                            .map(|media| digests.of(&media.bytes))
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request of a conversation: what's sent from `events`, its
    /// sources and messages digested as a turn's round digests them.
    fn request(events: &[Event], digests: &mut model::Digests) -> Projected {
        digests.sweep();
        let projection = model::projected_kept(events, true, &|_| unreachable!(), digests);
        projected(&projection.sources, &projection.messages, digests)
    }

    /// Turn after turn, a conversation's request digests only the
    /// messages it adds, as many each turn however long the conversation;
    /// a request sent again digests nothing; and every digest is the one
    /// made afresh.
    #[test]
    fn each_message_is_digested_once_across_turns() {
        let mut digests = model::Digests::default();
        let mut events: Vec<Event> = Vec::new();
        let mut added = Vec::new();
        for turn in 1..=40_u64 {
            events.push(Event {
                seq: 2 * turn - 1,
                ..Event::user(&format!("Question {turn}: {}", "words ".repeat(200)))
            });
            let before = digests.made();
            let projected = request(&events, &mut digests);
            added.push(digests.made() - before);
            assert_eq!(projected, request(&events, &mut model::Digests::default()));
            events.push(Event {
                seq: 2 * turn,
                role: event::ASSISTANT.into(),
                text: format!("Answer {turn}"),
                ..Event::default()
            });
        }
        assert!(added[1..].iter().all(|n| *n == added[1]), "{added:?}");
        assert!(added[1] <= 4, "{added:?}");
        let before = digests.made();
        request(&events, &mut digests);
        request(&events, &mut digests);
        assert_eq!(digests.made() - before, 1, "only the last answer is new");
    }
}
