//! A conversation's events as ACP's session updates: what `session/load`
//! replays, and what a running turn streams beside its answer's text.

use link_agent_state_protocol::acp::schema::v1::{
    Content, ContentBlock, ContentChunk, SessionUpdate, TextContent, ToolCall, ToolCallContent,
    ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};

use crate::event::{self, Event};
mod carried;
mod evidence;
pub(crate) use carried::{carried, goes_on};
pub(super) use evidence::session;

/// Every event of a conversation that a person reads, as the updates
/// that would have streamed it: the messages, the agent's answers, its
/// calls and their answers, a command as its owner typed it. Summaries
/// and thinking are the model's, and left out.
pub fn replayed(events: &[Event]) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();
    for event in events {
        if let Some(update) = &event.portable {
            updates.extend(update.iter().cloned());
            continue;
        }
        match event.role.as_str() {
            event::USER | event::COMMAND => {
                updates.push(SessionUpdate::UserMessageChunk(chunk(&event.text)));
            }
            event::ASSISTANT if !event.text.trim().is_empty() => {
                updates.push(SessionUpdate::AgentMessageChunk(chunk(&event.text)));
            }
            _ => {}
        }
        updates.extend(beside(event));
    }
    updates
}

/// A portable export includes the non-text records kept by the store.
/// Imported updates are copied exactly; native media is embedded from
/// the store rather than exposing a machine's file paths.
pub fn archived(
    events: &[Event],
    mut part: impl FnMut(&event::Part) -> anyhow::Result<Vec<u8>>,
) -> anyhow::Result<Vec<SessionUpdate>> {
    let mut updates = Vec::new();
    for event in events {
        if let Some(portable) = &event.portable {
            updates.extend(portable.iter().cloned());
            continue;
        }
        if internal(event) {
            continue;
        }
        for thinking in &event.thinking {
            let text = thinking
                .get("thinking")
                .or_else(|| thinking.get("text"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let chunk = serde_json::from_value(serde_json::json!({
                "content": {"type": "text", "text": text},
                "_meta": {"link": {"nativeThinking": thinking}}
            }))?;
            updates.push(SessionUpdate::AgentThoughtChunk(chunk));
        }
        let mut native = replayed(std::slice::from_ref(event));
        if event.from.is_some() {
            let said = link_connection_contract::acp::Said {
                from: event.from.clone(),
                from_name: event.from_name.clone(),
                hops: event.hops,
                ..Default::default()
            };
            for update in &mut native {
                if let SessionUpdate::UserMessageChunk(chunk) = update {
                    chunk.meta = Some(
                        said.clone()
                            .into_meta(link_connection_contract::acp::Meta::new()),
                    );
                }
            }
        }

        for file in &event.parts {
            let bytes = part(file)?;
            let media = event::Media::new(&file.media, file.name.as_deref(), bytes)
                .map_err(anyhow::Error::msg)?;
            let content: ContentBlock = serde_json::from_value(match file.kind {
                event::Kind::Image => {
                    serde_json::json!({"type": "image", "mimeType": file.media, "data": media.encoded()})
                }
                event::Kind::Voice => {
                    serde_json::json!({"type": "audio", "mimeType": file.media, "data": media.encoded()})
                }
                event::Kind::Document => serde_json::json!({"type": "resource", "resource": {
                    "uri": format!("attachment:{}", file.file), "mimeType": file.media, "blob": media.encoded()
                }}),
            })?;
            if event.role == event::TOOL {
                if let Some(SessionUpdate::ToolCallUpdate(update)) = native.last_mut() {
                    update
                        .fields
                        .content
                        .get_or_insert_with(Vec::new)
                        .push(ToolCallContent::Content(Content::new(content)));
                }
            } else {
                let chunk = ContentChunk::new(content);
                native.push(if event.role == event::ASSISTANT {
                    SessionUpdate::AgentMessageChunk(chunk)
                } else {
                    SessionUpdate::UserMessageChunk(chunk)
                });
            }
        }
        updates.extend(native);
    }
    Ok(updates)
}

/// Internal records have no user-facing ACP message role. Their media
/// references and explicit byte omissions belong to the native manifest.
fn internal(event: &Event) -> bool {
    matches!(event.role.as_str(), event::EVIDENCE | event::SUMMARY)
}

/// What a turn's `event` adds beside its answer's text, which streams
/// apart: each of an answer's calls, begun, and a tool's answer, ending
/// its call.
pub fn beside(event: &Event) -> Vec<SessionUpdate> {
    match event.role.as_str() {
        event::ASSISTANT => event
            .calls
            .iter()
            .zip(0..)
            .map(|(call, ordinal)| {
                let id = match event.evidence.as_ref().map(|evidence| &evidence.record) {
                    Some(link_harness_wire::evidence::Record::Proposed(_)) => {
                        link_harness_wire::evidence::CallRef {
                            proposal: event.seq,
                            ordinal,
                        }
                        .portable_id()
                    }
                    _ => call.id.clone(),
                };
                SessionUpdate::ToolCall(
                    ToolCall::new(ToolCallId::new(id), call.name.clone())
                        .name(call.name.clone())
                        .kind(kind(&call.name))
                        .status(ToolCallStatus::InProgress)
                        .raw_input(call.arguments.clone()),
                )
            })
            .collect(),
        event::TOOL => {
            let Some(provider_call) = &event.call else {
                return Vec::new();
            };
            let call = match event.evidence.as_ref().map(|evidence| &evidence.record) {
                Some(link_harness_wire::evidence::Record::Finished { call, .. }) => {
                    call.portable_id()
                }
                _ => provider_call.clone(),
            };
            let status = match event.failed {
                true => ToolCallStatus::Failed,
                false => ToolCallStatus::Completed,
            };
            let fields =
                ToolCallUpdateFields::new()
                    .status(status)
                    .content(vec![ToolCallContent::Content(Content::new(
                        ContentBlock::Text(TextContent::new(event.text.clone())),
                    ))]);
            vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                ToolCallId::new(call),
                fields,
            ))]
        }
        _ => Vec::new(),
    }
}

