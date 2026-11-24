//! Commands end to end, in process: the owner's `/new` over ACP
//! starts a new conversation without asking the model, and the next turn
//! sends nothing earlier; anyone else's `/new` is a message.

use crate::common;

use std::sync::Arc;

use link_harness::{
    agent::{Agent, Agents},
    commands,
    config::Config,
    server::{self, Server},
    store::Files,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::json;

struct World {
    root: TempDir,
    port: u16,
    asked: common::Asked,
}

/// An agent on a fake model, which answers "Noted".
async fn world(name: &str) -> World {
    world_pausing(name, std::time::Duration::ZERO).await
}

/// [`world`], the model's answer after its first piece `pause` later.
async fn world_pausing(name: &str, pause: std::time::Duration) -> World {
    world_of(name, pause, None).await
}

/// [`world_pausing`], its spec `spec`'s if given, its provider offering
/// `fake`, `other` and `third`.
async fn world_of(name: &str, pause: std::time::Duration, spec: Option<&str>) -> World {
    let root = TempDir::new(&format!("commands-{name}"));
    let models = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(|| async {
            axum::Json(json!({"data": [{"id": "fake"}, {"id": "other"}, {"id": "third"}]}))
        }),
    );
    let (port, asked) = common::fake_model_with(models, pause, |_, _| {
        vec![
            common::delta(json!({"content": "Not"})),
            common::delta(json!({"content": "ed"})),
        ]
    })
    .await;
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(&files));
    let store = Files::new(agents.clone());
    link_harness::store::Store::set_config(
        &store,
        &agent,
        &Config {
            model: Model {
                provider: "openai".into(),
                name: Some("fake".into()),
                base_url: Some(format!("http://127.0.0.1:{port}/v1")),
                ..Model::default()
            },
            endpoint_key: Some("the-key".to_string().into()),
            spec: spec.map(|spec| {
                let own = link_agent_spec::parse(&format!("schema = 1\n{spec}")).unwrap();
                let known = link_agent_spec::Known {
                    name: "main".into(),
                };
                link_agent_spec::resolve(Some(&own), &[], &known)
                    .unwrap()
                    .spec
            }),
            ..Config::default()
        },
    )
    .unwrap();
    let server = Arc::new(Server::new(
        Harness::new(agents, Arc::new(store)).unwrap(),
        agent,
    ));
    let port = common::serve(server::router(server)).await;
    World { root, port, asked }
}

/// Asks `text` on `telegram:42`, from `from` or the owner, as a bridge
/// does: 200 and its answer, or a refusal's status and why.
async fn send(world: &World, text: &str, from: Option<&str>) -> (u16, String) {
    let asking = common::Asking {
        key: "the-key",
        from,
        ..Default::default()
    };
    common::answered(common::ask_as(world.port, "telegram:42", text, asking).await)
        .unwrap_or((0, "nothing answered".into()))
}

fn folder(world: &World) -> std::path::PathBuf {
    world.root.join("files/chats/telegram-42")
}

/// Every message's text the model was sent in its `n`th request.
fn sent(world: &World, n: usize) -> String {
    world.asked.lock().unwrap()[n]["messages"].to_string()
}

