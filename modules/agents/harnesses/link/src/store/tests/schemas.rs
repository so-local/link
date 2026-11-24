//! Every stored event uses its owner's saved-file schema, through every way.

use super::*;
use link_test_kit::TempDir;

fn fixture(name: &str) -> (TempDir, Files, Agent, Thread) {
    let root = TempDir::new(name);
    let agent = Agent::new("main").unwrap();
    let thread = Thread::new("link:schemas").unwrap();
    let mut agents = Agents::default();
    agents.add(
        agent.clone(),
        root.join("harness"),
        Folder {
            home: root.join("home"),
            agent: root.join("own"),
        },
    );
    (root, Files::new(agents), agent, thread)
}

#[test]
fn a_log_names_its_schema_and_refuses_a_newer_one() {
    let (root, store, agent, thread) = fixture("event-schema-log");
    let event = said(1, "retained");
    store.append(&agent, &thread, &event, &[]).unwrap();
    let path = root.join("own/chats/link-schemas/events.jsonl");
    let line = std::fs::read(&path).unwrap();
    assert!(line.ends_with(b",\"schema\":1}\n"));
    assert_eq!(
        store.read(&agent, &thread).unwrap().as_slice(),
        std::slice::from_ref(&event)
    );
    let mut future: serde_json::Value = serde_json::from_slice(&line).unwrap();
    future["schema"] = 2.into();
    std::fs::write(&path, format!("{future}\n")).unwrap();
    let error = store.read(&agent, &thread).unwrap_err();
    assert!(format!("{error:#}").contains("needs a newer lnk"));
    // The shared no-schema policy also applies to an existing unversioned line.
    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();
    assert_eq!(store.read(&agent, &thread).unwrap(), [event]);
}

#[test]
fn driver_records_use_the_same_schema_reader() {
    let event = said(8, "driver");
    let bytes = link_files::saved::print_json(&event).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(event_of(value.clone()).unwrap(), event);
    let mut future = value;
    future["schema"] = 2.into();
    assert!(format!("{:#}", event_of(future).unwrap_err()).contains("needs a newer lnk"));
}

#[test]
fn moving_between_log_and_compact_keeps_schema_and_event_fields() {
    let (root, store, agent, thread) = fixture("event-schema-moving");
    let mut event = said(1, "retained");
    event.model = Some("fixture-model".into());
    event.latency_ms = Some(7);
    store.append(&agent, &thread, &event, &[]).unwrap();
    store.move_conversations(&agent, &Way::Compact).unwrap();
    assert_eq!(
        store.read(&agent, &thread).unwrap().as_slice(),
        std::slice::from_ref(&event)
    );
    let folder = Held::root(&root.join("own/chats/link-schemas"), false)
        .unwrap()
        .unwrap();
    let kept = std::fs::read(folder.at(LOG)).unwrap();
    let future = br#"{"seq":2,"role":"user","text":"future","at":"","schema":2}"#;
    let mut line = future.to_vec();
    line.push(b'\n');
    frames::append(&folder, 2, &line).unwrap();
    assert!(
        format!("{:#}", store.read(&agent, &thread).unwrap_err()).contains("needs a newer lnk")
    );
    std::fs::write(folder.at(LOG), kept).unwrap();
    drop(folder);
    store.move_conversations(&agent, &Way::Log).unwrap();
    assert_eq!(store.read(&agent, &thread).unwrap(), [event]);
    assert!(
        std::fs::read(root.join("own/chats/link-schemas/events.jsonl"))
            .unwrap()
            .ends_with(b",\"schema\":1}\n")
    );
}

#[test]
fn private_configuration_names_its_schema_and_refuses_a_newer_one() {
    use std::os::unix::fs::PermissionsExt;

    let (root, store, agent, _) = fixture("configuration-schema");
    let private = "private-fixture-credential";
    let config = Config {
        endpoint_key: Some(private.to_string().into()),
        ..Config::default()
    };
    store.set_config(&agent, &config).unwrap();
    let path = root.join("harness/config.json");
    let bytes = std::fs::read(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(store.config(&agent).unwrap(), config);

    // The shared no-schema policy accepts the existing current shape.
    // Missing read registrations remain absent rather than inferred.
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let fresh = Files::new(store.agents.clone());
    let read = fresh.config(&agent).unwrap();
    assert_eq!(read, config);
    assert!(read.read_sources.is_empty());

    value["schema"] = 2.into();
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let fresh = Files::new(store.agents.clone());
    let error = format!("{:#}", fresh.config(&agent).unwrap_err());
    assert!(error.contains("needs a newer lnk"));
    assert!(!error.contains(private));
}
