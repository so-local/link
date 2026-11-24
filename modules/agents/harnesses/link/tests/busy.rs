//! A message sent while the agent answers, in process: with `[turns]
//! busy`, it joins the running turn (steer), waits for a turn of its own
//! (queue), or ends the running one (interrupt); none is left
//! unanswered or answered twice. A turn a crash left running is finished
//! by the next to take it, once.

use crate::common;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{Json, Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};
use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    event::{self, Call, Event},
    server::{self, Server},
    store::{Files, Lease, Store, TURN, Took},
    tools,
    turn::{Asked, Harness, NOT_MADE_AGAIN},
};
use link_harness_contract::harness::Model;
use link_secretive::Duplicate;
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// A model that answers "Answer <n>": "Answer " at once, then `<n>` as
/// `hold` says, streamed; it keeps what it was asked. Asked with tools and a
/// message of the owner's last, it calls `slow__wait` twice in one step.
async fn fake_model(hold: common::Hold) -> (String, common::Asked) {
    let (port, asked) = common::fake_model_with(Router::new(), hold, |body, n| {
        let calls = body["tools"].is_array()
            && body["messages"].as_array().unwrap().last().unwrap()["role"] == "user";
        match calls {
            true => vec![json!({"id": "g", "choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "c1", "type": "function",
                 "function": {"name": "slow__wait", "arguments": "{}"}},
                {"index": 1, "id": "c2", "type": "function",
                 "function": {"name": "slow__wait", "arguments": "{}"}}]}}]})],
            false => vec![
                json!({"id": "g", "model": "fake", "choices": [{"delta": {"content": "Answer "}}]}),
                json!({"id": "g", "choices": [{"delta": {"content": n.to_string()}}]}),
            ],
        }
    })
    .await;
    (format!("http://127.0.0.1:{port}"), asked)
}

/// An MCP server whose one tool, `wait`, only reads (its `readOnlyHint`,
/// so a step's two run at once) and answers after `call`, and whose
/// sessions end as `end` holds them; the calls it was asked, and those
/// cancelled.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn slow_tool(
    call: Duration,
    end: impl Into<common::Hold>,
) -> (String, Arc<Mutex<(usize, usize)>>) {
    let end = end.into();
    let seen = Arc::new(Mutex::new((0, 0)));
    let app = Router::new()
        .route(
            "/mcp",
            post(
                move |State(seen): State<Arc<Mutex<(usize, usize)>>>,
                 _: HeaderMap,
                 Json(message): Json<Value>| async move {
                    let method = message["method"].as_str().unwrap_or_default().to_string();
                    match method.as_str() {
                        "initialize" => {
                            let answer = json!({"jsonrpc": "2.0", "id": message["id"],
                                "result": {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                                "serverInfo": {"name": "slow", "version": "1"}}});
                            ([("mcp-session-id", "s1")], Json(answer)).into_response()
                        }
                        "tools/list" => Json(json!({"jsonrpc": "2.0", "id": message["id"],
                            "result": {"tools": [{"name": "wait", "description": "Waits.",
                            "inputSchema": {"type": "object"},
                            "annotations": {"readOnlyHint": true}}]}}))
                        .into_response(),
                        "tools/call" => {
                            seen.lock().unwrap().0 += 1;
                            tokio::time::sleep(call).await;
                            Json(json!({"jsonrpc": "2.0", "id": message["id"],
                                "result": {"content": [{"type": "text", "text": "waited"}]}}))
                            .into_response()
                        }
                        "notifications/cancelled" => {
                            seen.lock().unwrap().1 += 1;
                            axum::http::StatusCode::ACCEPTED.into_response()
                        }
                        _ => axum::http::StatusCode::ACCEPTED.into_response(),
                    }
                },
            )
            .delete(move || async move {
                end.wait(1).await;
                axum::http::StatusCode::OK
            }),
        )
        .with_state(seen.clone());
    let port = common::serve(app).await;
    (format!("http://127.0.0.1:{port}/mcp"), seen)
}

struct World {
    root: TempDir,
    agent: Agent,
    thread: Thread,
    harness: Arc<Harness>,
    store: Arc<Files>,
    asked: Arc<Mutex<Vec<Value>>>,
    agents: Agents,
    /// Where its turns' measures go.
    measures: std::path::PathBuf,
}

