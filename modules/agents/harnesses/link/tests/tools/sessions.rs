//! Tools' sessions: kept until old, ended with their turn, and given up
//! on when a server doesn't start, answer or finish listing.

use super::*;

#[tokio::test]
async fn a_kept_session_ends_when_it_is_old() {
    let (url, seen) = fake_tool(false).await;
    let world = world("old", false, memory(&url)).await;
    ask(
        &world.endpoint,
        "telegram:42",
        "What did I say about driving?",
    )
    .await;
    // Not idle, but old.
    world
        .server
        .harness
        .end_idle_tools_within(IDLE, std::time::Duration::ZERO)
        .await;
    assert_eq!(seen.lock().unwrap().ended, ["s1"]);
    assert_eq!(world.server.harness.tool_sessions(), 0);
    ask(&world.endpoint, "telegram:42", "And when?").await;
    assert_eq!(seen.lock().unwrap().sessions, ["s1", "s2"]);
    assert_eq!(world.server.harness.tool_sessions(), 1, "kept again");
}

/// Only memory's session outlives its turn. Another tool's ends with it,
/// so what one conversation left in it (a browser's pages, a shell's
/// folder) isn't there for the next person's: named `memory` too, at
/// `$LNK_TOOL_MEMORY`, it isn't memory.
#[tokio::test]
async fn another_tools_session_ends_with_its_turn() {
    let (url, seen) = fake_tool(false).await;
    let world = world("per-turn", false, vec![("LNK_TOOL_MEMORY".into(), url)]).await;
    ask(
        &world.endpoint,
        "telegram:42",
        "What did I say about driving?",
    )
    .await;
    assert_eq!(seen.lock().unwrap().ended, ["s1"], "ended with the turn");
    assert_eq!(world.server.harness.tool_sessions(), 0);
    ask(&world.endpoint, "telegram:7", "And about flying?").await;
    let seen = seen.lock().unwrap();
    assert_eq!(seen.calls.len(), 2);
    assert_eq!(seen.sessions, ["s1", "s2"], "a session of its own");
    assert_eq!(seen.ended, ["s1", "s2"]);
    assert_eq!(world.server.harness.tool_sessions(), 0);
}

#[tokio::test]
async fn a_tool_that_doesnt_answer_is_left_out() {
    // Nothing listens there.
    let gone = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!(
            "http://127.0.0.1:{}/_link/mcp/gh",
            listener.local_addr().unwrap().port()
        )
    };
    let world = world("gone", false, vec![("LNK_TOOL_GH".into(), gone)]).await;
    assert_eq!(
        ask(&world.endpoint, "telegram:42", "Hi").await,
        "No tools here."
    );
    let asked = world.asked.lock().unwrap().clone();
    assert!(asked[0].get("tools").is_none(), "{}", asked[0]);
    assert_eq!(world.conversation_events("telegram-42").len(), 2);
}

/// A tool server whose `tools/list` never ends, alternating two cursors:
/// the turn reads a few pages, each cursor once, and goes on.
#[tokio::test]
async fn a_tool_list_that_never_ends_ends() {
    let pages = Arc::new(Mutex::new(0usize));
    let app = Router::new()
        .route(
            "/_link/mcp/looping",
            post(
                |State(pages): State<Arc<Mutex<usize>>>, Json(message): Json<Value>| async move {
                    let result = match message["method"].as_str() {
                        Some("initialize") => json!({"protocolVersion": "2025-06-18",
                            "capabilities": {"tools": {}}, "serverInfo": {"name": "loop", "version": "1"}}),
                        Some("tools/list") => {
                            *pages.lock().unwrap() += 1;
                            let next = match message["params"]["cursor"].as_str() {
                                Some("a") => "b",
                                _ => "a",
                            };
                            json!({"tools": [], "nextCursor": next})
                        }
                        _ => return axum::http::StatusCode::ACCEPTED.into_response(),
                    };
                    let answer = json!({"jsonrpc": "2.0", "id": message["id"], "result": result});
                    ([("mcp-session-id", "s1")], Json(answer)).into_response()
                },
            ),
        )
        .with_state(pages.clone());
    let port = common::serve(app).await;
    let url = format!("http://127.0.0.1:{port}/_link/mcp/looping");
    let world = world("looping", false, vec![("LNK_TOOL_LOOPING".into(), url)]).await;
    let answer = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        ask(&world.endpoint, "telegram:42", "Hi"),
    )
    .await
    .expect("the turn hung on the tool's list");
    assert_eq!(answer, "No tools here.");
    assert!(
        *pages.lock().unwrap() <= 3,
        "{} pages",
        pages.lock().unwrap()
    );
}

