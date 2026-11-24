//! `link-fetch` against a server on this machine: each rule seen on the
//! wire, and each way a call goes wrong.

use std::time::Duration;

use axum::Router;
use axum::extract::RawQuery;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use link_fetch::{Client, Error, MAX_ANSWER};
use link_secretive::Secret;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize)]
struct Account {
    username: String,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Serialize)]
struct Grant {
    name: String,
}

/// A server on this machine answering each route as its name says, and
/// its URL.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn serve() -> String {
    let app = Router::new()
        .route("/account", get(|| async { r#"{"username":"ada"}"# }))
        .route(
            "/unknown",
            get(|| async { r#"{"username":"ada","admin":true}"# }),
        )
        .route(
            "/nested-unknown",
            get(|| async { r#"[{"username":"ada","admin":true}]"# }),
        )
        .route("/wrong-type", get(|| async { r#"{"username":42}"# }))
        .route("/not-json", get(|| async { "<html>hello</html>" }))
        .route(
            "/huge",
            get(|| async { format!(r#"{{"username":"{}"}}"#, "a".repeat(MAX_ANSWER as usize)) }),
        )
        .route(
            "/streamed-huge",
            get(|| async {
                let chunks = (0..32).map(|_| Ok::<_, std::io::Error>(vec![b' '; 64 << 10]));
                axum::body::Body::from_stream(futures_util::stream::iter(chunks))
            }),
        )
        .route(
            "/redirect",
            get(|| async {
                (
                    StatusCode::FOUND,
                    [(header::LOCATION, "https://elsewhere.example/")],
                )
            }),
        )
        .route("/empty", post(|| async { StatusCode::NO_CONTENT }))
        .route(
            "/redirect-plain",
            get(|| async {
                (
                    StatusCode::FOUND,
                    [(header::LOCATION, "http://127.0.0.1:1/account")],
                )
            }),
        )
        .route("/signed-out", get(|| async { StatusCode::UNAUTHORIZED }))
        .route(
            "/later",
            get(|| async { (StatusCode::TOO_MANY_REQUESTS, [(header::RETRY_AFTER, "7")]) }),
        )
        .route(
            "/broken",
            get(|| async { (StatusCode::BAD_GATEWAY, "upstream \u{1b}[31mdown") }),
        )
        .route(
            "/unavailable",
            get(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(header::RETRY_AFTER, "3")],
                )
            }),
        )
        .route(
            "/refused",
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"authorization_pending"}"#,
                )
            }),
        )
        .route(
            "/echo",
            post(
                |headers: HeaderMap, RawQuery(query): RawQuery, body: String| async move {
                    let said = |name: header::HeaderName| {
                        headers
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_string()
                    };
                    serde_json::json!({
                        "authorization": said(header::AUTHORIZATION),
                        "user_agent": said(header::USER_AGENT),
                        "accept": said(header::ACCEPT),
                        "content_type": said(header::CONTENT_TYPE),
                        "api_key": said(header::HeaderName::from_static("x-api-key")),
                        "owner": said(header::HeaderName::from_static("x-link-owner")),
                        "query": query.unwrap_or_default(),
                        "body": body,
                    })
                    .to_string()
                    .into_response()
                },
            ),
        )
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                r#"{"username":"ada"}"#
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

#[tokio::test]
async fn an_answer_is_its_type() {
    let url = serve().await;
    let account: Account = Client::new()
        .unwrap()
        .get(&format!("{url}/account"))
        .answer()
        .await
        .unwrap();
    assert_eq!(
        account,
        Account {
            username: "ada".into(),
            email: None
        }
    );
}

#[tokio::test]
async fn an_unknown_field_is_refused() {
    let url = serve().await;
    let client = Client::new().unwrap();
    let err = client
        .get(&format!("{url}/unknown"))
        .answer::<Account>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unexpected { .. }), "{err:?}");
    let said = err.to_string();
    assert!(said.contains(&format!("GET {url}/unknown")), "{said}");
    assert!(said.contains("admin"), "{said}");
    // Anywhere in the answer, not only at its top.
    let err = client
        .get(&format!("{url}/nested-unknown"))
        .answer::<Vec<Account>>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unexpected { .. }), "{err:?}");
}

#[tokio::test]
async fn a_wrong_type_is_refused() {
    let url = serve().await;
    let client = Client::new().unwrap();
    for route in ["wrong-type", "not-json"] {
        let err = client
            .get(&format!("{url}/{route}"))
            .answer::<Account>()
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Unexpected { .. }), "{err:?}");
        assert!(err.to_string().contains(&format!("{url}/{route}")), "{err}");
        // What the answer held isn't repeated.
        assert!(!err.to_string().contains("html"), "{err}");
    }
}

