//! An agent on a harness that speaks no connection answering on Signal, end to end: `lnk agent
//! start` runs the mini harness and, beside it, the Signal module carrying the
//! messages (`lnk-signal bridge`), against a fake `signal-cli` (in
//! `LNK_SIGNAL_CLI`) and a bare chat completions model. The fake passes
//! its stdin and stdout to this test over TCP, so the test answers
//! JSON-RPC as `signal-cli jsonRpc` does.

#![cfg(unix)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use link_test_kit::TempDir;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

use link_harness_conformance::agent::{self, Agent, Shared};

/// An agent on the mini harness, its programs the core's (built in
/// `link-core` beside this folder) and this folder's `lnk-signal`, started
/// with `prepare` given its home.
async fn start_with(
    name: &str,
    sandboxed: bool,
    section: &str,
    env: &[(&str, String)],
    prepare: impl FnOnce(&std::path::Path, &mut agent::Block),
) -> Agent {
    let programs = agent::programs_beside(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        std::path::Path::new(env!("CARGO_BIN_EXE_lnk-signal")),
    );
    agent::start_fixture(&programs, name, sandboxed, section, env, prepare).await
}

/// The agent's number, and its owner's.
const AGENT: &str = "+15557654321";
const OWNER: &str = "+15551234567";
/// A group the agent is in, as signal-cli names it.
const GROUP: &str = "RnJpZW5kcw==";

/// What the fake `signal-cli` holds: notifications to hand out, and what
/// the bridge did.
#[derive(Default)]
struct Fake {
    /// Each run's arguments.
    runs: Vec<String>,
    notifications: Vec<Value>,
    /// How many notifications were handed out.
    handed: usize,
    /// The requests: their method and params.
    requests: Vec<(String, Value)>,
}

/// Runs of the fake, each one connection.
async fn accept(fake: Shared<Fake>, listener: TcpListener) {
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(run(fake.clone(), stream));
    }
}

/// One run: its arguments first, then JSON-RPC, one object per line.
async fn run(fake: Shared<Fake>, stream: TcpStream) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    loop {
        let next = {
            let mut fake = fake.lock().unwrap();
            let next = fake.notifications.get(fake.handed).cloned();
            if next.is_some() {
                fake.handed += 1;
            }
            next
        };
        if let Some(notification) = next {
            if write
                .write_all(format!("{notification}\n").as_bytes())
                .await
                .is_err()
            {
                return;
            }
            continue;
        }
        let line = match tokio::time::timeout(Duration::from_millis(50), lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None) | Err(_)) => return,
            Err(_) => continue,
        };
        if let Some(args) = line.strip_prefix("ARGS ") {
            fake.lock().unwrap().runs.push(args.to_string());
            continue;
        }
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["jsonrpc"], "2.0", "{request}");
        let method = request["method"].as_str().unwrap().to_string();
        let result = match method.as_str() {
            "send" => json!({"timestamp": 1_800_000_000_000u64, "results": []}),
            _ => json!({}),
        };
        fake.lock()
            .unwrap()
            .requests
            .push((method, request["params"].clone()));
        let reply = json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
        if write
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .is_err()
        {
            return;
        }
    }
}

/// A `receive` notification of an envelope from `from`.
fn received(from: &str, timestamp: u64, rest: Value) -> Value {
    let mut envelope = json!({"source": from, "sourceNumber": from, "sourceUuid": "u-1",
        "sourceName": "Dana", "sourceDevice": 1, "timestamp": timestamp});
    for (key, value) in rest.as_object().unwrap() {
        envelope[key] = value.clone();
    }
    json!({"jsonrpc": "2.0", "method": "receive",
        "params": {"envelope": envelope, "account": AGENT}})
}

/// A direct text message from `from`.
fn text(from: &str, timestamp: u64, message: &str) -> Value {
    received(
        from,
        timestamp,
        json!({"dataMessage": {"timestamp": timestamp, "message": message,
            "expiresInSeconds": 0, "viewOnce": false}}),
    )
}

