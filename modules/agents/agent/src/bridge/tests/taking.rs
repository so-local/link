//! The way in taking messages from others: a teammate's or a stranger's,
//! each call only as its proof says.

use super::*;

/// A message from others reaches the harness as theirs, with its
/// hops; a subagent's answer goes back to its parent's conversation,
/// one hop on, and that answer to the parent's chat, through the
/// connection's `send`. A subagent's own subagent answers up the chain.
#[test]
fn the_way_in_carries_messages_from_others() {
    use link_connection_contract::{AnswerTo, Firing};
    let fake = Fake::new("way-in", serde_json::json!([]));
    let answer = fake
        .runtime
        .block_on(fake.way_in.fire(Firing {
            parent: None,
            delivery: None,
            occurrence: None,
            model: None,
            thread: "task-a1b2c3d4e5f6".into(),
            from: "task:a1b2c3d4e5f6".into(),
            text: "Find a source.".into(),
            answer: AnswerTo::Thread("telegram:42".into()),
            hops: 1,
            tools: None,
            standing: link_connection_contract::access::Standing::Owner,
        }))
        .unwrap();
    assert_eq!(answer, "On Find a source.");
    assert_eq!(
        fake.asked.lock().unwrap().clone(),
        [
            asked(
                "task-a1b2c3d4e5f6",
                "task:a1b2c3d4e5f6",
                "1",
                "Find a source."
            ),
            asked("telegram:42", "task:a1b2c3d4e5f6", "2", "On Find a source."),
        ]
    );
    assert_eq!(
        fake.wait_sent(1),
        [("42".into(), "On On Find a source.".into())]
    );
    // Its subagent's answer goes to its conversation, and the answer
    // there up to where it was asked from, as from it.
    fake.asked.lock().unwrap().clear();
    fake.runtime
        .block_on(fake.way_in.fire(Firing {
            parent: None,
            delivery: None,
            occurrence: None,
            model: None,
            thread: "task-ffffffffffff".into(),
            from: "task:ffffffffffff".into(),
            text: "Dig.".into(),
            answer: AnswerTo::Thread("task-a1b2c3d4e5f6".into()),
            hops: 2,
            tools: None,
            standing: link_connection_contract::access::Standing::Owner,
        }))
        .unwrap();
    fake.wait_sent(2);
    assert_eq!(
        fake.asked.lock().unwrap().clone(),
        [
            asked("task-ffffffffffff", "task:ffffffffffff", "2", "Dig."),
            asked("task-a1b2c3d4e5f6", "task:ffffffffffff", "3", "On Dig."),
            asked("telegram:42", "task:a1b2c3d4e5f6", "2", "On On Dig."),
        ]
    );
    assert_eq!(fake.sent()[1], ("42".into(), "On On On Dig.".into()));
    // A sender no message may be from: refused before the harness.
    let refused = fake.runtime.block_on(fake.way_in.fire(Firing {
        thread: "telegram:42".into(),
        from: "owner:me".into(),
        text: "Hi.".into(),
        ..Default::default()
    }));
    assert!(refused.is_err());
    // Nothing here narrowed a conversation's tools.
    assert!(fake.narrowed.lock().unwrap().is_empty());
}

