//! Real harness/model/tool boundaries with deterministic providers and isolated coordination.
use crate::common;
use axum::{Json, Router, response::IntoResponse, routing::post};
use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    event::Event,
    store::{Files, Store as ConversationStore},
    tools,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_interactions::{sources, *};
use link_test_kit::{TempDir, net::Held, wait};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Serving(tokio::task::JoinHandle<()>);
impl Drop for Serving {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(app: Router) -> (String, Serving) {
    let listener = Held::loopback().listener();
    listener.set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (
        url,
        Serving(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap()
        })),
    )
}

struct World {
    _root: TempDir,
    _servers: Vec<Serving>,
    interactions: Store,
    harness: Arc<Harness>,
    agent: Agent,
    called: Arc<AtomicUsize>,
    model_requests: Arc<Mutex<Vec<Value>>>,
    release: tokio::sync::watch::Sender<bool>,
}

async fn world(hold: bool) -> World {
    let root = TempDir::new("interaction-loop");
    let interactions = Store::at(root.join("interactions"));
    let called = Arc::new(AtomicUsize::new(0));
    let (store, calls) = (interactions.clone(), called.clone());
    let tool = Router::new().route("/mcp", post(move |Json(message): Json<Value>| {
        let (store, calls) = (store.clone(), calls.clone());
        async move {
            let id = message["id"].clone();
            let result = match message["method"].as_str().unwrap_or_default() {
                "initialize" => json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"question","version":"1"}}),
                "tools/list" => json!({"tools":[{"name":"create","description":"Ask the owner", "inputSchema":QuestionProposal::schema()}]}),
                "tools/call" => {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::task::spawn_blocking(move || {
                        let meta = &message["params"]["_meta"];
                        let id = sources::create_question(&store, sources::QuestionCaller {
                            revision: meta["link.local/control_revision"].as_u64().unwrap(),
                            token: &link_secretive::Secret::new(meta["link.local/work"].as_str().unwrap().to_owned()), agent:"aaaaaaaaaaa1", service:"s1s2s3s4s5s6", label:"main",
                            session:meta["link.local/thread"].as_str().unwrap(), attempt:meta["link.local/attempt"].as_str().unwrap(),
                            call:meta["link.local/call"].as_str().unwrap()},
                            serde_json::from_value(message["params"]["arguments"].clone()).unwrap(),
                            &InteractionOpinions::default(), Duration::from_secs(30), now()).unwrap();
                        let snapshot = store.wait(&id, || false).unwrap();
                        let result = serde_json::to_value(snapshot.resolution.unwrap()).unwrap();
                        json!({"content":[{"type":"text","text":result.to_string()}], "structuredContent":result})
                    }).await.unwrap()
                }
                _ => return axum::http::StatusCode::ACCEPTED.into_response(),
            };
            ([("mcp-session-id","one")], Json(json!({"jsonrpc":"2.0","id":id,"result":result}))).into_response()
        }
    }).delete(|| async {axum::http::StatusCode::OK}));
    let (tool_url, tool_server) = serve(tool).await;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let (release, released) = tokio::sync::watch::channel(!hold);
    let model = Router::new().route("/v1/chat/completions", post(move |Json(body): Json<Value>| {
        let (log, mut released) = (log.clone(), released.clone());
        async move {
            log.lock().unwrap().push(body.clone());
            released.wait_for(|r| *r).await.unwrap();
            let has_result = body["messages"].as_array().unwrap().iter().any(|m| m["role"] == "tool");
            let delta = if has_result {json!({"content":"I received the owner's exact answer."})} else {
                json!({"tool_calls":[{"index":0,"id":"original-call","type":"function","function":{
                    "name":"question__create","arguments":json!({"title":"Report", "questions":[{"id":"focus", "prompt":"What should it emphasize?",
                        "answer_shape":{"kind":"text","max_bytes":1000,"allow_empty":false}}]}).to_string()}}]})
            };
            ([("content-type","text/event-stream")], format!("data: {}\n\ndata: [DONE]\n\n",
                json!({"id":"answer","model":"fake","choices":[{"delta":delta}]})))
        }
    }));
    let (model_url, model_server) = serve(model).await;
    let agent = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("harness"),
        common::flat(root.join("files")),
    );
    agents.allow_tools(
        &agent,
        tools::from_env([("LNK_TOOL_QUESTION".into(), format!("{tool_url}/mcp"))]),
    );
    let conversations = Arc::new(Files::new(agents.clone()));
    let (spec, defaults) = common::never_compacting();
    conversations
        .set_config(
            &agent,
            &Config {
                model: Model {
                    provider: "local".into(),
                    name: Some("fake".into()),
                    base_url: Some(model_url),
                    ..Default::default()
                },
                spec: Some(spec),
                defaults,
                ..Default::default()
            },
        )
        .unwrap();
    let harness = Arc::new(
        Harness::new(agents, conversations)
            .unwrap()
            .with_interactions(link_interactions::Host::shared(interactions.clone())),
    );
    World {
        _root: root,
        _servers: vec![tool_server, model_server],
        interactions,
        harness,
        agent,
        called,
        model_requests: requests,
        release,
    }
}

