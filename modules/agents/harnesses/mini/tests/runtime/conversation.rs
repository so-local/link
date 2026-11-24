use super::*;
use crate::core::{Answer, Call};
use serde_json::json;

#[test]
fn complete_transcripts_survive_reload_without_rewriting_the_prefix() {
    let dir = link_test_kit::TempDir::new("mini-transcript");
    let path = conversation_path(&dir, "connection:42").unwrap();
    let messages = vec![
        Message {
            role: "user".into(),
            text: "read".into(),
            ..Message::default()
        },
        Message {
            role: "assistant".into(),
            calls: vec![Call {
                id: "c1".into(),
                name: "read".into(),
                arguments: json!({"path":"note"}),
            }],
            thinking: vec![json!({"type":"thinking","thinking":"reading","signature":"opaque"})],
            ..Message::default()
        },
        Message {
            role: "user".into(),
            answers: vec![Answer {
                call: "c1".into(),
                text: "hello".into(),
                failed: false,
                parts: Vec::new(),
            }],
            ..Message::default()
        },
        Message {
            role: "assistant".into(),
            text: "hello".into(),
            ..Message::default()
        },
    ];
    assert!(load_conversation(&path).unwrap().is_empty());
    for message in &messages {
        append_message(&path, message).unwrap();
    }
    let prefix = std::fs::read(&path).unwrap();
    assert_eq!(load_conversation(&path).unwrap(), messages);
    check_complete(&load_conversation(&path).unwrap()).unwrap();
    append_message(
        &path,
        &Message {
            role: "user".into(),
            text: "again".into(),
            ..Message::default()
        },
    )
    .unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(&prefix));
    assert_eq!(list_sessions(&dir), ["connection:42"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn interrupted_calls_are_kept_but_never_silently_replayed() {
    let pending = Message {
        role: "assistant".into(),
        calls: vec![Call {
            id: "c1".into(),
            name: "write".into(),
            arguments: json!({}),
        }],
        ..Message::default()
    };
    assert!(
        check_complete(std::slice::from_ref(&pending))
            .unwrap_err()
            .to_string()
            .contains("unanswered")
    );
    let answer = Message {
        role: "user".into(),
        answers: vec![Answer {
            call: "c1".into(),
            text: "written".into(),
            failed: false,
            parts: Vec::new(),
        }],
        ..Message::default()
    };
    check_complete(&[pending, answer]).unwrap();
}

#[test]
fn damaged_records_and_unsafe_names_are_refused() {
    let dir = link_test_kit::TempDir::new("mini-damaged");
    let path = conversation_path(&dir, "normal").unwrap();
    std::fs::write(&path, "{broken\n").unwrap();
    assert!(
        load_conversation(&path)
            .unwrap_err()
            .to_string()
            .contains("line 1")
    );
    for name in [
        "",
        ".",
        "..",
        "../other",
        "/absolute",
        "a/b",
        "a\\b",
        "a%3Ab",
        "a\n",
    ] {
        assert!(conversation_path(&dir, name).is_err(), "{name:?}");
    }
}

/// Each line names its schema; a line a newer release wrote is refused,
/// naming the upgrade, and a damaged one names its line.
#[test]
fn lines_name_their_schema_and_refuse_a_newer_one() {
    let dir = link_test_kit::TempDir::new("mini-lines");
    let path = conversation_path(&dir, "t").unwrap();
    let message = Message {
        role: "user".into(),
        text: "hi".into(),
        ..Message::default()
    };
    append_message(&path, &message).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.trim_end().ends_with(r#""schema":1}"#), "{text}");
    let newer = text.replace(r#""schema":1}"#, r#""schema":2}"#);
    std::fs::write(&path, format!("{text}{newer}")).unwrap();
    let refused = format!("{:#}", load_conversation(&path).unwrap_err());
    assert!(refused.contains("line 2"), "{refused}");
    assert!(refused.contains("lnk upgrade"), "{refused}");
}