#[tokio::test]
async fn the_owners_new_starts_a_new_conversation() {
    let world = world("new").await;
    let (status, _) = send(&world, "My name is Bea.", None).await;
    assert_eq!(status, 200);

    // Answered by the harness, the model never asked.
    let (status, body) = send(&world, "/new", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body, commands::NEW);
    assert_eq!(world.asked.lock().unwrap().len(), 1);

    // Kept as an event of its own, and as the owner's in the copy.
    let events = common::events(&folder(&world));
    let ran = events.last().unwrap();
    assert_eq!(
        (ran["role"].as_str(), ran["text"].as_str()),
        (Some("command"), Some("/chat:create"))
    );
    let copy = std::fs::read_to_string(folder(&world).join("conversation.md")).unwrap();
    assert!(copy.contains("My name is Bea."), "{copy}");

    // The next turn sends nothing earlier, nor the command.
    let (status, _) = send(&world, "What's my name?", None).await;
    assert_eq!(status, 200);
    let after = sent(&world, 1);
    assert!(after.contains("What's my name?"), "{after}");
    assert!(
        !after.contains("Bea") && !after.contains("/new") && !after.contains("Noted"),
        "{after}"
    );

    // And the one after, from the window its answer kept.
    let (status, _) = send(&world, "And now?", None).await;
    assert_eq!(status, 200);
    let later = sent(&world, 2);
    assert!(
        later.contains("What's my name?") && later.contains("And now?"),
        "{later}"
    );
    assert!(!later.contains("Bea"), "{later}");
    let copy = std::fs::read_to_string(folder(&world).join("conversation.md")).unwrap();
    assert!(
        copy.contains("## You") && copy.contains("/chat:create"),
        "{copy}"
    );

    // Without its mark, the window is made anew from the events: still
    // after the `/new`.
    common::forget_marks(&folder(&world));
    let (status, _) = send(&world, "Once more", None).await;
    assert_eq!(status, 200);
    let made = sent(&world, 3);
    assert!(
        !made.contains("Bea") && made.contains("Once more"),
        "{made}"
    );
}

#[tokio::test]
async fn anyone_elses_new_is_a_message() {
    let world = world("others").await;
    let (status, _) = send(&world, "My name is Bea.", None).await;
    assert_eq!(status, 200);
    let (status, body) = send(&world, "/new", Some("member:telegram:7")).await;
    assert_eq!(status, 200, "{body}");
    // Sent to the model as theirs, and nothing before it left out.
    assert_eq!(world.asked.lock().unwrap().len(), 2);
    let theirs = sent(&world, 1);
    assert!(
        theirs.contains("/new") && theirs.contains("Bea"),
        "{theirs}"
    );
    let events = common::events(&folder(&world));
    assert!(events.iter().all(|e| e["role"] != "command"), "{events:?}");

    // As is the owner's `/new` with something after it.
    let (status, _) = send(&world, "/new idea: a garden", None).await;
    assert_eq!(status, 200);
    assert!(sent(&world, 2).contains("Bea"));
}

/// The owner's `/stop` ends the answer being written, keeping what it
/// wrote; with none running it says so; anyone else's is a message.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn the_owners_stop_ends_the_answer_being_written() {
    let world = world_pausing("stop", std::time::Duration::from_secs(5)).await;
    let (status, said) = send(&world, "/stop", None).await;
    assert_eq!((status, said.as_str()), (200, commands::NOTHING_TO_STOP));
    let started = std::time::Instant::now();
    let answering = tokio::spawn({
        let port = world.port;
        async move {
            let asking = common::Asking {
                key: "the-key",
                ..Default::default()
            };
            common::ask_as(port, "telegram:42", "Take your time.", asking).await
        }
    });
    // Asked, the model's first piece written, its rest a while away.
    for _ in 0..100 {
        if !world.asked.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (status, said) = send(&world, "/stop", None).await;
    assert_eq!((status, said.as_str()), (200, commands::STOPPED));
    let answered = answering.await.unwrap().unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert_eq!(answered, "Not");
    let events = common::events(&folder(&world));
    let cut = events.iter().find(|e| e["role"] == "assistant").unwrap();
    assert_eq!(cut["interrupted"], true, "{cut}");
    // Someone else's is text for the model.
    let (_, said) = send(&world, "/stop", Some("member:telegram:7")).await;
    assert_ne!(said, commands::NOTHING_TO_STOP);
}

