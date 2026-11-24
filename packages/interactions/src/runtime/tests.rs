use super::*;
use crate::sources;

#[test]
fn observing_work_preserves_the_committed_state_file() {
    use std::os::unix::fs::MetadataExt;
    let dir = link_test_kit::TempDir::new("work-observation");
    let store = Store::at(&*dir);
    let binding = admit(&store, "cli:main", "one", None);
    let token = binding.token.expose();
    let state = std::fs::File::open(dir.join(crate::constants::STATE_FILE)).unwrap();
    let before = state.metadata().unwrap();
    assert!(!store.interrupted_work(token).unwrap());
    assert!(store.ready_work("main").unwrap().is_empty());
    assert!(store.waiting_inputs(token, false).unwrap().is_empty());
    assert_eq!(store.works("main").unwrap().len(), 1);
    assert_eq!(
        store.work_step(token, Step::Model).unwrap(),
        binding.state.work
    );
    let after = std::fs::metadata(dir.join(crate::constants::STATE_FILE)).unwrap();
    assert_eq!(
        (before.dev(), before.ino()),
        (after.dev(), after.ino()),
        "observations replaced committed state"
    );
}

#[test]
fn observing_work_refuses_corrupt_unknown_and_linked_state() {
    let dir = link_test_kit::TempDir::new("work-observation-refusal");
    let store = Store::at(&*dir);
    admit(&store, "cli:main", "one", None);
    let path = dir.join(crate::constants::STATE_FILE);
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"incomplete state").unwrap();
    assert!(store.ready_work("main").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"incomplete state");
    let mut unknown: serde_json::Value = serde_json::from_slice(&original).unwrap();
    unknown["version"] = 0.into();
    std::fs::write(&path, serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert!(store.ready_work("main").is_err());
    std::fs::remove_file(&path).unwrap();
    let target = dir.join("original.json");
    std::fs::write(&target, &original).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(store.ready_work("main").is_err());
    assert_eq!(std::fs::read(target).unwrap(), original);
}

