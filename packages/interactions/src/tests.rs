use crate::{approval::*, delivery::*, sources::*, *};
use std::collections::BTreeMap;

pub(crate) fn source() -> Requester {
    requester("agent", "service:m1a2i3n4s5v6", "main").unwrap()
}
pub(crate) fn draft(key: &str) -> QuestionDraft {
    QuestionDraft {
        title: "Report preferences".into(),
        caller_idempotency_key: key.into(),
        questions: vec![QuestionItem {
            id: ItemId::new("format").unwrap(),
            prompt: "Which format?".into(),
            answer_shape: AnswerShape::Choice {
                options: vec![
                    Choice {
                        id: ChoiceId::new("brief").unwrap(),
                        label: "Brief".into(),
                        description: None,
                        recommended: true,
                    },
                    Choice {
                        id: ChoiceId::new("full").unwrap(),
                        label: "Full".into(),
                        description: None,
                        recommended: false,
                    },
                ],
                min_selected: 1,
                max_selected: 1,
                allow_other: true,
            },
            default_answer: None,
        }],
        requested_timeout: Some(EffectiveTimeout {
            after_secs: 10,
            item_fallbacks: BTreeMap::new(),
            policy_revision: "test".into(),
        }),
    }
}
pub(crate) fn delivery(store: &Store, id: &RequestId, route: &str) -> DeliveryEnvelope {
    store
        .plan_delivery(id, route, "test", &render::text_capabilities(), None, 100)
        .unwrap()
}
pub(crate) fn ingress(route: &str) -> Ingress {
    Ingress {
        actor: owner(),
        route: route.into(),
        surface: "test".into(),
        input_kind: InputKind::Typed,
    }
}
pub(crate) fn candidate(envelope: &DeliveryEnvelope, text: &str, event: &str) -> ResponseCandidate {
    ResponseCandidate {
        request: envelope.question.id.clone(),
        revision: envelope.question.revision,
        delivery: envelope.delivery.id.clone(),
        value: reply::parse(&envelope.question, text, false, false, false).unwrap(),
        transport_event: TransportEventId::new(event).unwrap(),
    }
}

#[test]
fn exact_wording_selection_and_user_qualification_survive_restart() {
    let dir = link_test_kit::TempDir::new("interaction-provenance");
    let store = Store::at(&*dir);
    let mut draft = draft("report");
    draft.questions.push(QuestionItem {
        id: ItemId::new("audience").unwrap(),
        prompt: "Audience?".into(),
        answer_shape: AnswerShape::Text {
            max_bytes: 100,
            allow_empty: false,
        },
        default_answer: None,
    });
    let id = store.create(source(), owner(), draft, 100).unwrap();
    let envelope = delivery(&store, &id, "cli:main");
    let response = candidate(
        &envelope,
        "q1: choice 1\nq1: note Keep the detail precise.\n--\n  Two pages, please.\n",
        "one",
    );
    assert_eq!(
        store
            .submit(&ingress("cli:main"), response.clone(), 101)
            .unwrap(),
        SubmitDisposition::Accepted
    );
    let got = Store::at(&*dir).snapshot(&id, 200).unwrap();
    assert_eq!(got.effect, EffectState::None);
    let resolution = got.resolution.unwrap();
    assert_eq!(resolution.comment.unwrap().text, "  Two pages, please.\n");
    let ItemResult::Answered {
        selections, text, ..
    } = &resolution.items[&ItemId::new("format").unwrap()]
    else {
        panic!("not answered")
    };
    assert_eq!(selections[0].supplied_by, source().actor);
    assert_eq!(selections[0].selected_by, Some(owner()));
    assert!(selections[0].choice.recommended);
    assert_eq!(text[0].text, "Keep the detail precise.");
    assert!(matches!(
        resolution.items[&ItemId::new("audience").unwrap()],
        ItemResult::Skipped { .. }
    ));
    assert_eq!(
        store.submit(&ingress("cli:main"), response, 200).unwrap(),
        SubmitDisposition::Accepted
    );
}

