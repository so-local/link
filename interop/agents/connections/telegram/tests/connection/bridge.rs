//! An agent on a harness that speaks no connection answering on Telegram, end to end: `lnk
//! agent start` runs the mini harness (`lnk-mini-harness`) and, beside it,
//! the Telegram module carrying the messages (`lnk-telegram bridge`),
//! against a fake Telegram and a bare chat completions model. The owner's
//! two messages are answered, the second knowing the first; a stranger's
//! is not; the owner's mention in a group is answered there, as a reply
//! to it, in a thread of its own, and a stranger's never reaches the
//! model; the bot's token never reaches the harness.

#![cfg(unix)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    routing::{get, post},
};
use serde_json::{Value, json};

use link_harness_conformance::agent::{self, Agent, Shared, serve};

/// An agent on the mini harness, its programs the core's (built in
/// `link-core` beside this folder) and this folder's `lnk-telegram`.
async fn start(name: &str, sandboxed: bool, section: &str, env: &[(&str, String)]) -> Agent {
    let programs = agent::programs_beside(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        std::path::Path::new(env!("CARGO_BIN_EXE_lnk-telegram")),
    );
    agent::start_fixture(&programs, name, sandboxed, section, env, |_, _| {}).await
}

const TOKEN: &str = concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw");
/// A token revoked with BotFather: Telegram refuses it.
const REVOKED: &str = concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDrev");
const OWNER: i64 = 42;
const BOT_NAME: &str = "link_main_bot";
/// Two groups the bot is in: the owner mentions it in one, a stranger in
/// the other.
const OWNERS_GROUP: i64 = -1001;
const STRANGERS_GROUP: i64 = -1002;