/// `update`, a tool call's begun, marked shown or not as `verbose`
/// says (`/chat:verbose`), for the bridge in place of its spec's
/// `[connections] activity`; any other, and with none, as it is.
pub fn shown(update: SessionUpdate, verbose: Option<bool>) -> SessionUpdate {
    match (update, verbose) {
        (SessionUpdate::ToolCall(call), Some(activity)) => {
            let said = link_connection_contract::acp::Said {
                activity: Some(activity),
                ..Default::default()
            };
            SessionUpdate::ToolCall(
                call.meta(said.into_meta(link_connection_contract::acp::Meta::new())),
            )
        }
        (update, _) => update,
    }
}

/// `update`, a tool call's begun, titled with what it works on
/// (`file read notes/plan.md`), as `summary` says: the arguments its
/// tool named for it ([`crate::tools::SUMMARY_META`]) and the tool's
/// schema. Any other update, and a call with no summary, as it is.
pub fn titled(
    update: SessionUpdate,
    summary: Option<(Vec<String>, serde_json::Value)>,
) -> SessionUpdate {
    match (update, summary) {
        (SessionUpdate::ToolCall(mut call), Some((arguments, schema))) => {
            let name = call.name.clone().unwrap_or_else(|| call.title.clone());
            let given = call.raw_input.clone().unwrap_or_default();
            call.title = title(&name, &arguments, &schema, &given);
            SessionUpdate::ToolCall(call)
        }
        (update, _) => update,
    }
}

/// A call of tool `name` as a person reads it: its server's name and
/// its own (`run command`), then each of `arguments` it was `given`, as
/// typed, a list's items each, quoted where it holds a space. One its
/// `schema` says is sensitive (`x-link-sensitive`) is never shown.
fn title(
    name: &str,
    arguments: &[String],
    schema: &serde_json::Value,
    given: &serde_json::Value,
) -> String {
    use serde_json::Value;
    let mut words = vec![link_connection_contract::policy::call_words(name)];
    let word = |value: &Value| match value {
        Value::String(text) if text.is_empty() || text.contains(char::is_whitespace) => {
            Some(format!("'{text}'"))
        }
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(yes) => Some(yes.to_string()),
        _ => None,
    };
    for argument in arguments {
        if schema["properties"][argument]["x-link-sensitive"] == Value::Bool(true) {
            continue;
        }
        match &given[argument] {
            Value::Array(items) => words.extend(items.iter().filter_map(word)),
            value => words.extend(word(value)),
        }
    }
    words.join(" ")
}

fn chunk(text: &str) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
}

