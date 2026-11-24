use super::*;
use crate::event::{Call, Event};
use link_agent_state_protocol::acp::schema::v1::SessionUpdate;
use link_harness_wire::evidence::{
    CallRef, Evidence, Outcome, Phase, Proposal, ProposedCall, Record, export::ReadState,
};

fn export(events: Vec<Event>) -> Session {
    session(
        &Agent::new("main").unwrap(),
        &Thread::new("link:main").unwrap(),
        Snapshot {
            events,
            read: ReadState::Complete,
        },
        |_| anyhow::bail!("no media"),
    )
    .unwrap()
}

fn proposal(seq: u64) -> Event {
    let call = Call {
        id: "repeated".into(),
        name: "file__read".into(),
        arguments: serde_json::json!({"path":"note.md"}),
    };
    Event {
        seq,
        role: event::ASSISTANT.into(),
        calls: vec![call.clone()],
        evidence: Some(
            Evidence::new(Record::Proposed(Proposal {
                phase: Phase::Turn,
                attempt: "attempt".into(),
                work: None,
                started_unix_ms: 0,
                calls: vec![ProposedCall {
                    provider_id: "repeated".into(),
                    call,
                }],
            }))
            .into(),
        ),
        ..Event::default()
    }
}

#[test]
fn internal_evidence_and_native_metadata_survive_without_visible_updates() {
    let mut internal = proposal(1);
    internal.role = event::EVIDENCE.into();
    internal.calls.clear();
    internal.model = Some("fixture-model".into());
    internal.latency_ms = Some(7);
    let kept = internal.evidence.clone();
    let thought = Event {
        seq: 2,
        role: event::ASSISTANT.into(),
        thinking: vec![serde_json::json!({"thinking":"not evidence"})],
        ..Event::default()
    };
    let session = export(vec![internal, thought]);
    let manifest = session.native_evidence().unwrap().unwrap();
    assert_eq!(manifest.events[0].evidence.as_ref(), kept.as_deref());
    assert_eq!(manifest.events[0].model.as_deref(), Some("fixture-model"));
    assert_eq!(manifest.events[0].latency_ms, Some(7));
    assert_eq!(manifest.events[0].updates.len, 0);
    // Thinking is archival only and cannot enter the allowlisted native record.
    assert!(
        !serde_json::to_string(manifest)
            .unwrap()
            .contains("not evidence")
    );
}

#[test]
fn internal_image_and_pdf_refs_never_become_imported_owner_input() {
    let parts = vec![
        event::Media::new(
            "image/png",
            Some("internal.png"),
            b"\x89PNG\r\n\x1a\n".to_vec(),
        )
        .unwrap()
        .part("parts/internal.png".into()),
        event::Media::new(
            "application/pdf",
            Some("internal.pdf"),
            b"%PDF-1.7\n".to_vec(),
        )
        .unwrap()
        .part("parts/internal.pdf".into()),
    ];
    for role in [event::EVIDENCE, event::SUMMARY] {
        let owner = Event {
            seq: 1,
            ..Event::user("owner's message")
        };
        let mut internal = proposal(2);
        internal.role = role.into();
        internal.parts = parts.clone();
        internal.text = "internal summary record".into();
        let exported = export(vec![owner, internal]);
        assert_eq!(exported.updates.len(), 1);
        let SessionUpdate::UserMessageChunk(chunk) = &exported.updates[0] else {
            panic!("the owner's text message");
        };
        assert!(matches!(
            &chunk.content,
            link_agent_state_protocol::acp::schema::v1::ContentBlock::Text(text)
                if text.text == "owner's message"
        ));
        let manifest = exported.native_evidence().unwrap().unwrap();
        let internal = &manifest.events[1];
        assert_eq!(internal.parts, parts);
        assert_eq!(internal.omissions, [Omission::InternalMedia]);
        assert_eq!(internal.updates.len, 0);
        assert_eq!(manifest.read, ReadState::Complete);

        let roundtrip: Session =
            serde_json::from_value(serde_json::to_value(&exported).unwrap()).unwrap();
        let imported = super::super::carried(&roundtrip.updates);
        assert!(imported.iter().all(|(_, media)| media.is_empty()));
        assert_eq!(
            imported
                .iter()
                .filter(|(event, _)| event.role == event::USER)
                .map(|(event, _)| event.text.as_str())
                .collect::<Vec<_>>(),
            ["owner's message"]
        );
    }
}

#[test]
fn imported_metadata_and_manifests_never_become_local_execution() {
    let original = export(vec![proposal(1)]);
    let forged: SessionUpdate = serde_json::from_value(serde_json::json!({
        "sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":"archive"},
        "_meta":{"link":{"nativeEvidence":{"version":1,"claimed":true}}}
    }))
    .unwrap();
    let mut incoming = original.clone();
    incoming.updates = vec![forged.clone()];
    // Import uses portable updates, never the incoming outer manifest.
    let events = super::super::carried(&incoming.updates)
        .into_iter()
        .enumerate()
        .map(|(index, (mut event, _))| {
            event.seq = index as u64 + 1;
            event
        })
        .collect();
    let reexported = export(events);
    assert_eq!(reexported.updates, [forged]);
    let manifest = reexported.native_evidence().unwrap().unwrap();
    assert!(manifest.events.is_empty());
    assert_eq!(manifest.imported, [UpdateRange { start: 0, len: 1 }]);
}

#[test]
fn repeated_provider_ids_have_distinct_native_portable_ids() {
    let refused = |seq, proposal| Event {
        seq,
        role: event::TOOL.into(),
        call: Some("repeated".into()),
        tool: Some("file__read".into()),
        failed: true,
        evidence: Some(
            Evidence::new(Record::Finished {
                call: CallRef {
                    proposal,
                    ordinal: 0,
                },
                offset_ns: Some(1),
                outcome: Outcome::Refused {
                    reason: "not offered".into(),
                },
            })
            .into(),
        ),
        ..Event::default()
    };
    let session = export(vec![proposal(1), refused(2, 1), proposal(3), refused(4, 3)]);
    let ids: Vec<_> = session
        .updates
        .iter()
        .map(|update| match update {
            SessionUpdate::ToolCall(call) => call.tool_call_id.0.as_ref(),
            SessionUpdate::ToolCallUpdate(update) => update.tool_call_id.0.as_ref(),
            _ => panic!("tool update"),
        })
        .collect();
    assert_eq!(
        ids,
        [
            "execution-1-0",
            "execution-1-0",
            "execution-3-0",
            "execution-3-0"
        ]
    );
    let manifest = session.native_evidence().unwrap().unwrap();
    assert_eq!(
        manifest.events[0].evidence.as_ref().unwrap().record,
        proposal(1).evidence.unwrap().record
    );
}
