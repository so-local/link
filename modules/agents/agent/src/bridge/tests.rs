use super::*;
use holding::paired;
use way_in::Failing;
mod answers;
mod carried;
mod outbox;
mod taking;

/// A request the harness was asked: its thread, sender, hops, text.
type Asked = (String, String, String, String);

/// Each prompt's thread and the standing its `Said` carried.
type Stood = Vec<(String, Option<link_connection_contract::access::Standing>)>;

/// A fake harness and connection: what the harness was asked (thread,
/// sender, hops, text), each answered "On <text>", and the tools each
/// message narrowed its conversation to (thread, `Said::tools`); what
/// the connection sent, one file per chat; what it was told went after a
/// crash.
struct Fake {
    _root: link_test_kit::TempDir,
    dir: PathBuf,
    endpoint: String,
    runtime: tokio::runtime::Runtime,
    way_in: WayIn,
    asked: std::sync::Arc<std::sync::Mutex<Vec<Asked>>>,
    narrowed: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
    /// The standing each prompt's `Said` carried, by its thread.
    stood: std::sync::Arc<std::sync::Mutex<Stood>>,
    done: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
}

impl Fake {
    /// `recovered`: what its harness finished after a crash.
    fn new(name: &str, recovered: serde_json::Value) -> Self {
        let root = link_test_kit::TempDir::new(name);
        let dir = root.to_path_buf();
        // A connection module whose `send` keeps what it's handed, by
        // chat, and fails for chat 99.
        let program = dir.join("lnk-fake");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\n[ \"$1\" = send ] || exit 0\nline=$(cat)\necho attempt >> '{dir}'/attempts\n\
                     case \"$line\" in *'\"chat\":\"99\"'*) exit 1 ;; esac\n\
                     n=$(ls '{dir}' | grep -c sent)\nprintf '%s\\n' \"$line\" > '{dir}'/part-$.json\nmv '{dir}'/part-$.json '{dir}'/sent-$n.json\nprintf '{{\"messages\":[\"fake-%s\"]}}\\n' \"$n\"\n",
                dir = dir.display()
            ),
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let done = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let narrowed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (kept, told, tooled) = (asked.clone(), done.clone(), narrowed.clone());
        let stood = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let standing = stood.clone();
        let recovered = serde_json::from_value(recovered).unwrap();
        // The harness, on ACP: each prompt kept as its thread, sender,
        // hops and text, as a bridge said them, its tools beside.
        let harness = runtime.block_on(link_connection_contract::acp::fake::serve_seen(
            |text| format!("On {text}"),
            recovered,
            move |seen| match seen {
                link_connection_contract::acp::fake::Seen::Prompted(asked) => {
                    let said = &asked.said;
                    standing
                        .lock()
                        .unwrap()
                        .push((asked.thread.clone(), said.standing));
                    if let Some(tools) = &said.tools {
                        tooled
                            .lock()
                            .unwrap()
                            .push((asked.thread.clone(), tools.join(",")));
                    }
                    let hops = match said.from {
                        Some(_) => said.hops.to_string(),
                        None => String::new(),
                    };
                    kept.lock().unwrap().push((
                        asked.thread.clone(),
                        said.from.clone().unwrap_or_default(),
                        hops,
                        asked.text.clone(),
                    ));
                }
                link_connection_contract::acp::fake::Seen::Recover(asked) => {
                    told.lock()
                        .unwrap()
                        .push(serde_json::to_value(asked).unwrap());
                }
            },
        ));
        let endpoint = harness.endpoint;
        let way_in = Self::way_in_at(&dir, &endpoint);
        Self {
            _root: root,
            dir,
            endpoint,
            runtime,
            way_in,
            asked,
            narrowed,
            stood,
            done,
        }
    }

