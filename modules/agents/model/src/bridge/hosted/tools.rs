//! A provider's own tools, run on its servers: removed from a request
//! unless the policy allows them, and a URL it would fetch refused.

use super::*;

/// The tool types a provider runs on the program's side: kept. Any other
/// is the provider's own, run on its servers: removed.
fn client_tool(provider: &str, tool: &Value) -> bool {
    let kind = tool.get("type").and_then(Value::as_str);
    match provider {
        "anthropic" => match kind {
            None | Some("custom") => true,
            Some(kind) => ["bash_", "text_editor_", "computer_", "memory_"]
                .iter()
                .any(|p| kind.starts_with(p)),
        },
        _ => match kind {
            Some(
                "function" | "custom" | "local_shell" | "apply_patch" | "computer_use_preview",
            ) => true,
            // A shell the program runs, not one in the provider's container.
            Some("shell") => tool
                .pointer("/environment/type")
                .and_then(Value::as_str)
                .is_none_or(|env| env == "local"),
            _ => false,
        },
    }
}

/// Removes from a request what makes the provider reach out for the
/// program: its server-side tools (and a `tool_choice` naming one), MCP
/// servers, containers, OpenRouter's web modules and `:online` models.
pub fn strip_server_tools(provider: &str, request: &mut Value) {
    let Some(object) = request.as_object_mut() else {
        return;
    };
    let mut removed = Vec::new();
    if let Some(Value::Array(tools)) = object.get_mut("tools") {
        tools.retain(|tool| {
            let keep = client_tool(provider, tool);
            if !keep {
                removed.push(tool.clone());
            }
            keep
        });
        if tools.is_empty() {
            object.remove("tools");
        }
    }
    if let Some(choice) = object.get("tool_choice").filter(|c| c.is_object()) {
        let kind = choice.get("type").and_then(Value::as_str);
        let named = choice
            .get("name")
            .or_else(|| choice.pointer("/function/name"));
        let names_removed = named.is_some_and(|name| {
            removed.iter().any(|tool| {
                tool.get("name") == Some(name) || tool.pointer("/function/name") == Some(name)
            })
        });
        let a_server_tool = !matches!(
            kind,
            None | Some("auto" | "any" | "none" | "required" | "tool" | "function" | "custom")
                | Some("allowed_tools")
        ) && !client_tool(provider, choice);
        if names_removed || a_server_tool || !object.contains_key("tools") {
            object.remove("tool_choice");
        }
    }
    for field in ["mcp_servers", "container", "web_search_options", "modules"] {
        object.remove(field);
    }
    if provider == "openrouter" {
        // `:online` on the model, or on any of its fallbacks.
        let offline = |model: &mut Value| {
            if let Value::String(name) = model
                && let Some(base) = name.strip_suffix(":online")
            {
                *name = base.to_string();
            }
        };
        if let Some(model) = object.get_mut("model") {
            offline(model);
        }
        if let Some(Value::Array(models)) = object.get_mut("models") {
            models.iter_mut().for_each(offline);
        }
    }
}

/// Why a request's model can't be taken with server-side tools off: an
/// OpenRouter preset (`@preset/...`), which can bring its own modules.
pub(super) fn preset(provider: &str, request: &Value) -> Option<String> {
    if provider != "openrouter" {
        return None;
    }
    let models = request.get("models").and_then(Value::as_array);
    std::iter::once(request.get("model"))
        .chain(models.into_iter().flatten().map(Some))
        .flatten()
        .filter_map(Value::as_str)
        .find(|model| model.trim_start().starts_with('@'))
        .map(|model| model.chars().take(80).collect())
}

/// A request's model, or one of its fallbacks, that searches the web
/// whatever the request asks ([`link_model_contract::route::searches_by_nature`]).
pub(super) fn searching(provider: &str, request: &Value) -> Option<String> {
    let models = request.get("models").and_then(Value::as_array);
    std::iter::once(request.get("model"))
        .chain(models.into_iter().flatten().map(Some))
        .flatten()
        .filter_map(Value::as_str)
        .find(|model| link_model_contract::route::searches_by_nature(provider, model))
        .map(|model| model.chars().take(80).collect())
}

/// The keys whose value, a web address, the provider fetches: an image's,
/// a file's, a document's. Any key ending in `url` or `_data` is one too
/// ([`fetches`]): OpenRouter fetches a PDF's `file_data`.
const URL_KEYS: &[&str] = &[
    "url",
    "image_url",
    "file_url",
    "file_data",
    "document_url",
    "audio_url",
    "video_url",
];

/// The blocks that are a tool's call, whose `input` is what the tool was
/// given as data, never something the provider fetches.
const TOOL_CALLS: &[&str] = &[
    "tool_use",
    "server_tool_use",
    "mcp_tool_use",
    "function_call",
    "custom_tool_call",
];

