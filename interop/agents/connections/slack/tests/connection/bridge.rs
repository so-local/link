//! An agent on a harness that speaks no connection answering on Slack, end to end: `lnk agent
//! start` runs the mini harness and, beside it, the Slack module carrying the
//! messages (`lnk-slack bridge`), against a fake Slack (its Web API and a
//! Socket Mode socket) and a bare chat completions model.

#![cfg(unix)]

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Form, Json, Router,
    extract::{
        Path as UrlPath, State,
        ws::{Message as Frame, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};

use link_harness_conformance::agent::{self, Agent, Shared, serve};

/// An agent on the mini harness, its programs the core's (built in
/// `link-core` beside this folder) and this folder's `lnk-slack`.
async fn start(name: &str, sandboxed: bool, section: &str, env: &[(&str, String)]) -> Agent {
    let programs = agent::programs_beside(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        std::path::Path::new(env!("CARGO_BIN_EXE_lnk-slack")),
    );
    agent::start_fixture(&programs, name, sandboxed, section, env, |_, _| {}).await
}

const SLACK_BOT: &str = concat!("xoxb", "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx");
const SLACK_APP: &str = concat!("xapp", "-1-A0123456789-1234567890123-abcdef0123456789");
/// A bot token revoked (the app uninstalled): Slack refuses it.
const REVOKED: &str = concat!("xoxb", "-1234567890-1234567890123-RevokedRevokedRevoked");
/// The owner's member id, and their direct message with the bot.
const MEMBER: &str = "U0DANA";
const DM: &str = "D0DANA";
/// The app's member id, and a channel the owner mentions it in.
const BOT: &str = "U0BOT";
const CHANNEL: &str = "C0GEN";

/// What the fake Slack holds: Socket Mode envelopes to hand out, and
/// what the bridge did.
#[derive(Default)]
struct Slack {
    /// Its own port, for the Socket Mode address.
    port: u16,
    envelopes: Vec<Value>,
    /// How many envelopes were handed out.
    handed: usize,
    /// The envelope ids acknowledged.
    acked: Vec<String>,
    /// The Web API calls with the bot token: the method and its form.
    calls: Vec<(String, HashMap<String, String>)>,
}

async fn slack_api(
    State(slack): State<Shared<Slack>>,
    UrlPath(method): UrlPath<String>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string();
    let mut slack = slack.lock().unwrap();
    if method == "apps.connections.open" {
        assert_eq!(token, SLACK_APP, "Socket Mode opened with another token");
        let url = format!("ws://127.0.0.1:{}/socket?ticket=t", slack.port);
        return Json(json!({"ok": true, "url": url}));
    }
    if token == REVOKED {
        return Json(json!({"ok": false, "error": "token_revoked"}));
    }
    assert_eq!(token, SLACK_BOT, "{method} with another token");
    if method == "auth.test" {
        return Json(
            json!({"ok": true, "user_id": BOT, "user": "link-main", "team": "Dana's",
            "bot_id": "B0BOT", "team_id": "T0ACME"}),
        );
    }
    if method == "conversations.info" {
        return Json(json!({"ok": true, "channel": {"name": "general"}}));
    }
    let ts = format!("2000.{:06}", slack.calls.len());
    slack.calls.push((method, form));
    Json(json!({"ok": true, "channel": DM, "ts": ts}))
}

/// Socket Mode: hello, then each envelope as it's queued; acknowledgements
/// are kept.
async fn socket(State(slack): State<Shared<Slack>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| hand_out(slack, socket))
}