    /// A way in to the harness at `endpoint`, its files in `dir`: its
    /// keyring `dir/keys`, its teams' files `dir/teams`.
    fn way_in_at(dir: &std::path::Path, endpoint: &str) -> WayIn {
        let request = link_connection_contract::Bridge {
            agent: "main".into(),
            settings: serde_json::json!({"bot_token": "123:secret", "allowed_users": [42]}).into(),
            ..Default::default()
        };
        WayIn {
            interaction_agent: "main".into(),
            interactions: Some(link_interactions::Store::at(dir.join("interactions"))),
            terms: link_connection_contract::Terms {
                name: "main".into(),
                me: link_sandbox_binding::Identity {
                    agent: "f0e1d2c3b4a5".into(),
                    service: String::new(),
                    owner: "local-link-aaaaaaaaaaaa".into(),
                    teams: Vec::new(),
                },
                tools: true,
                ..Default::default()
            },
            keys: Arc::new(link_sandbox_binding::Keyring::at(&dir.join("keys"))),
            owed: dir.join("owed"),
            endpoint: link_connection_contract::Bridge {
                agent: "main".into(),
                endpoint: endpoint.to_string(),
                ..Default::default()
            },
            connections: BTreeMap::from([(
                "telegram".into(),
                (
                    link_test_kit::recording::program(
                        &dir.join("lnk-fake"),
                        &dir.join("recorded-connections"),
                    )
                    .unwrap(),
                    request,
                ),
            )]),
            teams: dir.join("teams"),
            held: crate::chat::Held::at(&dir.join("held.jsonl")),
            received: dir.join("received"),
            per_day: Some(100),
            hops: Some(3),
            chains: Some(link_interactions::chains::Chains::at(
                &dir.join("interactions"),
            )),
            counting: Arc::new(std::sync::Mutex::new(())),
        }
    }

    /// What the connection sent, in order: (chat, text).
    fn sent(&self) -> Vec<(String, String)> {
        (0..)
            .map(|n| self.dir.join(format!("sent-{n}.json")))
            .take_while(|path| path.exists())
            .map(|path| {
                let sent: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                (
                    sent["chat"].as_str().unwrap().to_string(),
                    sent["text"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn wait_sent(&self, count: usize) -> Vec<(String, String)> {
        link_test_kit::wait::eventually(
            "the original destination accepted the answers",
            Duration::from_secs(10),
            || {
                let sent = self.sent();
                (sent.len() >= count).then_some(sent)
            },
        )
    }
}

fn asked(thread: &str, from: &str, hops: &str, text: &str) -> Asked {
    (thread.into(), from.into(), hops.into(), text.into())
}

/// A way in reads its agent's spec once, for its terms and its
/// scheduler's caps alike: each `lnk agent chat send` makes one, and
/// each read parses the agent's settings file.
#[test]
fn a_way_in_parses_its_agents_spec_once() {
    if !crate::settings::tests::own_config() {
        return;
    }
    let mut agent = Settings::new("main");
    agent.save().unwrap();
    let parses = || crate::settings::configuration::PARSES.with(std::cell::Cell::get);
    let before = parses();
    let _way_in = WayIn::new(&agent, &Plan::default());
    assert_eq!(parses() - before, 1);
}

/// Forgetting an agent's owner reader frees the events only it would
/// read; another agent's reader keeps its cursor and what it hasn't read.
/// `lnk agent delete` and `left` forgetting it are the agent's own tests'
/// (`removing_an_agent_takes_only_it`, `left_is_refused_while_its_keys_stay`).
#[test]
fn forgetting_an_owner_reader_frees_its_events_and_keeps_anothers() {
    let dir = link_test_kit::TempDir::new("owner-reader");
    let store = link_interactions::Store::at(dir.join("interactions"));
    assert_eq!(store.subscribe(&owner_reader("gone")).unwrap(), 0);
    assert_eq!(store.subscribe(&owner_reader("away")).unwrap(), 0);
    let ask = |key: &str| {
        store
            .create(
                link_interactions::sources::requester("agent", "service:m1a2i3n4s5v6", "away")
                    .unwrap(),
                link_interactions::sources::owner(),
                link_interactions::QuestionDraft {
                    caller_idempotency_key: key.into(),
                    title: "A Question".into(),
                    questions: vec![link_interactions::QuestionItem {
                        id: link_interactions::ItemId::new("ok").unwrap(),
                        prompt: "Go on?".into(),
                        answer_shape: link_interactions::AnswerShape::Confirmation,
                        default_answer: None,
                    }],
                    requested_timeout: None,
                },
                100,
            )
            .unwrap()
    };
    ask("one");
    ask("two");
    store.acknowledge(&owner_reader("away"), 2).unwrap();
    assert_eq!(store.events(0).unwrap().len(), 2);
    assert!(forget_owner_reader(&store, "gone").unwrap());
    assert_eq!(store.events(0).unwrap().len(), 1);
    assert_eq!(store.cursor(&owner_reader("away")).unwrap(), 2);
}