/// Another agent's message is taken only from a teammate, as the
/// teams' files say when it comes: answered in its conversation, the
/// answer to the scheduler that sent it, not to a chat.
#[test]
fn a_teammates_message_is_taken_and_a_strangers_isnt() {
    use link_connection_contract::{AnswerTo, Firing};
    let mut fake = Fake::new("teammate", serde_json::json!([]));
    fake.way_in.endpoint.identity.agent = "a1b2c3d4e5f6".into();
    fake.way_in.terms.me.agent = "a1b2c3d4e5f6".into();
    // Its chain's own test is below: here, room for all of them.
    fake.way_in.hops = Some(100);
    let message = || Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: "agent-g7h8j9k0m1n2".into(),
        from: "agent:g7h8j9k0m1n2".into(),
        text: "Draft it.".into(),
        answer: AnswerTo::Nowhere,
        hops: 1,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
    };
    let refused = fake.runtime.block_on(fake.way_in.fire(message()));
    assert!(
        refused.unwrap_err().to_string().contains("isn't on a team"),
        "a stranger's message"
    );
    assert!(fake.asked.lock().unwrap().is_empty());
    std::fs::create_dir_all(fake.dir.join("teams")).unwrap();
    std::fs::write(
        fake.dir.join("teams/research.toml"),
        "schema = 1\nid = \"t4m9q2x7k1ab\"\nname = \"research\"\n[member.main]\nid = \"a1b2c3d4e5f6\"\n\
             service = \"service:s1s2s3s4s5s6\"\n[member.writer]\nid = \"g7h8j9k0m1n2\"\n\
             service = \"service:s7s8s9s0s1s2\"\n",
    )
    .unwrap();
    let answer = fake.runtime.block_on(fake.way_in.fire(message())).unwrap();
    assert_eq!(answer, "On Draft it.");
    assert_eq!(
        fake.asked.lock().unwrap().clone(),
        [asked(
            "agent-g7h8j9k0m1n2",
            "agent:g7h8j9k0m1n2",
            "1",
            "Draft it."
        )]
    );
    assert!(fake.sent().is_empty());
    // From itself: no teammate of its own.
    let own = Firing {
        from: "agent:a1b2c3d4e5f6".into(),
        ..message()
    };
    assert!(fake.runtime.block_on(fake.way_in.fire(own)).is_err());

    // A teammate's messages are taken as many times a day as this
    // agent's own cap on what it sends, and no more.
    fake.way_in.per_day = Some(2);
    assert!(fake.runtime.block_on(fake.way_in.fire(message())).is_ok());
    let past = fake.runtime.block_on(fake.way_in.fire(message()));
    assert!(
        past.unwrap_err()
            .to_string()
            .contains("as many as it takes"),
        "past the cap"
    );
    assert_eq!(fake.asked.lock().unwrap().len(), 2);
    // Tomorrow, it starts again.
    let path = fake.dir.join("received/g7h8j9k0m1n2.json");
    std::fs::write(&path, r#"{"day": 1, "taken": 2, "schema": 1}"#).unwrap();
    assert!(fake.runtime.block_on(fake.way_in.fire(message())).is_ok());
    let taken = || {
        let bytes = std::fs::read(&path).unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["taken"].clone()
    };
    assert_eq!(taken(), 1);
    // One the harness never took spends none of the day's.
    let mut down = Fake::way_in_at(&fake.dir, "http://127.0.0.1:1/v1/chat/completions");
    down.per_day = Some(2);
    assert!(fake.runtime.block_on(down.fire(message())).is_err());
    assert_eq!(taken(), 1, "a failed delivery counts nothing");
    // A count that can't be read is refused, never taken as none.
    std::fs::write(&path, "not a count").unwrap();
    assert!(fake.runtime.block_on(fake.way_in.fire(message())).is_err());
    assert_eq!(fake.asked.lock().unwrap().len(), 3);
}

