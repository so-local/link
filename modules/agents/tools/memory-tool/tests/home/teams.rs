//! A team's memory: an agent's session of it, over the team's folder
//! through the files bridge as the agent, its index beside the folder.

use super::*;
use link_harness_contract::harness::TeamFolders;
use link_memory_tool_contract::TeamRef;

/// The agent's Home, and one team's folder, `research`, granted as
/// `grant` says now: asked at each call, as a box asks its store.
struct WithTeam {
    home: Arc<dyn Store>,
    team: Arc<dyn Store>,
    grant: Mutex<TeamFolders>,
}

impl Homes for WithTeam {
    fn home(&self, _: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: self.home.clone(),
            cap: None,
            write: vec![".".into()],
        })
    }
    fn team(&self, who: &Identity, team: &TeamRef) -> anyhow::Result<Option<files::TeamFolder>> {
        Ok((team.to_string() == "research").then(|| files::TeamFolder {
            store: self.team.clone(),
            id: RESEARCH.to_string(),
            owner: who.owner.clone(),
            folders: self.grant.lock().unwrap().clone(),
            keeps_index: true,
        }))
    }
}

const RESEARCH: &str = "t4m9q2x7k1ab";

fn grant(write: &[&str]) -> TeamFolders {
    TeamFolders {
        write: write.iter().map(|s| s.to_string()).collect(),
        read: Vec::new(),
    }
}

/// A hello for the team named `named`, its ID `id`, written whole or not.
fn with_team(client: &files::Client, named: &str, id: &str, writes: bool) -> Value {
    let mut hello = hello(client);
    hello["team"] = json!({"id": id, "named": named, "writes": writes});
    hello
}

/// A member granted all of a team's folder is served the team's memory,
/// a session of its own: the team's `memory/`, never its Home's, kept
/// current as it's told the folder changed, its index written back
/// beside the folder at a flush and ended with the agent. A member
/// granted part of it is refused, before its index is read.
#[test]
fn a_team_memory_serves_whole_members_from_beside_the_folder() {
    let dir = TempDir::short("home-team");
    std::fs::create_dir_all(&dir).unwrap();
    let binding = Keyring::at(&dir.join("keys"))
        .bind(
            &Identity {
                agent: AGENT.into(),
                service: link_store_contract::testing::TEST_SERVICE.into(),
                owner: "me@box".into(),
                teams: Vec::new(),
            },
            &link_secretive::Secret::from(link_store_contract::testing::TEST_KEY.to_string()),
        )
        .unwrap();
    let homes = Arc::new(WithTeam {
        home: Arc::new(DirStore::new(&dir.join("home")).unwrap()),
        team: Arc::new(DirStore::new(&dir.join("team")).unwrap()),
        grant: Mutex::new(grant(&["."])),
    });
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    let served = homes.clone();
    std::thread::spawn(move || files::serve(listener, keys, served));
    let client = files::Client { socket, binding };
    let research = files::InTeam {
        client: client.duplicate(),
        team: TeamRef::parse("research").unwrap(),
    };
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    research
        .put(
            "home/memory/plan.md",
            b"The launch plan is ready.\n",
            &Expect::Absent,
        )
        .unwrap();
    let service = cache_service(&dir, "cache");
    // Its own memory, and the team's: each its own folder alone.
    let (answer, mut own) = first(&service, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    let (answer, mut team) = first(&service, &with_team(&client, "research", RESEARCH, true));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut team, "launch")), ["memory/plan.md"]);
    assert!(found(&search(&mut team, "sails")).is_empty());
    assert!(found(&search(&mut own, "launch")).is_empty());
    // A teammate's write, told: found within 3 s.
    research
        .put(
            "home/memory/budget.md",
            b"The budget doubled.\n",
            &Expect::Absent,
        )
        .unwrap();
    let mut changed = serde_json::to_value(Control::Changed {
        owner: "me@box".into(),
        agent: AGENT.into(),
        path: "memory/budget.md".into(),
        team: Some(RESEARCH.into()),
    })
    .unwrap();
    assert_eq!(first(&service, &changed).0["ok"], true);
    let hits = link_test_kit::wait::eventually(
        "the change found by search",
        Duration::from_secs(3),
        || {
            let hits = found(&search(&mut team, "budget"));
            (!hits.is_empty()).then_some(hits)
        },
    );
    assert_eq!(hits, ["memory/budget.md"]);
    // The agent's flush writes the team's index back beside its folder.
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&service, &flush).0["ok"], true);
    assert!(!homes.team.list("index/").unwrap().is_empty());
    // Ended with the agent: the team's session answers nothing more.
    let end = serde_json::to_value(Control::End {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&service, &end).0["ok"], true);
    writeln!(
        team.get_mut(),
        "{}",
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping"})
    )
    .unwrap();
    let mut rest = String::new();
    let _ = team.read_to_string(&mut rest);
    assert!(rest.is_empty(), "{rest}");
    // Bound again from the copy beside the folder, its cache gone.
    std::fs::remove_dir_all(dir.join("cache/me@box").join(AGENT)).unwrap();
    let (answer, mut team) = first(&service, &with_team(&client, "research", RESEARCH, true));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut team, "budget")), ["memory/budget.md"]);
    assert_eq!(first(&service, &end).0["ok"], true);
    // Granted part of the folder now: refused at bind.
    *homes.grant.lock().unwrap() = grant(&["memory"]);
    let (answer, _) = first(&service, &with_team(&client, "research", RESEARCH, true));
    assert!(
        answer["error"]
            .as_str()
            .is_some_and(|e| e.contains("granted all")),
        "{answer}"
    );
    // A team it isn't on, and a team's memory with no files bridge.
    let (answer, _) = first(
        &service,
        &with_team(&client, "design", "z9y8x7w6v5u4", true),
    );
    assert_eq!(answer["ok"], Value::Null, "{answer}");
    changed["changed"]["team"] = "z9y8x7w6v5u4".into();
    assert_eq!(first(&service, &changed).0["ok"], true);
    // A field it doesn't know is refused, saying which.
    let mut unknown = with_team(&client, "research", RESEARCH, true);
    unknown["teem"] = "research".into();
    let (answer, _) = first(&service, &unknown);
    assert!(
        answer["error"]
            .as_str()
            .is_some_and(|e| e.contains("unknown field `teem`")),
        "{answer}"
    );
    let mut unbridged = with_team(&client, "research", RESEARCH, true);
    unbridged["bound"]["files"] = Value::Null;
    let (answer, _) = first(&service, &unbridged);
    assert_eq!(answer["ok"], Value::Null, "{answer}");
}