/// Link Harness for `main`, its spec Link's but for `own`, its model
/// holding each answer as `hold` says, with `tool` as a tool if given.
async fn world(name: &str, own: &str, hold: impl Into<common::Hold>, tool: Option<&str>) -> World {
    let root = TempDir::new(&format!("busy-{name}"));
    let (model_url, asked) = fake_model(hold.into()).await;
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(files));
    if let Some(url) = tool {
        agents.allow_tools(
            &agent,
            tools::from_env([("LNK_TOOL_SLOW".to_string(), url.to_string())]),
        );
    }
    let known = link_agent_spec::Known {
        name: "main".into(),
    };
    let own = link_agent_spec::parse(&format!("schema = 1\n{own}")).unwrap();
    let spec = link_agent_spec::resolve(Some(&own), &[], &known).unwrap();
    let store = Arc::new(Files::new(agents.clone()));
    store
        .set_config(
            &agent,
            &Config {
                model: Model {
                    provider: "local".into(),
                    name: Some("fake".into()),
                    base_url: Some(model_url),
                    ..Model::default()
                },
                endpoint_key: Some("test-key".to_string().into()),
                defaults: spec.links(),
                spec: Some(spec.spec),
                ..Config::default()
            },
        )
        .unwrap();
    let measures = root.join("measures.jsonl");
    World {
        agent,
        thread: Thread::new("telegram:42").unwrap(),
        harness: Arc::new(
            Harness::new(agents.clone(), store.clone())
                .unwrap()
                .with_measures(measures.clone()),
        ),
        store,
        asked,
        agents,
        measures,
        root,
    }
}

impl World {
    /// `text` asked, in a task: what came of it.
    fn ask(&self, text: &str) -> tokio::task::JoinHandle<anyhow::Result<(Asked, String)>> {
        self.ask_observed(text).0
    }

    /// A request and the bytes its caller has received, observed before
    /// another message interrupts the answer.
    fn ask_observed(
        &self,
        text: &str,
    ) -> (
        tokio::task::JoinHandle<anyhow::Result<(Asked, String)>>,
        tokio::sync::watch::Receiver<usize>,
    ) {
        let (harness, agent, thread, text) = (
            self.harness.clone(),
            self.agent.clone(),
            self.thread.clone(),
            text.to_string(),
        );
        let mut output = link_test_kit::io::ObservedOutput::default();
        let written = output.written();
        let reply = tokio::spawn(async move {
            let (pieces, mut received) = tokio::sync::mpsc::channel::<String>(64);
            let shown = tokio::spawn(async move {
                while let Some(piece) = received.recv().await {
                    output.write_all(piece.as_bytes()).unwrap();
                }
                String::from_utf8(output.into_bytes()).unwrap()
            });
            let asked = harness
                .ask(&agent, &thread, Event::user(&text), pieces, None)
                .await?;
            Ok((asked, shown.await.unwrap()))
        });
        (reply, written)
    }

    fn events(&self) -> Vec<Event> {
        self.store.read(&self.agent, &self.thread).unwrap()
    }

    fn conversation_events(&self) -> Vec<Event> {
        self.events()
            .into_iter()
            .filter(|event| event.role != event::EVIDENCE)
            .collect()
    }

    /// The measures of its first `turns` turns, once they're written:
    /// each turn's unit, and its points by name.
    async fn measured(&self, turns: usize) -> Vec<(String, Vec<(String, f64)>)> {
        common::measures(&self.measures, turns)
            .await
            .iter()
            .flat_map(link_measure_contract::client::read_otlp)
            .map(|report| {
                let points = report.points.into_iter();
                let points = points.map(|p| (p.measure.name.to_string(), p.value));
                (report.unit.id, points.collect())
            })
            .collect()
    }

    /// Each owner's message read by an answer that ends a turn, and each
    /// such answer the first to read one: none left, none twice.
    fn each_answered_once(&self) {
        let events = self.events();
        let mut read = 0;
        for answer in events
            .iter()
            .filter(|e| e.role == event::ASSISTANT && e.calls.is_empty())
        {
            assert!(
                events
                    .iter()
                    .any(|e| e.role == event::USER && e.seq > read && e.seq <= answer.seen),
                "an answer to nothing: {events:#?}"
            );
            read = answer.seen;
        }
        assert!(
            events
                .iter()
                .all(|e| e.role != event::USER || e.seq <= read),
            "a message left unanswered: {events:#?}"
        );
    }
}

