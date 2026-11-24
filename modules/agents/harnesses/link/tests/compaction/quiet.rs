//! A quiet conversation's compaction (`[connections] compact_after`):
//! kept with the agent's scheduler through its `cron` namespace after each
//! turn, and fired by it. A fake namespace (MCP over HTTP) keeps the calls
//! it's made; its firing is a message from the conversation's own
//! compaction job.

use super::*;
use link_harness::event::Event;
use link_scheduler_tool_contract::compaction;

/// A `cron` namespace offering `list` and never its `compact`, as
/// lnk-sandbox lists a harness's own call to no model: the calls it was
/// made.
async fn fake_scheduler() -> (String, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let app = Router::new()
        .route(
            "/_link/mcp/namespace-cron-main",
            post(
                |State(calls): State<Arc<Mutex<Vec<Value>>>>,
                 headers: HeaderMap,
                 Json(message): Json<Value>| async move {
                    let method = message["method"].as_str().unwrap_or_default();
                    if method == "initialize" {
                        let answer = json!({"jsonrpc": "2.0", "id": message["id"],
                            "result": {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                            "serverInfo": {"name": "fake", "version": "1"}}});
                        return ([("mcp-session-id", "s1")], Json(answer)).into_response();
                    }
                    if headers.get("mcp-session-id").is_none() {
                        return (axum::http::StatusCode::BAD_REQUEST, "no session").into_response();
                    }
                    if message.get("id").is_none() {
                        return axum::http::StatusCode::ACCEPTED.into_response();
                    }
                    let result = match method {
                        "tools/list" => json!({"tools": [{"name": "list",
                            "description": "List your jobs.",
                            "inputSchema": {"type": "object"}}]}),
                        "tools/call" => {
                            calls.lock().unwrap().push(message["params"].clone());
                            json!({"content": [{"type": "text", "text": "{}"}]})
                        }
                        _ => json!({}),
                    };
                    Json(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
                        .into_response()
                },
            ),
        )
        .with_state(calls.clone());
    let port = common::serve(app).await;
    (
        format!("http://127.0.0.1:{port}/_link/mcp/namespace-cron-main"),
        calls,
    )
}

impl World {
    /// `thread`'s compaction firing, as its scheduler sends it.
    async fn fire_quiet(&self, thread: &Thread) {
        let firing = Event {
            from: Some(compaction::from(thread.name())),
            ..Event::user(&compaction::compaction().text)
        };
        let (pieces, mut heard) = tokio::sync::mpsc::channel(8);
        self.harness
            .ask(&self.agent, thread, firing, pieces, None)
            .await
            .unwrap();
        assert!(heard.try_recv().is_err(), "nobody is told");
    }
}

/// Each turn keeps its conversation's compaction with the scheduler, due
/// the spec's 55 minutes on, moved by the next. Fired, what's before the
/// owner's last message leaves for one summary, asked as the last
/// answer's prompt up to that message, so the provider's cache holds;
/// the firing is kept nowhere and told nobody. Fired again with nothing
/// new, nothing is asked; the next message's window starts with it.
#[tokio::test]
async fn a_quiet_conversation_compacts_before_its_last_message() {
    let (scheduler, calls) = fake_scheduler().await;
    let own = "[window]\nsave_notes = false\n[connections]\ncompact_after = \"55m\"\n";
    let env = vec![("LNK_TOOL_CRON".into(), scheduler)];
    let world = world("quiet", own, env, 100_000).await;
    let thread = Thread::new("telegram:42").unwrap();
    world.say(&thread, 1, 3).await;

    let kept = calls.lock().unwrap().clone();
    assert_eq!(kept.len(), 3, "{kept:?}");
    for call in &kept {
        assert_eq!(call["name"], compaction::compaction().call);
        assert_eq!(call["arguments"]["after"], 55 * 60);
        assert_eq!(call["_meta"]["link.local/thread"], "telegram:42");
    }

    let asked = world.asked();
    let last = asked.last().unwrap().clone();
    assert!(answering(&last));
    world.fire_quiet(&thread).await;
    let before = asked.len();
    let asked = world.asked();
    assert_eq!(asked.len(), before + 1, "one summary asked");
    let summary = asked.last().unwrap();
    assert!(only_summarizing(summary), "{summary}");
    // Its prompt is the last answer's up to the owner's last message.
    let sent = summary["messages"].as_array().unwrap();
    let answered = last["messages"].as_array().unwrap();
    assert_eq!(sent.len(), answered.len());
    assert_eq!(sent[..sent.len() - 1], answered[..answered.len() - 1]);
    assert_eq!(summary["tools"], last["tools"]);

    let folder = world.folder("telegram-42");
    let kept = summaries(&folder);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0]["text"], "Summary of 1, 2.");
    let events = common::events(&folder);
    let third = events
        .iter()
        .find(|e| {
            e["text"]
                .as_str()
                .is_some_and(|t| t.starts_with("Message 3:"))
        })
        .unwrap();
    assert_eq!(kept[0]["covers"], third["seq"]);
    let from = compaction::from("telegram:42");
    assert!(events.iter().all(|e| e["from"] != from.as_str()));
    assert_eq!(calls.lock().unwrap().len(), 3, "the firing keeps none");

    world.fire_quiet(&thread).await;
    assert_eq!(world.asked().len(), asked.len(), "nothing new to compact");

    world.say(&thread, 4, 4).await;
    let last = world.asked().last().unwrap().clone();
    let head = last["messages"][1]["content"].as_str().unwrap();
    assert!(head.starts_with(SUMMARY_HEAD), "{head}");
    assert!(!head.contains("Message 1:"), "{head}");
    // When it compacted, for it to say if asked.
    let written = format!("(Written {}.)", kept[0]["at"].as_str().unwrap());
    assert!(head.contains(&written), "{head}");
    assert_eq!(calls.lock().unwrap().len(), 4);
}

