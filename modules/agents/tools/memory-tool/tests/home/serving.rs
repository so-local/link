//! The service binding a machine's agents, serving each from its copy,
//! and the embedders it is handed.

use super::*;

/// The service binding a machine's agents: an agent bound, a file
/// written through the bridge found within 3 s of the bridge saying so;
/// flushed, ended and its cache removed, bound again from the copy alone,
/// the same; ended, its sessions refused.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn the_service_serves_bound_agents_from_their_copy() {
    let dir = TempDir::short("home-service");
    let (client, _) = files_bridge(&dir);
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    let socket = cache_service(&dir, "cache");
    // Not bound: refused.
    let (answer, _) = first(&socket, &json!({"agent": AGENT}));
    assert_eq!(answer["ok"], Value::Null, "{answer}");
    let (answer, mut session) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut session, "sails")), ["memory/boat.md"]);
    client
        .write(
            "memory/anchor.md",
            b"The anchor chain is rusty.\n",
            Expect::Absent,
        )
        .unwrap();
    let changed = serde_json::to_value(Control::Changed {
        owner: "me@box".into(),
        agent: AGENT.into(),
        path: "memory/anchor.md".into(),
        team: None,
    })
    .unwrap();
    assert_eq!(first(&socket, &changed).0["ok"], true);
    let written = Instant::now();
    let mut hits = Vec::new();
    while written.elapsed() < Duration::from_secs(3) && hits.is_empty() {
        hits = found(&search(&mut session, "anchor"));
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(hits, ["memory/anchor.md"], "not found within 3 s");
    // Flushed: the copy holds it. Ended, and its cache removed, as the
    // machine removes an agent's that left it.
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&socket, &flush).0["ok"], true);
    let end = serde_json::to_value(Control::End {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&socket, &end).0["ok"], true);
    std::fs::remove_dir_all(dir.join("cache/me@box").join(AGENT)).unwrap();
    // The session open when it was unbound answers nothing more.
    writeln!(
        session.get_mut(),
        "{}",
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping"})
    )
    .unwrap();
    let mut rest = String::new();
    let _ = session.read_to_string(&mut rest);
    assert!(rest.is_empty(), "{rest}");
    // Bound again: from the copy beside Home, which has both.
    let (answer, mut session) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut session, "anchor")), ["memory/anchor.md"]);
    let end = serde_json::to_value(Control::End {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&socket, &end).0["ok"], true);
    let (answer, _) = first(&socket, &json!({"agent": AGENT}));
    assert_eq!(answer["ok"], Value::Null, "{answer}");
    // Its cache stays for the next binding.
    assert!(
        dir.join("cache/me@box")
            .join(AGENT)
            .join("index.sqlite")
            .exists()
    );
}

/// Two machines' caches of one agent's index: the one behind is refused,
/// fetches the copy again, and writes back what it adds.
#[test]
fn a_cache_behind_another_fetches_again() {
    let dir = TempDir::short("home-two");
    let (client, _) = files_bridge(&dir);
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    let one = cache_service(&dir, "one");
    let two = cache_service(&dir, "two");
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    // Both bound from no copy; the first writes it.
    let (_, mut a) = first(&one, &hello(&client));
    let (_, mut b) = first(&two, &hello(&client));
    assert_eq!(found(&search(&mut a, "sails")), ["memory/boat.md"]);
    assert_eq!(found(&search(&mut b, "sails")), ["memory/boat.md"]);
    assert_eq!(first(&one, &flush).0["ok"], true);
    let (_, written) = cache::manifest(&client).unwrap().unwrap();
    // The second is behind: refused, it fetches again and writes.
    let (answer, _) = first(&two, &flush);
    assert_eq!(answer["ok"], true, "{answer}");
    let (_, now) = cache::manifest(&client).unwrap().unwrap();
    assert_ne!(written, now);
}

/// Counts what it embeds.
struct Counting {
    texts: AtomicUsize,
}

impl Embedder for Counting {
    fn model(&self) -> &str {
        "counting"
    }
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.texts.fetch_add(texts.len(), Ordering::SeqCst);
        Ok(texts.iter().map(|t| vec![t.len() as f32, 1.0]).collect())
    }
}

