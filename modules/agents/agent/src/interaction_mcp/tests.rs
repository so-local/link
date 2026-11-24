use super::*;
use link_interactions::{control::*, runtime::WorkAdmission};

fn binding(store: &Store, session: &str, attempt: &str) -> runtime::Binding {
    store
        .register_work(WorkAdmission {
            owner_route: Some(session.into()),
            kind: WorkKind::Conversation,
            owner: sources::owner(),
            agent: "aaaaaaaaaaa1".into(),
            routing_agent: "main".into(),
            session: session.into(),
            attempt: attempt.into(),
            parent_attempt: None,
            checkpoint: Some("prompt".into()),
        })
        .unwrap()
}

fn metadata(binding: &runtime::Binding) -> Value {
    json!({"link.local/work": binding.token.expose(), "link.local/thread":binding.state.work.session,
        "link.local/attempt":binding.state.work.attempt, "link.local/control_revision":binding.state.work.control_revision})
}

fn control(store: &Store, work: WorkRef, id: &str, operation: ControlOperation) {
    store
        .control_work(
            WorkControl {
                id: ControlId::new(id).unwrap(),
                actor: sources::owner(),
                selector: ControlSelector::Exact { work },
                operation,
                transport_event: TransportEventId::new(id).unwrap(),
            },
            &store.lineage().unwrap(),
            now(),
        )
        .unwrap();
}

#[test]
fn question_and_proposal_actions_preserve_continuations_and_owner_holds() {
    let root = link_test_kit::TempDir::new("interaction-source-actions");
    let store = Store::at(&*root);
    let main = binding(&store, "link:main", "one");
    let other = binding(&store, "link:other", "two");
    let mut meta = metadata(&main);
    let other_meta = metadata(&other);
    let id = sources::create_question(&store, sources::QuestionCaller {
        token:&main.token, revision:0, agent:"aaaaaaaaaaa1", service:"s1s2s3s4s5s6", label:"main", session:"link:main", attempt:"one", call:"original",
    }, serde_json::from_value(json!({"title":"Report", "questions":[{"id":"focus", "prompt":"What should it emphasize?",
        "answer_shape":{"kind":"text", "max_bytes":128, "allow_empty":false}}]})).unwrap(),
        &InteractionOpinions::default(), std::time::Duration::from_secs(30), now()).unwrap();
    let call = |action: &str, args: Value, meta: &Value| {
        execute_at(&store, "aaaaaaaaaaa1", action, args, meta)
    };
    let read = call("question_read", json!({"id":id}), &meta).unwrap();
    assert_eq!(read["question"]["requester"]["work"]["attempt"], "one");
    assert!(read.get("response_code").is_none());
    for action in ["question_read", "question_cancel"] {
        assert!(call(action, json!({"id":id, "reason":"changed"}), &other_meta).is_err());
    }
    let mut forged = meta.clone();
    forged["link.local/work"] = json!("invented");
    assert!(call("question_read", json!({"id":id}), &forged).is_err());
    assert!(call("question_read", json!({"id":id, "extra":true}), &meta).is_err());
    call(
        "question_cancel",
        json!({"id":id, "reason":"Changed plan"}),
        &meta,
    )
    .unwrap();
    let result = store.snapshot(&id, now()).unwrap().resolution.unwrap();
    assert_eq!(result.trigger, Trigger::Closed);
    assert_eq!(result.closed_reason.as_deref(), Some("Changed plan"));

    let hold = HoldId::new("owner-hold").unwrap();
    control(
        &store,
        main.state.work.clone(),
        "hold",
        ControlOperation::AddHold {
            id: hold.clone(),
            terms: HoldTerms {
                children: None,
                blocked_actions: std::collections::BTreeSet::from([DelegationAction::SpawnChild]),
            },
        },
    );
    assert!(
        call("task_held", json!({}), &meta).is_err(),
        "old control revisions cannot read or change work"
    );
    let current = store
        .work_status("main", "link:main")
        .unwrap()
        .unwrap()
        .work;
    meta["link.local/control_revision"] = json!(current.control_revision);
    store
        .propose_delegation(
            meta["link.local/work"].as_str().unwrap(),
            current.control_revision,
            "job",
            DelegationAction::SpawnChild,
            Some("job".into()),
            b"exact job",
        )
        .unwrap();
    let proposals = call("task_held", json!({}), &meta).unwrap();
    assert_eq!(proposals[0]["id"], "job");
    assert_eq!(proposals[0]["state"], "held");
    assert_eq!(
        call("task_held", json!({}), &other_meta).unwrap(),
        json!([])
    );
    assert!(
        call(
            "task_revalidate",
            json!({"id":"job", "keep":true}),
            &other_meta
        )
        .is_err()
    );
    assert!(
        call("task_revalidate", json!({"id":"job", "keep":true}), &meta).is_err(),
        "a model cannot release an owner hold"
    );
    assert!(
        call(
            "task_revalidate",
            json!({"id":"job", "keep":false, "release":true}),
            &meta
        )
        .is_err()
    );
    assert!(!store.dispatch_delegation("job", b"exact job").unwrap());
    control(
        &store,
        current,
        "release",
        ControlOperation::ReleaseHold { hold },
    );
    assert!(call("task_revalidate", json!({"id":"job", "keep":true}), &meta).is_err());
    meta["link.local/control_revision"] = json!(
        store
            .work_status("main", "link:main")
            .unwrap()
            .unwrap()
            .work
            .control_revision
    );
    let kept = call("task_revalidate", json!({"id":"job", "keep":true}), &meta).unwrap();
    assert_eq!(kept["state"], "ready");
    assert!(store.dispatch_delegation("job", b"changed job").is_err());
    assert!(store.dispatch_delegation("job", b"exact job").unwrap());
}