/// A turn keeps no compaction for a conversation of `lnk agent chat`'s
/// (Link's `[connection.link]` says `never`), one on no connection (a
/// subagent's), or where `compact_after` or `compact_at` is `"never"`;
/// your `[connection.link]` turns it on.
#[tokio::test]
async fn only_a_chat_on_a_connection_keeps_one() {
    let never = "[connections]\ncompact_after = \"never\"\n";
    let at_never = "[window]\ncompact_at = \"never\"\n";
    let link_on = "[connection.link]\ncompact_after = \"55m\"\n";
    let fixed = "[connections]\ncompact_after = \"55m\"\n";
    for (name, own, thread, keeps) in [
        ("quiet-link", "", "link:main", false),
        ("quiet-task", fixed, "task-aaaaaaaaaaa1", false),
        ("quiet-never", never, "telegram:42", false),
        ("quiet-at-never", at_never, "telegram:42", false),
        ("quiet-link-on", link_on, "link:main", true),
    ] {
        let (scheduler, calls) = fake_scheduler().await;
        let env = vec![("LNK_TOOL_CRON".into(), scheduler)];
        let world = world(name, own, env, 100_000).await;
        world.say(&Thread::new(thread).unwrap(), 1, 1).await;
        assert_eq!(calls.lock().unwrap().len(), usize::from(keeps), "{name}");
    }
}

/// A conversation that compacted by tokens at its last turn's end isn't
/// compacted again when it's quiet: that summary was never a prompt's
/// start, so the cache wouldn't hold, and nothing is asked.
#[tokio::test]
async fn compacted_since_the_last_message_it_isnt_again() {
    let (scheduler, _) = fake_scheduler().await;
    let own = "[window]\nsave_notes = false\n[connections]\ncompact_after = \"55m\"\n";
    let env = vec![("LNK_TOOL_CRON".into(), scheduler)];
    let world = world("quiet-since", own, env, 600).await;
    let thread = Thread::new("telegram:42").unwrap();
    let folder = world.folder("telegram-42");
    let mut n = 0;
    while summaries(&folder).is_empty() {
        n += 1;
        assert!(n <= 20, "it never compacted by tokens");
        world.say(&thread, n, n).await;
    }
    let (asked, kept) = (world.asked().len(), summaries(&folder).len());
    world.fire_quiet(&thread).await;
    assert_eq!(world.asked().len(), asked);
    assert_eq!(summaries(&folder).len(), kept);
}

/// With `"auto"` and a model whose cache Link knows, a conversation too
/// small to be worth summarizing keeps no compaction: each turn takes
/// away any one waiting. A model Link knows nothing of keeps none either.
#[tokio::test]
async fn auto_keeps_none_while_rereading_is_cheaper() {
    let (scheduler, calls) = fake_scheduler().await;
    let env = vec![("LNK_TOOL_CRON".into(), scheduler)];
    let knows = world_of(
        "quiet-auto",
        "",
        env,
        Some(100_000),
        "claude-opus-5-5",
        std::time::Duration::ZERO.into(),
        false,
    )
    .await;
    knows.say(&Thread::new("telegram:42").unwrap(), 1, 2).await;
    let kept = calls.lock().unwrap().clone();
    assert_eq!(kept.len(), 2, "{kept:?}");
    for call in &kept {
        assert_eq!(call["name"], compaction::compaction().call);
        assert_eq!(call["arguments"], json!({}), "taken away");
    }

    let (scheduler, calls) = fake_scheduler().await;
    let env = vec![("LNK_TOOL_CRON".into(), scheduler)];
    let world = world("quiet-unknown", "", env, 100_000).await;
    world.say(&Thread::new("telegram:42").unwrap(), 1, 1).await;
    assert_eq!(calls.lock().unwrap()[0]["arguments"], json!({}));
}
