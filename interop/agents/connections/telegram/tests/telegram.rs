//! `lnk agent connect telegram` against a fake Bot API: the token is checked with
//! getMe, older messages are skipped, and the first person to send the bot
//! the one-time code in a private chat becomes the only one allowed.

link_test_kit::program!(system);

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct Bot {
    /// Offsets getUpdates was called with.
    offsets: Arc<Mutex<Vec<String>>>,
}

async fn api(
    State(bot): State<Bot>,
    Path((token, method)): Path<(String, String)>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    // Another program polls this one.
    if token == "bot654321:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDbusy" && method == "getUpdates" {
        return Json(json!({"ok": false, "error_code": 409,
            "description": "Conflict: terminated by other getUpdates request"}));
    }
    if token != "bot123456:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw" {
        return Json(json!({"ok": false, "error_code": 401, "description": "Unauthorized"}));
    }
    match method.as_str() {
        "getMe" => Json(json!({"ok": true, "result": {"username": "my_ai_bot"}})),
        "getUpdates" => {
            let offset = query.get("offset").cloned().unwrap_or_default();
            bot.offsets.lock().unwrap().push(offset.clone());
            let result = match offset.as_str() {
                // The latest update: an old message from someone else.
                "-1" => {
                    json!([{"update_id": 7, "message": {"from": {"id": 666, "first_name": "Old"}}}])
                }
                "8" => json!([
                    // A bot, a stranger with a message holding the code
                    // among others (trying many at once), the code in a
                    // group, and then the owner, with it.
                    {"update_id": 8, "message": {"from": {"id": 99, "is_bot": true, "first_name": "Bot"},
                        "chat": {"type": "private"}, "text": "/start K7Q2M9XA"}},
                    {"update_id": 9, "message": {"from": {"id": 13, "first_name": "Eve"},
                        "chat": {"type": "private"}, "text": "AAK7Q2M9XAZZ"}},
                    {"update_id": 10, "message": {"from": {"id": 14, "first_name": "Mallory"},
                        "chat": {"type": "group"}, "text": "K7Q2M9XA"}},
                    {"update_id": 11, "message": {"from": {"id": 42, "first_name": "Dana"},
                        "chat": {"type": "private"}, "text": "/start k7q2m9xa"}},
                ]),
                _ => json!([]),
            };
            Json(json!({"ok": true, "result": result}))
        }
        _ => Json(json!({"ok": false, "description": "no such method"})),
    }
}

#[tokio::test]
async fn whoever_sends_the_code_becomes_the_owner() {
    let bot = Bot::default();
    let app = Router::new()
        .route("/{token}/{method}", get(api))
        .with_state(bot.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    // SAFETY: the only test in this binary; nothing else reads the environment.
    unsafe {
        std::env::set_var("LNK_TELEGRAM_API", format!("http://{addr}"));
        std::env::set_var("NO_PROXY", "*");
    }

    use link_telegram as telegram;
    let token = concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw");
    assert_eq!(telegram::bot_name(token).await.unwrap(), "my_ai_bot");
    let err = telegram::bot_name(concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsXX"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Unauthorized"), "{err}");
    assert!(link_connection_contract::token_refused(&err), "{err:#}");

    // Only one program may read a bot's messages.
    assert_eq!(telegram::bot_id(token), Some(123456));
    assert_eq!(telegram::polled_elsewhere(token).await.unwrap(), None);
    assert_eq!(
        telegram::polled_elsewhere(concat!("6543", "21:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDbusy"))
            .await
            .unwrap(),
        Some(telegram::Elsewhere::Polled)
    );
    bot.offsets.lock().unwrap().clear();

    let (id, name) = telegram::owner(token, "K7Q2M9XA", Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!((id, name.as_str()), (42, "Dana"));
    // It skipped the old message, and acknowledged Dana's.
    assert_eq!(*bot.offsets.lock().unwrap(), ["-1", "8", "12"]);
}