/// What a tool's call does, as ACP sorts them for a client's icons:
/// by the tool and call of its name (`file__read`).
fn kind(name: &str) -> ToolKind {
    let (tool, call) = name
        .split_once(crate::tools::SEPARATOR)
        .unwrap_or((name, ""));
    match (tool, call) {
        ("file", "read" | "list") => ToolKind::Read,
        ("file", "write" | "edit") => ToolKind::Edit,
        ("file", "glob" | "grep") | ("memory", "search") | ("web", "search") => ToolKind::Search,
        ("web", "fetch") => ToolKind::Fetch,
        ("run", _) => ToolKind::Execute,
        _ => ToolKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Call;

    /// File's calls sort as their Claude Code namesakes do: Read reads,
    /// Write and Edit edit, Glob and Grep search.
    #[test]
    fn files_calls_sort_by_their_trained_names() {
        for (name, sorted) in [
            ("file__read", ToolKind::Read),
            ("file__write", ToolKind::Edit),
            ("file__edit", ToolKind::Edit),
            ("file__glob", ToolKind::Search),
            ("file__grep", ToolKind::Search),
        ] {
            assert_eq!(kind(name), sorted, "{name}");
        }
    }

    #[test]
    fn a_conversation_replays_as_it_streamed() {
        let call = Event {
            role: event::ASSISTANT.into(),
            calls: vec![Call {
                id: "c1".into(),
                name: "file__read".into(),
                arguments: serde_json::json!({"path": "a"}),
            }],
            ..Event::default()
        };
        let answered = Event {
            role: event::TOOL.into(),
            call: Some("c1".into()),
            text: "its text".into(),
            ..Event::default()
        };
        let summary = Event {
            role: event::SUMMARY.into(),
            text: "the model's".into(),
            ..Event::default()
        };
        let answer = Event {
            role: event::ASSISTANT.into(),
            ..Event::user("done")
        };
        let events = [Event::user("hello"), call, answered, summary, answer];
        let wire = serde_json::to_value(replayed(&events)).unwrap();
        let kinds: Vec<&str> = wire
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["sessionUpdate"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "user_message_chunk",
                "tool_call",
                "tool_call_update",
                "agent_message_chunk"
            ]
        );
        assert_eq!(wire[1]["kind"], "read");
        assert_eq!(wire[1]["rawInput"]["path"], "a");
        assert_eq!(wire[2]["status"], "completed");
        assert_eq!(wire[3]["content"]["text"], "done");
        // A turn streams its answer's text apart: beside it, only calls.
        assert!(beside(&events[4]).is_empty());
    }

    /// A call's title names its tool and what it works on, its summary's
    /// arguments as typed, never one its schema says is sensitive.
    #[test]
    fn a_call_is_titled_with_what_it_works_on() {
        let schema = serde_json::json!({"properties": {
            "program": {"type": "string"}, "args": {"type": "array"},
            "token": {"type": "string", "x-link-sensitive": true},
        }});
        let call = Event {
            role: event::ASSISTANT.into(),
            calls: vec![event::Call {
                id: "c1".into(),
                name: "run__command".into(),
                arguments: serde_json::json!({
                    "program": "git", "args": ["commit", "-m", "a fix"], "token": "hunter2",
                }),
            }],
            ..Event::default()
        };
        let summary = vec!["program".into(), "args".into(), "token".into()];
        let SessionUpdate::ToolCall(titled) =
            titled(beside(&call).remove(0), Some((summary, schema)))
        else {
            panic!("a call");
        };
        assert_eq!(titled.title, "run command git commit -m 'a fix'");
        assert_eq!(titled.name.as_deref(), Some("run__command"));
        // No summary: titled by its name, as it was.
        let SessionUpdate::ToolCall(bare) = super::titled(beside(&call).remove(0), None) else {
            panic!("a call");
        };
        assert_eq!(bare.title, "run__command");
    }

    /// A call begun is marked shown or not as its conversation chose,
    /// for the bridge to read in place of its spec; unchosen, as it is.
    #[test]
    fn a_call_is_marked_as_the_conversation_chose() {
        let call = Event {
            role: event::ASSISTANT.into(),
            calls: vec![event::Call {
                id: "c1".into(),
                name: "file__read".into(),
                arguments: serde_json::json!({}),
            }],
            ..Event::default()
        };
        let begun = || beside(&call).remove(0);
        for (verbose, activity) in [
            (Some(true), Some(true)),
            (Some(false), Some(false)),
            (None, None),
        ] {
            let SessionUpdate::ToolCall(marked) = shown(begun(), verbose) else {
                panic!("a call");
            };
            let said = link_connection_contract::acp::Said::of(marked.meta.as_ref()).unwrap();
            assert_eq!(said.activity, activity);
        }
        assert!(matches!(
            shown(SessionUpdate::AgentMessageChunk(chunk("a")), Some(true)),
            SessionUpdate::AgentMessageChunk(_)
        ));
    }
}