/// A chat's conversations: named, left with `/chat:create`, taken up again
/// by name, forked and written out, the model sent only the one it's on.
#[tokio::test]
async fn the_owner_resumes_forks_renames_and_exports_conversations() {
    let world = world("conversations").await;
    let ok = |(status, body): (u16, String)| {
        assert_eq!(status, 200, "{body}");
        body
    };
    ok(send(&world, "My name is Bea.", None).await);
    assert_eq!(
        ok(send(&world, "/chat:rename names", None).await),
        "Conversation 1 is names."
    );
    ok(send(&world, "/chat:create", None).await);
    ok(send(&world, "I like tea.", None).await);
    let listed = ok(send(&world, "/resume", None).await);
    assert_eq!(
        listed,
        "Conversations:\n1. names, 2 messages\n2. I like tea., 2 messages (on)"
    );
    // Taken up by its name: the model is sent it, never the other.
    let resumed = ok(send(&world, "/chat:resume names", None).await);
    assert!(resumed.starts_with("Resumed 1: names"), "{resumed}");
    ok(send(&world, "Who am I?", None).await);
    let sent_now = sent(&world, 2);
    assert!(
        sent_now.contains("Bea") && !sent_now.contains("tea") && !sent_now.contains("/chat:"),
        "{sent_now}"
    );
    // A fork holds it, and goes on apart from it.
    let forked = ok(send(&world, "/fork aside", None).await);
    assert!(
        forked.starts_with("Forked: conversation 3, aside"),
        "{forked}"
    );
    ok(send(&world, "Only in the fork.", None).await);
    assert!(sent(&world, 3).contains("Bea"));
    ok(send(&world, "/chat:resume 1", None).await);
    ok(send(&world, "Back.", None).await);
    let back = sent(&world, 4);
    assert!(
        back.contains("Who am I?") && !back.contains("Only in the fork."),
        "{back}"
    );
    // Refused, it begins nothing: still on 1.
    let refused = ok(send(&world, "/chat:resume 1", None).await);
    assert_eq!(refused, "Already on 1.");
    let events = common::events(&folder(&world));
    assert_eq!(events.last().unwrap()["failed"], true);
    // Written out as people read it, the others left out.
    let exported = ok(send(&world, "/chat:export", None).await);
    assert_eq!(
        exported,
        "Exported conversation 1 to exports/telegram-42-1.md, in the agent's folder."
    );
    let copy = std::fs::read_to_string(world.root.join("files/exports/telegram-42-1.md")).unwrap();
    assert!(
        copy.contains("Bea") && copy.contains("Back.") && !copy.contains("tea"),
        "{copy}"
    );
    // Without its mark, the window is made anew: still the one it's on.
    common::forget_marks(&folder(&world));
    ok(send(&world, "Once more", None).await);
    let made = sent(&world, 5);
    assert!(made.contains("Bea") && !made.contains("tea"), "{made}");
}

/// `/chat:compact` summarizes what's before the owner's last message as
/// a turn of its own, and the next turn starts with the summary.
#[tokio::test]
async fn the_owner_compacts_the_conversation_now() {
    let world = world("compact").await;
    let ok = |(status, body): (u16, String)| {
        assert_eq!(status, 200, "{body}");
        body
    };
    let nothing = ok(send(&world, "/compact", None).await);
    assert_eq!(nothing, commands::NOTHING_TO_COMPACT);
    ok(send(&world, "My name is Bea.", None).await);
    ok(send(&world, "I like tea.", None).await);
    let compacted = ok(send(&world, "/chat:compact", None).await);
    assert_eq!(compacted, commands::compacted(2, false));
    // Asked once more, for the summary, of what leaves.
    assert_eq!(world.asked.lock().unwrap().len(), 3);
    assert!(sent(&world, 2).contains("Bea"));
    let events = common::events(&folder(&world));
    let summary = events.iter().find(|e| e["role"] == "summary").unwrap();
    let staying = events.iter().find(|e| e["text"] == "I like tea.").unwrap();
    assert_eq!(summary["covers"], staying["seq"]);
    ok(send(&world, "And now?", None).await);
    let after = sent(&world, 3);
    assert!(
        after.contains("tea") && after.contains("And now?") && !after.contains("Bea"),
        "{after}"
    );
}