async fn hand_out(slack: Shared<Slack>, mut socket: WebSocket) {
    let hello = json!({"type": "hello", "num_connections": 1}).to_string();
    if socket.send(Frame::text(hello)).await.is_err() {
        return;
    }
    loop {
        let next = {
            let mut slack = slack.lock().unwrap();
            let next = slack.envelopes.get(slack.handed).cloned();
            if next.is_some() {
                slack.handed += 1;
            }
            next
        };
        if let Some(envelope) = next {
            if socket
                .send(Frame::text(envelope.to_string()))
                .await
                .is_err()
            {
                return;
            }
            continue;
        }
        match tokio::time::timeout(Duration::from_millis(50), socket.recv()).await {
            Ok(Some(Ok(Frame::Text(text)))) => {
                let ack: Value = serde_json::from_str(&text).unwrap();
                let id = ack["envelope_id"].as_str().unwrap().to_string();
                slack.lock().unwrap().acked.push(id);
            }
            Ok(None | Some(Err(_)) | Some(Ok(Frame::Close(_)))) => return,
            _ => {}
        }
    }
}

/// An `events_api` envelope for a message `user` sent in `channel`.
fn slack_event(id: &str, user: &str, connection: &str, kind: &str, ts: &str, text: &str) -> Value {
    json!({"envelope_id": id, "type": "events_api", "accepts_response_payload": false,
        "retry_attempt": 0, "payload": {"type": "event_callback", "event": {
            "type": "message", "user": user, "channel": connection, "channel_type": kind,
            "ts": ts, "text": text}}})
}

/// An `events_api` envelope for `user`'s mention of the app in `channel`.
fn slack_mention(id: &str, user: &str, connection: &str, ts: &str) -> Value {
    json!({"envelope_id": id, "type": "events_api", "accepts_response_payload": false,
        "retry_attempt": 0, "payload": {"type": "event_callback", "event": {
            "type": "app_mention", "user": user, "channel": connection, "ts": ts,
            "event_ts": ts, "text": format!("<@{BOT}> hello")}}})
}

