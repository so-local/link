//! A fallback model, in process, against fakes of both: with the first
//! model's runtime off, a turn is answered by the second, and so are the
//! turns while it's remembered off, then by the first again once it's
//! back; a prompt past the first's window goes to
//! the second, never cut; a hosted first answering busy or overloaded
//! (429, a 5xx) has the second answer that turn, and is tried again the
//! next; each answer says which and why, and the turn's measures count
//! it.

use crate::common;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use link_files::reading::ContentDigest;
use link_harness::{
    agent::{Agent, Agents, Thread},
    config::Config,
    event::{self, Event},
    store::{Files, Store},
    turn::{Asked, Harness},
};
use link_harness_contract::harness::Model;
use link_harness_wire::evidence::{
    Phase,
    index::{self, Located},
};
use link_model_contract::{Facts, Known};
use link_test_kit::TempDir;
use serde_json::json;

/// A model that answers `words`, streamed, and keeps what it was asked.
async fn model(words: &'static str) -> (u16, common::Asked) {
    common::fake_model_with(Router::new(), Duration::ZERO, move |_, _| {
        vec![json!({"id": "g", "model": words, "choices": [{"delta": {"content": words}}]})]
    })
    .await
}

/// A port nothing listens on: a runtime that's off.
async fn off_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

fn local(name: &str, port: u16) -> Model {
    Model {
        provider: "local".into(),
        name: Some(name.into()),
        base_url: Some(format!("http://127.0.0.1:{port}")),
        ..Model::default()
    }
}

/// What's known of `name`: a window of `window` tokens.
fn facts(name: &str, window: u32) -> Known {
    Known {
        name: name.into(),
        facts: Facts {
            window: Some(window),
            answer: Some(256),
            ..Facts::default()
        },
        yours: vec!["window".into()],
    }
}

struct World {
    _root: TempDir,
    agent: Agent,
    thread: Thread,
    harness: Arc<Harness>,
    store: Arc<Files>,
    measures: std::path::PathBuf,
}

/// How long a first model found off is left alone, here.
const OFF_FOR: Duration = Duration::from_millis(1500);

/// Link Harness for `main`: its model `first` (`first_window` tokens)
/// and its fallback `second`, a big window.
fn world(name: &str, first: Model, first_window: u32, second: Model) -> World {
    let root = TempDir::new(&format!("fallback-{name}"));
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(files));
    let (spec, defaults) = common::never_compacting();
    let store = Arc::new(Files::new(agents.clone()));
    store
        .set_config(
            &agent,
            &Config {
                model_facts: Some(facts(first.name.as_deref().unwrap(), first_window)),
                fallback_facts: Some(facts(second.name.as_deref().unwrap(), 100_000)),
                model: first,
                fallback: Some(second),
                defaults,
                spec: Some(spec),
                ..Config::default()
            },
        )
        .unwrap();
    let measures = root.join("measures.jsonl");
    World {
        agent,
        thread: Thread::new("telegram:42").unwrap(),
        harness: Arc::new(
            Harness::new(agents, store.clone())
                .unwrap()
                .with_measures(measures.clone())
                .with_off_for(OFF_FOR),
        ),
        store,
        measures,
        _root: root,
    }
}

impl World {
    /// `text` asked: its answer, and what was shown.
    async fn ask(&self, text: &str) -> (Event, String) {
        let (pieces, mut received) = tokio::sync::mpsc::channel::<String>(64);
        let shown = tokio::spawn(async move {
            let mut shown = String::new();
            while let Some(piece) = received.recv().await {
                shown.push_str(&piece);
            }
            shown
        });
        let asked = self
            .harness
            .ask(&self.agent, &self.thread, Event::user(text), pieces, None)
            .await
            .unwrap();
        let Asked::Answered(answer) = asked else {
            panic!("not answered: {asked:?}");
        };
        (*answer, shown.await.unwrap())
    }

    /// The fallback measures of its first `turns` turns, in order: off,
    /// window and overloaded.
    async fn fallbacks(&self, turns: usize) -> Vec<(Option<f64>, Option<f64>, Option<f64>)> {
        let lines = common::measures(&self.measures, turns).await;
        let mut reports: Vec<_> = lines
            .iter()
            .flat_map(link_measure_contract::client::read_otlp)
            .collect();
        // In the order the turns started: each turn's are written off its
        // answer's way, so a later turn's may be written first.
        reports.sort_by_key(|report| report.points.iter().filter_map(|p| p.start).min());
        reports
            .into_iter()
            .map(|report| {
                let value = |name: &str| {
                    report
                        .points
                        .iter()
                        .find(|p| p.measure.name == name)
                        .map(|p| p.value)
                };
                (
                    value("link.fallbacks.off.sum"),
                    value("link.fallbacks.window.sum"),
                    value("link.fallbacks.overloaded.sum"),
                )
            })
            .collect()
    }
}

