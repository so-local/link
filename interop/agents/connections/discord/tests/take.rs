//! `lnk-discord receive` against a fake Discord and a fake harness: a held
//! update from the owner goes to the harness's endpoint, its thread the
//! bridge's, and the answer is posted over Discord's HTTP API; a
//! member's mention in a channel the agent is shared with goes as theirs;
//! a stranger's is refused before either is asked, and an answer Discord
//! refuses fails as refused.

link_test_kit::program!(system);

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use link_connection_contract::{Bridge, pushed::Taking};
use link_secretive::Duplicate;
use serde_json::{Value, json};

const TOKEN: &str = concat!(
    "MTIzNDU2Nzg5MDEyMzQ1Njc4",
    ".GabcDe.",
    "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-"
);
const BOT: &str = "123456789012345678";
const OWNER: &str = "200000000000000002";
/// A channel Discord won't post in, and one it fails on.
const BLOCKED: &str = "1200000000000000403";
const DOWN: &str = "1200000000000000503";

#[derive(Clone, Default)]
struct Fakes {
    /// What the bot posted: its channel and body.
    posted: Arc<Mutex<Vec<(String, Value)>>>,
    /// Channels it showed typing in.
    typing: Arc<Mutex<Vec<String>>>,
    /// Each reaction added or removed: its method and path.
    reacted: Arc<Mutex<Vec<(String, String)>>>,
    /// What the harness was asked: its thread and the message.
    asked: Arc<Mutex<Vec<(String, String)>>>,
    /// Who each message was from (`Link-From`, `Link-From-Name`), when
    /// not the owner.
    from: Arc<Mutex<Vec<Sender>>>,
}

/// Who a message was from: its `Link-From` and `Link-From-Name`.
type Sender = (Option<String>, Option<String>);

async fn post_message(
    State(fakes): State<Fakes>,
    Path(connection): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match connection.as_str() {
        BLOCKED => {
            let said = json!({"message": "Cannot send messages to this user", "code": 50007});
            return (StatusCode::FORBIDDEN, Json(said)).into_response();
        }
        DOWN => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        _ => {}
    }
    fakes.posted.lock().unwrap().push((connection, body));
    Json(json!({"id": "1400000000000000001"})).into_response()
}

async fn typing(State(fakes): State<Fakes>, Path(connection): Path<String>) {
    fakes.typing.lock().unwrap().push(connection);
}

async fn react(
    State(fakes): State<Fakes>,
    method: axum::http::Method,
    uri: axum::http::Uri,
) -> StatusCode {
    let path = uri.path().trim_start_matches("/api").to_string();
    fakes
        .reacted
        .lock()
        .unwrap()
        .push((method.to_string(), path));
    StatusCode::NO_CONTENT
}

fn message(user: &str) -> Value {
    json!({"id": "1300000000000000001", "channel_id": "1200000000000000001",
        "type": 0, "content": "hello", "author": {"id": user, "username": "dana"}})
}