#[tokio::test]
async fn sharing_takes_only_the_owners_code_once_and_expires() {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let slack: Shared<Slack> = Arc::default();
    let port = serve(
        Router::new()
            .route("/api/{method}", post(slack_api))
            .route("/socket", get(socket))
            .with_state(slack.clone()),
    )
    .await;
    {
        let mut fake = slack.lock().unwrap();
        fake.port = port;
        for (id, user) in [("first", "U0EVE"), ("second", MEMBER)] {
            let mut event = slack_mention(id, user, CHANNEL, "1.3");
            event["payload"]["event"]["text"] = json!(format!("<@{BOT}> K7Q2M9XA"));
            fake.envelopes.push(event);
        }
    }
    for wait_secs in [5, 0] {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_lnk-slack"))
            .arg("share")
            .env("LNK_SLACK_API", format!("http://127.0.0.1:{port}/api"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = json!({"settings": {"bot_token": SLACK_BOT, "app_token": SLACK_APP, "allowed_users": [MEMBER]},
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
        assert_eq!(shared.chat.as_str(), "chat:slack:C0GEN");
        assert_eq!(shared.name.as_deref(), Some("#general"));
    }
    assert_eq!(
        slack
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(m, _)| m == "chat.postMessage")
            .count(),
        1
    );
}

/// The messages in `channel` as they're shown now: each posted one, as
/// last edited.
fn shown_in(slack: &Shared<Slack>, connection: &str) -> Vec<String> {
    let slack = slack.lock().unwrap();
    let mut messages: Vec<(String, String)> = Vec::new();
    for (i, (method, form)) in slack.calls.iter().enumerate() {
        if form.get("channel").map(String::as_str) != Some(connection) {
            continue;
        }
        let text = form.get("text").cloned().unwrap_or_default();
        match method.as_str() {
            "chat.postMessage" => messages.push((format!("2000.{i:06}"), text)),
            "chat.update" => {
                let ts = &form["ts"];
                let message = messages.iter_mut().find(|(t, _)| t == ts);
                message.expect("an edit of a message never posted").1 = text;
            }
            _ => {}
        }
    }
    messages.into_iter().map(|(_, text)| text).collect()
}

/// Waits until the owner's direct message ends with `ending`.
async fn shown_on_slack(slack: &Shared<Slack>, ending: &[&str], agent: &mut Agent) {
    shown_in_channel(slack, DM, ending, agent).await
}

/// Waits until `channel` ends with `ending`. Threads are answered side by
/// side, so one being done says nothing of another.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn shown_in_channel(
    slack: &Shared<Slack>,
    connection: &str,
    ending: &[&str],
    agent: &mut Agent,
) {
    let start = Instant::now();
    loop {
        let shown = shown_in(slack, connection);
        if shown.ends_with(&ending.iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
            return;
        }
        let head: Vec<String> = shown.iter().map(|s| s.chars().take(80).collect()).collect();
        agent.check(start, &format!("waiting for {ending:.80?}; shown {head:?}"));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A message's file, which only the bot's token downloads.
async fn file(headers: HeaderMap) -> Vec<u8> {
    let bearer = format!("Bearer {SLACK_BOT}");
    assert_eq!(headers.get("authorization").unwrap(), bearer.as_str());
    b"%PDF-1".to_vec()
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn talk_on_slack(name: &str, sandboxed: bool) {
    let slack: Shared<Slack> = Arc::default();
    let port = serve(
        Router::new()
            .route("/api/{method}", post(slack_api))
            .route("/api/files/a.pdf", get(file))
            .route("/socket", get(socket))
            .with_state(slack.clone()),
    )
    .await;
    slack.lock().unwrap().port = port;
    let section = agent::connection(
        "slack",
        &["bot_token", "app_token"],
        &format!(
            "bot_token = \"{SLACK_BOT}\"\napp_token = \"{SLACK_APP}\"\n\
             allowed_users = [\"{MEMBER}\"]"
        ),
        "",
    );
    let api = format!("http://127.0.0.1:{port}/api");
    let mut agent = start(name, sandboxed, &section, &[("LNK_SLACK_API", api.clone())]).await;

    // A stranger, the owner in a channel, and a bot; a stranger's mention
    // of the app and the owner's, which Slack sends again; then the owner
    // says hello, and Slack sends it again.
    let mut bots = slack_event("e3", MEMBER, DM, "im", "1000.3", "a bot's");
    bots["payload"]["event"]["bot_id"] = json!("B01");
    let mut again = slack_event("e5", MEMBER, DM, "im", "1000.4", "Hello");
    again["retry_attempt"] = json!(1);
    let mut mention_again = slack_mention("m3", MEMBER, CHANNEL, "1000.22");
    mention_again["retry_attempt"] = json!(1);
    slack.lock().unwrap().envelopes.extend([
        slack_event("e1", "U0EVE", "D0EVE", "im", "1000.1", "I'm not the owner"),
        slack_event("e2", MEMBER, "C0GEN", "channel", "1000.2", "Hello, channel"),
        bots,
        slack_mention("m1", "U0EVE", "C0EVE", "1000.21"),
        slack_mention("m2", MEMBER, CHANNEL, "1000.22"),
        mention_again,
        slack_event("e4", MEMBER, DM, "im", "1000.4", "Hello"),
        again,
    ]);
    shown_on_slack(&slack, &["I see 1 messages; the last: Hello"], &mut agent).await;
    let answer = ["I see 1 messages; the last: hello"];
    shown_in_channel(&slack, CHANNEL, &answer, &mut agent).await;
    // 👀 on the owner's message while it's answered, taken off after.
    let start = Instant::now();
    let reacted = |method: &str| {
        slack
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|(m, f)| m == method && f["timestamp"] == "1000.4" && f["name"] == "eyes")
    };
    let reacted_in_channel = || {
        slack.lock().unwrap().calls.iter().any(|(m, f)| {
            m == "reactions.remove" && f["channel"] == CHANNEL && f["timestamp"] == "1000.22"
        })
    };
    while !reacted("reactions.remove") || !reacted_in_channel() {
        agent.check(start, "waiting for the 👀 to be taken off");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(reacted("reactions.add"));
    {
        let slack = slack.lock().unwrap();
        // Streamed: the first piece posted, then edited into the answer.
        let methods: Vec<(&str, &str)> = slack
            .calls
            .iter()
            .map(|(m, f)| (m.as_str(), f.get("text").map_or("", String::as_str)))
            .collect();
        assert!(
            methods.contains(&("chat.postMessage", "I see")),
            "{methods:?}"
        );
        assert!(
            methods.iter().any(|(m, _)| *m == "chat.update"),
            "{methods:?}"
        );
        // Nothing goes to anyone else.
        let elsewhere: Vec<&(String, HashMap<String, String>)> = slack
            .calls
            .iter()
            .filter(|(_, f)| {
                ![Some(DM), Some(CHANNEL)].contains(&f.get("channel").map(String::as_str))
            })
            .collect();
        assert!(
            elsewhere.is_empty(),
            "someone else was answered: {elsewhere:?}"
        );
        // The owner's mention is answered once, in a thread on it, with
        // 👀 on it while it's written.
        let posts: Vec<&HashMap<String, String>> = slack
            .calls
            .iter()
            .filter(|(m, f)| m == "chat.postMessage" && f["channel"] == CHANNEL)
            .map(|(_, f)| f)
            .collect();
        assert_eq!(posts.len(), 1, "{posts:?}");
        assert_eq!(posts[0]["thread_ts"], "1000.22");
        let reacted = |method: &str| {
            slack
                .calls
                .iter()
                .any(|(m, f)| m == method && f["channel"] == CHANNEL && f["timestamp"] == "1000.22")
        };
        assert!(reacted("reactions.add") && reacted("reactions.remove"));
    }
    assert_eq!(
        shown_in(&slack, CHANNEL),
        ["I see 1 messages; the last: hello"]
    );

    // It remembers the thread: the model is sent the conversation.
    let next = slack_event("e6", MEMBER, DM, "im", "1000.6", "And again");
    slack.lock().unwrap().envelopes.push(next);
    shown_on_slack(
        &slack,
        &["I see 3 messages; the last: And again"],
        &mut agent,
    )
    .await;
    agent.remembered("slack-D0DANA", &[SLACK_BOT, SLACK_APP], 3);
    // The mention without the app's, in the conversation of the Slack
    // thread it starts.
    agent.asked_in_group("slack-C0GEN.1000.22", "hello");

    // An answer past Slack's limit is split into more messages, between
    // words where it can: a word longer than one message is cut.
    let long = "x".repeat(4100);
    let next = slack_event("e7", MEMBER, DM, "im", "1000.7", &long);
    slack.lock().unwrap().envelopes.push(next);
    let (head, tail) = long.split_at(4000);
    shown_on_slack(
        &slack,
        &["I see 5 messages; the last:", head, tail],
        &mut agent,
    )
    .await;

    // A document alone reaches the agent.
    let mut document = slack_event("e8", MEMBER, DM, "im", "1000.8", "");
    document["payload"]["event"]["subtype"] = json!("file_share");
    document["payload"]["event"]["files"] = json!([{"name": "a.pdf",
        "mimetype": "application/pdf", "size": 6,
        "url_private_download": format!("{api}/files/a.pdf")}]);
    slack.lock().unwrap().envelopes.push(document);
    let seen = "I see 7 messages; the last: [a.pdf: application/pdf, 6 bytes]";
    shown_on_slack(&slack, &[seen], &mut agent).await;

    let slack = slack.lock().unwrap();
    let mut acked = slack.acked.clone();
    acked.sort();
    assert_eq!(
        acked,
        [
            "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "m1", "m2", "m3"
        ]
    );
    drop(slack);
    // A revoked token is refused as such.
    agent.refuses_token(
        "slack",
        &json!({"bot_token": REVOKED, "allowed_users": [MEMBER], "dm_channel": DM}),
        DM,
        &[("LNK_SLACK_API", api), ("NO_PROXY", "*".into())],
    );
    agent.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_on_the_mini_harness_answers_on_slack() {
    talk_on_slack("slack-open", false).await;
}

/// The same from the sandbox (Linux, bubblewrap).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn it_answers_on_slack_from_its_sandbox() {
    if agent::bubblewrap() {
        talk_on_slack("slack-sandboxed", true).await;
    }
}
