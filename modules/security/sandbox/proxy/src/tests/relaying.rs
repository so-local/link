//! Connections relayed: `CONNECT` through an upstream, TLS checked, idle
//! tunnels ended, plain requests to the host checked, routes to bridges.

use super::*;

#[tokio::test]
async fn connect_goes_through_the_upstream_and_local_is_refused() {
    let target = echo().await;
    let (exit, used) = socks().await;
    // Its port isn't HTTPS's: named, or refused.
    let unnamed = Filter {
        upstream: Box::leak(Box::new(Upstream::Socks5(exit))),
        allowed: |_| true,
        hosts: None,
        tcp: &[],
        wire: None,
        routes: &crate::routes::NO_ROUTES,
        calling: None,
        url_secret: None,
        proxy_secret: None,
        lan: &[],
        internet: true,
        lan_only: false,
    };
    let answer = through(unnamed, &format!("CONNECT {target} HTTP/1.1\r\n\r\n")).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    let permissive = Filter {
        upstream: Box::leak(Box::new(Upstream::Socks5(exit))),
        allowed: |_| true,
        hosts: None,
        tcp: Vec::leak(vec![target.to_string()]),
        wire: None,
        routes: &crate::routes::NO_ROUTES,
        calling: None,
        url_secret: None,
        proxy_secret: None,
        lan: &[],
        internet: true,
        lan_only: false,
    };
    // CONNECT, then bytes that come back from the echo server.
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    tokio::spawn(proxy(server, permissive));
    client
        .write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\nhello").as_bytes())
        .await
        .unwrap();
    let mut got = [0u8; 64];
    let mut read = 0;
    while !String::from_utf8_lossy(&got[..read]).ends_with("hello") {
        read += client.read(&mut got[read..]).await.unwrap();
    }
    let text = String::from_utf8_lossy(&got[..read]);
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
    assert_eq!(used.load(std::sync::atomic::Ordering::SeqCst), 1);

    // The real rules: this machine is refused, and the exit never asked.
    let real = Filter::new(Upstream::Socks5(exit));
    let answer = through(real, &format!("CONNECT {target} HTTP/1.1\r\n\r\n")).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    let answer = through(
        real,
        "GET http://localhost:9/ HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(used.load(std::sync::atomic::Ordering::SeqCst), 1);
    let answer = through(real, "GET https://example.com/ HTTP/1.1\r\n\r\n").await;
    assert!(answer.starts_with("HTTP/1.1 400"), "{answer}");
}

/// A SOCKS upstream on a Unix socket, as `ssh -R` offers one: the
/// tunnel goes through it.
#[tokio::test]
async fn connect_goes_through_an_upstream_on_a_socket() {
    let target = echo().await;
    let (exit, used) = socks().await;
    let dir = std::env::temp_dir().join(format!("lnk-proxy-unix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("upstream.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut tcp = TcpStream::connect(exit).await.unwrap();
                let _ = tokio::io::copy_bidirectional(&mut s, &mut tcp).await;
            });
        }
    });
    let rules = Filter {
        allowed: |_| true,
        tcp: Vec::leak(vec![target.to_string()]),
        ..Filter::new(Upstream::Socks5Unix(socket))
    };
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    tokio::spawn(proxy(server, rules));
    client
        .write_all(format!("CONNECT {target} HTTP/1.1\r\n\r\nhello").as_bytes())
        .await
        .unwrap();
    let mut got = [0u8; 64];
    let mut read = 0;
    while !String::from_utf8_lossy(&got[..read]).ends_with("hello") {
        read += client.read(&mut got[read..]).await.unwrap();
    }
    assert!(String::from_utf8_lossy(&got[..read]).starts_with("HTTP/1.1 200"));
    assert_eq!(used.load(std::sync::atomic::Ordering::SeqCst), 1);
    // No socket: the exit isn't there, said as such.
    let gone = Filter {
        allowed: |_| true,
        tcp: Vec::leak(vec![target.to_string()]),
        ..Filter::new(Upstream::Socks5Unix(dir.join("none.sock")))
    };
    let answer = through(gone, &format!("CONNECT {target} HTTP/1.1\r\n\r\n")).await;
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_tunnels_tls_asks_for_the_host_checked() {
    let hello = client_hello("api.example.com");
    // Whole with the CONNECT, or arriving after it.
    let sent = hello_for(&mut &[][..], hello.clone(), "api.example.com").await;
    assert_eq!(sent.unwrap(), hello);
    let (first, later) = hello.split_at(7);
    let sent = hello_for(&mut &later[..], first.to_vec(), "API.example.com.").await;
    assert_eq!(sent.unwrap(), hello);
    // Another of a CDN's sites, a hello cut short, or no TLS at all.
    let err = hello_for(&mut &[][..], hello.clone(), "listed.example.com").await;
    assert!(format!("{:#}", err.unwrap_err()).contains("refused:"));
    let err = hello_for(&mut &[][..], first.to_vec(), "api.example.com").await;
    assert!(err.is_err());
    let plain = b"GET / HTTP/1.1\r\nHost: other.example\r\n\r\n".to_vec();
    let err = hello_for(&mut &[][..], plain, "api.example.com").await;
    assert!(format!("{:#}", err.unwrap_err()).contains("isn't TLS"));
    // A listed address: its TLS names no host, and goes through; a name
    // with no host's is still refused.
    let unnamed = client_hello("203.0.113.5");
    let sent = hello_for(&mut &[][..], unnamed.clone(), "203.0.113.5").await;
    assert_eq!(sent.unwrap(), unnamed);
    assert!(
        hello_for(&mut &[][..], unnamed.clone(), "[2001:db8::5]")
            .await
            .is_ok()
    );
    let err = hello_for(&mut &[][..], unnamed, "api.example.com").await;
    assert!(format!("{:#}", err.unwrap_err()).contains("names no host"));
}

