//! A provider's request split into its parts, by the API it speaks:
//! names and numbers kept, never its words.

use serde_json::Value;

use super::{IMAGE_TOKENS, NAME_MAX, PARTS_MAX, PDF_TOKENS_MAX, Part, text_tokens};

/// The API a request for a model's answer speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    /// Anthropic's Messages.
    Anthropic,
    /// OpenAI's Chat Completions, and those that speak it.
    Chat,
    /// OpenAI's Responses.
    Responses,
    /// OpenAI's Completions: a prompt alone.
    Completions,
    /// Ollama's `/api/chat`.
    OllamaChat,
    /// Ollama's `/api/generate`.
    OllamaGenerate,
}

impl Api {
    /// The API of a `POST` to `path` (no `?query`), when it asks for a
    /// model's answer.
    pub fn of(path: &str) -> Option<Api> {
        Some(match path {
            "/v1/messages" => Api::Anthropic,
            "/v1/chat/completions" => Api::Chat,
            "/v1/responses" => Api::Responses,
            "/v1/completions" => Api::Completions,
            "/api/chat" => Api::OllamaChat,
            "/api/generate" => Api::OllamaGenerate,
            _ => return None,
        })
    }
}

/// `body`, a request in `api`, split into its parts, in the order its
/// provider reads them, and so holds them in its cache: tools and
/// instructions, then the conversation.
pub fn census(api: Api, body: &Value) -> Vec<Part> {
    let mut parts = Parts::default();
    match api {
        Api::Anthropic => anthropic(body, &mut parts),
        Api::Chat | Api::OllamaChat => chat(body, &mut parts),
        Api::Responses => responses(body, &mut parts),
        Api::Completions => parts.text(USER, &body["prompt"]),
        Api::OllamaGenerate => {
            parts.text(INSTRUCTIONS, &body["system"]);
            parts.text(USER, &body["prompt"]);
            images(&body["images"], &mut parts);
        }
    }
    parts.parts
}

/// A fingerprint of `body`'s opening message, the first a person wrote:
/// what names an outside harness's chat, which says nothing else of it.
pub fn opening(api: Api, body: &Value) -> Option<u64> {
    let first = match api {
        Api::Anthropic | Api::Chat | Api::OllamaChat => body["messages"]
            .as_array()?
            .iter()
            .find(|m| m["role"] == "user")?
            .get("content")?,
        Api::Responses => match &body["input"] {
            text @ Value::String(_) => text,
            Value::Array(items) => items.iter().find(|i| i["role"] == "user")?.get("content")?,
            _ => return None,
        },
        Api::Completions | Api::OllamaGenerate => body.get("prompt")?,
    };
    Some(fingerprint(first.to_string().as_bytes()))
}

/// FNV-1a, 64 bits: the same on every machine and release.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

const INSTRUCTIONS: &str = "instructions";
const USER: &str = "conversation/user";
const ASSISTANT: &str = "conversation/assistant";
const THINKING: &str = "conversation/thinking";

fn said(role: &str) -> &'static str {
    match role {
        "system" | "developer" => INSTRUCTIONS,
        "assistant" | "model" => ASSISTANT,
        _ => USER,
    }
}

/// A path's last segment of a name the request gave: kept when it's a
/// plain name, else `other`.
fn named(kind: &str, name: Option<&str>) -> String {
    let plain = name.filter(|n| {
        (1..=NAME_MAX).contains(&n.len())
            && n.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
    });
    format!("{kind}/{}", plain.unwrap_or("other"))
}

/// The parts met so far, each path once.
#[derive(Default)]
struct Parts {
    parts: Vec<Part>,
}

impl Parts {
    fn add(&mut self, path: &str, bytes: usize, weight: usize) {
        if bytes == 0 && weight == 0 {
            return;
        }
        let (bytes, weight) = (bytes as u64, weight as u64);
        if let Some(part) = self.parts.iter_mut().find(|p| p.path == path) {
            part.bytes += bytes;
            part.weight += weight;
            return;
        }
        if self.parts.len() >= PARTS_MAX - 1 {
            // Past the most: its kind's other, the last kept.
            let other = match path.rsplit_once('/') {
                Some((kind, _)) if kind != "conversation" => format!("{kind}/other"),
                _ => "other".to_owned(),
            };
            if other != path {
                return self.add_other(other, bytes, weight);
            }
        }
        self.parts.push(Part {
            path: path.to_owned(),
            bytes,
            weight,
        });
    }