fn ask(
    w: &World,
    thread: &str,
) -> tokio::task::JoinHandle<anyhow::Result<link_harness::turn::Asked>> {
    ask_event(w, thread, Event::user("Prepare a report"))
}

fn ask_event(
    w: &World,
    thread: &str,
    message: Event,
) -> tokio::task::JoinHandle<anyhow::Result<link_harness::turn::Asked>> {
    let (harness, agent, thread) = (
        w.harness.clone(),
        w.agent.clone(),
        Thread::new(thread).unwrap(),
    );
    tokio::spawn(async move {
        let (pieces, mut received) = tokio::sync::mpsc::channel(64);
        let drain = tokio::spawn(async move { while received.recv().await.is_some() {} });
        let result = harness.ask(&agent, &thread, message, pieces, None).await;
        drain.await.unwrap();
        result
    })
}

#[tokio::test]
async fn idle_stop_is_benign_with_retained_work_coordination() {
    let w = world(false).await;
    let answer = ask_event(&w, "link:main", Event::user("/stop"))
        .await
        .unwrap()
        .unwrap();
    let link_harness::turn::Asked::Answered(answer) = answer else {
        panic!("an idle stop must answer immediately");
    };
    assert_eq!(answer.text, link_harness::commands::NOTHING_TO_STOP);
    assert!(w.model_requests.lock().unwrap().is_empty());
    let refused = ask_event(&w, "link:main", Event::user("/work stop"))
        .await
        .unwrap();
    assert!(
        refused.is_err(),
        "an explicit missing work target stays refused"
    );
    assert!(w.model_requests.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retained_child_delivery_is_included_once_after_its_parent_resumes() {
    let w = world(false).await;
    let initial = ask(&w, "link:main");
    answer_question(&w, "Use the measured cost").await;
    initial.await.unwrap().unwrap();
    control(&w, "/work pause", "pause-parent");
    let mut message = Event::user("The child measured the cost at 12 units.");
    message.from = Some("task:child".into());
    message.hops = 1;
    message.delivery = Some(TransportEventId::new("child-result-3").unwrap());
    assert!(
        ask_event(&w, "link:main", message.clone())
            .await
            .unwrap()
            .is_err()
    );
    control(&w, "/work resume", "resume-parent");
    ask_event(&w, "link:main", message.clone())
        .await
        .unwrap()
        .unwrap();
    let count = w.model_requests.lock().unwrap().len();
    let repeated = ask_event(&w, "link:main", message.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(repeated, link_harness::turn::Asked::Steered));
    assert_eq!(
        w.model_requests.lock().unwrap().len(),
        count,
        "a delivery retry cannot start another model turn"
    );
    let events = w
        .harness
        .events(&w.agent, &Thread::new("link:main").unwrap())
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.delivery == message.delivery)
            .count(),
        1
    );
    message.text = "Changed child result".into();
    assert!(ask_event(&w, "link:main", message).await.unwrap().is_err());
    assert_eq!(w.model_requests.lock().unwrap().len(), count);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_codes_and_chat_replies_resume_the_original_model_tool_call() {
    for thread in ["link:main", "telegram:42"] {
        let w = world(false).await;
        let task = ask(&w, thread);
        let snapshot =
            wait::eventually_async("the model's question", Duration::from_secs(10), || {
                w.interactions.pending(now()).unwrap().into_iter().next()
            })
            .await;
        assert_eq!(
            snapshot.question.requester.tool_call.as_deref(),
            Some("original-call")
        );
        let surface = thread.split(':').next().unwrap();
        let e = w
            .interactions
            .plan_delivery(
                &snapshot.question.id,
                thread,
                surface,
                &render::text_capabilities(),
                None,
                now(),
            )
            .unwrap();
        w.interactions
            .delivered_ids(&e.delivery.id, &["question-message".into()])
            .unwrap();
        assert!(
            link_connection_contract::questions::caught_at(
                &w.interactions,
                "main",
                thread,
                "ordinary conversation"
            )
            .is_none()
        );
        let answer = "Emphasize the measured cost, please.";
        let receipt = if surface == "link" {
            link_connection_contract::questions::caught_at(
                &w.interactions,
                "main",
                thread,
                &format!("{} {answer}", e.response_code),
            )
        } else {
            link_connection_contract::questions::caught_reply_at(
                &w.interactions,
                "main",
                thread,
                "question-message",
                answer,
                "owner-event",
            )
        };
        assert!(receipt.is_some());
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let events = w
            .harness
            .events(&w.agent, &Thread::new(thread).unwrap())
            .unwrap();
        let result = events
            .iter()
            .find(|e| e.call.as_deref() == Some("original-call"))
            .unwrap();
        assert!(result.text.contains(answer), "{}", result.text);
        assert_eq!(w.called.load(Ordering::SeqCst), 1);
        assert_eq!(w.model_requests.lock().unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_pause_finishes_the_model_response_without_dispatching_its_tools() {
    let w = world(true).await;
    let task = ask(&w, "link:main");
    wait::eventually_async(
        "the in-flight model request",
        Duration::from_secs(10),
        || !w.model_requests.lock().unwrap().is_empty(),
    )
    .await;
    owner::command(
        &w.interactions,
        &sources::owner(),
        "main",
        "link:main",
        "/work pause",
        TransportEventId::new("pause").unwrap(),
    )
    .unwrap();
    w.release.send_replace(true);
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(w.called.load(Ordering::SeqCst), 0);
    assert_eq!(
        w.interactions.works("main").unwrap()[0].state.observed,
        control::Execution::Paused
    );
    assert!(w.interactions.pending(now()).unwrap().is_empty());
}

fn control(w: &World, text: &str, event: &str) -> String {
    owner::command(
        &w.interactions,
        &sources::owner(),
        "main",
        "link:main",
        text,
        TransportEventId::new(event).unwrap(),
    )
    .unwrap()
}

async fn answer_question(w: &World, text: &str) {
    let q = wait::eventually_async("the current question", Duration::from_secs(10), || {
        w.interactions.pending(now()).unwrap().into_iter().next()
    })
    .await;
    let e = w
        .interactions
        .plan_delivery(
            &q.question.id,
            "link:main",
            "link",
            &render::text_capabilities(),
            None,
            now(),
        )
        .unwrap();
    assert!(
        link_connection_contract::questions::caught_at(
            &w.interactions,
            "main",
            "link:main",
            &format!("{} {text}", e.response_code)
        )
        .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steering_invalidates_tools_generated_from_the_old_model_context() {
    let w = world(true).await;
    let task = ask(&w, "link:main");
    wait::eventually_async("in-flight model request", Duration::from_secs(10), || {
        !w.model_requests.lock().unwrap().is_empty()
    })
    .await;
    let receipt = control(&w, "/work steer Use the corrected brief", "steer");
    assert!(receipt.contains("Accepted"));
    w.release.send_replace(true);
    answer_question(&w, "Use the cost figures").await;
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        w.called.load(Ordering::SeqCst),
        1,
        "old generated tool call was never dispatched"
    );
    let requests = w.model_requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].to_string().contains("Use the corrected brief"));
    assert!(requests[1].to_string().contains("Use the corrected brief"));
    let events = w
        .harness
        .events(&w.agent, &Thread::new("link:main").unwrap())
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.text == "Use the corrected brief")
            .count(),
        1
    );
    let input = events.iter().find_map(|e| e.input.clone()).unwrap();
    assert_eq!(
        w.interactions
            .receipts(input.as_str())
            .unwrap()
            .last()
            .unwrap()
            .stage,
        control::ReceiptStage::Included
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_input_waits_for_a_new_turn_and_is_included_once() {
    let w = world(true).await;
    let task = ask(&w, "link:main");
    wait::eventually_async("in-flight model request", Duration::from_secs(10), || {
        !w.model_requests.lock().unwrap().is_empty()
    })
    .await;
    control(&w, "/work queue Review the appendix next", "queue");
    w.release.send_replace(true);
    answer_question(&w, "Use the cost figures").await;
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        !w.model_requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.to_string().contains("Review the appendix next"))
    );
    w.harness.recover(&w.agent).await;
    assert_eq!(w.model_requests.lock().unwrap().len(), 3);
    assert!(
        w.model_requests.lock().unwrap()[2]
            .to_string()
            .contains("Review the appendix next")
    );
    w.harness.recover(&w.agent).await;
    assert_eq!(
        w.model_requests.lock().unwrap().len(),
        3,
        "recovery never repeats an included queue input"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redirect_closes_the_old_question_and_promotes_only_its_recorded_input() {
    let w = world(false).await;
    // The outbox keeps receipts for a subscriber, as the owner's bridge is.
    w.interactions.subscribe("owner-delivery:main").unwrap();
    let task = ask(&w, "link:main");
    let old = wait::eventually_async("old question", Duration::from_secs(10), || {
        w.interactions.pending(now()).unwrap().into_iter().next()
    })
    .await;
    let notice = control(
        &w,
        "/work redirect Produce a one-page summary instead",
        "redirect",
    );
    assert!(notice.contains("StopRequested"));
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let closed = w.interactions.snapshot(&old.question.id, now()).unwrap();
    assert_eq!(closed.resolution.unwrap().trigger, Trigger::Closed);
    let events = w
        .harness
        .events(&w.agent, &Thread::new("link:main").unwrap())
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.text == "Produce a one-page summary instead")
            .count(),
        1
    );
    let receipts = w.interactions.events(0).unwrap();
    assert!(receipts.iter().any(|e| {
        e.receipt
            .as_ref()
            .is_some_and(|r| r.stage == control::ReceiptStage::RedirectApplied)
    }));
    assert_eq!(w.called.load(Ordering::SeqCst), 1);
}

/// A harness answering "ok" to every turn, binding its work in a store of
/// its own, as one behind its sandbox does: no source shares the store.
async fn on_its_own_store(name: &str) -> (TempDir, Serving, Arc<Harness>, Agent) {
    let root = TempDir::new(name);
    let model = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                [("content-type", "text/event-stream")],
                format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"id":"answer","model":"fake","choices":[{"delta":{"content":"ok"}}]})
                ),
            )
        }),
    );
    let (model_url, model) = serve(model).await;
    let agent = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("harness"),
        common::flat(root.join("files")),
    );
    let conversations = Arc::new(Files::new(agents.clone()));
    let (spec, defaults) = common::never_compacting();
    conversations
        .set_config(
            &agent,
            &Config {
                model: Model {
                    provider: "local".into(),
                    name: Some("fake".into()),
                    base_url: Some(model_url),
                    ..Default::default()
                },
                spec: Some(spec),
                defaults,
                ..Default::default()
            },
        )
        .unwrap();
    let harness = Arc::new(
        Harness::new(agents, conversations)
            .unwrap()
            .with_interactions(link_interactions::Host {
                store: Store::at(root.join("own")),
                shared: false,
            }),
    );
    (root, model, harness, agent)
}

