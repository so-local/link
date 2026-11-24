//! How connections are made: the inbound socket, families in turn, silent
//! addresses passed over, bounded accepts, socks and HTTP upstreams.

use super::*;

/// Outside connects to the sandbox's own `in.sock`, never to where a
/// link the sandbox put there points, nor to anything but a socket.
#[tokio::test]
async fn the_inbound_socket_is_reached_only_as_itself() {
    use tokio::io::AsyncReadExt;
    let dir = std::env::temp_dir().join(format!("lnk-inbound-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (inbound, elsewhere) = (dir.join("in.sock"), dir.join("bus"));
    let own = tokio::net::UnixListener::bind(&inbound).unwrap();
    let bus = tokio::net::UnixListener::bind(&elsewhere).unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = bus.accept().await {
            let _ = tokio::io::AsyncWriteExt::write_all(&mut s, b"bus").await;
        }
    });
    tokio::spawn(async move {
        while let Ok((mut s, _)) = own.accept().await {
            let _ = tokio::io::AsyncWriteExt::write_all(&mut s, b"own").await;
        }
    });
    let mut said = String::new();
    let mut link = connect_inside(&inbound, me()).await.unwrap();
    link.read_to_string(&mut said).await.unwrap();
    assert_eq!(said, "own");
    // Another user's agent: only its socket, never this user's.
    let refused = connect_inside(&inbound, me() + 1).await;
    assert!(refused.is_err(), "it took a socket of another user's");
    // A link in its place, to a socket the sandbox can't reach.
    std::fs::remove_file(&inbound).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &inbound).unwrap();
    let refused = connect_inside(&inbound, me()).await;
    assert!(refused.is_err(), "it followed the link");
    // A file, a folder: not a socket.
    std::fs::remove_file(&inbound).unwrap();
    std::fs::write(&inbound, "x").unwrap();
    assert!(connect_inside(&inbound, me()).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn families_take_turns() {
    let a = |s: &str| s.parse::<std::net::SocketAddr>().unwrap();
    let addrs = [a("[::1]:1"), a("[::2]:1"), a("[::3]:1"), a("1.1.1.1:1")];
    assert_eq!(
        interleaved(&addrs),
        [a("[::1]:1"), a("1.1.1.1:1"), a("[::2]:1"), a("[::3]:1")]
    );
    assert!(interleaved(&[]).is_empty());
}

/// An address that never answers holds up the next by a quarter
/// second, not the whole timeout.
#[tokio::test]
async fn a_silent_address_is_passed_over() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let open = listener.local_addr().unwrap();
    // TEST-NET-1: goes nowhere.
    let silent: std::net::SocketAddr = "192.0.2.1:9".parse().unwrap();
    let started = std::time::Instant::now();
    let direct = &Upstream::Direct;
    let (_, answered) = connect_any(&[(silent, direct), (open, direct)])
        .await
        .unwrap();
    assert_eq!(answered, open);
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

/// Connections numbered in turn, with failed accepts where asked;
/// then none.
struct Fake {
    next: std::sync::Mutex<std::collections::VecDeque<std::io::Result<usize>>>,
}

impl Listener for Fake {
    type Stream = usize;
    async fn next(&self) -> std::io::Result<usize> {
        let next = self.next.lock().unwrap().pop_front();
        match next {
            Some(result) => result,
            None => std::future::pending().await,
        }
    }
}

#[tokio::test]
async fn an_address_that_doesnt_answer_is_passed_for_the_next() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let there = listener.local_addr().unwrap();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = closed.local_addr().unwrap();
    drop(closed);
    let direct = &Upstream::Direct;
    let (stream, _) = connect_any(&[(gone, direct), (there, direct)])
        .await
        .unwrap();
    let Dialed::Tcp(stream) = stream else {
        panic!("dialed directly, not through a socket")
    };
    assert_eq!(stream.peer_addr().unwrap(), there);
    assert!(stream.nodelay().unwrap());
    assert!(connect_any(&[(gone, direct)]).await.is_err());
}