/// Another agent's messages on a chat are taken up to the scheduler's
/// hops since a person last spoke there, as they are past their own hops:
/// a person speaking starts its chain again, and one the harness never
/// took counts nothing. A conversation only agents write in, where no
/// person speaks, is bounded by each message's own hops alone. Each
/// refusal is final ([`Why::Refused`]), so its sender never tries again.
#[test]
fn agents_messages_in_a_chat_stop_until_a_person_speaks() {
    use link_connection_contract::{AnswerTo, Firing};
    use link_sandbox_binding::{Failure, Why};
    let mut fake = Fake::new("chain", serde_json::json!([]));
    fake.way_in.endpoint.identity.agent = "a1b2c3d4e5f6".into();
    fake.way_in.terms.me.agent = "a1b2c3d4e5f6".into();
    fake.way_in.hops = Some(2);
    std::fs::create_dir_all(fake.dir.join("teams")).unwrap();
    std::fs::write(
        fake.dir.join("teams/research.toml"),
        "schema = 1\nid = \"t4m9q2x7k1ab\"\nname = \"research\"\n[member.main]\nid = \"a1b2c3d4e5f6\"\n\
             service = \"service:s1s2s3s4s5s6\"\n[member.writer]\nid = \"g7h8j9k0m1n2\"\n\
             service = \"service:s7s8s9s0s1s2\"\n",
    )
    .unwrap();
    // A teammate's answer lands on the chat its message was sent from.
    let chat = "telegram:42";
    let message = |thread: &str, hops: u32| Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: thread.into(),
        from: "agent:g7h8j9k0m1n2".into(),
        text: "Again.".into(),
        answer: AnswerTo::Nowhere,
        hops,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
    };
    let fire = |fake: &Fake, thread: &str, hops| {
        fake.runtime
            .block_on(fake.way_in.fire(message(thread, hops)))
    };
    let why = |err: anyhow::Error| err.downcast::<Failure>().unwrap().why;
    // Past its own hops: refused, whatever the chain.
    let far = fire(&fake, chat, 3).unwrap_err();
    assert!(far.to_string().contains("past the cap"), "{far}");
    assert_eq!(why(far), Why::Refused);
    assert!(fire(&fake, chat, 1).is_ok());
    assert!(fire(&fake, chat, 1).is_ok());
    let looping = fire(&fake, chat, 1).unwrap_err();
    assert!(
        looping.to_string().contains("until a person speaks"),
        "{looping}"
    );
    assert_eq!(why(looping), Why::Refused);
    assert_eq!(fake.asked.lock().unwrap().len(), 2);
    // The team's conversation has no person to end a chain: as many
    // messages within their own hops as come are taken.
    for _ in 0..3 {
        assert!(fire(&fake, "team-t4m9q2x7k1ab", 2).is_ok());
    }
    assert_eq!(fake.asked.lock().unwrap().len(), 5);
    // A person speaks there: the chain starts again.
    let chains = link_interactions::chains::Chains::at(&fake.dir.join("interactions"));
    chains.person_spoke("a1b2c3d4e5f6", chat).unwrap();
    assert!(fire(&fake, chat, 1).is_ok());
    // One the harness never took counts nothing, and isn't a refusal.
    let mut down = Fake::way_in_at(&fake.dir, "http://127.0.0.1:1/v1/chat/completions");
    down.terms.me.agent = "a1b2c3d4e5f6".into();
    down.hops = Some(2);
    let unreached = fake
        .runtime
        .block_on(down.fire(message(chat, 1)))
        .unwrap_err();
    assert!(
        unreached
            .downcast_ref::<Failure>()
            .is_none_or(|f| f.why != Why::Refused)
    );
    assert_eq!(chains.of("a1b2c3d4e5f6", chat).unwrap().agents, 1);
}

