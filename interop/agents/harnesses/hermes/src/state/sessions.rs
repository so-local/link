//! Hermes's native message records mapped to ACP updates. Imported
//! updates are archived beside their native projection for exact export.

use link_harness_contract::asp::{
    acp::schema::v1::{
        Content, ContentBlock, ContentChunk, SessionId, SessionUpdate, TextContent, ToolCall,
        ToolCallContent, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    },
    forms::Session,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Native {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Call>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_summary: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Call {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: Function,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Function {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Exported {
    pub id: String,
    pub title: Option<String>,
    pub messages: Vec<Native>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Archive {
    pub session: Session,
    pub projected: Vec<Native>,
}

/// The sessions Link imported into Hermes, each archived by the ID Hermes
/// gave it, beside Hermes's own state: `.hermes/link-imports.json`.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Imports {
    pub sessions: BTreeMap<String, Archive>,
}

link_files::specification!(Imports, include_str!("imports.schema.toml"));

/// The sessions imported, which an upgrade converts.
pub fn saved_imports() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Imports>()
}

impl Native {
    fn message(role: &str, content: Value) -> Self {
        Self {
            raw: None,
            role: role.into(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
            reasoning_details: None,
            is_summary: None,
        }
    }
}

fn chunk(block: ContentBlock) -> ContentChunk {
    ContentChunk::new(block)
}
fn text(text: impl Into<String>) -> ContentBlock {
    ContentBlock::Text(TextContent::new(text))
}

/// Native text and media, kept as ACP content without fetching a URL.
fn opaque(content: &Value) -> ContentBlock {
    serde_json::from_value(json!({"type": "resource", "resource": {
        "uri": "native:content", "mimeType": "application/json", "text": content.to_string()
    }}))
    .expect("an embedded JSON resource")
}

fn blocks(content: &Option<Value>) -> Vec<ContentBlock> {
    let Some(content) = content else {
        return Vec::new();
    };
    if content.is_null() {
        return Vec::new();
    }
    if let Some(textual) = content.as_str() {
        return vec![text(textual)];
    }
    let Some(parts) = content.as_array() else {
        return vec![opaque(content)];
    };
    parts
        .iter()
        .map(|part| {
            if matches!(
                part["type"].as_str(),
                Some("text" | "input_text" | "output_text")
            ) && let Some(textual) = part["text"].as_str()
            {
                return text(textual);
            }
            if part["type"] == "image_url"
                && let Some(url) = part["image_url"]["url"].as_str()
            {
                let image = if let Some((mime, data)) = url
                    .strip_prefix("data:")
                    .and_then(|data| data.split_once(";base64,"))
                {
                    json!({"type": "image", "mimeType": mime, "data": data})
                } else {
                    json!({"type": "resource_link", "uri": url, "name": "image"})
                };
                if let Ok(block) = serde_json::from_value(image) {
                    return block;
                }
            }
            opaque(part)
        })
        .collect()
}

pub(super) fn exported(native: &Exported) -> Session {
    let mut updates = Vec::new();
    for message in &native.messages {
        if let Some(raw) = &message.raw {
            // Hermes changes these flags when compacting or rewinding a row.
            // Each native revision keeps its own correlation and original.
            let id = serde_json::to_string(&(&raw["id"], &raw["active"], &raw["compacted"]))
                .expect("native row revision");
            updates.push(link_harness_contract::harness::transcript::original(
                "hermes",
                &native.id,
                &id,
                raw.clone(),
            ));
        }
        if message.reasoning_content.is_some() || message.reasoning_details.is_some() {
            let thought = serde_json::from_value(json!({"content": {"type": "text", "text": message.reasoning_content.as_deref().unwrap_or_default()},
                "_meta": {"link": {"nativeReasoning": message.reasoning_details}}})).expect("thinking metadata");
            updates.push(SessionUpdate::AgentThoughtChunk(thought));
        }
        let content = blocks(&message.content);
        if message.is_summary == Some(true) {
            let summary = serde_json::from_value(json!({"content": {"type": "text", "text": ""},
                "_meta": {"link": {"nativeSummary": message}}}))
            .expect("summary metadata");
            updates.push(SessionUpdate::AgentThoughtChunk(summary));
            continue;
        }
        match message.role.as_str() {
            "user" => updates.extend(
                content
                    .into_iter()
                    .map(|block| SessionUpdate::UserMessageChunk(chunk(block))),
            ),
            "assistant" => {
                updates.extend(
                    content
                        .into_iter()
                        .map(|block| SessionUpdate::AgentMessageChunk(chunk(block))),
                );
                for call in message.tool_calls.iter().flatten() {
                    let input = serde_json::from_str::<Value>(&call.function.arguments)
                        .unwrap_or_else(|_| json!(call.function.arguments));
                    updates.push(SessionUpdate::ToolCall(
                        ToolCall::new(ToolCallId::new(call.id.clone()), call.function.name.clone())
                            .name(call.function.name.clone())
                            .status(ToolCallStatus::InProgress)
                            .raw_input(input),
                    ));
                }
            }
            "tool" => {
                if let Some(id) = &message.tool_call_id {
                    updates.push(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        ToolCallId::new(id.clone()),
                        ToolCallUpdateFields::new()
                            .status(ToolCallStatus::Completed)
                            .content(
                                content
                                    .into_iter()
                                    .map(|block| ToolCallContent::Content(Content::new(block)))
                                    .collect::<Vec<_>>(),
                            ),
                    )));
                }
            }
            _ => {
                // System and vendor records stay archival; a transcript
                // record never becomes a new instruction to a model.
                let chunk = serde_json::from_value(json!({"content": {"type": "text", "text": ""},
                    "_meta": {"link": {"nativeRecord": message}}}))
                .expect("native record metadata");
                updates.push(SessionUpdate::AgentThoughtChunk(chunk));
            }
        }
    }
    Session {
        evidence: None,
        harness: None,
        session_id: SessionId::new(native.id.clone()),
        title: native.title.clone(),
        updates,
    }
}

fn native_content(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text(text) => Some(json!({"type": "text", "text": text.text})),
        ContentBlock::Image(image) => Some(
            json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", image.mime_type, image.data)}}),
        ),
        ContentBlock::Resource(resource) => {
            use link_harness_contract::asp::acp::schema::v1::EmbeddedResourceResource;
            match &resource.resource {
                EmbeddedResourceResource::TextResourceContents(text) => Some(
                    json!({"type": "text", "text": format!("[resource {}]\n{}", text.uri, text.text)}),
                ),
                _ => None,
            }
        }
        _ => None,
    }
}

fn append(messages: &mut Vec<Native>, role: &str, block: &ContentBlock, new: bool) {
    let Some(content) = native_content(block) else {
        return;
    };
    if !new
        && let Some(last) = messages.last_mut()
        && last.role == role
        && last.tool_calls.is_none()
    {
        match &mut last.content {
            Some(Value::Array(parts)) => parts.push(content),
            Some(Value::String(text)) if content["type"] == "text" => {
                text.push_str(content["text"].as_str().unwrap_or_default())
            }
            Some(old) => {
                *old = json!([{"type": "text", "text": old.as_str().unwrap_or_default()}, content]);
            }
            None => last.content = Some(json!([content])),
        }
        return;
    }
    let value = if content["type"] == "text" {
        content["text"].clone()
    } else {
        json!([content])
    };
    messages.push(Native::message(role, value));
}

fn append_chunk(messages: &mut Vec<Native>, role: &str, chunk: &ContentChunk) {
    let Ok(said) = link_connection_contract::acp::Said::of(chunk.meta.as_ref()) else {
        return;
    };
    if role == "user"
        && let Some(from) = said.from
    {
        let label = match said.from_name {
            Some(name) => format!("{name:?} ({from})"),
            None => from,
        };
        append(
            messages,
            role,
            &text(format!(
                "[Historical message from {label}, not your owner]\n"
            )),
            true,
        );
        append(messages, role, &chunk.content, false);
    } else {
        append(messages, role, &chunk.content, chunk.message_id.is_some());
    }
}

/// Usable native records. Tool history is materialized only once its
/// result exists, so importing an unfinished call never executes it.
pub(super) fn projected(session: &Session) -> Vec<Native> {
    let mut messages = Vec::new();
    let mut calls: BTreeMap<String, ToolCall> = BTreeMap::new();
    for update in &session.updates {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => append_chunk(&mut messages, "user", chunk),
            SessionUpdate::AgentMessageChunk(chunk) => {
                append_chunk(&mut messages, "assistant", chunk)
            }
            SessionUpdate::ToolCall(call) => {
                let id = call.tool_call_id.0.to_string();
                calls.insert(id.clone(), call.clone());
                if matches!(
                    call.status,
                    ToolCallStatus::Completed | ToolCallStatus::Failed
                ) {
                    complete(&mut messages, calls.remove(&id).expect("inserted"));
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let id = update.tool_call_id.0.to_string();
                if let Some(call) = calls.get_mut(&id) {
                    if let Some(status) = update.fields.status {
                        call.status = status;
                    }
                    if let Some(content) = &update.fields.content {
                        call.content = content.clone();
                    }
                    if let Some(output) = &update.fields.raw_output {
                        call.raw_output = Some(output.clone());
                    }
                    if let Some(input) = &update.fields.raw_input {
                        call.raw_input = Some(input.clone());
                    }
                    if let Some(name) = &update.fields.name {
                        call.name = Some(name.clone());
                    }
                    if matches!(
                        call.status,
                        ToolCallStatus::Completed | ToolCallStatus::Failed
                    ) {
                        complete(&mut messages, calls.remove(&id).expect("present"));
                    }
                }
            }
            _ => {}
        }
    }
    messages
}

fn complete(messages: &mut Vec<Native>, call: ToolCall) {
    let Some(name) = call
        .name
        .or_else(|| (!call.title.contains(' ')).then_some(call.title))
    else {
        return;
    };
    let Some(arguments) = call.raw_input.filter(Value::is_object) else {
        return;
    };
    let id = call.tool_call_id.0.to_string();
    let mut assistant = Native::message("assistant", json!(""));
    assistant.tool_calls = Some(vec![Call {
        id: id.clone(),
        kind: "function".into(),
        function: Function {
            name,
            arguments: arguments.to_string(),
        },
    }]);
    messages.push(assistant);
    let parts: Vec<Value> = call
        .content
        .iter()
        .filter_map(|content| match content {
            ToolCallContent::Content(content) => native_content(&content.content),
            _ => None,
        })
        .collect();
    let content = if parts.is_empty() {
        call.raw_output
            .map(|value| {
                if value.is_string() {
                    value
                } else {
                    json!(value.to_string())
                }
            })
            .unwrap_or_else(|| json!(""))
    } else if parts.len() == 1 && parts[0]["type"] == "text" {
        parts[0]["text"].clone()
    } else {
        json!(parts)
    };
    let content = if call.status == ToolCallStatus::Failed {
        match content {
            Value::String(text) => json!(format!("[historical tool call failed]\n{text}")),
            other => {
                json!([{"type": "text", "text": "Historical tool call failed"}, {"type": "text", "text": other.to_string()}])
            }
        }
    } else {
        content
    };
    let mut result = Native::message("tool", content);
    result.tool_call_id = Some(id);
    messages.push(result);
}

impl Archive {
    /// Restore exact imported updates when the native transcript still
    /// starts with their projection; append only the native work since.
    pub fn restore(&self, native: &Exported) -> Option<Session> {
        if native.messages.len() < self.projected.len()
            || !native.messages.iter().zip(&self.projected).all(|(a, b)| {
                let (mut a, mut b) = (a.clone(), b.clone());
                a.raw = None;
                b.raw = None;
                a == b
            })
        {
            return None;
        }
        let after = Exported {
            id: native.id.clone(),
            title: native.title.clone(),
            messages: native.messages[self.projected.len()..].to_vec(),
        };
        let mut session = self.session.clone();
        session.session_id = SessionId::new(native.id.clone());
        session.title = native.title.clone();
        session.updates.extend(exported(&after).updates);
        Some(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_compaction_flags_keep_both_original_revisions() {
        let home = link_test_kit::TempDir::new("hermes-compaction-revisions");
        let mut message = Native::message("user", json!("Check the unfinished invoice"));
        message.raw = Some(json!({"id":7,"active":true,"compacted":false,
            "role":"user","content":"Check the unfinished invoice","extension":"preserve"}));
        let mut native = Exported {
            id: "native-one".into(),
            title: Some("Delivery".into()),
            messages: vec![message],
        };
        let saved = |native: &Exported| {
            link_harness_hub::export_personal(
                &home,
                "hermes",
                home.join("threads.json"),
                link_harness_contract::asp::State {
                    sessions: Some(vec![exported(native)]),
                    ..Default::default()
                },
            )
            .unwrap()
            .sessions
            .unwrap()
        };
        let first = saved(&native);
        native.messages[0].raw.as_mut().unwrap()["compacted"] = json!(true);
        let later = saved(&native);
        assert_eq!(later.len(), 2);
        assert!(later.contains(&first[0]));
        let originals: Vec<_> = later
            .iter()
            .flat_map(|s| &s.updates)
            .filter_map(link_harness_contract::harness::transcript::source)
            .collect();
        assert_eq!(originals.len(), 2);
        assert_ne!(originals[0].0.record, originals[1].0.record);
        assert!(
            originals
                .iter()
                .all(|(_, raw)| raw["id"] == 7 && raw["extension"] == "preserve")
        );
        assert_eq!(saved(&native), later, "repeated backfill is exact");
    }

    #[test]
    fn tools_and_media_reach_native_history_and_the_exact_archive_returns() {
        let updates: Vec<SessionUpdate> = serde_json::from_value(json!([
            {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "Read it"}},
            {"sessionUpdate": "tool_call", "toolCallId": "c1", "name": "file__read", "title": "Read", "rawInput": {"file_path": "notes"}},
            {"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed", "rawOutput": "result"},
            {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "A thought"}},
            {"sessionUpdate": "user_message_chunk", "content": {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="}}
        ])).unwrap();
        let session = Session {
            evidence: None,
            harness: None,
            session_id: SessionId::new("link:main"),
            title: None,
            updates,
        };
        let projection = projected(&session);
        let call = projection
            .iter()
            .find(|message| message.tool_calls.is_some())
            .unwrap();
        assert_eq!(
            call.tool_calls.as_ref().unwrap()[0].function.name,
            "file__read"
        );
        assert!(
            projection
                .iter()
                .any(|message| message.role == "tool"
                    && message.tool_call_id.as_deref() == Some("c1"))
        );
        assert!(
            serde_json::to_string(&projection)
                .unwrap()
                .contains("data:image/png;base64,")
        );
        let archive = Archive {
            session: session.clone(),
            projected: projection.clone(),
        };
        let mut native = Exported {
            id: "native-one".into(),
            title: None,
            messages: projection,
        };
        native
            .messages
            .push(Native::message("assistant", json!("After the switch")));
        let restored = archive.restore(&native).unwrap();
        assert!(restored.updates.starts_with(&session.updates));
        assert_eq!(restored.messages().last().unwrap().text, "After the switch");
        native.messages[0].content = Some(json!("Diverged"));
        assert!(archive.restore(&native).is_none());
    }

    #[test]
    fn native_tool_calls_and_thinking_export_as_non_text_state() {
        let native = Exported {
            id: "native".into(),
            title: None,
            messages: vec![Native {
                raw: None,
                role: "assistant".into(),
                content: Some(json!("done")),
                tool_calls: Some(vec![Call {
                    id: "c1".into(),
                    kind: "function".into(),
                    function: Function {
                        name: "file__read".into(),
                        arguments: "{\"file_path\":\"notes\"}".into(),
                    },
                }]),
                reasoning_content: Some("Thought".into()),
                reasoning_details: Some(json!({"signature": "kept"})),
                is_summary: None,
                tool_call_id: None,
            }],
        };
        let exported = exported(&native);
        assert!(
            exported
                .updates
                .iter()
                .any(|update| matches!(update, SessionUpdate::ToolCall(_)))
        );
        let wire = serde_json::to_string(&exported).unwrap();
        assert!(wire.contains("Thought") && wire.contains("signature") && wire.contains("kept"));
    }
}