#[tokio::test]
async fn an_oversized_answer_is_refused() {
    let url = serve().await;
    let client = Client::new().unwrap();
    for route in ["huge", "streamed-huge"] {
        let err = client
            .get(&format!("{url}/{route}"))
            .answer::<Account>()
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::TooLarge { limit, .. } if limit == MAX_ANSWER),
            "{route}: {err:?}"
        );
    }
    // A call that says it may be longer reads it.
    let account: Account = client
        .get(&format!("{url}/huge"))
        .limit(2 * MAX_ANSWER)
        .answer()
        .await
        .unwrap();
    assert_eq!(account.username.len() as u64, MAX_ANSWER);
}

#[tokio::test]
async fn a_redirect_is_never_followed() {
    let url = serve().await;
    let err = Client::new()
        .unwrap()
        .get(&format!("{url}/redirect"))
        .answer::<Account>()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Redirected { status: 302, .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn statuses_map_once() {
    let url = serve().await;
    let client = Client::new().unwrap();
    let get = |route: &str| client.get(&format!("{url}/{route}")).answer::<Account>();

    let err = get("signed-out").await.unwrap_err();
    assert!(matches!(err, Error::SignedOut(_)), "{err:?}");
    assert_eq!(err.status(), Some(401));

    let err = get("later").await.unwrap_err();
    assert!(
        matches!(&err, Error::Later(refusal) if refusal.retry_after() == Some(Duration::from_secs(7))),
        "{err:?}"
    );
    assert_eq!(err.status(), Some(429));
    assert!(err.to_string().ends_with("try again in 7s"), "{err}");

    let err = get("broken").await.unwrap_err();
    assert!(matches!(err, Error::Failed(_)), "{err:?}");
    assert_eq!(err.status(), Some(502));
    // What it said is shown, its escapes made harmless.
    assert!(
        err.to_string().ends_with("upstream \\u{1b}[31mdown"),
        "{err}"
    );

    let err = get("refused").await.unwrap_err();
    #[derive(Deserialize)]
    struct Refused {
        error: String,
    }
    let refused: Refused = err.refusal().unwrap().read().unwrap();
    assert_eq!(refused.error, "authorization_pending");
    assert_eq!(err.status(), Some(400));
}

#[tokio::test]
async fn no_content_is_null() {
    let url = serve().await;
    let client = Client::new().unwrap();
    client
        .post(&format!("{url}/empty"))
        .answer::<()>()
        .await
        .unwrap();
    let none: Option<Account> = client.post(&format!("{url}/empty")).answer().await.unwrap();
    assert_eq!(none, None);
    // An answer that should hold something and holds nothing is refused.
    let err = client
        .post(&format!("{url}/empty"))
        .answer::<Account>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unexpected { .. }), "{err:?}");
}

