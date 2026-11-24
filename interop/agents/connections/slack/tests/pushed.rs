//! Slack's messages pushed by the Events API, against a fake Web API and
//! a fake harness endpoint: `hook` switches the app to a request URL with
//! its configuration token (or gives the steps without one), `unhook`
//! switches it back, and `take` answers a pushed message through the
//! endpoint and posts the answer.

link_test_kit::program!(system);

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use link_connection_contract::{
    Bridge,
    pushed::{Hooking, Taking, Unhooking},
};
use link_secretive::Duplicate;
use link_slack as slack;
use serde_json::{Value, json};

const BOT: &str = concat!("xoxb", "-1234567890-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx");
const CONFIG: &str = concat!("xoxe.xoxp", "-1-Mi0yLTEyMzQ1Njc4OTAtMTIzNDU2Nzg5MA");
const URL: &str = "https://push.example/h/abc";

#[derive(Clone, Default)]
struct Fake {
    /// The manifests apps.manifest.update was given.
    updated: Arc<Mutex<Vec<Value>>>,
    /// What chat.postMessage was asked to post: channel and text.
    posted: Arc<Mutex<Vec<(String, String)>>>,
    /// The threads and texts the endpoint was asked.
    asked: Arc<Mutex<Vec<(String, String)>>>,
    speakers: Arc<Mutex<Vec<(String, String)>>>,
}

async fn api(
    State(fake): State<Fake>,
    Path(method): Path<String>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    let ok = |mut value: Value| {
        value["ok"] = json!(true);
        Json(value)
    };
    let field = |name: &str| form.get(name).cloned().unwrap_or_default();
    match (method.as_str(), token) {
        ("auth.test", BOT) => ok(json!({
            "user_id": "U0BOT", "user": "link-main", "team": "Acme", "team_id": "T0ACME",
            "bot_id": "B01"
        })),
        ("bots.info", BOT) if field("bot") == "B01" => ok(json!({"bot": {"app_id": "A0APP"}})),
        ("apps.manifest.export", CONFIG) if field("app_id") == "A0APP" => {
            ok(json!({"manifest": slack::manifest("main")}))
        }
        ("apps.manifest.update", CONFIG) if field("app_id") == "A0APP" => {
            let manifest = serde_json::from_str(&field("manifest")).unwrap();
            fake.updated.lock().unwrap().push(manifest);
            ok(json!({"app_id": "A0APP"}))
        }
        // A conversation gone, and one Slack says to wait on.
        ("chat.postMessage", BOT) if field("channel") == "D0GONE" => {
            Json(json!({"ok": false, "error": "channel_not_found"}))
        }
        ("chat.postMessage", BOT) if field("channel") == "D0BUSY" => {
            Json(json!({"ok": false, "error": "ratelimited"}))
        }
        ("chat.postMessage", BOT) => {
            let mut posted = fake.posted.lock().unwrap();
            posted.push((field("channel"), field("text")));
            ok(json!({"ts": format!("2.{}", posted.len())}))
        }
        ("users.info", BOT) => ok(json!({"user": {"profile": {"display_name": "Alice"}}})),
        ("reactions.add" | "reactions.remove", BOT) => ok(json!({})),
        _ => Json(json!({"ok": false, "error": "invalid_auth"})),
    }
}

/// The fake Slack and endpoint, on their own thread: the Web API's
/// address is the process's.
fn fake() -> (&'static Fake, String) {
    static FAKE: OnceLock<(Fake, String)> = OnceLock::new();
    let (fake, endpoint_url) = FAKE.get_or_init(|| {
        let fake = Fake::default();
        let app = Router::new()
            .route("/api/{method}", post(api))
            .with_state(fake.clone());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let (told, endpoint) = std::sync::mpsc::channel();
        let seen = fake.clone();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    // The harness, on ACP: each message kept as its thread
                    // and text, and who it's from.
                    let harness = link_connection_contract::acp::fake::serve_seen(
                        |_| "hello <you>".into(),
                        Vec::new(),
                        move |asked| {
                            if let link_connection_contract::acp::fake::Seen::Prompted(asked) =
                                asked
                            {
                                seen.asked
                                    .lock()
                                    .unwrap()
                                    .push((asked.thread.clone(), asked.text.clone()));
                                let said = &asked.said;
                                seen.speakers.lock().unwrap().push((
                                    said.from.clone().unwrap_or_default(),
                                    said.from_name.clone().unwrap_or_default(),
                                ));
                            }
                        },
                    )
                    .await;
                    told.send(harness.endpoint).unwrap();
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    axum::serve(listener, app).await.unwrap();
                });
        });
        // SAFETY: set once, before any test here reads the environment.
        unsafe {
            std::env::set_var("LNK_SLACK_API", format!("http://{addr}/api"));
            std::env::set_var("NO_PROXY", "*");
        }
        (fake, endpoint.recv().unwrap())
    });
    (fake, endpoint_url.clone())
}

