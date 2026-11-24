//! A turn calling tools: what it searches and carries on from, the
//! tools a conversation is narrowed to, how many rounds and calls it
//! makes, and what its measures keep.

use super::*;

#[tokio::test]
async fn it_searches_memory_and_carries_on_from_it() {
    for sse in [false, true] {
        let (url, seen) = fake_tool(sse).await;
        let world = world(&format!("memory-{sse}"), false, memory(&url)).await;
        let answer = ask(
            &world.endpoint,
            "telegram:42",
            "What did I say about driving?",
        )
        .await;
        assert_eq!(
            answer,
            "Let me look.\n\nFound it. chats/telegram-42: you'd drive to Albuquerque on \
             Friday."
        );

        // The call went to the tool by its own name, once, in a session.
        {
            let seen = seen.lock().unwrap();
            // With the tool's own opinions, its table in the agent's
            // spec (Link's here), and only its own.
            let mut calls = seen.calls.clone();
            let spec = calls[0]["_meta"]
                .as_object_mut()
                .unwrap()
                .remove("link.local/spec")
                .unwrap();
            // And which attempt and call it is, the attempt's id made fresh.
            let meta = calls[0]["_meta"].as_object_mut().unwrap();
            assert!(meta.remove("link.local/attempt").unwrap().is_string());
            assert_eq!(meta.remove("link.local/call").unwrap(), "call_2");
            assert_eq!(spec["tokens"], 6000);
            assert_eq!(spec["answer"], "32 KB");
            assert!(spec.get("fetch").is_none(), "{spec}");
            // With where it's made: the conversation, the hops of the
            // message being answered (the owner's: none), and the owner's
            // standing.
            assert_eq!(
                calls,
                [
                    json!({"name": "search", "arguments": {"query": "Albuquerque"},
                    "_meta": {"link.local/thread": "telegram:42", "link.local/hops": 0,
                        "link.local/tools": ["memory__search"], "link.local/standing": "owner",
                        "link.local/model": "fake"}})
                ]
            );
            assert_eq!(seen.sessions, ["s1"]);
            assert!(seen.ended.is_empty(), "kept for the next turn");
        }

        // Each round is an event: the owner, the call, its answer, the
        // answer after it.
        let events = world.conversation_events("telegram-42");
        assert_eq!(events.len(), 4);
        let call = &events[1];
        assert_eq!(call["role"], "assistant");
        assert_eq!(call["text"], "Let me look.");
        assert_eq!(call["calls"][0]["id"], "call_2");
        assert_eq!(call["calls"][0]["name"], "memory__search");
        assert_eq!(
            call["calls"][0]["arguments"],
            json!({"query": "Albuquerque"})
        );
        assert_eq!(call["tokens"]["input"], 30);
        let answered = &events[2];
        assert_eq!(answered["role"], "tool");
        assert_eq!(answered["call"], "call_2");
        assert_eq!(answered["tool"], "memory__search");
        assert!(answered["text"].as_str().unwrap().contains("Albuquerque"));
        assert!(answered.get("failed").is_none());
        assert!(answered["latency_ms"].is_u64());
        assert_eq!(events[3]["role"], "assistant");
        let md =
            std::fs::read_to_string(world.root.join("files/chats/telegram-42/conversation.md"))
                .unwrap();
        // The copy is what was said: memory mustn't find its own answers.
        assert!(!md.contains("memory__search"), "{md}");
        assert!(!md.contains("## Tool"), "{md}");

        // The model was offered the tool, once, by the server's name
        // before its own, and read the answer in the next round, after
        // the same start: the cache holds within a turn.
        let asked = world.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(asked[0]["tools"][0]["function"]["name"], "memory__search");
        assert_eq!(
            asked[0]["tools"][0]["function"]["description"],
            "Search your files and conversations."
        );
        assert_eq!(asked[0]["tools"], asked[1]["tools"]);
        let second = asked[1]["messages"].as_array().unwrap();
        assert_eq!(asked[0]["messages"].as_array().unwrap()[..], second[..2]);
        assert_eq!(second[2]["tool_calls"][0]["id"], "call_2");
        assert_eq!(second[3]["role"], "tool");
        assert_eq!(second[3]["tool_call_id"], "call_2");

        // The next turn calls in the same session, so the tool's process
        // isn't started again; the calls and answers are read back from
        // the events. The tools are as they were listed.
        ask(&world.endpoint, "telegram:42", "And when?").await;
        assert_eq!(seen.lock().unwrap().sessions, ["s1"]);
        assert!(seen.lock().unwrap().ended.is_empty());
        assert_eq!(seen.lock().unwrap().lists, 1);
        // Idle, it ends, and its process with it; not before.
        world.server.harness.end_idle_tools_within(IDLE, AGE).await;
        assert_eq!(world.server.harness.tool_sessions(), 1);
        world
            .server
            .harness
            .end_idle_tools_within(std::time::Duration::ZERO, AGE)
            .await;
        assert_eq!(world.server.harness.tool_sessions(), 0);
        assert_eq!(seen.lock().unwrap().ended, ["s1"]);
        let asked = world.asked.lock().unwrap().clone();
        let third = asked[2]["messages"].as_array().unwrap();
        assert_eq!(second[..], third[..second.len()]);
        assert_eq!(world.conversation_events("telegram-42").len(), 8);
    }
}

