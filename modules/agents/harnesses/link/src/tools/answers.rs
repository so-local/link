//! A call's answer, as text and parts within its size ([`Answered`]).

use super::*;

/// A tool's answer to a call, for the model: its text, whether it
/// failed, and the images and documents it holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answered {
    pub text: String,
    pub failed: bool,
    pub media: Vec<Media>,
    pub media_sources: Vec<usize>,
    pub media_omissions: std::collections::BTreeMap<usize, String>,
    pub observation: Observation,
    pub original_text_bytes: usize,
    pub cut_text_bytes: usize,
    pub read_authority: Option<link_harness_wire::evidence::ReadAuthority>,
}

impl Answered {
    /// An answer saying `why` the call failed.
    pub fn failed(why: &str) -> Self {
        Self {
            text: why.to_string(),
            failed: true,
            ..Self::default()
        }
    }

    pub(super) fn unknown(why: &str) -> Self {
        Self {
            observation: Observation::Unknown,
            ..Self::failed(why)
        }
    }
}

/// A tool's answer (`tools/call`'s result): its text parts, what else
/// it holds named by kind, the text cut to `max` bytes; its images
/// (`image`) and the images and documents of its resources (`blob`), as
/// a message's are taken, at most [`crate::event::PARTS_MAX`] (one that
/// isn't taken is named, and why); and whether it says it failed.
pub fn answered(result: &Value, max: usize) -> Answered {
    let mut parts = Vec::new();
    let mut media = Vec::new();
    let mut media_sources = Vec::new();
    let mut media_omissions = std::collections::BTreeMap::new();
    let mut take = |index: usize, kind: &str, name: Option<&str>, data: &Value, mime: &Value| {
        let decoded = data
            .as_str()
            .ok_or_else(|| "it holds no data".to_string())
            .and_then(crate::event::decode)
            .and_then(|bytes| Media::new(mime.as_str().unwrap_or_default(), name, bytes));
        match decoded {
            Ok(_) if media.len() >= crate::event::PARTS_MAX => {
                let why = format!(
                    "[{kind} left out: an answer holds at most {} images and documents]",
                    crate::event::PARTS_MAX
                );
                media_omissions.insert(index, why.clone());
                why
            }
            Ok(taken) => {
                let named = taken.named();
                media.push(taken);
                media_sources.push(index);
                named
            }
            Err(why) => {
                let why = format!("[{kind} left out: {why}]");
                media_omissions.insert(index, why.clone());
                why
            }
        }
    };
    for (index, part) in result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        match part["type"].as_str() {
            Some("text") => parts.push(part["text"].as_str().unwrap_or_default().to_string()),
            Some("image") => {
                parts.push(take(index, "image", None, &part["data"], &part["mimeType"]))
            }
            Some("resource") => {
                let resource = &part["resource"];
                match (resource["text"].as_str(), resource.get("blob")) {
                    (Some(text), _) => parts.push(text.to_string()),
                    (None, Some(blob)) => {
                        let name = resource["uri"]
                            .as_str()
                            .and_then(|uri| uri.rsplit('/').next());
                        parts.push(take(index, "a resource", name, blob, &resource["mimeType"]));
                    }
                    (None, None) => parts.push("[a resource]".into()),
                }
            }
            Some(kind) => parts.push(format!("[{}]", link_format::clean(kind))),
            None => {}
        }
    }
    let mut text = parts.join("\n");
    if text.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        text = structured.to_string();
    }
    Answered {
        original_text_bytes: text.len(),
        cut_text_bytes: text.len().saturating_sub(fit(&text, max)),
        text: cut(text, max),
        failed: result["isError"] == true,
        media,
        media_sources,
        media_omissions,
        observation: Observation::Reply(result.clone()),
        read_authority: None,
    }
}

