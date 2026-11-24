//! The fixture model requests real protected tools and verifies their replies.

use axum::{Json, extract::State, response::Response};
use serde_json::{Value, json};

use crate::agent;

pub(super) async fn answer(
    State(asked): State<agent::Shared<Vec<Value>>>,
    Json(body): Json<Value>,
) -> Response {
    asked.lock().unwrap().push(body.clone());
    let messages = body["messages"].as_array().unwrap();
    let last = messages.iter().rposition(|m| m["role"] == "user").unwrap();
    let user = &messages[last]["content"];
    let text = user.as_str().map(str::to_string).unwrap_or_else(|| {
        user.as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ")
    });
    // Full Link adds the sender's identity before the original message.
    // The connection assertion names the controlled workload, not that wrapper.
    let text = text.find("runtime ").map(|at| &text[at..]).unwrap_or(&text);
    let replies: Vec<&Value> = messages[last..]
        .iter()
        .filter(|m| m["role"] == "tool")
        .collect();
    let calls = if text.contains("runtime protected") {
        vec![
            (
                "task",
                "run",
                json!({"command": "printf 'runtime\\n' > work/made"}),
            ),
            ("file", "read", json!({"file_path":"work/made"})),
            (
                "file",
                "write",
                json!({"file_path":"chats/forged", "content":"refused"}),
            ),
        ]
    } else if text.contains("runtime scheduler") {
        vec![("task", "list", json!({}))]
    } else {
        Vec::new()
    };
    let delta = match calls.get(replies.len()) {
        Some((tool, method, arguments)) => {
            let name = body["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["function"]["name"].as_str())
                .find(|n| n.contains(tool) && n.ends_with(method))
                .unwrap_or_else(|| {
                    let names: Vec<_> = body["tools"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|t| t["function"]["name"].as_str())
                        .collect();
                    panic!("the requested {tool}/{method} wasn't offered: {names:?}")
                });
            json!({"tool_calls":[{"index":0,"id":format!("call_{}", replies.len()),"type":"function",
                "function":{"name":name,"arguments":arguments.to_string()}}]})
        }
        None => {
            if text.contains("runtime protected") {
                assert!(
                    replies[0]["content"].as_str().unwrap().contains("exit 0"),
                    "{}",
                    replies[0]
                );
                assert!(
                    replies[1]["content"].as_str().unwrap().contains("runtime"),
                    "{}",
                    replies[1]
                );
                assert!(
                    replies[2]["content"]
                        .as_str()
                        .unwrap()
                        .contains("you may not write chats/"),
                    "{}",
                    replies[2]
                );
            }
            if text.contains("runtime scheduler") {
                assert!(replies[0]["content"].as_str().unwrap().contains("jobs"));
            }
            json!({"content":format!("completed {text}")})
        }
    };
    let chunk = json!({"id":"runtime", "choices":[{"index":0,"delta":delta}]});
    let stop = json!({"id":"runtime", "choices":[{"index":0,"delta":{},"finish_reason":if calls.len() > replies.len() { "tool_calls" } else { "stop" }}]});
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from(format!(
            "data: {chunk}\n\ndata: {stop}\n\ndata: [DONE]\n\n"
        )))
        .unwrap()
}