/// A team's memory is the team's by its ID: its cache kept under it, and
/// a team that takes another's name is a session of its own. A member
/// that reads all of the folder but writes none of it is served the
/// copy beside the folder and lists what changed, writing nothing back;
/// its bridge would refuse it anyway.
#[test]
fn a_team_memory_is_its_teams_by_id_and_a_reader_writes_nothing() {
    let dir = TempDir::short("home-team-reader");
    std::fs::create_dir_all(&dir).unwrap();
    let binding = Keyring::at(&dir.join("keys"))
        .bind(
            &Identity {
                agent: AGENT.into(),
                service: link_store_contract::testing::TEST_SERVICE.into(),
                owner: "me@box".into(),
                teams: Vec::new(),
            },
            &link_secretive::Secret::from(link_store_contract::testing::TEST_KEY.to_string()),
        )
        .unwrap();
    let homes = Arc::new(WithTeam {
        home: Arc::new(DirStore::new(&dir.join("home")).unwrap()),
        team: Arc::new(DirStore::new(&dir.join("team")).unwrap()),
        grant: Mutex::new(grant(&["."])),
    });
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    let served = homes.clone();
    std::thread::spawn(move || files::serve(listener, keys, served));
    let client = files::Client { socket, binding };
    let research = files::InTeam {
        client: client.duplicate(),
        team: TeamRef::parse("research").unwrap(),
    };
    research
        .put(
            "home/memory/plan.md",
            b"The launch plan is ready.\n",
            &Expect::Absent,
        )
        .unwrap();
    let service = cache_service(&dir, "cache");
    let (answer, _own) = first(&service, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    // The writer makes the copy.
    let (answer, _) = first(&service, &with_team(&client, "research", RESEARCH, true));
    assert_eq!(answer["ok"], true, "{answer}");
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&service, &flush).0["ok"], true);
    let cache = dir.join("cache/me@box").join(AGENT).join("teams");
    assert!(cache.join(RESEARCH).join("index.sqlite").exists());
    // Another team, now named as this one was: a session of its own.
    let (answer, _) = first(
        &service,
        &with_team(&client, "research", "p3q4r5s6t7u8", true),
    );
    assert_eq!(answer["ok"], true, "{answer}");
    assert!(cache.join("p3q4r5s6t7u8").join("index.sqlite").exists());
    // Slept, and bound again reading alone.
    let end = serde_json::to_value(Control::End {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&service, &end).0["ok"], true);
    let (answer, _own) = first(&service, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    // Read alone now: served the copy, its listing kept, nothing written.
    *homes.grant.lock().unwrap() = TeamFolders {
        write: Vec::new(),
        read: vec![".".into()],
    };
    let copy = |store: &dyn Store| -> Vec<(String, String)> {
        let mut objects: Vec<_> = store
            .list("index/")
            .unwrap()
            .into_iter()
            .map(|o| (o.path, format!("{:?}", o.version)))
            .collect();
        objects.sort();
        objects
    };
    let before = copy(homes.team.as_ref());
    let (answer, mut reading) = first(&service, &with_team(&client, "research", RESEARCH, false));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut reading, "launch")), ["memory/plan.md"]);
    homes
        .team
        .put(
            "home/memory/budget.md",
            b"The budget doubled.\n",
            &Expect::Absent,
        )
        .unwrap();
    let changed = serde_json::to_value(Control::Changed {
        owner: "me@box".into(),
        agent: AGENT.into(),
        path: "memory/budget.md".into(),
        team: Some(RESEARCH.into()),
    })
    .unwrap();
    assert_eq!(first(&service, &changed).0["ok"], true);
    let hits = link_test_kit::wait::eventually(
        "the change found by search",
        Duration::from_secs(3),
        || {
            let hits = found(&search(&mut reading, "budget"));
            (!hits.is_empty()).then_some(hits)
        },
    );
    assert_eq!(hits, ["memory/budget.md"]);
    assert_eq!(first(&service, &flush).0["ok"], true);
    assert_eq!(
        copy(homes.team.as_ref()),
        before,
        "a reader wrote the index"
    );
}

