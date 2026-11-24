use super::*;
use link_harness_wire::evidence::{Evidence, Phase, Proposal, Record};
use link_store_contract::store::compact;
use link_test_kit::TempDir;

/// The agent's ID here, which no archive carries.
const ID: &str = "c0d3r1a2b3c4";

/// The evidence of a call proposed in work this machine names, or in
/// none.
fn proposed(work: bool) -> Evidence {
    Evidence::new(Record::Proposed(Proposal {
        phase: Phase::Turn,
        attempt: "turn-1".into(),
        work: work.then(|| link_interactions_contract::WorkRef {
            kind: link_interactions_contract::WorkKind::Conversation,
            owner: link_interactions_contract::Actor::Owner,
            agent: ID.into(),
            session: "telegram:42".into(),
            task: None,
            attempt: "turn-1".into(),
            generation: 1,
            control_revision: 0,
        }),
        started_unix_ms: 0,
        calls: Vec::new(),
    }))
}

/// A conversation's lines: a message, then a proposal's evidence.
fn lines(work: bool) -> String {
    let said = serde_json::json!({"seq": 1, "role": "user", "text": "Hi", "at": ""});
    let evidence = serde_json::json!({"seq": 2, "role": "evidence", "text": "", "at": "",
        "evidence": proposed(work)});
    format!("{said}\n{evidence}\n")
}

fn asked(dir: &TempDir) -> Carry {
    let staging = dir.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    Carry {
        thing: Thing {
            kind: KIND.into(),
            name: "main".into(),
            folders: [(Entry::Home(Home::Agents).name(), dir.join("Agents/main"))].into(),
        },
        staging,
    }
}

/// A conversation naming this machine's work goes as a log of its
/// events, each record's `work` none, a compact one's frames left
/// behind; one naming none goes as it is, and the agent's own files
/// are never changed.
#[test]
fn conversations_go_without_this_machines_work() {
    let dir = TempDir::new("archive-carry-chats");
    let chats = dir.join("Agents/main/agent/chats");
    let (log, compacted, plain) = (
        chats.join("telegram-42"),
        chats.join("slack-D1"),
        chats.join("link-main"),
    );
    for folder in [&log, &compacted, &plain] {
        std::fs::create_dir_all(folder).unwrap();
    }
    std::fs::write(log.join(EVENTS), lines(true)).unwrap();
    let mut frames = compact::named("slack:D1");
    frames.extend(compact::encode(lines(true).as_bytes()).unwrap());
    std::fs::write(compacted.join(FRAMES), &frames).unwrap();
    std::fs::write(plain.join(EVENTS), lines(false)).unwrap();

    let asked = asked(&dir);
    let carried = carry(&asked).unwrap();
    let mut replaced = carried.replaced.clone();
    replaced.sort_by(|a, b| a.file.cmp(&b.file));
    assert_eq!(
        replaced
            .iter()
            .map(|r| (r.file.clone(), r.with.is_some()))
            .collect::<Vec<_>>(),
        [
            (compacted.join(EVENTS), true),
            (compacted.join(FRAMES), false),
            (log.join(EVENTS), true),
        ]
    );
    for r in replaced.iter().filter_map(|r| r.with.as_ref()) {
        assert!(r.starts_with(&asked.staging), "{}", r.display());
        let text = std::fs::read_to_string(r).unwrap();
        assert!(!text.contains(ID), "{text}");
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), 2, "{text}");
        assert!(text.lines().nth(1).unwrap().starts_with(r#"{"seq":2,"#));
        let evidence: Evidence = serde_json::from_value(events[1]["evidence"].clone()).unwrap();
        assert_eq!(evidence, proposed(false));
    }
    // Its own files, as they were.
    assert_eq!(
        std::fs::read_to_string(log.join(EVENTS)).unwrap(),
        lines(true)
    );
    assert_eq!(std::fs::read(compacted.join(FRAMES)).unwrap(), frames);
}

/// An agent with no conversations carries nothing in place of its files.
#[test]
fn an_agent_without_chats_carries_its_files_as_they_are() {
    let dir = TempDir::new("archive-carry-none");
    std::fs::create_dir_all(dir.join("Agents/main/home")).unwrap();
    assert!(carry(&asked(&dir)).unwrap().replaced.is_empty());
}