/// A tunnel carries both ways, and ends once nothing has crossed for
/// its idle time.
#[tokio::test]
async fn a_tunnel_ends_when_idle() {
    let (mut client, near) = tokio::io::duplex(1024);
    let (far, mut server) = tokio::io::duplex(1024);
    let idle = std::time::Duration::from_millis(300);
    let joined = tokio::spawn(joined_until_idle(near, far, idle));
    client.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    server.read_exact(&mut got).await.unwrap();
    // Before the last crossing: its idle time runs from that crossing.
    let started = std::time::Instant::now();
    server.write_all(b"pong").await.unwrap();
    client.read_exact(&mut got).await.unwrap();
    assert_eq!(&got, b"pong");
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), joined)
        .await
        .expect("an idle tunnel stayed open")
        .unwrap();
    // Its activity is counted in whole milliseconds.
    let waited = started.elapsed() + std::time::Duration::from_millis(1);
    assert!(
        ended.is_err() && waited >= idle,
        "{ended:?} after {waited:?}"
    );
}

#[tokio::test]
async fn a_missing_exit_fails_closed() {
    // Nothing listens on this port: no way out, and no way around.
    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let rules = Filter {
        upstream: Box::leak(Box::new(Upstream::Socks5(closed))),
        allowed: |_| true,
        hosts: None,
        tcp: &[],
        wire: None,
        routes: &crate::routes::NO_ROUTES,
        calling: None,
        url_secret: None,
        proxy_secret: None,
        lan: &[],
        internet: true,
        lan_only: false,
    };
    let target = echo().await;
    let rules = rules.with_tcp(vec![target.to_string()]);
    let answer = through(rules, &format!("CONNECT {target} HTTP/1.1\r\n\r\n")).await;
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
}

/// A plain request's `Host` is the host checked, never the client's,
/// and the connection carries one request: a second one on it, to
/// another host by its `Host`, never reaches the origin.
#[tokio::test]
async fn a_plain_request_goes_to_the_host_checked_and_alone() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = listener.local_addr().unwrap();
    let (seen, got) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut all = Vec::new();
        let _ =
            tokio::time::timeout(std::time::Duration::from_secs(2), s.read_to_end(&mut all)).await;
        let _ = seen.send(String::from_utf8_lossy(&all).to_string());
    });
    let rules = Filter {
        allowed: |_| true,
        tcp: Vec::leak(vec![origin.to_string()]),
        ..Filter::new(Upstream::Direct)
    };
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    tokio::spawn(proxy(server, rules));
    let request = format!(
        "POST http://{origin}/a HTTP/1.1\r\nHost: attacker.example\r\n\
         Content-Length: 5\r\nConnection: keep-alive\r\n\r\nhello\
         GET / HTTP/1.1\r\nHost: attacker.example\r\n\r\n"
    );
    client.write_all(request.as_bytes()).await.unwrap();
    let seen = got.await.unwrap();
    assert!(seen.starts_with("POST /a HTTP/1.1\r\n"), "{seen}");
    assert!(seen.contains(&format!("\r\nHost: {origin}\r\n")), "{seen}");
    assert!(seen.contains("\r\nConnection: close\r\n"), "{seen}");
    assert!(seen.ends_with("\r\n\r\nhello"), "{seen}");
    assert!(!seen.contains("attacker"), "{seen}");
    // A body in chunks can't be framed here: refused.
    let chunked =
        format!("POST http://{origin}/a HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n");
    assert!(through(rules, &chunked).await.starts_with("HTTP/1.1 400"));
}