/// `/chat:ask` answers on the side, keeping nothing.
#[tokio::test]
async fn the_owner_asks_on_the_side() {
    let world = world("btw").await;
    let ok = |(status, body): (u16, String)| {
        assert_eq!(status, 200, "{body}");
        body
    };
    ok(send(&world, "My name is Bea.", None).await);
    assert_eq!(world.asked.lock().unwrap().len(), 1);
    let before = common::events(&folder(&world)).len();
    let answer = ok(send(&world, "/btw what's my name?", None).await);
    assert_eq!(answer, "Noted");
    let side = sent(&world, 1);
    assert!(
        side.contains("Bea") && side.contains("what's my name?"),
        "{side}"
    );
    assert!(world.asked.lock().unwrap()[1]["tools"].is_null());
    assert_eq!(common::events(&folder(&world)).len(), before);
    assert_eq!(ok(send(&world, "/btw", None).await), commands::NO_QUESTION);
    ok(send(&world, "Go on.", None).await);
    assert!(!sent(&world, 2).contains("what's my name?"));
}

/// `/model:set` is the conversation's, an alias read as the model it
/// names; `--every-chat` makes it every chat's, `--fast` asks the provider's
/// faster service and `--effort` how hard it thinks.
#[tokio::test]
async fn the_owner_chooses_the_conversations_model() {
    let spec = "[model.aliases]\nhigh = \"other\"\n";
    let world = world_of("models", std::time::Duration::ZERO, Some(spec)).await;
    let ok = |(status, body): (u16, String)| {
        assert_eq!(status, 200, "{body}");
        body
    };
    let model = |n: usize| world.asked.lock().unwrap()[n]["model"].clone();
    assert_eq!(
        ok(send(&world, "/model:set high", None).await),
        "Model: other, in this conversation."
    );
    ok(send(&world, "Hi", None).await);
    assert_eq!(model(0), "other");
    // A new conversation keeps the agent's.
    ok(send(&world, "/chat:create", None).await);
    ok(send(&world, "Again", None).await);
    assert_eq!(model(1), "fake");
    let refused = ok(send(&world, "/model nope", None).await);
    assert!(refused.starts_with("No nope"), "{refused}");
    // Fast, of a provider that offers it.
    let fast = ok(send(&world, "/model:set --fast", None).await);
    assert!(fast.starts_with("Fast: fake"), "{fast}");
    ok(send(&world, "Quick", None).await);
    assert_eq!(world.asked.lock().unwrap()[2]["service_tier"], "priority");
    ok(send(&world, "/model:set --no-fast", None).await);
    ok(send(&world, "Slow", None).await);
    assert!(world.asked.lock().unwrap()[3]["service_tier"].is_null());
    // How hard it thinks, alone and set.
    assert!(ok(send(&world, "/model:set", None).await).contains("Effort: "));
    assert_eq!(
        ok(send(&world, "/model:set --effort high", None).await),
        "Effort: high, in this conversation."
    );
    assert!(ok(send(&world, "/model:set --effort max", None).await).starts_with("No max"));
    // Every chat's, with --every-chat: the next conversation starts on it.
    assert_eq!(
        ok(send(&world, "/model:set third --every-chat", None).await),
        "Model: third, in every chat, until the agent starts again with its own."
    );
    ok(send(&world, "/chat:create", None).await);
    ok(send(&world, "Later", None).await);
    assert_eq!(model(4), "third");
    let status = ok(send(&world, "/chat:status", None).await);
    assert!(status.starts_with("Model: third (openai)"), "{status}");
}

#[tokio::test]
async fn model_get_and_help_are_marked_commands_without_a_model_turn() {
    let world = world_of(
        "model-command-responses",
        std::time::Duration::ZERO,
        Some("command_response = \"🤖 Automated Response\"\n"),
    )
    .await;
    let read = ok(send(&world, "/model:get", None).await);
    assert!(
        read.starts_with("🤖 Automated Response\nModel: fake\n"),
        "{read}"
    );
    assert!(read.contains("Effort:"), "{read}");
    let help = ok(send(&world, "/model:help", None).await);
    for name in ["/model:get", "/model:set", "/model:help"] {
        assert!(help.contains(name), "{help}");
    }
    assert!(help.starts_with("🤖 Automated Response\nCommands:"));
    let set = ok(send(&world, "/model:set other", None).await);
    assert_eq!(
        set,
        "🤖 Automated Response\nModel: other, in this conversation."
    );
    let read = ok(send(&world, "/model", None).await);
    assert!(
        read.starts_with("🤖 Automated Response\nModel: other\n"),
        "{read}"
    );
    assert!(world.asked.lock().unwrap().is_empty());
    assert_eq!(common::events(&folder(&world))[0]["text"], "/model:get");
    assert_eq!(ok(send(&world, "Hi", None).await), "Noted");
    assert_eq!(world.asked.lock().unwrap()[0]["model"], "other");
    // A stranger's slash message is still a model turn, never marked.
    assert_eq!(
        ok(send(&world, "/model:help", Some("webhook:hook")).await),
        "Noted"
    );
}

