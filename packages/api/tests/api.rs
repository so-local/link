//! Every route called as the spec says, against a fake account service
//! on this installation answering as the spec says: its method, its path, its
//! credential and its body, and its answer read into the spec's types,
//! or refused for what the spec doesn't name.

link_test_kit::program!(system);

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use link_api::types::{
    ErrorCode, GrantInstallation, InstallationId, PollDevice, Report, StartDevice, TokenRequest,
};
use link_api::{Api, Error, refused};
use link_secretive::Secret;
use serde_json::{Value, json};

/// What the fake service was asked: the method, the path, the bearer
/// token, and the body.
type Asked = Arc<Mutex<Vec<(String, String, Option<String>, Value)>>>;

/// Its answer to each: a status and a body, by method and path.
type Answers = Arc<Mutex<Vec<(&'static str, &'static str, u16, Value)>>>;

/// An access token, as the fake hands them out: three base64url parts.
const TOKEN: &str = "eyJhbGciOiJFZERTQSJ9.eyJzY29wZSI6ImFwaSJ9.c2lnbmVk";
/// An installation's key's public half.
const PUBLIC_KEY: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";
const INSTALLATION: &str = "0aa1b2c3-0000-4000-8000-000000000001";
const ACCOUNT: &str = "6f1c1b7e-1f4b-4c43-9a55-3b0e8c2d4a10";

fn account() -> Value {
    json!({
        "owner": "acct-6f1c1b7e1f4b4c439a553b0e8c2d4a10",
        "username": "ada",
        "account_id": ACCOUNT,
        "installation": {"id": INSTALLATION, "name": "ada's mac"},
    })
}

fn access_token() -> Value {
    json!({"access_token": TOKEN, "expires_at": 1_800_000_000})
}

/// The answers the spec gives each route, a success each.
fn answers() -> Vec<(&'static str, &'static str, u16, Value)> {
    vec![
        (
            "POST",
            "/api/v1/device",
            200,
            json!({
                "device_code": "d".repeat(64),
                "user_code": "BCDF-GHJK",
                "verification_uri": "https://accounts.example.com/device",
                "verification_uri_complete": "https://accounts.example.com/device?code=BCDF-GHJK",
                "expires_in": 600,
                "interval": 2,
            }),
        ),
        ("POST", "/api/v1/device/token", 200, account()),
        ("GET", "/api/v1/account", 200, account()),
        ("POST", "/api/v1/logout", 204, Value::Null),
        (
            "GET",
            "/api/v1/installations",
            200,
            json!({"installations": [
                {"id": INSTALLATION, "name": "ada's mac", "created_at": "2026-10-10T09:15:02.123Z",
                 "last_seen_at": "2026-10-10T09:20:00.000Z", "this": true},
                {"id": "0bb1b2c3-0000-4000-8000-000000000002", "name": "box-1",
                 "created_at": "2026-10-11T10:00:00Z", "last_seen_at": "2026-10-11T10:05:00Z",
                 "this": false, "granted_by": "ada's mac"},
            ]}),
        ),
        ("POST", "/api/v1/installations", 200, account()),
        (
            "DELETE",
            "/api/v1/installations/0bb1b2c3-0000-4000-8000-000000000002",
            204,
            Value::Null,
        ),
        ("POST", "/api/v1/token", 200, access_token()),
        (
            "GET",
            "/api/v1/keys",
            200,
            json!({"keys": [{"kid": "k1", "x": "A".repeat(43)}]}),
        ),
        (
            "POST",
            "/api/v1/agents",
            200,
            json!({"actions": [
                {"id": "7d0c4b1e-5b8f-4e2a-9c3d-2a1b0c9d8e7f", "kind": "start", "agent": "ada"},
            ]}),
        ),
    ]
}

/// A fake account service on this installation, answering `answers`.
async fn service(answers: Vec<(&'static str, &'static str, u16, Value)>) -> (Api, Asked, Answers) {
    let asked = Asked::default();
    let answers = Answers::new(Mutex::new(answers));
    let state = (asked.clone(), answers.clone());
    let app = Router::new().fallback(answer).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (Api::new(&url).unwrap(), asked, answers)
}

