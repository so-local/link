use super::*;
use link_interactions::{control::*, runtime::WorkAdmission, sources};

fn admission(store: &link_interactions::Store, child: bool) -> link_interactions::runtime::Binding {
    store
        .register_work(WorkAdmission {
            owner_route: Some("telegram:42".into()),
            kind: if child {
                link_interactions::WorkKind::Child {
                    id: "a1b2c3d4e5f6".into(),
                }
            } else {
                link_interactions::WorkKind::Conversation
            },
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: if child {
                "task-a1b2c3d4e5f6"
            } else {
                "telegram:42"
            }
            .into(),
            attempt: if child { "child" } else { "parent" }.into(),
            parent_attempt: child.then(|| "parent".into()),
            checkpoint: Some("retained".into()),
        })
        .unwrap()
}
fn control(
    store: &link_interactions::Store,
    work: &link_interactions::WorkRef,
    id: &str,
    operation: ControlOperation,
) -> AddressedControl {
    store
        .control_work(
            WorkControl {
                id: link_interactions::ControlId::new(id).unwrap(),
                actor: sources::owner(),
                selector: ControlSelector::Exact { work: work.clone() },
                operation,
                transport_event: link_interactions::TransportEventId::new(id).unwrap(),
            },
            &store.lineage().unwrap(),
            link_interactions::now(),
        )
        .unwrap()
        .members
        .remove(0)
}

#[test]
fn a_child_result_survives_a_disconnected_parent_and_keeps_its_original_destination() {
    let mut fake = Fake::new("child-outbox", serde_json::json!([]));
    let endpoint = std::mem::take(&mut fake.way_in.endpoint.endpoint);
    let owed = Owed {
        from: "task:a1b2c3d4e5f6".into(),
        answer: link_connection_contract::AnswerTo::Thread("telegram:42".into()),
        ..Owed::default()
    };
    assert!(
        fake.runtime
            .block_on(fake.way_in.answer_received(
                "task-a1b2c3d4e5f6",
                &owed,
                "A measured result",
                Some("task-a1b2c3d4e5f6/3")
            ))
            .is_err()
    );
    assert!(fake.asked.lock().unwrap().is_empty());
    fake.way_in
        .owe(
            "task-a1b2c3d4e5f6",
            &Owed {
                answer: link_connection_contract::AnswerTo::Thread("telegram:99".into()),
                ..owed.clone()
            },
        )
        .unwrap();
    fake.way_in.endpoint.endpoint = endpoint;
    fake.runtime.block_on(fake.way_in.retry_answers()).unwrap();
    link_test_kit::wait::eventually("parent answer delivered", Duration::from_secs(10), || {
        (fake.sent().len() == 1).then_some(())
    });
    assert_eq!(
        fake.asked.lock().unwrap().as_slice(),
        &[asked(
            "telegram:42",
            "task:a1b2c3d4e5f6",
            "1",
            "A measured result"
        )]
    );
    fake.runtime.block_on(fake.way_in.retry_answers()).unwrap();
    assert_eq!(fake.asked.lock().unwrap().len(), 1);
    assert_eq!(fake.sent()[0].0, "42");
}

#[test]
fn deletion_removes_a_paused_childs_queued_result_before_acknowledging_completion() {
    let fake = Fake::new("child-outbox-delete", serde_json::json!([]));
    let store = fake.way_in.interactions.as_ref().unwrap();
    admission(store, false);
    let child = admission(store, true);
    store
        .finish_work(child.token.expose(), Some("finished".into()))
        .unwrap();
    control(
        store,
        &child.state.work,
        "pause",
        ControlOperation::Pause {
            boundary: PauseBoundary::FinishModelResponse,
        },
    );
    let owed = Owed {
        from: "task:a1b2c3d4e5f6".into(),
        answer: link_connection_contract::AnswerTo::Thread("telegram:42".into()),
        ..Owed::default()
    };
    fake.way_in.owe("task-a1b2c3d4e5f6", &owed).unwrap();
    assert!(
        fake.runtime
            .block_on(fake.way_in.answer_received(
                "task-a1b2c3d4e5f6",
                &owed,
                "Private result",
                Some("task-a1b2c3d4e5f6/3")
            ))
            .is_err()
    );
    assert_eq!(
        std::fs::read_dir(fake.way_in.owed.join("pending"))
            .unwrap()
            .count(),
        1
    );
    fake.runtime.block_on(fake.way_in.retry_answers()).unwrap();
    assert!(fake.asked.lock().unwrap().is_empty());
    let current = store
        .work_status("main", "task-a1b2c3d4e5f6")
        .unwrap()
        .unwrap();
    let member = control(
        store,
        &current.work,
        "delete",
        ControlOperation::Delete {
            scope: DeleteScope::TaskRecord,
        },
    );
    store.deleted_work(&member).unwrap();
    store.deletion_applied(&member, "scheduler").unwrap();
    assert_eq!(
        store
            .work_status("main", "task-a1b2c3d4e5f6")
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Deleting
    );
    fake.way_in.delete_selected_deliveries().unwrap();
    assert_eq!(
        store
            .work_status("main", "task-a1b2c3d4e5f6")
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Deleted
    );
    assert_eq!(
        std::fs::read_dir(fake.way_in.owed.join("pending"))
            .unwrap()
            .count(),
        0
    );
    assert!(!fake.way_in.owed_path("task-a1b2c3d4e5f6").exists());
    assert!(
        fake.runtime
            .block_on(fake.way_in.answer_received(
                "task-a1b2c3d4e5f6",
                &owed,
                "Private result",
                Some("task-a1b2c3d4e5f6/3")
            ))
            .is_err()
    );
    assert_eq!(
        std::fs::read_dir(fake.way_in.owed.join("pending"))
            .unwrap()
            .count(),
        0
    );
}
