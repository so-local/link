//! An agent on a harness that speaks no connection answering on Discord, end to end: `lnk agent
//! start` runs the mini harness and, beside it, the Discord module carrying
//! the messages (`lnk-discord bridge`), against a fake Discord (its
//! HTTP API and a Gateway websocket) and a bare chat completions model.

#![cfg(unix)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{
        Path as UrlPath, State,
        ws::{Message as Frame, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get, patch, post},
};
use serde_json::{Value, json};

use link_harness_conformance::agent::{self, Agent, Shared, serve};

/// An agent on the mini harness, its programs the core's (built in
/// `link-core` beside this folder) and this folder's `lnk-discord`.
async fn start(name: &str, sandboxed: bool, section: &str, env: &[(&str, String)]) -> Agent {
    let programs = agent::programs_beside(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        std::path::Path::new(env!("CARGO_BIN_EXE_lnk-discord")),
    );
    agent::start_fixture(&programs, name, sandboxed, section, env, |_, _| {}).await
}

const TOKEN: &str = concat!(
    "MTIzNDU2Nzg5MDEyMzQ1Njc4",
    ".GabcDe.",
    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);
/// A token reset in the developer portal: Discord refuses it.
const REVOKED: &str = concat!(
    "MTIzNDU2Nzg5MDEyMzQ1Njc4",
    ".GabcDe.",
    "RevokedRevokedRevokedRevokedRevoked0123"
);
/// The owner's user id, and their direct message with the bot.
const OWNER: &str = "200000000000000002";
const DM: &str = "1200000000000000001";
const BOT: &str = "123456789012345678";
/// A channel in a server the bot is in.
const SERVER_CHANNEL: &str = "1200000000000000003";

/// What the fake Discord holds: Gateway events to hand out, and what the
/// bridge did.
#[derive(Default)]
struct Discord {
    /// Its own port, for the Gateway's address.
    port: u16,
    events: Vec<Value>,
    /// How many events were handed out.
    handed: usize,
    /// The Identify the Gateway got.
    identify: Value,
    heartbeats: usize,
    /// The HTTP calls with the bot token: method, path and body.
    calls: Vec<(Method, String, Value)>,
    /// Whether an edit was refused once, as Discord's rate limit does.
    limited: bool,
}

fn bot_token(headers: &HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bot "))
        .unwrap_or_default()
        .to_string()
}

async fn gateway_bot(State(discord): State<Shared<Discord>>, headers: HeaderMap) -> Json<Value> {
    assert_eq!(
        bot_token(&headers),
        TOKEN,
        "the Gateway asked with another token"
    );
    let port = discord.lock().unwrap().port;
    Json(json!({"url": format!("ws://127.0.0.1:{port}"), "shards": 1}))
}

async fn typing(
    State(discord): State<Shared<Discord>>,
    UrlPath(connection): UrlPath<String>,
    headers: HeaderMap,
) -> StatusCode {
    assert_eq!(bot_token(&headers), TOKEN, "typing with another token");
    let path = format!("/channels/{connection}/typing");
    discord
        .lock()
        .unwrap()
        .calls
        .push((Method::POST, path, Value::Null));
    StatusCode::NO_CONTENT
}

async fn create(
    State(discord): State<Shared<Discord>>,
    UrlPath(connection): UrlPath<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if bot_token(&headers) == REVOKED {
        let unauthorized = json!({"message": "401: Unauthorized", "code": 0});
        return (StatusCode::UNAUTHORIZED, Json(unauthorized)).into_response();
    }
    assert_eq!(bot_token(&headers), TOKEN, "posting with another token");
    let mut discord = discord.lock().unwrap();
    let id = format!("1400000000{:09}", discord.calls.len());
    let path = format!("/channels/{connection}/messages");
    discord.calls.push((Method::POST, path, body));
    Json(json!({"id": id, "channel_id": connection})).into_response()
}

