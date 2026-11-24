//! `lnk model runtime add`: a runtime at an address of its own is listed as the
//! ones at the default ports are (`lnk model runtime list --json`, which `lnk agent
//! start` reads), served by its model's name, and removed again.

use std::process::Stdio;

use axum::{Json, Router, routing::get};
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// A model someone is building: its own port, OpenAI's `/v1/models`.
async fn start_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new().route(
        "/v1/models",
        get(|| async { Json(json!({"object": "list", "data": [{"id": "my-model-lnk-test"}]})) }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://localhost:{port}")
}

fn lnk(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-model"), home)
        .args(args)
        .env("LNK_SERVICES", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn listed(config: &std::path::Path) -> Vec<Value> {
    let output = lnk(config, &["runtime", "list", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_added_address_is_listed_served_and_forgotten() {
    let config = TempDir::new("added");
    let url = start_server().await;
    let config = &config;

    let ours = |runtimes: &[Value]| {
        runtimes
            .iter()
            .find(|r| r["base_url"] == url.as_str())
            .cloned()
    };
    assert_eq!(ours(&listed(config)), None);

    let output = lnk(
        config,
        &["runtime", "add", &format!("{url}/v1"), "--name", "mymodel"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("It serves my-model-lnk-test"), "{stderr}");
    assert_eq!(
        ours(&listed(config)),
        Some(
            json!({"name": "mymodel", "base_url": url, "models": ["my-model-lnk-test"],
            "added": true, "state": "running"})
        )
    );

    // An address nothing answers at is listed too, until it's removed.
    let quiet = link_test_kit::net::HeldNumber::port();
    let quiet_url = format!("http://localhost:{}", quiet.port_number());
    let output = lnk(config, &["runtime", "add", &quiet_url, "--name", "quiet"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let row = listed(config).into_iter().find(|r| r["name"] == "quiet");
    assert_eq!(
        row,
        Some(json!({"name": "quiet", "base_url": quiet_url, "models": [],
            "added": true, "state": "not_answering"}))
    );
    assert!(
        lnk(config, &["runtime", "remove", "quiet"])
            .status
            .success()
    );
    assert!(!listed(config).iter().any(|r| r["name"] == "quiet"));

    // Served by name until the caller closes stdin.
    let mut serve = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-model"), config)
        .args(["serve", "--model", "my-model-lnk-test", "--json"])
        .env("LNK_SERVICES", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(serve.stdout.take().unwrap()),
        &mut line,
    )
    .unwrap();
    let served: Value = serde_json::from_str(&line).unwrap();
    assert!(
        served["description"]
            .as_str()
            .unwrap()
            .contains(&format!("{url}/v1 (mymodel, model my-model-lnk-test)")),
        "{served}"
    );
    drop(serve.stdin.take());
    serve.wait().unwrap();

    let output = lnk(config, &["runtime", "remove", "mymodel"]);
    assert!(output.status.success());
    assert_eq!(ours(&listed(config)), None);
    let output = lnk(config, &["runtime", "remove", &url]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no runtime added as"));

    // An address off this machine is refused.
    let output = lnk(config, &["runtime", "add", "http://example.com:5000"]);
    assert!(!output.status.success());
}
