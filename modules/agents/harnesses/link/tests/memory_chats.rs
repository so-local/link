//! Shared notes belong to the agent; transcripts belong to one conversation.
use crate::turns;
use link_connection_contract::{Bridge, ask};
use link_harness::agent::Thread;
use serde_json::Value;

#[tokio::test]
async fn saved_notes_reach_discord_and_fresh_chats_after_history_is_deleted() {
    let world = turns::world("notes-across-chats").await;
    let bridge = Bridge {
        agent: "main".into(),
        endpoint: world.endpoint.clone(),
        key: "the-key".to_string().into(),
        proof: true,
        settings: Value::Null.into(),
        ..Default::default()
    };
    let (progress, _) = tokio::sync::watch::channel(String::new());
    ask(
        &bridge,
        "link:main",
        "CLI-only transcript marker",
        &progress,
    )
    .await
    .unwrap();
    let notes = world.root.join("files/memory/notes.md");
    std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
    std::fs::write(&notes, "- Owner's name is Mika; favorite color is teal.\n").unwrap();
    let history = world
        .root
        .join("files/chats")
        .join(Thread::new("link:main").unwrap().folder());
    std::fs::remove_dir_all(history).unwrap();
    for thread in ["discord:12345", "link:fresh"] {
        ask(&bridge, thread, "What do you know about me?", &progress)
            .await
            .unwrap();
    }
    let asked = world.asked.lock().unwrap();
    assert_eq!(asked.len(), 3);
    for request in &asked[1..] {
        let system = request["messages"][0]["content"].as_str().unwrap();
        assert!(
            system.contains("Mika") && system.contains("teal"),
            "{request}"
        );
        assert!(
            !request.to_string().contains("CLI-only transcript marker"),
            "{request}"
        );
    }
    assert!(notes.is_file());
}

#[tokio::test]
async fn unsaved_facts_in_one_chat_do_not_become_another_chats_memory() {
    let world = turns::world("unsaved-chat-facts").await;
    let bridge = Bridge {
        agent: "main".into(),
        endpoint: world.endpoint.clone(),
        key: "the-key".to_string().into(),
        proof: true,
        settings: Value::Null.into(),
        ..Default::default()
    };
    let (progress, _) = tokio::sync::watch::channel(String::new());
    ask(
        &bridge,
        "link:first",
        "My name is UnrecordedMika and my favorite color is UnrecordedTeal.",
        &progress,
    )
    .await
    .unwrap();
    for thread in ["discord:12345", "link:second"] {
        ask(
            &bridge,
            thread,
            "What is my name and favorite color?",
            &progress,
        )
        .await
        .unwrap();
    }
    let asked = world.asked.lock().unwrap();
    assert_eq!(asked.len(), 3);
    assert!(asked[0].to_string().contains("UnrecordedMika"));
    for request in &asked[1..] {
        let sent = request.to_string();
        assert!(
            !sent.contains("UnrecordedMika") && !sent.contains("UnrecordedTeal"),
            "{sent}"
        );
    }
    assert!(!world.root.join("files/memory/notes.md").exists());
}
