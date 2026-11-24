//! `lnk-discord hold` against a fake Discord: two bots' Gateways held in
//! one process, their messages written as each one's key; one let go,
//! whose connection closes; one cut, which resumes its session and gets
//! the message it missed.

link_test_kit::program!(system);

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::Uri,
    response::Response,
    routing::{any, get},
};
use link_connection_contract::pushed::{Held, HoldLine, HoldState};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Two bots: their ids are their tokens' first parts, in base64.
const A: &str = concat!(
    "MTExMTExMTExMTExMTExMTEx",
    ".GabcDe.",
    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);
const B: &str = concat!(
    "MjIyMjIyMjIyMjIyMjIyMjIy",
    ".GabcDe.",
    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);
/// A bot whose token Discord refuses: its Gateway closes with 4004.
const C: &str = concat!(
    "MzMzMzMzMzMzMzMzMzMzMzMz",
    ".GabcDe.",
    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);

/// What the fake Gateway does to a bot's connection.
enum Do {
    Send(Value),
    /// Drops it, as a cut would.
    Cut,
}

#[derive(Clone, Default)]
struct Gateway {
    addr: Arc<Mutex<Option<SocketAddr>>>,
    /// Each bot's open connection, by token.
    open: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Do>>>>,
    /// What each connection began with (Identify, op 2, or Resume, op
    /// 6), and the path it came to.
    hellos: Arc<Mutex<Vec<(String, Value)>>>,
    /// Tokens whose connection Link closed.
    closed: Arc<Mutex<Vec<String>>>,
    /// The message a bot's session missed while it was cut, sent on its
    /// resume.
    missed: Arc<Mutex<HashMap<String, Value>>>,
}

async fn gateway_bot(State(gateway): State<Gateway>) -> Json<Value> {
    let addr = gateway.addr.lock().unwrap().unwrap();
    Json(json!({"url": format!("ws://{addr}")}))
}

async fn connect(State(gateway): State<Gateway>, uri: Uri, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| session(gateway, uri.path().to_string(), socket))
}

fn text(v: Value) -> Message {
    Message::Text(v.to_string().into())
}

async fn session(gateway: Gateway, path: String, mut ws: WebSocket) {
    let _ = ws
        .send(text(json!({"op": 10, "d": {"heartbeat_interval": 45000}})))
        .await;
    let hello = loop {
        let Some(Ok(Message::Text(frame))) = ws.recv().await else {
            return;
        };
        let event: Value = serde_json::from_str(frame.as_str()).unwrap();
        if event["op"] == 2 || event["op"] == 6 {
            break event;
        }
    };
    let token = hello["d"]["token"].as_str().unwrap().to_string();
    if token == C {
        // Authentication failed.
        let close = axum::extract::ws::CloseFrame {
            code: 4004,
            reason: "Authentication failed.".into(),
        };
        let _ = ws.send(Message::Close(Some(close))).await;
        return;
    }
    gateway.hellos.lock().unwrap().push((path, hello.clone()));
    let id = &token[..2];
    if hello["op"] == 2 {
        let addr = gateway.addr.lock().unwrap().unwrap();
        let ready = json!({"op": 0, "s": 1, "t": "READY", "d": {
            "session_id": format!("session-{id}"),
            "resume_gateway_url": format!("ws://{addr}/resume"),
            "user": {"id": format!("bot-{id}")},
        }});
        let _ = ws.send(text(ready)).await;
    } else {
        // What it missed, then that it's resumed.
        let seq = hello["d"]["seq"].as_u64().unwrap_or(0);
        let missed = gateway.missed.lock().unwrap().remove(&token);
        if let Some(missed) = missed {
            let event = json!({"op": 0, "s": seq + 1, "t": "MESSAGE_CREATE", "d": missed});
            let _ = ws.send(text(event)).await;
        }
        let resumed = json!({"op": 0, "s": seq + 2, "t": "RESUMED", "d": null});
        let _ = ws.send(text(resumed)).await;
    }
    let (doing, mut todo) = mpsc::unbounded_channel();
    gateway.open.lock().unwrap().insert(token.clone(), doing);
    loop {
        tokio::select! {
            frame = ws.recv() => match frame {
                Some(Ok(_)) => {}
                _ => {
                    gateway.closed.lock().unwrap().push(token);
                    return;
                }
            },
            what = todo.recv() => match what {
                Some(Do::Send(event)) => {
                    let _ = ws.send(text(event)).await;
                }
                _ => return,
            },
        }
    }
}

fn message(id: &str) -> Value {
    json!({"id": id, "channel_id": "1200000000000000001", "type": 0, "content": "hi",
        "author": {"id": "200000000000000002", "username": "dana"}})
}

/// Sends `event` on bot `token`'s open connection.
fn push(gateway: &Gateway, token: &str, event: Value) {
    let open = gateway.open.lock().unwrap();
    open[token].send(Do::Send(event)).ok().unwrap();
}