/// A tool that takes longer to start than the harness waits is given up
/// on, and its connection closed, so whatever starts it (the sandbox's
/// tool bridge) sees the start abandoned and can end it.
#[tokio::test]
async fn a_start_given_up_on_closes_its_connection() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/_link/mcp/slow",
        listener.local_addr().unwrap().port()
    );
    // Reads the harness's start, never answers, and says when the
    // harness closes the connection. Another test's probe of a port it
    // once held can reach this one first: only the start counts, and the
    // port stays open for it.
    let closed = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let read = socket.read(&mut buf).await.unwrap_or(0);
            if !buf[..read].starts_with(b"POST /_link/mcp/slow ") {
                continue;
            }
            while socket.read(&mut buf).await.unwrap_or(0) > 0 {}
            return std::time::Instant::now();
        }
    });
    let root = TempDir::new("tools-given-up");
    let world = world("given-up-model", false, vec![]).await;
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(root.join("files")).unwrap();
    std::fs::copy(
        world.root.join("home/config.json"),
        home.join("config.json"),
    )
    .unwrap();
    let wait = std::time::Duration::from_millis(300);
    let endpoint = serve_waiting(&root, vec![("LNK_TOOL_SLOW".into(), url)], wait).await;
    let started = std::time::Instant::now();
    assert_eq!(ask(&endpoint, "telegram:42", "Hi").await, "No tools here.");
    let closed = tokio::time::timeout(std::time::Duration::from_secs(5), closed)
        .await
        .expect("the connection was left open")
        .unwrap();
    // Closed once the harness gave up, never before its wait. A
    // connection left open is the timeout above: the endpoint outlives
    // the turn, so how soon after the wait it closed proves nothing more.
    let after = closed.duration_since(started);
    eprintln!("given up on, and closed, after {after:?}");
    assert!(after >= wait, "{after:?}");
}

/// A tool that starts a session but takes longer to list its tools than
/// the harness waits has its session ended (MCP's DELETE): it holds
/// nothing for a listing nobody waits for.
#[tokio::test]
async fn a_listing_given_up_on_ends_its_session() {
    let (url, seen) = fake_tool(false).await;
    seen.lock().unwrap().slow = Some("tools/list");
    let root = TempDir::new("tools-list-given-up");
    let world = world("list-given-up-model", false, vec![]).await;
    std::fs::create_dir_all(root.join("home")).unwrap();
    std::fs::create_dir_all(root.join("files")).unwrap();
    std::fs::copy(
        world.root.join("home/config.json"),
        root.join("home/config.json"),
    )
    .unwrap();
    let wait = std::time::Duration::from_millis(300);
    let endpoint = serve_waiting(&root, memory(&url), wait).await;
    assert_eq!(ask(&endpoint, "telegram:42", "Hi").await, "No tools here.");
    assert_eq!(seen.lock().unwrap().ended, ["s1"]);
}

/// A tool listed at an earlier turn has as long to start again, and to
/// list its tools after a call, as it had the first time: a slow start
/// is answered as unavailable, a slow listing keeps what it listed.
#[tokio::test]
async fn a_tool_slow_to_start_later_is_left_out_as_at_first() {
    let (url, seen) = fake_tool(false).await;
    let world = world("slow", false, Vec::new()).await;
    let endpoint =
        serve_waiting(&world.root, memory(&url), std::time::Duration::from_secs(1)).await;
    let answer = ask(&endpoint, "telegram:42", "What did I say?").await;
    assert!(answer.ends_with("on Friday."), "{answer}");
    let timed = |text: &'static str| {
        let endpoint = endpoint.clone();
        async move {
            let started = std::time::Instant::now();
            let answer = ask(&endpoint, "telegram:42", text).await;
            assert!(
                started.elapsed() < std::time::Duration::from_secs(10),
                "{text}"
            );
            answer
        }
    };

    // Its process stopped, so its session is gone and starts again.
    seen.lock().unwrap().lost = true;
    seen.lock().unwrap().slow = Some("initialize");
    let answer = timed("And when?").await;
    assert!(answer.contains("took too long to start"), "{answer}");

    seen.lock().unwrap().slow = Some("tools/list");
    let answer = timed("And where?").await;
    assert!(answer.ends_with("on Friday."), "{answer}");
    let asked = world.asked.lock().unwrap().clone();
    assert_eq!(asked.last().unwrap()["tools"], asked[0]["tools"]);
}

/// A tool slower to start than a turn waits, on a busy machine, is left
/// out of that turn but not given up on: its start goes on, and the next
/// turn has its tools from that same start, never a second one begun.
#[tokio::test]
async fn a_start_past_a_turns_wait_serves_the_next_turn() {
    let (url, seen) = fake_tool(false).await;
    let held = Arc::new(tokio::sync::Semaphore::new(0));
    seen.lock().unwrap().held = Some(held.clone());
    let world = world("outlives", false, Vec::new()).await;
    let (endpoint, _server) = serve_limited(
        &world.root,
        memory(&url),
        std::time::Duration::from_millis(300),
        std::time::Duration::from_secs(60),
    )
    .await;
    assert_eq!(ask(&endpoint, "telegram:42", "Hi").await, "No tools here.");
    // The start the turn stopped waiting for, let through now, lists the
    // tool: a turn from then on has it, as listed or as its start ends.
    held.add_permits(1);
    link_test_kit::eventually_async(
        "the start's listing",
        std::time::Duration::from_secs(20),
        || seen.lock().unwrap().lists > 0,
    )
    .await;
    let answer = ask(&endpoint, "telegram:42", "What did I say?").await;
    assert!(answer.ends_with("on Friday."), "{answer}");
    assert_eq!(seen.lock().unwrap().sessions, ["s1"], "one start");
}
