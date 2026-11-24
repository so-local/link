//! Where each answer goes: with a subagent's task, once a hook's post is
//! taken, recovered, held for the next chat, or left to its own.

use super::*;

/// Explicit messages fail closed for unsupported destinations and preserve
/// connection refusals. A successful call delivers once, without a turn.
#[test]
fn owner_messages_use_connection_delivery_and_never_retry_a_refusal() {
    let fake = Fake::new("owner-send", serde_json::json!([]));
    for thread in ["task-a1b2c3d4e5f6", "missing:42", "telegram:"] {
        assert!(fake.way_in.send_owner(thread, "Ready").is_err());
    }
    assert!(fake.sent().is_empty());
    assert!(fake.way_in.send_owner("telegram:99", "Refuse").is_err());
    assert!(fake.sent().is_empty());
    assert_eq!(
        std::fs::read_to_string(fake.dir.join("attempts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    fake.way_in.send_owner("telegram:42", "Ready").unwrap();
    assert_eq!(fake.sent(), [("42".into(), "Ready".into())]);
    assert!(fake.asked.lock().unwrap().is_empty());
    fake.way_in.send_owner("link:main", "Held").unwrap();
    assert_eq!(fake.sent().len(), 1);
    assert_eq!(fake.way_in.held.take("link:main").unwrap(), ["Held"]);
}

/// A subagent's fewer tools reach its harness with its task, and its
/// answer, to the parent, carries none: the parent keeps its own. A
/// harness that can't keep them is sent nothing.
#[test]
fn a_subagents_tools_go_with_its_task() {
    use link_connection_contract::{AnswerTo, Firing};
    let mut fake = Fake::new("way-in-tools", serde_json::json!([]));
    let task = || Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: "task-a1b2c3d4e5f6".into(),
        from: "task:a1b2c3d4e5f6".into(),
        text: "Find a source.".into(),
        answer: AnswerTo::Thread("telegram:42".into()),
        hops: 1,
        tools: Some(vec!["memory__search".into(), "web__fetch".into()]),
        standing: link_connection_contract::access::Standing::Owner,
    };
    fake.runtime.block_on(fake.way_in.fire(task())).unwrap();
    // Its standing goes with it, the harness told the most it stands.
    assert_eq!(
        fake.stood.lock().unwrap()[0],
        (
            "task-a1b2c3d4e5f6".to_string(),
            Some(link_connection_contract::access::Standing::Owner)
        )
    );
    assert_eq!(
        fake.narrowed.lock().unwrap().clone(),
        [(
            "task-a1b2c3d4e5f6".to_string(),
            "memory__search,web__fetch".to_string()
        )]
    );
    assert_eq!(fake.asked.lock().unwrap().len(), 2);
    // None, for no tools at all.
    let none = Firing {
        tools: Some(Vec::new()),
        ..task()
    };
    fake.runtime.block_on(fake.way_in.fire(none)).unwrap();
    assert_eq!(fake.narrowed.lock().unwrap()[1].1, "");
    // A name no tool has: refused before the harness.
    let bad = Firing {
        tools: Some(vec!["web fetch".into()]),
        ..task()
    };
    assert!(fake.runtime.block_on(fake.way_in.fire(bad)).is_err());
    assert_eq!(fake.asked.lock().unwrap().len(), 4);
    // A harness that doesn't keep them: refused, never sent all.
    fake.way_in.terms.tools = false;
    let refused = fake.runtime.block_on(fake.way_in.fire(task()));
    assert!(
        format!("{:#}", refused.unwrap_err()).contains("fewer tools"),
        "refused for another reason"
    );
    assert_eq!(fake.asked.lock().unwrap().len(), 4);
    assert_eq!(fake.narrowed.lock().unwrap().len(), 2);
    // A schedule a member's conversation made stands as a member.
    let members = Firing {
        standing: link_connection_contract::access::Standing::Member,
        ..task()
    };
    fake.way_in.terms.tools = true;
    fake.runtime.block_on(fake.way_in.fire(members)).unwrap();
    let stood = fake.stood.lock().unwrap().clone();
    let task_said = stood
        .iter()
        .rev()
        .find(|(thread, _)| thread == "task-a1b2c3d4e5f6")
        .unwrap();
    assert_eq!(
        task_said.1,
        Some(link_connection_contract::access::Standing::Member),
        "{stood:?}"
    );
}

/// A hook's post is delivered once the harness has taken it, while
/// its turn still runs; the turn's answer is read after, and its
/// conversation then owes nothing.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_hooks_post_is_delivered_once_taken() {
    use link_connection_contract::acp::fake::{self, Turn};
    use link_connection_contract::{AnswerTo, Firing};
    let dir = link_test_kit::TempDir::new("way-in-taken");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    // A harness that takes the message at once and ends its turn only
    // when `end` says.
    let (end, ended) = tokio::sync::mpsc::channel::<()>(1);
    let ended = Arc::new(tokio::sync::Mutex::new(ended));
    let endpoint = runtime
        .block_on(fake::serve_script(
            move |_| {
                let ended = ended.clone();
                let rest = futures_util::stream::once(async move {
                    ended.lock().await.recv().await;
                    String::new()
                });
                Turn::Answer(Box::pin(futures_util::StreamExt::chain(
                    futures_util::stream::once(std::future::ready("On it".to_string())),
                    rest,
                )))
            },
            |_| Vec::new(),
            |_| {},
        ))
        .endpoint;
    let way_in = Arc::new(Fake::way_in_at(&dir, &endpoint));
    let owed = way_in.owed_path("webhook:deploys");
    let deliver = crate::pushed::delivering(way_in);
    let post = Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: "webhook:deploys".into(),
        from: "webhook:deploys".into(),
        text: "A deploy.".into(),
        answer: AnswerTo::Nowhere,
        hops: 0,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
    };
    runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(10), deliver(post))
            .await
            .expect("delivered before its turn ends")
            .unwrap();
        assert!(owed.exists(), "owed while its turn runs");
        end.send(()).await.unwrap();
        for _ in 0..200 {
            if !owed.exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("its turn's answer was never read");
    });
}

