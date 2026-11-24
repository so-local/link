//! `lnk-discord share` against a fake Discord: on its Gateway, only the
//! owner's mention of the bot with the share's code, in a server's
//! channel, shares that channel; the bot says so there, and the channel
//! is printed with its name. The code is never printed.

#![cfg(unix)]

use std::{
    io::Write,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::{any, get, post},
};
use serde_json::{Value, json};

const TOKEN: &str = concat!(
    "MTIzNDU2Nzg5MDEyMzQ1Njc4",
    ".GabcDe.",
    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);
const BOT: &str = "123456789012345678";
const OWNER: &str = "200000000000000002";
const CODE: &str = "K7Q2M9XA";
/// The channel shared, in a server the bot is in.
const TEAM: &str = "1200000000000000003";

#[derive(Clone, Default)]
struct Discord {
    port: u16,
    /// The Gateway events to hand out once READY is sent.
    events: Arc<Mutex<Vec<Value>>>,
    /// What the bot posted: its channel and body.
    posted: Arc<Mutex<Vec<(String, Value)>>>,
    /// The Identify the Gateway got.
    identify: Arc<Mutex<Value>>,
}

async fn gateway_bot(State(discord): State<Discord>) -> Json<Value> {
    Json(json!({"url": format!("ws://127.0.0.1:{}", discord.port)}))
}

/// A thread of the shared channel's, which has an id of its own.
const THREAD: &str = "1200000000000000008";

async fn connection(Path(connection): Path<String>) -> Json<Value> {
    // A text channel, or a public thread.
    let kind = if connection == THREAD { 11 } else { 0 };
    Json(json!({"id": connection, "name": "team", "type": kind, "guild_id": "900000000000000001"}))
}

async fn post_message(
    State(discord): State<Discord>,
    Path(connection): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    discord.posted.lock().unwrap().push((connection, body));
    Json(json!({"id": "1400000000000000001"}))
}

async fn gateway(State(discord): State<Discord>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| session(discord, socket))
}

async fn session(discord: Discord, mut ws: WebSocket) {
    let send = |v: Value| Message::Text(v.to_string().into());
    let _ = ws
        .send(send(json!({"op": 10, "d": {"heartbeat_interval": 45000}})))
        .await;
    while let Some(Ok(Message::Text(text))) = ws.recv().await {
        let event: Value = serde_json::from_str(text.as_str()).unwrap();
        if event["op"] == 2 {
            *discord.identify.lock().unwrap() = event["d"].clone();
            break;
        }
    }
    let user = json!({"id": BOT, "username": "link-main", "bot": true});
    let _ = ws
        .send(send(
            json!({"op": 0, "s": 1, "t": "READY", "d": {"user": user}}),
        ))
        .await;
    let events = discord.events.lock().unwrap().clone();
    for (s, message) in events.into_iter().enumerate() {
        let event = json!({"op": 0, "s": s + 2, "t": "MESSAGE_CREATE", "d": message});
        let _ = ws.send(send(event)).await;
    }
    // Until Link hangs up.
    while let Some(Ok(_)) = ws.recv().await {}
}

/// A message from `user` in `channel` of a server, mentioning the bot
/// when `mention`.
fn said(id: &str, user: &str, connection: &str, text: &str, mention: bool) -> Value {
    let mentions = match mention {
        true => json!([{"id": BOT, "bot": true}]),
        false => json!([]),
    };
    json!({"id": id, "channel_id": connection, "guild_id": "900000000000000001", "type": 0,
        "content": text, "author": {"id": user, "username": "dana"}, "mentions": mentions})
}

async fn fake(events: Vec<Value>) -> Discord {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let discord = Discord {
        port: listener.local_addr().unwrap().port(),
        ..Discord::default()
    };
    *discord.events.lock().unwrap() = events;
    let app = Router::new()
        .route("/", any(gateway))
        .route("/api/gateway/bot", get(gateway_bot))
        .route("/api/channels/{connection}", get(connection))
        .route("/api/channels/{connection}/messages", post(post_message))
        .with_state(discord.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });
    discord
}