#[tokio::test]
async fn what_is_sent() {
    let url = serve().await;
    let client = Client::new().unwrap();
    let token = Secret::from("lnk_s3cret".to_string());
    let echo: Echo = client
        .post(&format!("{url}/echo"))
        .bearer(&token)
        .json(&Grant { name: "box".into() })
        .answer()
        .await
        .unwrap();
    assert_eq!(echo.authorization, "Bearer lnk_s3cret");
    assert_eq!(echo.user_agent, link_fetch::USER_AGENT);
    assert!(echo.user_agent.starts_with("lnk/"));
    assert_eq!(echo.accept, "application/json");
    assert_eq!(echo.content_type, "application/json");
    assert_eq!(echo.body, r#"{"name":"box"}"#);

    let password = Secret::from("ada:hunter2".to_string());
    let echo: Echo = client
        .post(&format!("{url}/echo"))
        .basic(&password)
        .answer()
        .await
        .unwrap();
    assert_eq!(echo.authorization, "Basic YWRhOmh1bnRlcjI=");
}

#[tokio::test]
async fn plain_http_goes_only_to_this_machine() {
    let token = Secret::from("lnk_s3cret".to_string());
    let err = Client::new()
        .unwrap()
        .get("http://accounts.example.com/api/v1/account")
        .bearer(&token)
        .answer::<Account>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Insecure { .. }), "{err:?}");
    assert!(!format!("{err} {err:?}").contains("s3cret"));
}

#[tokio::test]
async fn a_slow_answer_times_out() {
    let url = serve().await;
    let err = Client::with_timeout(Duration::from_millis(300))
        .unwrap()
        .get(&format!("{url}/slow"))
        .answer::<Account>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unreachable { .. }), "{err:?}");
    assert!(err.to_string().contains("no answer in time"), "{err}");
}

#[tokio::test]
async fn an_error_prints_no_credential_or_query() {
    let url = serve().await;
    let token = Secret::from("lnk_s3cret".to_string());
    let client = Client::new().unwrap();
    for route in ["signed-out", "broken", "refused", "wrong-type", "redirect"] {
        let err = client
            .get(&format!("{url}/{route}?code=q5ecret"))
            .bearer(&token)
            .answer::<Account>()
            .await
            .unwrap_err();
        let said = format!("{err} {err:?}");
        assert!(!said.contains("s3cret"), "{said}");
        assert!(!said.contains("q5ecret"), "{said}");
    }
}

#[tokio::test]
async fn nothing_listening_is_unreachable() {
    let token = Secret::from("lnk_s3cret".to_string());
    let err = Client::new()
        .unwrap()
        .get("http://127.0.0.1:1/account?code=q5ecret")
        .bearer(&token)
        .answer::<Account>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unreachable { .. }), "{err:?}");
    let said = format!("{err} {err:?}");
    assert!(
        said.starts_with("reaching GET http://127.0.0.1:1/account: "),
        "{said}"
    );
    assert!(
        !said.contains("s3cret") && !said.contains("q5ecret"),
        "{said}"
    );
}

/// `localhost` and every name under it are this machine's loopback, found
/// without asking DNS: here, the server on 127.0.0.1.
#[tokio::test]
async fn names_under_localhost_are_this_machine() {
    let url = serve().await;
    let port = url.rsplit(':').next().unwrap();
    let client = Client::new().unwrap();
    for host in ["localhost", "localhost.", "dev.localhost", "a.b.localhost."] {
        let account: Account = client
            .get(&format!("http://{host}:{port}/account"))
            .answer()
            .await
            .unwrap_or_else(|e| panic!("{host}: {e}"));
        assert_eq!(account.username, "ada", "{host}");
    }
}

#[derive(Deserialize)]
struct Echo {
    authorization: String,
    user_agent: String,
    accept: String,
    content_type: String,
    api_key: String,
    owner: String,
    query: String,
    body: String,
}

/// A service that isn't Link's adds fields without a new version: read
/// open, a field its type doesn't name is passed over, a wrong type still
/// refused.
#[tokio::test]
async fn an_outside_answer_is_read_open() {
    let url = serve().await;
    let outside = Client::builder().outside().build().unwrap();
    let account: Account = outside
        .get(&format!("{url}/unknown"))
        .answer()
        .await
        .unwrap();
    assert_eq!(account.username, "ada");
    let err = outside
        .get(&format!("{url}/wrong-type"))
        .answer::<Account>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unexpected { .. }), "{err:?}");
}