/// A stranger's message gets only what its spec gives strangers, and
/// none by default; a harness that can't narrow a conversation's tools
/// never takes one.
#[test]
fn a_stranger_gets_only_what_strangers_are_given() {
    use link_connection_contract::{AnswerTo, Firing};
    let given = vec!["web__fetch".to_string()];
    let terms = |strangers: &[String]| link_connection_contract::Terms {
        strangers: strangers.to_vec(),
        ..Default::default()
    };
    assert_eq!(terms(&given).strangers_tools(None), given);
    assert_eq!(terms(&[]).strangers_tools(None), Vec::<String>::new());
    assert_eq!(
        terms(&given).strangers_tools(Some(vec!["memory__search".into(), "web__fetch".into()])),
        given
    );
    let mut fake = Fake::new("way-in-strangers", serde_json::json!([]));
    let stranger = || Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: "webhook:github".into(),
        from: "webhook:github".into(),
        text: "A push.".into(),
        answer: AnswerTo::Nowhere,
        hops: 0,
        tools: None,
        standing: link_connection_contract::access::Standing::Owner,
    };
    // None by default: its conversation is narrowed to nothing.
    fake.runtime.block_on(fake.way_in.fire(stranger())).unwrap();
    fake.way_in.terms.strangers = given.clone();
    let asked_more = Firing {
        tools: Some(vec!["memory__search".into(), "web__fetch".into()]),
        ..stranger()
    };
    fake.runtime.block_on(fake.way_in.fire(asked_more)).unwrap();
    assert_eq!(
        fake.narrowed.lock().unwrap().clone(),
        [
            ("webhook:github".to_string(), String::new()),
            ("webhook:github".to_string(), "web__fetch".to_string())
        ]
    );
    // Another conversation, or an answer anywhere: never asked.
    for elsewhere in [
        Firing {
            thread: "telegram:42".into(),
            ..stranger()
        },
        Firing {
            thread: "task-a1b2c3d4e5f6".into(),
            ..stranger()
        },
        Firing {
            answer: AnswerTo::Chat,
            ..stranger()
        },
        Firing {
            answer: AnswerTo::Thread("telegram:42".into()),
            ..stranger()
        },
    ] {
        assert!(fake.runtime.block_on(fake.way_in.fire(elsewhere)).is_err());
    }
    assert_eq!(fake.asked.lock().unwrap().len(), 2);
    // A harness that can't narrow them: never asked.
    fake.way_in.terms.tools = false;
    let refused = fake.runtime.block_on(fake.way_in.fire(stranger()));
    assert!(
        format!("{:#}", refused.unwrap_err()).contains("can't narrow"),
        "refused for another reason"
    );
    assert_eq!(fake.asked.lock().unwrap().len(), 2);
}