fn ok((status, body): (u16, String)) -> String {
    assert_eq!(status, 200, "{body}");
    body
}

#[tokio::test]
async fn command_response_can_be_custom_or_empty() {
    for (name, spec, expected) in [
        (
            "custom",
            "command_response = \"Harness says:\"\n",
            "Harness says:\n",
        ),
        ("empty", "command_response = \"\"\n", ""),
    ] {
        let world = world_of(name, std::time::Duration::ZERO, Some(spec)).await;
        assert_eq!(
            ok(send(&world, "/new", None).await),
            format!("{expected}{}", commands::NEW)
        );
        assert!(world.asked.lock().unwrap().is_empty());
    }
}

/// A subagent's conversation, started on a model, an alias of its
/// spec's, runs on it, and keeps its choice as an event from its asker.
#[tokio::test]
async fn a_subagent_runs_on_the_model_it_was_started_with() {
    let spec = "[model.aliases]\nhigh = \"other\"\n";
    let world = world_of("subagent", std::time::Duration::ZERO, Some(spec)).await;
    let asking = common::Asking {
        key: "the-key",
        from: Some("task:a1b2c3d4e5f6"),
        model: Some("high"),
        ..Default::default()
    };
    let answer = common::ask_as(world.port, "task-a1b2c3d4e5f6", "Look it up.", asking).await;
    assert_eq!(answer.as_deref(), Ok("Noted"));
    assert_eq!(world.asked.lock().unwrap()[0]["model"], "other");
    let events = common::events(&world.root.join("files/chats/task-a1b2c3d4e5f6"));
    let chose = events.iter().find(|e| e["role"] == "command").unwrap();
    assert_eq!(
        (chose["text"].as_str(), chose["from"].as_str()),
        (Some("/model:set other"), Some("task:a1b2c3d4e5f6"))
    );
    // Never one its spec doesn't allow.
    let held = world_of(
        "subagent-allow",
        std::time::Duration::ZERO,
        Some("[model]\nallow = [\"fake\"]\n"),
    )
    .await;
    let asking = common::Asking {
        model: Some("other"),
        ..asking
    };
    let refused = common::ask_as(held.port, "task-a1b2c3d4e5f6", "Look it up.", asking).await;
    assert!(refused.is_err(), "{refused:?}");
    assert!(held.asked.lock().unwrap().is_empty());
}

