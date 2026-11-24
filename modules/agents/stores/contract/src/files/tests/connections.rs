//! Connections: each agent's and owner's share, and one let go however it ends.

use super::*;

/// Homes where agent `ours00000001`'s calls wait until let through,
/// counted while they wait, and agent `panics000001`'s end its thread.
struct Gated {
    plain: Plain,
    open: Mutex<bool>,
    opened: std::sync::Condvar,
    waiting: std::sync::atomic::AtomicUsize,
}

impl Homes for Gated {
    fn home(&self, who: &Identity) -> anyhow::Result<Home> {
        assert_ne!(who.agent, "panics000001", "a call's thread ends");
        if who.agent.starts_with("ours") {
            use std::sync::atomic::Ordering;
            self.waiting.fetch_add(1, Ordering::SeqCst);
            let _waited = Waited(&self.waiting);
            let open = self.open.lock().unwrap();
            drop(self.opened.wait_while(open, |open| !*open).unwrap());
        }
        self.plain.home(who)
    }
}

/// A call no longer waiting at the gate.
struct Waited<'a>(&'a std::sync::atomic::AtomicUsize);

impl Drop for Waited<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn gated(dir: &Path) -> (PathBuf, Arc<Bridge>, Arc<Gated>) {
    let gate = Arc::new(Gated {
        plain: Plain {
            store: Arc::new(DirStore::new(&dir.join("bucket")).unwrap()),
            cap: None,
        },
        open: Mutex::new(false),
        opened: std::sync::Condvar::new(),
        waiting: Default::default(),
    });
    let (socket, bridge) = served(dir, gate.clone());
    (socket, bridge, gate)
}

/// Homes counting the calls they answer.
struct Answering {
    plain: Plain,
    ran: Arc<AtomicUsize>,
}

impl Homes for Answering {
    fn home(&self, who: &Identity) -> anyhow::Result<Home> {
        self.ran.fetch_add(1, Ordering::SeqCst);
        self.plain.home(who)
    }
}

/// A files bridge passes the test every bridge passes.
#[test]
fn the_files_bridge_is_a_bridge() {
    let dir = TempDir::short("files-conformance");
    let keyring = crate::bridge::Keyring::at(&dir.join("keys"));
    let agents: Vec<Binding> = (1..=CONNECTIONS / AGENT_CONNECTIONS + 1)
        .map(|n| {
            let identity = Identity {
                owner: format!("owner{n}@box"),
                ..who(&format!("agent{n:07}"))
            };
            keyring
                .bind(&identity, &format!("{n:0>64}").into())
                .unwrap()
        })
        .collect();
    let store = Arc::new(DirStore::new(&dir.join("bucket")).unwrap());
    // Larger, as its answer, than a connection holds.
    store
        .put("home/work/big", &vec![7u8; 4 << 20], &Expect::Absent)
        .unwrap();
    let ran = Arc::new(AtomicUsize::new(0));
    let homes = Answering {
        plain: Plain { store, cap: None },
        ran: ran.clone(),
    };
    let limits = link_sandbox_bridge::conformance::brief(LIMITS);
    let (socket, _) = served_within(&dir, Arc::new(homes), limits);
    let read = |path: &str| {
        serde_json::to_value(Action::Read {
            path: path.into(),
            team: None,
        })
        .unwrap()
    };
    link_sandbox_bridge::conformance::check(&link_sandbox_bridge::conformance::Subject {
        socket,
        agents,
        action: read("work/a"),
        ran: Arc::new(move || ran.load(Ordering::SeqCst)),
        big: Some(read("work/big")),
        limits,
    });
}