/// An answer its harness finished after a crash goes where its turn's
/// was to go: kept from the firing that asked it, else its chat; the
/// harness is told each one that went, and keeps the rest.
#[test]
fn a_recovered_answer_goes_where_its_turn_was_to_go() {
    use link_connection_contract::AnswerTo;
    let fake = Fake::new(
        "recovered",
        serde_json::json!([
            {"thread": "telegram:42", "text": "Owner's.", "id": "1"},
            {"thread": "telegram:43", "text": "Kept.", "id": "2"},
            {"thread": "task-a1b2c3d4e5f6", "text": "Found.", "id": "3"},
            {"thread": "task-bbbbbbbbbbbb", "text": "Lost.", "id": "4"},
            {"thread": "telegram:99", "text": "Unsent.", "id": "5"},
            {"thread": "telegram:42", "text": "Once."},
        ]),
    );
    let way_in = &fake.way_in;
    // What the firings that crashed said.
    let owed = |answer: AnswerTo| Owed {
        from: "schedule:abcdef123456".into(),
        hops: 1,
        answer,
        standing: link_connection_contract::access::Standing::Owner,
        sent: false,
    };
    way_in.owe("telegram:43", &owed(AnswerTo::Nowhere)).unwrap();
    way_in
        .owe(
            "task-a1b2c3d4e5f6",
            &owed(AnswerTo::Thread("telegram:42".into())),
        )
        .unwrap();
    fake.runtime.block_on(way_in.recover()).unwrap();
    let mut sent = fake.wait_sent(3);
    // Independent parent continuations finish separately; routing and content stay exact.
    sent.sort();
    let mut expected = vec![
        ("42".into(), "Owner's.".into()),
        ("42".into(), "On Found.".into()),
        ("42".into(), "Once.".into()),
    ];
    expected.sort();
    assert_eq!(sent, expected);
    assert_eq!(
        fake.asked.lock().unwrap().clone(),
        [asked("telegram:42", "schedule:abcdef123456", "2", "Found.")]
    );
    let done = fake.done.lock().unwrap().clone();
    assert_eq!(done[0], serde_json::json!({"two_phase": true}));
    assert!(done.contains(&serde_json::json!({"done": ["1", "2", "3", "4"]})));
    // Delivered, a chat's record goes; a subagent's stays for its own.
    assert!(!way_in.owed_path("telegram:43").exists());
    assert!(way_in.owed_path("task-a1b2c3d4e5f6").exists());
}

/// A chat on `link` thread `thread` of the fake's agent.
fn chat_on(fake: &Fake, thread: &str) -> crate::chat::Chat {
    crate::chat::Chat::with(
        Fake::way_in_at(&fake.dir, &fake.endpoint),
        thread.into(),
        crate::chat::Held::at(&fake.dir.join("held.jsonl")),
        true,
    )
}

