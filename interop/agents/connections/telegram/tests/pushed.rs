//! Telegram's messages pushed (`hook`, `unhook`, `sift`, `take`) against
//! a fake Bot API and a fake harness endpoint: the webhook is set with its
//! secret and no token is printed, rolled by setting it again, a push is
//! kept only with the secret (or the one before it, while it rolls) and
//! from an allowed user, and a pushed message is answered in its chat.

link_test_kit::program!(system);

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::any,
};
use link_connection_contract::{
    Bridge,
    pushed::{Hooking, Pushed, Say, Sifting, Taking, Unhooking, Verdict},
};
use link_secretive::Duplicate;
use link_telegram as telegram;
use serde_json::{Value, json};

const TOKEN: &str = concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw");
const SECRET: &str = "s3cret_push-token";
/// The secret before [`SECRET`], while it rolls.
const BEFORE: &str = "0ld_push-token";
/// The owner's chats Telegram won't send to.
const BLOCKED: i64 = 4242;
const FLOODED: i64 = 4343;
/// The owner's chat whose messages' HTML Telegram can't read.
const UNPARSED: i64 = 4444;

/// Each call the fake Bot API took: its method and body.
type Calls = Arc<Mutex<Vec<(String, Value)>>>;

async fn bot_api(
    State(calls): State<Calls>,
    Path((token, method)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Json<Value> {
    // A bot whose webhook is set.
    if token == "bot654321:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDhook" && method == "getUpdates" {
        return Json(json!({"ok": false, "error_code": 409,
            "description": "Conflict: can't use getUpdates method while webhook is active; \
                use deleteWebhook to delete the webhook first"}));
    }
    if token != format!("bot{TOKEN}") {
        return Json(json!({"ok": false, "error_code": 401, "description": "Unauthorized"}));
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or_default();
    calls.lock().unwrap().push((method.clone(), body.clone()));
    match method.as_str() {
        "getMe" => Json(json!({"ok": true, "result": {"id": 123456, "username": "my_ai_bot"}})),
        // A chat whose user blocked the bot, and one told to wait too long.
        "sendMessage" if body["chat_id"] == BLOCKED => Json(json!({"ok": false,
            "error_code": 403, "description": "Forbidden: bot was blocked by the user"})),
        "sendMessage" if body["chat_id"] == FLOODED => Json(json!({"ok": false,
            "error_code": 429, "description": "Too Many Requests: retry after 600",
            "parameters": {"retry_after": 600}})),
        "sendMessage" if body["chat_id"] == UNPARSED && body["parse_mode"].is_string() => {
            Json(json!({"ok": false, "error_code": 400,
                "description": "Bad Request: can't parse entities: unsupported start tag"}))
        }
        "sendMessage" => Json(json!({"ok": true, "result": {"message_id": 77}})),
        _ => Json(json!({"ok": true, "result": true})),
    }
}

fn pushed() -> Pushed {
    telegram::about()
        .pushed
        .expect("Telegram's messages are pushed")
}

fn settings() -> Value {
    json!({"bot_token": TOKEN, "allowed_users": [42]})
}

#[tokio::test]
async fn hooked_taken_and_unhooked() {
    let calls = Calls::default();
    let app = Router::new()
        .route("/{token}/{method}", any(bot_api))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    // SAFETY: the only test in this binary to touch the environment.
    unsafe {
        std::env::set_var("LNK_TELEGRAM_API", format!("http://{addr}"));
        std::env::set_var("NO_PROXY", "*");
    }
    let pushed = pushed();

    // A secret Telegram wouldn't take, an address it wouldn't push to.
    let hook = pushed.hook.unwrap();
    for (url, secret) in [
        ("https://push.example.com/t/1", "not legal!"),
        ("https://push.example.com/t/1", ""),
        ("http://push.example.com/t/1", SECRET),
    ] {
        let hooking = Hooking {
            settings: settings().into(),
            url: url.into(),
            secret: secret.to_string().into(),
        };
        assert!(hook(hooking).await.is_err(), "{url} {secret}");
    }
    assert!(calls.lock().unwrap().is_empty());

    let hooked = hook(Hooking {
        settings: settings().into(),
        url: "https://push.example.com/t/1".into(),
        secret: SECRET.to_string().into(),
    })
    .await
    .unwrap();
    assert_eq!(
        hooked.settings,
        json!({"allowed_users": [42], "bot_id": 123456, "bot_username": "my_ai_bot"})
    );
    assert!(hooked.steps.is_empty());
    assert!(!serde_json::to_string(&hooked).unwrap().contains("AAHdq"));
    let set = calls
        .lock()
        .unwrap()
        .iter()
        .find(|(method, _)| method == "setWebhook")
        .map(|(_, body)| body.clone())
        .expect("setWebhook called");
    assert_eq!(
        set,
        json!({"url": "https://push.example.com/t/1", "secret_token": SECRET,
            "allowed_updates": ["message"], "drop_pending_updates": false})
    );

    // A roll: `hook` again with the new secret sets the webhook with it,
    // while `sift` still takes the one before (below).
    calls.lock().unwrap().clear();
    hook(Hooking {
        settings: settings().into(),
        url: "https://push.example.com/t/1".into(),
        secret: "n3w_push-token".to_string().into(),
    })
    .await
    .unwrap();
    let set = calls
        .lock()
        .unwrap()
        .iter()
        .find(|(method, _)| method == "setWebhook")
        .map(|(_, body)| body.clone())
        .expect("setWebhook called");
    assert_eq!(set["secret_token"], "n3w_push-token");

    // The owner's pushed message, answered in its chat.
    let asked = Arc::new(Mutex::new(Vec::<Value>::new()));
    // The harness, on ACP: each message kept as the request it was, with
    // who it's from.
    let harness = {
        let asked = asked.clone();
        link_connection_contract::acp::fake::serve_seen(
            |_| "hello back".into(),
            Vec::new(),
            move |seen| {
                if let link_connection_contract::acp::fake::Seen::Prompted(prompted) = seen {
                    asked.lock().unwrap().push(json!({
                        "messages": [{"content": prompted.text}],
                        "sender": [prompted.said.from, prompted.said.from_name],
                    }));
                }
            },
        )
        .await
    };
    let endpoint = harness.endpoint.clone();
    calls.lock().unwrap().clear();
    let taking = Taking {
        bridge: Bridge {
            agent: "main".into(),
            endpoint,
            settings: settings().into(),
            ..Bridge::default()
        },
        update: json!({"update_id": 9, "message": {"message_id": 5,
            "from": {"id": 42, "is_bot": false}, "chat": {"id": 42, "type": "private"},
            "text": "hi"}}),
    };
    (pushed.take)(taking.duplicate()).await.unwrap();
    assert_eq!(asked.lock().unwrap()[0]["messages"][0]["content"], "hi");
    let sent: Vec<(String, Value)> = calls.lock().unwrap().clone();
    assert!(
        sent.iter().any(|(method, body)| method == "sendMessage"
            && *body == json!({"chat_id": 42, "text": "hello back", "parse_mode": "HTML"})),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|(method, body)| method == "sendChatAction" && body["action"] == "typing"),
        "{sent:?}"
    );

    // An answer Telegram refuses (the bot blocked): `take` fails as the
    // answer refused, so it exits `REPLY_REFUSED`; one it only said to
    // wait for isn't.
    let in_chat = |chat: i64| {
        let mut taking = taking.duplicate();
        taking.update["message"]["chat"]["id"] = chat.into();
        taking
    };
    let err = (pushed.take)(in_chat(BLOCKED)).await.unwrap_err();
    assert!(link_connection_contract::reply_refused(&err), "{err:#}");
    assert!(!link_connection_contract::token_refused(&err), "{err:#}");
    let err = (pushed.take)(in_chat(FLOODED)).await.unwrap_err();
    assert!(!link_connection_contract::reply_refused(&err), "{err:#}");
    // HTML Telegram can't read: the message sent again once, as plain
    // text.
    calls.lock().unwrap().clear();
    (pushed.take)(in_chat(UNPARSED)).await.unwrap();
    let sent: Vec<Value> = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, _)| method == "sendMessage")
        .map(|(_, body)| body.clone())
        .collect();
    assert_eq!(
        sent,
        [
            json!({"chat_id": UNPARSED, "text": "hello back", "parse_mode": "HTML"}),
            json!({"chat_id": UNPARSED, "text": "hello back"}),
        ]
    );

    // A token Telegram refuses: `take` and `send` fail as refused, so
    // they exit `TOKEN_REFUSED`.
    let mut revoked = taking.duplicate();
    revoked.bridge.settings =
        json!({"bot_token": concat!("1234", "56:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsXX"), "allowed_users": [42]})
            .into();
    let err = (pushed.take)(revoked.duplicate()).await.unwrap_err();
    assert!(link_connection_contract::token_refused(&err), "{err:#}");
    assert!(!link_connection_contract::reply_refused(&err), "{err:#}");
    let send = telegram::about().send.unwrap();
    let err = send(revoked.bridge, "42".into(), "hello".into())
        .await
        .unwrap_err();
    assert!(link_connection_contract::token_refused(&err), "{err:#}");
    // Any other refusal isn't.
    let err = send(taking.bridge.duplicate(), "43".into(), "hello".into())
        .await
        .unwrap_err();
    assert!(!link_connection_contract::token_refused(&err), "{err:#}");

    // A stranger's, pushed anyway, isn't asked.
    let mut member = taking.duplicate();
    member.bridge.shared = vec!["-1001".into()];
    member.update = json!({"update_id": 10, "message": {"message_id": 6, "text": "hello",
        "from": {"id": 43, "is_bot": false, "first_name": "Alice"},
        "chat": {"id": -1001, "type": "group"}, "reply_to_message": {"from": {"id": 123456}}}});
    (pushed.take)(member.duplicate()).await.unwrap();
    assert_eq!(
        asked.lock().unwrap().last().unwrap()["sender"],
        json!(["member:telegram:43", "Alice"])
    );
    let before = asked.lock().unwrap().len();
    member.bridge.shared.clear();
    (pushed.take)(member).await.unwrap();
    assert_eq!(asked.lock().unwrap().len(), before);
    let mut stranger = taking;
    stranger.update["message"]["from"]["id"] = 43.into();
    let before = asked.lock().unwrap().len();
    (pushed.take)(stranger).await.unwrap();
    assert_eq!(asked.lock().unwrap().len(), before);

    calls.lock().unwrap().clear();
    (pushed.unhook.unwrap())(Unhooking {
        settings: settings().into(),
    })
    .await
    .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        [(
            "deleteWebhook".to_string(),
            json!({"drop_pending_updates": false})
        )]
    );

    // A bot whose webhook is set: said so, naming the way back.
    assert_eq!(
        telegram::polled_elsewhere(concat!("6543", "21:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDhook"))
            .await
            .unwrap(),
        Some(telegram::Elsewhere::Hooked)
    );
    assert!(telegram::HOOKED.contains("lnk agent move"));
    assert!(telegram::webhook_set(
        "Telegram refused getUpdates: Conflict: can't use getUpdates method while webhook is \
         active; use deleteWebhook to delete the webhook first"
    ));
    assert!(!telegram::webhook_set(
        "Telegram refused getUpdates: Conflict: terminated by other getUpdates request"
    ));
}