/// An index moved with Home (its files beside a copy of the folder) is
/// used as it is: no chunk embedded again.
#[test]
fn an_index_moved_with_home_is_used_as_it_is() {
    let here = TempDir::new("home-moved-here");
    std::fs::create_dir_all(here.join("memory")).unwrap();
    std::fs::write(
        here.join("memory/a.md"),
        "# Garden\n\nSow the beans in April.\n",
    )
    .unwrap();
    let file = here.join("index.sqlite");
    let embedder = Arc::new(Counting {
        texts: AtomicUsize::new(0),
    });
    {
        let mut index = Index::open(&here, Some(&file)).unwrap();
        index.embed_with(embedder.clone()).unwrap();
        index.refresh().unwrap();
        while index.embed_pending(16).unwrap() > 0 {}
    }
    assert!(embedder.texts.load(Ordering::SeqCst) > 0);
    // Elsewhere: the folder copied, as a move puts it, and the index file.
    let there = TempDir::new("home-moved-there");
    std::fs::create_dir_all(there.join("memory")).unwrap();
    std::fs::copy(here.join("memory/a.md"), there.join("memory/a.md")).unwrap();
    let modified = std::fs::metadata(here.join("memory/a.md"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(there.join("memory/a.md"))
        .unwrap()
        .set_modified(modified)
        .unwrap();
    // Its vectors beside it, in files of their own, and its words in a
    // folder.
    for end in ["", ".codes", ".small"] {
        std::fs::copy(
            here.join(format!("index.sqlite{end}")),
            there.join(format!("index.sqlite{end}")),
        )
        .unwrap();
    }
    std::fs::create_dir_all(there.join("index.sqlite.words")).unwrap();
    for file in std::fs::read_dir(here.join("index.sqlite.words")).unwrap() {
        let file = file.unwrap();
        std::fs::copy(
            file.path(),
            there.join("index.sqlite.words").join(file.file_name()),
        )
        .unwrap();
    }
    let moved = Arc::new(Counting {
        texts: AtomicUsize::new(0),
    });
    let mut index = Index::open(&there, Some(&there.join("index.sqlite"))).unwrap();
    index.embed_with(moved.clone()).unwrap();
    index.refresh().unwrap();
    assert_eq!(index.embed_pending(16).unwrap(), 0);
    assert_eq!(moved.texts.load(Ordering::SeqCst), 0);
    assert_eq!(paths(&index, "beans"), ["memory/a.md"]);
}

/// A Unix embedding request carries a binding proof, never an owner.
#[test]
fn an_embedder_on_a_socket_proves_its_binding() {
    let dir = TempDir::short("home-embed");
    let socket = dir.join("embed.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let headers = Arc::new(Mutex::new(Vec::new()));
    let seen = headers.clone();
    let serving = std::thread::spawn(move || {
        for stream in listener.incoming().take(2).flatten() {
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let original = line.trim_end();
                let line = original.to_ascii_lowercase();
                if line.is_empty() {
                    break;
                }
                if line.starts_with("x-link-owner:") || line.starts_with("authorization:") {
                    seen.lock().unwrap().push(original.to_string());
                }
                if let Some(n) = line.strip_prefix("content-length: ") {
                    length = n.parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let asked: Value = serde_json::from_slice(&body).unwrap();
            let data: Vec<Value> = asked["input"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, _)| json!({"index": i, "embedding": [1.0, 0.5]}))
                .collect();
            let reply = json!({"data": data}).to_string();
            let mut stream = reader.into_inner();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{reply}\r\n0\r\n\r\n",
                reply.len()
            )
            .unwrap();
        }
    });
    let binding = link_sandbox_binding::Binding {
        identity: link_sandbox_binding::Identity {
            agent: "a1b2c3d4e5f6".into(),
            service: String::new(),
            owner: "acct-42".into(),
            teams: Vec::new(),
        },
        key: "the-binding-secret-0123456789abcdef".to_string().into(),
    };
    let http = link_memory_tool::semantic::Http::for_binding(
        &format!("unix:{}", socket.display()),
        "qwen3-embedding",
        &binding,
        None,
    )
    .unwrap();
    let vectors = http.embed(&["one".into(), "two".into()]).unwrap();
    assert_eq!(vectors, [vec![1.0, 0.5], vec![1.0, 0.5]]);
    assert_eq!(http.embed_query("query").unwrap(), vec![1.0, 0.5]);
    serving.join().unwrap();
    let headers = headers.lock().unwrap();
    assert_eq!(headers.len(), 2);
    assert!(
        headers
            .iter()
            .all(|h| !h.to_ascii_lowercase().starts_with("x-link-owner:")),
        "{headers:?}"
    );
    for header in headers.iter() {
        let authentication = link_memory_tool_contract::embedding::Authentication::from_header(
            header.strip_prefix("Authorization: ").unwrap(),
        )
        .unwrap();
        assert!(authentication.matches(&binding));
    }
    for url in ["http://127.0.0.1:1", "https://example.com"] {
        assert!(link_memory_tool::semantic::Http::for_binding(url, "m", &binding, None).is_err());
    }
}