/// The next line `hold` writes.
async fn next(held: &mut mpsc::Receiver<Held>) -> Held {
    tokio::time::timeout(Duration::from_secs(10), held.recv())
        .await
        .expect("hold wrote nothing")
        .expect("hold ended")
}

fn state(key: &str, state: HoldState) -> Held {
    Held {
        key: key.into(),
        update: None,
        state: Some(state),
    }
}

fn update(key: &str, id: &str) -> Held {
    Held {
        key: key.into(),
        update: Some(message(id)),
        state: None,
    }
}

#[tokio::test]
async fn one_process_holds_many_bots_and_resumes_a_cut() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let gateway = Gateway::default();
    *gateway.addr.lock().unwrap() = Some(addr);
    let app = Router::new()
        .route("/", any(connect))
        .route("/resume/", any(connect))
        .route("/api/gateway/bot", get(gateway_bot))
        .with_state(gateway.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });
    // SAFETY: the only test in this binary; nothing else reads the environment.
    unsafe {
        std::env::set_var("LNK_DISCORD_API", format!("http://{addr}/api"));
        std::env::set_var("NO_PROXY", "*");
    }

    let (lines, read) = mpsc::channel(8);
    let (write, mut held) = mpsc::channel(64);
    let holding = tokio::spawn(link_discord::held::hold(read, write));
    for (key, token) in [("a", A), ("b", B)] {
        let settings = json!({"bot_token": token, "allowed_users": ["200000000000000002"]});
        let line = HoldLine::Hold {
            key: key.into(),
            settings: settings.into(),
        };
        lines.send(line).await.unwrap();
    }
    let mut up = [next(&mut held).await, next(&mut held).await];
    up.sort_by(|x, y| x.key.cmp(&y.key));
    assert_eq!(up, [state("a", HoldState::Up), state("b", HoldState::Up)]);
    // Each identified as a bot, with the bridge's intents.
    for (path, hello) in gateway.hellos.lock().unwrap().iter() {
        assert_eq!((path.as_str(), &hello["op"]), ("/", &json!(2)));
        assert_eq!(hello["d"]["intents"], (1 << 12) | (1 << 9));
    }

    // Each bot's message, as its key.
    push(
        &gateway,
        A,
        json!({"op": 0, "s": 2, "t": "MESSAGE_CREATE", "d": message("31")}),
    );
    assert_eq!(next(&mut held).await, update("a", "31"));
    push(
        &gateway,
        B,
        json!({"op": 0, "s": 2, "t": "MESSAGE_CREATE", "d": message("32")}),
    );
    assert_eq!(next(&mut held).await, update("b", "32"));

    // Let go of b: its connection closes.
    lines.send(HoldLine::Drop("b".into())).await.unwrap();
    link_test_kit::eventually_async("b's connection to close", Duration::from_secs(10), || {
        gateway.closed.lock().unwrap().iter().any(|t| t == B)
    })
    .await;

    // Cut a, which misses a message while it's down: it resumes its
    // session where the last one ended, and gets it.
    gateway
        .missed
        .lock()
        .unwrap()
        .insert(A.into(), message("33"));
    let cut = gateway.open.lock().unwrap().remove(A).unwrap();
    cut.send(Do::Cut).ok().unwrap();
    assert_eq!(next(&mut held).await, state("a", HoldState::Down));
    assert_eq!(next(&mut held).await, update("a", "33"));
    assert_eq!(next(&mut held).await, state("a", HoldState::Up));
    let hellos = gateway.hellos.lock().unwrap().clone();
    let (path, resume) = hellos.last().unwrap();
    assert_eq!(path, "/resume/");
    assert_eq!(
        resume,
        &json!({"op": 6, "d": {"token": A, "session_id": "session-MT", "seq": 2}})
    );
    // a identified once, b once; nothing more from b.
    assert_eq!(hellos.iter().filter(|(_, h)| h["op"] == 2).count(), 2);

    // A bot whose token Discord refuses: refused, not merely down, and
    // said once however often it tries again.
    let settings = json!({"bot_token": C, "allowed_users": ["200000000000000002"]});
    lines
        .send(HoldLine::Hold {
            key: "c".into(),
            settings: settings.into(),
        })
        .await
        .unwrap();
    assert_eq!(next(&mut held).await, state("c", HoldState::Refused));
    lines.send(HoldLine::Drop("c".into())).await.unwrap();

    // Its input ends: every hold is let go, and it returns.
    drop(lines);
    tokio::time::timeout(Duration::from_secs(10), holding)
        .await
        .expect("hold didn't end")
        .unwrap()
        .unwrap();
    link_test_kit::eventually_async("a's connection to close", Duration::from_secs(10), || {
        gateway.closed.lock().unwrap().iter().any(|t| t == A)
    })
    .await;
    while let Ok(line) = held.try_recv() {
        assert_ne!(line.key, "b", "b wrote after it was let go: {line:?}");
        assert_ne!(line.key, "c", "c wrote after it was refused: {line:?}");
    }
}
