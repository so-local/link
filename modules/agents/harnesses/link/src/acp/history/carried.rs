//! Native inference projections beside the exact imported ACP archive.
//! Historical tools become records only: no call is ever executed here.

use crate::event::{self, Call, Event, Media};
use link_agent_state_protocol::acp::schema::v1::{
    SessionUpdate, ToolCall, ToolCallContent, ToolCallStatus,
};
use std::collections::BTreeMap;

fn projected(mut event: Event) -> Event {
    event.portable = Some(Vec::new());
    event
}

/// Each original update is kept once, including updates inference cannot
/// represent. Native projections remain separate from that archive.
pub(crate) fn carried(updates: &[SessionUpdate]) -> Vec<(Event, Vec<Media>)> {
    let mut events = Vec::new();
    let mut calls: BTreeMap<String, ToolCall> = BTreeMap::new();
    for update in updates {
        events.push((
            Event {
                role: event::COMMAND.into(),
                portable: Some(vec![update.clone()]),
                ..Event::default()
            },
            Vec::new(),
        ));
        match update {
            SessionUpdate::UserMessageChunk(chunk) | SessionUpdate::AgentMessageChunk(chunk) => {
                // An unrepresentable resource remains in the archive. No
                // foreign URI is fetched and no native path is followed.
                if let Ok((text, media)) =
                    super::super::prompt::message(vec![chunk.content.clone()])
                {
                    let Ok(said) = link_connection_contract::acp::Said::of(chunk.meta.as_ref())
                    else {
                        continue;
                    };
                    let mut event = Event::user(&text);
                    event.from = said.from;
                    event.from_name = said.from_name;
                    event.hops = said.hops;
                    if matches!(update, SessionUpdate::AgentMessageChunk(_)) {
                        event.role = event::ASSISTANT.into();
                    }
                    events.push((projected(event), media));
                }
            }
            SessionUpdate::ToolCall(call) => {
                let id = call.tool_call_id.0.to_string();
                calls.insert(id.clone(), call.clone());
                if matches!(
                    call.status,
                    ToolCallStatus::Completed | ToolCallStatus::Failed
                ) {
                    completed(&mut events, calls.remove(&id).expect("inserted"));
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let id = update.tool_call_id.0.to_string();
                if let Some(call) = calls.get_mut(&id) {
                    let fields = &update.fields;
                    if let Some(status) = fields.status {
                        call.status = status;
                    }
                    if let Some(name) = &fields.name {
                        call.name = Some(name.clone());
                    }
                    if let Some(title) = &fields.title {
                        call.title = title.clone();
                    }
                    if let Some(input) = &fields.raw_input {
                        call.raw_input = Some(input.clone());
                    }
                    if let Some(output) = &fields.raw_output {
                        call.raw_output = Some(output.clone());
                    }
                    if let Some(content) = &fields.content {
                        call.content = content.clone();
                    }
                    if matches!(
                        call.status,
                        ToolCallStatus::Completed | ToolCallStatus::Failed
                    ) {
                        completed(&mut events, calls.remove(&id).expect("present"));
                    }
                }
            }
            _ => {}
        }
    }
    events
}

/// Where `carried` goes on from the conversation `kept`: the index of its
/// first event after them, when its owner's turns open with `kept`'s, each
/// holding the words of `kept`'s; none when they part. Another harness
/// may split a turn or wrap its words, so turns are compared, not events.
pub(crate) fn goes_on(kept: &[Event], carried: &[(Event, Vec<Media>)]) -> Option<usize> {
    let own = turns(kept.iter().enumerate());
    let came = turns(carried.iter().map(|(event, _)| event).enumerate());
    let held = own.iter().zip(&came).all(|((_, own), (_, came))| {
        let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        words(came).contains(&words(own))
    });
    if !held || came.len() < own.len() {
        return None;
    }
    // A turn's first event follows the update it came from, kept with it.
    Some(
        came.get(own.len())
            .map_or(carried.len(), |(at, _)| at.saturating_sub(1)),
    )
}

/// The owner's turns of `events`, each where it opens and its words: what
/// one or more messages of the owner's said, with no reply between them.
fn turns<'e>(events: impl Iterator<Item = (usize, &'e Event)>) -> Vec<(usize, String)> {
    let mut turns: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    for (at, event) in events {
        match event.role.as_str() {
            event::USER if open => {
                let turn = turns.last_mut().expect("open");
                turn.1.push(' ');
                turn.1.push_str(&event.text);
            }
            event::USER => {
                turns.push((at, event.text.clone()));
                open = true;
            }
            event::COMMAND => {}
            _ => open = false,
        }
    }
    turns
}

