//! Compaction end to end, in process: a conversation past its window
//! keeps going, what left it summarized in an event of its own, and
//! saved to notes first where the agent has a tool to. A fake model
//! (OpenAI's chat completions, streamed) and a fake memory (MCP over
//! HTTP, `search` and `remember`) stand in for both.

use crate::common;

mod quiet;
mod steered;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{Json, Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};
use link_harness::{
    agent::{Agent, Agents, Thread},
    compact,
    config::Config,
    event,
    model::SUMMARY_HEAD,
    store::{Files, Store},
    tools,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// A memory with `search` and `remember`: the calls it was made.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn fake_memory() -> (String, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let app = Router::new()
        .route(
            "/_link/mcp/memory-main",
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
                    let schema = json!({"type": "object", "properties": {"text": {"type": "string"}}});
                    let result = match method {
                        "tools/list" => json!({"tools": [
                            {"name": "search", "description": "Search your files.", "inputSchema": schema},
                            {"name": "remember", "description": "Add to your notes.", "inputSchema": schema}]}),
                        "tools/call" => {
                            calls.lock().unwrap().push(message["params"].clone());
                            json!({"content": [{"type": "text", "text": "Kept."}]})
                        }
                        _ => json!({}),
                    };
                    Json(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
                        .into_response()
                },
            ),
        )
        .route(
            "/_link/mcp/memory-main",
            axum::routing::delete(|| async {
                // Ending a session takes a while, as a busy tool's may.
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }),
        )
        .with_state(calls.clone());
    let port = common::serve(app).await;
    (
        format!("http://127.0.0.1:{port}/_link/mcp/memory-main"),
        calls,
    )
}