async fn answer(
    State((asked, answers)): State<(Asked, Answers)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from);
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    asked
        .lock()
        .unwrap()
        .push((method.to_string(), uri.path().to_string(), bearer, body));
    let answers = answers.lock().unwrap();
    let Some((_, _, status, said)) = answers
        .iter()
        .find(|(m, p, ..)| *m == method.as_str() && *p == uri.path())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let status = StatusCode::from_u16(*status).unwrap();
    match said {
        Value::Null => status.into_response(),
        said => (
            status,
            [("content-type", "application/json")],
            said.to_string(),
        )
            .into_response(),
    }
}

fn token() -> Secret {
    Secret::new(TOKEN.to_string())
}

/// Each route is called by its function, with its method, its path, its
/// credential where it takes one and its body, and its answer read as
/// its type.
#[tokio::test]
async fn every_route_is_called_as_the_spec_says() {
    let (api, asked, _) = service(answers()).await;
    let installation: InstallationId = "0bb1b2c3-0000-4000-8000-000000000002".parse().unwrap();

    let started = api
        .start_device(&StartDevice {
            device: "ada's mac".parse().unwrap(),
            public_key: PUBLIC_KEY.parse().unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(started.user_code.as_str(), "BCDF-GHJK");
    let polled = api
        .poll_device(&PollDevice {
            device_code: started.device_code,
        })
        .await
        .unwrap();
    assert_eq!(polled.installation.id.to_string(), INSTALLATION);
    let who = api.account(&token()).await.unwrap();
    assert_eq!(
        (who.username.as_str(), who.account_id.to_string().as_str()),
        ("ada", ACCOUNT)
    );
    api.logout(&token()).await.unwrap();
    let listed = api
        .list_installations(&token())
        .await
        .unwrap()
        .installations;
    assert_eq!(listed.len(), 2);
    assert!(listed[0].this && listed[0].granted_by.is_none());
    assert_eq!(
        listed[1].granted_by.as_deref().map(|n| n.as_str()),
        Some("ada's mac")
    );
    assert_eq!(listed[0].created_at.to_string(), "2026-10-10T09:15:02.123Z");
    let granted = api
        .grant_installation(
            &token(),
            &GrantInstallation {
                name: "box-2".parse().unwrap(),
                public_key: PUBLIC_KEY.parse().unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(granted.installation.id.to_string(), INSTALLATION);
    api.sign_out_installation(&token(), &installation)
        .await
        .unwrap();
    let traded = api
        .token(&TokenRequest {
            assertion: Secret::new("a.b.c".into()),
        })
        .await
        .unwrap();
    assert_eq!(traded.expires_at, 1_800_000_000);
    assert_eq!(traded.access_token.expose(), TOKEN);
    assert_eq!(api.keys().await.unwrap().keys[0].kid.as_str(), "k1");
    let report: Report = serde_json::from_value(json!({
        "agents": [{"workload": "agent:a1", "name": "ada", "harness": "link", "model": null,
                    "machine": {"resource": format!("installation:{INSTALLATION}"), "name": "ada's mac"}, "running": true,
                    "location": null, "connections": ["telegram"]}],
        "actions": true,
    }))
    .unwrap();
    let actions = api.report_agents(&token(), &report).await.unwrap().actions;
    assert_eq!(actions[0]["kind"], "start");

    let asked = asked.lock().unwrap().clone();
    let called: Vec<(&str, &str, bool)> = asked
        .iter()
        .map(|(m, p, bearer, _)| {
            if let Some(bearer) = bearer {
                assert_eq!(bearer, TOKEN, "{m} {p}");
            }
            (m.as_str(), p.as_str(), bearer.is_some())
        })
        .collect();
    assert_eq!(
        called,
        [
            ("POST", "/api/v1/device", false),
            ("POST", "/api/v1/device/token", false),
            ("GET", "/api/v1/account", true),
            ("POST", "/api/v1/logout", true),
            ("GET", "/api/v1/installations", true),
            ("POST", "/api/v1/installations", true),
            (
                "DELETE",
                "/api/v1/installations/0bb1b2c3-0000-4000-8000-000000000002",
                true
            ),
            ("POST", "/api/v1/token", false),
            ("GET", "/api/v1/keys", false),
            ("POST", "/api/v1/agents", true),
        ]
    );
    // Each body as the spec's request types write it.
    let bodies: Vec<&Value> = asked.iter().map(|(.., body)| body).collect();
    assert_eq!(
        *bodies[0],
        json!({"device": "ada's mac", "public_key": PUBLIC_KEY})
    );
    assert_eq!(*bodies[1], json!({"device_code": "d".repeat(64)}));
    assert_eq!(
        *bodies[5],
        json!({"name": "box-2", "public_key": PUBLIC_KEY})
    );
    assert_eq!(*bodies[7], json!({"assertion": "a.b.c"}));
    assert_eq!(bodies[9]["actions"], true);
    assert_eq!(bodies[9]["agents"][0]["connections"], json!(["telegram"]));
}

/// An answer with a field the spec doesn't name, or a value its pattern
/// doesn't take, is refused, naming the route; never a guess.
#[tokio::test]
async fn an_answer_the_spec_doesnt_name_is_refused() {
    let mut extra = account();
    extra["admin"] = json!(true);
    let mut shouting = account();
    shouting["username"] = json!("ADA");
    let mut not_a_uuid = account();
    not_a_uuid["installation"]["id"] = json!("../logout");
    for (case, said) in [
        ("extra", extra),
        ("pattern", shouting),
        ("uuid", not_a_uuid),
    ] {
        let (api, _, _) = service(vec![("GET", "/api/v1/account", 200, said)]).await;
        let err = api.account(&token()).await.unwrap_err();
        assert!(matches!(&err, Error::Unexpected { .. }), "{case}: {err:?}");
        assert!(err.to_string().contains("GET http://127.0.0.1:"), "{err}");
        assert!(err.to_string().contains("/api/v1/account"), "{err}");
    }
    // Timings the service changes are taken: a login's caller holds them
    // to its own bounds (the accounts module's, an hour and a minute).
    let start = StartDevice {
        device: "mac".parse().unwrap(),
        public_key: PUBLIC_KEY.parse().unwrap(),
    };
    let mut started = answers().remove(0).3;
    started["expires_in"] = json!(900);
    started["interval"] = json!(5);
    let (api, _, _) = service(vec![("POST", "/api/v1/device", 200, started)]).await;
    let started = api.start_device(&start).await.unwrap();
    assert_eq!((started.expires_in, started.interval.get()), (900, 5));
}

/// A refusal is read in the spec's words: a 401 is signed out, a poll's
/// 400 says whether to wait.
#[tokio::test]
async fn a_refusal_says_what_it_is() {
    let (api, _, _) = service(vec![
        (
            "GET",
            "/api/v1/account",
            401,
            json!({"error": "invalid_token", "error_description": "No installation has this token."}),
        ),
        (
            "POST",
            "/api/v1/device/token",
            400,
            json!({"error": "authorization_pending"}),
        ),
        ("POST", "/api/v1/token", 429, json!({"error": "slow_down"})),
        (
            "DELETE",
            "/api/v1/installations/0bb1b2c3-0000-4000-8000-000000000002",
            404,
            json!({"error": "not_found"}),
        ),
    ])
    .await;
    let err = api.account(&token()).await.unwrap_err();
    assert!(matches!(err, Error::SignedOut(_)), "{err:?}");
    assert_eq!(refused(&err), Some(ErrorCode::InvalidToken));
    let poll = PollDevice {
        device_code: Secret::new("d".repeat(64)),
    };
    let err = api.poll_device(&poll).await.unwrap_err();
    assert_eq!(err.status(), Some(400));
    assert_eq!(refused(&err), Some(ErrorCode::AuthorizationPending));
    let asked = TokenRequest {
        assertion: Secret::new("a.b.c".into()),
    };
    let err = api.token(&asked).await.unwrap_err();
    assert!(matches!(err, Error::Later(_)), "{err:?}");
    let installation: InstallationId = "0bb1b2c3-0000-4000-8000-000000000002".parse().unwrap();
    let err = api
        .sign_out_installation(&token(), &installation)
        .await
        .unwrap_err();
    assert_eq!(
        (err.status(), refused(&err)),
        (Some(404), Some(ErrorCode::NotFound))
    );
    // A refusal in words the spec doesn't have says none.
    let (api, _, _) = service(vec![(
        "GET",
        "/api/v1/account",
        403,
        json!({"error": "forbidden"}),
    )])
    .await;
    assert_eq!(refused(&api.account(&token()).await.unwrap_err()), None);
}

/// The report's answer is read action by action: one of a kind this
/// installation doesn't know is kept for it to refuse alone, and a report
/// taking no actions is answered with none.
#[tokio::test]
async fn the_reports_answer_is_read_action_by_action() {
    let (api, _, _) = service(vec![(
        "POST",
        "/api/v1/agents",
        200,
        json!({"actions": [{"id": "a", "kind": "start", "agent": "ada"}, {"id": "b", "kind": "new"}]}),
    )])
    .await;
    let report: Report = serde_json::from_value(json!({"agents": []})).unwrap();
    assert_eq!(
        api.report_agents(&token(), &report)
            .await
            .unwrap()
            .actions
            .len(),
        2
    );
    let (api, _, _) = service(vec![("POST", "/api/v1/agents", 204, Value::Null)]).await;
    assert!(
        api.report_agents(&token(), &report)
            .await
            .unwrap()
            .actions
            .is_empty()
    );
}

/// A credential is a secret in every type that holds one: never printed.
#[test]
fn a_credential_is_never_printed() {
    let made: link_api::types::AccessToken = serde_json::from_value(access_token()).unwrap();
    let shown = format!("{made:?}");
    assert!(!shown.contains(TOKEN), "{shown}");
    // Written whole, as the route hands it over.
    assert_eq!(serde_json::to_value(&made).unwrap()["access_token"], TOKEN);
    let asked = TokenRequest {
        assertion: Secret::new(TOKEN.into()),
    };
    assert!(!format!("{asked:?}").contains(TOKEN));
    let started: link_api::types::DeviceLogin =
        serde_json::from_value(answers().remove(0).3).unwrap();
    assert!(!format!("{started:?}").contains(&"d".repeat(64)));
    // A connect's token is never the API's plain: only sealed, which
    // holds nothing readable.
    let telegram = format!("123456:{}", "A".repeat(35));
    let plain = serde_json::from_value::<link_api::types::Action>(json!({
        "id": "7d0c4b1e-5b8f-4e2a-9c3d-2a1b0c9d8e7f", "kind": "connect-telegram",
        "agent": "ada", "token": telegram,
    }));
    assert!(plain.is_err());
    let to = link_secretive::Recipient::new();
    let about = link_secretive::About {
        owner: "ada".into(),
        kind: "connect-telegram".into(),
        version: 1,
    };
    let sealed =
        link_secretive::Sealed::seal_json(&json!({"token": telegram}), to.seal_to(), about)
            .unwrap();
    let action: link_api::types::Action = serde_json::from_value(json!({
        "id": "7d0c4b1e-5b8f-4e2a-9c3d-2a1b0c9d8e7f", "kind": "connect-telegram",
        "agent": "ada", "sealed": sealed,
    }))
    .unwrap();
    let shown = format!("{action:?}{}", serde_json::to_string(&action).unwrap());
    assert!(!shown.contains(&telegram), "{shown}");
}

/// An action is the kind its `kind` says, closed: one with a field of
/// another kind is none.
#[test]
fn an_action_is_its_kind() {
    let id = "7d0c4b1e-5b8f-4e2a-9c3d-2a1b0c9d8e7f";
    let stop: link_api::types::Action =
        serde_json::from_value(json!({"id": id, "kind": "stop", "agent": "ada"})).unwrap();
    assert!(matches!(stop, link_api::types::Action::Stop(_)));
    for refused in [
        json!({"id": id, "kind": "run", "agent": "ada"}),
        json!({"id": id, "kind": "stop", "agent": "--all"}),
        json!({"id": id, "kind": "move", "agent": "ada"}),
        json!({"id": id, "kind": "stop", "agent": "ada", "to": "aws"}),
    ] {
        assert!(
            serde_json::from_value::<link_api::types::Action>(refused.clone()).is_err(),
            "{refused}"
        );
    }
}

/// Every route in the spec that `lnk` calls has its one function here,
/// and nothing here calls a route the spec doesn't have. A browser's
/// route (signed in with `browserSession`, `/visit`) is the app's alone.
#[test]
fn one_function_a_route() {
    let spec = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/account.yaml"),
    )
    .unwrap();
    let spec: Value = serde_norway::from_str(&spec).unwrap();
    // The spec states the protocol's version, as every call's path does.
    assert_eq!(
        spec["servers"][0]["url"],
        format!("https://accounts.example.com{}", link_api::base_path()).as_str()
    );
    assert_eq!(
        spec["info"]["version"],
        link_api::PROTOCOL.current.to_string().as_str()
    );
    let mut routes: Vec<(String, String, String)> = Vec::new();
    for (path, methods) in spec["paths"].as_object().unwrap() {
        for (method, operation) in methods.as_object().unwrap() {
            if operation["security"][0].get("browserSession").is_some() {
                continue;
            }
            let id = operation["operationId"].as_str().unwrap();
            routes.push((method.to_uppercase(), path.clone(), snake(id)));
        }
    }
    routes.sort();
    let mut ours: Vec<(String, String, String)> = link_api::ROUTES
        .iter()
        .map(|(m, p, f)| (m.to_string(), p.to_string(), f.to_string()))
        .collect();
    ours.sort();
    assert_eq!(ours, routes);
    // And each function is the one its route's line names.
    let source = include_str!("../src/lib.rs");
    for (method, path, function) in link_api::ROUTES {
        assert!(
            source.contains(&format!("pub async fn {function}(")),
            "{function}"
        );
        assert!(
            source.contains(&format!("/// `{method} {path}`")),
            "{method} {path}"
        );
    }
}

/// `ConnectKind` is the one list of connects: each kind in the actions'
/// discriminator that's a connect is one of it, and each of it is one
/// action whose `kind` is it. The generated type reads each as the spec
/// writes it.
#[test]
fn every_connect_is_a_connect_kind() {
    let spec = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/account.yaml"),
    )
    .unwrap();
    let spec: Value = serde_norway::from_str(&spec).unwrap();
    let schemas = &spec["components"]["schemas"];
    let kinds: Vec<&str> = schemas["ConnectKind"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    let mapping = schemas["Action"]["discriminator"]["mapping"]
        .as_object()
        .unwrap();
    let mut connects: Vec<&str> = mapping
        .keys()
        .map(String::as_str)
        .filter(|k| k.starts_with("connect"))
        .collect();
    let mut sorted = kinds.clone();
    sorted.sort();
    connects.sort();
    assert_eq!(connects, sorted);
    for kind in kinds {
        let schema = mapping[kind].as_str().unwrap().rsplit('/').next().unwrap();
        assert_eq!(schemas[schema]["properties"]["kind"]["const"], kind);
        let typed: link_api::types::ConnectKind = kind.parse().unwrap();
        assert_eq!(typed.to_string(), kind);
    }
}

/// `startDevice` -> `start_device`.
fn snake(id: &str) -> String {
    id.chars()
        .flat_map(|c| match c.is_ascii_uppercase() {
            true => vec!['_', c.to_ascii_lowercase()],
            false => vec![c],
        })
        .collect()
}