#[test]
fn invalid_answers_leave_the_question_open_and_recommendations_do_not_submit() {
    let dir = link_test_kit::TempDir::new("interaction-validation");
    let store = Store::at(&*dir);
    let id = store
        .create(source(), owner(), draft("choices"), 100)
        .unwrap();
    let e = delivery(&store, &id, "chat:owner");
    for text in ["0", "1,1", "1,2", "other ", "1x", ""] {
        let value = reply::parse(&e.question, text, false, false, false);
        if let Ok(value) = value {
            let mut c = candidate(&e, "1", "invalid");
            c.value = value;
            assert!(
                store.submit(&ingress("chat:owner"), c, 101).is_err(),
                "{text}"
            );
        }
        assert!(store.snapshot(&id, 101).unwrap().resolution.is_none());
    }
    assert!(reply::parse(&e.question, "", true, false, false).is_err());
    assert!(
        store
            .submit(
                &ingress("chat:other"),
                candidate(&e, "1", "wrong-chat"),
                101
            )
            .is_err()
    );
    let mut wrong_owner = ingress("chat:owner");
    wrong_owner.actor = "user:s0m3o1n2e3e4".parse().unwrap();
    assert!(
        store
            .submit(&wrong_owner, candidate(&e, "1", "wrong-owner"), 101)
            .is_err()
    );
    assert!(
        store
            .submit(
                &ingress("chat:owner"),
                candidate(&e, "Keep it short.", "comment"),
                101
            )
            .is_ok()
    );
}

#[test]
fn creation_is_idempotent_and_publication_immutable() {
    let dir = link_test_kit::TempDir::new("interaction-idempotency");
    let store = Store::at(&*dir);
    let id = store.create(source(), owner(), draft("same"), 100).unwrap();
    assert_eq!(
        store.create(source(), owner(), draft("same"), 105).unwrap(),
        id
    );
    let mut changed = draft("same");
    changed.title = "Changed".into();
    assert!(store.create(source(), owner(), changed, 105).is_err());
    let original = delivery(&store, &id, "cli:main");
    store.cancel(&id, &source(), "edited", 102).unwrap();
    let fresh = store
        .create(source(), owner(), draft("replacement"), 103)
        .unwrap();
    let next = store
        .plan_delivery(
            &fresh,
            "cli:main",
            "test",
            &render::text_capabilities(),
            None,
            103,
        )
        .unwrap();
    assert_ne!(original.response_code, next.response_code);
    assert_ne!(id, fresh);
    assert_eq!(store.find_code(&original.response_code).unwrap(), Some(id));
}

#[test]
fn competing_surfaces_commit_one_whole_response_and_one_outbox_resolution() {
    let dir = link_test_kit::TempDir::new("interaction-race");
    let store = Store::at(&*dir);
    let id = store.create(source(), owner(), draft("race"), 100).unwrap();
    let a = delivery(&store, &id, "cli:main");
    let b = delivery(&store, &id, "chat:owner");
    std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            store
                .submit(&ingress("cli:main"), candidate(&a, "1", "a"), 101)
                .unwrap()
        });
        let two = scope.spawn(|| {
            store
                .submit(&ingress("chat:owner"), candidate(&b, "2", "b"), 101)
                .unwrap()
        });
        let outcomes = [one.join().unwrap(), two.join().unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|o| **o == SubmitDisposition::Accepted)
                .count(),
            1
        );
    });
    assert_eq!(
        store
            .events(0)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "resolved")
            .count(),
        1
    );
}