/// The messages sent to the owner so far.
fn sent(fake: &Shared<Fake>) -> Vec<String> {
    let fake = fake.lock().unwrap();
    fake.requests
        .iter()
        .filter(|(method, params)| method == "send" && params["recipient"] == json!([OWNER]))
        .map(|(_, params)| params["message"].as_str().unwrap().to_string())
        .collect()
}

/// Waits until what was sent ends with `ending`.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn sent_on_signal(fake: &Shared<Fake>, ending: &[&str], agent: &mut Agent) {
    let start = Instant::now();
    loop {
        let sent = sent(fake);
        if sent.ends_with(&ending.iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
            return;
        }
        let head: Vec<String> = sent.iter().map(|s| s.chars().take(80).collect()).collect();
        agent.check(start, &format!("waiting for {ending:.80?}; sent {head:?}"));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Repeated active indicators refresh Signal's typing state. Stopping
/// or sending twice, or restarting typing after either, stays visible.
fn typing_transitions(mut steps: Vec<String>) -> Vec<String> {
    steps.dedup_by(|next, previous| {
        next == previous && previous.as_str() == "sendTyping stop=false"
    });
    steps
}

#[test]
fn typing_refreshes_preserve_stop_and_single_delivery_checks() {
    let expected = ["sendTyping stop=false", "sendTyping stop=true", "send"];
    for refreshes in [1, 2, 300] {
        let steps = std::iter::repeat_n("sendTyping stop=false", refreshes)
            .chain(["sendTyping stop=true", "send"])
            .map(str::to_string)
            .collect();
        assert_eq!(typing_transitions(steps), expected);
    }
    for invalid in [
        vec!["sendTyping stop=true", "send"],
        vec![
            "sendTyping stop=false",
            "sendTyping stop=true",
            "sendTyping stop=true",
            "send",
        ],
        vec![
            "sendTyping stop=false",
            "sendTyping stop=true",
            "send",
            "send",
        ],
        vec![
            "sendTyping stop=false",
            "sendTyping stop=true",
            "sendTyping stop=false",
            "send",
        ],
    ] {
        assert_ne!(
            typing_transitions(invalid.into_iter().map(str::to_string).collect()),
            expected
        );
    }
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn talk_on_signal(name: &str, sandboxed: bool) {
    let fake: Shared<Fake> = Arc::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(accept(fake.clone(), listener));
    // The fake: its stdin and stdout are this test's connection.
    let dir = TempDir::new(&format!("{name}-fake"));
    let program = dir.join("signal-cli");
    link_test_kit::write_program(
        &program,
        format!(
            "#!/bin/bash\nexec 3<>/dev/tcp/127.0.0.1/{port}\nprintf 'ARGS %s\\n' \"$*\" >&3\n\
             cat <&3 &\nexec cat >&3\n"
        ),
    )
    .unwrap();

    let section = agent::connection(
        "signal",
        &[],
        &format!("account = \"{AGENT}\"\nallowed_users = [\"{OWNER}\"]"),
        &format!("id = \"{AGENT}\""),
    );
    let state = |home: &std::path::Path| home.join(".lnk/safe/agents/main/connections/signal");
    // Linked already, as `lnk-signal link` leaves it.
    let link = |home: &std::path::Path| {
        let accounts = state(home).join(".signal-cli/data/data");
        std::fs::create_dir_all(&accounts).unwrap();
        let linked = json!({"accounts": [{"number": AGENT, "path": "123456"}], "version": 2});
        std::fs::write(accounts.join("accounts.json"), linked.to_string()).unwrap();
    };
    let env = [("LNK_SIGNAL_CLI", program.display().to_string())];
    let mut agent = start_with(name, sandboxed, &section, &env, |home, _| link(home)).await;

    // A stranger, the owner in a group, a copy of what the agent sent
    // from its phone, and a receipt; then the owner says hello, and it
    // comes again.
    let mut group = text(OWNER, 1002, "Hello, group");
    group["params"]["envelope"]["dataMessage"]["groupInfo"] =
        json!({"groupId": "Z3JvdXA=", "type": "DELIVER"});
    let sync = received(
        AGENT,
        1003,
        json!({"syncMessage": {"sentMessage": {"destinationNumber": OWNER,
            "timestamp": 1003, "message": "Sent from the phone"}}}),
    );
    let receipt = received(
        OWNER,
        1004,
        json!({"receiptMessage": {"when": 1004, "isDelivery": true, "timestamps": [1]}}),
    );
    // A stranger's mention of the agent in a group, and the owner's, which
    // comes again.
    let mention = |from: &str, timestamp: u64| {
        let mut mention = text(from, timestamp, "\u{fffc} hello");
        let data = &mut mention["params"]["envelope"]["dataMessage"];
        data["groupInfo"] = json!({"groupId": GROUP, "groupName": "Friends", "revision": 1,
            "type": "DELIVER"});
        data["mentions"] = json!([{"name": AGENT, "number": AGENT, "uuid": "a-1",
            "start": 0, "length": 1}]);
        mention
    };
    fake.lock().unwrap().notifications.extend([
        text("+15550000000", 1001, "I'm not the owner"),
        group,
        sync,
        receipt,
        mention("+15550000000", 1010),
        mention(OWNER, 1011),
        mention(OWNER, 1011),
        text(OWNER, 1005, "Hello"),
        text(OWNER, 1005, "Hello"),
    ]);
    sent_on_signal(&fake, &["I see 1 messages; the last: Hello"], &mut agent).await;
    // The group's answer, streamed beside the owner's, may end after it.
    let start = Instant::now();
    while !fake
        .lock()
        .unwrap()
        .requests
        .iter()
        .any(|(method, params)| method == "send" && params["groupId"] == GROUP)
    {
        agent.check(start, "waiting for the group's answer");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    {
        let fake = fake.lock().unwrap();
        // Nothing goes to anyone else.
        let elsewhere: Vec<&(String, Value)> = fake
            .requests
            .iter()
            .filter(|(_, params)| {
                params["recipient"] != json!([OWNER]) && params["groupId"] != GROUP
            })
            .collect();
        assert!(
            elsewhere.is_empty(),
            "someone else was answered: {elsewhere:?}"
        );
        // The owner's mention is answered once, in the group: "typing",
        // stopped, then the whole answer.
        let group: Vec<String> = fake
            .requests
            .iter()
            .filter(|(_, params)| params["groupId"] == GROUP)
            .map(|(method, params)| match method.as_str() {
                "sendTyping" => format!("sendTyping stop={}", params["stop"]),
                _ => format!("{method} {}", params["message"].as_str().unwrap()),
            })
            .collect();
        assert_eq!(
            typing_transitions(group),
            [
                "sendTyping stop=false",
                "sendTyping stop=true",
                "send I see 1 messages; the last: hello"
            ],
            "{:?}",
            fake.requests
        );
        // Its own signal-cli, with the account in the agent's folder for
        // Signal.
        let run = &fake.runs[0];
        let data = state(&agent.home).join(".signal-cli/data");
        assert_eq!(
            run,
            &format!("--config {} -a {AGENT} jsonRpc", data.display())
        );
        // "Typing" while it's written, stopped, then the whole answer:
        // nothing edited.
        let steps: Vec<String> = fake
            .requests
            .iter()
            .filter(|(_, params)| params["recipient"] == json!([OWNER]))
            .map(|(method, params)| match method.as_str() {
                "sendTyping" => format!("sendTyping stop={}", params["stop"]),
                _ => method.clone(),
            })
            .collect();
        assert_eq!(
            typing_transitions(steps),
            ["sendTyping stop=false", "sendTyping stop=true", "send"],
            "{:?}",
            fake.requests
        );
    }

    // A picture signal-cli didn't keep: the agent is told nothing, the
    // owner why. It remembers the thread: the model is sent the
    // conversation.
    let picture = received(
        OWNER,
        1006,
        json!({"dataMessage": {"timestamp": 1006, "message": null,
            "attachments": [{"contentType": "image/jpeg", "id": "a.jpg", "size": 1}]}}),
    );
    fake.lock()
        .unwrap()
        .notifications
        .extend([picture, text(OWNER, 1007, "And again")]);
    sent_on_signal(
        &fake,
        &[
            "I can read text, images (PNG, JPEG, GIF or WebP), documents (PDF or text) \
             and voice notes only, for now: an image couldn't be downloaded.",
            "I see 3 messages; the last: And again",
        ],
        &mut agent,
    )
    .await;
    agent.remembered("signal-15551234567", &[], 3);
    // The mention without the agent's, in the group's own thread: its id
    // in base64url.
    agent.asked_in_group("signal-g-RnJpZW5kcw", "hello");

    // An answer nobody waits for (a schedule's), through `lnk-signal
    // send` and the running bridge: to the owner, and to the group they
    // mentioned the agent in; never a stranger or a group they didn't.
    let settings = json!({"account": AGENT, "allowed_users": [OWNER]});
    let sent = agent.send_on("signal", &settings, &OWNER[1..], "Good morning.");
    assert!(sent.status.success(), "{sent:?}");
    sent_on_signal(&fake, &["Good morning."], &mut agent).await;
    let sent = agent.send_on("signal", &settings, "g-RnJpZW5kcw", "Good evening.");
    assert!(sent.status.success(), "{sent:?}");
    assert!(
        fake.lock()
            .unwrap()
            .requests
            .iter()
            .any(|(method, params)| {
                method == "send"
                    && params["groupId"] == GROUP
                    && params["message"] == "Good evening."
            })
    );
    for elsewhere in ["15550000000", "g-T3RoZXJz"] {
        let refused = agent.send_on("signal", &settings, elsewhere, "Hi.");
        assert!(!refused.status.success(), "{elsewhere}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("not sending there"),
            "{refused:?}"
        );
    }

    // An answer past Signal's limit is split into more messages, between
    // words where it can: a word longer than one message is cut.
    let long = "x".repeat(2100);
    fake.lock()
        .unwrap()
        .notifications
        .push(text(OWNER, 1008, &long));
    let (head, tail) = long.split_at(2000);
    sent_on_signal(
        &fake,
        &["I see 5 messages; the last:", head, tail],
        &mut agent,
    )
    .await;

    // A picture signal-cli kept reaches the agent, its caption with it.
    let attachments = state(&agent.home).join(".signal-cli/data/attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    std::fs::write(attachments.join("b.jpg"), [0xff, 0xd8, 0xff, 0xe0]).unwrap();
    let picture = received(
        OWNER,
        1009,
        json!({"dataMessage": {"timestamp": 1009, "message": "and this?",
            "attachments": [{"contentType": "image/jpeg", "id": "b.jpg", "size": 4}]}}),
    );
    fake.lock().unwrap().notifications.push(picture);
    let seen = "I see 7 messages; the last: and this?\n[image/jpeg, 4 bytes]";
    sent_on_signal(&fake, &[seen], &mut agent).await;

    assert_eq!(fake.lock().unwrap().runs.len(), 1, "signal-cli restarted");
    agent.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_on_the_mini_harness_answers_on_signal() {
    talk_on_signal("signal-open", false).await;
}

/// The same from the sandbox (Linux, bubblewrap).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn it_answers_on_signal_from_its_sandbox() {
    if agent::bubblewrap() {
        talk_on_signal("signal-sandboxed", true).await;
    }
}