fn settings(config: Option<&str>) -> Value {
    let mut settings = json!({
        "bot_token": BOT,
        "app_token": concat!("xapp", "-1-A0123456789-1234567890123-abcdef0123456789"),
        "signing_secret": "8f742231b10e8888abcd99eeef0085a5",
        "allowed_users": ["U0DANA"],
        "dm_channel": "D0DANA",
    });
    if let Some(config) = config {
        settings["config_token"] = json!(config);
    }
    settings
}

/// The last manifest apps.manifest.update was given, after `before` of
/// them.
fn updated_after(fake: &Fake, before: usize) -> Option<Value> {
    let updated = fake.updated.lock().unwrap();
    (updated.len() > before).then(|| updated.last().unwrap().clone())
}

/// One test, the calls one after another: the crate's one client keeps
/// its connections on the runtime that made them.
#[tokio::test]
async fn pushed_messages() {
    hook_switches_the_app_to_the_url_and_unhook_back().await;
    without_a_signing_secret_hook_switches_nothing().await;
    without_a_configuration_token_hook_gives_the_steps().await;
    take_answers_the_owners_message_and_posts_it().await;
    shared_members_keep_their_identity_at_the_endpoint().await;
}

async fn hook_switches_the_app_to_the_url_and_unhook_back() {
    let (fake, _) = fake();
    let before = fake.updated.lock().unwrap().len();
    let hooked = slack::pushed::hook(Hooking {
        settings: settings(Some(CONFIG)).into(),
        url: URL.into(),
        secret: link_secretive::Secret::default(),
    })
    .await
    .unwrap();
    // What sift is handed: no token, no secret.
    assert_eq!(
        hooked.settings,
        json!({"allowed_users": ["U0DANA"], "bot_user": "U0BOT", "team": "T0ACME"})
    );
    assert!(hooked.steps.is_empty(), "{:?}", hooked.steps);
    let manifest = updated_after(fake, before).expect("the manifest updated");
    assert_eq!(manifest["settings"]["socket_mode_enabled"], false);
    assert_eq!(
        manifest["settings"]["event_subscriptions"]["request_url"],
        URL
    );
    let events = manifest["settings"]["event_subscriptions"]["bot_events"].to_string();
    assert!(
        events.contains("message.im") && events.contains("app_mention"),
        "{events}"
    );

    let before = fake.updated.lock().unwrap().len();
    let steps = slack::pushed::unhook(Unhooking {
        settings: settings(Some(CONFIG)).into(),
    })
    .await
    .unwrap();
    assert!(steps.is_empty(), "{steps:?}");
    let manifest = updated_after(fake, before).expect("the manifest updated");
    assert_eq!(manifest["settings"]["socket_mode_enabled"], true);
    assert!(manifest["settings"]["event_subscriptions"]["request_url"].is_null());
}

/// `lnk agent connect slack` keeps no signing secret: without one in
/// what it's given, `hook` says where it is and switches nothing, since
/// every push would be refused.
async fn without_a_signing_secret_hook_switches_nothing() {
    let (fake, _) = fake();
    let before = fake.updated.lock().unwrap().len();
    let mut given = settings(Some(CONFIG));
    given.as_object_mut().unwrap().remove("signing_secret");
    let err = slack::pushed::hook(Hooking {
        settings: given.into(),
        url: URL.into(),
        secret: link_secretive::Secret::default(),
    })
    .await
    .unwrap_err();
    assert!(format!("{err:#}").contains("Signing Secret"), "{err:#}");
    assert!(updated_after(fake, before).is_none());
}

async fn without_a_configuration_token_hook_gives_the_steps() {
    let (fake, _) = fake();
    let before = fake.updated.lock().unwrap().len();
    // None, and one Slack refuses (expired).
    for config in [None, Some(concat!("xoxe.xoxp", "-1-ExpiredExpiredExpired"))] {
        let hooked = slack::pushed::hook(Hooking {
            settings: settings(config).into(),
            url: URL.into(),
            secret: link_secretive::Secret::default(),
        })
        .await
        .unwrap();
        assert_eq!(hooked.settings["bot_user"], "U0BOT");
        let steps = hooked.steps.join("\n");
        assert!(steps.contains("Socket Mode off"), "{steps}");
        assert!(steps.contains(&format!("Request URL to {URL}")), "{steps}");
        let steps = slack::pushed::unhook(Unhooking {
            settings: settings(config).into(),
        })
        .await
        .unwrap()
        .join("\n");
        assert!(steps.contains("Socket Mode on"), "{steps}");
    }
    assert!(updated_after(fake, before).is_none());
    // Slack pushes only to HTTPS.
    let err = slack::pushed::hook(Hooking {
        settings: settings(None).into(),
        url: "http://push.example/h/abc".into(),
        secret: link_secretive::Secret::default(),
    })
    .await
    .unwrap_err();
    assert!(err.to_string().contains("https://"), "{err}");
}