/// The way in passes the test every bridge passes.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn the_way_in_is_a_bridge() {
    use link_connection_contract::{AnswerTo, Firing};
    use link_sandbox_binding::Binding;
    use link_sandbox_binding::Identity;
    use link_sandbox_binding::Keyring;
    let fake = Fake::new("way-in-conformance", serde_json::json!([]));
    let keyring = Keyring::at(&fake.dir.join("keys"));
    let agents: Vec<Binding> = (1..=WAY_IN.calls / WAY_IN.per_agent + 1)
        .map(|n| {
            let identity = Identity {
                agent: format!("a1b2c3d4e5f{n}"),
                service: String::new(),
                owner: "me@box".into(),
                teams: Vec::new(),
            };
            keyring
                .bind(&identity, &format!("{n:0>32}").into())
                .unwrap()
        })
        .collect();
    let mut way_in = Fake::way_in_at(&fake.dir, &fake.endpoint);
    way_in.endpoint.identity = agents[0].identity.clone();
    way_in.terms.me = way_in.endpoint.identity();
    let limits = link_sandbox_bridge::conformance::brief(WAY_IN);
    let socket = fake.dir.join("way-in.sock");
    {
        let socket = socket.clone();
        let stop = AtomicBool::new(false);
        std::thread::spawn(move || way_in.listen_within(&socket, &stop, &limits));
    }
    while !socket.exists() {
        std::thread::sleep(Duration::from_millis(5));
    }
    let task = Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: format!("task-{}", agents[0].identity.agent),
        from: format!("task:{}", agents[0].identity.agent),
        text: "Look it up.".into(),
        answer: AnswerTo::Nowhere,
        hops: 1,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
    };
    let asked = fake.asked.clone();
    link_sandbox_bridge::conformance::check(&link_sandbox_bridge::conformance::Subject {
        socket,
        agents,
        action: serde_json::to_value(&task).unwrap(),
        ran: Arc::new(move || asked.lock().unwrap().len()),
        big: None,
        limits,
    });
}

