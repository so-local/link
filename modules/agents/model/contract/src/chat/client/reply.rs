//! An answer as it streams in, taken in one event at a time.

use super::*;

/// An answer as it streams in.
#[derive(Default)]
pub struct Reply {
    pub text: String,
    pub id: Option<String>,
    pub model: Option<String>,
    pub tokens: Tokens,
    pub cost: Option<f64>,
    /// The calls as they stream in, by the provider's index: an id, a
    /// name, and the arguments' JSON so far.
    calls: std::collections::BTreeMap<u64, (String, String, String)>,
    /// Its thinking blocks as they stream in, by the provider's index.
    pub thinking: std::collections::BTreeMap<u64, Value>,
}

impl Reply {
    /// Takes in one streamed event; the text it adds, if any.
    pub fn take(&mut self, event: &Value, anthropic: bool) -> anyhow::Result<Option<String>> {
        if let Some(why) = event["error"]["message"].as_str() {
            bail!("the model failed: {}", link_format::clean(why));
        }
        let piece = if anthropic {
            match event["type"].as_str() {
                Some("message_start") => {
                    let message = &event["message"];
                    self.id = message["id"].as_str().map(str::to_string);
                    self.model = message["model"].as_str().map(str::to_string);
                    self.anthropic_usage(&message["usage"]);
                    None
                }
                Some("message_delta") => {
                    self.anthropic_usage(&event["usage"]);
                    None
                }
                Some("content_block_start")
                    if matches!(
                        event["content_block"]["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    ) =>
                {
                    let index = event["index"].as_u64().unwrap_or(0);
                    self.thinking.insert(index, event["content_block"].clone());
                    None
                }
                Some("content_block_start") if event["content_block"]["type"] == "tool_use" => {
                    let block = &event["content_block"];
                    let call = self
                        .calls
                        .entry(event["index"].as_u64().unwrap_or(0))
                        .or_default();
                    call.0 = block["id"].as_str().unwrap_or_default().to_string();
                    call.1 = block["name"].as_str().unwrap_or_default().to_string();
                    None
                }
                Some("content_block_delta")
                    if matches!(
                        event["delta"]["type"].as_str(),
                        Some("thinking_delta" | "signature_delta")
                    ) =>
                {
                    let index = event["index"].as_u64().unwrap_or(0);
                    let block = self
                        .thinking
                        .entry(index)
                        .or_insert_with(|| json!({"type": "thinking", "thinking": ""}));
                    let delta = &event["delta"];
                    if let Some(more) = delta["thinking"].as_str() {
                        let so_far = block["thinking"].as_str().unwrap_or_default();
                        block["thinking"] = json!(format!("{so_far}{more}"));
                    }
                    if let Some(signature) = delta["signature"].as_str() {
                        block["signature"] = json!(signature);
                    }
                    None
                }
                Some("content_block_delta") => {
                    if let Some(json) = event["delta"]["partial_json"].as_str() {
                        let index = event["index"].as_u64().unwrap_or(0);
                        self.calls.entry(index).or_default().2.push_str(json);
                    }
                    event["delta"]["text"].as_str()
                }
                _ => None,
            }
        } else {
            if self.id.is_none() {
                self.id = event["id"].as_str().map(str::to_string);
                self.model = event["model"].as_str().map(str::to_string);
            }
            self.openai_usage(&event["usage"]);
            let delta = &event["choices"][0]["delta"];
            for (n, part) in delta["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let index = part["index"].as_u64().unwrap_or(n as u64);
                let call = self.calls.entry(index).or_default();
                if let Some(id) = part["id"].as_str() {
                    call.0 = id.to_string();
                }
                if let Some(name) = part["function"]["name"].as_str() {
                    call.1.push_str(name);
                }
                if let Some(arguments) = part["function"]["arguments"].as_str() {
                    call.2.push_str(arguments);
                }
            }
            delta["content"].as_str()
        };
        Ok(piece.filter(|p| !p.is_empty()).map(|piece| {
            self.text.push_str(piece);
            piece.to_string()
        }))
    }

    /// Takes in a whole answer, not streamed.
    pub fn whole(&mut self, body: &Value, anthropic: bool) {
        self.id = body["id"].as_str().map(str::to_string);
        self.model = body["model"].as_str().map(str::to_string);
        if anthropic {
            let blocks = body["content"].as_array().into_iter().flatten();
            for (n, block) in blocks.enumerate() {
                match block["type"].as_str() {
                    Some("thinking" | "redacted_thinking") => {
                        self.thinking.insert(n as u64, block.clone());
                    }
                    Some("tool_use") => {
                        let id = block["id"].as_str().unwrap_or_default().to_string();
                        let name = block["name"].as_str().unwrap_or_default().to_string();
                        self.calls
                            .insert(n as u64, (id, name, block["input"].to_string()));
                    }
                    _ => self
                        .text
                        .push_str(block["text"].as_str().unwrap_or_default()),
                }
            }
            self.anthropic_usage(&body["usage"]);
        } else {
            let message = &body["choices"][0]["message"];
            self.text = message["content"].as_str().unwrap_or_default().to_string();
            let calls = message["tool_calls"].as_array().into_iter().flatten();
            for (n, call) in calls.enumerate() {
                let id = call["id"].as_str().unwrap_or_default().to_string();
                let function = &call["function"];
                let name = function["name"].as_str().unwrap_or_default().to_string();
                let arguments = match &function["arguments"] {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                self.calls.insert(n as u64, (id, name, arguments));
            }
            self.openai_usage(&body["usage"]);
        }
    }

    /// The calls it made, their arguments read: an object, `{}` for
    /// none, else what was sent, as a string, which the call is refused
    /// for.
    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .values()
            .filter(|(_, name, _)| !name.is_empty())
            .map(|(id, name, arguments)| Call {
                id: id.clone(),
                name: name.clone(),
                arguments: match arguments.trim() {
                    "" => json!({}),
                    text => serde_json::from_str(text).unwrap_or_else(|_| json!(text)),
                },
            })
            .collect()
    }

    /// Anthropic counts cached tokens apart from `input_tokens`.
    pub fn anthropic_usage(&mut self, usage: &Value) {
        let count = |key: &str| usage[key].as_u64();
        if let Some(input) = count("input_tokens") {
            let read = count("cache_read_input_tokens").unwrap_or(0);
            let written = count("cache_creation_input_tokens").unwrap_or(0);
            self.tokens.input = input.saturating_add(read).saturating_add(written);
            self.tokens.cached = read;
        }
        if let Some(output) = count("output_tokens") {
            self.tokens.output = output;
        }
    }

    fn openai_usage(&mut self, usage: &Value) {
        if let Some(input) = usage["prompt_tokens"].as_u64() {
            self.tokens.input = input;
            self.tokens.cached = usage["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0);
        }
        if let Some(output) = usage["completion_tokens"].as_u64() {
            self.tokens.output = output;
        }
        if let Some(cost) = usage["cost"].as_f64() {
            self.cost = Some(cost);
        }
    }
}