#[test]
fn a_connections_future_stays_small() {
    // Each verb's runs boxed: a debug build's worker stack holds
    // several copies of this one while it's polled.
    let (client, _) = tokio::io::duplex(8);
    let serving = proxy(client, Filter::new(Upstream::Direct));
    let size = std::mem::size_of_val(&serving);
    assert!(size < 16 * 1024, "{size} bytes");
}

#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_failed_accept_isnt_the_listeners_end_and_connections_are_bounded() {
    let emfile = || Err(std::io::Error::from_raw_os_error(libc::EMFILE));
    let mut next: std::collections::VecDeque<_> = [emfile(), Ok(0), emfile()].into();
    next.extend((1..MAX_CONNECTIONS + 10).map(Ok));
    let fake = Fake {
        next: std::sync::Mutex::new(next),
    };
    let served = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (_hold, held) = tokio::sync::watch::channel(());
    let serving = served.clone();
    let task = tokio::spawn(each(fake, move |n| {
        let (served, mut held) = (serving.clone(), held.clone());
        async move {
            served.lock().unwrap().push(n);
            // Open until the test ends.
            let _ = held.changed().await;
        }
    }));
    for _ in 0..100 {
        if served.lock().unwrap().len() >= MAX_CONNECTIONS {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let served = served.lock().unwrap().clone();
    assert_eq!(served.len(), MAX_CONNECTIONS, "at most that many at once");
    assert_eq!(served[..3], [0, 1, 2], "served on after failed accepts");
    task.abort();
}

/// A name with no address is kept as such for a few seconds: asked
/// again at once, as resolvers retry, it asks no one, and isn't a
/// lookup to count.
#[tokio::test]
async fn a_name_without_an_address_is_kept_a_while() {
    let name = format!("nowhere-{}.invalid", std::process::id());
    let started = std::time::Instant::now();
    assert!(resolve(&name, 443).await.is_err());
    let first = started.elapsed();
    assert!(is_resolved(&name));
    let started = std::time::Instant::now();
    for _ in 0..50 {
        assert!(resolve(&name, 443).await.is_err());
    }
    let again = started.elapsed() / 50;
    eprintln!("a name with no address: {first:?} the first time, {again:?} each after");
    assert!(again < first);
}

/// The SOCKS greeting and request go at once: an exit that answers
/// only once it has both still gets them, in one round trip.
#[tokio::test]
async fn socks_asks_in_one_round_trip() {
    let (client, mut exit) = tokio::io::duplex(1024);
    let target: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let answering = tokio::spawn(async move {
        let mut asked = [0u8; 13];
        exit.read_exact(&mut asked).await.unwrap();
        exit.write_all(&[5, 0, 5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        (asked, exit)
    });
    let connected = tokio::time::timeout(std::time::Duration::from_secs(5), socks5(client, target))
        .await
        .expect("waited for the greeting's answer first");
    connected.unwrap();
    let (asked, _exit) = answering.await.unwrap();
    assert_eq!(asked, [5, 1, 0, 5, 1, 0, 1, 192, 0, 2, 7, 1, 187]);
}

/// An HTTP upstream is handed an internet host's name, never an
/// address resolved here: a name this machine can't resolve is still
/// reached, through it. What it refuses is refused.
#[tokio::test]
async fn an_http_upstream_resolves_names_itself() {
    let target = echo().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let heard = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let heard = heard.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0u8; 1];
                    if s.read(&mut byte).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let line = String::from_utf8_lossy(&head)
                    .lines()
                    .next()
                    .unwrap()
                    .to_string();
                heard.lock().unwrap().push(line.clone());
                if !line.starts_with("CONNECT reached.") {
                    let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
                    return;
                }
                let mut to = TcpStream::connect(target).await.unwrap();
                s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut s, &mut to).await;
            });
        }
    });
    let rules = Filter::new(Upstream::Http(upstream));
    let mut dialed = dial("reached.invalid", 443, rules).await.unwrap();
    dialed.write_all(b"hello").await.unwrap();
    let mut back = [0u8; 5];
    dialed.read_exact(&mut back).await.unwrap();
    assert_eq!(&back, b"hello");
    let err = dial("refused.invalid", 443, rules).await.err().unwrap();
    assert!(format!("{err:#}").contains("403"), "{err:#}");
    assert_eq!(
        *asked.lock().unwrap(),
        [
            "CONNECT reached.invalid:443 HTTP/1.1",
            "CONNECT refused.invalid:443 HTTP/1.1"
        ]
    );
    // A name no sandbox may reach is refused here, the upstream never
    // asked.
    let listed = Filter {
        hosts: Some(Vec::leak(vec!["reached.invalid".to_string()])),
        ..rules
    };
    assert!(dial("other.invalid", 443, listed).await.is_err());
    assert_eq!(asked.lock().unwrap().len(), 2);
    // An address however a URL writes it is an address, checked here as
    // one: this machine and the metadata service, refused, the upstream
    // never handed them.
    for host in [
        "2130706433",
        "127.1",
        "0x7f.1",
        "017700000001",
        "2852039166",
    ] {
        let err = dial(host, 443, rules).await.err().unwrap();
        assert!(!format!("{err:#}").contains("403"), "{host}: {err:#}");
    }
    // `[lan] *` sends an internet name upstream all the same; a name it
    // lists by itself, or one of no internet's, is dialed from here.
    let lan_any = rules.with_lan(vec!["*".into()], true);
    let mut dialed = dial("reached.example.com", 443, lan_any).await.unwrap();
    dialed.write_all(b"hello").await.unwrap();
    dialed.read_exact(&mut back).await.unwrap();
    assert_eq!(asked.lock().unwrap().len(), 3);
    let named = rules.with_lan(vec!["nas.invalid".into()], true);
    assert!(dial("nas.invalid", 443, named).await.is_err());
    assert!(dial("printer.local", 443, lan_any).await.is_err());
    assert_eq!(asked.lock().unwrap().len(), 3);
    let metadata = dial("2852039166", 80, rules).await.err().unwrap();
    assert!(metadata.is::<crate::wire::Metadata>(), "{metadata:#}");
    assert_eq!(
        asked.lock().unwrap().len(),
        3,
        "{:?}",
        asked.lock().unwrap()
    );
}

