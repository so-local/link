//! One process for two agents of one owner, as `lnk agent start` runs it
//! where the machine shares a harness among an owner's agents:
//! `lnk-harness serve --agents <file>`, each agent on its own port,
//! with its own key, folders and tool server. Each port answers as its
//! own agent, from its own conversation and with its own tool; neither
//! agent's key opens the other's; neither's words reach the other's
//! folders or prompt. A turn one of them had cut short is finished as the
//! process starts, as after any restart.

use crate::common;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
};
use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    event::{self, Event},
    store::{Files, Lease, Store, TURN},
};
use link_harness_contract::harness::{Model, SharedAgent};
use serde_json::{Value, json};

/// An MCP server named `name`, with one tool, `search`, whose answers
/// name it.
async fn fake_tool(name: &'static str) -> String {
    let app = Router::new().route(
        "/_link/mcp/memory",
        post(
            move |headers: HeaderMap, Json(message): Json<Value>| async move {
                let method = message["method"].as_str().unwrap_or_default().to_string();
                if method == "initialize" {
                    let answer = json!({"jsonrpc": "2.0", "id": message["id"],
                    "result": {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                    "serverInfo": {"name": name, "version": "1"}}});
                    return ([("mcp-session-id", name)], Json(answer)).into_response();
                }
                if message.get("id").is_none() {
                    return axum::http::StatusCode::ACCEPTED.into_response();
                }
                let _ = headers;
                let result = match method.as_str() {
                    "tools/list" => json!({"tools": [{"name": "search",
                    "description": "Search.", "inputSchema": {"type": "object"}}]}),
                    _ => json!({"content": [{"type": "text", "text": format!("{name}'s memory")}]}),
                };
                Json(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
                    .into_response()
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/_link/mcp/memory",
        listener.local_addr().unwrap().port()
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

/// A model that searches once for each new message, then answers with
/// what the tool said, or, asked to go on after a crash, says so; it keeps
/// what it was asked.
async fn fake_model() -> (String, Arc<Mutex<Vec<Value>>>) {
    let asked = Arc::new(Mutex::new(Vec::<Value>::new()));
    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(
                |State(asked): State<Arc<Mutex<Vec<Value>>>>, Json(body): Json<Value>| async move {
                    asked.lock().unwrap().push(body.clone());
                    let messages = body["messages"].as_array().unwrap();
                    let last = messages.last().unwrap();
                    let data = |v: Value| format!("data: {v}\n\n");
                    let said = last["content"].as_str().unwrap_or_default();
                    let text = match last["role"].as_str() {
                        Some("user") if said.contains("Bea's unfinished") || said.contains("go on") => {
                            data(json!({"id": "g", "choices": [{"delta": {"content": "Finished at last"}}]}))
                        }
                        Some("user") => data(json!({"id": "g", "choices": [{"delta": {"tool_calls": [
                            {"index": 0, "id": format!("call_{}", messages.len()), "type": "function",
                             "function": {"name": "memory__search", "arguments": "{}"}}]}}]})),
                        _ => data(json!({"id": "g", "choices": [{"delta": {"content":
                            format!("Found {said}")}}]})),
                    };
                    Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(axum::body::Body::from(text + "data: [DONE]\n\n"))
                        .unwrap()
                },
            ),
        )
        .with_state(asked.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, asked)
}

/// Asks the harness on `port` over ACP as a bridge would, naming the
/// agent if `name`, with `key`: 200 and the answer, or a refusal's
/// status and why.
async fn ask(port: u16, name: Option<&str>, key: &str, text: &str) -> (u16, String) {
    let asking = common::Asking {
        key,
        agent: name,
        ..Default::default()
    };
    common::answered(common::ask_as(port, "telegram:42", text, asking).await)
        .unwrap_or((0, "nothing answered".into()))
}

/// Every file under `dir`, read whole, as one text.
fn everything_in(dir: &Path) -> String {
    let mut all = String::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match path.is_dir() {
            true => all.push_str(&everything_in(&path)),
            false => all.push_str(&String::from_utf8_lossy(
                &std::fs::read(&path).unwrap_or_default(),
            )),
        }
    }
    all
}

#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn one_process_answers_each_agent_as_itself() {
    link_fetch::install_crypto_provider();
    let root = std::env::temp_dir().join(format!("lnk-shared-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (model_url, asked) = fake_model().await;
    let ada = Agent::with_identity("ada", "ada000000001", "me@here").unwrap();
    let bea = Agent::with_identity("bea", "bea000000002", "me@here").unwrap();
    let mut shared = Vec::new();
    // Each agent's port, which the harness binds itself: held until the
    // test ends.
    let mut held = Vec::new();
    for (agent, key, tool) in [
        (&ada, "ada-key", fake_tool("ada").await),
        (&bea, "bea-key", fake_tool("bea").await),
    ] {
        let (home, files) = (
            root.join(agent.name()).join("home"),
            root.join(agent.name()).join("files"),
        );
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(files.join("instructions")).unwrap();
        std::fs::write(files.join("instructions/instruction.md"), "Be brief.\n").unwrap();
        let mut agents = Agents::default();
        agents.add(agent.clone(), home.clone(), common::flat(files.clone()));
        Files::new(agents)
            .set_config(
                agent,
                &Config {
                    model: Model {
                        provider: "local".into(),
                        name: Some("fake".into()),
                        base_url: Some(model_url.clone()),
                        ..Model::default()
                    },
                    endpoint_key: Some(key.to_string().into()),
                    ..Config::default()
                },
            )
            .unwrap();
        // The agent's port and the one after it, for its tools: a block.
        let port = crate::common::free_block();
        shared.push(SharedAgent {
            name: agent.name().into(),
            id: agent.id().into(),
            owner: agent.owner().into(),
            home,
            files,
            agent: None,
            port: port.number() as u16,
            env: [("LNK_MEMORY".to_string(), tool)].into_iter().collect(),
        });
        held.push(port);
    }
    let (ada_port, bea_port) = (shared[0].port, shared[1].port);

    // A turn of Bea's a crash cut short: her message, and a lease long
    // lapsed.
    let thread = Thread::new("telegram:42").unwrap();
    let mut agents = Agents::default();
    agents.add(
        bea.clone(),
        shared[1].home.clone(),
        common::flat(shared[1].files.clone()),
    );
    let store = Files::new(agents);
    let message = Event {
        seq: 1,
        at: event::now(),
        ..Event::user("Bea's unfinished question")
    };
    store.append(&bea, &thread, &message, &[]).unwrap();
    let folder = shared[1].files.join("chats").join(thread.folder());
    let lease = Lease {
        holder: "1-dead".into(),
        thread: thread.name().into(),
        started: 1,
        renewed: 1,
    };
    std::fs::write(folder.join(TURN), serde_json::to_vec(&lease).unwrap()).unwrap();

    let file = root.join("agents.json");
    let agents = link_harness_contract::harness::SharedAgents {
        agents: shared.to_vec(),
    };
    std::fs::write(&file, link_files::saved::print_json(&agents).unwrap()).unwrap();
    let mut harness = Command::new(env!("CARGO_BIN_EXE_lnk-harness"))
        .args(["serve", "--agents"])
        .arg(&file)
        .env_clear()
        .env("HOME", &root)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&root))
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let answering = |port: u16| std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    let started = std::time::Instant::now();
    while !(answering(ada_port) && answering(bea_port)) {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "it didn't listen"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Each on its own port, named or not, with its own tool.
    let (status, said) = ask(ada_port, None, "ada-key", "Ada's secret").await;
    assert_eq!((status, said.as_str()), (200, "Found ada's memory"));
    let (status, said) = ask(bea_port, Some("bea"), "bea-key", "Bea's secret").await;
    assert_eq!((status, said.as_str()), (200, "Found bea's memory"));

    // Neither's key opens the other's endpoint, by its port or its name.
    let (status, _) = ask(bea_port, None, "ada-key", "Let me in").await;
    assert_eq!(status, 401);
    let (status, _) = ask(ada_port, Some("bea"), "ada-key", "Let me in").await;
    assert_eq!(status, 401);
    // An agent it doesn't serve is refused.
    let (status, _) = ask(ada_port, Some("cid"), "ada-key", "Hi").await;
    assert_eq!(status, 404);

    // Bea's cut-short turn was finished as it started.
    let started = std::time::Instant::now();
    loop {
        let bea_said = everything_in(&shared[1].files.join("chats"));
        if bea_said.contains("Finished at last") {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "Bea's turn wasn't finished: {bea_said}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !shared[1]
            .files
            .join("chats")
            .join(thread.folder())
            .join(TURN)
            .exists()
    );

    // Each one's folders, and each prompt, hold only its own words.
    let ada_kept = everything_in(&shared[0].files) + &everything_in(&shared[0].home);
    let bea_kept = everything_in(&shared[1].files) + &everything_in(&shared[1].home);
    assert!(
        ada_kept.contains("Ada's secret") && !ada_kept.contains("Bea's"),
        "{ada_kept}"
    );
    assert!(
        bea_kept.contains("Bea's secret") && !bea_kept.contains("Ada's"),
        "{bea_kept}"
    );
    for body in asked.lock().unwrap().iter() {
        let text = body.to_string();
        assert!(
            !(text.contains("Ada's secret") && text.contains("Bea's")),
            "one prompt held both: {text}"
        );
    }

    let _ = harness.kill();
    let _ = harness.wait();
    let _ = std::fs::remove_dir_all(&root);
}

/// Its agents changing, the shared process stops taking connections, but
/// one a bridge opened just before is answered: asked its prompt after
/// the stop, its turn runs and answers, and only then does the process
/// end. Never cut between its opening and its prompt, which its bridge
/// would tell its owner as "couldn't answer".
#[tokio::test]
async fn a_connection_open_as_it_stops_is_answered() {
    use link_agent_state_protocol::acp;
    use link_agent_state_protocol::acp::schema::v1::{
        ContentBlock, InitializeRequest, PromptRequest, ResumeSessionRequest, SessionId,
        SessionNotification, SessionUpdate, TextContent,
    };
    link_fetch::install_crypto_provider();
    let root = link_test_kit::TempDir::new("shared-stopping");
    let (model, _) = common::fake_model(|_, _| {
        vec![common::delta(json!({"content": "Answered after the stop"}))]
    })
    .await;
    let ada = Agent::with_identity("ada", "ada000000001", "me@here").unwrap();
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let mut agents = Agents::default();
    agents.add(ada.clone(), home.clone(), common::flat(files.clone()));
    Files::new(agents)
        .set_config(
            &ada,
            &Config {
                model: Model {
                    provider: "local".into(),
                    name: Some("fake".into()),
                    base_url: Some(format!("http://127.0.0.1:{model}")),
                    ..Model::default()
                },
                endpoint_key: Some("ada-key".to_string().into()),
                ..Config::default()
            },
        )
        .unwrap();
    // Its ports, the one given and the one after it, which the harness
    // binds itself: held as a block until the test ends.
    let held = crate::common::free_block();
    let port = held.number() as u16;
    let shared = [SharedAgent {
        name: ada.name().into(),
        id: ada.id().into(),
        owner: ada.owner().into(),
        home,
        files,
        agent: None,
        port,
        env: Default::default(),
    }];
    let file = root.join("agents.json");
    let agents = link_harness_contract::harness::SharedAgents {
        agents: shared.to_vec(),
    };
    std::fs::write(&file, link_files::saved::print_json(&agents).unwrap()).unwrap();
    let harness = link_test_kit::ChildGroup::spawn(
        link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-harness"), &root)
            .args(["serve", "--agents"])
            .arg(&file)
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .unwrap();
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    link_test_kit::wait::accepting("the shared harness", address, Duration::from_secs(20));

    let said = Arc::new(Mutex::new(String::new()));
    let heard = said.clone();
    let session = SessionId::new("telegram:42");
    let answered = acp::Client
        .builder()
        .on_receive_notification(
            async move |update: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(chunk) = update.update
                    && let ContentBlock::Text(text) = chunk.content
                {
                    heard.lock().unwrap().push_str(&text.text);
                }
                Ok(())
            },
            acp::on_receive_notification!(),
        )
        .connect_with(
            common::acp_lines(&common::acp(port), "ada-key").await,
            async |cx: acp::ConnectionTo<acp::Agent>| {
                cx.send_request(InitializeRequest::new(acp::schema::ProtocolVersion::V1))
                    .block_task()
                    .await?;
                // Asked to stop, as its owner's harness asks for a change
                // of its agents: it takes no new connection.
                std::fs::remove_file(&file).unwrap();
                link_test_kit::wait::eventually_async(
                    "the stopping harness to take no new connection",
                    Duration::from_secs(20),
                    || std::net::TcpStream::connect(address).is_err(),
                )
                .await;
                cx.send_request(ResumeSessionRequest::new(session.clone(), "/"))
                    .block_task()
                    .await?;
                let text = ContentBlock::Text(TextContent::new("hello"));
                cx.send_request(PromptRequest::new(session, vec![text]))
                    .block_task()
                    .await
            },
        )
        .await;
    assert!(answered.is_ok(), "cut off: {answered:?}");
    assert_eq!(*said.lock().unwrap(), "Answered after the stop");
    // Its one connection closed, it ends.
    let ended = link_test_kit::wait::eventually_async(
        "the stopped harness to end",
        Duration::from_secs(20),
        || harness.try_status().unwrap(),
    )
    .await;
    assert!(ended.success(), "{ended}");
}
