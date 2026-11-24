use super::*;
use std::collections::BTreeMap;

fn ingress(route: &str) -> Ingress {
    Ingress {
        actor: sources::owner(),
        route: route.into(),
        surface: "test".into(),
        input_kind: InputKind::Control,
    }
}
fn edit(e: &DeliveryEnvelope, revision: u64, value: SetSubmission) -> Control {
    Control {
        delivery: e.delivery.id.clone(),
        question_revision: 1,
        anchor: e.delivery.anchor.clone(),
        transport_event: TransportEventId::new(format!("edit-{revision}")).unwrap(),
        action: Action::Edit {
            expected_revision: revision,
            value,
        },
    }
}
fn submit_draft(e: &DeliveryEnvelope, revision: u64) -> Control {
    Control {
        delivery: e.delivery.id.clone(),
        question_revision: 1,
        anchor: e.delivery.anchor.clone(),
        transport_event: TransportEventId::new("submit").unwrap(),
        action: Action::Submit {
            draft_revision: revision,
        },
    }
}

#[test]
fn inline_drafts_are_isolated_per_delivery_and_only_explicit_submission_resolves_the_set() {
    let dir = link_test_kit::TempDir::new("interaction-forms");
    let store = Store::at(&*dir);
    let mut draft = sources::confirmation(
        "Question set".into(),
        "Include charts?".into(),
        "one".into(),
        std::time::Duration::from_secs(30),
    );
    draft.questions.push(QuestionItem {
        id: ItemId::new("audience").unwrap(),
        prompt: "Who reads it?".into(),
        answer_shape: AnswerShape::Text {
            max_bytes: 100,
            allow_empty: false,
        },
        default_answer: None,
    });
    let id = store
        .create(
            sources::requester("agent", "service:m1a2i3n4s5v6", "main").unwrap(),
            sources::owner(),
            draft,
            100,
        )
        .unwrap();
    let mut caps = render::text_capabilities();
    caps.inline = true;
    let anchor = ContextAnchor {
        container: "message".into(),
        element: Some("question-element".into()),
        content_revision: "one".into(),
    };
    let a = store
        .plan_delivery(&id, "owner-a", "test", &caps, Some(anchor.clone()), 100)
        .unwrap();
    let b = store
        .plan_delivery(
            &id,
            "owner-b",
            "test",
            &render::text_capabilities(),
            None,
            100,
        )
        .unwrap();
    let value = SetSubmission {
        answers: BTreeMap::from([(
            ItemId::new("confirmation").unwrap(),
            ItemAnswer::Confirmation { yes: true },
        )]),
        comment: Some("Include the uncertainty.".into()),
    };
    store
        .form_control(&ingress("owner-a"), edit(&a, 0, value), 101)
        .unwrap();
    assert!(store.snapshot(&id, 101).unwrap().resolution.is_none());
    assert!(
        store
            .form(&ingress("owner-b"), &b.delivery.id, 101)
            .unwrap()
            .draft
            .value
            .answers
            .is_empty()
    );
    let mut stale = submit_draft(&a, 1);
    stale.anchor.as_mut().unwrap().content_revision = "changed".into();
    assert!(store.form_control(&ingress("owner-a"), stale, 102).is_err());
    assert!(
        store
            .form_control(&ingress("owner-b"), submit_draft(&a, 1), 102)
            .is_err()
    );
    store
        .form_control(&ingress("owner-a"), submit_draft(&a, 1), 102)
        .unwrap();
    let result = store.snapshot(&id, 102).unwrap().resolution.unwrap();
    assert_eq!(result.comment.unwrap().text, "Include the uncertainty.");
    assert!(matches!(
        result.items[&ItemId::new("audience").unwrap()],
        ItemResult::Skipped { .. }
    ));
    assert_eq!(
        store
            .form_control(&ingress("owner-a"), submit_draft(&a, 1), 103)
            .unwrap(),
        Some(SubmitDisposition::Accepted)
    );
    assert!(
        store
            .form_control(
                &ingress("owner-b"),
                edit(&b, 0, SetSubmission::default()),
                103
            )
            .is_err()
    );
}

#[test]
fn changing_the_anchored_question_context_closes_every_delivery_without_changing_surrounding_content()
 {
    let dir = link_test_kit::TempDir::new("interaction-anchor");
    let store = Store::at(&*dir);
    let id = store
        .create(
            sources::requester("agent", "service:m1a2i3n4s5v6", "main").unwrap(),
            sources::owner(),
            sources::confirmation(
                "Question".into(),
                "Continue?".into(),
                "one".into(),
                std::time::Duration::from_secs(30),
            ),
            100,
        )
        .unwrap();
    let mut caps = render::text_capabilities();
    caps.inline = true;
    let anchor = ContextAnchor {
        container: "message".into(),
        element: Some("action".into()),
        content_revision: "one".into(),
    };
    let e = store
        .plan_delivery(&id, "owner", "test", &caps, Some(anchor.clone()), 100)
        .unwrap();
    store
        .invalidate_anchor(&e.delivery.id, &anchor, 101)
        .unwrap();
    assert!(
        store
            .snapshot(&id, 101)
            .unwrap()
            .resolution
            .unwrap()
            .closed_reason
            .unwrap()
            .contains("context changed")
    );
    assert!(
        store
            .form_control(
                &ingress("owner"),
                edit(&e, 0, SetSubmission::default()),
                102
            )
            .is_err()
    );
}
