//! The wire: calls a minute, a web page's requests uncounted, each call
//! logged, refusals pausing it, watched calls ending, metadata services.

use super::*;

#[tokio::test]
async fn a_wire_over_its_calls_a_minute_answers_429() {
    let target = echo().await;
    let dir = std::env::temp_dir().join(format!("lnk-proxy-limit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = Filter {
        allowed: |_| true,
        ..Filter::new(Upstream::Direct)
    }
    .with_tcp(vec![target.to_string()])
    .on(crate::wire::Wire::new("run-1", dir.clone()).limited(Some(2)));
    let connect = format!("CONNECT {target} HTTP/1.1\r\n\r\n");
    for _ in 0..2 {
        let answer = through(rules, &connect).await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    }
    let answer = through(rules, &connect).await;
    assert!(answer.starts_with("HTTP/1.1 429"), "{answer}");
    assert!(answer.contains("Retry-After: "), "{answer}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A web page (on macOS any page the user visits, which can send
/// requests to this machine's loopback) is refused before its calls
/// count: it never pauses the wire, uses up its calls a minute, or
/// fills its log.
#[tokio::test]
async fn a_web_pages_requests_never_count_against_the_wire() {
    let target = echo().await;
    let dir = std::env::temp_dir().join(format!("lnk-proxy-pages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wire = crate::wire::Wire::new("run-9", dir.clone()).limited(Some(3));
    let rules = Filter {
        allowed: |_| true,
        ..Filter::new(Upstream::Direct)
    }
    .with_tcp(vec![target.to_string()])
    .on(wire);
    let pages = [
        "GET /x HTTP/1.1\r\nHost: 127.0.0.1:40781\r\nOrigin: https://evil.example\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:40781\r\nSec-Fetch-Site: cross-site\r\n\r\n",
        "GET /img.png HTTP/1.1\r\nHost: rebound.example:40781\r\n\r\n",
        "POST /_link/model/v1/messages HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: null\r\n\r\n",
    ];
    for i in 0..(crate::wire::BURST * 2) {
        let page = pages[i % pages.len()];
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        client.write_all(page.as_bytes()).await.unwrap();
        assert!(proxy(server, rules).await.is_err());
        let mut answer = Vec::new();
        client.read_to_end(&mut answer).await.unwrap();
        let answer = String::from_utf8_lossy(&answer);
        assert!(answer.starts_with("HTTP/1.1 403"), "{page}: {answer}");
    }
    let wire = rules.wire.unwrap();
    assert_eq!(wire.why_stopped(), None, "a web page paused the wire");
    // Its own calls still go, all three of the minute's.
    let connect = format!("CONNECT {target} HTTP/1.1\r\n\r\n");
    for _ in 0..3 {
        let answer = through(rules, &connect).await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    }
    // Logged once a minute, with a count, not once each.
    let pages: Vec<_> = crate::wire::read(&dir)
        .into_iter()
        .filter(|e| e.verb == "browser")
        .collect();
    assert_eq!(pages.len(), 1, "{pages:?}");
    assert!(
        !pages[0].to.contains("evil"),
        "no header in the log: {pages:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn each_call_is_logged_and_a_stopped_wire_ends_them_all() {
    let target = echo().await;
    let dir = std::env::temp_dir().join(format!("lnk-wire-proxy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rules = Filter {
        allowed: |_| true,
        tcp: Vec::leak(vec![target.to_string(), "127.0.0.1:9".into()]),
        ..Filter::new(Upstream::Direct)
    }
    .on(crate::wire::Wire::new("main-otter", dir.clone()));
    // A call in progress: its bytes echoed.
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let served = tokio::spawn(proxy(server, rules));
    client
        .write_all(format!("CONNECT {target} HTTP/1.1\r\n\r\nhello").as_bytes())
        .await
        .unwrap();
    let mut got = [0u8; 128];
    let mut read = 0;
    while !String::from_utf8_lossy(&got[..read]).ends_with("hello") {
        read += client.read(&mut got[read..]).await.unwrap();
    }
    // Stopped: it ends within a second or so, and new calls are refused.
    crate::wire::stop(&dir, true).unwrap();
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), served)
        .await
        .expect("the call in progress didn't end");
    assert!(format!("{:#}", ended.unwrap().unwrap_err()).contains("stopped"));
    let answer = through(rules, &format!("CONNECT {target} HTTP/1.1\r\n\r\n")).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    crate::wire::stop(&dir, false).unwrap();
    let answer = through(rules, "GET http://127.0.0.1:9/secret?q HTTP/1.1\r\n\r\n").await;
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");

    let log = crate::wire::read(&dir);
    let seen: Vec<_> = log
        .iter()
        .map(|e| (e.verb.as_str(), e.to.as_str(), e.outcome))
        .collect();
    let to = target.to_string();
    use crate::wire::Outcome::*;
    assert_eq!(
        seen,
        [
            ("tcp", to.as_str(), Allowed),
            ("tcp", to.as_str(), Ended),
            ("tcp", to.as_str(), Refused),
            ("http", "127.0.0.1:9", Failed),
        ]
    );
    // What went each way, and never a path.
    assert!(log[1].up >= 5 && log[1].down >= 5, "{:?}", log[1]);
    assert!(!log.iter().any(|e| e.to.contains("secret")));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_burst_of_refusals_pauses_the_wire_until_resumed() {
    let target = echo().await;
    let dir = std::env::temp_dir().join(format!("lnk-wire-burst-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = Filter {
        allowed: |_| true,
        tcp: Vec::leak(vec![target.to_string()]),
        ..Filter::new(Upstream::Direct)
    }
    .on(crate::wire::Wire::new("run-9", dir.clone()));
    let open = format!("CONNECT {target} HTTP/1.1\r\n\r\n");
    assert!(through(rules, &open).await.starts_with("HTTP/1.1 200"));
    // Ports tried in turn, each refused: the wire pauses itself.
    for port in 1..=crate::wire::BURST {
        let probe = format!("CONNECT example.com:{port} HTTP/1.1\r\n\r\n");
        assert!(through(rules, &probe).await.starts_with("HTTP/1.1 403"));
    }
    assert!(
        through(rules, &open).await.starts_with("HTTP/1.1 403"),
        "paused"
    );
    assert_eq!(crate::wire::paused(&dir).len(), 1);
    assert!(crate::wire::resume(&dir, "run-9").unwrap());
    assert!(
        through(rules, &open).await.starts_with("HTTP/1.1 200"),
        "resumed"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Bytes crossing a watched stream keep its call alive; once nothing
/// crosses for the idle time, the watch ends it, as for a pooled
/// Telegram call waiting on an answer that never comes.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_watched_call_ends_when_idle() {
    let (mut client, mut near) = tokio::io::duplex(1024);
    let idle = std::time::Duration::from_millis(300);
    let activity = Activity::new();
    let mut watched = activity.watch(&mut near);
    let started = std::time::Instant::now();
    let call = async {
        let mut got = [0u8; 4];
        loop {
            watched.read_exact(&mut got).await.unwrap();
        }
    };
    let talking = async {
        for _ in 0..4 {
            client.write_all(b"ping").await.unwrap();
            tokio::time::sleep(idle / 2).await;
        }
        std::future::pending::<()>().await
    };
    tokio::select! {
        () = call => unreachable!(),
        () = talking => unreachable!(),
        () = activity.idle(idle) => {}
    }
    // Kept alive while it talked (four pings, half the idle time
    // apart), then ended.
    assert!(started.elapsed() >= idle * 2, "{:?}", started.elapsed());
}

/// A cloud's metadata service, however it's written, pauses the wire
/// at once: it goes by the address the name resolves to.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_metadata_service_written_otherwise_pauses_the_wire() {
    for (n, to) in [
        "2852039166:443",
        "0xa9fea9fe:443",
        "[::ffff:169.254.169.254]:443",
    ]
    .into_iter()
    .enumerate()
    {
        let dir = std::env::temp_dir().join(format!("lnk-proxy-meta-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rules = Filter::new(Upstream::Direct).on(crate::wire::Wire::new("run-8", dir.clone()));
        let answer = through(rules, &format!("CONNECT {to} HTTP/1.1\r\n\r\n")).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{to}: {answer}");
        // The call is logged, and the wire paused, as it ends.
        for _ in 0..50 {
            if !crate::wire::paused(&dir).is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(crate::wire::paused(&dir).len(), 1, "{to} didn't pause");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
