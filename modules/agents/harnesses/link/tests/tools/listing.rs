//! What's offered of what each tool listed: kept for a new process, cut
//! to its limits, each name once.

use super::*;

#[tokio::test]
async fn a_busy_bridge_does_not_hide_the_tool_for_five_minutes() {
    let (url, seen) = fake_tool(false).await;
    seen.lock().unwrap().busy = 2;
    let world = world("busy-listing", false, memory(&url)).await;
    let answer = ask(&world.endpoint, "link:fresh", "Look it up.").await;
    assert!(!answer.contains("unavailable"), "{answer}");
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.busy, 0);
        assert_eq!(seen.lists, 1);
        assert_eq!(seen.calls.len(), 1);
    }
    assert!(
        world.asked.lock().unwrap()[0]["tools"]
            .to_string()
            .contains("memory__search")
    );
    world
        .server
        .harness
        .end_idle_tools_within(std::time::Duration::ZERO, AGE)
        .await;
    seen.lock().unwrap().busy = 2;
    let answer = ask(&world.endpoint, "link:fresh", "Look again.").await;
    assert!(!answer.contains("unavailable"), "{answer}");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.calls.len(), 2);
    assert_eq!(seen.sessions.len(), 2);
}

/// A tool that fails to start after it was listed: still offered the
/// same way, so the prompt stays the same, and its call answered as
/// unavailable. A turn starts no session before it calls.
#[tokio::test]
async fn a_tool_that_fails_to_start_is_still_offered() {
    let (url, seen) = fake_tool(false).await;
    let world = world("down", false, memory(&url)).await;
    ask(&world.endpoint, "telegram:42", "What did I say?").await;
    assert_eq!(seen.lock().unwrap().lists, 1);
    // Its session ended while idle, and it doesn't start again.
    world
        .server
        .harness
        .end_idle_tools_within(std::time::Duration::ZERO, AGE)
        .await;
    seen.lock().unwrap().down = true;
    let answer = ask(&world.endpoint, "telegram:42", "And when?").await;
    assert!(answer.contains("is unavailable"), "{answer}");
    let asked = world.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 4);
    assert_eq!(asked[2]["tools"], asked[0]["tools"]);
    assert_eq!(asked[3]["tools"], asked[0]["tools"]);
    // The first turn's prompt is the start of the next one's.
    let (first, next) = (
        asked[1]["messages"].as_array().unwrap(),
        asked[2]["messages"].as_array().unwrap(),
    );
    assert_eq!(first[..], next[..first.len()]);
    let events = world.raw_events("telegram-42");
    let call = &asked[3]["messages"].as_array().unwrap().last().unwrap()["tool_call_id"];
    let failed = events
        .iter()
        .find(|event| event["role"] == "tool" && event["call"] == *call)
        .expect("the unavailable call has a persisted tool answer");
    assert_eq!(failed["role"], "tool");
    assert_eq!(failed["failed"], true);
    assert_eq!(seen.lock().unwrap().sessions, ["s1"]);
}

/// Of `asked`'s requests after the first, how many a provider's cache
/// misses (their tools aren't the request before's, or their messages
/// don't start with its messages), and how many bytes of their tools and
/// messages are past what they share with it, in all.
fn cache_misses(asked: &[Value]) -> (usize, usize) {
    let prompt = |a: &Value| format!("{}{}", a["tools"], a["messages"]);
    let (mut misses, mut uncached) = (0, 0);
    for pair in asked.windows(2) {
        let before = pair[0]["messages"].as_array().unwrap();
        let now = pair[1]["messages"].as_array().unwrap();
        if pair[0]["tools"] != pair[1]["tools"] || !now.starts_with(before) {
            misses += 1;
        }
        let (before, now) = (prompt(&pair[0]), prompt(&pair[1]));
        let shared = before
            .bytes()
            .zip(now.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        uncached += now.len() - shared;
    }
    (misses, uncached)
}

/// A new process offers each tool as the last process listed it, until
/// it lists it itself: a tool down at its start is still offered, and
/// its call answered as unavailable, so the tools, and the provider's
/// cache, hold across a restart.
#[tokio::test]
async fn a_new_process_offers_the_tools_the_last_one_listed() {
    let (memory_url, memory) = fake_tool(false).await;
    let (gh_url, _) = fake_tool(false).await;
    let env: Vec<(String, String)> = vec![
        ("LNK_MEMORY".into(), memory_url),
        ("LNK_TOOL_GH".into(), gh_url),
    ];
    let world = world("restart", false, env.clone()).await;
    ask(&world.endpoint, "telegram:42", "What did I say?").await;
    // The next process starts with memory down.
    memory.lock().unwrap().down = true;
    let next = serve(&world.root, env.clone()).await;
    let answer = ask(&next, "telegram:42", "And when?").await;
    assert!(answer.contains("is unavailable"), "{answer}");
    // And the one after with it up again.
    memory.lock().unwrap().down = false;
    let after = serve(&world.root, env).await;
    let answer = ask(&after, "telegram:42", "And where?").await;
    assert!(answer.ends_with("on Friday."), "{answer}");
    let asked = world.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 6);
    let (misses, uncached) = cache_misses(&asked);
    eprintln!("across two restarts: {misses} cache misses, {uncached} bytes past the cache");
    assert!(asked.iter().all(|a| a["tools"] == asked[0]["tools"]));
    assert_eq!(misses, 0);
}