async fn edit(
    State(discord): State<Shared<Discord>>,
    UrlPath((connection, id)): UrlPath<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    assert_eq!(bot_token(&headers), TOKEN, "editing with another token");
    let mut discord = discord.lock().unwrap();
    // The first edit is refused: the bridge waits and tries again.
    if !discord.limited {
        discord.limited = true;
        let limited = json!({"message": "You are being rate limited.", "retry_after": 0.3,
            "global": false});
        return (StatusCode::TOO_MANY_REQUESTS, Json(limited)).into_response();
    }
    let path = format!("/channels/{connection}/messages/{id}");
    discord.calls.push((Method::PATCH, path, body));
    Json(json!({"id": id, "channel_id": connection})).into_response()
}

/// The Gateway: Hello, then READY once identified, then each event as
/// it's queued; heartbeats are acknowledged.
/// A 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f,
    0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A message's file on the CDN, which is never sent the bot's token.
async fn attachment(headers: HeaderMap) -> Response {
    assert!(
        headers.get("authorization").is_none(),
        "the CDN got a token"
    );
    PNG.into_response()
}

async fn gateway(State(discord): State<Shared<Discord>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| hand_out(discord, socket))
}

async fn hand_out(discord: Shared<Discord>, mut socket: WebSocket) {
    let send = |v: Value| Frame::text(v.to_string());
    let hello = json!({"op": 10, "d": {"heartbeat_interval": 1000}});
    if socket.send(send(hello)).await.is_err() {
        return;
    }
    let mut ready = false;
    let mut sequence = 1;
    loop {
        let next = {
            let mut discord = discord.lock().unwrap();
            let next = discord.events.get(discord.handed).cloned();
            if ready && next.is_some() {
                discord.handed += 1;
            }
            next.filter(|_| ready)
        };
        if let Some(message) = next {
            sequence += 1;
            let event = json!({"op": 0, "s": sequence, "t": "MESSAGE_CREATE", "d": message});
            if socket.send(send(event)).await.is_err() {
                return;
            }
            continue;
        }
        let text = match tokio::time::timeout(Duration::from_millis(50), socket.recv()).await {
            Ok(Some(Ok(Frame::Text(text)))) => text,
            Ok(None | Some(Err(_)) | Some(Ok(Frame::Close(_)))) => return,
            _ => continue,
        };
        let event: Value = serde_json::from_str(&text).unwrap();
        match event["op"].as_u64() {
            Some(2) => {
                discord.lock().unwrap().identify = event["d"].clone();
                let user = json!({"id": BOT, "username": "link-main", "bot": true});
                let event = json!({"op": 0, "s": 1, "t": "READY", "d": {"v": 10, "user": user}});
                if socket.send(send(event)).await.is_err() {
                    return;
                }
                ready = true;
            }
            Some(1) => {
                discord.lock().unwrap().heartbeats += 1;
                if socket.send(send(json!({"op": 11}))).await.is_err() {
                    return;
                }
            }
            _ => {}
        }
    }
}

/// A message `user` sent in `channel`.
fn message(id: &str, user: &str, connection: &str, text: &str) -> Value {
    json!({"id": id, "channel_id": connection, "type": 0, "content": text,
        "attachments": [], "author": {"id": user, "username": "someone"}})
}

/// The messages in `channel` as they're shown now: each posted one, as
/// last edited.
fn shown_in(discord: &Shared<Discord>, connection: &str) -> Vec<String> {
    let discord = discord.lock().unwrap();
    let posts = format!("/channels/{connection}/messages");
    let mut messages: Vec<(String, String)> = Vec::new();
    for (i, (method, path, body)) in discord.calls.iter().enumerate() {
        let text = body["content"].as_str().unwrap_or_default().to_string();
        if *method == Method::POST && *path == posts {
            messages.push((format!("1400000000{i:09}"), text));
        } else if *method == Method::PATCH
            && let Some(id) = path.strip_prefix(&format!("{posts}/"))
        {
            let message = messages.iter_mut().find(|(m, _)| m == id);
            message.expect("an edit of a message never posted").1 = text;
        }
    }
    messages.into_iter().map(|(_, text)| text).collect()
}

/// Waits until the owner's direct message ends with `ending`.
async fn shown_on_discord(discord: &Shared<Discord>, ending: &[&str], agent: &mut Agent) {
    shown_in_channel(discord, DM, ending, agent).await
}

