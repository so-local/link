//! Lossless native record envelopes beside a portable readable projection.
//! Each adapter owns its native format and chooses what it can expose.

use link_agent_state_protocol::acp::schema::v1::{
    ContentBlock, ContentChunk, SessionUpdate, TextContent,
};
use serde_json::{Value, json};

/// Preserve the raw record and stable native correlation without promoting
/// system records, compaction metadata or vendor extensions into instructions.
pub fn original(adapter: &str, session: &str, record: &str, raw: Value) -> SessionUpdate {
    let chunk = serde_json::from_value(json!({"content":{"type":"text","text":""},
        "_meta":{"link":{"nativeRecord":raw,"source":{
            "adapter":adapter,"session":session,"record":record}}}}))
    .expect("native record metadata");
    SessionUpdate::AgentThoughtChunk(chunk)
}

pub fn text(user: bool, text: &str, record: &str) -> SessionUpdate {
    let mut chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
    chunk.message_id = Some(record.to_string().into());
    if user {
        SessionUpdate::UserMessageChunk(chunk)
    } else {
        SessionUpdate::AgentMessageChunk(chunk)
    }
}

/// Text blocks shared by native message formats. Non-text blocks remain in
/// `original`; an adapter separately projects supported tools and media.
pub fn text_content(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            matches!(
                part["type"].as_str(),
                Some("text" | "input_text" | "output_text")
            )
            .then(|| part["text"].as_str())
            .flatten()
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn source(update: &SessionUpdate) -> Option<(crate::store::context::Source, &Value)> {
    let SessionUpdate::AgentThoughtChunk(chunk) = update else {
        return None;
    };
    let link = chunk.meta.as_ref()?.get("link")?;
    let source = serde_json::from_value(link.get("source")?.clone()).ok()?;
    Some((source, link.get("nativeRecord")?))
}
