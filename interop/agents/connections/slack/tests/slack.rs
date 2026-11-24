//! `lnk agent connect slack` against a fake Web API: the bot token is
//! checked with auth.test and the app-level token with Socket Mode,
//! and the first person to send the bot the
//! one-time code in a direct message becomes the only one allowed.

link_test_kit::program!(unit);

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Form, Json, Router,
    extract::{Path, State, WebSocketUpgrade, ws::Message},
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};

const BOT: &str = concat!("xoxb", "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx");
const APP: &str = concat!("xapp", "-1-A0123456789-1234567890123-abcdef0123456789");

#[derive(Clone, Default)]
struct Slack {
    /// What chat.postMessage was asked to post, by channel.
    posted: Arc<Mutex<Vec<(String, String)>>>,
    port: Arc<Mutex<u16>>,
    events: Arc<Mutex<Vec<Value>>>,
    acked: Arc<Mutex<Vec<String>>>,
    ack_done: Arc<tokio::sync::Notify>,
}

async fn api(
    State(slack): State<Slack>,
    Path(method): Path<String>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    let ok = |value: Value| {
        let mut value = value;
        value["ok"] = json!(true);
        Json(value)
    };
    match (method.as_str(), token) {
        ("apps.connections.open", APP) => {
            ok(json!({"url": format!("ws://127.0.0.1:{}/socket", slack.port.lock().unwrap())}))
        }
        ("apps.connections.open", BOT) => {
            Json(json!({"ok": false, "error": "not_allowed_token_type"}))
        }
        ("auth.test", BOT) => ok(json!({
            "user_id": "U0BOT", "user": "link-main", "team": "Acme", "bot_id": "B01"
        })),
        ("users.info", BOT) => ok(json!({"user": {
            "name": "dana", "real_name": "Dana Q", "profile": {"display_name": ""}
        }})),
        ("chat.postMessage", BOT) => {
            slack.ack_done.notified().await;
            slack.posted.lock().unwrap().push((
                form.get("channel").cloned().unwrap_or_default(),
                form.get("text").cloned().unwrap_or_default(),
            ));
            ok(json!({}))
        }
        _ => Json(json!({"ok": false, "error": "invalid_auth"})),
    }
}

async fn socket(State(slack): State<Slack>, ws: WebSocketUpgrade) -> impl IntoResponse {
    let events = slack.events.lock().unwrap().clone();
    ws.on_upgrade(move |mut socket| async move {
        socket
            .send(Message::text(json!({"type":"hello"}).to_string()))
            .await
            .unwrap();
        for event in events {
            socket.send(Message::text(event.to_string())).await.unwrap();
            if event["type"] == "disconnect" {
                return;
            }
            let Some(Ok(Message::Text(ack))) = socket.recv().await else {
                panic!("no envelope acknowledgement")
            };
            let ack: Value = serde_json::from_str(&ack).unwrap();
            assert_eq!(ack["envelope_id"], event["envelope_id"]);
            slack
                .acked
                .lock()
                .unwrap()
                .push(ack["envelope_id"].as_str().unwrap().into());
        }
        slack.ack_done.notify_one();
        // Remain connected in the empty-stream timeout case.
        let _ = socket.recv().await;
    })
}

#[tokio::test]
async fn whoever_sends_the_code_becomes_the_owner() {
    let slack = Slack::default();
    let app = Router::new()
        .route("/{method}", post(api))
        .route("/socket", get(socket))
        .with_state(slack.clone());
    let port = link_harness_conformance::agent::serve(app).await;
    *slack.port.lock().unwrap() = port;
    // SAFETY: the only test in this binary; nothing else reads the environment.
    unsafe {
        std::env::set_var("LNK_SLACK_API", format!("http://127.0.0.1:{port}"));
        std::env::set_var("NO_PROXY", "*");
    }

    use link_slack as slack;
    let bot = slack::bot(BOT).await.unwrap();
    assert_eq!(
        (bot.id.as_str(), bot.name.as_str(), bot.team.as_str()),
        ("U0BOT", "link-main", "Acme")
    );
    let err = slack::bot("xoxb-wrong").await.unwrap_err();
    assert!(err.to_string().contains("invalid_auth"), "{err}");
    assert!(link_connection_contract::token_refused(&err), "{err:#}");
    slack::check_app_token(APP).await.unwrap();
    let err = format!("{:#}", slack::check_app_token(BOT).await.unwrap_err());
    assert!(err.contains("connections:write"), "{err}");

    let events = [
        json!({"type":"message","channel_type":"channel","user":"U0EVE","text":"K7Q2M9XA"}),
        json!({"type":"message","channel_type":"im","user":"U0BOT","bot_id":"B01","text":"K7Q2M9XA"}),
        json!({"type":"message","channel_type":"im","user":"U0EVE","text":"AAK7Q2M9XAZZ"}),
        json!({"type":"message","channel_type":"im","user":"U0DANA","subtype":"message_changed","text":"K7Q2M9XA"}),
        json!({"type":"message","channel_type":"im","channel":"D0DANA","user":"U0DANA","text":" *k7q2m9xa* ","blocks":[{"type":"rich_text","elements":[{"type":"rich_text_section","elements":[{"type":"text","text":"k7q2m9xa","style":{"bold":true}}]}]}]}),
    ];
    *slack.events.lock().unwrap() = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| {
            json!({
                "type":"events_api","envelope_id":i.to_string(),"payload":{"event":event}
            })
        })
        .collect();
    let mut pairing = slack::Pairing::open(APP).await.unwrap();
    let mut progress = Vec::new();
    let (id, name, dm) = pairing
        .owner(BOT, "K7Q2M9XA", Duration::from_secs(10), |step| {
            progress.push(step)
        })
        .await
        .unwrap();
    assert_eq!(
        (id.as_str(), name.as_str(), dm.as_str()),
        ("U0DANA", "Dana Q", "D0DANA")
    );
    // Every envelope is acknowledged, and only the human DM paired.
    assert_eq!(*slack.acked.lock().unwrap(), ["0", "1", "2", "3", "4"]);
    let posted = slack.posted.lock().unwrap().clone();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].0, "D0DANA");
    assert_eq!(
        progress,
        [
            slack::PairingProgress::Connected,
            slack::PairingProgress::OtherEvent,
            slack::PairingProgress::UnmatchedMessage,
            slack::PairingProgress::UnmatchedMessage,
            slack::PairingProgress::UnmatchedMessage,
            slack::PairingProgress::Confirming,
        ]
    );
    for step in &progress {
        let text = step.text();
        for hidden in [BOT, APP, "K7Q2M9XA", "U0DANA", "D0DANA"] {
            assert!(!text.contains(hidden), "pairing progress revealed {hidden}");
        }
    }
    *slack.events.lock().unwrap() = vec![json!({"type":"disconnect"})];
    let mut pairing = slack::Pairing::open(APP).await.unwrap();
    assert!(
        pairing
            .owner(BOT, "K7Q2M9XA", Duration::from_secs(10), |_| {})
            .await
            .unwrap_err()
            .to_string()
            .contains("closed Socket Mode")
    );
    slack.events.lock().unwrap().clear();
    let mut pairing = slack::Pairing::open(APP).await.unwrap();
    // A code nobody sent in its time is the contract's expired code, which
    // says so and names the command for a new one.
    let expired = pairing
        .owner(BOT, "K7Q2M9XA", Duration::ZERO, |_| {})
        .await
        .unwrap_err();
    assert!(
        expired
            .downcast_ref::<link_connection_contract::Expired>()
            .is_some(),
        "{expired:#}"
    );
}
