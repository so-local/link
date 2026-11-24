//! Preserve source attribution while pairing model calls and answers.

use super::*;

/// Each call answered in the message after it, in the calls' order, and
/// nothing else answered: a missing answer is [`UNANSWERED`], one to no
/// call left out.
pub(super) fn pair_answers(messages: &mut Vec<Message>, sources: &mut Vec<Contribution>) {
    let mut i = 0;
    while i < messages.len() {
        let calls: Vec<String> = match i.checked_sub(1).map(|j| &messages[j]) {
            Some(before) if before.role == event::ASSISTANT => {
                before.calls.iter().map(|c| c.id.clone()).collect()
            }
            _ => Vec::new(),
        };
        if messages[i].role == event::USER {
            let given = std::mem::take(&mut messages[i].answers);
            let chosen: Vec<_> = calls
                .iter()
                .map(|id| given.iter().position(|answer| &answer.call == id))
                .collect();
            for source in sources
                .iter_mut()
                .filter(|source| source.message as usize == i)
            {
                if let Slot::Answer { index, .. } = &mut source.slot {
                    match chosen
                        .iter()
                        .position(|chosen| *chosen == Some(*index as usize))
                    {
                        Some(next) => *index = next as u32,
                        None => {
                            source.coverage = Coverage::Omitted {
                                reason: "unasked or repeated tool answer".into(),
                            }
                        }
                    }
                }
            }
            for (index, _) in chosen
                .iter()
                .enumerate()
                .filter(|(_, chosen)| chosen.is_none())
            {
                sources.push(Contribution {
                    source: Source::Synthetic,
                    message: i as u32,
                    slot: Slot::Answer {
                        index: index as u32,
                        call: calls[index].clone(),
                    },
                    source_text: ContentDigest::of(UNANSWERED.as_bytes()),
                    projected_text: ContentDigest::of(UNANSWERED.as_bytes()),
                    coverage: Coverage::Whole,
                    media: vec![],
                    omissions: vec![],
                });
            }
            messages[i].answers = calls
                .iter()
                .map(|id| {
                    given
                        .iter()
                        .find(|a| &a.call == id)
                        .cloned()
                        .unwrap_or(Answer {
                            call: id.clone(),
                            text: UNANSWERED.into(),
                            failed: true,
                            parts: Vec::new(),
                        })
                })
                .collect();
        }
        let unanswered = messages[i].role == event::ASSISTANT
            && !messages[i].calls.is_empty()
            && messages
                .get(i + 1)
                .is_none_or(|next| next.role != event::USER);
        if unanswered {
            for source in sources
                .iter_mut()
                .filter(|source| source.message as usize > i)
            {
                source.message += 1;
            }
            messages.insert(
                i + 1,
                Message {
                    role: event::USER.into(),
                    ..Message::default()
                },
            );
        }
        i += 1;
    }
}