#[test]
fn timeout_is_automatic_and_cannot_fill_a_submitted_partial_set() {
    let dir = link_test_kit::TempDir::new("interaction-timeout");
    let store = Store::at(&*dir);
    let mut d = draft("fallback");
    d.requested_timeout
        .as_mut()
        .unwrap()
        .item_fallbacks
        .insert(ItemId::new("format").unwrap(), Fallback::UseRecommended);
    let id = store.create(source(), owner(), d.clone(), 100).unwrap();
    let e = delivery(&store, &id, "chat:owner");
    assert_eq!(
        store
            .submit(&ingress("chat:owner"), candidate(&e, "2", "late"), 110)
            .unwrap(),
        SubmitDisposition::Expired
    );
    let result = store.snapshot(&id, 110).unwrap().resolution.unwrap();
    let ItemResult::Answered {
        provenance,
        selections,
        ..
    } = &result.items[&ItemId::new("format").unwrap()]
    else {
        panic!("no fallback")
    };
    assert!(matches!(provenance, Provenance::TimeoutRecommended { .. }));
    assert_eq!(selections[0].selected_by, None);
    d.caller_idempotency_key = "partial".into();
    let id = store.create(source(), owner(), d, 100).unwrap();
    let e = delivery(&store, &id, "chat:owner");
    store
        .submit(
            &ingress("chat:owner"),
            candidate(&e, "Use my earlier preferences.", "partial"),
            101,
        )
        .unwrap();
    let result = store.snapshot(&id, 200).unwrap().resolution.unwrap();
    assert!(
        result
            .items
            .values()
            .all(|i| matches!(i, ItemResult::Skipped { .. }))
    );
    assert_eq!(result.trigger, Trigger::UserSubmission);
}

fn permission(store: &Store, key: &str) -> (RequestId, BoundOperation) {
    permission_at(store, key, None)
}
pub(crate) fn permission_at(
    store: &Store,
    key: &str,
    work: Option<WorkRef>,
) -> (RequestId, BoundOperation) {
    let mut source = requester("sandbox", OWNER, "main").unwrap();
    source.work = work;
    let review = PermissionReview {
        capability: "Run this tool call".into(),
        targets: vec!["example tool".into()],
        lifetime: "This call once".into(),
        consequences: vec!["The external tool may change data.".into()],
        requester_reason: None,
        exact_operation: "write(path=report, bytes=hello)".into(),
    };
    let operation = operation(
        source.source.clone(),
        b"exact payload",
        &review,
        Scope::OperationOnce,
        source.work.clone(),
    )
    .unwrap();
    let id = store
        .create_bound(
            source,
            owner(),
            DomainId::new("tool.operation").unwrap(),
            confirmation(
                "Tool operation".into(),
                "Allow this call?".into(),
                key.into(),
                std::time::Duration::from_secs(10),
            ),
            Effect::Authorize(Box::new(operation.clone())),
            Some(review),
            100,
        )
        .unwrap();
    (id, operation)
}

#[test]
fn permission_checks_claims_and_unknown_outcomes_do_not_replay() {
    let dir = link_test_kit::TempDir::new("interaction-claim");
    let store = Store::at(&*dir);
    let (id, operation) = permission(&store, "effect");
    let e = delivery(&store, &id, "cli:main");
    let mut c = candidate(&e, "yes", "allow");
    let checks = SourceChecks {
        operation: operation.clone(),
        authority_current: true,
        generation_current: true,
        control_fence_clear: true,
    };
    assert!(store.claim(&id, &checks, 101).is_err());
    store.submit(&ingress("cli:main"), c.clone(), 101).unwrap();
    let mut stale = checks.clone();
    stale.operation.payload_digest = "f".repeat(64);
    assert!(store.claim(&id, &stale, 102).is_err());
    stale = checks.clone();
    stale.control_fence_clear = false;
    assert!(store.claim(&id, &stale, 102).is_err());
    let claim = store.claim(&id, &checks, 102).unwrap();
    assert!(Store::at(&*dir).claim(&id, &checks, 102).is_err());
    store
        .report(
            &id,
            &operation.source,
            &claim,
            EffectOutcome::Unknown {
                reason: "lost connection".into(),
            },
            103,
        )
        .unwrap();
    assert!(store.claim(&id, &checks, 103).is_err());
    c.transport_event = TransportEventId::new("again").unwrap();
    assert_eq!(
        store.submit(&ingress("cli:main"), c, 103).unwrap(),
        SubmitDisposition::AlreadyResolved
    );
    let (qualified, op) = permission(&store, "qualified");
    let e = delivery(&store, &qualified, "cli:main");
    let mut c = candidate(&e, "yes", "qualified");
    let ResponseValue::Submit(s) = &mut c.value else {
        unreachable!()
    };
    s.comment = Some("Only draft files".into());
    store.submit(&ingress("cli:main"), c, 101).unwrap();
    assert_eq!(
        store.snapshot(&qualified, 102).unwrap().effect,
        EffectState::RequiresClarification
    );
    assert!(
        store
            .claim(
                &qualified,
                &SourceChecks {
                    operation: op,
                    ..checks
                },
                102
            )
            .is_err()
    );
}