#[tokio::test]
async fn headers_a_query_and_a_form_are_sent() {
    let url = serve().await;
    let key = Secret::from("sk-s3cret".to_string());
    #[derive(Serialize)]
    struct Refresh<'a> {
        grant_type: &'a str,
        refresh_token: &'a str,
    }
    let echo: Echo = Client::new()
        .unwrap()
        .post(&format!("{url}/echo"))
        .secret_header("x-api-key", &key)
        .header("x-link-owner", "acct-1")
        .query(&[("after", "a b"), ("n", "2")])
        .form(&Refresh {
            grant_type: "refresh_token",
            refresh_token: "r&1",
        })
        .answer()
        .await
        .unwrap();
    assert_eq!(echo.api_key, "sk-s3cret");
    assert_eq!(echo.owner, "acct-1");
    assert_eq!(echo.query, "after=a+b&n=2");
    assert_eq!(echo.content_type, "application/x-www-form-urlencoded");
    assert_eq!(echo.body, "grant_type=refresh_token&refresh_token=r%261");
    assert_eq!(echo.accept, "application/json");
    assert_eq!(echo.authorization, "");
}

#[tokio::test]
async fn raw_answers_are_capped_too() {
    let url = serve().await;
    let client = Client::new().unwrap();
    let bytes = client.get(&format!("{url}/account")).bytes().await.unwrap();
    assert_eq!(bytes, br#"{"username":"ada"}"#);
    let text = client.get(&format!("{url}/not-json")).text().await.unwrap();
    assert_eq!(text, "<html>hello</html>");
    client.post(&format!("{url}/empty")).send().await.unwrap();
    let err = client
        .get(&format!("{url}/signed-out"))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::SignedOut(_)), "{err:?}");
    let err = client
        .get(&format!("{url}/huge"))
        .bytes()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::TooLarge { .. }), "{err:?}");
    let err = client
        .post(&format!("{url}/echo"))
        .bytes_body("application/octet-stream", vec![1, 2, 3])
        .header("bad\nname", "x")
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unsendable { .. }), "{err:?}");
}

#[tokio::test]
async fn a_stream_stops_at_its_limit() {
    let url = serve().await;
    let client = Client::new().unwrap();
    let mut streamed = client
        .get(&format!("{url}/streamed-huge"))
        .stream()
        .await
        .unwrap();
    let mut read = 0u64;
    let err = loop {
        match streamed.chunk().await {
            Ok(Some(chunk)) => read += chunk.len() as u64,
            Ok(None) => panic!("read it all: {read}"),
            Err(err) => break err,
        }
    };
    assert!(matches!(err, Error::TooLarge { .. }), "{err:?}");
    assert!(read <= MAX_ANSWER, "{read}");
    let mut streamed = client
        .get(&format!("{url}/streamed-huge"))
        .limit(4 << 20)
        .stream()
        .await
        .unwrap();
    let mut read = 0;
    while let Some(chunk) = streamed.chunk().await.unwrap() {
        read += chunk.len();
    }
    assert_eq!(read, 32 * (64 << 10));
}

/// A client that follows redirects follows them to https:// alone, and
/// carries no credential, which one would take elsewhere.
#[tokio::test]
async fn redirects_followed_only_as_said() {
    let url = serve().await;
    let follows = Client::builder().follow_redirects(3).build().unwrap();
    let err = follows
        .get(&format!("{url}/redirect-plain"))
        .bytes()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unreachable { .. }), "{err:?}");
    assert!(err.to_string().contains("other than https://"), "{err}");
    let token = Secret::from("lnk_s3cret".to_string());
    let err = follows
        .get(&format!("{url}/account"))
        .bearer(&token)
        .bytes()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unsendable { .. }), "{err:?}");
}

