use super::*;
use link_interactions::{runtime::WorkAdmission, sources, *};
use serde_json::json;

#[test]
fn source_calls_cannot_drop_host_bindings_or_reuse_a_control_revision() {
    let root = link_test_kit::TempDir::new("source-binding");
    let mut tools = Tools::new(
        "main",
        link_files::layout::Hidden::Sandboxes.at(&root),
        vec![],
    );
    tools.asking = Some(link_sandbox_client::routes::Asking {
        agent: "main".into(),
        wait_secs: 30,
    });
    let store = crate::bridges::asks::store(&tools.dir).unwrap();
    let binding = store
        .register_work(WorkAdmission {
            owner_route: Some("link:main".into()),
            kind: WorkKind::Conversation,
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: "link:main".into(),
            attempt: "one".into(),
            parent_attempt: None,
            checkpoint: Some("original".into()),
        })
        .unwrap();
    assert!(
        route::work_of(
            &tools,
            &json!({"method":"tools/call","params":{"name":"write"}})
        )
        .is_err()
    );
    assert!(
        route::work_of(&tools, &json!({"method":"tools/list"}))
            .unwrap()
            .is_none()
    );
    let call = json!({"method":"tools/call", "params":{"name":"write", "_meta":{"link.local/work":binding.token.expose(),
        "link.local/thread":"link:main", "link.local/attempt":"one", "link.local/control_revision":0}}});
    assert_eq!(
        route::work_of(&tools, &call).unwrap(),
        Some(binding.state.work)
    );
    owner::command(
        &store,
        &sources::owner(),
        "main",
        "link:main",
        "/work steer Use the new scope",
        TransportEventId::new("steer").unwrap(),
    )
    .unwrap();
    assert!(route::work_of(&tools, &call).is_err());
    store.finish_work(binding.token.expose(), None).unwrap();
    let current = store
        .work_status("main", "link:main")
        .unwrap()
        .unwrap()
        .work;
    let deletion = store
        .control_work(
            control::WorkControl {
                id: ControlId::new("delete").unwrap(),
                actor: sources::owner(),
                selector: control::ControlSelector::Exact { work: current },
                operation: control::ControlOperation::Delete {
                    scope: control::DeleteScope::Conversation,
                },
                transport_event: TransportEventId::new("delete").unwrap(),
            },
            &store.lineage().unwrap(),
            now(),
        )
        .unwrap();
    store.deleted_work(&deletion.members[0]).unwrap();
    store
        .deletion_applied(&deletion.members[0], "bridge")
        .unwrap();
    assert!(store.works("main").unwrap().is_empty());
    assert!(
        route::work_of(
            &tools,
            &json!({"method":"tools/call", "params":{"name":"write"}})
        )
        .is_err(),
        "deleting the last work record must not turn a bound caller into an independent client"
    );
}

#[test]
fn schedule_definitions_alone_do_not_require_a_runtime_binding_for_independent_calls() {
    let root = link_test_kit::TempDir::new("definition-without-runtime");
    let mut tools = Tools::new(
        "main",
        link_files::layout::Hidden::Sandboxes.at(&root),
        vec![],
    );
    tools.asking = Some(link_sandbox_client::routes::Asking {
        agent: "main".into(),
        wait_secs: 30,
    });
    let store = crate::bridges::asks::store(&tools.dir).unwrap();
    store
        .register_schedule(
            "main",
            &ScheduleId("daily".into()),
            1,
            Some("link:main".into()),
        )
        .unwrap();
    assert!(
        route::work_of(
            &tools,
            &json!({"method":"tools/call", "params":{"name":"write"}})
        )
        .unwrap()
        .is_none()
    );
}
