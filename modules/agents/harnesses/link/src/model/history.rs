//! The conversation as the model reads it: its events made messages, each
//! speaker named, a call and its answer paired.

use super::digests::Digests;
use super::*;
use link_files::reading::ContentDigest;
use link_harness_wire::evidence::request::{Contribution, Coverage, Slot, Source};
#[cfg(test)]
mod attribution_tests;
mod pairing;
#[cfg(test)]
mod replay_tests;
use pairing::pair_answers;

pub struct Projection {
    pub messages: Vec<Message>,
    pub sources: Vec<Contribution>,
}

/// `events` as the prompt's messages, alternating as providers want
/// them: two of the same role in a row (an answer that failed) become
/// one, a tool's answer goes with the owner's next message, and a
/// command's event, the harness's own, is left out.
///
/// With `tools`, calls and answers are the provider's own blocks, each
/// call answered in the next message (a call its turn left unanswered,
/// with [`UNANSWERED`]). Without, as when an agent's tools are taken
/// away, they're written into the text, which every provider takes.
///
/// Each image and document is read with `parts`, the latest within
/// [`PARTS_SENT_MAX`]: one that can't be, or isn't what it says, or is
/// past it, is a line saying so. An owner's message names each of its
/// parts at its head (`[image: photo.jpg]`).
///
/// Each recorded call is named as its tool and command are today, by the
/// release's history ([`link_spec::follow`]): the conversation's own
/// record is never rewritten, and a call since removed keeps its name.
pub fn messages(
    events: &[Event],
    tools: bool,
    parts: &dyn Fn(&Part) -> anyhow::Result<Vec<u8>>,
) -> Vec<Message> {
    projected(events, tools, parts).messages
}

pub fn projected(
    events: &[Event],
    tools: bool,
    parts: &dyn Fn(&Part) -> anyhow::Result<Vec<u8>>,
) -> Projection {
    projected_by(
        events,
        tools,
        parts,
        &link_spec::follow::replayed,
        &mut ContentDigest::of,
    )
}

/// [`projected`], each digest the one `digests` keeps, made once
/// ([`Digests`]).
pub fn projected_kept(
    events: &[Event],
    tools: bool,
    parts: &dyn Fn(&Part) -> anyhow::Result<Vec<u8>>,
    digests: &mut Digests,
) -> Projection {
    projected_by(
        events,
        tools,
        parts,
        &link_spec::follow::replayed,
        &mut |bytes| digests.of(bytes),
    )
}

/// [`projected`], each call named by `named`, each digest made by
/// `digest`.
fn projected_by(
    events: &[Event],
    tools: bool,
    parts: &dyn Fn(&Part) -> anyhow::Result<Vec<u8>>,
    named: &dyn Fn(&str) -> String,
    digest: &mut dyn FnMut(&[u8]) -> ContentDigest,
) -> Projection {
    let sent = sent(events);
    let mut sources = Vec::new();
    let mut messages: Vec<Message> = Vec::new();
    for event in events {
        let role = match event.role.as_str() {
            event::USER | event::TOOL | event::SUMMARY => event::USER,
            event::ASSISTANT => event::ASSISTANT,
            _ => continue,
        };
        let attached: Vec<Result<Media, String>> = event
            .parts
            .iter()
            .filter(|part| part.kind != Kind::Voice)
            .map(|part| read(part, sent.contains(part.file.as_str()), parts))
            .collect();
        let left: Vec<&str> = attached
            .iter()
            .filter_map(|a| a.as_ref().err().map(String::as_str))
            .collect();
        let media = attached.iter().filter_map(|a| a.as_ref().ok().cloned());
        if messages.last().is_none_or(|last| last.role != role) {
            messages.push(Message {
                role: role.into(),
                ..Message::default()
            });
        }
        let message_index = messages.len().saturating_sub(1) as u32;
        let Some(last) = messages.last_mut() else {
            continue;
        };
        let start = last.text.len();
        let omissions = left.iter().map(|why| (*why).to_owned()).collect();
        let selected_media = event
            .parts
            .iter()
            .filter(|part| part.kind != Kind::Voice)
            .zip(&attached)
            .filter(|(_, loaded)| loaded.is_ok())
            .map(|(part, _)| part.clone())
            .collect();
        match event.role.as_str() {
            event::TOOL if tools => last.answers.push(Answer {
                call: event.call.clone().unwrap_or_default(),
                text: [event.text.as_str()]
                    .into_iter()
                    .chain(left)
                    .collect::<Vec<_>>()
                    .join("\n"),
                failed: event.failed,
                parts: media.collect(),
            }),
            // Quoted, as from others: a tool's answer is the owner's
            // message's text only here, and none of it reads as theirs.
            event::TOOL => {
                let tool = event
                    .tool
                    .as_deref()
                    .map_or_else(|| "a tool".to_string(), named);
                let said = if event.failed { "failed" } else { "answered" };
                let text = [event.text.as_str()]
                    .into_iter()
                    .chain(left)
                    .collect::<Vec<_>>();
                add_text(
                    last,
                    &format!(
                        "({tool} {said}, not your owner:)\n{}",
                        quoted(&text.join("\n"))
                    ),
                );
                last.parts.extend(media);
            }
            // What left the window, in the agent's own words, before
            // what's still in it.
            // When it was written, so it can say when it compacted.
            event::SUMMARY => add_text(
                last,
                &format!("{SUMMARY_HEAD}\n(Written {}.)\n\n{}", event.at, event.text),
            ),
            event::USER => {
                // Each part named, or why it was left out, first.
                let text: Vec<String> = attached
                    .iter()
                    .map(|a| a.as_ref().map_or_else(Clone::clone, Media::named))
                    .chain((!event.text.is_empty()).then(|| event.text.clone()))
                    .collect();
                let text = text.join("\n");
                last.parts.extend(media);
                let said = match &event.from {
                    // Who's speaking, when it isn't the owner: a line of
                    // its own, and each line of theirs quoted, so none
                    // reads as the owner's.
                    Some(from) => from_others(from, event.from_name.as_deref(), &text),
                    None => text,
                };
                // When it came, its connection and who's speaking, first.
                match &event.now {
                    Some(now) => add_text(last, &format!("{now}\n{said}")),
                    None => add_text(last, &said),
                }
            }
            _ => {
                add_text(last, &event.text);
                if tools {
                    last.thinking.extend(event.thinking.iter().cloned());
                }
                for call in &event.calls {
                    let call = &crate::event::Call {
                        name: named(&call.name),
                        ..call.clone()
                    };
                    match tools {
                        true => last.calls.push(call.clone()),
                        false => {
                            add_text(last, &format!("[called {} {}]", call.name, call.arguments))
                        }
                    }
                }
            }
        }
        let (slot, projected_text) = if event.role == event::TOOL && tools {
            let index = last.answers.len() - 1;
            let answer = &last.answers[index];
            (
                Slot::Answer {
                    index: index as u32,
                    call: answer.call.clone(),
                },
                digest(answer.text.as_bytes()),
            )
        } else {
            (
                Slot::Text {
                    start: start as u64,
                    end: last.text.len() as u64,
                },
                digest(&last.text.as_bytes()[start..]),
            )
        };
        sources.push(Contribution {
            source: if event.seq == 0 {
                Source::Synthetic
            } else if event.portable.is_some() {
                Source::Imported { seq: event.seq }
            } else {
                Source::Native { seq: event.seq }
            },
            message: message_index,
            slot,
            source_text: digest(event.text.as_bytes()),
            projected_text,
            coverage: Coverage::Whole,
            media: selected_media,
            omissions,
        });
    }
    if tools {
        pair_answers(&mut messages, &mut sources);
    }
    Projection { messages, sources }
}