/// An address someone else gave reaches no address of this machine or
/// its network.
#[tokio::test]
async fn public_only_reaches_no_private_address() {
    let url = serve().await;
    let public = Client::builder().public_only().build().unwrap();
    for to in [
        format!("{url}/account"),
        url.replace("http://", "https://") + "/account",
        "https://[::1]/a".to_string(),
        "https://169.254.169.254/latest/meta-data".to_string(),
        "https://10.0.0.1/a".to_string(),
    ] {
        let err = public.get(&to).bytes().await.unwrap_err();
        assert!(matches!(err, Error::Insecure { .. }), "{to}: {err:?}");
    }
    let err = public.get("https://localhost/a").bytes().await.unwrap_err();
    assert!(
        err.to_string().contains("isn't at a public address"),
        "{err}"
    );
}

#[test]
fn trusting_takes_certificates() {
    let err = Client::builder()
        .trusting(b"not a certificate")
        .build()
        .unwrap_err();
    assert!(matches!(err, Error::Setup(_)), "{err:?}");
}

#[tokio::test]
async fn plain_http_beyond_this_machine_only_when_said() {
    let token = Secret::from("lnk_s3cret".to_string());
    let err = Client::new()
        .unwrap()
        .get("http://store.invalid:4318/v1/metrics")
        .bearer(&token)
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Insecure { .. }), "{err:?}");
    // Said so, it's sent: here, to a name nobody has.
    let err = Client::builder()
        .plain_http()
        .build()
        .unwrap()
        .get("http://store.invalid:4318/v1/metrics")
        .bearer(&token)
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unreachable { .. }), "{err:?}");
}

/// A URL that holds a secret is named by what it's said to be, never by
/// itself, whatever goes wrong.
#[tokio::test]
async fn a_secret_url_is_never_shown() {
    let url = serve().await;
    let client = Client::new().unwrap();
    for route in ["signed-out", "broken", "wrong-type", "nowhere"] {
        let secret = Secret::from(format!("{url}/{route}?bot=s3cret"));
        let err = client
            .secret_url(link_fetch::Method::GET, &secret, "the bot's getMe")
            .query(&[("limit", "1")])
            .answer::<Account>()
            .await
            .unwrap_err();
        let said = format!("{err} {err:?}");
        assert!(said.contains("GET the bot's getMe"), "{said}");
        assert!(!said.contains("s3cret") && !said.contains(&url), "{said}");
    }
    let secret = Secret::from(format!("{url}/account"));
    let account: Account = client
        .secret_url(link_fetch::Method::GET, &secret, "an account")
        .answer()
        .await
        .unwrap();
    assert_eq!(account.username, "ada");
}

/// `https://` alone means this machine too.
#[tokio::test]
async fn https_only_refuses_plain_http_here_too() {
    let url = serve().await;
    let err = Client::builder()
        .https_only()
        .build()
        .unwrap()
        .get(&format!("{url}/account"))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Insecure { .. }), "{err:?}");
    let said = err.to_string();
    assert!(said.ends_with("only to https://"), "{said}");
    assert!(!said.contains("this machine"), "{said}");
}

/// A caller that reads some of an answer's fields says so; a wrong type
/// is still refused.
#[tokio::test]
async fn a_part_of_an_answer_is_read_when_said() {
    let url = serve().await;
    let client = Client::new().unwrap();
    let account: Account = client
        .get(&format!("{url}/unknown"))
        .answer_part()
        .await
        .unwrap();
    assert_eq!(account.username, "ada");
    let err = client
        .get(&format!("{url}/wrong-type"))
        .answer_part::<Account>()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unexpected { .. }), "{err:?}");
}

/// Any refusal that says how long to wait says it, a 503's too.
#[tokio::test]
async fn a_wait_asked_is_kept() {
    let url = serve().await;
    let err = Client::new()
        .unwrap()
        .get(&format!("{url}/unavailable"))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Failed(_)), "{err:?}");
    assert_eq!(
        err.refusal().unwrap().retry_after(),
        Some(Duration::from_secs(3))
    );
}