/// The value of measure `name` in `points`.
fn value(points: &[(String, f64)], name: &str) -> Option<f64> {
    points.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
}

impl World {
    /// Once the model has been asked `n` times: its `n`th answer, held,
    /// is being written.
    async fn asked_times(&self, n: usize) {
        let asked = self.asked.clone();
        link_test_kit::wait::eventually_async(
            &format!("the model asked {n} times"),
            Duration::from_secs(10),
            || {
                let times = asked.lock().unwrap().len();
                (times >= n).then_some(()).ok_or(times)
            },
        )
        .await;
    }
}

#[tokio::test]
async fn a_message_steers_the_answer_it_comes_during() {
    let (let_go, hold) = common::LetGo::new();
    let world = world("steer", "", hold, None).await;
    let first = world.ask("Plan my day.");
    // Sent while the first answer is held, never after a pause: on a
    // loaded machine the first's task can start after the second's.
    world.asked_times(1).await;
    let (steered, _) = world.ask("And book lunch.").await.unwrap().unwrap();
    assert_eq!(steered, Asked::Steered);
    let_go.all();
    let (answered, shown) = first.await.unwrap().unwrap();
    let Asked::Answered(answer) = answered else {
        panic!("the first message's turn answered nothing");
    };
    // The answer that started before it, then one that read it: both on
    // the first message's reply.
    assert_eq!(shown, "Answer 1\n\nAnswer 2");
    assert_eq!(answer.text, "Answer 2");
    let asked = world.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    assert!(asked[1]["messages"].to_string().contains("And book lunch."));
    let roles: Vec<String> = world
        .conversation_events()
        .into_iter()
        .map(|e| e.role)
        .collect();
    assert_eq!(roles, ["user", "user", "assistant", "assistant"]);
    // The lease is gone with the turn.
    assert!(!world.store.turn_running(&world.agent, &world.thread));
}

#[tokio::test]
async fn a_queued_message_gets_a_turn_of_its_own() {
    let world = world(
        "queue",
        "[turns]\nbusy = \"queue\"\n",
        Duration::from_millis(400),
        None,
    )
    .await;
    let first = world.ask("One.");
    // Sent once the first's turn has asked the model, never by a pause: on
    // a loaded machine the first's task can start after the second's.
    let asked = world.asked.clone();
    link_test_kit::wait::eventually_async(
        "the first message's turn",
        Duration::from_secs(5),
        || !asked.lock().unwrap().is_empty(),
    )
    .await;
    let second = world.ask("Two.");
    let (one, shown_one) = first.await.unwrap().unwrap();
    let (two, shown_two) = second.await.unwrap().unwrap();
    assert!(matches!(one, Asked::Answered(_)));
    assert!(matches!(two, Asked::Answered(_)));
    assert_eq!(shown_one, "Answer 1");
    assert_eq!(shown_two, "Answer 2");
    let events = world.conversation_events();
    assert_eq!(events.len(), 4);
    assert_eq!(
        events.iter().filter(|e| e.role == event::USER).count(),
        2,
        "{events:?}"
    );
}

/// `/chat:queue` alone, or with a message, has the owner's next message
/// wait for a turn of its own where the spec steers.
#[tokio::test]
async fn a_queue_command_has_the_next_message_wait() {
    for (command, next) in [("/chat:queue", "Two."), ("/queue Two.", "")] {
        let world = world("queue-command", "", Duration::from_millis(400), None).await;
        let first = world.ask("One.");
        let asked = world.asked.clone();
        link_test_kit::wait::eventually_async(
            "the first message's turn",
            Duration::from_secs(5),
            || !asked.lock().unwrap().is_empty(),
        )
        .await;
        let ran = world.ask(command).await.unwrap().unwrap();
        let second = match next {
            "" => ran,
            next => {
                assert!(matches!(ran.0, Asked::Answered(_)), "{command}");
                world.ask(next).await.unwrap().unwrap()
            }
        };
        let (one, _) = first.await.unwrap().unwrap();
        assert!(matches!(one, Asked::Answered(_)), "{command}");
        assert_eq!(second.1, "Answer 2", "{command}");
        let events = world.events();
        let said: Vec<&str> = events
            .iter()
            .filter(|e| e.role == event::USER)
            .map(|e| e.text.as_str())
            .collect();
        assert_eq!(said, ["One.", "Two."], "{command}");
        world.each_answered_once();
    }
}