/// A plain request to an origin that never answers ends once nothing
/// crosses for the idle time, and holds no connection after.
#[tokio::test]
async fn a_plain_request_to_a_silent_origin_ends_when_idle() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (s, _) = listener.accept().await.unwrap();
        std::future::pending::<()>().await;
        drop(s);
    });
    let rules = Filter {
        allowed: |_| true,
        tcp: Vec::leak(vec![origin.to_string()]),
        ..Filter::new(Upstream::Direct)
    };
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let idle = std::time::Duration::from_millis(300);
    let proxied = tokio::spawn(TEST_IDLE.scope(idle, proxy(server, rules)));
    client
        .write_all(format!("GET http://{origin}/ HTTP/1.1\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), proxied)
        .await
        .expect("a silent origin held the request open")
        .unwrap();
    assert!(format!("{:#}", ended.unwrap_err()).contains("nothing crossed"));
}

#[tokio::test]
async fn plain_requests_reach_the_origin_in_origin_form() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let (head, _) = read_head(&mut s).await.unwrap();
        let head = String::from_utf8_lossy(&head).to_string();
        let body = if head.starts_with("GET /path?q=1 HTTP/1.1\r\n") && !head.contains("Proxy-") {
            "ok"
        } else {
            "bad"
        };
        s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{body}").as_bytes())
            .await
            .unwrap();
    });
    let rules = Filter {
        allowed: |_| true,
        tcp: Vec::leak(vec![origin.to_string()]),
        ..Filter::new(Upstream::Direct)
    };
    let answer = through(
        rules,
        &format!("GET http://{origin}/path?q=1 HTTP/1.1\r\nHost: {origin}\r\nProxy-Connection: keep-alive\r\n\r\n"),
    )
    .await;
    assert!(answer.ends_with("ok"), "{answer}");
}