    fn add_other(&mut self, path: String, bytes: u64, weight: u64) {
        match self.parts.iter_mut().find(|p| p.path == path) {
            Some(part) => {
                part.bytes += bytes;
                part.weight += weight;
            }
            None => self.parts.push(Part {
                path,
                bytes,
                weight,
            }),
        }
    }

    /// Text, or a value whose JSON is sent as it is (a call's input).
    fn text(&mut self, path: &str, value: &Value) {
        match value {
            Value::Null => {}
            Value::String(text) => self.add(path, text.len(), text_tokens(text)),
            other => {
                let json = other.to_string();
                self.add(path, json.len(), text_tokens(&json));
            }
        }
    }

    /// An image, `data` bytes as sent, under `path`, else as media.
    fn image(&mut self, path: Option<&str>, data: usize) {
        let path = path.unwrap_or("conversation/media/image");
        self.add(path, data, IMAGE_TOKENS);
    }

    /// A document `data` bytes long as sent, under `path`, else as
    /// media: base64 for a PDF, which decodes to three in four.
    fn document(&mut self, path: Option<&str>, media: &str, data: usize) {
        let weight = match media.starts_with("text/") {
            true => data / 4 + 8,
            false => (data * 3 / 4 / 3).min(PDF_TOKENS_MAX),
        };
        let path = path.unwrap_or("conversation/media/document");
        self.add(path, data, weight);
    }
}

fn len(value: &Value) -> usize {
    value.as_str().map_or(0, str::len)
}

/// Anthropic's Messages: `tools`, `system`, and `messages` of blocks, in
/// the order Anthropic reads them.
fn anthropic(body: &Value, parts: &mut Parts) {
    tools(body, parts, |tool| tool["name"].as_str());
    match &body["system"] {
        Value::Array(blocks) => blocks
            .iter()
            .for_each(|b| parts.text(INSTRUCTIONS, &b["text"])),
        system => parts.text(INSTRUCTIONS, system),
    }
    let mut called = Vec::new();
    for message in body["messages"].as_array().into_iter().flatten() {
        let role = said(message["role"].as_str().unwrap_or_default());
        match &message["content"] {
            Value::Array(blocks) => {
                for block in blocks {
                    anthropic_block(block, role, None, &mut called, parts);
                }
            }
            text => parts.text(role, text),
        }
    }
}

/// A block of a message by `role`, or of a result `within` its path.
fn anthropic_block(
    block: &Value,
    role: &str,
    within: Option<&str>,
    called: &mut Vec<(String, String)>,
    parts: &mut Parts,
) {
    match block["type"].as_str().unwrap_or_default() {
        "text" => parts.text(role, &block["text"]),
        "thinking" => parts.text(THINKING, &block["thinking"]),
        "redacted_thinking" => parts.text(THINKING, &block["data"]),
        "tool_use" | "server_tool_use" => {
            let name = block["name"].as_str();
            if let Some(id) = block["id"].as_str() {
                called.push((id.to_owned(), name.unwrap_or_default().to_owned()));
            }
            parts.text(&named("conversation/calls", name), &block["input"]);
        }
        "tool_result" => {
            let path = result_path(called, block["tool_use_id"].as_str());
            match &block["content"] {
                Value::Array(inner) => {
                    for block in inner {
                        anthropic_block(block, &path, Some(&path), called, parts);
                    }
                }
                content => parts.text(&path, content),
            }
        }
        "image" => parts.image(within, len(&block["source"]["data"])),
        "document" => {
            let source = &block["source"];
            let media = source["media_type"].as_str().unwrap_or_default();
            parts.document(within, media, len(&source["data"]));
        }
        _ => parts.text(role, block),
    }
}

/// A result's path, by the call it answers.
fn result_path(called: &[(String, String)], id: Option<&str>) -> String {
    let name = id.and_then(|id| called.iter().rev().find(|(c, _)| c == id));
    named("conversation/results", name.map(|(_, n)| n.as_str()))
}