#[tokio::test]
async fn an_interrupting_message_ends_the_answer() {
    let (let_go, hold) = common::LetGo::new();
    let world = world("interrupt", "[turns]\nbusy = \"interrupt\"\n", hold, None).await;
    let (first, written) = world.ask_observed("Write me an essay.");
    link_test_kit::eventually_async(
        "the interrupted answer's first fragment is displayed",
        Duration::from_secs(10),
        || *written.borrow() > 0,
    )
    .await;
    let second = world.ask("Stop, never mind.");
    link_test_kit::eventually_async(
        "the interrupted turn ends while its model is still held",
        Duration::from_secs(10),
        || first.is_finished(),
    )
    .await;
    let (one, shown) = first.await.unwrap().unwrap();
    // Cut short: what it wrote is kept, marked.
    let Asked::Answered(cut) = one else {
        panic!("no answer");
    };
    assert!(cut.interrupted);
    assert_eq!(shown, "Answer ");
    second.abort();
    let _ = second.await;
    let_go.all();
    let events = world.events();
    let cut = events
        .iter()
        .find(|e| e.interrupted)
        .expect("the cut answer is kept");
    assert_eq!(cut.text, "Answer ");
}

impl World {
    /// `events` kept on `thread` as a turn that crashed left them, its
    /// lease lapsed: the folder they're in.
    fn crashed(&self, thread: &Thread, events: Vec<Event>) -> std::path::PathBuf {
        let mut kept = Vec::new();
        for event in events {
            let event = Event {
                seq: kept.len() as u64 + 1,
                at: event::now(),
                ..event
            };
            self.store
                .append(&self.agent, thread, &event, &kept)
                .unwrap();
            kept.push(event);
        }
        let lease = Lease {
            holder: "1-dead".into(),
            thread: thread.name().into(),
            started: 1,
            renewed: 1,
        };
        let folder = self.root.join("files/chats").join(thread.folder());
        std::fs::write(folder.join(TURN), serde_json::to_vec(&lease).unwrap()).unwrap();
        folder
    }
}

/// A turn that called a tool and crashed is finished once, however
/// many take it at once (the harness starting, and its bridge asking):
/// its call answered as not made again, never made, and the model asked
/// once to go on. One that crashed after its answer only has its lease
/// let go: nothing more is asked or sent.
#[tokio::test]
async fn a_crashed_turn_is_finished_and_nothing_runs_twice() {
    let (url, seen) = slow_tool(Duration::from_secs(1), Duration::ZERO).await;
    let world = world("recover", "", Duration::ZERO, Some(&url)).await;
    let folder = world.crashed(
        &world.thread,
        vec![
            Event::user("Look it up."),
            Event {
                role: event::ASSISTANT.into(),
                calls: vec![Call {
                    id: "c1".into(),
                    name: "slow__wait".into(),
                    arguments: json!({}),
                }],
                ..Event::default()
            },
        ],
    );
    let answered = Thread::new("telegram:43").unwrap();
    let done = world.crashed(
        &answered,
        vec![
            Event::user("Hi."),
            Event {
                role: event::ASSISTANT.into(),
                text: "Hello.".into(),
                ..Event::default()
            },
        ],
    );
    assert!(!world.store.turn_running(&world.agent, &world.thread));
    assert_eq!(world.store.lapsed_turns(&world.agent).len(), 2);

    // Two takers at once, sharing the store: finished, and its answer
    // handed over once, to the agent's bridge, to send to the chat
    // nobody was waiting on.
    let other = Arc::new(Harness::new(world.agents.clone(), world.store.clone()).unwrap());
    let (one, two) = tokio::join!(
        world.harness.recovered(&world.agent),
        other.recovered(&world.agent)
    );
    let recovered: Vec<_> = one.into_iter().chain(two).collect();
    assert_eq!(recovered.len(), 1, "{recovered:?}");
    assert_eq!(recovered[0].0, world.thread.name());
    assert!(!recovered[0].1.trim().is_empty());
    assert!(world.harness.recovered(&world.agent).await.is_empty());
    let events = world.conversation_events();
    // The call answered as not made again, and the model asked to go on,
    // once; the tool never called.
    assert_eq!(events[2].role, event::TOOL);
    assert_eq!(events[2].text, NOT_MADE_AGAIN);
    assert!(events[2].failed);
    assert_eq!(events[3].role, event::ASSISTANT);
    assert_eq!(events.len(), 4, "{events:#?}");
    assert_eq!(world.asked.lock().unwrap().len(), 1);
    assert_eq!(seen.lock().unwrap().0, 0);
    assert!(!folder.join(TURN).exists());
    // The one that had answered: as it was, its lease gone.
    assert_eq!(world.store.read(&world.agent, &answered).unwrap().len(), 2);
    assert!(!done.join(TURN).exists());
    assert!(world.store.lapsed_turns(&world.agent).is_empty());
}