/// What `hook` printed, as `sift` is handed it.
fn sifting(secret: Option<&str>, update: Value) -> Sifting {
    Sifting {
        settings: json!({"allowed_users": [42], "bot_id": 123456, "bot_username": "my_ai_bot"}),
        secret: SECRET.to_string().into(),
        headers: secret
            .map(|s| ("x-telegram-bot-api-secret-token".to_string(), s.to_string()))
            .into_iter()
            .collect(),
        body: update.to_string(),
        ..Default::default()
    }
}

fn direct(from: i64) -> Value {
    json!({"update_id": 9, "message": {"message_id": 5,
        "from": {"id": from, "is_bot": false}, "chat": {"id": from, "type": "private"},
        "text": "hi"}})
}

/// The owner's direct message `update`, taken.
fn taken(update: &str) -> Verdict {
    Verdict::Take {
        thread: "telegram:42".into(),
        chat: "42".into(),
        dedupe: update.into(),
        from: None,
    }
}

/// Every case a push is sifted for, each with what `sift` answers: the
/// webhook's secret, its roll, its cap and the dedupe the keeper refuses
/// a repeat by, through Link's one way in.
#[test]
fn a_table_of_pushes_each_answered() {
    let sift = pushed().sift;
    // Carrying `carried`, sifted with `current` and, while it rolls,
    // `previous`.
    let rolled = |carried: &str, current: &str, previous: &str| {
        let mut sifting = sifting(Some(carried), direct(42));
        sifting.secret = current.to_string().into();
        sifting.previous = previous.to_string().into();
        sifting
    };
    let mut again = direct(42);
    again["update_id"] = 10.into();
    let mut big = sifting(Some(SECRET), direct(42));
    big.body = "a".repeat(link_connection_contract::pushed::UPDATE_MAX + 1);
    let cases = [
        ("a good post", sifting(Some(SECRET), direct(42)), taken("9")),
        // Telegram's retry, or a replay: the same update, the same
        // dedupe, which the keeper refuses a repeat by.
        (
            "replayed, the same dedupe",
            sifting(Some(SECRET), direct(42)),
            taken("9"),
        ),
        (
            "another update, another dedupe",
            sifting(Some(SECRET), again),
            taken("10"),
        ),
        // The secret wrong, missing, or a prefix of it.
        (
            "a wrong secret",
            sifting(Some("wrong"), direct(42)),
            Verdict::drop(401),
        ),
        (
            "no secret carried",
            sifting(None, direct(42)),
            Verdict::drop(401),
        ),
        (
            "a prefix of it",
            sifting(Some("s3cret"), direct(42)),
            Verdict::drop(401),
        ),
        // None set: closed, and the sender tries again.
        ("no secret set", rolled(SECRET, "", ""), Verdict::drop(503)),
        // While it rolls, the one before is taken beside the new one;
        // once rolled, not; and never alone.
        (
            "the previous secret, rolling",
            rolled(BEFORE, SECRET, BEFORE),
            taken("9"),
        ),
        (
            "the new secret, rolling",
            rolled(SECRET, SECRET, BEFORE),
            taken("9"),
        ),
        (
            "the previous secret, rolled",
            rolled(BEFORE, SECRET, ""),
            Verdict::drop(401),
        ),
        (
            "a previous secret alone",
            rolled(BEFORE, "", BEFORE),
            Verdict::drop(503),
        ),
        // Over the cap, before anything is read.
        ("too big", big, Verdict::drop(413)),
    ];
    for (case, sifting, verdict) in cases {
        assert_eq!(sift(&sifting), verdict, "{case}");
    }
}