/// What the fake Telegram holds: updates to hand out, and what the bot
/// did.
#[derive(Default)]
struct Telegram {
    updates: Vec<Value>,
    calls: Vec<(String, Value)>,
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn bot(
    State(telegram): State<Shared<Telegram>>,
    UrlPath((bot, method)): UrlPath<(String, String)>,
    body: Option<Json<Value>>,
) -> Json<Value> {
    if bot == format!("bot{REVOKED}") {
        return Json(json!({"ok": false, "error_code": 401, "description": "Unauthorized"}));
    }
    assert_eq!(bot, format!("bot{TOKEN}"), "the bridge used another token");
    let body = body.map(|Json(b)| b).unwrap_or(Value::Null);
    if method == "getUpdates" {
        let offset = body["offset"].as_i64().unwrap_or(0);
        let start = Instant::now();
        // A long poll, shortened.
        loop {
            let ready: Vec<Value> = telegram
                .lock()
                .unwrap()
                .updates
                .iter()
                .filter(|u| u["update_id"].as_i64().unwrap() >= offset)
                .cloned()
                .collect();
            if !ready.is_empty() || start.elapsed() > Duration::from_millis(500) {
                return Json(json!({"ok": true, "result": ready}));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    if method == "getFile" {
        assert_eq!(body["file_id"], "photo-large");
        return Json(json!({"ok": true, "result": {"file_id": "photo-large",
            "file_path": "photos/file_1.jpg"}}));
    }
    if method == "getMe" {
        let me = json!({"id": 123456, "is_bot": true, "first_name": "Link",
            "username": BOT_NAME});
        return Json(json!({"ok": true, "result": me}));
    }
    let mut telegram = telegram.lock().unwrap();
    let id = telegram.calls.len() as i64 + 1000;
    telegram.calls.push((method, body));
    Json(json!({"ok": true, "result": {"message_id": id}}))
}

/// A photo's file, as the bot downloads it: a JPEG's first bytes.
async fn file(UrlPath(bot): UrlPath<String>) -> Vec<u8> {
    assert_eq!(bot, format!("bot{TOKEN}"), "the bridge used another token");
    vec![0xff, 0xd8, 0xff, 0xe0]
}

fn message(update: i64, from: i64, text: &str) -> Value {
    json!({"update_id": update, "message": {"message_id": update, "text": text,
        "from": {"id": from, "is_bot": false, "first_name": "Dana"},
        "chat": {"id": from, "type": "private"}}})
}

/// A message from `from` in `group` mentioning the bot.
fn mention(update: i64, from: i64, group: i64) -> Value {
    let text = format!("@{BOT_NAME} hello");
    json!({"update_id": update, "message": {"message_id": update, "text": text,
        "entities": [{"type": "mention", "offset": 0, "length": BOT_NAME.len() + 1}],
        "from": {"id": from, "is_bot": false, "first_name": "Dana"},
        "chat": {"id": group, "type": "supergroup", "title": "Friends"}}})
}

#[tokio::test]
async fn sharing_takes_only_the_owners_code_once_and_expires() {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let telegram: Shared<Telegram> = Arc::default();
    let port = serve(
        Router::new()
            .route("/{bot}/{method}", post(bot))
            .with_state(telegram.clone()),
    )
    .await;
    for (update, from) in [(1, 43), (2, OWNER)] {
        let mut update = mention(update, from, OWNERS_GROUP);
        update["message"]["text"] = json!(format!("@{BOT_NAME} K7Q2M9XA"));
        telegram.lock().unwrap().updates.push(update);
    }
    for wait_secs in [5, 0] {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_lnk-telegram"))
            .arg("share")
            .env("LNK_TELEGRAM_API", format!("http://127.0.0.1:{port}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = json!({"settings": {"bot_token": TOKEN, "allowed_users": [OWNER]},
            "code": "K7Q2M9XA", "wait_secs": wait_secs});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        if wait_secs == 0 {
            assert!(!output.status.success());
            continue;
        }
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let shared: link_connection_contract::Shared =
            serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(shared.chat.as_str(), "chat:telegram:-1001");
        assert_eq!(shared.name.as_deref(), Some("Friends"));
    }
    assert_eq!(
        telegram
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(m, _)| m == "sendMessage")
            .count(),
        1
    );
}

/// Waits until the bot's calls hold a message to the owner containing
/// `text`; what it sent them, all of it.
async fn answered(telegram: &Shared<Telegram>, text: &str, agent: &mut Agent) -> Vec<String> {
    answered_in(telegram, OWNER, text, agent).await
}

/// Waits until the bot's calls hold a message in `chat` containing
/// `text`; what it sent there, all of it.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn answered_in(
    telegram: &Shared<Telegram>,
    chat: i64,
    text: &str,
    agent: &mut Agent,
) -> Vec<String> {
    let start = Instant::now();
    loop {
        let sent: Vec<String> = telegram
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(m, b)| (m == "sendMessage" || m == "editMessageText") && b["chat_id"] == chat)
            .map(|(_, b)| b["text"].as_str().unwrap_or_default().to_string())
            .collect();
        if sent.iter().any(|s| s.contains(text)) {
            return sent;
        }
        agent.check(start, &format!("no answer with {text:?}; sent {sent:?}"));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn talk_on_telegram(name: &str, sandboxed: bool) {
    let telegram: Shared<Telegram> = Arc::default();
    let telegram_port = serve(
        Router::new()
            .route("/{bot}/{method}", post(bot))
            .route("/file/{bot}/photos/file_1.jpg", get(file))
            .with_state(telegram.clone()),
    )
    .await;
    let section = agent::connection(
        "telegram",
        &["bot_token"],
        &format!("bot_token = \"{TOKEN}\"\nallowed_users = [{OWNER}]"),
        "",
    );
    let api = format!("http://127.0.0.1:{telegram_port}");
    let mut agent = start(
        name,
        sandboxed,
        &section,
        &[("LNK_TELEGRAM_API", api.clone())],
    )
    .await;

    // The owner says hello; a stranger tries too. Each mentions the bot
    // in a group.
    telegram.lock().unwrap().updates.extend([
        message(1, 7, "I'm not the owner"),
        mention(2, 7, STRANGERS_GROUP),
        mention(3, OWNER, OWNERS_GROUP),
        message(4, OWNER, "Hello"),
    ]);
    let sent = answered(&telegram, "the last: Hello", &mut agent).await;
    // Streamed: the first piece shown, then the whole answer.
    assert!(sent.iter().any(|s| s == "I see"), "{sent:?}");
    assert_eq!(sent.last().unwrap(), "I see 1 messages; the last: Hello");
    // The owner's mention is answered in the group, streamed as a reply to
    // it; the stranger's gets nothing.
    let group = "I see 1 messages; the last: hello";
    let shown = answered_in(&telegram, OWNERS_GROUP, group, &mut agent).await;
    assert_eq!(shown.last().unwrap(), group);
    let in_group = |group: i64| -> Vec<(String, Value)> {
        let telegram = telegram.lock().unwrap();
        let calls = telegram.calls.iter().filter(|(_, b)| b["chat_id"] == group);
        calls.cloned().collect()
    };
    let sent: Vec<(String, Value)> = in_group(OWNERS_GROUP)
        .into_iter()
        .filter(|(m, _)| m == "sendMessage")
        .collect();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].1["text"], "I see");
    assert_eq!(sent[0].1["parse_mode"], "HTML");
    assert_eq!(sent[0].1["reply_parameters"]["message_id"], 3);
    assert!(
        in_group(STRANGERS_GROUP).is_empty(),
        "the stranger's mention was answered"
    );

    // It remembers the thread: the model is sent the conversation.
    telegram
        .lock()
        .unwrap()
        .updates
        .push(message(5, OWNER, "And again"));
    answered(
        &telegram,
        "I see 3 messages; the last: And again",
        &mut agent,
    )
    .await;
    agent.remembered("telegram-42", &[TOKEN], 3);
    // The mention without the bot's name, in the group's own thread.
    agent.asked_in_group("telegram--1001", "hello");
    // A mention in a forum's topic is a conversation of its own, answered
    // in the topic.
    let mut topic = mention(6, OWNER, OWNERS_GROUP);
    topic["message"]["text"] = json!(format!("@{BOT_NAME} plans"));
    topic["message"]["is_topic_message"] = json!(true);
    topic["message"]["message_thread_id"] = json!(9);
    telegram.lock().unwrap().updates.push(topic);
    let in_topic = "I see 1 messages; the last: plans";
    answered_in(&telegram, OWNERS_GROUP, in_topic, &mut agent).await;
    let topics: Vec<Value> = in_group(OWNERS_GROUP)
        .into_iter()
        .filter(|(m, b)| m == "sendMessage" && b["reply_parameters"]["message_id"] == 6)
        .map(|(_, b)| b["message_thread_id"].clone())
        .collect();
    assert_eq!(topics, [json!(9)]);
    agent.asked_in_group("telegram--1001.9", "plans");
    // The stranger got nothing.
    assert!(
        !telegram
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|(_, b)| b["chat_id"] == 7),
        "the stranger was answered"
    );
    // A photo reaches the agent at its largest, its caption with it.
    telegram
        .lock()
        .unwrap()
        .updates
        .push(json!({"update_id": 7, "message": {
        "message_id": 7, "caption": "what's this?",
        "photo": [{"file_id": "photo-small", "file_size": 2},
            {"file_id": "photo-large", "file_size": 4}],
        "from": {"id": OWNER, "is_bot": false, "first_name": "Dana"},
        "chat": {"id": OWNER, "type": "private"}}}));
    let seen = "I see 5 messages; the last: what's this?\n[image/jpeg, 4 bytes]";
    answered(&telegram, seen, &mut agent).await;
    // A revoked token is refused as such.
    agent.refuses_token(
        "telegram",
        &json!({"bot_token": REVOKED, "allowed_users": [OWNER]}),
        "42",
        &[("LNK_TELEGRAM_API", api), ("NO_PROXY", "*".into())],
    );
    agent.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_on_the_mini_harness_answers_on_telegram() {
    talk_on_telegram("open", false).await;
}

/// The same with the sandbox, the default: the harness in a network of
/// its own, its endpoint served out by its proxy (Linux, bubblewrap).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn it_answers_from_its_sandbox() {
    if agent::bubblewrap() {
        talk_on_telegram("sandboxed", true).await;
    }
}