#[test]
fn owner_messages_and_scheduled_answers_choose_a_connection() {
    let mut fake = Fake::new("selected-connection", serde_json::json!([]));
    let output = fake.dir.join("selected.json");
    let program = fake.dir.join("lnk-selected");
    link_test_kit::write_program(&program, format!(
        "#!/bin/sh\ncase \"$1\" in\nowner) cat >/dev/null; printf '[\"42\"]\\n' ;;\nsend) cat > '{}'; printf '{{\"messages\":[\"one\"]}}\\n' ;;\n*) exit 1 ;;\nesac\n", output.display()
    )).unwrap();
    fake.way_in.connections.insert(
        "discord".into(),
        (program, link_connection_contract::Bridge::default()),
    );
    fake.way_in
        .send_connection("discord", "Hello there")
        .unwrap();
    let sent: serde_json::Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(sent["chat"], "42");
    assert_eq!(sent["text"], "Hello there");
    assert!(
        fake.way_in
            .send_connection("missing", "Never sent")
            .is_err()
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&output).unwrap()).unwrap(),
        sent
    );
    fake.runtime
        .block_on(fake.way_in.fire(link_connection_contract::Firing {
            parent: None,
            delivery: None,
            occurrence: None,
            thread: "link:scheduling".into(),
            from: "schedule:abcdef123456".into(),
            text: "a joke".into(),
            answer: link_connection_contract::AnswerTo::Connection("discord".into()),
            hops: 1,
            tools: None,
            model: None,
            standing: link_connection_contract::access::Standing::Owner,
        }))
        .unwrap();
    let sent: serde_json::Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(sent["text"], "On a joke");
    fake.way_in
        .send_connection("cli", "Your name is Mika")
        .unwrap();
    let cli = chat_on(&fake, "link:fresh-session");
    assert_eq!(
        fake.runtime.block_on(cli.handed()).unwrap(),
        ["Your name is Mika"]
    );
    assert!(fake.runtime.block_on(cli.handed()).unwrap().is_empty());
}