/// The bridge of the harness on `port`, as Link's connections carry it.
fn bridge(port: u16) -> link_connection_contract::Bridge {
    link_connection_contract::Bridge {
        agent: "main".into(),
        endpoint: common::acp(port),
        key: "test-key".to_string().into(),
        settings: Value::Null.into(),
        ..Default::default()
    }
}

/// A recovered answer is handed over until the bridge says it was sent.
#[tokio::test]
async fn a_recovered_answer_is_kept_until_its_sent() {
    let world = world("two-phase", "", Duration::ZERO, None).await;
    world.crashed(&world.thread, vec![Event::user("Still there?")]);
    let server = Arc::new(Server::new(
        Harness::new(world.agents.clone(), world.store.clone()).unwrap(),
        world.agent.clone(),
    ));
    let bridge = bridge(common::serve(server::router(server)).await);
    let answers = link_connection_contract::recovered(&bridge).await.unwrap();
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0].thread, "telegram:42");
    assert_eq!(answers[0].text, "Answer 1");
    let id = answers[0].id.clone().unwrap();
    // Not sent yet: handed over again.
    assert_eq!(
        link_connection_contract::recovered(&bridge).await.unwrap(),
        answers
    );
    // Sent: let go; an id that isn't one is passed over.
    let done = vec![id, "telegram:42/9".into(), "nonsense".into()];
    link_connection_contract::recovered_done(&bridge, done)
        .await
        .unwrap();
    assert!(
        link_connection_contract::recovered(&bridge)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(world.asked.lock().unwrap().len(), 1);
}

