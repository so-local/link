use std::time::{Duration, SystemTime};

use serde_json::json;

use super::*;

fn asked(chat: &str, weight: u64) -> Asked {
    Asked {
        chat: chat.into(),
        model: Some("model".into()),
        window: Some(1000),
        price: None,
        above: Vec::new(),
        parent: None,
        parts: vec![Part {
            path: "instructions".into(),
            bytes: weight * 4,
            weight,
        }],
    }
}

fn measured(input: u64) -> Option<Tokens> {
    Some(Tokens {
        input,
        output: 1,
        cached: 0,
    })
}

#[test]
fn calls_are_kept_in_order_per_chat() {
    let dir = link_test_kit::TempDir::new("context-kept");
    asked("main", 10)
        .keep(&dir, "work", measured(12))
        .expect("kept");
    asked("main", 20).keep(&dir, "work", None).expect("kept");
    asked("telegram:42", 5)
        .keep(&dir, "work", measured(6))
        .expect("kept");

    let read = read(&dir, "work", Some("main")).expect("read");
    let report = read.chat.expect("main's report");
    assert_eq!(report.chat, "main");
    assert_eq!(report.calls, 2);
    assert_eq!(report.latest.seq, 2);
    assert_eq!(report.latest.tokens, None);
    assert_eq!(report.over[0].tokens, 12 + 20, "measured, then estimated");
    assert_eq!(read.chats.len(), 2);

    // A name that isn't plain has a file named by its fingerprint.
    let other = read_chat(&dir, "telegram:42");
    assert_eq!(other.latest.tokens, measured(6));
    // Another agent's are its own.
    assert_eq!(super::read(&dir, "main", None).expect("read").chats, []);
    assert!(super::read(&dir, "work", Some("gone")).is_err());
    assert!(asked("main", 1).keep(&dir, "../work", None).is_err());
}

fn read_chat(dir: &Path, chat: &str) -> context::Report {
    super::read(dir, "work", Some(chat))
        .expect("read")
        .chat
        .expect("report")
}

#[test]
fn a_call_longer_than_the_tail_is_still_found() {
    let dir = link_test_kit::TempDir::new("context-long");
    let mut long = asked("main", 1);
    long.parts = (0..2000)
        .map(|i| Part {
            path: format!("tools/tool{i:0>100}"),
            bytes: 1,
            weight: 1,
        })
        .collect();
    long.keep(&dir, "work", None).expect("kept");
    asked("main", 1).keep(&dir, "work", None).expect("kept");
    assert_eq!(read_chat(&dir, "main").latest.seq, 2);
}

#[test]
fn files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = link_test_kit::TempDir::new("context-private");
    asked("main", 1).keep(&dir, "work", None).expect("kept");
    let file = super::dir(&dir, "work").unwrap().join("main.jsonl");
    let mode = std::fs::metadata(&file).expect("file").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_full_chat_drops_its_oldest_half() {
    let dir = link_test_kit::TempDir::new("context-halve");
    for weight in 1..=10 {
        asked("main", weight)
            .keep(&dir, "work", None)
            .expect("kept");
    }
    halve(&super::dir(&dir, "work").unwrap().join("main.jsonl")).expect("halved");
    let report = read_chat(&dir, "main");
    assert_eq!(report.calls, 5);
    assert_eq!(report.latest.seq, 10);
    // The next goes on from the last, whatever was dropped.
    asked("main", 1).keep(&dir, "work", None).expect("kept");
    assert_eq!(read_chat(&dir, "main").latest.seq, 11);
}

#[test]
fn the_latest_chat_is_reported_without_one_named() {
    let dir = link_test_kit::TempDir::new("context-latest");
    assert_eq!(super::read(&dir, "work", None).expect("read").chat, None);
    asked("old", 1).keep(&dir, "work", None).expect("kept");
    let folder = super::dir(&dir, "work").unwrap();
    File::options()
        .write(true)
        .open(folder.join("old.jsonl"))
        .expect("file")
        .set_modified(SystemTime::now() - Duration::from_secs(60))
        .expect("dated");
    let mut newer = asked("new", 1);
    newer.chat = "new".into();
    newer.keep(&dir, "work", None).expect("kept");
    let read = super::read(&dir, "work", None).expect("read");
    assert_eq!(read.chats.len(), 2);
    assert_eq!(read.chat.expect("latest").chat, read.chats[0].chat);
}

#[test]
fn a_chat_is_its_callers_name_else_its_opening() {
    assert_eq!(chat(Some("main"), Some(1)).as_deref(), Some("main"));
    assert_eq!(
        chat(Some(""), Some(255)).as_deref(),
        Some("opening-00000000000000ff")
    );
    assert_eq!(chat(None, None), None);
    let body = json!({"messages": [{"role": "user", "content": "hi"}], "model": "m"});
    let asked = Asked::of(Api::Chat, &body, None).expect("asked");
    assert!(asked.chat.starts_with("opening-"));
    assert_eq!(asked.model.as_deref(), Some("m"));
    assert_eq!(asked.parts[0].path, "conversation/user");
}

#[test]
fn a_chat_reports_the_work_it_asked_for() {
    let dir = link_test_kit::TempDir::new("context-work");
    let keep = |chat: &str, parent: Option<&str>, weight: u64| {
        let mut asked = asked(chat, weight);
        asked.parent = parent.map(str::to_owned);
        asked.keep(&dir, "work", measured(weight)).expect("kept");
    };
    keep("main", None, 10);
    keep("task-1", Some("main"), 4);
    keep("task-2", Some("task-1"), 2);
    keep("main/compaction", Some("main"), 6);
    let read = read(&dir, "work", None).expect("read");
    let report = read.chat.expect("the person's chat");
    assert_eq!(report.chat, "main", "not its compaction, though later");
    let over: Vec<(&str, u64)> = report
        .over
        .iter()
        .map(|s| (s.path.as_str(), s.tokens))
        .collect();
    assert_eq!(over.len(), 4, "{over:?}");
    for part in [
        ("instructions", 10),
        ("subagents/task-1/instructions", 4),
        ("subagents/task-1/subagents/task-2/instructions", 2),
        ("compaction/instructions", 6),
    ] {
        assert!(over.contains(&part), "{part:?} in {over:?}");
    }
    let kept = read
        .chats
        .iter()
        .find(|k| k.chat == "task-2")
        .expect("kept");
    assert_eq!(kept.parent.as_deref(), Some("task-1"));
}