/// A call no connection was made for wasn't sent: another can be tried
/// elsewhere.
#[tokio::test]
async fn unreached_says_whether_it_was_sent() {
    let err = Client::new()
        .unwrap()
        .get("http://127.0.0.1:1/account")
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Unreachable { sent: false, .. }),
        "{err:?}"
    );
}

/// A credential that can't be a header's is said before anything is sent,
/// never printed.
#[tokio::test]
async fn a_credential_that_isnt_a_header_is_refused() {
    let url = serve().await;
    let token = Secret::from("lnk_s3cret\nx".to_string());
    let err = Client::new()
        .unwrap()
        .get(&format!("{url}/account"))
        .bearer(&token)
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unsendable { .. }), "{err:?}");
    assert!(!format!("{err} {err:?}").contains("s3cret"));
}

/// The metadata server's client sends plain `http://` there alone, and
/// nothing of a secret.
#[tokio::test]
async fn the_metadata_server_alone_and_no_credential() {
    let url = serve().await;
    let token = Secret::from("lnk_s3cret".to_string());
    let metadata = Client::builder().metadata_server().build().unwrap();
    for to in [
        "http://169.254.169.254/computeMetadata/v1/",
        "http://metadata.google.internal/computeMetadata/v1/",
    ] {
        let err = metadata.get(to).bearer(&token).send().await.unwrap_err();
        assert!(matches!(err, Error::Unsendable { .. }), "{to}: {err:?}");
        let err = metadata
            .get(to)
            .secret_header("x-goog-api-key", &token)
            .send()
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Unsendable { .. }), "{to}: {err:?}");
        let secret = Secret::from(format!("{to}?token=s3cret"));
        let err = metadata
            .secret_url(link_fetch::Method::GET, &secret, "a token")
            .send()
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Unsendable { .. }), "{to}: {err:?}");
    }
    for to in [
        "http://store.invalid/v1/metrics",
        "http://169.254.169.253/",
        "http://metadata.google.internal.example/",
    ] {
        let err = metadata.get(to).send().await.unwrap_err();
        assert!(matches!(err, Error::Insecure { .. }), "{to}: {err:?}");
        assert!(err.to_string().contains("metadata server"), "{err}");
    }
    // This machine, as any client, its credential too.
    let account: Account = metadata
        .get(&format!("{url}/account"))
        .bearer(&token)
        .answer()
        .await
        .unwrap();
    assert_eq!(account.username, "ada");
}

/// A stream's Debug names its route, never its URL.
#[tokio::test]
async fn a_streamed_answer_shows_no_url() {
    let url = serve().await;
    let secret = Secret::from(format!("{url}/account?bot=s3cret"));
    let streamed = Client::new()
        .unwrap()
        .secret_url(link_fetch::Method::GET, &secret, "the bot's file")
        .stream()
        .await
        .unwrap();
    let shown = format!("{streamed:?}");
    assert!(shown.contains("the bot's file"), "{shown}");
    assert!(
        !shown.contains("s3cret") && !shown.contains(&url),
        "{shown}"
    );
}

#[test]
fn is_public_keeps_out_what_carries_another_address() {
    for ip in [
        "8.8.8.8",
        "1.1.1.1",
        "2606:4700:4700::1111",
        "64:ff9b::808:808",
    ] {
        assert!(link_fetch::is_public(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "192.88.99.1",
        "100.64.0.1",
        "::1",
        "fe80::1",
        "fd00::1",
        "::ffff:127.0.0.1",
        "64:ff9b::a00:1",
        "64:ff9b:1::808:808",
        "2001:0:4136:e378:8000:63bf:3fff:fdd2",
        "2002:7f00:1::",
        "2001:db8::1",
    ] {
        assert!(!link_fetch::is_public(ip.parse().unwrap()), "{ip}");
    }
}