#[test]
fn corrupt_future_and_linked_state_fail_closed() {
    let dir = link_test_kit::TempDir::new("interaction-state");
    let store = Store::at(dir.join("store"));
    let id = store
        .create(source(), owner(), draft("state"), 100)
        .unwrap();
    let path = store.dir().join(constants::STATE_FILE);
    let original = std::fs::read(&path).unwrap();
    let at = |schema: u32| {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value["schema"] = serde_json::json!(schema);
        serde_json::to_vec(&value).unwrap()
    };
    // A future schema.
    for bytes in [b"{".to_vec(), at(2)] {
        std::fs::write(&path, bytes).unwrap();
        assert!(store.snapshot(&id, 101).is_err());
    }
    let outside = dir.join("outside");
    std::fs::write(&outside, &original).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(store.snapshot(&id, 101).is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), original);
}

#[test]
fn strict_wire_and_surface_requirements_cannot_reduce_permissions() {
    let dir = link_test_kit::TempDir::new("interaction-wire");
    let store = Store::at(&*dir);
    let (id, _) = permission(&store, "wire");
    let mut caps = render::text_capabilities();
    caps.max_review_bytes = 1;
    assert!(
        store
            .plan_delivery(&id, "short", "test", &caps, None, 100)
            .is_err()
    );
    let mut value = serde_json::to_value(draft("draft")).unwrap();
    value["owner"] = serde_json::json!("someone");
    assert!(serde_json::from_value::<QuestionDraft>(value).is_err());
    for domain in render::domains() {
        if let Some(parent) = &domain.parent {
            let parent = render::domains().iter().find(|d| d.id == *parent).unwrap();
            assert_eq!(parent.authorization, domain.authorization);
        }
    }
}

fn target(task: &str) -> WorkRef {
    WorkRef {
        kind: WorkKind::Conversation,
        owner: owner(),
        agent: "agent-id".into(),
        session: format!("task-{task}"),
        task: Some(task.into()),
        attempt: format!("attempt-{task}"),
        generation: 1,
        control_revision: 2,
    }
}

#[test]
fn queue_retains_questions_while_steer_supersedes_only_the_exact_work() {
    use link_interactions_contract::control::*;
    let dir = link_test_kit::TempDir::new("interaction-steer");
    let store = Store::at(&*dir);
    let mut requester = source();
    requester.work = Some(target("a"));
    let a = store
        .create(requester.clone(), owner(), draft("a"), 100)
        .unwrap();
    requester.work = Some(target("b"));
    let b = store.create(requester, owner(), draft("b"), 100).unwrap();
    let state = WorkState {
        work: target("a"),
        lifecycle: Lifecycle::Open,
        pause_reasons: Default::default(),
        observed: Execution::Running,
        checkpoint: None,
        in_flight_effects: Default::default(),
    };
    let mut input = UserInput {
        id: InputId::new("queue").unwrap(),
        actor: owner(),
        target: target("a"),
        content: TextEvidence {
            text: "Use the latest instructions".into(),
            supplied_by: owner(),
            input_kind: InputKind::Typed,
        },
        admission: Admission::Queue,
        transport_event: TransportEventId::new("queue-event").unwrap(),
        submitted_at: 101,
    };
    assert_eq!(
        store.admit_input(input.clone(), &state, 101).unwrap().stage,
        ReceiptStage::Queued
    );
    assert!(store.snapshot(&a, 101).unwrap().resolution.is_none());
    input.id = InputId::new("steer").unwrap();
    input.admission = Admission::Steer;
    input.transport_event = TransportEventId::new("steer-event").unwrap();
    store.admit_input(input.clone(), &state, 101).unwrap();
    let result = Store::at(&*dir)
        .snapshot(&a, 101)
        .unwrap()
        .resolution
        .unwrap();
    assert_eq!(result.trigger, Trigger::Closed);
    assert_eq!(
        result.closed_reason.as_deref(),
        Some("superseded by steering")
    );
    assert!(store.snapshot(&b, 101).unwrap().resolution.is_none());
    store
        .record_receipt(
            input.id.as_str(),
            &input.target,
            ReceiptStage::Delivered,
            "written as input event 7",
            102,
        )
        .unwrap();
    store
        .record_receipt(
            input.id.as_str(),
            &input.target,
            ReceiptStage::Included,
            "model input event 7",
            102,
        )
        .unwrap();
    assert!(
        store
            .record_receipt(
                input.id.as_str(),
                &input.target,
                ReceiptStage::Queued,
                "late ingress",
                102
            )
            .is_err()
    );
    assert_eq!(
        store.admit_input(input, &state, 103).unwrap().stage,
        ReceiptStage::Accepted
    );
}