/// An answer whose caller went before it was told it (its bridge
/// stopped) is handed over at recover, as a crashed turn's is, once; one
/// its caller took isn't. The model is asked once.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn an_answer_whose_caller_went_is_handed_over() {
    let (let_go, hold) = common::LetGo::new();
    let world = world("untaken", "", hold, None).await;
    let_go.up_to(1);
    let server = Arc::new(Server::new(
        Harness::new(world.agents.clone(), world.store.clone()).unwrap(),
        world.agent.clone(),
    ));
    let port = common::serve(server::router(server.clone())).await;
    let bridge = bridge(port);
    let ask = |thread: &'static str| {
        let asking = common::Asking {
            key: "test-key",
            ..Default::default()
        };
        common::ask_as(port, thread, "Hi.", asking)
    };
    // Taken: nothing to hand over.
    assert_eq!(ask("telegram:41").await.unwrap(), "Answer 1");
    assert!(
        link_connection_contract::recovered(&bridge)
            .await
            .unwrap()
            .is_empty()
    );
    // Gone mid-turn, its answer held: each answer handed over.
    for (n, thread) in [(2, "telegram:42"), (3, "telegram:43")] {
        let gone = tokio::spawn(ask(thread));
        world.asked_times(n).await;
        gone.abort();
    }
    // The server knows they went, or their answers would read as told.
    link_test_kit::wait::eventually_async(
        "the callers' connections closed",
        Duration::from_secs(10),
        || {
            let open = server.connections();
            (open == 0).then_some(()).ok_or(open)
        },
    )
    .await;
    let_go.all();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let answers = loop {
        let answers = link_connection_contract::recovered(&bridge).await.unwrap();
        if answers.len() == 2 {
            break answers;
        }
        assert!(std::time::Instant::now() < deadline, "{answers:?}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let mut threads: Vec<&str> = answers.iter().map(|a| a.thread.as_str()).collect();
    threads.sort();
    assert_eq!(threads, ["telegram:42", "telegram:43"]);
    let done = answers.iter().filter_map(|a| a.id.clone()).collect();
    link_connection_contract::recovered_done(&bridge, done)
        .await
        .unwrap();
    assert!(
        link_connection_contract::recovered(&bridge)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(world.asked.lock().unwrap().len(), 3);
}

/// In queue mode, a message that comes after the turn's last answer,
/// while it lets its tools go, is answered once, in a turn of its own.
#[tokio::test]
async fn a_message_after_the_last_answer_is_answered_once() {
    // Its tool's session ends once the test lets it go.
    let (let_go, hold) = common::LetGo::new();
    let (url, _) = slow_tool(Duration::ZERO, hold).await;
    let world = world(
        "after",
        "[turns]\nbusy = \"queue\"\n",
        Duration::ZERO,
        Some(&url),
    )
    .await;
    let first = world.ask("One.");
    // Sent once its last answer is written, while it lets its tool go.
    let events = || world.events();
    link_test_kit::wait::eventually_async(
        "the first turn's answer",
        Duration::from_secs(10),
        || {
            events()
                .iter()
                .any(|e| e.role == event::ASSISTANT && e.calls.is_empty())
        },
    )
    .await;
    let second = world.ask("Two.");
    link_test_kit::wait::eventually_async(
        "the second message written",
        Duration::from_secs(10),
        || {
            events()
                .iter()
                .any(|e| e.role == event::USER && e.text == "Two.")
        },
    )
    .await;
    let_go.all();
    let (one, shown_one) = first.await.unwrap().unwrap();
    let (two, shown_two) = second.await.unwrap().unwrap();
    assert!(matches!(one, Asked::Answered(_)));
    assert!(matches!(two, Asked::Answered(_)));
    assert_eq!(shown_one, "Answer 2");
    assert_eq!(shown_two, "Answer 4");
    assert_eq!(world.asked.lock().unwrap().len(), 4);
    world.each_answered_once();
}

/// A message steered into a turn's last round is answered all the same,
/// by the same turn, measured as one.
#[tokio::test]
async fn a_message_steered_into_the_last_round_is_answered() {
    let (let_go, hold) = common::LetGo::new();
    let world = world("last-round", "[turns]\nrounds = 1\n", hold, None).await;
    let first = world.ask("Plan my day.");
    world.asked_times(1).await;
    let (steered, _) = world.ask("And book lunch.").await.unwrap().unwrap();
    assert_eq!(steered, Asked::Steered);
    // Into the second answer, the last round, while it's held.
    let_go.up_to(1);
    world.asked_times(2).await;
    let (steered, _) = world.ask("And dinner.").await.unwrap().unwrap();
    assert_eq!(steered, Asked::Steered);
    let_go.all();
    let (answered, _) = first.await.unwrap().unwrap();
    let Asked::Answered(answer) = answered else {
        panic!("the first message's turn answered nothing");
    };
    assert_eq!(answer.text, "Answer 3");
    let asked = world.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 3);
    assert!(asked[2]["messages"].to_string().contains("And dinner."));
    world.each_answered_once();
    assert!(!world.store.turn_running(&world.agent, &world.thread));
    // One turn, under the agent's ID.
    let measured = world.measured(1).await;
    assert_eq!(measured.len(), 1, "{measured:?}");
    assert_eq!(measured[0].0, world.agent.id());
    assert_eq!(value(&measured[0].1, "link.turns.sum"), Some(1.0));
}

/// A turn that fails is measured too, as failed.
#[tokio::test]
async fn a_failed_turn_is_measured() {
    let world = world("failed", "", Duration::ZERO, None).await;
    // Instructions that aren't text: refused before the model is asked.
    std::fs::create_dir_all(world.root.join("files/instructions")).unwrap();
    std::fs::write(
        world.root.join("files/instructions/instruction.md"),
        [0xff, 0xfe, 0xfd],
    )
    .unwrap();
    assert!(world.ask("Hi.").await.unwrap().is_err());
    assert!(world.asked.lock().unwrap().is_empty());
    let measured = world.measured(1).await;
    assert_eq!(measured.len(), 1, "{measured:?}");
    let points = &measured[0].1;
    assert_eq!(value(points, "link.turns.sum"), Some(1.0));
    assert_eq!(value(points, "link.errors.sum"), Some(1.0));
    assert_eq!(value(points, "link.time.turn.own"), None);
}

/// How long a lease lasts is the agent's spec's: one renewed half a
/// minute ago holds under Link's minute, and lapsed under twenty seconds.
#[tokio::test]
async fn a_lease_lasts_as_the_spec_says() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for (name, own, lapsed) in [
        ("lapse-link", "", false),
        (
            "lapse-short",
            "[turns]\nrenew = \"5s\"\nlapse = \"20s\"\n",
            true,
        ),
    ] {
        let world = world(name, own, Duration::ZERO, None).await;
        world
            .store
            .append(
                &world.agent,
                &world.thread,
                &Event {
                    seq: 1,
                    ..Event::user("Hi.")
                },
                &[],
            )
            .unwrap();
        let lease = Lease {
            holder: "1-quiet".into(),
            thread: world.thread.name().into(),
            started: now - 30,
            renewed: now - 30,
        };
        let folder = world.root.join("files/chats/telegram-42");
        std::fs::write(folder.join(TURN), serde_json::to_vec(&lease).unwrap()).unwrap();
        assert_eq!(
            world.store.turn_running(&world.agent, &world.thread),
            !lapsed,
            "{name}"
        );
        assert_eq!(
            world.store.lapsed_turns(&world.agent).len(),
            usize::from(lapsed),
            "{name}"
        );
    }
}