#[test]
fn a_schedule_that_already_sent_its_message_does_not_send_its_acknowledgement() {
    use link_connection_contract::{AnswerTo, Firing};
    let mut fake = Fake::new("schedule-sent-once", serde_json::json!([]));
    let output = fake.dir.join("messages.jsonl");
    let program = fake.dir.join("lnk-discord");
    link_test_kit::write_program(&program, format!(
        "#!/bin/sh\ncase \"$1\" in\nowner) cat >/dev/null; printf '[\"42\"]\\n' ;;\nsend) cat >> '{}'; printf '{{\"messages\":[\"one\"]}}\\n' ;;\n*) exit 1 ;;\nesac\n", output.display()
    )).unwrap();
    fake.way_in
        .connections
        .insert("discord".into(), (program.clone(), Default::default()));
    let firing = || Firing {
        thread: "link:reminder".into(),
        from: "schedule:abcdef123456".into(),
        text: "I sent the joke.".into(),
        answer: AnswerTo::Connection("discord".into()),
        ..Default::default()
    };
    let taken = fake.runtime.block_on(fake.way_in.take(firing())).unwrap();
    fake.way_in
        .send_connection("discord", "A penguin joke.")
        .unwrap();
    // Neither another schedule nor another destination can mark this firing.
    for (from, connection) in [
        ("schedule:bbbbbbbbbbbb", "discord"),
        ("schedule:abcdef123456", "telegram"),
    ] {
        fake.way_in
            .sent_in_schedule("link:reminder", from, connection)
            .unwrap();
        assert!(!fake.way_in.owed("link:reminder").sent);
    }
    fake.way_in
        .sent_in_schedule("link:reminder", "schedule:abcdef123456", "discord")
        .unwrap();
    // The marker survives rebuilding the bridge while the turn finishes.
    let mut rebuilt = Fake::way_in_at(&fake.dir, &fake.endpoint);
    rebuilt
        .connections
        .insert("discord".into(), (program, Default::default()));
    fake.runtime.block_on(rebuilt.finish(taken)).unwrap();
    let messages = || {
        std::fs::read_to_string(&output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(messages().len(), 1);
    assert_eq!(messages()[0]["text"], "A penguin joke.");
    // The next firing of the same recurring schedule owes a fresh answer.
    fake.runtime.block_on(fake.way_in.fire(firing())).unwrap();
    assert_eq!(messages().len(), 2);
    assert_eq!(messages()[1]["text"], "On I sent the joke.");
}

/// A schedule's answer in a `link` thread goes to no connection: it's
/// held, and handed over at the next chat on that thread alone.
#[test]
fn a_link_answer_nobody_waits_for_is_held_for_the_next_chat() {
    use link_connection_contract::{AnswerTo, Firing};
    let fake = Fake::new("chat-held", serde_json::json!([]));
    let answer = fake
        .runtime
        .block_on(fake.way_in.fire(Firing {
            thread: "link:main".into(),
            from: "schedule:abcdef123456".into(),
            text: "Remind me to stretch.".into(),
            answer: AnswerTo::Chat,
            ..Default::default()
        }))
        .unwrap();
    assert_eq!(answer, "On Remind me to stretch.");
    assert!(fake.sent().is_empty());
    let other = chat_on(&fake, "link:qa");
    assert!(fake.runtime.block_on(other.handed()).unwrap().is_empty());
    let chat = chat_on(&fake, "link:main");
    assert_eq!(
        fake.runtime.block_on(chat.handed()).unwrap(),
        ["On Remind me to stretch."]
    );
    assert!(fake.runtime.block_on(chat.handed()).unwrap().is_empty());
}

/// The bridge sends a connection's answer its harness kept to its chat,
/// leaves a `link` thread's kept, and holds one the harness doesn't
/// keep. A chat is handed its own thread's alone, leaving the rest
/// kept for the bridge, but for one the harness doesn't keep.
#[test]
fn a_chat_takes_its_threads_kept_answers_and_leaves_the_rest() {
    let fake = Fake::new(
        "chat-recovered",
        serde_json::json!([
            {"thread": "link:main", "text": "Mine.", "id": "1"},
            {"thread": "link:qa", "text": "Theirs.", "id": "2"},
            {"thread": "telegram:42", "text": "Owner's.", "id": "3"},
            {"thread": "link:other", "text": "Once."},
        ]),
    );
    fake.runtime.block_on(fake.way_in.recover()).unwrap();
    let done = fake.done.lock().unwrap().clone();
    assert!(done.contains(&serde_json::json!({"done": ["3"]})));
    let chat = chat_on(&fake, "link:main");
    assert_eq!(fake.runtime.block_on(chat.handed()).unwrap(), ["Mine."]);
    let done = fake.done.lock().unwrap().clone();
    assert!(done.contains(&serde_json::json!({"done": ["1"]})));
    // The connection's answer went once, by the bridge.
    assert_eq!(fake.sent(), [("42".into(), "Owner's.".into())]);
    let held = crate::chat::Held::at(&fake.dir.join("held.jsonl"));
    assert_eq!(held.take("link:other").unwrap(), ["Once.", "Once."]);
}

/// A conversation: what was held shows first, each line typed is
/// answered after a prompt, and the end of the lines ends it.
#[test]
fn a_conversation_answers_each_line() {
    let fake = Fake::new("chat-converse", serde_json::json!([]));
    crate::chat::Held::at(&fake.dir.join("held.jsonl"))
        .keep("link:main", "Stretch!")
        .unwrap();
    let chat = chat_on(&fake, "link:main");
    let (typed, mut lines) = tokio::sync::mpsc::channel(4);
    let (_interrupt, mut interrupts) = tokio::sync::mpsc::channel(1);
    typed.try_send("Hello".to_string()).unwrap();
    typed.try_send("  ".to_string()).unwrap();
    typed.try_send("Again".to_string()).unwrap();
    drop(typed);
    let mut out = Vec::new();
    fake.runtime
        .block_on(crate::chat::converse(
            &chat,
            None,
            &mut lines,
            &mut interrupts,
            &mut out,
        ))
        .unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "While you were away:\nStretch!\n\n> On Hello\n> > On Again\n> \n"
    );
    let asked: Vec<String> = fake
        .asked
        .lock()
        .unwrap()
        .iter()
        .map(|a| a.0.clone())
        .collect();
    assert_eq!(asked, ["link:main", "link:main"]);
}

/// A conversation opened with a first message shows it after the first
/// prompt, as though typed, and answers it before any line typed.
#[test]
fn a_conversation_opens_with_its_first_message() {
    let fake = Fake::new("chat-seeded", serde_json::json!([]));
    let chat = chat_on(&fake, "link:main");
    let (typed, mut lines) = tokio::sync::mpsc::channel(4);
    let (_interrupt, mut interrupts) = tokio::sync::mpsc::channel(1);
    typed.try_send("Again".to_string()).unwrap();
    drop(typed);
    let mut out = Vec::new();
    fake.runtime
        .block_on(crate::chat::converse(
            &chat,
            Some("What can you do?".to_string()),
            &mut lines,
            &mut interrupts,
            &mut out,
        ))
        .unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "> What can you do?\nOn What can you do?\n> On Again\n> \n"
    );
}