#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn the_fallback_answers_while_the_first_is_off_and_the_first_again_after() {
    let port = off_port().await;
    let (second_port, second_asked) = model("Hosted.").await;
    let world = world(
        "off",
        local("qwen3", port),
        32_000,
        local("hosted", second_port),
    );
    // Each model's own instructions, in its folder.
    let models = world._root.join("files/instructions/models/local");
    for (name, said) in [("qwen3", "Think in Qwen."), ("hosted", "Think hosted.")] {
        std::fs::create_dir_all(models.join(name)).unwrap();
        std::fs::write(models.join(name).join("instruction.md"), said).unwrap();
    }
    let system = |asked: &common::Asked, n: usize| {
        asked.lock().unwrap()[n]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // Off: the second answers, and says so.
    let (answer, shown) = world.ask("Hi.").await;
    assert_eq!(shown, "Hosted.");
    assert_eq!(answer.fallback.as_deref(), Some("off"));
    assert_eq!(second_asked.lock().unwrap().len(), 1);
    // Asked as itself: its own name.
    assert_eq!(second_asked.lock().unwrap()[0]["model"], "hosted");
    // Sent its own model's instructions, never the first's.
    let sent = system(&second_asked, 0);
    assert!(
        sent.contains("\n\nThink hosted.\n\n# Where you are"),
        "{sent}"
    );
    assert!(!sent.contains("Think in Qwen."), "{sent}");

    // Back, on its own port.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let first_asked = common::Asked::default();
    let kept = first_asked.clone();
    let chat = move |axum::Json(body): axum::Json<serde_json::Value>| {
        let kept = kept.clone();
        async move {
            kept.lock().unwrap().push(body);
            let event = json!({"id": "g", "choices": [{"delta": {"content": "Local."}}]});
            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from(format!(
                    "data: {event}\n\ndata: [DONE]\n\n"
                )))
                .unwrap()
        }
    };
    let app = Router::new().route("/v1/chat/completions", axum::routing::post(chat));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    // Found off moments ago: the second answers at once, the first not
    // asked.
    let (answer, shown) = world.ask("Still?").await;
    assert_eq!(shown, "Hosted.");
    assert_eq!(answer.fallback.as_deref(), Some("off"));
    assert!(first_asked.lock().unwrap().is_empty());
    assert_eq!(second_asked.lock().unwrap().len(), 2);

    // Left alone long enough: the next turn is the first's.
    tokio::time::sleep(OFF_FOR).await;
    let (answer, shown) = world.ask("Again.").await;
    assert_eq!(shown, "Local.");
    assert_eq!(answer.fallback, None);
    assert_eq!(first_asked.lock().unwrap().len(), 1);
    assert_eq!(second_asked.lock().unwrap().len(), 2);
    let sent = system(&first_asked, 0);
    assert!(
        sent.contains("\n\nThink in Qwen.\n\n# Where you are"),
        "{sent}"
    );
    assert!(!sent.contains("Think hosted."), "{sent}");

    // Each answer kept says which; the measures count the first turns'.
    let answers: Vec<Option<String>> = world
        .store
        .read(&world.agent, &world.thread)
        .unwrap()
        .into_iter()
        .filter(|e| e.role == event::ASSISTANT)
        .map(|e| e.fallback)
        .collect();
    assert_eq!(
        answers,
        [Some("off".to_string()), Some("off".to_string()), None]
    );
    assert_eq!(
        world.fallbacks(3).await,
        [
            (Some(1.0), None, None),
            (Some(1.0), None, None),
            (None, None, None)
        ]
    );
}

#[tokio::test]
async fn a_prompt_past_the_first_window_goes_to_the_fallback_never_cut() {
    let (first_port, first_asked) = model("Local.").await;
    let (second_port, second_asked) = model("Hosted.").await;
    let world = world(
        "window",
        local("qwen3", first_port),
        2_000,
        local("hosted", second_port),
    );

    // A short message fits the first.
    let (answer, shown) = world.ask("Hi.").await;
    assert_eq!((shown.as_str(), answer.fallback), ("Local.", None));

    // One past its window: the second, sent the whole of it.
    let long = "word ".repeat(4_000);
    let (answer, shown) = world.ask(&long).await;
    assert_eq!(shown, "Hosted.");
    assert_eq!(answer.fallback.as_deref(), Some("window"));
    assert_eq!(first_asked.lock().unwrap().len(), 1);
    let sent = second_asked.lock().unwrap()[0].to_string();
    assert!(sent.contains(long.trim_end()), "cut: {} bytes", sent.len());

    assert_eq!(
        world.fallbacks(2).await,
        [(None, None, None), (None, Some(1.0), None)]
    );
}