/// Over ACP, a steered message's prompt answers at once, saying so; a
/// bridge reads it as such.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_steered_reply_is_a_header() {
    let world = world("header", "", Duration::from_millis(600), None).await;
    let server = Arc::new(Server::new(
        Harness::new(
            {
                let mut agents = Agents::default();
                agents.add(
                    world.agent.clone(),
                    world.root.join("home"),
                    common::flat(world.root.join("files")),
                );
                agents
            },
            world.store.clone(),
        )
        .unwrap(),
        world.agent.clone(),
    ));
    let port = common::serve(server::router(server)).await;
    let endpoint = common::acp(port);
    let bridge = link_connection_contract::Bridge {
        agent: "main".into(),
        endpoint,
        key: "test-key".to_string().into(),
        settings: Value::Null.into(),
        ..Default::default()
    };
    let first = {
        let bridge = bridge.duplicate();
        tokio::spawn(async move {
            let (progress, _) = tokio::sync::watch::channel(String::new());
            link_connection_contract::ask_for(&bridge, "telegram:42", "One.", &progress).await
        })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (progress, _) = tokio::sync::watch::channel(String::new());
    let second = link_connection_contract::ask_for(&bridge, "telegram:42", "Two.", &progress)
        .await
        .unwrap();
    assert_eq!(second, link_connection_contract::Asked::Steered);
    assert_eq!(
        first.await.unwrap().unwrap(),
        link_connection_contract::Asked::Answer("Answer 1\n\nAnswer 2".into())
    );
}

/// A turn dropped mid-way (its request gone) stops renewing its lease:
/// it lapses, for the next to take it to finish. Its measures are kept,
/// as failed.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_dropped_turn_lets_its_lease_lapse() {
    let world = world(
        "dropped",
        "[turns]\nrenew = \"1s\"\nlapse = \"3s\"\n",
        Duration::from_secs(30),
        None,
    )
    .await;
    let first = world.ask("Write me an essay.");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(world.store.turn_running(&world.agent, &world.thread));
    first.abort();
    let measured = world.measured(1).await;
    assert_eq!(value(&measured[0].1, "link.errors.sum"), Some(1.0));
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!world.store.turn_running(&world.agent, &world.thread));
    assert_eq!(world.store.lapsed_turns(&world.agent).len(), 1);
}

/// A call takes as long as its spec lets it, then is cancelled, and the
/// model told so.
#[tokio::test]
async fn a_call_takes_as_long_as_its_spec_says() {
    let (url, seen) = slow_tool(Duration::from_secs(30), Duration::ZERO).await;
    let world = world(
        "timeout",
        "[tool.slow]\ntimeout = \"1s\"\n\n[connections]\nask_wait = \"1s\"\n",
        Duration::ZERO,
        Some(&url),
    )
    .await;
    let started = Instant::now();
    let (asked, _) = world.ask("Wait twice.").await.unwrap().unwrap();
    assert!(matches!(asked, Asked::Answered(_)));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(*seen.lock().unwrap(), (2, 2));
    let events = world.events();
    let answers: Vec<&Event> = events.iter().filter(|e| e.role == event::TOOL).collect();
    assert_eq!(answers.len(), 2);
    for answer in answers {
        assert!(answer.failed);
        // Its own second and the second it may wait to be allowed.
        assert_eq!(answer.text, "It took longer than 2s, and was cancelled.");
    }
}