/// `message` asked on `thread`, as the harness's way in hands it.
async fn asked_on(
    harness: &Arc<Harness>,
    agent: &Agent,
    thread: &str,
    message: Event,
) -> anyhow::Result<link_harness::turn::Asked> {
    let (harness, agent) = (harness.clone(), agent.clone());
    let thread = Thread::new(thread).unwrap();
    let (pieces, mut received) = tokio::sync::mpsc::channel(64);
    let drain = tokio::spawn(async move { while received.recv().await.is_some() {} });
    let asked = harness.ask(&agent, &thread, message, pieces, None).await;
    drain.await.unwrap();
    asked
}

/// A recurring schedule fires each occurrence into a conversation of its
/// own. Behind its sandbox the harness binds work in a store of its own,
/// which the scheduler's projections never reach: each occurrence, not
/// only the first, is still admitted and answered.
#[tokio::test]
async fn each_occurrence_of_a_recurring_schedule_is_answered_in_a_store_of_its_own() {
    let (_root, _model, harness, agent) = on_its_own_store("occurrences").await;
    for at in [1_794_516_000_u64, 1_794_516_060, 1_794_516_120] {
        let mut message = Event::user("Tell me the time.");
        message.occurrence = Some(ScheduledOccurrence {
            schedule: ScheduleId("tool:every-minute".into()),
            revision: 1,
            scheduled_at: at,
        });
        let asked = asked_on(
            &harness,
            &agent,
            &format!("schedule-every-minute-{at}"),
            message,
        )
        .await;
        let Ok(link_harness::turn::Asked::Answered(answer)) = asked else {
            panic!("the occurrence at {at} wasn't answered: {asked:?}");
        };
        assert_eq!(answer.text, "ok");
    }
}