/// `lnk-discord share`, handed the bot's settings and the code, waiting
/// `wait_secs`.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn share(discord: &Discord, wait_secs: u64) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lnk-discord"))
        .arg("share")
        .env(
            "LNK_DISCORD_API",
            format!("http://127.0.0.1:{}/api", discord.port),
        )
        .env("NO_PROXY", "*")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let line = json!({
        "settings": {"bot_token": TOKEN, "allowed_users": [OWNER], "dm_channel": "1200000000000000001"},
        "code": CODE,
        "wait_secs": wait_secs,
    });
    writeln!(child.stdin.take().unwrap(), "{line}").unwrap();
    child.wait_with_output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_owners_mention_with_the_code_shares_a_channel() {
    let mention = format!("<@{BOT}> {}", CODE.to_lowercase());
    let mut bots = said(
        "1300000000000000003",
        OWNER,
        "1200000000000000005",
        &mention,
        true,
    );
    bots["author"]["bot"] = json!(true);
    let mut direct = said(
        "1300000000000000004",
        OWNER,
        "1200000000000000001",
        CODE,
        false,
    );
    direct["guild_id"] = json!(null);
    let discord = fake(vec![
        // A stranger with the code, the owner not mentioning the bot, a
        // bot with the owner's id, the code in the owner's direct message,
        // and a wrong code: none shares.
        said(
            "1300000000000000001",
            "999999999999999999",
            "1200000000000000004",
            &mention,
            true,
        ),
        said(
            "1300000000000000002",
            OWNER,
            "1200000000000000006",
            CODE,
            false,
        ),
        bots,
        direct,
        said(
            "1300000000000000005",
            OWNER,
            "1200000000000000007",
            &format!("<@{BOT}> K7Q2M9XB"),
            true,
        ),
        // The owner's mention with it in a thread: refused, saying so.
        said("1300000000000000007", OWNER, THREAD, &mention, true),
        // Then the owner's mention with it, in any case.
        said("1300000000000000006", OWNER, TEAM, &mention, true),
    ])
    .await;
    let out = tokio::task::spawn_blocking({
        let discord = discord.clone();
        move || share(&discord, 30)
    })
    .await
    .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out.status.success(), "{stderr}");
    let shared: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        shared,
        json!({"chat": format!("chat:discord:{TEAM}"), "name": "#team"})
    );
    for printed in [&stdout, &stderr] {
        assert!(!printed.to_uppercase().contains(CODE), "{printed}");
    }
    // The thread told it can't be shared; then said in the channel alone,
    // each as a reply that can mention no one.
    let mut posted = discord.posted.lock().unwrap().clone();
    assert_eq!(posted.len(), 2, "{posted:?}");
    assert_eq!(posted[0].0, THREAD);
    let refused = posted[0].1["content"].as_str().unwrap();
    assert!(refused.contains("A thread can't be shared"), "{refused}");
    assert_eq!(posted[0].1["allowed_mentions"], json!({"parse": []}));
    posted.remove(0);
    assert_eq!(posted[0].0, TEAM);
    assert!(
        posted[0].1["content"]
            .as_str()
            .unwrap()
            .starts_with("Shared with #team"),
        "{posted:?}"
    );
    assert_eq!(
        posted[0].1["message_reference"]["message_id"],
        "1300000000000000006"
    );
    assert_eq!(posted[0].1["allowed_mentions"], json!({"parse": []}));
    // As the bot, for servers' messages alone.
    let identify = discord.identify.lock().unwrap().clone();
    assert_eq!(identify["token"], TOKEN);
    assert_eq!(identify["intents"], 1 << 9);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_code_in_time_shares_nothing_and_says_so() {
    let discord = fake(vec![said(
        "1300000000000000001",
        "999999999999999999",
        TEAM,
        &format!("<@{BOT}> {CODE}"),
        true,
    )])
    .await;
    let out = tokio::task::spawn_blocking({
        let discord = discord.clone();
        move || share(&discord, 1)
    })
    .await
    .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(stderr.contains("lnk agent share add discord"), "{stderr}");
    assert!(!stderr.contains(CODE), "{stderr}");
    assert!(discord.posted.lock().unwrap().is_empty());
}

/// `describe` says a Discord channel can be shared.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn describe_says_it_shares() {
    let out = Command::new(env!("CARGO_BIN_EXE_lnk-discord"))
        .arg("describe")
        .output()
        .unwrap();
    let describe: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(describe["share"], true);
}
