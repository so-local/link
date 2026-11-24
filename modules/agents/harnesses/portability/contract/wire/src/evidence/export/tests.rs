use super::*;
use crate::runtime::AgentKey;

fn native(seq: u64, start: u64, len: u64) -> NativeEvent {
    NativeEvent {
        seq,
        role: "evidence".into(),
        at: "".into(),
        updates: UpdateRange { start, len },
        request: None,
        model: None,
        provider: None,
        response_id: None,
        tokens: None,
        latency_ms: None,
        cost: None,
        parts: vec![],
        omissions: vec![],
        evidence: None,
    }
}

fn session() -> SessionEvidence {
    SessionEvidence {
        version: Version::V1,
        conversation: Conversation {
            agent: AgentKey {
                owner: "owner".into(),
                id: "agent".into(),
            },
            thread: "link:main".into(),
        },
        events: vec![native(1, 0, 1), native(3, 2, 0), native(4, 2, 1)],
        imported: vec![UpdateRange { start: 1, len: 1 }],
        read: ReadState::Complete,
    }
}

#[test]
fn internal_events_and_imported_updates_have_distinct_provenance() {
    let mut manifest = session();
    manifest.events[1].omissions.push(Omission::InternalMedia);
    manifest.validate(3).unwrap();
    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        serde_json::from_value::<SessionEvidence>(wire.clone()).unwrap(),
        manifest
    );
    let mut newer = wire.clone();
    newer["version"] = 2.into();
    assert!(serde_json::from_value::<SessionEvidence>(newer).is_err());
    let mut unexpected = wire;
    unexpected["trusted"] = true.into();
    assert!(serde_json::from_value::<SessionEvidence>(unexpected).is_err());
    let mut unexpected = serde_json::to_value(&manifest).unwrap();
    unexpected["events"][1]["omissions"][0]["kind"] = "unknown".into();
    assert!(serde_json::from_value::<SessionEvidence>(unexpected).is_err());
}

#[test]
fn duplicate_native_sequences_and_invalid_update_classification_are_refused() {
    let mut manifest = session();
    manifest.events[1].seq = 1;
    assert!(manifest.validate(3).is_err());
    manifest = session();
    manifest.imported[0].start = 0;
    assert!(manifest.validate(3).is_err());
    manifest = session();
    manifest.imported.clear();
    assert!(manifest.validate(3).is_err());
    manifest = session();
    manifest.events[1].updates.start = 4;
    assert!(manifest.validate(3).is_err());
    manifest = session();
    manifest.events[0].updates.len = u64::MAX;
    assert!(manifest.validate(3).is_err());
    manifest = session();
    manifest.read = ReadState::Incomplete { gaps: vec![] };
    assert!(manifest.validate(3).is_err());
}
