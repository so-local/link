//! `lnk agent connect discord` against a fake Discord: the token is checked
//! with /users/@me and the application's flags, and on a fake Gateway the
//! first person to send the bot the one-time code in a direct message
//! becomes the only one allowed.

link_test_kit::program!(system);

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use serde_json::{Value, json};

const TOKEN: &str = concat!(
    "MTIzNDU2Nzg5MDEyMzQ1Njc4",
    ".GabcDe.",
    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);
/// A bot whose application has no Message Content intent.
const PLAIN: &str = concat!(
    "OTg3",
    "NjU0MzIxMDk4NzY1NDMy.GabcDe.AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);

#[derive(Clone)]
struct Discord {
    addr: SocketAddr,
    /// What the bot posted, by channel.
    posted: Arc<Mutex<Vec<(String, String)>>>,
    /// The Identify the Gateway got.
    identify: Arc<Mutex<Value>>,
}

fn authorized(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bot "))
        .filter(|t| *t == TOKEN || *t == PLAIN)
}

async fn me(headers: HeaderMap) -> Response {
    match authorized(&headers) {
        Some(TOKEN) => {
            Json(json!({"id": "123456789012345678", "username": "link-main", "bot": true}))
                .into_response()
        }
        Some(_) => Json(json!({"id": "987654321098765432", "username": "plain", "bot": true}))
            .into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"message": "401: Unauthorized"})),
        )
            .into_response(),
    }
}

async fn application(headers: HeaderMap) -> Json<Value> {
    // 1 << 19: Message Content, for bots in under 100 servers.
    let flags = if authorized(&headers) == Some(TOKEN) {
        1 << 19
    } else {
        0
    };
    Json(json!({"id": "123456789012345678", "flags": flags}))
}

async fn gateway_bot(State(discord): State<Discord>) -> Json<Value> {
    Json(json!({"url": format!("ws://{}", discord.addr)}))
}

async fn post_message(
    State(discord): State<Discord>,
    Path(connection): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    discord.posted.lock().unwrap().push((
        connection,
        body["content"].as_str().unwrap_or_default().to_string(),
    ));
    Json(json!({"id": "1"}))
}

async fn gateway(State(discord): State<Discord>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| session(discord, socket))
}

async fn session(discord: Discord, mut ws: WebSocket) {
    let send = |v: Value| Message::Text(v.to_string().into());
    let _ = ws
        .send(send(json!({"op": 10, "d": {"heartbeat_interval": 45000}})))
        .await;
    // Identify.
    while let Some(Ok(Message::Text(text))) = ws.recv().await {
        let event: Value = serde_json::from_str(text.as_str()).unwrap();
        if event["op"] == 2 {
            *discord.identify.lock().unwrap() = event["d"].clone();
            break;
        }
    }
    let _ = ws
        .send(send(json!({"op": 0, "s": 1, "t": "READY", "d": {}})))
        .await;
    let dana = json!({"id": "200000000000000002", "username": "dana", "global_name": "Dana"});
    for (s, message) in [
        // The code in a server, a bot with it, a stranger with a message
        // holding it among others (trying many codes at once), and then
        // the owner, in a direct message, with it alone.
        json!({"author": dana, "guild_id": "9", "channel_id": "10", "content": "K7Q2M9XA"}),
        json!({"author": {"id": "5", "username": "b", "bot": true}, "channel_id": "11", "content": "K7Q2M9XA"}),
        json!({"author": {"id": "6", "username": "eve"}, "channel_id": "12", "content": "AAK7Q2M9XAZZ"}),
        json!({"author": dana, "channel_id": "13", "content": " k7q2m9xa\n"}),
    ]
    .into_iter()
    .enumerate()
    {
        let _ = ws
            .send(send(json!({"op": 0, "s": s + 2, "t": "MESSAGE_CREATE", "d": message})))
            .await;
    }
    // Until Link hangs up.
    while let Some(Ok(_)) = ws.recv().await {}
}

#[tokio::test]
async fn whoever_sends_the_code_directly_becomes_the_owner() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let discord = Discord {
        addr,
        posted: Arc::default(),
        identify: Arc::default(),
    };
    let app = Router::new()
        .route("/", any(gateway))
        .route("/api/users/@me", get(me))
        .route("/api/oauth2/applications/@me", get(application))
        .route("/api/gateway/bot", get(gateway_bot))
        .route("/api/channels/{connection}/messages", post(post_message))
        .with_state(discord.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });
    // SAFETY: the only test in this binary; nothing else reads the environment.
    unsafe {
        std::env::set_var("LNK_DISCORD_API", format!("http://{addr}/api"));
        std::env::set_var("NO_PROXY", "*");
    }

    use link_discord as discord;
    let bot = discord::bot(TOKEN).await.unwrap();
    assert_eq!(
        (bot.id.as_str(), bot.name.as_str(), bot.application.as_str()),
        ("123456789012345678", "link-main", "123456789012345678")
    );
    assert!(bot.message_content);
    assert!(!discord::bot(PLAIN).await.unwrap().message_content);
    let err = discord::bot("MTIz.wrong.token").await.unwrap_err();
    assert!(err.to_string().contains("401"), "{err}");
    assert!(link_connection_contract::token_refused(&err), "{err:#}");

    let ready = AtomicBool::new(false);
    let (id, name, dm_channel) = discord::owner(TOKEN, "K7Q2M9XA", Duration::from_secs(10), || {
        ready.store(true, Ordering::SeqCst)
    })
    .await
    .unwrap();
    assert_eq!((id.as_str(), name.as_str()), ("200000000000000002", "Dana"));
    // Their direct message, which a harness takes as its home channel.
    assert_eq!(dm_channel, "13");
    // The code was shown once Link listened, as the bot, for direct
    // messages only; the owner is told, in their direct message.
    assert!(ready.load(Ordering::SeqCst));
    let identify = discord.identify.lock().unwrap().clone();
    assert_eq!(identify["token"], TOKEN);
    assert_eq!(identify["intents"], 1 << 12);
    let posted = discord.posted.lock().unwrap().clone();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].0, "13");
}