/// A model that answers each message "Answer <n>". Asked to summarize,
/// it first calls `memory__remember` and `memory__search` when it's
/// asked to save and may call, then answers with the numbers of the
/// messages it was sent: "Summary of 1, 2, 3." It keeps what it was
/// asked.
async fn fake_model(hold: common::Hold) -> (String, common::Asked) {
    let (port, asked) = common::fake_model_with(axum::Router::new(), hold, |body, n| {
        let messages = body["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        let text = last["content"].as_str().unwrap_or_default();
        let summarizing = text.starts_with("[Link: everything above");
        let may_call = body["tools"].is_array() && body["tool_choice"] != "none";
        let mut said = Vec::new();
        if summarizing && may_call && text.contains("memory__remember") {
            for (i, (name, args)) in [
                (
                    "memory__remember",
                    r#"{"text":"The owner counts messages."}"#,
                ),
                ("memory__search", r#"{"text":"anything"}"#),
            ]
            .into_iter()
            .enumerate()
            {
                said.push(json!({"id": "gen-1", "choices": [{"delta": {"tool_calls": [
                    {"index": i, "id": format!("call_{n}_{i}"), "type": "function",
                     "function": {"name": name, "arguments": args}}]}}]}));
            }
        } else {
            let answer = match last["role"] == "tool" || summarizing {
                true => {
                    let all = serde_json::to_string(messages).unwrap();
                    // What the summary before held, and the
                    // messages since.
                    let before: Vec<String> = all
                        .split_once("Summary of ")
                        .and_then(|(_, rest)| rest.split_once('.'))
                        .map(|(list, _)| list.split(", ").map(str::to_string).collect())
                        .unwrap_or_default();
                    let numbers: Vec<String> = (1..=99)
                        .map(|k| k.to_string())
                        .filter(|k| before.contains(k) || all.contains(&format!("Message {k}:")))
                        .collect();
                    format!("Summary of {}.", numbers.join(", "))
                }
                false => format!("Answer {n}"),
            };
            said.push(common::delta(json!({"content": answer})));
        }
        // A model of Link's facts' (Haiku 5.5's) counts its prompt as
        // Link estimates it, so its cost is its prompt's size's.
        let prompt = match body["model"] == "claude-haiku-5-5" {
            true => link_model_contract::context::text_tokens(&body.to_string()),
            false => 30,
        };
        said.push(
            json!({"id": "gen-1", "choices": [], "usage": {"prompt_tokens": prompt,
            "completion_tokens": 5}}),
        );
        said
    })
    .await;
    (format!("http://127.0.0.1:{port}"), asked)
}

struct World {
    root: TempDir,
    agent: Agent,
    harness: Harness,
    asked: Arc<Mutex<Vec<Value>>>,
}

/// Link Harness for `main`, with Link's spec but `own`, the tools of
/// `env`, and a conversation's share of the window of `budget` tokens.
async fn world(name: &str, own: &str, env: Vec<(String, String)>, budget: usize) -> World {
    world_with(
        name,
        own,
        env,
        budget,
        std::time::Duration::ZERO.into(),
        false,
    )
    .await
}

/// [`world`] with Link's spec but `own`, on a model of Link's facts
/// named `model` (the fake one behind it), its conversation's share of
/// the window as the spec and the facts make it.
async fn world_on(name: &str, own: &str, model: &str) -> World {
    world_of(
        name,
        own,
        vec![],
        None,
        model,
        std::time::Duration::ZERO.into(),
        false,
    )
    .await
}

/// [`world`], with a model whose every answer is held as `hold` says
/// after its first piece.
async fn world_paused(
    name: &str,
    own: &str,
    env: Vec<(String, String)>,
    budget: usize,
    hold: impl Into<common::Hold>,
) -> World {
    world_with(name, own, env, budget, hold.into(), false).await
}

/// [`world_paused`], with the fake model as its fallback behind a first
/// that's off when `first_off`.
async fn world_with(
    name: &str,
    own: &str,
    env: Vec<(String, String)>,
    budget: usize,
    hold: common::Hold,
    first_off: bool,
) -> World {
    world_of(name, own, env, Some(budget), "fake", hold, first_off).await
}

/// [`world_with`], the fake model named `model`, the conversation's
/// share of the window `budget` tokens where it's given.
async fn world_of(
    name: &str,
    own: &str,
    env: Vec<(String, String)>,
    budget: Option<usize>,
    model: &str,
    hold: common::Hold,
    first_off: bool,
) -> World {
    let root = TempDir::new(&format!("compaction-{name}"));
    let (model_url, asked) = fake_model(hold).await;
    let fake = Model {
        provider: "local".into(),
        name: Some(model.into()),
        base_url: Some(model_url),
        ..Model::default()
    };
    let (model, fallback) = match first_off {
        false => (fake, None),
        true => {
            // A port nothing listens on.
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let off = listener.local_addr().unwrap().port();
            drop(listener);
            let first = Model {
                provider: "local".into(),
                name: Some("qwen3".into()),
                base_url: Some(format!("http://127.0.0.1:{off}")),
                ..Model::default()
            };
            (first, Some(fake))
        }
    };
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let own = link_agent_spec::parse(&format!("schema = 1\n{own}")).unwrap();
    let known = link_agent_spec::Known {
        name: "main".into(),
    };
    let spec = link_agent_spec::resolve(Some(&own), &[], &known).unwrap();
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(files));
    Files::new(agents.clone())
        .set_config(
            &agent,
            &Config {
                model,
                fallback,
                defaults: spec.links(),
                spec: Some(spec.spec),
                ..Config::default()
            },
        )
        .unwrap();
    agents.allow_tools(&agent, tools::from_env(env));
    let harness = Harness::new(agents.clone(), Arc::new(Files::new(agents))).unwrap();
    let harness = match budget {
        Some(budget) => harness.with_budget(budget),
        None => harness,
    };
    World {
        root,
        agent,
        harness,
        asked,
    }
}

impl World {
    /// Sends messages `from` to `to` on `thread`, about 100 tokens each:
    /// each answered, never with a summary.
    async fn say(&self, thread: &Thread, from: usize, to: usize) {
        for n in from..=to {
            let text = format!("Message {n}: {}", "word ".repeat(80));
            let (pieces, mut heard) = tokio::sync::mpsc::channel(64);
            let answer = self
                .harness
                .turn(&self.agent, thread, &text, pieces)
                .await
                .unwrap();
            assert!(answer.text.starts_with("Answer "), "{}", answer.text);
            let mut all = String::new();
            while let Ok(piece) = heard.try_recv() {
                all += &piece;
            }
            assert!(!all.contains("Summary"), "{all}");
        }
    }

    fn folder(&self, thread: &str) -> PathBuf {
        self.root.join("files/chats").join(thread)
    }

    fn asked(&self) -> Vec<Value> {
        self.asked.lock().unwrap().clone()
    }
}

/// Each request's last message, as text.
fn last_text(request: &Value) -> String {
    let messages = request["messages"].as_array().unwrap();
    messages.last().unwrap()["content"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn summarizing(request: &Value) -> bool {
    last_text(request).starts_with("[Link: everything above")
}

/// Whether `request` asks for an answer to a message of the owner's.
fn answering(request: &Value) -> bool {
    let messages = request["messages"].as_array().unwrap();
    !summarizing(request) && messages.last().unwrap()["role"] == "user"
}

/// Whether `request` asks only for a summary, saving nothing.
fn only_summarizing(request: &Value) -> bool {
    let text = last_text(request);
    summarizing(request)
        && !text.contains("memory__remember")
        && text.ends_with(&compact::SUMMARIZE[compact::SUMMARIZE.len() - 40..])
}

/// The summaries in `folder`'s events.
fn summaries(folder: &Path) -> Vec<Value> {
    common::events(folder)
        .into_iter()
        .filter(|e| e["role"] == event::SUMMARY)
        .collect()
}

/// Past its window, a conversation keeps going: what leaves is saved to
/// notes, then summarized in an event of its own, which the window
/// starts with. The turn answers first, with the window it last sent;
/// the summary is asked after, as that prompt and its answer and the
/// request, so the provider's cache holds for all of it; each summary
/// covers the one before; it never reaches the chat or the copy for
/// people.
#[tokio::test]
async fn a_conversation_past_its_window_keeps_going_with_a_summary() {
    let (memory, calls) = fake_memory().await;
    let world = world("summary", "", vec![("LNK_MEMORY".into(), memory)], 600).await;
    let thread = Thread::new("telegram:42").unwrap();
    world.say(&thread, 1, 14).await;

    let folder = world.folder("telegram-42");
    let summaries = summaries(&folder);
    assert!(summaries.len() >= 2, "{summaries:?}");
    let mut covers = 0;
    for summary in &summaries {
        // Each covers more, and holds every message the one before did.
        assert!(summary["covers"].as_u64().unwrap() > covers, "{summary}");
        covers = summary["covers"].as_u64().unwrap();
        assert!(
            summary["text"]
                .as_str()
                .unwrap()
                .starts_with("Summary of 1, ")
        );
        // Saved first: the remember call kept with it, the other refused.
        let saved: Vec<&str> = summary["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(saved, [compact::REMEMBER], "{summary}");
        assert!(summary["tokens"]["input"].as_u64().unwrap() > 0);
    }
    let made: Vec<Value> = calls.lock().unwrap().clone();
    assert_eq!(made.len(), summaries.len(), "{made:?}");
    assert!(made.iter().all(|c| c["name"] == "remember"), "{made:?}");

    let asked = world.asked();
    // The last answer: a summary at the window's head, the first message
    // gone.
    let last = asked.iter().rev().find(|r| answering(r)).unwrap();
    let head = last["messages"][1]["content"].as_str().unwrap();
    assert!(head.starts_with(SUMMARY_HEAD), "{head}");
    assert!(
        summaries
            .iter()
            .any(|s| head.contains(s["text"].as_str().unwrap())),
        "{head}"
    );
    assert!(!head.contains("Message 1:"), "{head}");
    // Each summary asked right after the answer, as that request, its
    // answer and the ask: the provider's cache holds for all of it. It
    // says which message stays.
    let mut checked = 0;
    for pair in asked.windows(2) {
        let (before, now) = (&pair[0], &pair[1]);
        if summarizing(now) {
            assert!(answering(before), "{before}");
            let now_messages = now["messages"].as_array().unwrap();
            let before_messages = before["messages"].as_array().unwrap();
            assert_eq!(now_messages.len(), before_messages.len() + 2);
            assert_eq!(now_messages[..before_messages.len()], before_messages[..]);
            assert_eq!(now_messages[before_messages.len()]["role"], "assistant");
            assert_eq!(now["tools"], before["tools"]);
            assert_eq!(now.get("tool_choice"), before.get("tool_choice"));
            let ask = last_text(now);
            assert!(ask.contains("memory__remember"), "{ask}");
            assert!(ask.contains("the message that begins \"Message "), "{ask}");
            checked += 1;
        }
    }
    assert_eq!(checked, summaries.len());

    // Never to people: the copy holds the messages, not the summaries.
    let copy = std::fs::read_to_string(folder.join("conversation.md")).unwrap();
    assert!(copy.contains("Message 14:"));
    assert!(!copy.contains("Summary of"), "{copy}");

    // A process with none of it (the window's mark gone) starts the
    // window with the same summary.
    common::forget_marks(&folder);
    world.say(&thread, 15, 15).await;
    let asked = world.asked();
    let last = asked.iter().rev().find(|r| answering(r)).unwrap();
    let head = last["messages"][1]["content"].as_str().unwrap().to_string();
    assert!(head.starts_with(SUMMARY_HEAD), "{head}");
}

/// With the first model off, what leaves the window is summarized by the
/// fallback, as the turns are answered by it.
#[tokio::test]
async fn with_the_first_model_off_the_fallback_summarizes() {
    let world = world_with(
        "fallback",
        "",
        vec![],
        600,
        std::time::Duration::ZERO.into(),
        true,
    )
    .await;
    let thread = Thread::new("telegram:42").unwrap();
    world.say(&thread, 1, 10).await;
    let summaries = summaries(&world.folder("telegram-42"));
    assert!(!summaries.is_empty());
    for summary in &summaries {
        let text = summary["text"].as_str().unwrap();
        assert!(text.starts_with("Summary of 1, "), "{summary}");
    }
    assert!(world.asked().iter().any(summarizing));
}

/// The turn that compacts answers first: its first piece comes as soon
/// as any turn's, and its answer is told while the summary's answers are
/// still held, so it never waits for them; the summary runs after, under
/// its lease, before the turn ends. A model that holds each answer after
/// its first piece until the test lets it go stands in: the order is
/// what the test let go, never how long anything took.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_that_compacts_answers_first() {
    let (memory, _calls) = fake_memory().await;
    let (let_go, hold) = common::LetGo::new();
    let env = vec![("LNK_MEMORY".into(), memory)];
    let world = world_paused("first", "", env, 600, hold).await;
    let thread = Thread::new("telegram:42").unwrap();
    let folder = world.folder("telegram-42");
    let mut compacted = 0;
    for n in 1..=12 {
        let text = format!("Message {n}: {}", "word ".repeat(80));
        let (pieces, mut heard) = tokio::sync::mpsc::channel(64);
        let (told, answer) = tokio::sync::oneshot::channel();
        let before = summaries(&folder).len();
        let asked_before = world.asked().len();
        let turn = world.harness.ask_told(
            &world.agent,
            &thread,
            event::Event::user(&text),
            pieces,
            None,
            Some(told),
            None,
        );
        let watch = async {
            // Its first piece while the model holds the rest: the answer
            // was asked first, nothing before it.
            heard.recv().await.expect("its answer's first piece");
            let asked = world.asked();
            assert_eq!(asked.len(), asked_before + 1, "message {n}: {asked:?}");
            assert!(answering(&asked[asked_before]), "message {n}: {asked:?}");
            let_go.up_to(asked_before + 1);
            // Told with every answer after its own held: it waits for
            // none of the summary's. Not told, the turn ended.
            let limit = link_test_kit::wait::scaled(std::time::Duration::from_secs(30));
            let told = tokio::time::timeout(limit, answer)
                .await
                .unwrap_or_else(|_| panic!("message {n}: not told in {limit:?}, the rest held"))
                .ok();
            if told.is_some() {
                assert_eq!(summaries(&folder).len(), before, "message {n}");
                let_go.all();
            }
            told
        };
        let (asked, told) = tokio::join!(turn, watch);
        asked.unwrap();
        // The next turn's answers held again.
        let_go.up_to(world.asked().len());
        if summaries(&folder).len() == before {
            assert!(told.is_none(), "message {n}: told without a summary");
            continue;
        }
        compacted += 1;
        let told = told.expect("a turn that compacts tells its answer first");
        assert!(told.text.starts_with("Answer "), "{}", told.text);
        // The summary (a save, then a summary: two answers) asked after
        // the answer, and written before the turn ended.
        let asked = world.asked();
        let since = &asked[asked_before..];
        assert_eq!(since.len(), 3, "message {n}: {since:?}");
        assert!(answering(&since[0]), "message {n}: {since:?}");
        assert!(summarizing(&since[1]), "message {n}: {since:?}");
        assert!(!answering(&since[2]), "message {n}: {since:?}");
    }
    assert!(compacted >= 1);
}

/// A message that comes while a turn summarizes after its answer waits
/// for a turn of its own: the answer it would have joined is given.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_while_it_summarizes_gets_its_own_answer() {
    let (memory, _calls) = fake_memory().await;
    let pause = std::time::Duration::from_millis(300);
    let env = vec![("LNK_MEMORY".into(), memory)];
    let world = world_paused("while", "", env, 600, pause).await;
    let thread = Thread::new("telegram:42").unwrap();
    // Message 5 is the first whose turn compacts (as above).
    world.say(&thread, 1, 4).await;
    let say = |n: usize| event::Event::user(&format!("Message {n}: {}", "word ".repeat(80)));
    let (pieces, _heard) = tokio::sync::mpsc::channel(64);
    let (told, answer) = tokio::sync::oneshot::channel();
    let turn = world.harness.ask_told(
        &world.agent,
        &thread,
        say(5),
        pieces,
        None,
        Some(told),
        None,
    );
    let next = async {
        let told = answer.await.expect("message 5's turn compacts");
        assert!(told.text.starts_with("Answer "), "{}", told.text);
        let (pieces, _heard) = tokio::sync::mpsc::channel(64);
        let second = world
            .harness
            .ask(&world.agent, &thread, say(6), pieces, None)
            .await
            .unwrap();
        (told.text, second)
    };
    let (first, (told, second)) = tokio::join!(turn, next);
    // The turn that told its answer answers nothing after it: never the
    // message that came meanwhile, whose caller it can't reach.
    let link_harness::turn::Asked::Answered(first) = first.unwrap() else {
        panic!("message 5 joined another turn");
    };
    assert_eq!(first.text, told, "message 5's turn answered message 6");
    let link_harness::turn::Asked::Answered(second) = second else {
        panic!("it joined a turn that had answered: {second:?}");
    };
    assert!(second.text.starts_with("Answer "), "{}", second.text);
    assert_ne!(second.text, told);
    assert_eq!(summaries(&world.folder("telegram-42")).len(), 1);
}

/// A message that comes after the summary is written but before the
/// turn lets its lease go (it ends its tools' sessions first) still gets
/// its own answer, not joined to a turn that won't answer it.
#[tokio::test(flavor = "multi_thread")]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_message_as_the_summarizing_turn_ends_gets_its_own_answer() {
    let (memory, _calls) = fake_memory().await;
    // A tool of its own, not memory's kept one: its session ends with
    // the turn, slowly.
    let env = vec![("LNK_TOOL_MEMORY".into(), memory)];
    let world = world("ending", "", env, 600).await;
    let thread = Thread::new("telegram:42").unwrap();
    let folder = world.folder("telegram-42");
    world.say(&thread, 1, 4).await;
    let say = |n: usize| event::Event::user(&format!("Message {n}: {}", "word ".repeat(80)));
    let (pieces, _heard) = tokio::sync::mpsc::channel(64);
    let (told, answer) = tokio::sync::oneshot::channel();
    let turn = world.harness.ask_told(
        &world.agent,
        &thread,
        say(5),
        pieces,
        None,
        Some(told),
        None,
    );
    let next = async {
        answer.await.expect("message 5's turn compacts");
        // Summarized: the turn now ends its tools' sessions.
        while summaries(&folder).is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let (pieces, _heard) = tokio::sync::mpsc::channel(64);
        world
            .harness
            .ask(&world.agent, &thread, say(6), pieces, None)
            .await
            .unwrap()
    };
    let (first, second) = tokio::join!(turn, next);
    first.unwrap();
    let link_harness::turn::Asked::Answered(second) = second else {
        panic!("it joined a turn that had answered: {second:?}");
    };
    assert!(second.text.starts_with("Answer "), "{}", second.text);
}

/// A conversation only of others' messages (a teammate's) is summarized
/// without saving notes: notes are the owner's.
#[tokio::test]
async fn others_conversations_save_no_notes() {
    let (memory, calls) = fake_memory().await;
    let world = world("others", "", vec![("LNK_MEMORY".into(), memory)], 600).await;
    let thread = Thread::new("agent:abc").unwrap();
    for n in 1..=10 {
        let text = format!("Message {n}: {}", "word ".repeat(80));
        let (pieces, _heard) = tokio::sync::mpsc::channel(64);
        let message = event::Event {
            from: Some("agent:abc".into()),
            ..event::Event::user(&text)
        };
        world
            .harness
            .ask(&world.agent, &thread, message, pieces, None)
            .await
            .unwrap();
    }
    let asked = world.asked();
    let requests: Vec<&Value> = asked.iter().filter(|r| summarizing(r)).collect();
    assert!(!requests.is_empty());
    for request in requests {
        assert!(only_summarizing(request), "{}", last_text(request));
    }
    assert!(calls.lock().unwrap().is_empty());
}

/// Without a tool to save with, or with `save_notes = false`, it only
/// summarizes, calling nothing; with `compact_at = "never"`, what
/// leaves the window leaves unsummarized.
#[tokio::test]
async fn it_saves_only_where_it_can_and_compacts_only_when_asked() {
    let (memory, calls) = fake_memory().await;
    for (name, own, env) in [
        ("bare", "", vec![]),
        (
            "unsaved",
            "[window]\nsave_notes = false\n",
            vec![("LNK_MEMORY".to_string(), memory.clone())],
        ),
    ] {
        let world = world(name, own, env, 600).await;
        let thread = Thread::new("telegram:42").unwrap();
        world.say(&thread, 1, 10).await;
        let summaries = summaries(&world.folder("telegram-42"));
        assert!(!summaries.is_empty(), "{name}");
        assert!(summaries.iter().all(|s| s.get("calls").is_none()), "{name}");
        let asked = world.asked();
        let requests: Vec<&Value> = asked.iter().filter(|r| summarizing(r)).collect();
        assert_eq!(requests.len(), summaries.len(), "{name}");
        for request in requests {
            assert!(only_summarizing(request), "{name}: {}", last_text(request));
        }
    }
    assert!(calls.lock().unwrap().is_empty());

    let world = world("never", "[window]\ncompact_at = \"never\"\n", vec![], 600).await;
    let thread = Thread::new("telegram:42").unwrap();
    world.say(&thread, 1, 10).await;
    assert!(summaries(&world.folder("telegram-42")).is_empty());
    let asked = world.asked();
    assert!(!asked.iter().any(summarizing));
    let last = asked.last().unwrap().to_string();
    assert!(!last.contains("Message 1:"), "{last}");
    assert!(!last.contains(SUMMARY_HEAD));
}

/// A model whose price jumps compacts within its first price: Haiku 5.5
/// costs five times as much past 100,000 tokens of prompt, so its
/// window stops there, whatever `[window] max` says, and a conversation
/// of 112,000 tokens is summarized. With `past_price_jump = true` the
/// window is the whole of `max`, and the same conversation fits: each
/// answer's cost is estimated at its prompt's price, the higher past the
/// jump.
#[tokio::test]
async fn a_conversation_compacts_before_its_models_price_jumps() {
    for (name, own, compacts) in [
        ("capped", "[window]\nmax = 1000000\n", true),
        (
            "past",
            "[window]\nmax = 1000000\npast_price_jump = true\n",
            false,
        ),
    ] {
        let world = world_on(name, own, "claude-haiku-5-5").await;
        let thread = Thread::new("telegram:42").unwrap();
        // Twenty-eight messages of about 4,000 tokens each.
        for n in 1..=28 {
            let text = format!("Message {n}: {}", "word ".repeat(3200));
            let (pieces, _heard) = tokio::sync::mpsc::channel(64);
            let answer = world.harness.turn(&world.agent, &thread, &text, pieces);
            assert!(answer.await.unwrap().text.starts_with("Answer "), "{name}");
        }
        let summaries = summaries(&world.folder("telegram-42"));
        assert_eq!(!summaries.is_empty(), compacts, "{name}");
        let sent = |r: &Value| link_model_contract::context::text_tokens(&r.to_string());
        let longest = world.asked().iter().map(sent).max().unwrap();
        match compacts {
            true => assert!(longest < 100_000, "{name}: {longest}"),
            false => assert!(longest > 100_000, "{name}: {longest}"),
        }
        let facts = link_model_contract::known("claude-haiku-5-5")
            .unwrap()
            .unwrap()
            .facts;
        let (base, jump) = (facts.price.unwrap(), facts.above[0]);
        let mut past = 0;
        let answers = common::events(&world.folder("telegram-42"));
        for answer in answers.iter().filter(|e| e["role"] == "assistant") {
            let input = answer["tokens"]["input"].as_u64().unwrap();
            let price = match input > u64::from(jump.tokens) {
                true => jump.price,
                false => base,
            };
            past += usize::from(input > u64::from(jump.tokens));
            let cost = answer["cost"].as_f64().unwrap();
            assert!(
                (cost - price.of(input, 0, 5)).abs() < 1e-12,
                "{name}: {answer}"
            );
        }
        assert_eq!(past > 0, !compacts, "{name}: a call past the jump");
    }
}