#[test]
fn control_batches_record_members_once_and_fence_unclaimed_permissions() {
    use link_interactions_contract::control::*;
    let dir = link_test_kit::TempDir::new("interaction-control");
    let store = Store::at(&*dir);
    let (id, operation) = permission_at(&store, "control-race", Some(target("a")));
    let e = delivery(&store, &id, "cli:main");
    store
        .submit(&ingress("cli:main"), candidate(&e, "yes", "allow"), 101)
        .unwrap();
    let mut lineage = LineageSnapshot {
        revision: 3,
        members: vec![
            LineageMember {
                work: target("root"),
                parent_attempt: None,
            },
            LineageMember {
                work: target("a"),
                parent_attempt: Some(target("root").attempt),
            },
        ],
    };
    let control = WorkControl {
        id: ControlId::new("pause-tree").unwrap(),
        actor: owner(),
        selector: ControlSelector::Tree {
            root: target("root"),
            scope: TreeScope::Descendants,
        },
        operation: ControlOperation::Pause {
            boundary: PauseBoundary::FinishModelResponse,
        },
        transport_event: TransportEventId::new("owner-pause-event").unwrap(),
    };
    let batch = store.control_work(control.clone(), &lineage, 102).unwrap();
    assert_eq!(batch.members.len(), 1);
    assert!(
        store
            .claim(
                &id,
                &SourceChecks {
                    operation,
                    authority_current: true,
                    generation_current: true,
                    control_fence_clear: true
                },
                102
            )
            .is_err()
    );
    lineage.revision += 1;
    lineage.members.push(LineageMember {
        work: target("new-child"),
        parent_attempt: Some(target("root").attempt),
    });
    assert_eq!(
        Store::at(&*dir)
            .control_work(control, &lineage, 103)
            .unwrap(),
        batch
    );
    let receipts = store.receipts(batch.members[0].id.as_str()).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].stage, ReceiptStage::Accepted);
    assert!(!receipts.iter().any(|r| r.stage == ReceiptStage::Paused));
}

#[test]
fn permission_deadlines_revoke_unclaimed_yes_but_never_revoke_applied_scope() {
    let dir = link_test_kit::TempDir::new("interaction-deadline");
    let store = Store::at(&*dir);
    let (id, operation) = permission(&store, "unclaimed");
    let e = delivery(&store, &id, "cli:main");
    store
        .submit(&ingress("cli:main"), candidate(&e, "yes", "allow"), 101)
        .unwrap();
    let expired = store.snapshot(&id, 110).unwrap();
    assert!(matches!(expired.effect, EffectState::Revoked { .. }));
    assert_eq!(sources::confirmed(&expired), Some(true));
    assert!(
        store
            .claim(
                &id,
                &SourceChecks {
                    operation,
                    authority_current: true,
                    generation_current: true,
                    control_fence_clear: true
                },
                110
            )
            .is_err()
    );
    let (id, operation) = permission(&store, "applied");
    let e = delivery(&store, &id, "cli:main");
    store
        .submit(
            &ingress("cli:main"),
            candidate(&e, "yes", "applied-allow"),
            101,
        )
        .unwrap();
    let claim = store
        .claim(
            &id,
            &SourceChecks {
                operation: operation.clone(),
                authority_current: true,
                generation_current: true,
                control_fence_clear: true,
            },
            102,
        )
        .unwrap();
    let outcome = EffectOutcome::Applied {
        detail: "resource applied".into(),
    };
    store
        .report(&id, &operation.source, &claim, outcome.clone(), 103)
        .unwrap();
    let before = store.events(0).unwrap();
    store
        .report(&id, &operation.source, &claim, outcome, 104)
        .unwrap();
    assert_eq!(store.events(0).unwrap(), before);
    assert!(matches!(
        store.snapshot(&id, 200).unwrap().effect,
        EffectState::Applied { .. }
    ));
}

