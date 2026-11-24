//! The blocking client: the same calls, waited for, from a program with
//! no runtime of its own.

use std::io::Read;
use std::time::Duration;

use futures_util::StreamExt;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::{get, post};
use link_fetch::Error;
use link_fetch::blocking::Client;
use link_secretive::Secret;

#[derive(Debug, serde::Deserialize)]
struct Account {
    username: String,
}

/// A server on a thread of its own, and its URL.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn serve() -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let app = Router::new()
                .route("/account", get(|| async { r#"{"username":"ada"}"# }))
                .route("/signed-out", get(|| async { StatusCode::UNAUTHORIZED }))
                .route(
                    "/upload",
                    post(|body: axum::body::Bytes| async move { body.len().to_string() }),
                )
                .route(
                    "/upload-then-think",
                    post(|body: axum::body::Bytes| async move {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        body.len().to_string()
                    }),
                )
                .route(
                    "/think",
                    get(|| async {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        "late"
                    }),
                )
                .route(
                    "/stalls",
                    get(|| async {
                        let first = futures_util::stream::once(async {
                            Ok::<_, std::io::Error>(vec![b'x'; 1024])
                        });
                        axum::body::Body::from_stream(first.chain(futures_util::stream::pending()))
                    }),
                )
                .route("/big", get(|| async { vec![b'x'; 3 << 20] }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    format!("http://{}", rx.recv().unwrap())
}

#[test]
fn calls_are_waited_for() {
    let url = serve();
    let client = Client::new().unwrap();
    let token = Secret::from("lnk_s3cret".to_string());
    let account: Account = client
        .get(&format!("{url}/account"))
        .bearer(&token)
        .answer()
        .unwrap();
    assert_eq!(account.username, "ada");
    let err = client.get(&format!("{url}/signed-out")).send().unwrap_err();
    assert!(matches!(err, Error::SignedOut(_)), "{err:?}");
    assert!(matches!(
        client.get(&format!("{url}/big")).bytes().unwrap_err(),
        Error::TooLarge { .. }
    ));
}

#[test]
fn a_reader_is_sent_and_an_answer_read_as_it_comes() {
    let url = serve();
    let client = Client::builder()
        .streaming(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let sent = std::io::Cursor::new(vec![7u8; 1 << 20]);
    let said = client
        .post(&format!("{url}/upload"))
        .reader_body("application/x-tar", sent)
        .text()
        .unwrap();
    assert_eq!(said, (1 << 20).to_string());
    let mut got = Vec::new();
    client
        .get(&format!("{url}/big"))
        .limit(4 << 20)
        .stream()
        .unwrap()
        .read_to_end(&mut got)
        .unwrap();
    assert_eq!(got.len(), 3 << 20);
    let mut capped = Vec::new();
    let err = client
        .get(&format!("{url}/big"))
        .limit(1 << 20)
        .stream()
        .map(|mut s| s.read_to_end(&mut capped))
        .err();
    // Said before it's read, by its length.
    assert!(matches!(err, Some(Error::TooLarge { .. })), "{err:?}");
}

#[test]
fn a_client_outlives_the_thread_that_made_it() {
    let url = serve();
    let client = std::thread::spawn(|| Client::new().unwrap())
        .join()
        .unwrap();
    let account: Account = client.get(&format!("{url}/account")).answer().unwrap();
    assert_eq!(account.username, "ada");
}

/// A reader that gives `pieces` of 1 KiB, `every` apart, and then
/// waits `then` before its end.
struct Slow {
    pieces: usize,
    every: Duration,
    then: Duration,
}

impl Read for Slow {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pieces == 0 {
            std::thread::sleep(std::mem::take(&mut self.then));
            return Ok(0);
        }
        std::thread::sleep(self.every);
        self.pieces -= 1;
        let n = buf.len().min(1024);
        buf[..n].fill(7);
        Ok(n)
    }
}

/// An upload is given up only when none of it goes for the stall's
/// length: one that takes far longer, moving, is sent, and its answer
/// waited for however long the service works on it.
#[test]
fn an_upload_goes_for_as_long_as_it_moves() {
    let url = serve();
    let client = Client::builder()
        .streaming(Duration::from_millis(500))
        .build()
        .unwrap();
    let slow = Slow {
        pieces: 12,
        every: Duration::from_millis(200),
        then: Duration::ZERO,
    };
    let started = std::time::Instant::now();
    let said = client
        .post(&format!("{url}/upload-then-think"))
        .reader_body("application/x-tar", slow)
        .text()
        .unwrap();
    assert_eq!(said, (12 * 1024).to_string());
    assert!(started.elapsed() > Duration::from_secs(4));
}

/// One whose reader stops giving is given up, as is an answer that
/// stops coming, or that doesn't start within the stall's length.
#[test]
fn a_stalled_stream_is_given_up() {
    let url = serve();
    let client = Client::builder()
        .streaming(Duration::from_millis(500))
        .build()
        .unwrap();
    let stuck = Slow {
        pieces: 1,
        every: Duration::ZERO,
        then: Duration::from_secs(5),
    };
    let started = std::time::Instant::now();
    let err = client
        .post(&format!("{url}/upload"))
        .reader_body("application/x-tar", stuck)
        .send()
        .unwrap_err();
    assert!(err.to_string().contains("nothing moved"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(4));
    let mut got = Vec::new();
    let err = client
        .get(&format!("{url}/stalls"))
        .stream()
        .unwrap()
        .read_to_end(&mut got)
        .unwrap_err();
    assert!(err.to_string().contains("nothing moved"), "{err}");
    assert_eq!(got.len(), 1024);
    let err = client.get(&format!("{url}/stalls")).bytes().unwrap_err();
    assert!(err.to_string().contains("nothing moved"), "{err}");
    let err = client.get(&format!("{url}/think")).send().unwrap_err();
    assert!(err.to_string().contains("nothing moved"), "{err}");
}
