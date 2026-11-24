//! The kit's own model and tool, and how their requests are read.

use super::*;

/// This machine's [`NETWORK_ENV`], as it's set.
pub(super) fn network_env() -> Vec<(String, String)> {
    NETWORK_ENV
        .iter()
        .filter_map(|var| Some((var.to_string(), std::env::var(var).ok()?)))
        .collect()
}

/// A message's text: its content, or its text parts.
fn text_of(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// What the kit sends a harness, each as its own message.
const ASKS: &[&str] = &["Hello", "Again", "Other", "After", SLOW_ASK, TOOL_ASK];

/// The turn a request to the model is for: the last of the kit's
/// messages in it, and the kit's tool's answer, when one came after it.
/// A harness may add messages of its own around them (instructions, the
/// date, what this machine can run), so neither is looked for last.
fn turn_of(body: &Value) -> (String, Option<String>) {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let ask = messages.iter().enumerate().rev().find_map(|(n, m)| {
        let text = text_of(m);
        let ask = ASKS.iter().find(|ask| text.contains(**ask))?;
        (m["role"] == "user").then_some((n, ask.to_string()))
    });
    let Some((n, ask)) = ask else {
        return (messages.last().map(text_of).unwrap_or_default(), None);
    };
    let tool = messages[n..]
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .map(text_of);
    (ask, tool)
}

/// The kit's message a request to the model is for.
pub(super) fn last_text(body: &Value) -> String {
    turn_of(body).0
}

/// The model's name for the kit's tool, in the tools a request offers:
/// whatever the harness puts before it, it ends with its call's name.
pub(super) fn tool_named(body: &Value) -> Option<String> {
    body["tools"].as_array()?.iter().find_map(|tool| {
        let name = tool["function"]["name"]
            .as_str()
            .or(tool["name"].as_str())?;
        (name.contains(TOOL) && name.ends_with(TOOL_CALL)).then(|| name.to_string())
    })
}

/// A bare chat completions model: answers with how many messages it was
/// sent and the kit's last one ([`turn_of`]), streamed in two pieces when asked to stream.
/// Asked [`TOOL_ASK`] and offered the kit's tool, it calls it; sent the
/// tool's answer, it answers with it. Asked [`SLOW_ASK`], it takes
/// [`SLOW`].
#[expect(clippy::disallowed_methods, reason = "a fake model slow on purpose")]
pub(super) async fn model(
    State(seen): State<Shared>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let messages = body["messages"].as_array().map_or(0, Vec::len);
    let stream = body["stream"] == true;
    let (text, answered) = turn_of(&body);
    let tool = tool_named(&body);
    let handoff = body.to_string();
    let continuation = text.contains(crate::agent_state::NEXT_ASK);
    seen.lock()
        .unwrap_or_else(|e| e.into_inner())
        .asked
        .push(body);
    if text == SLOW_ASK {
        tokio::time::sleep(SLOW).await;
    }
    let call = match (&answered, tool) {
        (None, Some(name)) if text == TOOL_ASK => Some(name),
        _ => None,
    };
    let answer = if continuation {
        if handoff.contains("green replaces blue")
            && handoff.contains("unfinished")
            && handoff.contains("invoice")
        {
            crate::agent_state::NEXT_ANSWER.to_string()
        } else {
            "Missing the delivery correction or unfinished invoice.".to_string()
        }
    } else {
        match &answered {
            Some(said) => format!("The tool said: {said}"),
            None => format!("I see {messages} messages; the last: {text}"),
        }
    };
    seen.lock()
        .unwrap_or_else(|e| e.into_inner())
        .answered
        .push(text);
    let arguments = json!({"note": "conformance"}).to_string();
    if !stream {
        let message = match &call {
            Some(name) => json!({"role": "assistant", "content": null, "tool_calls": [{
                "id": "call_kit_1", "type": "function",
                "function": {"name": name, "arguments": arguments}}]}),
            None => json!({"role": "assistant", "content": answer}),
        };
        let finish = if call.is_some() { "tool_calls" } else { "stop" };
        return Json(
            json!({"id": "chatcmpl-kit", "object": "chat.completion", "model": MODEL,
            "choices": [{"index": 0, "message": message, "finish_reason": finish}]}),
        )
        .into_response();
    }
    let chunk = |delta: Value, finish: Value| {
        let data = json!({"id": "chatcmpl-kit", "object": "chat.completion.chunk", "model": MODEL,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
        format!("data: {data}\n\n")
    };
    let body = match &call {
        Some(name) => {
            let (first, rest) = arguments.split_at(5);
            chunk(
                json!({"role": "assistant", "tool_calls": [{"index": 0, "id": "call_kit_1",
                    "type": "function", "function": {"name": name, "arguments": first}}]}),
                Value::Null,
            ) + &chunk(
                json!({"tool_calls": [{"index": 0, "function": {"arguments": rest}}]}),
                Value::Null,
            ) + &chunk(json!({}), json!("tool_calls"))
        }
        None => {
            let (first, rest) = answer.split_at(6);
            chunk(json!({"role": "assistant", "content": first}), Value::Null)
                + &chunk(json!({"content": rest}), Value::Null)
                + &chunk(json!({}), json!("stop"))
        }
    };
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from(body + "data: [DONE]\n\n"))
        .unwrap_or_default()
}

/// The kit's tool: an MCP server over HTTP, as Link's bridge runs a tool
/// for a harness, with one call, [`TOOL_CALL`], which answers
/// [`STAMP`].
pub(super) async fn tool(
    State(seen): State<Shared>,
    Json(message): Json<Value>,
) -> axum::response::Response {
    let Some(id) = message.get("id").cloned() else {
        // A notification: nothing to answer.
        return StatusCode::ACCEPTED.into_response();
    };
    let answer = |result: Value| json!({"jsonrpc": "2.0", "id": id, "result": result});
    let params = &message["params"];
    let body = match message["method"].as_str().unwrap_or_default() {
        "initialize" => answer(json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "link-conformance", "version": "1"},
        })),
        "tools/list" => answer(json!({"tools": [{
            "name": TOOL_CALL,
            "description": "Stamps a note, and answers with the stamp.",
            "inputSchema": {"type": "object", "properties": {"note": {"type": "string"}}},
        }]})),
        "tools/call" => {
            seen.lock()
                .unwrap_or_else(|e| e.into_inner())
                .called
                .push(params.clone());
            answer(json!({"content": [{"type": "text", "text": STAMP}]}))
        }
        "ping" => answer(json!({})),
        method => json!({"jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("no method {method}")}}),
    };
    ([("mcp-session-id", "conformance-session")], Json(body)).into_response()
}