#[test]
fn opinion_parentage_and_request_overrides_cannot_manufacture_permission() {
    let items = draft("opinions").questions;
    let domain = DomainId::new("question").unwrap();
    let requested = EffectiveTimeout {
        after_secs: 9,
        item_fallbacks: BTreeMap::from([(items[0].id.clone(), Fallback::UseRecommended)]),
        policy_revision: "untrusted-suggestion".into(),
    };
    let fallback = std::time::Duration::from_secs(50);
    let mut opinions = InteractionOpinions::default();
    let initial =
        effective_timeout(&opinions, &domain, &items, Some(&requested), fallback).unwrap();
    assert_eq!(initial.after_secs, 50);
    assert!(initial.item_fallbacks.is_empty());
    opinions.domains.insert(
        domain.clone(),
        DomainOpinion {
            wait_secs: Some(20),
            choice: Some(Fallback::UseRecommended),
            ..Default::default()
        },
    );
    let fixed = effective_timeout(&opinions, &domain, &items, Some(&requested), fallback).unwrap();
    assert_eq!(fixed.after_secs, 20);
    assert_ne!(fixed.policy_revision, requested.policy_revision);
    opinions
        .domains
        .get_mut(&domain)
        .unwrap()
        .allow_request_overrides = Some(true);
    assert_eq!(
        effective_timeout(&opinions, &domain, &items, Some(&requested), fallback)
            .unwrap()
            .after_secs,
        9
    );
    let parent = DomainId::new("sandbox.permission").unwrap();
    let leaf = DomainId::new("sandbox.permission.network").unwrap();
    opinions.domains.insert(
        parent.clone(),
        DomainOpinion {
            wait_secs: Some(30),
            ..Default::default()
        },
    );
    assert_eq!(
        effective_timeout(&opinions, &leaf, &items, None, fallback)
            .unwrap()
            .after_secs,
        30
    );
    opinions.domains.get_mut(&parent).unwrap().choice = Some(Fallback::UseRecommended);
    assert!(effective_timeout(&opinions, &leaf, &items, None, fallback).is_err());
    assert!(
        effective_timeout(
            &opinions,
            &DomainId::new("sandbox.permission.unknown").unwrap(),
            &items,
            None,
            fallback
        )
        .is_err()
    );
}

/// An unknown state field is refused without changing the retained bytes.
/// The error's first line names the file, the unreadable field, and the
/// outstanding operations to reconcile before replacing the store.
#[test]
fn unreadable_state_names_its_file_and_why() {
    let dir = link_test_kit::TempDir::new("interaction-unreadable");
    let store = Store::at(dir.join("store"));
    let id = store
        .create(source(), owner(), draft("state"), 100)
        .unwrap();
    let path = store.dir().join(constants::STATE_FILE);
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    let unreadable = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &unreadable).unwrap();
    let said = store.snapshot(&id, 101).unwrap_err().to_string();
    assert!(said.contains(&path.display().to_string()), "{said}");
    assert!(said.contains("unknown field `unexpected`"), "{said}");
    assert!(
        said.contains("Preserve this file before recovery"),
        "{said}"
    );
    assert!(
        said.contains("effects, deliveries, controls, and cursors"),
        "{said}"
    );
    assert!(
        said.contains("Reconcile outstanding operations before replacing state"),
        "{said}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), unreadable);
}