/// The way in takes a call only as the agent its proof is of: its
/// own agent's any message; a teammate's only as itself, to its
/// conversation with this agent or a team's they share, answered back
/// only to it; nobody else's, and nothing once a key's unbound.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn the_way_in_takes_a_call_only_as_its_proof_says() {
    use link_connection_contract::{AnswerTo, Firing};
    use link_sandbox_binding::Binding;
    use link_sandbox_binding::Identity;
    use link_sandbox_binding::Keyring;
    let fake = Fake::new("way-in-proof", serde_json::json!([]));
    std::fs::create_dir_all(fake.dir.join("teams")).unwrap();
    std::fs::write(
        fake.dir.join("teams/research.toml"),
        "schema = 1\nid = \"t4m9q2x7k1ab\"\nname = \"research\"\n[member.main]\nid = \"a1b2c3d4e5f6\"\n\
             service = \"service:s1s2s3s4s5s6\"\n[member.writer]\nid = \"g7h8j9k0m1n2\"\n\
             service = \"service:s7s8s9s0s1s2\"\n",
    )
    .unwrap();
    let keyring = Keyring::at(&fake.dir.join("keys"));
    let who = |agent: &str| Identity {
        agent: agent.into(),
        service: String::new(),
        owner: "me@box".into(),
        teams: Vec::new(),
    };
    let me = keyring
        .bind(
            &who("a1b2c3d4e5f6"),
            &link_secretive::Secret::from("0123456789abcdef0123456789abcdef".to_string()),
        )
        .unwrap();
    let writer = keyring
        .bind(
            &who("g7h8j9k0m1n2"),
            &link_secretive::Secret::from("1123456789abcdef0123456789abcdef".to_string()),
        )
        .unwrap();
    let stranger = keyring
        .bind(
            &who("zzzzzzzzzzzz"),
            &link_secretive::Secret::from("2123456789abcdef0123456789abcdef".to_string()),
        )
        .unwrap();
    let mut way_in = Fake::way_in_at(&fake.dir, &fake.endpoint);
    way_in.endpoint.identity = me.identity.clone();
    way_in.terms.me = way_in.endpoint.identity();
    let socket = fake.dir.join("way-in.sock");
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (socket, stop) = (socket.clone(), stop.clone());
        std::thread::spawn(move || way_in.listen(&socket, &stop));
    }
    while !socket.exists() {
        std::thread::sleep(Duration::from_millis(5));
    }
    let fire = |as_: &Binding, firing: Firing| {
        link_connection_contract::fire(&socket, as_, &firing).map_err(|err| format!("{err:#}"))
    };
    let teammates = || Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: "agent-g7h8j9k0m1n2".into(),
        from: "agent:g7h8j9k0m1n2".into(),
        text: "Draft it.".into(),
        answer: AnswerTo::Nowhere,
        hops: 1,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
    };
    // Its own scheduler's subagent.
    let task = Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: "task-a1b2c3d4e5f6".into(),
        from: "task:a1b2c3d4e5f6".into(),
        text: "Look.".into(),
        answer: AnswerTo::Nowhere,
        hops: 1,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
    };
    assert_eq!(fire(&me, task.clone()).unwrap().answer, "On Look.");
    // A teammate, as itself, to its conversation or their team's.
    assert_eq!(fire(&writer, teammates()).unwrap().answer, "On Draft it.");
    let to_team = Firing {
        thread: "team-t4m9q2x7k1ab".into(),
        ..teammates()
    };
    assert!(fire(&writer, to_team).is_ok());
    let asked = fake.asked.lock().unwrap().len();
    // Into any other conversation, the owner's chat say: refused.
    let elsewhere = Firing {
        thread: "telegram:42".into(),
        ..teammates()
    };
    let refused = fire(&writer, elsewhere).unwrap_err();
    assert!(
        refused.contains("goes only to its conversation"),
        "{refused}"
    );
    // Its answer anywhere but back to it: refused.
    let answered = Firing {
        answer: AnswerTo::Chat,
        ..teammates()
    };
    assert!(
        fire(&writer, answered)
            .unwrap_err()
            .contains("answered only to it")
    );
    // As anyone but itself: refused, whatever the message says.
    let posing = Firing {
        from: "agent:zzzzzzzzzzzz".into(),
        ..teammates()
    };
    assert!(
        fire(&writer, posing)
            .unwrap_err()
            .contains("only as itself")
    );
    let scheduling = Firing {
        from: "schedule:a1b2c3d4e5f6".into(),
        ..teammates()
    };
    assert!(fire(&writer, scheduling).is_err());
    // A call naming another agent, proved with this one's key.
    let named_me = Binding {
        identity: me.identity.clone(),
        key: writer.key.duplicate(),
    };
    let refused = fire(&named_me, task.clone()).unwrap_err();
    assert!(
        refused.contains("proof isn't agent a1b2c3d4e5f6's"),
        "{refused}"
    );
    // Someone bound but on no team with it.
    let stranger_says = Firing {
        thread: "agent-zzzzzzzzzzzz".into(),
        from: "agent:zzzzzzzzzzzz".into(),
        ..teammates()
    };
    assert!(
        fire(&stranger, stranger_says)
            .unwrap_err()
            .contains("isn't on a team")
    );
    // None of those reached the harness.
    assert_eq!(fake.asked.lock().unwrap().len(), asked);
    // Unbound (stopped, or a unit that slept), its key proves nothing.
    keyring.unbind(
        &writer.identity.agent,
        &link_secret_contract::keeper::RouteRevoker,
    );
    assert!(
        fire(&writer, teammates())
            .unwrap_err()
            .contains("isn't bound here")
    );
    // Bound again for one request, with a key of its own: its message
    // taken while bound; once that binding goes, its key proves nothing.
    let bound = keyring
        .bind(
            &writer.identity,
            &link_secretive::Secret::from("c".repeat(64)),
        )
        .unwrap();
    assert!(fire(&bound, teammates()).is_ok());
    keyring.unbind_key(&bound, &link_secret_contract::keeper::RouteRevoker);
    assert!(
        fire(&bound, teammates())
            .unwrap_err()
            .contains("isn't bound here")
    );
    keyring.unbind(
        &me.identity.agent,
        &link_secret_contract::keeper::RouteRevoker,
    );
    assert!(fire(&me, task).unwrap_err().contains("isn't bound here"));
    stop.store(true, Ordering::Relaxed);
    let _ = std::os::unix::net::UnixStream::connect(&socket);
}