#[test]
fn one_agent_holds_at_most_its_share_of_connections() {
    let dir = TempDir::short("files-held");
    let ours = crate::testing::bound(&dir.join("keys"), "ours00000001");
    let theirs = crate::bridge::Keyring::at(&dir.join("keys"))
        .bind(
            &who("theirs000001"),
            &link_secretive::Secret::from("1".repeat(64)),
        )
        .unwrap();
    let (socket, _, gate) = gated(&dir);
    let client = |binding: &Binding| Client {
        socket: socket.clone(),
        binding: binding.duplicate(),
    };
    let waiting: Vec<_> = (0..AGENT_CONNECTIONS)
        .map(|_| {
            let client = client(&ours);
            std::thread::spawn(move || client.read("work/a"))
        })
        .collect();
    link_test_kit::eventually(
        "its calls to be held",
        std::time::Duration::from_secs(20),
        || gate.waiting.load(std::sync::atomic::Ordering::SeqCst) == AGENT_CONNECTIONS,
    );
    let err = client(&ours).read("work/a").unwrap_err();
    assert!(format!("{err:#}").contains("busy"), "{err:#}");
    // Another agent's call is still served.
    assert_eq!(client(&theirs).read("work/a").unwrap(), None);
    *gate.open.lock().unwrap() = true;
    gate.opened.notify_all();
    for call in waiting {
        assert_eq!(call.join().unwrap().unwrap(), None);
    }
    // Answered, its places are given back.
    assert_eq!(client(&ours).read("work/a").unwrap(), None);
}

#[test]
fn a_connection_is_let_go_however_its_call_ends() {
    let dir = TempDir::short("files-served");
    let panics = crate::bridge::Keyring::at(&dir.join("keys"))
        .bind(
            &who("panics000001"),
            &link_secretive::Secret::from("1".repeat(64)),
        )
        .unwrap();
    let (socket, _, _) = gated(&dir);
    let client = Client {
        socket,
        binding: panics,
    };
    // Each ends its thread, and gives its places back: never busy.
    for _ in 0..CONNECTIONS + 1 {
        let err = client.read("work/a").unwrap_err();
        assert!(!format!("{err:#}").contains("busy"), "{err:#}");
    }
}

#[test]
fn one_owner_holds_at_most_its_share_of_connections() {
    let dir = TempDir::short("files-owner");
    let keys = dir.join("keys");
    let ours: Vec<_> = (1..=2)
        .map(|n| crate::testing::bound(&keys, &format!("ours0000000{n}")))
        .collect();
    let also_ours = crate::bridge::Keyring::at(&keys)
        .bind(
            &who("theirs000001"),
            &link_secretive::Secret::from("1".repeat(64).to_string()),
        )
        .unwrap();
    let elsewhere = crate::bridge::Keyring::at(&keys)
        .bind(
            &Identity {
                owner: "else@box".into(),
                ..who("theirs000002")
            },
            &link_secretive::Secret::from("2".repeat(64).to_string()),
        )
        .unwrap();
    let (socket, _, gate) = gated(&dir);
    let client = |binding: &Binding| Client {
        socket: socket.clone(),
        binding: binding.duplicate(),
    };
    let waiting: Vec<_> = ours
        .iter()
        .flat_map(|binding| std::iter::repeat_n(binding, AGENT_CONNECTIONS))
        .map(|binding| {
            let client = client(binding);
            std::thread::spawn(move || client.read("work/a"))
        })
        .collect();
    assert_eq!(waiting.len(), OWNER_CONNECTIONS);
    link_test_kit::eventually(
        "the owner's calls to be held",
        std::time::Duration::from_secs(20),
        || gate.waiting.load(std::sync::atomic::Ordering::SeqCst) == OWNER_CONNECTIONS,
    );
    // Another agent of the same owner is turned away; another
    // owner's isn't.
    let err = client(&also_ours).read("work/a").unwrap_err();
    assert!(format!("{err:#}").contains("busy"), "{err:#}");
    assert_eq!(client(&elsewhere).read("work/a").unwrap(), None);
    *gate.open.lock().unwrap() = true;
    gate.opened.notify_all();
    for call in waiting {
        assert_eq!(call.join().unwrap().unwrap(), None);
    }
    // Answered, the owner's places are given back.
    assert_eq!(client(&also_ours).read("work/a").unwrap(), None);
}

/// The files bridge keeps room for as many peers proving as every
/// bridge does: what a machine serving it to many users (a box's
/// slots) is set up for. Each peer's share is the server's
/// (`link_sandbox_bridge`'s tests, and the test every bridge passes).
#[test]
fn the_files_bridge_keeps_room_for_its_peers() {
    assert_eq!(PEERS, crate::bridge::PEERS);
    const { assert!(PEERS * crate::bridge::PEER_PROVING <= crate::bridge::PROVING) };
}

