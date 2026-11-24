//! What's sent to the model: each provider's address and request body,
//! with the images and documents it takes.

use super::*;

/// `{base}/v1/messages`: Anthropic's clients add `/v1` to their base.
pub fn anthropic_url(model: &Model) -> String {
    let base = model
        .base_url
        .as_deref()
        .unwrap_or("https://api.anthropic.com")
        .trim_end_matches('/');
    format!("{base}/v1/messages")
}

/// `{base}/chat/completions`: a hosted provider's base holds its `/v1`
/// (the proxy's too), a runtime's here doesn't.
pub fn openai_url(model: &Model) -> anyhow::Result<String> {
    Ok(format!("{}/chat/completions", openai_base(model)?))
}

/// Where the provider lists its models, `/v1/models` on every one, as
/// Link's proxy passes it: Anthropic's a thousand at once, its most.
pub fn models_url(model: &Model) -> anyhow::Result<String> {
    Ok(match model.provider.as_str() {
        "anthropic" => anthropic_url(model).replace("/v1/messages", "/v1/models?limit=1000"),
        _ => format!("{}/models", openai_base(model)?),
    })
}

/// A provider's address up to and with its `/v1`.
pub fn openai_base(model: &Model) -> anyhow::Result<String> {
    Ok(match (&model.base_url, model.provider.as_str()) {
        (Some(base), "ollama" | "local") => format!("{}/v1", base.trim_end_matches('/')),
        (Some(base), _) => base.trim_end_matches('/').to_string(),
        (None, "openai") => "https://api.openai.com/v1".into(),
        (None, "openrouter") => "https://openrouter.ai/api/v1".into(),
        (None, other) => match crate::models::providers::declared(other) {
            Some(declared) => declared.base_url.clone(),
            None => bail!("no address for the model's provider, {other}"),
        },
    })
}

/// The system prompt and the last message marked for the cache: the
/// next turn reads everything before its own message from it. The tools
/// come before the system prompt, so its mark covers them too.
pub fn anthropic_body(name: &str, prompt: &Prompt) -> Value {
    let last = prompt.messages.len().saturating_sub(1);
    let messages: Vec<Value> = prompt
        .messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut blocks: Vec<Value> = m
                .answers
                .iter()
                .map(|a| {
                    let content: Vec<Value> =
                        [json!({"type": "text", "text": or_nothing(&a.text)})]
                            .into_iter()
                            .chain(a.parts.iter().map(anthropic_part))
                            .collect();
                    json!({"type": "tool_result", "tool_use_id": a.call, "is_error": a.failed,
                        "content": content})
                })
                .collect();
            blocks.extend(m.thinking.iter().cloned());
            // Its images and documents before its text.
            blocks.extend(m.parts.iter().map(anthropic_part));
            if !m.text.is_empty()
                || (m.answers.is_empty() && m.calls.is_empty() && m.parts.is_empty())
            {
                blocks.push(json!({"type": "text", "text": or_nothing(&m.text)}));
            }
            blocks.extend(m.calls.iter().map(|c| {
                let input = match &c.arguments {
                    Value::Object(_) => c.arguments.clone(),
                    _ => json!({}),
                };
                json!({"type": "tool_use", "id": c.id, "name": c.name, "input": input})
            }));
            if i == last
                && let Some(block) = blocks.last_mut()
            {
                block["cache_control"] = cache_control(prompt.cache);
            }
            json!({"role": m.role, "content": blocks})
        })
        .collect();
    let mut body = json!({
        "model": name,
        "max_tokens": prompt.answer,
        "stream": true,
        "messages": messages,
    });
    if !prompt.system.is_empty() {
        body["system"] = json!([{"type": "text", "text": prompt.system,
            "cache_control": cache_control(prompt.cache)}]);
    }
    if !prompt.tools.is_empty() {
        body["tools"] = prompt
            .tools
            .iter()
            .map(
                |t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}),
            )
            .collect();
        if prompt.no_calls {
            body["tool_choice"] = json!({"type": "none"});
        }
    }
    let think = prompt.thinking;
    match (think.thinks, think.level) {
        // Off, as far as the model can be: omitted, at the least effort
        // for one that thinks whatever it's told.
        (Thinks::Adaptive, Thinking::Off) => body["output_config"] = json!({"effort": "low"}),
        (Thinks::Adaptive, level) => {
            body["thinking"] = json!({"type": "adaptive"});
            body["output_config"] = json!({"effort": level.word()});
        }
        // Its budget under the answer's, as the provider requires.
        (Thinks::Budget, level) if level != Thinking::Off => {
            let budget = think.budget.min(prompt.answer.saturating_sub(1));
            if budget >= 1024 {
                body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
            }
        }
        _ => {}
    }
    if prompt.fast && super::super::offers_fast("anthropic", name) {
        body["speed"] = json!("fast");
    }
    body
}

/// A part as Anthropic takes it: an image or a PDF a block of its own, in
/// base64; a text document its text, under its name; an image larger
/// than Anthropic takes, a line saying so.
/// Anthropic's mark that the prompt up to it is cached: kept as long as
/// `cache` says (`"1h"`, its `ttl`), else as long as Anthropic keeps one
/// unasked.
fn cache_control(cache: Option<&str>) -> Value {
    match cache {
        Some(ttl) => json!({"type": "ephemeral", "ttl": ttl}),
        None => json!({"type": "ephemeral"}),
    }
}