/// The owner's skills and the repository's run as `/skill:<name>`, and by
/// a bare name as their precedence says; `/skill:list` names them.
#[tokio::test]
async fn the_owner_runs_skills_by_name() {
    let world = world("skills").await;
    // This fixture owns the repository boundary under every state directory.
    std::fs::create_dir(world.root.join("files/.git")).unwrap();
    let ok = |(status, body): (u16, String)| {
        assert_eq!(status, 200, "{body}");
        body
    };
    let write = |folder: &str, name: &str, body: &str| {
        let dir = world.root.join("files").join(folder).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} it.\n---\n{body}\n"),
        )
        .unwrap();
    };
    write("skills", "deploy", "Run the deploy.");
    write(".agents/skills", "review", "Review the diff.");
    write(".claude/skills", "new", "Not a command.");
    let listed = ok(send(&world, "/skill:list", None).await);
    assert!(
        listed.contains("/skill:deploy - deploy it. (yours)")
            && listed.contains("/skill:review - review it. (the repository's"),
        "{listed}"
    );
    ok(send(&world, "/deploy staging", None).await);
    let deploy = sent(&world, 0);
    assert!(
        deploy.contains("Run the deploy.") && deploy.contains("staging"),
        "{deploy}"
    );
    let loaded = |needle: &str| {
        let events = common::events(&folder(&world));
        let event = events
            .iter()
            .rev()
            .find(|event| {
                event["role"] == "user"
                    && event["text"]
                        .as_str()
                        .is_some_and(|text| text.contains(needle))
            })
            .unwrap();
        let evidence: link_harness_wire::evidence::Evidence =
            serde_json::from_value(event["evidence"].clone()).unwrap();
        let link_harness_wire::evidence::Record::SkillLoaded { load } = evidence.record else {
            panic!("an invocation has no skill-loading evidence");
        };
        load.validate().unwrap();
        assert!(
            load.model_view
                .matches(event["text"].as_str().unwrap().as_bytes())
        );
        load
    };
    let deploy_load = loaded("Run the deploy.");
    assert_eq!(
        deploy_load.original_input.as_deref(),
        Some("/deploy staging")
    );
    assert!(deploy_load.complete);
    assert!(
        deploy_load
            .supplied
            .as_ref()
            .unwrap()
            .matches(b"Run the deploy.")
    );
    assert_eq!(
        deploy_load.source,
        link_harness_wire::evidence::SkillSource::Owned {
            path: "skills/deploy/SKILL.md".into(),
        }
    );
    // The model knows the repository's, by their paths.
    let system = world.asked.lock().unwrap()[0]["messages"][0]["content"].to_string();
    assert!(
        system.contains("- review: review it. (.agents/skills/review/SKILL.md)"),
        "{system}"
    );
    ok(send(&world, "/review", None).await);
    assert!(sent(&world, 1).contains("Review the diff."));
    assert_eq!(
        loaded("Review the diff.").source,
        link_harness_wire::evidence::SkillSource::Repository {
            path: ".agents/skills/review/SKILL.md".into(),
        }
    );
    // A repository's never takes over a command: /new is Link's.
    assert_eq!(ok(send(&world, "/new", None).await), commands::NEW);
    ok(send(&world, "/skill:new", None).await);
    assert!(sent(&world, 2).contains("Not a command."));
    write("skills", "deploy", "Deploy changed instructions.");
    ok(send(&world, "/deploy production", None).await);
    let changed = loaded("Deploy changed instructions.");
    assert_ne!(changed.read.source, deploy_load.read.source);
    assert_eq!(
        changed.original_input.as_deref(),
        Some("/deploy production")
    );
    write(
        "skills",
        "large",
        &"x".repeat(link_harness::store::INSTRUCTIONS_MAX as usize + 10),
    );
    ok(send(&world, "/skill:large", None).await);
    let large = loaded("/skill:large");
    assert!(!large.complete);
    assert!(!large.omissions.is_empty());
    assert!(matches!(
        large.read.source,
        link_files::reading::ReadSnapshot::Prefix { .. }
    ));
}

/// `/chat:verbose` alone does the other of what the chat does now, and
/// `on` or `off` says which.
#[tokio::test]
async fn the_owner_turns_tool_calls_on_and_off() {
    let world = world("verbose").await;
    let ok = |(status, body): (u16, String)| {
        assert_eq!(status, 200, "{body}");
        body
    };
    // Link's spec shows activity: alone, it goes quiet.
    assert_eq!(
        ok(send(&world, "/verbose", None).await),
        commands::verbose(false)
    );
    assert_eq!(
        ok(send(&world, "/chat:verbose", None).await),
        commands::verbose(true)
    );
    assert_eq!(
        ok(send(&world, "/chat:verbose off", None).await),
        commands::verbose(false)
    );
    assert!(ok(send(&world, "/chat:verbose loud", None).await).starts_with("No loud"));
    let events = common::events(&folder(&world));
    let kept: Vec<&str> = events.iter().filter_map(|e| e["text"].as_str()).collect();
    assert_eq!(
        kept,
        [
            "/chat:verbose off",
            "/chat:verbose on",
            "/chat:verbose off",
            "/chat:verbose loud"
        ]
    );
}