/// A proved call whose action comes a byte at a time is let go once
/// its time is up, its place given back. That it holds its place until
/// then is the next test's, with a wait no busy machine outlasts.
#[test]
fn a_proved_call_sent_slowly_is_let_go() {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;
    let dir = TempDir::short("files-slow");
    let ours = crate::testing::bound(&dir.join("keys"), "plain0000001");
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(crate::bridge::Keyring::at(&dir.join("keys")));
    let homes = Arc::new(Plain {
        store: Arc::new(DirStore::new(&dir.join("bucket")).unwrap()),
        cap: None,
    });
    let bridge = Arc::new(Bridge::new(keys, homes));
    let limits = crate::bridge::Limits {
        action_wait: std::time::Duration::from_secs(2),
        per_agent: 1,
        ..LIMITS
    };
    std::thread::spawn(move || serve_bridge(listener, bridge, &limits));
    let started = std::time::Instant::now();
    let stream = UnixStream::connect(&socket).unwrap();
    let mut lines = std::io::BufReader::new(stream.try_clone().unwrap());
    let mut challenge = String::new();
    std::io::BufRead::read_line(&mut lines, &mut challenge).unwrap();
    let challenge: crate::bridge::Challenge = serde_json::from_str(&challenge).unwrap();
    let mut proven = serde_json::to_vec(&ours.prove(&challenge.challenge)).unwrap();
    proven.push(b'\n');
    (&stream).write_all(&proven).unwrap();
    let client = Client {
        socket: socket.clone(),
        binding: ours,
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
    let mut answer = String::new();
    loop {
        // A bridge that answered and closed refuses the byte, its
        // answer still waiting to be read: read it once all the same.
        let wrote = (&stream).write_all(b" ").is_ok();
        if std::io::BufRead::read_line(&mut lines, &mut answer).is_ok_and(|n| n > 0) || !wrote {
            break;
        }
        assert!(started.elapsed().as_secs() < 15, "never let go");
    }
    assert!(answer.contains("didn't come in time"), "{answer}");
    // Its place given back.
    assert_eq!(client.read("work/a").unwrap(), None);
}

/// A proved call holds its agent's one place while its action is still
/// coming, so another call of its is busy, and gives it back when it
/// ends. Its wait is longer than any test takes, so seeing it busy never
/// races its being let go.
#[test]
fn a_proved_call_holds_its_place_until_it_ends() {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;
    let dir = TempDir::short("files-held");
    let ours = crate::testing::bound(&dir.join("keys"), "plain0000001");
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(crate::bridge::Keyring::at(&dir.join("keys")));
    let homes = Arc::new(Plain {
        store: Arc::new(DirStore::new(&dir.join("bucket")).unwrap()),
        cap: None,
    });
    let bridge = Arc::new(Bridge::new(keys, homes));
    let limits = crate::bridge::Limits {
        action_wait: std::time::Duration::from_secs(600),
        per_agent: 1,
        ..LIMITS
    };
    std::thread::spawn(move || serve_bridge(listener, bridge, &limits));
    // A call proved, its action never sent: it holds the agent's one
    // place for as long as it's open.
    let proved = || {
        let stream = UnixStream::connect(&socket).unwrap();
        let mut lines = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut challenge = String::new();
        std::io::BufRead::read_line(&mut lines, &mut challenge).unwrap();
        let challenge: crate::bridge::Challenge = serde_json::from_str(&challenge).unwrap();
        let mut proven = serde_json::to_vec(&ours.prove(&challenge.challenge)).unwrap();
        proven.push(b'\n');
        (&stream).write_all(&proven).unwrap();
        stream
    };
    // Whether the bridge answered `stream`, or closed it: refused, as a
    // call that came for the place before it took it. What's read is
    // thrown away with the stream.
    let answered = |stream: &UnixStream| {
        use std::io::Read as _;
        stream.set_nonblocking(true).unwrap();
        let answered = !matches!(
            (&*stream).read(&mut [0u8; 1]),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock
        );
        stream.set_nonblocking(false).unwrap();
        answered
    };
    let mut stream = proved();
    // Its one place held meanwhile: another call of its is busy, at
    // once. That busy answer shows the proved call holds the place, as
    // nothing else here keeps one; if a call made here came first and
    // the proved one was refused, it proves again.
    let read = Action::Read {
        path: "work/a".into(),
        team: None,
    };
    link_test_kit::eventually(
        "its call to hold its place",
        std::time::Duration::from_secs(20),
        || {
            if answered(&stream) {
                stream = proved();
            }
            // One call, never made again while busy.
            let once = || -> anyhow::Result<Answer> {
                let stream = UnixStream::connect(&socket)?;
                crate::bridge::call(&stream, &ours, &read)?;
                crate::bridge::read_answer(&stream, ANSWER_MAX)
            };
            once().is_err_and(|err| {
                err.downcast_ref::<Failure>()
                    .is_some_and(|f| f.why == Why::Busy)
            })
        },
    );
    // Ended, its place is given back.
    drop(stream);
    let client = Client {
        socket: socket.clone(),
        binding: ours,
    };
    link_test_kit::eventually(
        "its place given back",
        std::time::Duration::from_secs(20),
        || client.read("work/a").is_ok_and(|read| read.is_none()),
    );
}

/// A caller that never proves: a byte a second, never a line, until
/// the bridge answers it. What it was answered, or nothing.
fn never_proving(
    mut stream: std::os::unix::net::UnixStream,
    mut lines: std::io::BufReader<std::os::unix::net::UnixStream>,
) -> String {
    use std::io::Write as _;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
    loop {
        // A bridge that answered and closed refuses the byte, its
        // answer still waiting to be read: read it once all the same.
        let wrote = stream.write_all(b" ").is_ok();
        let mut more = String::new();
        if std::io::BufRead::read_line(&mut lines, &mut more).is_ok_and(|n| n > 0) {
            return more;
        }
        if !wrote {
            return String::new();
        }
    }
}

/// Answered and closed before its next byte, it still reads the
/// answer: the byte refused is no reason to leave it unread.
#[test]
fn a_caller_closed_before_its_byte_reads_its_answer() {
    use std::io::Write as _;
    let (ours, mut bridge) = std::os::unix::net::UnixStream::pair().unwrap();
    bridge.write_all(b"{\"why\":\"busy\"}\n").unwrap();
    drop(bridge);
    let lines = std::io::BufReader::new(ours.try_clone().unwrap());
    assert!(never_proving(ours, lines).contains("busy"));
}

/// A connection that never proves who it is, a byte at a time, is
/// let go once its time to prove is up; its peer's next connection
/// takes its place meanwhile, closing the peer's oldest, so its
/// peer's calls are served throughout.
#[test]
fn a_connection_that_never_proves_is_let_go() {
    use std::os::unix::net::UnixStream;
    let peer_proving = crate::bridge::PEER_PROVING;
    let dir = TempDir::short("files-unproven");
    let ours = crate::testing::bound(&dir.join("keys"), "plain0000001");
    let (socket, _, _) = gated(&dir);
    let started = std::time::Instant::now();
    let connect = |socket: PathBuf| {
        let stream = UnixStream::connect(&socket).unwrap();
        let mut lines = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut challenge = String::new();
        std::io::BufRead::read_line(&mut lines, &mut challenge).unwrap();
        (stream, lines)
    };
    let holding: Vec<_> = (0..peer_proving)
        .map(|_| {
            let (stream, lines) = connect(socket.clone());
            std::thread::spawn(move || never_proving(stream, lines))
        })
        .collect();
    // Its peer's call is served all the same: the oldest of those is
    // closed for it, busy.
    let client = Client {
        socket: socket.clone(),
        binding: ours,
    };
    assert_eq!(client.read("work/a").unwrap(), None);
    let answers: Vec<String> = holding.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(answers[0].contains("busy"), "{answers:?}");
    for answer in &answers[1..] {
        assert!(answer.contains("no proof of who's calling"), "{answer}");
    }
    assert!(
        started.elapsed() < crate::bridge::PROOF_WAIT + std::time::Duration::from_secs(5),
        "let go after {:?}",
        started.elapsed()
    );
    assert_eq!(client.read("work/a").unwrap(), None);
}