pub(super) fn checked(result: &Value, max: usize) -> anyhow::Result<Answered> {
    anyhow::ensure!(result.is_object(), "a tools/call result isn't an object");
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("a tools/call result has no content array"))?;
    anyhow::ensure!(
        content
            .iter()
            .all(|part| part.is_object() && part["type"].is_string()),
        "a tools/call result has malformed content"
    );
    for part in content {
        match part["type"].as_str() {
            Some("text") => anyhow::ensure!(
                part["text"].is_string(),
                "a tool text block has no text string"
            ),
            Some("image") => anyhow::ensure!(
                part["data"].is_string() && part["mimeType"].is_string(),
                "a tool image block has malformed data or media type"
            ),
            Some("resource") => {
                let resource = &part["resource"];
                anyhow::ensure!(resource["uri"].is_string(), "a tool resource has no URI");
                anyhow::ensure!(
                    matches!(
                        (resource.get("text"), resource.get("blob")),
                        (Some(Value::String(_)), None) | (None, Some(Value::String(_)))
                    ),
                    "a tool resource needs exactly one text or blob string"
                );
            }
            _ => {}
        }
    }
    anyhow::ensure!(
        result.get("isError").is_none_or(Value::is_boolean),
        "a tools/call isError isn't a boolean"
    );
    anyhow::ensure!(
        result.get("structuredContent").is_none_or(Value::is_object),
        "a tools/call structuredContent isn't an object"
    );
    Ok(answered(result, max))
}

/// `text` in at most `max` bytes, cut at a character, saying how much
/// was left out.
pub(super) fn cut(mut text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let end = fit(&text, max);
    let left = text.len() - end;
    text.truncate(end);
    text.push_str(&format!("\n[{left} more bytes cut]"));
    text
}

fn fit(text: &str, max: usize) -> usize {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Failed calls reach the process log as well as the model's tool answer,
/// placed there by the conversation and call IDs the log line carries.
pub(super) fn failure_notice(call: &crate::event::Call, answered: &Answered) -> Option<String> {
    answered
        .failed
        .then(|| format!("Tool {} failed: {}", call.name, answered.text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_known_content_is_not_a_completed_mcp_reply() {
        for part in [
            json!({"type":"text","text":7}),
            json!({"type":"image","data":7,"mimeType":"image/png"}),
            json!({"type":"resource","resource":{"uri":"file:///x","text":"both","blob":"both"}}),
        ] {
            assert!(checked(&json!({"content":[part]}), 1024).is_err());
        }
        assert!(checked(&json!({"content":null}), 1024).is_err());
        assert!(checked(&Value::Null, 1024).is_err());
    }

    #[test]
    fn tool_failures_have_a_log_notice_with_the_error_and_no_call_arguments() {
        let call = crate::event::Call {
            name: "browser__open".into(),
            arguments: serde_json::json!({"url":"https://example.org/private-input"}),
            ..Default::default()
        };
        let notice = failure_notice(
            &call,
            &Answered::failed("Chrome: Library not loaded: Chrome Framework"),
        )
        .unwrap();
        assert!(notice.contains("browser__open"));
        assert!(notice.contains("Library not loaded: Chrome Framework"));
        assert!(!notice.contains("private-input"));
        assert!(failure_notice(&call, &Answered::default()).is_none());
    }

    /// A 1×1 PNG, in base64.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    #[test]
    fn answers_as_text_and_parts() {
        let mixed = answered(
            &json!({"content": [
                {"type": "text", "text": "one"},
                {"type": "image", "data": PNG, "mimeType": "image/png"},
                {"type": "image", "data": "...", "mimeType": "image/png"},
                {"type": "audio", "data": "..."},
                {"type": "resource", "resource": {"uri": "file:///a", "text": "two"}},
                {"type": "resource", "resource": {"uri": "file:///b/c.pdf",
                    "blob": "JVBERi0xLjQK", "mimeType": "application/pdf"}},
            ]}),
            1 << 10,
        );
        assert_eq!(
            mixed.text,
            "one\n[image]\n[image left out: its data isn't base64]\n[audio]\ntwo\n[document: c.pdf]"
        );
        assert!(!mixed.failed);
        let kinds: Vec<_> = mixed.media.iter().map(|m| m.media.as_str()).collect();
        assert_eq!(kinds, ["image/png", "application/pdf"]);
        let failed = answered(
            &json!({"content": [], "structuredContent": {"n": 1}, "isError": true}),
            1 << 10,
        );
        assert_eq!(failed.text, r#"{"n":1}"#);
        assert!(failed.failed);
        let long = answered(
            &json!({"content": [{"type": "text", "text": "é".repeat(32 << 10)}]}),
            32 << 10,
        )
        .text;
        assert!(long.len() < (32 << 10) + 40, "{}", long.len());
        assert!(
            long.ends_with("more bytes cut]"),
            "{}",
            &long[long.len() - 30..]
        );
    }
}