/// A harness that streams `slow` without end, joins a message sent
/// meanwhile to that turn, and keeps the turn's answer for a chat
/// once it's asked for (its caller went): `Both.`, as id 9.
fn slow_harness(
    runtime: &tokio::runtime::Runtime,
    kept: Vec<(&'static str, &'static str)>,
) -> String {
    use link_connection_contract::acp::fake::{self, Turn};
    let kept = std::sync::Arc::new(std::sync::Mutex::new(kept));
    let joined = kept.clone();
    runtime
        .block_on(fake::serve_script(
            move |asked| {
                if asked.text != "slow" {
                    joined.lock().unwrap().push(("9", "Both."));
                    return Turn::Steered;
                }
                // The first piece, then nothing: a turn going on.
                Turn::Answer(Box::pin(futures_util::StreamExt::chain(
                    futures_util::stream::once(std::future::ready("Thinking".to_string())),
                    futures_util::stream::pending(),
                )))
            },
            move |asked| {
                let mut kept = kept.lock().unwrap();
                kept.retain(|(id, _)| !asked.done.iter().any(|done| done == id));
                kept.iter()
                    .map(|(id, text)| link_connection_contract::Recovered {
                        thread: "link:main".into(),
                        text: text.to_string(),
                        id: Some(id.to_string()),
                    })
                    .collect()
            },
            |_| {},
        ))
        .endpoint
}

/// Ctrl-C stops waiting for an answer, as much of it shown as came;
/// the next message, joining the turn still running, is answered by
/// that turn's answer once its harness keeps it.
#[test]
fn ctrl_c_stops_waiting_and_the_next_message_is_answered() {
    let fake = Fake::new("chat-stop", serde_json::json!([]));
    // An answer kept before: not the joined turn's.
    let endpoint = slow_harness(&fake.runtime, vec![("8", "Old.")]);
    let chat = crate::chat::Chat::with(
        Fake::way_in_at(&fake.dir, &endpoint),
        "link:main".into(),
        crate::chat::Held::at(&fake.dir.join("held.jsonl")),
        true,
    );
    let mut out = link_test_kit::io::ObservedOutput::default();
    let written = out.written();
    let said = fake
        .runtime
        .block_on(async {
            let stop = link_test_kit::eventually_async(
                "the first streamed fragment is displayed",
                std::time::Duration::from_secs(10),
                || *written.borrow() > 0,
            );
            chat.turn("slow", &mut out, stop).await
        })
        .unwrap();
    assert_eq!(said, crate::chat::Said::Stopped);
    assert_eq!(String::from_utf8(out.into_bytes()).unwrap(), "Thinking\n");
    let mut out = Vec::new();
    let said = fake
        .runtime
        .block_on(chat.turn("Next", &mut out, std::future::pending()))
        .unwrap();
    assert_eq!(said, crate::chat::Said::Answered("Both.".into()));
    assert_eq!(String::from_utf8(out).unwrap(), "Both.\n");
    // The one kept before comes at the next chat, once.
    assert_eq!(fake.runtime.block_on(chat.handed()).unwrap(), ["Old."]);
    assert!(fake.runtime.block_on(chat.handed()).unwrap().is_empty());
}

/// A `link` thread's answer kept for somewhere other than its chat (a
/// firing whose answer went nowhere) is the bridge's to send, never a
/// chat's to show.
#[test]
fn a_chat_leaves_an_answer_not_owed_to_it() {
    use link_connection_contract::AnswerTo;
    let fake = Fake::new(
        "chat-not-owed",
        serde_json::json!([{"thread": "link:main", "text": "Nobody's.", "id": "1"}]),
    );
    let owed = Owed {
        from: "schedule:abcdef123456".into(),
        hops: 1,
        answer: AnswerTo::Nowhere,
        standing: link_connection_contract::access::Standing::Owner,
        sent: false,
    };
    fake.way_in.owe("link:main", &owed).unwrap();
    let chat = chat_on(&fake, "link:main");
    assert!(fake.runtime.block_on(chat.handed()).unwrap().is_empty());
    fake.runtime.block_on(fake.way_in.recover()).unwrap();
    let done = fake.done.lock().unwrap().clone();
    assert_eq!(done.last().unwrap(), &serde_json::json!({"done": ["1"]}));
}