/// A kept session that has served long enough ends though turns keep
/// using it, and the next turn starts another: what memory started
/// without (its model, not up yet) is tried again.
/// A conversation narrowed to some tools (a subagent's) is offered only
/// those from then on, and says so to the tools it calls: a later list
/// narrows it again and never widens it, and no other conversation is
/// narrowed with it.
#[tokio::test(flavor = "multi_thread")]
async fn a_narrowed_conversation_has_only_its_tools() {
    let (url, seen) = fake_tool(false).await;
    let world = world("narrowed", false, memory(&url)).await;
    let offered = |world: &World| {
        let asked = world.asked.lock().unwrap();
        asked.last().unwrap()["tools"]
            .as_array()
            .map(|tools| tools.len())
            .unwrap_or(0)
    };
    // What the tool says of itself is in the prompt only while its tools
    // are offered.
    let told = |world: &World| {
        let asked = world.asked.lock().unwrap();
        asked.last().unwrap()["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains(
                "# Your tools\n\nWhat each of your tools says of itself:\n\n## memory\n\n\
                 Search before you say you don't know.",
            )
    };
    let search = vec!["memory__search".to_string(), "web__fetch".to_string()];
    let thread = "task-aaaaaaaaaaaa";
    let answer = ask_with(
        &world.endpoint,
        thread,
        "task:aaaaaaaaaaaa",
        "Find it.",
        Some(&search),
    )
    .await;
    assert!(answer.contains("Found it."), "{answer}");
    assert!(told(&world));
    assert_eq!(
        seen.lock().unwrap().calls[0]["_meta"]["link.local/tools"],
        json!(["memory__search"])
    );
    let kept = world
        .root
        .join("files/chats/task-aaaaaaaaaaaa")
        .join(link_harness::store::TOOLS);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&kept).unwrap()).unwrap(),
        json!(["memory__search", "web__fetch"])
    );
    // Narrowed to none, then given more, then nothing: none, each time.
    // Each from a task of its own: one task's second message is its retry.
    for (from, tools) in [
        ("task:bbbbbbbbbbbb", Some(Vec::new())),
        ("task:cccccccccccc", Some(search.clone())),
        ("task:dddddddddddd", None),
    ] {
        let answer = ask_with(&world.endpoint, thread, from, "Again.", tools.as_deref()).await;
        assert_eq!(answer, "No tools here.", "{tools:?}");
        assert_eq!(offered(&world), 0, "{tools:?}");
        assert!(!told(&world), "{tools:?}");
    }
    assert_eq!(seen.lock().unwrap().calls.len(), 1);
    // Another conversation keeps every tool.
    let answer = ask(&world.endpoint, "telegram:42", "Find it.").await;
    assert!(answer.contains("Found it."), "{answer}");
    // A list that isn't one: refused before the model.
    let asked = world.asked.lock().unwrap().len();
    let hi = vec![
        link_agent_state_protocol::acp::schema::v1::ContentBlock::Text(
            link_agent_state_protocol::acp::schema::v1::TextContent::new("Hi."),
        ),
    ];
    let meta = json!({"link": {"tools": ["not a tool"]}});
    let refused = common::prompt_with(&world.endpoint, "test-key", thread, hi, meta).await;
    assert!(refused.unwrap_err().contains("tools"));
    assert_eq!(world.asked.lock().unwrap().len(), asked);
}

#[tokio::test]
async fn a_turn_calls_at_most_so_many_rounds() {
    let (url, seen) = fake_tool(false).await;
    let world = world("rounds", true, memory(&url)).await;
    let answer = ask(&world.endpoint, "telegram:42", "Search forever").await;
    assert!(answer.ends_with("on Friday."), "{answer}");
    let asked = world.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), rounds() + 1);
    assert_eq!(asked.last().unwrap()["tool_choice"], "none");
    assert!(
        asked[..rounds()]
            .iter()
            .all(|a| a.get("tool_choice").is_none())
    );
    assert_eq!(seen.lock().unwrap().calls.len(), rounds());
    // The owner, then a call and its answer each round, then the answer.
    assert_eq!(
        world.conversation_events("telegram-42").len(),
        2 + 2 * rounds()
    );
}

/// A model making hundreds of calls at once gets the first few made and
/// kept, never the rest.
#[tokio::test]
async fn an_answer_makes_at_most_so_many_calls() {
    let (url, seen) = fake_tool(false).await;
    let world = world_calling("calls", false, 300, memory(&url)).await;
    let answer = ask(&world.endpoint, "telegram:42", "Search everything").await;
    assert!(answer.ends_with("on Friday."), "{answer}");
    assert_eq!(seen.lock().unwrap().calls.len(), calls());
    // The owner, the answer with its calls, their answers, the answer.
    assert_eq!(world.conversation_events("telegram-42").len(), 3 + calls());
}