/// Waits until `channel` ends with `ending`. Threads are answered side by
/// side, so one being done says nothing of another.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn shown_in_channel(
    discord: &Shared<Discord>,
    connection: &str,
    ending: &[&str],
    agent: &mut Agent,
) {
    let start = Instant::now();
    loop {
        let shown = shown_in(discord, connection);
        if shown.ends_with(&ending.iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
            return;
        }
        let head: Vec<String> = shown.iter().map(|s| s.chars().take(80).collect()).collect();
        agent.check(start, &format!("waiting for {ending:.80?}; shown {head:?}"));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn talk_on_discord(name: &str, sandboxed: bool) {
    let discord: Shared<Discord> = Arc::default();
    let port = serve(
        Router::new()
            .route("/", any(gateway))
            .route("/api/gateway/bot", get(gateway_bot))
            .route("/api/channels/{connection}/typing", post(typing))
            .route("/api/channels/{connection}/messages", post(create))
            .route("/api/channels/{connection}/messages/{id}", patch(edit))
            .route("/api/attachments/a.png", get(attachment))
            .with_state(discord.clone()),
    )
    .await;
    discord.lock().unwrap().port = port;
    let section = agent::connection(
        "discord",
        &["bot_token"],
        &format!("bot_token = \"{TOKEN}\"\nallowed_users = [\"{OWNER}\"]"),
        "",
    );
    let api = format!("http://127.0.0.1:{port}/api");
    let mut agent = start(
        name,
        sandboxed,
        &section,
        &[("LNK_DISCORD_API", api.clone())],
    )
    .await;

    // A stranger, the owner in a server, and a bot; then the owner says
    // hello, and Discord sends it again.
    let mut server = message(
        "1300000000000000002",
        OWNER,
        "1200000000000000003",
        "Hi, server",
    );
    server["guild_id"] = json!("900000000000000001");
    let mut bots = message("1300000000000000003", OWNER, DM, "a bot's");
    bots["author"]["bot"] = json!(true);
    let hello = message("1300000000000000004", OWNER, DM, "Hello");
    // A stranger's mention of the bot in a server, and the owner's, which
    // Discord sends again.
    let mention = |id: &str, user: &str| {
        let mut mention = message(id, user, SERVER_CHANNEL, &format!("<@{BOT}> hello"));
        mention["guild_id"] = json!("900000000000000001");
        mention["mentions"] = json!([{"id": BOT, "username": "link-main", "bot": true}]);
        mention
    };
    let owners_mention = mention("1300000000000000012", OWNER);
    discord.lock().unwrap().events.extend([
        message(
            "1300000000000000001",
            "999999999999999999",
            "1200000000000000002",
            "I'm not the owner",
        ),
        server,
        bots,
        mention("1300000000000000011", "999999999999999999"),
        owners_mention.clone(),
        owners_mention,
        hello.clone(),
        hello,
    ]);
    shown_on_discord(&discord, &["I see 1 messages; the last: Hello"], &mut agent).await;
    let answer = ["I see 1 messages; the last: hello"];
    shown_in_channel(&discord, SERVER_CHANNEL, &answer, &mut agent).await;
    assert_eq!(
        shown_in(&discord, SERVER_CHANNEL),
        ["I see 1 messages; the last: hello"]
    );
    {
        let discord = discord.lock().unwrap();
        let calls: Vec<(&str, &str, &str)> = discord
            .calls
            .iter()
            .map(|(m, p, b)| (m.as_str(), p.as_str(), b["content"].as_str().unwrap_or("")))
            .collect();
        // "Typing" while the answer is on its way.
        let typing = format!("/channels/{DM}/typing");
        assert!(calls.iter().any(|(_, p, _)| *p == typing), "{calls:?}");
        // Streamed: the first piece posted, then edited into the answer.
        let posts = format!("/channels/{DM}/messages");
        assert!(calls.contains(&("POST", &posts, "I see")), "{calls:?}");
        assert!(
            calls
                .iter()
                .any(|(m, p, _)| *m == "PATCH" && p.starts_with(&posts)),
            "{calls:?}"
        );
        // Nothing goes to anyone else.
        let elsewhere: Vec<&(Method, String, Value)> = discord
            .calls
            .iter()
            .filter(|(_, p, _)| {
                !p.starts_with(&format!("/channels/{DM}/"))
                    && !p.starts_with(&format!("/channels/{SERVER_CHANNEL}/"))
            })
            .collect();
        assert!(elsewhere.is_empty(), "someone else was answered: {calls:?}");
        // The owner's mention is answered once, as a reply to it, "typing"
        // shown first, and edited into the answer after a rate limit.
        let server_posts = format!("/channels/{SERVER_CHANNEL}/messages");
        let posted: Vec<&Value> = discord
            .calls
            .iter()
            .filter(|(m, p, _)| *m == Method::POST && *p == server_posts)
            .map(|(_, _, b)| b)
            .collect();
        assert_eq!(posted.len(), 1, "{calls:?}");
        assert_eq!(
            posted[0]["message_reference"]["message_id"],
            "1300000000000000012"
        );
        let typing = format!("/channels/{SERVER_CHANNEL}/typing");
        assert!(calls.iter().any(|(_, p, _)| *p == typing), "{calls:?}");
        assert!(discord.limited);
        // As the bot, for direct messages and servers' messages, with a
        // heartbeat kept.
        assert_eq!(discord.identify["token"], TOKEN);
        assert_eq!(discord.identify["intents"], (1 << 12) | (1 << 9));
    }

    // It remembers the thread: the model is sent the conversation.
    let next = message("1300000000000000005", OWNER, DM, "And again");
    discord.lock().unwrap().events.push(next);
    shown_on_discord(
        &discord,
        &["I see 3 messages; the last: And again"],
        &mut agent,
    )
    .await;
    agent.remembered(&format!("discord-{DM}"), &[TOKEN], 3);
    // The mention without the bot's, in the channel's own thread.
    agent.asked_in_group(&format!("discord-{SERVER_CHANNEL}"), "hello");

    // An answer past Discord's limit is split into more messages, between
    // words where it can: a word longer than one message is cut.
    let long = "x".repeat(2100);
    let next = message("1300000000000000006", OWNER, DM, &long);
    discord.lock().unwrap().events.push(next);
    let (head, tail) = long.split_at(2000);
    shown_on_discord(
        &discord,
        &["I see 5 messages; the last:", head, tail],
        &mut agent,
    )
    .await;

    // A file it can't get: the agent is told nothing, the owner why.
    let mut file = message("1300000000000000007", OWNER, DM, "");
    file["attachments"] = json!([{"id": "1", "filename": "a.png"}]);
    discord.lock().unwrap().events.push(file);
    let unread = "I can read text, images (PNG, JPEG, GIF or WebP), documents (PDF or text) \
                  and voice notes only, for now.";
    shown_on_discord(&discord, &[unread], &mut agent).await;

    // A picture from the CDN reaches the agent, its caption with it.
    let mut picture = message("1300000000000000008", OWNER, DM, "what's this?");
    let url = format!("{api}/attachments/a.png");
    picture["attachments"] = json!([{"id": "2", "filename": "a.png",
        "content_type": "image/png", "size": PNG.len(), "url": url}]);
    discord.lock().unwrap().events.push(picture);
    // Shown as written: Discord's `[` escaped, so it opens no link.
    let seen = "I see 7 messages; the last: what's this?\n\\[image/png, 70 bytes]";
    shown_on_discord(&discord, &[seen], &mut agent).await;

    let discord = discord.lock().unwrap();
    // Every message sent could mention no one.
    for (method, path, body) in &discord.calls {
        if !path.ends_with("/typing") {
            assert_eq!(
                body["allowed_mentions"],
                json!({"parse": []}),
                "{method} {path}"
            );
        }
    }
    assert!(discord.heartbeats > 0, "no heartbeat");
    drop(discord);
    // A reset token is refused as such.
    agent.refuses_token(
        "discord",
        &json!({"bot_token": REVOKED, "allowed_users": [OWNER], "dm_channel": "1200000000000000009"}),
        "1200000000000000009",
        &[("LNK_DISCORD_API", api), ("NO_PROXY", "*".into())],
    );
    agent.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_on_the_mini_harness_answers_on_discord() {
    talk_on_discord("discord-open", false).await;
}

/// The same from the sandbox (Linux, bubblewrap).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn it_answers_on_discord_from_its_sandbox() {
    if agent::bubblewrap() {
        talk_on_discord("discord-sandboxed", true).await;
    }
}