/// A call cancelled names its own request, even when its server ended
/// the session it was first sent in and it was sent again in a new one.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_cancel_names_the_call_it_cancels() {
    // The calls that waited, by id, and the ids cancelled.
    type Seen = Arc<Mutex<(Vec<Value>, Vec<Value>, bool)>>;
    let seen: Seen = Arc::default();
    let app = Router::new()
        .route(
            "/mcp",
            post(
                |State(seen): State<Seen>, Json(message): Json<Value>| async move {
                    let id = message["id"].clone();
                    match message["method"].as_str().unwrap_or_default() {
                        "initialize" => {
                            let answer = json!({"jsonrpc": "2.0", "id": id,
                                "result": {"protocolVersion": "2025-06-18",
                                "capabilities": {"tools": {}},
                                "serverInfo": {"name": "slow", "version": "1"}}});
                            ([("mcp-session-id", "s1")], Json(answer)).into_response()
                        }
                        "tools/list" => Json(json!({"jsonrpc": "2.0", "id": id,
                            "result": {"tools": [{"name": "wait", "description": "Waits.",
                            "inputSchema": {"type": "object"}}]}}))
                        .into_response(),
                        "tools/call" => {
                            // The first call finds its session gone.
                            let gone = !std::mem::replace(&mut seen.lock().unwrap().2, true);
                            if gone {
                                return axum::http::StatusCode::NOT_FOUND.into_response();
                            }
                            seen.lock().unwrap().0.push(id);
                            tokio::time::sleep(Duration::from_secs(30)).await;
                            axum::http::StatusCode::ACCEPTED.into_response()
                        }
                        "notifications/cancelled" => {
                            let cancelled = message["params"]["requestId"].clone();
                            seen.lock().unwrap().1.push(cancelled);
                            axum::http::StatusCode::ACCEPTED.into_response()
                        }
                        _ => axum::http::StatusCode::ACCEPTED.into_response(),
                    }
                },
            )
            .delete(|| async { axum::http::StatusCode::OK }),
        )
        .with_state(seen.clone());
    let url = format!("http://127.0.0.1:{}/mcp", common::serve(app).await);
    let world = world(
        "cancel-ids",
        "[tool.slow]\ntimeout = \"1s\"\n\n[connections]\nask_wait = \"1s\"\n",
        Duration::ZERO,
        Some(&url),
    )
    .await;
    let (asked, _) = world.ask("Wait twice.").await.unwrap().unwrap();
    assert!(matches!(asked, Asked::Answered(_)));
    let (mut waited, mut cancelled, _) = seen.lock().unwrap().clone();
    let key = |v: &Value| v.as_u64();
    waited.sort_by_key(key);
    cancelled.sort_by_key(key);
    assert_eq!(waited.len(), 2, "{waited:?}");
    assert_eq!(cancelled, waited);
}

/// A harness stopping lets go of the lease of a turn still running: the
/// next process takes the conversation at once, never waiting out the
/// lapse of a turn the stop cut off.
#[tokio::test]
async fn a_stop_lets_go_of_a_turn_it_cuts_off() {
    let (let_go, hold) = common::LetGo::new();
    let world = world("let-go", "", hold, None).await;
    let first = world.ask("Plan my day.");
    world.asked_times(1).await;
    assert!(world.store.turn_running(&world.agent, &world.thread));
    // Another process, as the next start would be, finds it held.
    let other = Files::new(world.agents.clone());
    assert_eq!(
        other
            .take_turn(&world.agent, &world.thread, "next")
            .unwrap(),
        Took::Held
    );
    assert_eq!(world.harness.let_go(), 1);
    assert_ne!(
        other
            .take_turn(&world.agent, &world.thread, "next")
            .unwrap(),
        Took::Held,
        "the conversation stayed held after the stop let it go"
    );
    let_go.all();
    let _ = first.await;
}