/// Each of `tools`, under its name.
fn tools(body: &Value, parts: &mut Parts, name: impl Fn(&Value) -> Option<&str>) {
    for tool in body["tools"].as_array().into_iter().flatten() {
        let json = tool.to_string();
        let name = name(tool).or_else(|| tool["type"].as_str());
        parts.add(&named("tools", name), json.len(), text_tokens(&json));
    }
}

/// OpenAI's Chat Completions and Ollama's chat: `messages` by role, a
/// tool's answer named by its call.
fn chat(body: &Value, parts: &mut Parts) {
    tools(body, parts, |tool| tool["function"]["name"].as_str());
    let mut called = Vec::new();
    for message in body["messages"].as_array().into_iter().flatten() {
        let role = message["role"].as_str().unwrap_or_default();
        if role == "tool" {
            let path = match message["tool_name"].as_str() {
                Some(name) => named("conversation/results", Some(name)),
                None => result_path(&called, message["tool_call_id"].as_str()),
            };
            chat_content(&message["content"], &path, true, parts);
            continue;
        }
        let role = said(role);
        chat_content(&message["content"], role, false, parts);
        for key in ["reasoning_content", "reasoning", "thinking"] {
            parts.text(THINKING, &message[key]);
        }
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            let name = call["function"]["name"].as_str();
            if let Some(id) = call["id"].as_str() {
                called.push((id.to_owned(), name.unwrap_or_default().to_owned()));
            }
            parts.text(
                &named("conversation/calls", name),
                &call["function"]["arguments"],
            );
        }
        images(&message["images"], parts);
    }
}

/// A Chat Completions message's content under `path`: text, or its
/// parts, media apart unless it's `within` a result.
fn chat_content(content: &Value, path: &str, within: bool, parts: &mut Parts) {
    let media = within.then_some(path);
    let Value::Array(pieces) = content else {
        return parts.text(path, content);
    };
    for piece in pieces {
        match piece["type"].as_str().unwrap_or_default() {
            "text" | "input_text" | "output_text" => parts.text(path, &piece["text"]),
            "image_url" => parts.image(media, len(&piece["image_url"]["url"])),
            "input_image" => parts.image(media, len(&piece["image_url"])),
            "file" | "input_file" => {
                let file = piece.get("file").unwrap_or(piece);
                parts.document(media, "application/pdf", len(&file["file_data"]));
            }
            "input_audio" => {
                let data = len(&piece["input_audio"]["data"]);
                parts.add(media.unwrap_or("conversation/media/audio"), data, 16);
            }
            _ => parts.text(path, piece),
        }
    }
}

/// Ollama's images: base64, each an image.
fn images(images: &Value, parts: &mut Parts) {
    for image in images.as_array().into_iter().flatten() {
        parts.image(None, len(image));
    }
}

/// OpenAI's Responses: `instructions`, `tools`, and `input` of items.
fn responses(body: &Value, parts: &mut Parts) {
    parts.text(INSTRUCTIONS, &body["instructions"]);
    tools(body, parts, |tool| tool["name"].as_str());
    let items = match &body["input"] {
        Value::Array(items) => items,
        input => return parts.text(USER, input),
    };
    let mut called = Vec::new();
    for item in items {
        match item["type"].as_str().unwrap_or("message") {
            "message" => {
                let role = said(item["role"].as_str().unwrap_or_default());
                chat_content(&item["content"], role, false, parts);
            }
            "function_call" | "custom_tool_call" => {
                let name = item["name"].as_str();
                if let Some(id) = item["call_id"].as_str() {
                    called.push((id.to_owned(), name.unwrap_or_default().to_owned()));
                }
                let input = item.get("arguments").unwrap_or(&item["input"]);
                parts.text(&named("conversation/calls", name), input);
            }
            "function_call_output" | "custom_tool_call_output" => {
                let path = result_path(&called, item["call_id"].as_str());
                chat_content(&item["output"], &path, true, parts);
            }
            "reasoning" => {
                for summary in item["summary"].as_array().into_iter().flatten() {
                    parts.text(THINKING, &summary["text"]);
                }
                parts.text(THINKING, &item["encrypted_content"]);
            }
            _ => parts.text(USER, item),
        }
    }
}