#[tokio::test]
async fn without_a_fallback_an_off_model_fails_the_turn_as_before() {
    let port = off_port().await;
    let root = TempDir::new("fallback-none");
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(files));
    let store = Arc::new(Files::new(agents.clone()));
    store
        .set_config(
            &agent,
            &Config {
                model: local("qwen3", port),
                ..Config::default()
            },
        )
        .unwrap();
    let harness = Harness::new(agents, store).unwrap();
    let (pieces, _received) = tokio::sync::mpsc::channel::<String>(64);
    let thread = Thread::new("telegram:42").unwrap();
    let failed = harness
        .ask(&agent, &thread, Event::user("Hi."), pieces, None)
        .await
        .unwrap_err();
    assert!(
        format!("{failed:#}").contains("reaching the model"),
        "{failed:#}"
    );
}

/// Its spec turning the fallback off (`[model] fallback = false`): an
/// off model fails the turn, as with none, and the fallback is never
/// asked.
#[tokio::test]
async fn a_spec_turning_the_fallback_off_fails_the_turn_as_without_one() {
    let port = off_port().await;
    let (second_port, second_asked) = model("Hosted.").await;
    let world = world(
        "spec-off",
        local("qwen3", port),
        32_000,
        local("hosted", second_port),
    );
    let mut config = world.store.config(&world.agent).unwrap();
    let spec = config.spec.as_mut().unwrap();
    spec.model.get_or_insert_default().fallback = Some(false);
    world.store.set_config(&world.agent, &config).unwrap();
    let (pieces, _received) = tokio::sync::mpsc::channel::<String>(64);
    let failed = world
        .harness
        .ask(
            &world.agent,
            &world.thread,
            Event::user("Hi."),
            pieces,
            None,
        )
        .await
        .unwrap_err();
    assert!(
        format!("{failed:#}").contains("reaching the model"),
        "{failed:#}"
    );
    assert!(second_asked.lock().unwrap().is_empty());
}

/// Anthropic's Messages API on a port of its own: each request kept,
/// answered `busy` with Anthropic's error while it has statuses left
/// (one a request), then `words`, streamed.
async fn anthropic(busy: &'static [u16], words: &'static str) -> (u16, common::Asked) {
    let asked = common::Asked::default();
    let kept = asked.clone();
    let messages = move |axum::Json(body): axum::Json<serde_json::Value>| {
        let kept = kept.clone();
        async move {
            let n = {
                let mut kept = kept.lock().unwrap();
                kept.push(body);
                kept.len()
            };
            let response = axum::response::Response::builder();
            if let Some(&status) = busy.get(n - 1) {
                let error = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
                return response
                    .status(status)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(error.to_string()))
                    .unwrap();
            }
            let events = [
                json!({"type": "message_start", "message": {"id": "m", "model": words, "usage": {"input_tokens": 1}}}),
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": words}}),
                json!({"type": "message_stop"}),
            ];
            let body: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
            response
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from(body))
                .unwrap()
        }
    };
    let app = Router::new().route("/v1/messages", axum::routing::post(messages));
    (common::serve(app).await, asked)
}

fn hosted(name: &str, port: u16) -> Model {
    Model {
        actions_url: None,
        provider: "anthropic".into(),
        api_key: "sk-ant-placeholder".to_string().into(),
        name: Some(name.into()),
        base_url: Some(format!("http://127.0.0.1:{port}")),
    }
}

