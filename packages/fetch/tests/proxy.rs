//! A plain `http://` call to this machine goes through no proxy, whatever
//! the environment says, where an `https://` one goes through it as any
//! program's would. A program of its own: it sets the proxy variables,
//! which every call in the process reads.

link_test_kit::program!(system);

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::get;
use link_fetch::Client;
use link_secretive::Secret;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug, serde::Deserialize)]
struct Account {
    username: String,
}

#[tokio::test]
async fn plain_http_goes_through_no_proxy() {
    // A proxy that keeps the first line of each request it's sent.
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let kept = seen.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = proxy.accept().await {
            let mut line = String::new();
            let _ = BufReader::new(stream).read_line(&mut line).await;
            kept.lock().unwrap().push(line.trim().to_string());
        }
    });
    // SAFETY: this program's only test, set before any thread reads them.
    unsafe {
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
            "HTTPS_PROXY",
            "https_proxy",
        ] {
            std::env::set_var(name, &proxy_url);
        }
        for name in ["NO_PROXY", "no_proxy"] {
            std::env::remove_var(name);
        }
    }

    let app = Router::new().route("/account", get(|| async { r#"{"username":"ada"}"# }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = Client::new().unwrap();
    let token = Secret::from("lnk_s3cret".to_string());
    for host in ["127.0.0.1", "localhost", "dev.localhost"] {
        let account: Account = client
            .get(&format!("http://{host}:{port}/account"))
            .bearer(&token)
            .answer()
            .await
            .unwrap_or_else(|e| panic!("{host}: {e}"));
        assert_eq!(account.username, "ada");
    }
    // IPv6's loopback too, whether or not anything answers there.
    let _ = client
        .get(&format!("http://[::1]:{port}/account"))
        .bearer(&token)
        .answer::<Account>()
        .await;
    assert!(
        seen.lock().unwrap().is_empty(),
        "{:?}",
        seen.lock().unwrap()
    );

    // https:// goes through it, as it would for any program here.
    let _ = client
        .get("https://link-fetch-proxy-test.invalid/account")
        .answer::<Account>()
        .await
        .unwrap_err();
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.as_slice(),
        ["CONNECT link-fetch-proxy-test.invalid:443 HTTP/1.1"],
        "{seen:?}"
    );
}