/// Tools taken away: the conversation's calls and answers go to the
/// model as text, which takes them without any tool offered.
#[tokio::test]
async fn calls_read_as_text_once_the_tools_are_gone() {
    let (url, _) = fake_tool(false).await;
    let world = world("away", false, memory(&url)).await;
    ask(
        &world.endpoint,
        "telegram:42",
        "What did I say about driving?",
    )
    .await;

    // Another process takes the next turn, with no tools.
    let without = serve(&world.root, Vec::new()).await;
    assert_eq!(ask(&without, "telegram:42", "And?").await, "No tools here.");
    let asked = world.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 3);
    let last = &asked[2];
    assert!(last.get("tools").is_none());
    let messages = last["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .all(|m| m["role"] != "tool" && m.get("tool_calls").is_none())
    );
    assert!(
        messages.iter().any(|m| m["content"]
            .as_str()
            .unwrap_or_default()
            .starts_with("(memory__search answered, not your owner:)\n> ")),
        "{messages:?}"
    );
}

/// A turn's measures are numbers only: none of its words, the owner's,
/// the model's, a tool call's arguments or the tool's answer, is in what
/// it reports (`LNK_MEASURES`).
#[tokio::test]
async fn a_turns_measures_hold_none_of_its_words() {
    let (url, _seen) = fake_tool(false).await;
    let world = world("measures", false, memory(&url)).await;
    let path = world.root.join("home/.link/measures.jsonl");
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        world.root.join("home"),
        common::flat(world.root.join("files")),
    );
    agents.allow_tools(&agent, tools::from_env(memory(&url)));
    let store = Arc::new(Files::new(agents.clone()));
    let server = Arc::new(Server::new(
        Harness::new(agents, store)
            .unwrap()
            .with_measures(path.clone()),
        agent,
    ));
    let port = common::serve(server::router(server)).await;
    let endpoint = common::acp(port);

    let phrase = "the purple walrus sings at noon";
    let answer = ask(&endpoint, "telegram:42", &format!("Did {phrase}?")).await;
    assert!(answer.contains("Albuquerque"), "{answer}");
    // Written after the answer, off its way.
    let request = common::measures(&path, 1).await.remove(0);
    let text = request.to_string();
    let points: Vec<(String, f64)> = link_measure_contract::client::read_otlp(&request)
        .into_iter()
        .flat_map(|r| r.points)
        .map(|p| (p.measure.name.to_string(), p.value))
        .collect();
    let value = |name: &str| points.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
    assert_eq!(value("link.turns.sum"), Some(1.0));
    assert_eq!(value("link.tool_calls.sum"), Some(1.0));
    // Two rounds of the model's, 30 and 5 tokens each.
    assert_eq!(value("link.tokens.input.sum"), Some(60.0));
    assert_eq!(value("link.tokens.output.sum"), Some(10.0));
    let (to_model, turn) = (
        value("link.time.to_model.own").unwrap(),
        value("link.time.turn.own").unwrap(),
    );
    let first = value("link.time.first_token.own").unwrap();
    assert!(to_model <= first && first <= turn, "{points:?}");
    for word in [
        "walrus",
        "purple",
        "Albuquerque",
        "Friday",
        "Let me look",
        "Found it",
        "telegram",
        "query",
    ] {
        assert!(!text.contains(word), "the measures hold {word:?}: {text}");
    }
}

/// A connection shows each tool called above the answer being written,
/// with what it works on, as its spec says: gone from the finished
/// answer (`replace`), kept above it (`keep`), or never (`off`).
#[tokio::test]
async fn a_connection_shows_the_tools_called_as_its_spec_says() {
    use link_connection_contract::policy::Activity;
    let (url, _) = fake_tool(false).await;
    let world = world("activity", false, memory(&url)).await;
    for (n, activity) in [Activity::Replace, Activity::Keep, Activity::Off]
        .into_iter()
        .enumerate()
    {
        let bridge = link_connection_contract::Bridge {
            agent: "main".into(),
            endpoint: world.endpoint.clone(),
            key: "test-key".to_string().into(),
            spec: Some(link_connection_contract::policy::Connection {
                activity: Some(activity),
                activity_mark: Some("›".into()),
                activity_separator: Some(": ".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (progress, _) = tokio::sync::watch::channel(String::new());
        let thread = format!("telegram:{n}");
        let answer = link_connection_contract::ask(&bridge, &thread, "Driving?", &progress)
            .await
            .unwrap();
        let shown = progress.borrow().clone();
        let line = "› memory search: Albuquerque";
        match activity {
            Activity::Replace => {
                assert!(answer.starts_with("Let me look."), "{answer}");
                assert_eq!(shown, format!("{line}\n\n{answer}"));
            }
            Activity::Keep => {
                assert!(
                    answer.starts_with(&format!("{line}\n\nLet me look.")),
                    "{answer}"
                );
                assert_eq!(shown, answer);
            }
            Activity::Off => {
                assert!(answer.starts_with("Let me look."), "{answer}");
                assert_eq!(shown, answer);
            }
        }
    }
}