#[tokio::test]
async fn the_fallback_answers_a_turn_the_first_is_overloaded_for_and_the_next_tries_it() {
    // Overloaded (529), rate-limited (429), unavailable (503), then well.
    let (first_port, first_asked) = anthropic(&[529, 429, 503], "Opus.").await;
    let (second_port, second_asked) = anthropic(&[], "Sonnet.").await;
    let world = world(
        "overloaded",
        hosted("claude-opus-5-5", first_port),
        100_000,
        hosted("claude-sonnet-5-5", second_port),
    );
    let models = world._root.join("files/instructions/models/anthropic");
    for (model, words) in [
        ("claude-opus-5-5", "Think in Opus."),
        ("claude-sonnet-5-5", "Think in Sonnet."),
    ] {
        std::fs::create_dir_all(models.join(model)).unwrap();
        std::fs::write(models.join(model).join("instruction.md"), words).unwrap();
    }

    let mut answers = Vec::new();
    for (n, text) in ["Hi.", "Again.", "Still?"].into_iter().enumerate() {
        // Busy: the second answers that turn, and says why.
        let (answer, shown) = world.ask(text).await;
        assert_eq!(shown, "Sonnet.");
        assert_eq!(answer.fallback.as_deref(), Some("overloaded"));
        // Never left alone: each turn tried the first.
        assert_eq!(first_asked.lock().unwrap().len(), n + 1);
        assert_eq!(second_asked.lock().unwrap().len(), n + 1);
        assert_eq!(
            second_asked.lock().unwrap()[n]["model"],
            "claude-sonnet-5-5"
        );
        answers.push(answer);
    }

    // Well again: the next turn is the first's.
    let (answer, shown) = world.ask("And now?").await;
    assert_eq!(shown, "Opus.");
    assert_eq!(answer.fallback, None);
    assert_eq!(first_asked.lock().unwrap().len(), 4);
    assert_eq!(second_asked.lock().unwrap().len(), 3);
    answers.push(answer);

    let overloaded = (None, None, Some(1.0));
    assert_eq!(
        world.fallbacks(4).await,
        [overloaded, overloaded, overloaded, (None, None, None)]
    );

    let state = world
        .harness
        .exported_state(&world.agent, &[link_agent_state_protocol::Kind::Sessions])
        .unwrap();
    let session = state
        .sessions
        .unwrap()
        .into_iter()
        .find(|session| {
            session
                .evidence
                .as_ref()
                .is_some_and(|native| native.conversation.thread == world.thread.name())
        })
        .unwrap();
    let native = session.evidence.unwrap();
    native.validate(session.updates.len()).unwrap();
    let trace = index::index_with_responses(
        native.events.iter().filter_map(|event| {
            event.evidence.as_ref().map(|evidence| Located {
                seq: event.seq,
                evidence,
            })
        }),
        native
            .events
            .iter()
            .filter_map(|event| event.request.map(|request| (event.seq, request))),
    )
    .unwrap();
    let requests: Vec<_> = trace.requests.iter().collect();
    let first = first_asked.lock().unwrap();
    let second = second_asked.lock().unwrap();
    let observed = [
        (&first[0], "Think in Opus.", "Think in Sonnet."),
        (&second[0], "Think in Sonnet.", "Think in Opus."),
        (&first[1], "Think in Opus.", "Think in Sonnet."),
        (&second[1], "Think in Sonnet.", "Think in Opus."),
        (&first[2], "Think in Opus.", "Think in Sonnet."),
        (&second[2], "Think in Sonnet.", "Think in Opus."),
        (&first[3], "Think in Opus.", "Think in Sonnet."),
    ];
    assert_eq!(requests.len(), observed.len());
    for ((_, request), (body, own, other)) in requests.iter().zip(observed) {
        // These are the actual Messages API bodies received by the fakes.
        assert_eq!(request.provider, "anthropic");
        assert_eq!(request.model.as_deref(), body["model"].as_str());
        assert_eq!(request.phase, Phase::Turn);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        let sent = system[0]["text"].as_str().unwrap();
        assert!(sent.contains(own), "{sent}");
        assert!(!sent.contains(other), "{sent}");
        assert_eq!(request.system, ContentDigest::of(sent.as_bytes()));
    }
    assert_ne!(requests[0].1.system, requests[1].1.system);
    assert_eq!(trace.responses.len(), answers.len());
    for (turn, answer) in answers.iter().enumerate() {
        let producing = *requests[if turn < 3 { turn * 2 + 1 } else { 6 }].0;
        assert_eq!(answer.request, Some(producing));
        assert_eq!(trace.responses.get(&producing), Some(&answer.seq));
        let exported = native
            .events
            .iter()
            .find(|event| event.seq == answer.seq)
            .unwrap();
        assert_eq!(exported.role, event::ASSISTANT);
        assert_eq!(exported.request, answer.request);
        if turn < 3 {
            let rejected = *requests[turn * 2].0;
            assert_ne!(answer.request, Some(rejected));
            assert!(!trace.responses.contains_key(&rejected));
        }
    }
}

#[tokio::test]
async fn without_a_fallback_an_overloaded_model_fails_the_turn_as_before() {
    let (port, _asked) = anthropic(&[529], "Opus.").await;
    let root = TempDir::new("fallback-overloaded-none");
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::with_identity("main", "aaaaaaaaaaa1", "alice@box").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(files));
    let store = Arc::new(Files::new(agents.clone()));
    store
        .set_config(
            &agent,
            &Config {
                model: hosted("claude-opus-5-5", port),
                ..Config::default()
            },
        )
        .unwrap();
    let harness = Harness::new(agents, store).unwrap();
    let (pieces, _received) = tokio::sync::mpsc::channel::<String>(64);
    let thread = Thread::new("telegram:42").unwrap();
    let failed = harness
        .ask(&agent, &thread, Event::user("Hi."), pieces, None)
        .await
        .unwrap_err();
    assert!(
        format!("{failed:#}").contains("the model answered 529 <unknown status code> Overloaded"),
        "{failed:#}"
    );
}