fn completed(events: &mut Vec<(Event, Vec<Media>)>, call: ToolCall) {
    let name = call.name.as_deref().unwrap_or(&call.title);
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return;
    }
    let Some(arguments) = call.raw_input.filter(serde_json::Value::is_object) else {
        return;
    };
    let blocks = call
        .content
        .iter()
        .filter_map(|content| match content {
            ToolCallContent::Content(content) => Some(content.content.clone()),
            _ => None,
        })
        .collect();
    let (text, media) = super::super::prompt::message(blocks).unwrap_or_else(|_| {
        (
            call.raw_output
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            Vec::new(),
        )
    });
    let id = call.tool_call_id.0.to_string();
    events.push((
        projected(Event {
            role: event::ASSISTANT.into(),
            calls: vec![Call {
                id: id.clone(),
                name: name.into(),
                arguments,
            }],
            ..Event::default()
        }),
        Vec::new(),
    ));
    events.push((
        projected(Event {
            role: event::TOOL.into(),
            text,
            call: Some(id),
            tool: Some(name.into()),
            failed: call.status == ToolCallStatus::Failed,
            ..Event::default()
        }),
        media,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn non_text_updates_round_trip_without_replaying_tool_effects() {
        let updates: Vec<SessionUpdate> = serde_json::from_value(json!([
            {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "Read it"}},
            {"sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "Read a file", "name": "file__read",
             "rawInput": {"path": "notes.md"}, "status": "in_progress"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": "completed",
             "content": [{"type": "content", "content": {"type": "text", "text": "kept result"}}]},
            {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "A thought"}},
            {"sessionUpdate": "user_message_chunk", "content": {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="}},
            {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Done"}}
        ])).unwrap();
        let projected = carried(&updates);
        let events: Vec<Event> = projected.iter().map(|(event, _)| event.clone()).collect();
        assert_eq!(super::super::replayed(&events), updates);
        let call = events.iter().find(|event| !event.calls.is_empty()).unwrap();
        assert_eq!(call.calls[0].name, "file__read");
        assert_eq!(call.calls[0].arguments, json!({"path": "notes.md"}));
        let answered = events
            .iter()
            .find(|event| event.role == event::TOOL)
            .unwrap();
        assert_eq!(answered.call.as_deref(), Some("call-1"));
        assert_eq!(answered.text, "kept result");
        assert_eq!(
            projected
                .iter()
                .map(|(_, media)| media.len())
                .sum::<usize>(),
            1
        );
    }

    fn said(chunks: &[(&str, &str)]) -> Vec<(Event, Vec<Media>)> {
        let updates: Vec<SessionUpdate> = chunks
            .iter()
            .map(|(kind, text)| {
                serde_json::from_value(
                    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}}),
                )
                .unwrap()
            })
            .collect();
        carried(&updates)
    }

    /// A conversation that went on elsewhere goes on from its first new
    /// turn, its turns split and wrapped another harness's way; one that
    /// parted, or holds less, doesn't.
    #[test]
    fn a_conversation_goes_on_from_its_first_new_turn() {
        let kept = vec![
            Event::user("remember qa-1"),
            Event {
                role: event::ASSISTANT.into(),
                text: "qa-ok".into(),
                ..Event::default()
            },
        ];
        let (user, agent) = ("user_message_chunk", "agent_message_chunk");
        let on = said(&[
            (user, "[Telegram] remember"),
            (user, "qa-1"),
            (agent, "qa-ok"),
            (user, "hello"),
            (agent, "qa-ok"),
        ]);
        let at = goes_on(&kept, &on).expect("goes on");
        let rest: Vec<&str> = on[at..]
            .iter()
            .map(|(e, _)| e.text.as_str())
            .filter(|t| !t.is_empty())
            .collect();
        assert_eq!(rest, ["hello", "qa-ok"]);
        assert_eq!(goes_on(&kept, &on[..6]), Some(6), "nothing new");
        assert_eq!(goes_on(&[], &on), Some(0));
        assert_eq!(
            goes_on(&kept, &said(&[(user, "forget it"), (agent, "qa-ok")])),
            None,
            "parted"
        );
        assert_eq!(goes_on(&kept, &[]), None, "holds less");
    }
}