#[tokio::test]
async fn take_answers_the_owner_over_discords_api() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let fakes = Fakes::default();
    let app = Router::new()
        .route("/api/channels/{connection}/messages", post(post_message))
        .route("/api/channels/{connection}/typing", post(typing))
        .route(
            "/api/channels/{connection}/messages/{id}/reactions/{emoji}/@me",
            axum::routing::put(react).delete(react),
        )
        .with_state(fakes.clone());
    // The harness, on ACP: each message kept as its thread and text,
    // and who it's from.
    let harness = {
        let fakes = fakes.clone();
        link_connection_contract::acp::fake::serve_seen(
            |_| "hi there".into(),
            Vec::new(),
            move |seen| {
                if let link_connection_contract::acp::fake::Seen::Prompted(asked) = seen {
                    fakes
                        .asked
                        .lock()
                        .unwrap()
                        .push((asked.thread.clone(), asked.text.clone()));
                    let said = &asked.said;
                    fakes
                        .from
                        .lock()
                        .unwrap()
                        .push((said.from.clone(), said.from_name.clone()));
                }
            },
        )
        .await
    };
    tokio::spawn(async move { axum::serve(listener, app).await });
    // SAFETY: the only test in this binary; nothing else reads the environment.
    unsafe {
        std::env::set_var("LNK_DISCORD_API", format!("http://{addr}/api"));
        std::env::set_var("NO_PROXY", "*");
    }
    let state = link_test_kit::TempDir::new("discord-take");
    let bridge = Bridge {
        agent: "main".into(),
        endpoint: harness.endpoint.clone(),
        settings: json!({"bot_token": TOKEN, "allowed_users": [OWNER], "bot_id": BOT}).into(),
        state: state.to_path_buf(),
        ..Bridge::default()
    };
    let take = |update: Value| {
        link_discord::held::take(Taking {
            bridge: bridge.duplicate(),
            update,
        })
    };

    // A stranger's: refused, nobody asked.
    let err = take(message("999999999999999999")).await.unwrap_err();
    assert!(format!("{err:#}").contains("not answering"), "{err:#}");
    assert!(fakes.asked.lock().unwrap().is_empty());
    assert!(fakes.posted.lock().unwrap().is_empty());

    // The owner's direct message: asked on the bridge's thread, the
    // answer posted in it, after typing.
    take(message(OWNER)).await.unwrap();
    assert_eq!(
        fakes.asked.lock().unwrap().clone(),
        [(
            "discord:1200000000000000001".to_string(),
            "hello".to_string()
        )]
    );
    assert_eq!(fakes.typing.lock().unwrap()[0], "1200000000000000001");
    let posted = fakes.posted.lock().unwrap().clone();
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(posted[0].0, "1200000000000000001");
    assert_eq!(posted[0].1["content"], "hi there");
    assert!(posted[0].1["message_reference"].is_null());
    // No reaction unless the spec names one.
    assert!(fakes.reacted.lock().unwrap().is_empty());

    // Its spec names one: on the message while it's answered, gone once
    // it is.
    let reacting = Bridge {
        spec: Some(link_connection_contract::policy::Connection {
            working: Some("eyes".into()),
            ..Default::default()
        }),
        ..bridge.duplicate()
    };
    link_discord::held::take(Taking {
        bridge: reacting,
        update: message(OWNER),
    })
    .await
    .unwrap();
    let eyes =
        "/channels/1200000000000000001/messages/1300000000000000001/reactions/%F0%9F%91%80/@me";
    assert_eq!(
        fakes.reacted.lock().unwrap().clone(),
        [
            ("PUT".into(), eyes.into()),
            ("DELETE".into(), eyes.to_string())
        ]
    );
    fakes.asked.lock().unwrap().pop();
    fakes.from.lock().unwrap().pop();
    fakes.posted.lock().unwrap().pop();

    // Their mention in a server: without the mention, answered as a reply.
    let mut mention = message(OWNER);
    mention["guild_id"] = json!("900000000000000001");
    mention["channel_id"] = json!("1200000000000000003");
    mention["content"] = json!(format!("<@{BOT}> hello"));
    mention["mentions"] = json!([{"id": BOT, "bot": true}]);
    take(mention).await.unwrap();
    let asked = fakes.asked.lock().unwrap().clone();
    assert_eq!(
        asked[1],
        (
            "discord:1200000000000000003".to_string(),
            "hello".to_string()
        )
    );
    let posted = fakes.posted.lock().unwrap().clone();
    assert_eq!(posted[1].0, "1200000000000000003");
    assert_eq!(
        posted[1].1["message_reference"]["message_id"],
        "1300000000000000001"
    );
    // The owner's, both: from nobody else.
    assert_eq!(
        fakes.from.lock().unwrap().clone(),
        [(None, None), (None, None)]
    );

    // A member's mention: refused while the channel isn't shared, then,
    // shared, asked as theirs, by their name in the server.
    let mut member = message("300000000000000003");
    member["guild_id"] = json!("900000000000000001");
    member["channel_id"] = json!("1200000000000000005");
    member["content"] = json!(format!("<@{BOT}> what's due?"));
    member["mentions"] = json!([{"id": BOT, "bot": true}]);
    member["member"] = json!({"nick": "Ali Ünal"});
    let err = take(member.clone()).await.unwrap_err();
    assert!(format!("{err:#}").contains("not answering"), "{err:#}");
    assert_eq!(fakes.asked.lock().unwrap().len(), 2);
    let shared = Bridge {
        shared: vec!["1200000000000000005".into()],
        ..bridge.duplicate()
    };
    link_discord::held::take(Taking {
        bridge: shared.duplicate(),
        update: member.clone(),
    })
    .await
    .unwrap();
    assert_eq!(
        fakes.asked.lock().unwrap()[2],
        (
            "discord:1200000000000000005".to_string(),
            "what's due?".to_string()
        )
    );
    assert_eq!(
        fakes.from.lock().unwrap()[2],
        (
            Some("member:discord:300000000000000003".to_string()),
            Some("Ali Ünal".to_string())
        )
    );
    // Only the owner's channel is kept for answers nobody asked for: a
    // member's is sent to only while it's shared.
    assert!(link_connection_contract::knows_group(
        &state,
        "1200000000000000003"
    ));
    assert!(!link_connection_contract::knows_group(
        &state,
        "1200000000000000005"
    ));
    // Their direct message isn't shared.
    let mut direct = member;
    direct["guild_id"] = json!(null);
    let err = link_discord::held::take(Taking {
        bridge: shared,
        update: direct,
    })
    .await
    .unwrap_err();
    assert!(format!("{err:#}").contains("not answering"), "{err:#}");
    assert_eq!(fakes.asked.lock().unwrap().len(), 3);

    // An answer Discord refuses (the owner blocked the bot): `take` fails
    // as the answer refused, so it exits `REPLY_REFUSED`; Discord failing
    // isn't a refusal.
    let mut blocked = message(OWNER);
    blocked["channel_id"] = json!(BLOCKED);
    let err = take(blocked).await.unwrap_err();
    assert!(link_connection_contract::reply_refused(&err), "{err:#}");
    assert!(!link_connection_contract::token_refused(&err), "{err:#}");
    let mut down = message(OWNER);
    down["channel_id"] = json!(DOWN);
    if let Err(err) = take(down).await {
        assert!(!link_connection_contract::reply_refused(&err), "{err:#}");
    }
}