/// A restarted agent's first ask for a tool can come before its sandbox
/// grants it: the start waits for the grant, so the tool is listed, not
/// left out or offered as last listed, and nothing is warned.
#[tokio::test]
async fn a_first_listing_waits_for_its_tools_grant() {
    let (url, seen) = fake_tool(false).await;
    seen.lock().unwrap().refused = 2;
    let world = world("granted-late", false, memory(&url)).await;
    let answer = ask(&world.endpoint, "link:fresh", "Look it up.").await;
    assert!(!answer.contains("unavailable"), "{answer}");
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.refused, 0);
        assert_eq!(seen.lists, 1);
        assert_eq!(seen.calls.len(), 1);
    }
    assert!(
        world.asked.lock().unwrap()[0]["tools"]
            .to_string()
            .contains("memory__search")
    );
}

/// A tool offered as the last process listed it is asked again at the
/// next turn, and what it lists then replaces the saved list: a tool
/// added since appears, whatever was refused at the start.
#[tokio::test]
async fn a_saved_list_is_asked_again_at_the_next_turn() {
    let (url, seen) = fake_tool(false).await;
    let world = world("saved-again", false, memory(&url)).await;
    let path = world.root.join("home/tool-lists.json");
    ask(&world.endpoint, "telegram:42", "What did I say?").await;
    // The next process starts with it down: offered as last listed.
    seen.lock().unwrap().down = true;
    let next = serve(&world.root, memory(&url)).await;
    let answer = ask(&next, "telegram:42", "And when?").await;
    assert!(answer.contains("is unavailable"), "{answer}");
    let offered = |asked: &Value| asked["tools"].to_string();
    assert!(!offered(world.asked.lock().unwrap().last().unwrap()).contains("memory__remember"));
    // Up again, with a tool added since: the same process's next turn
    // lists it.
    {
        let mut seen = seen.lock().unwrap();
        seen.down = false;
        seen.more = true;
    }
    ask(&next, "telegram:42", "And where?").await;
    assert!(offered(world.asked.lock().unwrap().last().unwrap()).contains("memory__remember"));
    assert_eq!(seen.lock().unwrap().lists, 2);
    let kept: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(kept.to_string().contains("remember"), "{kept}");
}

/// A start lists its tools and keeps what each says of itself before any
/// turn: the prompt `instructions` prints then has the tools section the
/// first turn sends.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_keeps_what_its_tools_say_before_a_turn() {
    let (memory_url, _) = fake_tool(false).await;
    let world = world("started", false, vec![("LNK_MEMORY".into(), memory_url)]).await;
    let path = world.root.join("home/tool-lists.json");
    assert!(!path.exists());
    world
        .server
        .harness
        .list_tools(&Agent::new("main").unwrap())
        .await;
    let lists: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(lists["servers"][0]["name"], "memory");
    assert_eq!(
        lists["servers"][0]["instructions"],
        "Search before you say you don't know."
    );
    assert!(world.asked.lock().unwrap().is_empty());
}
/// The lists a process keeps are derived: unreadable, they're made again
/// from what the tools list; kept again only when a list changed; and
/// only of the agent's tools now, so a tool taken away, or moved, isn't
/// offered as it was.
#[tokio::test]
async fn the_kept_lists_are_derived() {
    use std::os::unix::fs::MetadataExt;
    let (memory_url, _) = fake_tool(false).await;
    let (gh_url, _) = fake_tool(false).await;
    let env: Vec<(String, String)> = vec![
        ("LNK_MEMORY".into(), memory_url.clone()),
        ("LNK_TOOL_GH".into(), gh_url.clone()),
    ];
    let world = world("derived", false, env.clone()).await;
    let path = world.root.join("home/tool-lists.json");
    let kept = || -> Value { serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap() };
    ask(&world.endpoint, "telegram:42", "What did I say?").await;
    let lists = kept();
    let names: Vec<&str> = lists["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["memory", "gh"]);
    // With what each says of itself, for a process whose tool won't start.
    assert_eq!(
        lists["servers"][0]["instructions"],
        "Search before you say you don't know."
    );
    // Its address, which holds the proxy's secret, isn't in it.
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(&memory_url) && !text.contains(&gh_url));

    // Unreadable, it's made again.
    std::fs::write(&path, "not the lists").unwrap();
    let next = serve(&world.root, env.clone()).await;
    ask(&next, "telegram:42", "And when?").await;
    assert_eq!(kept(), lists);

    // The same lists aren't written again.
    let inode = std::fs::metadata(&path).unwrap().ino();
    let after = serve(&world.root, env).await;
    ask(&after, "telegram:42", "And where?").await;
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);

    // Memory taken away, and gh moved to where nothing answers: neither
    // is offered as it was.
    let (moved, moved_seen) = fake_tool(false).await;
    moved_seen.lock().unwrap().down = true;
    let without = serve(&world.root, vec![("LNK_TOOL_GH".into(), moved)]).await;
    assert_eq!(ask(&without, "telegram:7", "Hi").await, "No tools here.");
    assert!(
        world
            .asked
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .get("tools")
            .is_none()
    );
    assert_eq!(kept()["servers"], json!([]));
}