/// A subagent's first message names the conversation that started it.
/// Behind its sandbox the harness learns the subagent's lineage from it
/// in a store of its own, which the scheduler's delegation never reaches,
/// and answers it; with no parent named, the subagent is still refused.
#[tokio::test]
async fn a_subagent_is_answered_in_a_store_of_its_own_once_its_parent_is_named() {
    let (_root, _model, harness, agent) = on_its_own_store("subagents").await;
    let asked = asked_on(
        &harness,
        &agent,
        "link:main",
        Event::user("Research the deepest lake."),
    )
    .await;
    assert!(
        matches!(asked, Ok(link_harness::turn::Asked::Answered(_))),
        "{asked:?}"
    );
    let orphan = asked_on(&harness, &agent, "task-orphan", Event::user("Look it up.")).await;
    assert!(
        orphan
            .as_ref()
            .is_err_and(|e| e.to_string().contains("lineage")),
        "{orphan:?}"
    );
    let mut message = Event::user("Find the deepest lake.");
    message.parent = Some("link:main".into());
    let asked = asked_on(&harness, &agent, "task-lake", message).await;
    let Ok(link_harness::turn::Asked::Answered(answer)) = asked else {
        panic!("the subagent wasn't answered: {asked:?}");
    };
    assert_eq!(answer.text, "ok");
}