#[test]
fn sift_keeps_the_owners_messages_with_the_secret() {
    let sift = pushed().sift;
    // A stranger, a bot, something not a message, a body not JSON.
    assert_eq!(sift(&sifting(Some(SECRET), direct(43))), Verdict::drop(200));
    let mut bot = direct(42);
    bot["message"]["from"]["is_bot"] = true.into();
    assert_eq!(sift(&sifting(Some(SECRET), bot)), Verdict::drop(200));
    let edited = json!({"update_id": 10, "edited_message": direct(42)["message"]});
    assert_eq!(sift(&sifting(Some(SECRET), edited)), Verdict::drop(200));
    let mut garbled = sifting(Some(SECRET), json!({}));
    garbled.body = "not json".into();
    assert_eq!(sift(&garbled), Verdict::drop(400));
    // The owner's direct message.
    assert_eq!(sift(&sifting(Some(SECRET), direct(42))), taken("9"));
    // Their mention of the bot in a group; a group message not to it.
    let mention = json!({"update_id": 11, "message": {"message_id": 7,
        "from": {"id": 42, "is_bot": false},
        "chat": {"id": -1001234567890i64, "type": "supergroup"},
        "text": "@my_ai_bot hi", "entities": [{"type": "mention", "offset": 0, "length": 10}]}});
    assert_eq!(
        sift(&sifting(Some(SECRET), mention.clone())),
        Verdict::Take {
            thread: "telegram:-1001234567890".into(),
            chat: "-1001234567890".into(),
            dedupe: "11".into(),
            from: None,
        }
    );
    let mut aside = mention;
    aside["message"]["entities"] = json!([]);
    assert_eq!(sift(&sifting(Some(SECRET), aside)), Verdict::drop(200));
}

#[test]
fn a_say_is_sent_in_the_pushs_response() {
    let sift = pushed().sift;
    let say = |chat: &str, text: &str| Sifting {
        settings: json!({"allowed_users": [42]}),
        say: Some(Say {
            chat: chat.into(),
            text: text.into(),
        }),
        ..Sifting::default()
    };
    let Verdict::Answer {
        status,
        content_type,
        body,
    } = sift(&say("42", "on it"))
    else {
        panic!("not answered");
    };
    assert_eq!((status, content_type.as_str()), (200, "application/json"));
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"method": "sendMessage", "chat_id": 42, "text": "on it", "parse_mode": "HTML"})
    );
    // In Telegram's HTML: the agent's own tags shown, never sent.
    let Verdict::Answer { body, .. } = sift(&say("42", "**done** <b>")) else {
        panic!("not answered");
    };
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["text"], "<b>done</b> &lt;b&gt;");
    // Longer than one message: sent with `send`, split.
    assert!(matches!(
        sift(&say("42", &"a".repeat(5000))),
        Verdict::Take { ref chat, .. } if chat == "42"
    ));
    assert_eq!(sift(&say("me", "hi")), Verdict::drop(400));
}