/// With an HTTP upstream, a name `[lan] *` lets through but the internet
/// doesn't is refused before this machine's resolver hears it (AL8's
/// class: every way `[lan]` alone reaches the one lookup in `dial`). The
/// upstream, which refuses all, is never asked either.
#[tokio::test]
async fn an_http_upstream_leaves_lan_star_no_lookup_here() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let heard = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            heard.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
        }
    });
    let rules = Filter::new(Upstream::Http(upstream));
    let lan_any = |internet| rules.with_lan(vec!["*".into()], internet);
    let paths = [
        ("no-internet.al8.example.com", lan_any(false)),
        (
            "unlisted.al8.example.com",
            Filter {
                hosts: Some(Vec::leak(vec!["listed.al8.example.com".to_string()])),
                ..lan_any(true)
            },
        ),
        (
            "lan-only.al8.example.com",
            Filter {
                lan_only: true,
                ..lan_any(true)
            },
        ),
    ];
    for (host, rules) in paths {
        let err = dial(host, 443, rules).await.err().unwrap();
        assert!(format!("{err:#}").contains("refused"), "{host}: {err:#}");
        assert!(
            !link_sandbox_route::resolve::is_resolved(host),
            "{host} was looked up here"
        );
        // Its DNS, the other way a name reaches the resolver, refuses it
        // too (a query has no port, so `lan_only`, a call's, isn't its).
        if rules.lan_only {
            continue;
        }
        for qtype in [crate::dns::A, crate::dns::AAAA] {
            let plan = crate::dns::plan(host, qtype, &rules);
            assert!(
                matches!(plan, crate::dns::Plan::Refuse(_)),
                "{host}: {plan:?}"
            );
        }
    }
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
}