/// Whether the provider reads a web address under key `name` as one to
/// fetch: one of [`URL_KEYS`], or any key ending in `url` or `_data`, in
/// any case (`imageUrl`).
fn fetches(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    URL_KEYS.contains(&name.as_str()) || name.ends_with("url") || name.ends_with("_data")
}

/// A URL the request asks the provider to fetch (an image, a document,
/// a file), if any: a web address under a key that [`fetches`], in any
/// block, whatever its type (`HTTPS://` too), but never in a tool's
/// call's input ([`TOOL_CALLS`]), and never the request's own `input`,
/// whatever `type` it has beside it.
pub(super) fn fetched_url(request: &Value) -> Option<String> {
    fetched_in(request, true)
}

/// [`fetched_url`] in `value`, the request itself if `top`.
fn fetched_in(value: &Value, top: bool) -> Option<String> {
    let web = |url: &str| {
        let url = url.trim_start().to_ascii_lowercase();
        url.starts_with("http://") || url.starts_with("https://")
    };
    match value {
        Value::Object(object) => {
            let tool_call = !top
                && object
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| TOOL_CALLS.contains(&kind));
            object
                .iter()
                // Not into what a tool was given as data.
                .filter(|(name, _)| !(tool_call && name.as_str() == "input"))
                .find_map(|(name, value)| match value.as_str() {
                    Some(url) if fetches(name) && web(url) => Some(url.chars().take(200).collect()),
                    _ => fetched_in(value, false),
                })
        }
        Value::Array(items) => items.iter().find_map(|item| fetched_in(item, false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{key, sent, wired};
    use super::*;
    use serde_json::json;

    #[test]
    fn server_side_tools_are_removed() {
        let mut request = json!({
            "model": "claude-opus-5-5",
            "tools": [
                {"name": "read_file", "input_schema": {}},
                {"type": "custom", "name": "run", "input_schema": {}},
                {"type": "bash_20250124", "name": "bash"},
                {"type": "text_editor_20250728", "name": "str_replace_based_edit_tool"},
                {"type": "web_search_20260209", "name": "web_search"},
                {"type": "web_fetch_20260209", "name": "web_fetch"},
                {"type": "code_execution_20260521", "name": "code_execution"},
                {"type": "mcp_toolset", "mcp_server_name": "x"},
            ],
            "tool_choice": {"type": "tool", "name": "web_search"},
            "mcp_servers": [{"type": "url", "url": "https://example.com/mcp"}],
            "container": {"skills": []},
        });
        strip_server_tools("anthropic", &mut request);
        let names: Vec<_> = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["read_file", "run", "bash", "str_replace_based_edit_tool"]
        );
        for gone in ["tool_choice", "mcp_servers", "container"] {
            assert!(request.get(gone).is_none(), "{gone}");
        }

        let mut request = json!({
            "model": "gpt-6.1-sol",
            "tools": [
                {"type": "function", "name": "read_file"},
                {"type": "web_search"},
                {"type": "file_search"},
                {"type": "code_interpreter", "container": {"type": "auto"}},
                {"type": "mcp", "server_url": "https://example.com"},
                {"type": "image_generation"},
                {"type": "shell", "environment": {"type": "container_auto"}},
                {"type": "shell"},
            ],
            "tool_choice": {"type": "web_search"},
        });
        strip_server_tools("openai", &mut request);
        assert_eq!(
            request["tools"],
            json!([{"type": "function", "name": "read_file"}, {"type": "shell"}])
        );
        assert!(request.get("tool_choice").is_none());

        let mut request = json!({
            "model": "anthropic/claude-opus-5.5:online",
            "models": ["openai/gpt-6:online", "x/y"],
            "modules": [{"id": "web"}],
            "web_search_options": {},
            "tools": [{"type": "openrouter:web_search"}],
            "tool_choice": "auto",
        });
        strip_server_tools("openrouter", &mut request);
        assert_eq!(
            request,
            json!({"model": "anthropic/claude-opus-5.5", "models": ["openai/gpt-6", "x/y"],
                "tool_choice": "auto"})
        );

        // A function the program runs, chosen by name, stays chosen.
        let mut request = json!({
            "tools": [{"type": "function", "function": {"name": "f"}}],
            "tool_choice": {"type": "function", "function": {"name": "f"}},
        });
        let before = request.clone();
        strip_server_tools("openai", &mut request);
        assert_eq!(request, before);
    }

    #[test]
    fn a_url_the_provider_would_fetch_is_refused() {
        let head = b"POST /_link/model/v1/messages HTTP/1.1\r\nContent-Length: 1\r\n\r\n";
        for body in [
            json!({"messages": [{"role": "user", "content": [
                {"type": "image", "source": {"type": "url", "url": "https://x.example/?secret"}}
            ]}]}),
            json!({"messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "http://x.example/a.png"}}
            ]}]}),
            json!({"input": [{"role": "user", "content": [
                {"type": "input_file", "file_url": "https://x.example/a.pdf"}
            ]}]}),
            // In capitals, which a URL's parser reads the same.
            json!({"messages": [{"role": "user", "content": [
                {"type": "image", "source": {"type": "url", "url": " HTTPS://x.example/?secret"}}
            ]}]}),
            // In a block the proxy doesn't know by name.
            json!({"input": [{"type": "computer_call_output",
                "output": {"type": "computer_screenshot", "image_url": "https://x.example/"}}]}),
            // A PDF OpenRouter fetches, and any key naming a URL or data.
            json!({"messages": [{"role": "user", "content": [
                {"type": "file", "file": {"filename": "a.pdf", "file_data": "https://x.example/a.pdf"}}
            ]}]}),
            json!({"messages": [{"content": [{"type": "x", "imageUrl": "https://x.example/"}]}]}),
            json!({"messages": [{"content": [{"type": "x", "audio_data": "http://x.example/"}]}]}),
            // A `type` beside the request's own input, or in a block that
            // isn't a tool's call, hides nothing.
            json!({"type": "x", "input": [{"role": "user", "content": [
                {"type": "input_image", "image_url": "https://x.example/?secret"}
            ]}]}),
            json!({"input": [{"type": "message", "input": {"url": "https://x.example/"}}]}),
            json!({"type": "tool_use", "input": [{"role": "user", "content": [
                {"type": "input_image", "image_url": "https://x.example/?secret"}
            ]}]}),
        ] {
            let body = serde_json::to_vec(&body).unwrap();
            let err = prepare(&key("anthropic"), &wired(head), &body, false).unwrap_err();
            assert!(err.contains("would fetch"), "{err}");
        }
        // An OpenRouter preset can bring its own modules.
        let chat = b"POST /_link/model/v1/chat/completions HTTP/1.1\r\nContent-Length: 1\r\n\r\n";
        for body in [
            json!({"model": "@preset/research"}),
            json!({"model": "x/y", "models": ["@preset/research"]}),
        ] {
            let body = serde_json::to_vec(&body).unwrap();
            let err = prepare(&key("openrouter"), &wired(chat), &body, false).unwrap_err();
            assert!(err.contains("preset"), "{err}");
        }
        // A model that searches by nature, for a sandbox that lists its
        // hosts; any other sandbox reaches the web itself anyway.
        for body in [
            json!({"model": "perplexity/sonar-pro"}),
            json!({"model": "x/y", "models": ["openai/gpt-4o-search-preview"]}),
        ] {
            let body = serde_json::to_vec(&body).unwrap();
            let err = prepare(&key("openrouter"), &wired(chat), &body, true).unwrap_err();
            assert!(err.contains("searches the web"), "{err}");
            assert!(prepare(&key("openrouter"), &wired(chat), &body, false).is_ok());
        }
        let body = serde_json::to_vec(&json!({"model": "openai/gpt-6.1-sol"})).unwrap();
        assert!(prepare(&key("openrouter"), &wired(chat), &body, true).is_ok());
        // A tool's input isn't fetched.
        for block in [
            json!({"type": "tool_use", "id": "t", "name": "fetch", "input": {"url": "https://x.example"}}),
            json!({"type": "function_call", "name": "fetch", "input": {"file_url": "https://x.example"}}),
        ] {
            let body = serde_json::to_vec(
                &json!({"messages": [{"role": "assistant", "content": [block]}]}),
            )
            .unwrap();
            assert!(prepare(&key("anthropic"), &wired(head), &body, false).is_ok());
        }
        // Inline data isn't fetched.
        let body = serde_json::to_vec(&json!({"messages": [{"content": [
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
        ]}]}))
        .unwrap();
        let chat = b"POST /_link/model/v1/chat/completions HTTP/1.1\r\nContent-Length: 1\r\n\r\n";
        assert!(prepare(&key("openai"), &wired(chat), &body, false).is_ok());
        // Allowed by the policy: passed as sent.
        let body = serde_json::to_vec(&json!({"tools": [{"type": "web_search"}], "x": {
            "source": {"type": "url", "url": "https://x.example"}}}))
        .unwrap();
        let allowing = ModelKey {
            server_tools: true,
            ..key("anthropic")
        };
        let prepared = prepare(&allowing, &wired(head), &body, false).unwrap();
        assert!(sent(&prepared).ends_with(std::str::from_utf8(&body).unwrap()));
    }
}
