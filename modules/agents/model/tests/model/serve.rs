//! `lnk model serve`'s documented contract with other modules.

use link_test_kit::TempDir;

/// A model no runtime here serves: exit code 2 (a usage error).
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_missing_model_exits_with_2() {
    let home = TempDir::new("serve");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lnk-model"))
        .args(["serve", "--model", "no-such-model-lnk-test", "--json"])
        // Never this machine's settings, nor its Ollama app started.
        .env("HOME", &home)
        .env_remove("XDG_CONFIG_HOME")
        .env("LNK_SERVICES", "off")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("no model named"), "{stderr}");
}

/// Its owner gone (the stdin of `--json` closed), the server closes its
/// port at once but answers the request it already took, then ends:
/// never a connection taken and left unanswered.
#[tokio::test(flavor = "multi_thread")]
async fn an_owner_gone_closes_the_port_and_answers_what_it_took() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let home = TempDir::new("serve-owner");
    // The runtime: holds `/v1/models` until it's let go.
    let asked = Arc::new(tokio::sync::Notify::new());
    let go = Arc::new(tokio::sync::Notify::new());
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", upstream.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get({
            let (asked, go) = (asked.clone(), go.clone());
            move || async move {
                asked.notify_one();
                go.notified().await;
                axum::Json(serde_json::json!({"object": "list", "data": [{"id": "held"}]}))
            }
        }),
    );
    tokio::spawn(async move { axum::serve(upstream, app).await.unwrap() });

    let mut command = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-model"), &home);
    command
        .args(["serve", "--upstream", &upstream_url, "--json"])
        .env("LNK_SERVICES", "off")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut server = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
    let owner = server.take_stdin().unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(server.take_stdout().unwrap()),
        &mut line,
    )
    .unwrap();
    let served: serde_json::Value = serde_json::from_str(&line).unwrap();
    let address = served["url"]
        .as_str()
        .unwrap()
        .trim_start_matches("http://")
        .to_string();

    // A request it took, waiting on the runtime as its owner goes.
    let mut taken = tokio::net::TcpStream::connect(&address).await.unwrap();
    taken
        .write_all(b"GET /v1/models HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), asked.notified())
        .await
        .expect("the runtime was never asked");
    drop(owner);
    link_test_kit::eventually(
        "its port refusing connections",
        std::time::Duration::from_secs(10),
        || std::net::TcpStream::connect(&address).is_err(),
    );
    go.notify_one();
    let mut answer = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        taken.read_to_string(&mut answer),
    )
    .await
    .expect("the request it took was never answered")
    .unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.contains("held"), "{answer}");
    link_test_kit::eventually(
        "the server ending",
        std::time::Duration::from_secs(10),
        || server.try_status().unwrap().is_some(),
    );
}