/// A route the proxy names nothing of: matched by its prefix, behind the
/// URL secret if it needs it, logged by its name and its bridge's
/// verdict, and handed to its bridge with the secret taken out of its
/// head, who calls proved. Nothing at `/_link/` but its routes.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_route_is_matched_checked_and_handed_to_its_bridge() {
    use link_sandbox_binding::Binding;
    use link_sandbox_binding::Identity;
    use link_sandbox_binding::Keyring;
    use link_sandbox_route::Bridge;
    use link_sandbox_route::Route;
    use link_sandbox_route::Verdict;

    let dir = link_test_kit::TempDir::new("proxy-route");
    let caller = Binding {
        identity: Identity {
            agent: "agent-1".into(),
            service: String::new(),
            owner: "me@here".into(),
            teams: Vec::new(),
        },
        key: "k".repeat(32).into(),
    };
    Keyring::at(&dir.join("keys"))
        .bind(&caller.identity, &caller.key)
        .unwrap();
    // A bridge answering each call with its request's target, and the
    // route and sandbox it was told.
    let started = link_sandbox_route::bridge::Started {
        socket: dir.join("bridges").join("echo.sock"),
        keys: dir.join("keys"),
        settings: dir.to_path_buf(),
        runtime: dir.join("runtime"),
    };
    let limits = link_sandbox_binding::Limits {
        name: "the echo bridge",
        calls: 8,
        per_agent: 8,
        per_owner: 8,
        action_max: 1 << 20,
        action_wait: std::time::Duration::from_secs(5),
        answer_wait: std::time::Duration::from_secs(5),
    };
    let serving = started.clone();
    tokio::spawn(async move {
        let _ = link_sandbox_route::bridge::serve(
            &serving,
            &limits,
            std::time::Duration::from_secs(60),
            |who, call, mut stream| async move {
                let link_sandbox_route::To::Request { head } = &call.to else {
                    anyhow::bail!("not a request");
                };
                let target = head.split(' ').nth(1).unwrap_or_default().to_string();
                let refused = target.contains("no");
                let to = format!("echoed-for-{}", who.agent);
                let verdict = match refused {
                    true => Verdict::refused(to),
                    false => Verdict::allowed(to),
                };
                stream.verdict(&verdict).await?;
                let body = format!("{} {} {target}", call.route, call.sandbox);
                let status = if refused { "403 Forbidden" } else { "200 OK" };
                let answer = format!(
                    "HTTP/1.1 {status}\r\nListed: {}\r\nContent-Length: {}\r\n\r\n{body}",
                    call.listed,
                    body.len()
                );
                stream.write_all(answer.as_bytes()).await?;
                stream.shutdown().await?;
                Ok(())
            },
        )
        .await;
    });
    let bridge = Bridge {
        socket: started.socket.clone(),
        start: Vec::new(),
    };
    while tokio::net::UnixStream::connect(&bridge.socket)
        .await
        .is_err()
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    const SECRET: &str = "0123456789abcdef0123456789abcdef";
    let route = |name: &str, prefix: &str, needs_url_secret| Route {
        name: name.into(),
        prefix: prefix.into(),
        needs_url_secret,
        bridge: bridge.clone(),
        given: "{}".to_string().into(),
        ..Route::default()
    };
    let routes = crate::routes::Routes {
        routes: vec![
            route("open-echo", "/_link/echo", false),
            route("kept-echo", "/_link/kept/", true),
        ],
    };
    let rules = Filter::new(Upstream::Direct)
        .with_routes(routes, Some(SECRET.to_string().into()))
        .calling(crate::bridged::Calling {
            caller: caller.duplicate(),
            sandbox: "run-1".into(),
            upstream: "direct".into(),
            inbound: None,
        });
    let get = |target: &str| format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");

    let got = through(rules, &get("/_link/echo?q")).await;
    assert!(
        got.starts_with("HTTP/1.1 200") && got.ends_with("open-echo run-1 /_link/echo?q"),
        "{got}"
    );
    // Every public host: not listed. No internet at all (only its routes,
    // or `[lan]`): listed, never taken for every host.
    assert!(got.contains("\r\nListed: false\r\n"), "{got}");
    let closed = rules.with_lan(Vec::new(), false);
    let got = through(closed, &get("/_link/echo?q")).await;
    assert!(got.contains("\r\nListed: true\r\n"), "{got}");
    let got = through(rules, &get(&format!("/_link/url/{SECRET}/kept/x"))).await;
    assert!(got.ends_with("kept-echo run-1 /_link/kept/x"), "{got}");
    // Refused by its bridge: its answer, as it gives it.
    let got = through(rules, &get("/_link/echo/no")).await;
    assert!(got.starts_with("HTTP/1.1 403"), "{got}");
    for refused in [
        "/_link/kept/x".to_string(),
        "/_link/url/fedcba9876543210fedcba9876543210/kept/x".into(),
        format!("/_link/url/{SECRET}/echo"),
        "/_link/echoes".into(),
        "/_link/nothing".into(),
    ] {
        let got = through(rules, &get(&refused)).await;
        assert!(got.starts_with("HTTP/1.1 403"), "{refused}: {got}");
    }
    assert_eq!(
        crate::routes::logged("GET", &format!("/_link/url/{SECRET}/kept/x"), rules.routes),
        ("kept-echo", String::new())
    );
    // Only a host and port, never a path or a user in an address.
    for (method, target, to) in [
        ("CONNECT", "example.com:443/the/path?token=abc", ""),
        ("CONNECT", "example.com:443", "example.com:443"),
        ("CONNECT", "[fd00::1]:443", "[fd00::1]:443"),
        ("GET", "http://[::1]:8080/x", "[::1]:8080"),
        ("GET", "http://user:hunter2@example.com/", ""),
        ("GET", "http://example.com/a?b", "example.com:80"),
    ] {
        let (_, logged) = crate::routes::logged(method, target, rules.routes);
        assert_eq!(logged, to, "{target}");
    }

    // A caller its bridge doesn't know is refused there, before its call.
    let stranger = rules.calling(crate::bridged::Calling {
        caller: Binding {
            key: "x".repeat(32).into(),
            ..caller.duplicate()
        },
        sandbox: "run-1".into(),
        upstream: "direct".into(),
        inbound: None,
    });
    let got = through(stranger, &get("/_link/echo")).await;
    assert!(got.starts_with("HTTP/1.1 403"), "{got}");
    // No bridge there, nor anything to start one: said.
    let gone = Filter::new(Upstream::Direct)
        .with_routes(
            crate::routes::Routes {
                routes: vec![Route {
                    bridge: Bridge {
                        socket: dir.join("bridges").join("none.sock"),
                        start: Vec::new(),
                    },
                    ..route("open-echo", "/_link/echo", false)
                }],
            },
            None,
        )
        .calling(crate::bridged::Calling {
            caller: caller.duplicate(),
            sandbox: "run-1".into(),
            upstream: "direct".into(),
            inbound: None,
        });
    let got = through(gone, &get("/_link/echo")).await;
    assert!(
        got.starts_with("HTTP/1.1 502") && got.contains("nothing starts it"),
        "{got}"
    );
}

/// A connection on `local.sock` or `in.sock` that never names its port
/// is given up once its time to name one is up, so a sandbox can't hold
/// its places there; one that names it in time is heard.
#[tokio::test]
async fn a_link_that_names_no_port_is_given_up() {
    let wait = std::time::Duration::from_millis(50);
    let (mut silent, _held) = tokio::net::UnixStream::pair().unwrap();
    assert_eq!(crate::relay::port_named(&mut silent, wait).await, None);
    let (mut named, mut sandbox) = tokio::net::UnixStream::pair().unwrap();
    sandbox.write_u16(8080).await.unwrap();
    assert_eq!(crate::relay::port_named(&mut named, wait).await, Some(8080));
}