async fn take_answers_the_owners_message_and_posts_it() {
    let (fake, endpoint) = fake();
    let bridge = Bridge {
        agent: "main".into(),
        endpoint,
        settings: settings(None).into(),
        ..Bridge::default()
    };
    let envelope = json!({
        "type": "event_callback",
        "team_id": "T0ACME",
        "event_id": "Ev0TAKE",
        "event": {"type": "message", "user": "U0DANA", "channel": "D0DANA",
            "channel_type": "im", "ts": "1.2", "text": "hi &lt;there&gt;"},
    });
    slack::pushed::take(Taking {
        bridge: bridge.duplicate(),
        update: envelope.clone(),
    })
    .await
    .unwrap();
    assert!(
        fake.asked
            .lock()
            .unwrap()
            .contains(&("slack:D0DANA".into(), "hi <there>".into()))
    );
    assert!(
        fake.posted
            .lock()
            .unwrap()
            .contains(&("D0DANA".into(), "hello &lt;you&gt;".into()))
    );

    // An answer Slack refuses (its conversation gone): `take` fails as
    // the answer refused, so it exits `REPLY_REFUSED`; one it only said
    // to wait for isn't.
    for (connection, refused) in [("D0GONE", true), ("D0BUSY", false)] {
        let mut gone = envelope.clone();
        gone["event"]["channel"] = json!(connection);
        let err = slack::pushed::take(Taking {
            bridge: bridge.duplicate(),
            update: gone,
        })
        .await
        .unwrap_err();
        assert_eq!(
            link_connection_contract::reply_refused(&err),
            refused,
            "{connection}: {err:#}"
        );
        assert!(!link_connection_contract::token_refused(&err), "{err:#}");
    }

    // A stranger's is refused before the endpoint is asked.
    let (revoked, owners) = (bridge.duplicate(), envelope.clone());
    let asked = fake.asked.lock().unwrap().len();
    let mut stranger = envelope;
    stranger["event"]["user"] = json!("U0EVE");
    let err = slack::pushed::take(Taking {
        bridge,
        update: stranger,
    })
    .await
    .unwrap_err();
    assert!(err.to_string().contains("not answering"), "{err}");
    assert!(!link_connection_contract::token_refused(&err), "{err:#}");
    assert_eq!(fake.asked.lock().unwrap().len(), asked);

    // A token Slack refuses: `take` and `send` fail as refused, so they
    // exit `TOKEN_REFUSED`.
    let mut token = settings(None);
    token["bot_token"] = json!("xoxb-revoked");
    let revoked = Bridge {
        settings: token.into(),
        ..revoked
    };
    let err = slack::pushed::take(Taking {
        bridge: revoked.duplicate(),
        update: owners,
    })
    .await
    .unwrap_err();
    assert!(link_connection_contract::token_refused(&err), "{err:#}");
    assert!(!link_connection_contract::reply_refused(&err), "{err:#}");
    let err = slack::bridge::send(revoked, "D0DANA".into(), "hello".into())
        .await
        .unwrap_err();
    assert!(link_connection_contract::token_refused(&err), "{err:#}");
}

async fn shared_members_keep_their_identity_at_the_endpoint() {
    let (fake, endpoint) = fake();
    let mut bridge = Bridge {
        agent: "shared".into(),
        endpoint,
        settings: settings(None).into(),
        shared: vec!["C0SHARED".into()],
        ..Default::default()
    };
    let update = json!({"event": {"type": "app_mention", "user": "U0ALICE", "user_team": "T0ACME",
        "channel": "C0SHARED", "ts": "3.1", "text": "<@U0BOT> hello"}});
    slack::pushed::take(Taking {
        bridge: bridge.duplicate(),
        update: update.clone(),
    })
    .await
    .unwrap();
    assert!(
        fake.speakers
            .lock()
            .unwrap()
            .contains(&("member:slack:T0ACME/U0ALICE".into(), "Alice".into()))
    );
    let before = fake.asked.lock().unwrap().len();
    bridge.shared.clear();
    assert!(
        slack::pushed::take(Taking { bridge, update })
            .await
            .is_err()
    );
    assert_eq!(fake.asked.lock().unwrap().len(), before);
}