/// An embeddings endpoint on a Unix socket counting each request it
/// answers, every text embedded the same.
fn embedder(socket: &Path) -> Arc<AtomicUsize> {
    let listener = UnixListener::bind(socket).unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let counting = asked.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            counting.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let line = line.trim_end().to_ascii_lowercase();
                if line.is_empty() {
                    break;
                }
                if let Some(n) = line.strip_prefix("content-length: ") {
                    length = n.parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            if reader.read_exact(&mut body).is_err() {
                continue;
            }
            let asked: Value = serde_json::from_slice(&body).unwrap_or_default();
            let data: Vec<Value> = asked["input"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, _)| json!({"index": i, "embedding": [1.0, 0.5]}))
                .collect();
            let reply = json!({"data": data}).to_string();
            let mut stream = reader.into_inner();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    asked
}

/// A team's memory for a member that doesn't keep its index embeds
/// nothing, so its account pays for none of it (the team owner pays for
/// the team's memory): with the keeper's copy it finds what the copy
/// holds and what changed since by their words, and with no copy yet,
/// the folder by its words, never asking the embeddings endpoint. The
/// keeper's session does ask it.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "five flushes, each the 200 ms a keeper's embedder asks within, show one that keeps no index never asks"
)]
fn a_member_that_keeps_no_index_never_embeds() {
    let dir = TempDir::short("home-team-embed");
    std::fs::create_dir_all(&dir).unwrap();
    let asked = embedder(&dir.join("embed.sock"));
    let binding = Keyring::at(&dir.join("keys"))
        .bind(
            &Identity {
                agent: AGENT.into(),
                service: link_store_contract::testing::TEST_SERVICE.into(),
                owner: "me@box".into(),
                teams: Vec::new(),
            },
            &link_secretive::Secret::from(link_store_contract::testing::TEST_KEY.to_string()),
        )
        .unwrap();
    let homes = Arc::new(WithTeam {
        home: Arc::new(DirStore::new(&dir.join("home")).unwrap()),
        team: Arc::new(DirStore::new(&dir.join("team")).unwrap()),
        grant: Mutex::new(grant(&["."])),
    });
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    let served = homes.clone();
    std::thread::spawn(move || files::serve(listener, keys, served));
    let client = files::Client { socket, binding };
    homes
        .team
        .put(
            "home/memory/plan.md",
            b"The launch plan is ready.\n",
            &Expect::Absent,
        )
        .unwrap();
    let service = dir.join("embedding.sock");
    let config = service::Config {
        cache: Some(dir.join("cache")),
        write_every: Duration::from_secs(3600),
        files_bridge: Some(dir.join("files.sock")),
        embed: Some((
            format!("unix:{}", dir.join("embed.sock").display()),
            "qwen3-embedding".into(),
        )),
        ..service::Config::default()
    };
    let serving = service.clone();
    std::thread::spawn(move || {
        service::serve_with(
            &serving,
            link_memory_tool_contract::declared::Agents(Default::default()),
            config,
        )
    });
    link_test_kit::wait::eventually("the service to listen", Duration::from_secs(10), || {
        service.exists()
    });
    let control = |control: Control| first(&service, &serde_json::to_value(control).unwrap()).0;
    let flush = || {
        control(Control::Flush {
            owner: "me@box".into(),
            agent: AGENT.into(),
        })
    };
    let end = || {
        control(Control::End {
            owner: "me@box".into(),
            agent: AGENT.into(),
        })
    };
    // Flushed for a second: a keeper's embedder asks within 200 ms of one.
    let settled = || {
        for _ in 0..5 {
            assert_eq!(flush()["ok"], true);
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    // With no copy yet, a member that keeps none: by words, no call.
    let (answer, _own) = first(&service, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    let (answer, mut reading) = first(&service, &with_team(&client, "research", RESEARCH, false));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut reading, "launch")), ["memory/plan.md"]);
    settled();
    let teams_asked = |before: usize| asked.load(Ordering::SeqCst) - before;
    assert_eq!(
        teams_asked(0),
        0,
        "a member keeping no index asked for embeddings"
    );
    assert!(
        homes.team.list("index/").unwrap().is_empty(),
        "it wrote a copy"
    );
    assert_eq!(end()["ok"], true);
    // The keeper makes the copy, and asks the endpoint.
    let (answer, _own) = first(&service, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    let (answer, mut keeping) = first(&service, &with_team(&client, "research", RESEARCH, true));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut keeping, "launch")), ["memory/plan.md"]);
    // Its embedder is found in the background: it asks within 10 s.
    link_test_kit::wait::eventually("the keeper to embed", Duration::from_secs(10), || {
        assert_eq!(flush()["ok"], true);
        asked.load(Ordering::SeqCst) > 0
    });
    assert!(!homes.team.list("index/").unwrap().is_empty());
    assert_eq!(end()["ok"], true);
    // With the copy: what it holds and what changed since, by words alone.
    let before = asked.load(Ordering::SeqCst);
    std::fs::remove_dir_all(dir.join("cache/me@box").join(AGENT)).unwrap();
    let (answer, _own) = first(&service, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    let (answer, mut reading) = first(&service, &with_team(&client, "research", RESEARCH, false));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut reading, "launch")), ["memory/plan.md"]);
    homes
        .team
        .put(
            "home/memory/budget.md",
            b"The budget doubled.\n",
            &Expect::Absent,
        )
        .unwrap();
    let changed = control(Control::Changed {
        owner: "me@box".into(),
        agent: AGENT.into(),
        path: "memory/budget.md".into(),
        team: Some(RESEARCH.into()),
    });
    assert_eq!(changed["ok"], true);
    let hits = link_test_kit::wait::eventually(
        "the change found by search",
        Duration::from_secs(3),
        || {
            let hits = found(&search(&mut reading, "budget"));
            (!hits.is_empty()).then_some(hits)
        },
    );
    assert_eq!(hits, ["memory/budget.md"]);
    settled();
    assert_eq!(
        teams_asked(before),
        0,
        "a member keeping no index asked for embeddings"
    );
}