/// A tool server listing what `tools` gives each `tools/list`, one page.
async fn listing_tool(tools: Value) -> String {
    let app = Router::new()
        .route(
            "/_link/mcp/big",
            post(
                |State(tools): State<Arc<Value>>, Json(message): Json<Value>| async move {
                    let result = match message["method"].as_str() {
                        Some("initialize") => json!({"protocolVersion": "2025-06-18",
                            "capabilities": {"tools": {}}, "serverInfo": {"name": "big", "version": "1"}}),
                        Some("tools/list") => json!({"tools": *tools}),
                        _ => return axum::http::StatusCode::ACCEPTED.into_response(),
                    };
                    let answer = json!({"jsonrpc": "2.0", "id": message["id"], "result": result});
                    ([("mcp-session-id", "s1")], Json(answer)).into_response()
                },
            ),
        )
        .with_state(Arc::new(tools));
    let port = common::serve(app).await;
    let url = format!("http://127.0.0.1:{port}/_link/mcp/big");
    url
}

/// A tool can't put megabytes into every prompt: a long description is
/// cut, a large schema leaves its tool out, and what's offered in all
/// has a size.
#[tokio::test]
async fn a_tool_offers_only_so_much() {
    let schema = |bytes: usize| json!({"type": "object", "description": "s".repeat(bytes)});
    let mut listed = vec![
        json!({"name": "long", "description": "d".repeat(100_000), "inputSchema": schema(10)}),
        json!({"name": "wide", "description": "Wide.", "inputSchema": schema(100_000)}),
    ];
    for n in 0..300 {
        listed.push(
            json!({"name": format!("t{n}"), "description": "x".repeat(2000),
            "inputSchema": schema(10_000)}),
        );
    }
    let url = listing_tool(Value::Array(listed)).await;
    let world = world("big", false, vec![("LNK_TOOL_BIG".into(), url)]).await;
    ask(&world.endpoint, "telegram:42", "Hi").await;
    let asked = world.asked.lock().unwrap().clone();
    let offered = asked[0]["tools"].as_array().unwrap();
    let named = |name: &str| {
        offered
            .iter()
            .find(|t| t["function"]["name"] == name)
            .cloned()
    };
    let long = named("big__long").expect("the long one, cut");
    let description = long["function"]["description"].as_str().unwrap();
    assert!(description.len() < 2100, "{}", description.len());
    assert!(named("big__wide").is_none());
    assert!(named("big__t0").is_some());
    let size = serde_json::to_string(offered).unwrap().len();
    assert!(size < 140 << 10, "{size} bytes offered");
    assert!(offered.len() < 20, "{} offered", offered.len());
}

/// Two tools whose names come out the same, or a name too long for a
/// provider: the first is offered, and a cut name never. What's offered
/// is in order of name, whatever order the server lists it in.
#[tokio::test]
async fn a_tool_name_is_offered_once_and_whole() {
    let long = "l".repeat(60);
    let url = listing_tool(json!([
        {"name": "repo.view", "description": "First."},
        {"name": "repo_view", "description": "Second."},
        {"name": long, "description": "Too long."},
        {"name": "ok", "description": "Fine."},
    ]))
    .await;
    let world = world("names", false, vec![("LNK_TOOL_BIG".into(), url)]).await;
    ask(&world.endpoint, "telegram:42", "Hi").await;
    let asked = world.asked.lock().unwrap().clone();
    let offered: Vec<(&str, &str)> = asked[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["function"]["name"].as_str().unwrap(),
                t["function"]["description"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        offered,
        [("big__ok", "Fine."), ("big__repo_view", "First.")]
    );
}