/// The files of the parts of `events` a prompt sends: the latest, as
/// many as [`PARTS_SENT_MAX`] holds.
fn sent(events: &[Event]) -> std::collections::HashSet<&str> {
    let mut room = PARTS_SENT_MAX;
    let mut sent = std::collections::HashSet::new();
    let parts = events
        .iter()
        .rev()
        .filter(|event| event.role != event::EVIDENCE)
        .flat_map(|e| e.parts.iter().rev());
    for part in parts.filter(|part| part.kind != Kind::Voice) {
        if part.bytes <= room {
            room -= part.bytes;
            sent.insert(part.file.as_str());
        }
    }
    sent
}

/// `part` read with `parts`, when it's `sent`, and checked again as it
/// was when it came; else a line naming it and saying why it's left out.
fn read(
    part: &Part,
    sent: bool,
    parts: &dyn Fn(&Part) -> anyhow::Result<Vec<u8>>,
) -> Result<Media, String> {
    let left = |why: &str| format!("{} was left out: {why}", part.named());
    if !sent {
        return Err(left(&format!(
            "a prompt holds at most {} MiB of images and documents, the latest first",
            PARTS_SENT_MAX >> 20
        )));
    }
    let bytes = parts(part).map_err(|err| left(&format!("its file can't be read ({err:#})")))?;
    Media::new(&part.media, part.name.as_deref(), bytes).map_err(|why| left(&why))
}

/// Who a message from `from` is from, for the model: never the owner. A
/// person in a shared chat by `name`, as their connection gives it, and
/// their connection.
pub fn speaker(from: &str, name: Option<&str>) -> String {
    if let Ok(member) = from.parse::<link_agent_identity::access::ChatMember>() {
        let connection = crate::prompt::title(member.connection());
        let who = match name {
            Some(name) => format!("from {:?}", link_format::clean(name)),
            None => "from someone".to_string(),
        };
        return format!(
            "{who} on {connection}, in a chat your owner shared you with, not your owner"
        );
    }
    match from.split_once(':') {
        Some(("schedule", id)) => format!("a message you scheduled ({id}), not from your owner"),
        Some(("task", id)) => format!("from your subagent {id}, not your owner"),
        Some(("agent", id)) => format!("from agent {id}, not your owner"),
        Some(("run", id)) => format!("from your background command {id}, not your owner"),
        Some(("webhook", name)) => {
            format!("a post to your webhook {name}, a stranger's, not your owner")
        }
        _ => "not from your owner".into(),
    }
}

/// A message from `from`, who isn't the owner, as the model reads it:
/// who's speaking, then its text with each line quoted (`> `), blank
/// ones too, so no paragraph of it passes for the owner's.
fn from_others(from: &str, name: Option<&str>, text: &str) -> String {
    format!("({}:)\n{}", speaker(from, name), quoted(text))
}

/// `text` with each line quoted (`> `), blank ones too: a line ends at
/// any of Unicode's line breaks.
pub(super) fn quoted(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split([
            '\n', '\r', '\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}',
        ])
        .map(|line| match line {
            "" => ">".to_string(),
            line => format!("> {line}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn add_text(message: &mut Message, text: &str) {
    if !message.text.is_empty() && !text.is_empty() {
        message.text.push_str("\n\n");
    }
    message.text.push_str(text);
}