fn anthropic_part(part: &Media) -> Value {
    if part.is_text() {
        return json!({"type": "text", "text": text_document(part)});
    }
    let data = part.encoded();
    match part.kind {
        Kind::Image if data.len() > ANTHROPIC_IMAGE_MAX => json!({"type": "text", "text":
            format!("{} was left out: Anthropic takes images of at most {} MiB in base64",
                part.named(), ANTHROPIC_IMAGE_MAX >> 20)}),
        Kind::Image => json!({"type": "image",
            "source": {"type": "base64", "media_type": part.media, "data": data}}),
        Kind::Document => json!({"type": "document",
            "source": {"type": "base64", "media_type": part.media, "data": data}}),
        Kind::Voice => json!({"type": "text", "text": heard(part)}),
    }
}

/// A voice note, which no model is sent: its words are its message's.
fn heard(part: &Media) -> String {
    format!("{}: heard, its words in the message", part.named())
}

/// A text document as every model reads it: its text, under its name.
fn text_document(part: &Media) -> String {
    format!("{}\n{}", part.named(), String::from_utf8_lossy(&part.bytes))
}

/// A part in an OpenAI-shaped request: an image as an `image_url`, a PDF
/// as a `file`, each a `data:` URL, to a provider that `takes` them
/// (OpenAI, OpenRouter); a text document as its text, to any; else a
/// line saying it was left out.
fn openai_part(part: &Media, takes: bool) -> Value {
    match (part.kind, takes) {
        _ if part.is_text() => json!({"type": "text", "text": text_document(part)}),
        (Kind::Voice, _) => json!({"type": "text", "text": heard(part)}),
        (Kind::Image, true) => json!({"type": "image_url", "image_url": {"url": part.data_url()}}),
        (Kind::Document, true) => json!({"type": "file", "file": {
            "filename": part.name.clone().unwrap_or_else(|| format!("document.{}", part.extension())),
            "file_data": part.data_url()}}),
        (kind, false) => json!({"type": "text", "text": format!(
            "{} was left out: this model takes no {}s through Link yet",
            part.named(), kind.word())}),
    }
}

/// Text a provider takes: never empty.
fn or_nothing(text: &str) -> &str {
    match text.is_empty() {
        true => "(nothing)",
        false => text,
    }
}

pub fn openai_body(name: &str, provider: &str, prompt: &Prompt) -> Value {
    // Only these take images and documents through Link.
    let takes = matches!(provider, "openai" | "openrouter");
    let mut all = Vec::new();
    if !prompt.system.is_empty() {
        all.push(json!({"role": "system", "content": prompt.system}));
    }
    for m in prompt.messages {
        for a in &m.answers {
            let text = match a.failed {
                true => format!("It failed: {}", a.text),
                false => a.text.clone(),
            };
            all.push(json!({"role": "tool", "tool_call_id": a.call, "content": text}));
        }
        if m.calls.is_empty() {
            // A tool's message takes text only: its answer's images and
            // documents go in the message after, before its own.
            let parts: Vec<Value> = m
                .answers
                .iter()
                .flat_map(|a| &a.parts)
                .chain(&m.parts)
                .map(|part| openai_part(part, takes))
                .collect();
            if !parts.is_empty() {
                all.push(json!({"role": m.role, "content": openai_content(parts, &m.text, takes)}));
            } else if !m.text.is_empty() || m.answers.is_empty() {
                all.push(json!({"role": m.role, "content": m.text}));
            }
            continue;
        }
        let calls: Vec<Value> = m
            .calls
            .iter()
            .map(|c| {
                let arguments = match &c.arguments {
                    Value::Object(_) => c.arguments.to_string(),
                    _ => "{}".into(),
                };
                json!({"id": c.id, "type": "function",
                    "function": {"name": c.name, "arguments": arguments}})
            })
            .collect();
        let text = match m.text.is_empty() {
            true => Value::Null,
            false => json!(m.text),
        };
        all.push(json!({"role": m.role, "content": text, "tool_calls": calls}));
    }
    let mut body = json!({
        "model": name,
        "messages": all,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if !prompt.tools.is_empty() {
        body["tools"] = prompt
            .tools
            .iter()
            .map(|t| {
                json!({"type": "function", "function": {"name": t.name,
                    "description": t.description, "parameters": t.schema}})
            })
            .collect();
        if prompt.no_calls {
            body["tool_choice"] = json!("none");
        }
    }
    // The longest answer, by the name each provider gives it.
    if prompt.answer > 0 {
        match provider {
            "openai" => body["max_completion_tokens"] = json!(prompt.answer),
            _ => body["max_tokens"] = json!(prompt.answer),
        }
    }
    if provider == "openrouter" {
        // Its cost in the usage.
        body["usage"] = json!({"include": true});
    }
    if prompt.thinking.thinks == Thinks::Effort {
        let effort = match prompt.thinking.level {
            Thinking::Off => "minimal",
            level => level.word(),
        };
        match provider {
            "openrouter" => body["reasoning"] = json!({"effort": effort}),
            _ => body["reasoning_effort"] = json!(effort),
        }
    }
    if prompt.fast && super::super::offers_fast(provider, name) {
        body["service_tier"] = json!("priority");
    }
    body
}

/// A message's `parts`, then its `text`: a list of them for a provider
/// that `takes` images and documents, else all as text, which any
/// runtime takes.
fn openai_content(parts: Vec<Value>, text: &str, takes: bool) -> Value {
    let text = (!text.is_empty()).then(|| json!({"type": "text", "text": text}));
    let all = parts.into_iter().chain(text);
    match takes {
        true => Value::Array(all.collect()),
        false => json!(
            all.filter_map(|part| part["text"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
                .join("\n\n")
        ),
    }
}
