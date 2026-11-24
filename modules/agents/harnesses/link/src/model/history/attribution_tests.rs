use super::*;

fn call(id: &str) -> crate::event::Call {
    crate::event::Call {
        id: id.into(),
        name: "file__read".into(),
        arguments: serde_json::json!({"path":"skills/example/SKILL.md"}),
    }
}

fn answer(seq: u64, id: &str, text: &str) -> Event {
    Event {
        seq,
        role: event::TOOL.into(),
        call: Some(id.into()),
        text: text.into(),
        ..Default::default()
    }
}

#[test]
fn completion_order_keeps_exact_sources_through_answer_pairing() {
    let events = vec![
        Event {
            seq: 1,
            ..Event::user("Use the skill")
        },
        Event {
            seq: 2,
            role: event::ASSISTANT.into(),
            calls: vec![call("a"), call("b")],
            ..Default::default()
        },
        answer(3, "b", "second body"),
        answer(4, "a", "first body"),
    ];
    let projected = projected(&events, true, &|_| unreachable!());
    assert_eq!(
        projected.messages[2]
            .answers
            .iter()
            .map(|answer| answer.call.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    for (seq, index, text) in [(3, 1, "second body"), (4, 0, "first body")] {
        let source = projected
            .sources
            .iter()
            .find(|source| source.source == (Source::Native { seq }))
            .unwrap();
        assert_eq!(
            source.slot,
            Slot::Answer {
                index,
                call: if index == 0 { "a" } else { "b" }.into()
            }
        );
        assert_eq!(source.source_text, ContentDigest::of(text.as_bytes()));
        assert_eq!(source.projected_text, source.source_text);
        assert_eq!(source.coverage, Coverage::Whole);
    }
}

#[test]
fn explicit_input_has_an_exact_source_span_and_unanswered_content_is_synthetic() {
    let body = "Use these instructions.\nA complete skill body.\nOwner question.";
    let mut owner = Event {
        seq: 1,
        ..Event::user(body)
    };
    owner.now = Some("A contextual header".into());
    let events = vec![
        owner,
        Event {
            seq: 2,
            role: event::ASSISTANT.into(),
            calls: vec![call("missing")],
            ..Default::default()
        },
    ];
    let projected = projected(&events, true, &|_| unreachable!());
    let source = projected
        .sources
        .iter()
        .find(|source| source.source == (Source::Native { seq: 1 }))
        .unwrap();
    assert_eq!(source.source_text, ContentDigest::of(body.as_bytes()));
    let Slot::Text { start, end } = source.slot else {
        panic!("owner isn't text")
    };
    let selected = &projected.messages[source.message as usize].text[start as usize..end as usize];
    assert!(selected.contains(body));
    assert!(source.projected_text.matches(selected.as_bytes()));
    assert!(
        projected
            .sources
            .iter()
            .any(|source| source.source == Source::Synthetic
                && matches!(&source.slot, Slot::Answer { call, .. } if call == "missing"))
    );
}

#[test]
fn imported_history_never_becomes_a_native_source() {
    let event = Event {
        seq: 1,
        portable: Some(vec![]),
        ..Event::user("imported body")
    };
    let projected = projected(&[event], false, &|_| unreachable!());
    assert_eq!(projected.sources[0].source, Source::Imported { seq: 1 });
}