#[test]
#[ignore = "manual same-host observation measurement"]
fn measure_work_observation() {
    let dir = link_test_kit::TempDir::new("work-observation-cost");
    let store = Store::at(&*dir);
    let binding = admit(&store, "cli:main", "one", None);
    let token = binding.token.expose();
    for n in 0..100 {
        admit(&store, &format!("chat:{n}"), &format!("attempt-{n}"), None);
    }
    let start = std::time::Instant::now();
    for _ in 0..100 {
        assert!(!store.interrupted_work(token).unwrap());
        assert!(store.ready_work("main").unwrap().is_empty());
        assert!(store.waiting_inputs(token, false).unwrap().is_empty());
    }
    eprintln!(
        "300 observations of 101 work bindings: {:.3} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

pub(crate) fn admit(store: &Store, session: &str, attempt: &str, parent: Option<&str>) -> Binding {
    store
        .register_work(admission(session, attempt, parent))
        .unwrap()
}

/// The host's admission of `attempt` of the work in `session`.
pub(crate) fn admission(session: &str, attempt: &str, parent: Option<&str>) -> WorkAdmission {
    WorkAdmission {
        owner_route: session.contains(':').then(|| session.into()),
        kind: session
            .strip_prefix("task-")
            .map_or(WorkKind::Conversation, |id| WorkKind::Child {
                id: id.into(),
            }),
        owner: sources::owner(),
        agent: "main".into(),
        routing_agent: "main".into(),
        session: session.into(),
        attempt: attempt.into(),
        parent_attempt: parent.map(str::to_owned),
        checkpoint: Some(session.into()),
    }
}
fn control(
    store: &Store,
    work: &WorkRef,
    id: &str,
    operation: ControlOperation,
) -> ResolvedControlBatch {
    store
        .control_work(
            WorkControl {
                id: ControlId::new(id).unwrap(),
                actor: sources::owner(),
                selector: ControlSelector::Exact { work: work.clone() },
                operation,
                transport_event: TransportEventId::new(id).unwrap(),
            },
            &store.lineage().unwrap(),
            100,
        )
        .unwrap()
}

#[test]
fn host_bindings_reject_forged_children_and_replaced_attempts() {
    let dir = link_test_kit::TempDir::new("work-binding");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    assert!(
        !format!("{main:?}").contains(main.token.expose()),
        "host credentials must not enter diagnostic output"
    );
    let child = admit(&store, "task-research", "child", Some("one"));
    assert!(
        store
            .authenticate_work(main.token.expose(), "main", "task-research", "child")
            .is_err()
    );
    assert!(
        store
            .authenticate_work("invented", "main", "cli:main", "one")
            .is_err()
    );
    assert_eq!(
        store
            .authenticate_work(child.token.expose(), "main", "task-research", "child")
            .unwrap(),
        child.state.work
    );
    store
        .finish_work(main.token.expose(), Some("event:4".into()))
        .unwrap();
    let replacement = admit(&store, "cli:main", "two", None);
    assert_eq!(replacement.state.work.generation, 2);
    assert!(
        store
            .authenticate_work(main.token.expose(), "main", "cli:main", "one")
            .is_err()
    );
    assert_eq!(
        store
            .lineage()
            .unwrap()
            .members
            .iter()
            .find(|m| m.work.attempt == "child")
            .unwrap()
            .parent_attempt
            .as_deref(),
        Some("two")
    );
}

#[test]
fn ordinary_pause_allows_no_follow_on_tool_and_retains_work_until_explicit_resume() {
    let dir = link_test_kit::TempDir::new("work-pause");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let batch = control(
        &store,
        &main.state.work,
        "pause",
        ControlOperation::Pause {
            boundary: PauseBoundary::FinishModelResponse,
        },
    );
    assert!(
        !store.interrupted_work(main.token.expose()).unwrap(),
        "ordinary pause lets current model finish"
    );
    assert!(store.work_step(main.token.expose(), Step::Tool).is_err());
    store
        .finish_work(main.token.expose(), Some("event:8".into()))
        .unwrap();
    let paused = store.works("main").unwrap().remove(0);
    assert_eq!(paused.state.observed, Execution::Paused);
    assert_eq!(paused.state.checkpoint.as_deref(), Some("event:8"));
    assert!(
        Store::at(&*dir)
            .work_step(main.token.expose(), Step::Model)
            .is_err()
    );
    control(
        &store,
        &paused.state.work,
        "resume",
        ControlOperation::Resume {
            reasons: BTreeSet::from([batch.members[0].id.clone()]),
        },
    );
    let next = admit(&store, "cli:main", "two", None);
    assert!(store.work_step(next.token.expose(), Step::Tool).is_ok());
}

#[test]
fn a_hold_survives_restart_and_release_still_requires_plan_revalidation() {
    let dir = link_test_kit::TempDir::new("work-hold");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let hold = HoldId::new("hold").unwrap();
    control(
        &store,
        &main.state.work,
        "hold-control",
        ControlOperation::AddHold {
            id: hold.clone(),
            terms: HoldTerms {
                children: None,
                blocked_actions: BTreeSet::from([DelegationAction::SpawnChild]),
            },
        },
    );
    let p = store
        .propose_delegation(
            main.token.expose(),
            store.works("main").unwrap()[0].state.work.control_revision,
            "job",
            DelegationAction::SpawnChild,
            Some("job".into()),
            b"exact job",
        )
        .unwrap();
    assert_eq!(p.state, ProposalState::Held);
    assert!(
        !Store::at(&*dir)
            .dispatch_delegation("job", b"exact job")
            .unwrap()
    );
    assert!(
        store
            .revalidate_delegation(
                main.token.expose(),
                current_revision(&store, "cli:main"),
                "job",
                true
            )
            .is_err()
    );
    let current = store
        .works("main")
        .unwrap()
        .into_iter()
        .find(|b| b.state.work.session == "cli:main")
        .unwrap();
    control(
        &store,
        &current.state.work,
        "release",
        ControlOperation::ReleaseHold { hold },
    );
    assert!(!store.dispatch_delegation("job", b"exact job").unwrap());
    store
        .revalidate_delegation(
            main.token.expose(),
            current_revision(&store, "cli:main"),
            "job",
            true,
        )
        .unwrap();
    assert!(store.dispatch_delegation("job", b"changed job").is_err());
    assert!(store.dispatch_delegation("job", b"exact job").unwrap());
    assert!(
        store.dispatch_delegation("job", b"exact job").unwrap(),
        "unadmitted child input may be retried"
    );
    let child = admit(&store, "task-job", "child", None);
    assert_eq!(child.parent_attempt.as_deref(), Some("one"));
    assert!(
        !store.dispatch_delegation("job", b"exact job").unwrap(),
        "an admitted child must never be replayed"
    );
}

mod lifecycle;

fn current_revision(store: &Store, session: &str) -> u64 {
    store
        .works("main")
        .unwrap()
        .into_iter()
        .find(|b| b.state.work.session == session)
        .unwrap()
        .state
        .work
        .control_revision
}

#[test]
fn tree_pause_fixes_members_without_cascading_a_parent_only_stop() {
    let dir = link_test_kit::TempDir::new("work-tree");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let child = admit(&store, "task-job", "child", Some("one"));
    control(&store, &main.state.work, "stop", ControlOperation::Stop);
    assert!(store.work_step(main.token.expose(), Step::Tool).is_err());
    assert!(store.work_step(child.token.expose(), Step::Tool).is_ok());
}

#[test]
fn a_tree_hold_has_independent_member_ids_and_fences_every_selected_coordinator() {
    let dir = link_test_kit::TempDir::new("tree-hold");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let child = admit(&store, "task-job", "child", Some("one"));
    let batch = store
        .control_work(
            WorkControl {
                id: ControlId::new("tree-hold").unwrap(),
                actor: sources::owner(),
                selector: ControlSelector::Tree {
                    root: main.state.work,
                    scope: TreeScope::Subtree,
                },
                operation: ControlOperation::AddHold {
                    id: HoldId::new("hold").unwrap(),
                    terms: HoldTerms {
                        children: None,
                        blocked_actions: BTreeSet::from([DelegationAction::SpawnChild]),
                    },
                },
                transport_event: TransportEventId::new("tree-hold").unwrap(),
            },
            &store.lineage().unwrap(),
            100,
        )
        .unwrap();
    assert_eq!(batch.members.len(), 2);
    assert_ne!(batch.members[0].operation, batch.members[1].operation);
    assert_eq!(
        store.control_status(&batch.control.id).unwrap().stage,
        ReceiptStage::HoldActive
    );
    for (token, id) in [
        (main.token.expose(), "main-job"),
        (child.token.expose(), "child-job"),
    ] {
        let work = store.work_step(token, Step::Model).unwrap();
        let proposal = store
            .propose_delegation(
                token,
                work.control_revision,
                id,
                DelegationAction::SpawnChild,
                Some(id.into()),
                b"original task",
            )
            .unwrap();
        assert_eq!(proposal.state, ProposalState::Held);
        assert!(!store.dispatch_delegation(id, b"original task").unwrap());
    }
}

#[test]
fn display_name_reuse_never_selects_another_agents_retained_work() {
    let dir = link_test_kit::TempDir::new("work-route");
    let store = Store::at(&*dir);
    let register = |id: &str, attempt: &str| {
        store
            .register_work(WorkAdmission {
                owner_route: Some("cli:main".into()),
                kind: WorkKind::Conversation,
                owner: sources::owner(),
                agent: id.into(),
                routing_agent: "main".into(),
                session: "cli:main".into(),
                attempt: attempt.into(),
                parent_attempt: None,
                checkpoint: Some("retained".into()),
            })
            .unwrap()
    };
    let old = register("stable-one", "one");
    store.finish_work(old.token.expose(), None).unwrap();
    store.register_agent_route("stable-one", "renamed").unwrap();
    register("stable-two", "two");
    assert_eq!(store.works("main").unwrap().len(), 1);
    assert_eq!(
        store.works("main").unwrap()[0].state.work.agent,
        "stable-two"
    );
    assert_eq!(
        store.works("renamed").unwrap()[0].state.work.agent,
        "stable-one"
    );
    assert_eq!(
        store
            .work_status("stable-one", "cli:main")
            .unwrap()
            .unwrap()
            .work
            .agent,
        "stable-one"
    );
}
