use super::*;
use crate::{
    approval::*,
    tests::{candidate, delivery, ingress, permission_at},
};

#[test]
fn an_unknown_source_effect_blocks_restart_and_stop_until_that_claim_is_reconciled() {
    let dir = link_test_kit::TempDir::new("unknown-control-effect");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let (id, operation) = permission_at(&store, "effect", Some(main.state.work.clone()));
    let envelope = delivery(&store, &id, "cli:main");
    store
        .submit(
            &ingress("cli:main"),
            candidate(&envelope, "yes", "answer"),
            101,
        )
        .unwrap();
    let checks = SourceChecks {
        operation: operation.clone(),
        authority_current: true,
        generation_current: true,
        control_fence_clear: true,
    };
    let claim = store.claim(&id, &checks, 101).unwrap();
    let stopped = control(&store, &main.state.work, "stop", ControlOperation::Stop);
    store.recover_work("main", "cli:main").unwrap();
    assert_eq!(
        store.works("main").unwrap()[0].state.observed,
        Execution::Unknown
    );
    assert!(store.ready_work("main").unwrap().is_empty());
    assert!(store.claim(&id, &checks, 102).is_err());
    store
        .report(
            &id,
            &operation.source,
            &claim,
            EffectOutcome::Unknown {
                reason: "connection lost".into(),
            },
            102,
        )
        .unwrap();
    assert!(
        !store
            .receipts(stopped.members[0].id.as_str())
            .unwrap()
            .iter()
            .any(|r| r.stage == ReceiptStage::Stopped)
    );
    let reopened = Store::at(&*dir);
    reopened
        .report(
            &id,
            &operation.source,
            &claim,
            EffectOutcome::Applied {
                detail: "source confirmed the original operation".into(),
            },
            103,
        )
        .unwrap();
    assert_eq!(
        reopened.works("main").unwrap()[0].state.observed,
        Execution::Stopped
    );
    assert_eq!(
        reopened
            .receipts(stopped.members[0].id.as_str())
            .unwrap()
            .last()
            .unwrap()
            .stage,
        ReceiptStage::Stopped
    );
    assert!(reopened.claim(&id, &checks, 104).is_err());
}

#[test]
fn standing_policy_adds_no_question_but_its_in_flight_execution_still_fences_stop() {
    let dir = link_test_kit::TempDir::new("policy-call-stop");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let source = SourceId::new("sandbox").unwrap();
    let application = store
        .policy_call(&main.state.work, &source, "run/command", "call", b"payload")
        .unwrap();
    assert!(
        store.pending(100).unwrap().is_empty(),
        "standing policy must never add a blanket approval"
    );
    assert!(
        store
            .policy_call(&main.state.work, &source, "run/command", "call", b"payload")
            .is_err()
    );
    let stopped = control(&store, &main.state.work, "stop", ControlOperation::Stop);
    store
        .finish_work(main.token.expose(), Some("progress".into()))
        .unwrap();
    assert_eq!(
        store.works("main").unwrap()[0].state.observed,
        Execution::Unknown
    );
    application
        .report(EffectOutcome::Applied {
            detail: "source confirmed the original call ended".into(),
        })
        .unwrap();
    assert_eq!(
        store.works("main").unwrap()[0].state.observed,
        Execution::Stopped
    );
    assert_eq!(
        store
            .receipts(stopped.members[0].id.as_str())
            .unwrap()
            .last()
            .unwrap()
            .stage,
        ReceiptStage::Stopped
    );
}

#[test]
fn prepared_children_have_selective_controls_and_no_model_runs_before_source_admission() {
    let dir = link_test_kit::TempDir::new("prepared-child-controls");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    store
        .propose_delegation(
            main.token.expose(),
            0,
            "job",
            DelegationAction::SpawnChild,
            Some("job".into()),
            b"task",
        )
        .unwrap();
    let child = store
        .works("main")
        .unwrap()
        .into_iter()
        .find(|b| b.state.work.task.as_deref() == Some("job"))
        .unwrap();
    assert_eq!(child.state.work.generation, 0);
    assert!(store.ready_work("main").unwrap().is_empty());
    let pause = control(
        &store,
        &child.state.work,
        "pause-child",
        ControlOperation::Pause {
            boundary: PauseBoundary::FinishModelResponse,
        },
    );
    assert!(!store.dispatch_delegation("job", b"task").unwrap());
    assert!(store.work_step(main.token.expose(), Step::Model).is_ok());
    let paused = store
        .works("main")
        .unwrap()
        .into_iter()
        .find(|b| b.state.work.task.is_some())
        .unwrap();
    control(
        &store,
        &paused.state.work,
        "resume-child",
        ControlOperation::Resume {
            reasons: BTreeSet::from([pause.members[0].id.clone()]),
        },
    );
    assert!(store.dispatch_delegation("job", b"task").unwrap());
    assert!(store.delegation_unstarted("job").unwrap());
    let child = admit(&store, "task-job", "child", None);
    assert_eq!(child.state.work.generation, 1);
    assert_eq!(
        store.owner_route(&child.state.work).unwrap().as_deref(),
        Some("cli:main")
    );
    assert!(!store.delegation_unstarted("job").unwrap());
    assert!(!store.dispatch_delegation("job", b"task").unwrap());
}

#[test]
fn a_start_hold_stops_runner_dispatch_without_pausing_the_coordinator() {
    let dir = link_test_kit::TempDir::new("child-start-hold");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    control(
        &store,
        &main.state.work,
        "hold",
        ControlOperation::AddHold {
            id: HoldId::new("start").unwrap(),
            terms: HoldTerms {
                children: Some(BTreeSet::from(["job".into()])),
                blocked_actions: BTreeSet::from([DelegationAction::StartChild]),
            },
        },
    );
    let work = store.work_step(main.token.expose(), Step::Model).unwrap();
    store
        .propose_delegation(
            main.token.expose(),
            work.control_revision,
            "job",
            DelegationAction::SpawnChild,
            Some("job".into()),
            b"task",
        )
        .unwrap();
    assert!(!store.dispatch_delegation("job", b"task").unwrap());
    assert!(store.work_step(main.token.expose(), Step::Model).is_ok());
    let child = store
        .works("main")
        .unwrap()
        .into_iter()
        .find(|b| b.state.work.task.as_deref() == Some("job"))
        .unwrap();
    control(
        &store,
        &child.state.work,
        "owner-start",
        ControlOperation::StartPreparedWork,
    );
    assert!(
        store.dispatch_delegation("job", b"task").unwrap(),
        "an explicit owner start authorizes only this child"
    );
    let _child = admit(&store, "task-job", "child", None);
    assert!(
        store
            .works("main")
            .unwrap()
            .into_iter()
            .find(|b| b.state.work.session == "cli:main")
            .unwrap()
            .state
            .pause_reasons
            .is_empty()
    );
    assert!(
        !store
            .change(|s| Ok(s.controls.runtime.holds[&HoldId::new("start").unwrap()].released))
            .unwrap()
    );
}

#[test]
fn schedule_and_occurrence_controls_keep_distinct_scope_and_revision() {
    let dir = link_test_kit::TempDir::new("schedule-controls");
    let store = Store::at(&*dir);
    let id = ScheduleId("briefing".into());
    store
        .register_schedule("main", &id, 1, Some("cli:main".into()))
        .unwrap();
    let occurrence = ScheduledOccurrence {
        schedule: id.clone(),
        revision: 1,
        scheduled_at: 100,
    };
    assert!(store.schedule_ready("main", &occurrence).unwrap());
    let schedule = store.works("main").unwrap().remove(0);
    let admitted = store
        .register_work(WorkAdmission {
            owner_route: None,
            kind: WorkKind::Occurrence {
                occurrence: occurrence.clone(),
            },
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: "occurrence".into(),
            attempt: "first".into(),
            parent_attempt: None,
            checkpoint: Some("retained-input".into()),
        })
        .unwrap();
    control(
        &store,
        &schedule.state.work,
        "pause-definition",
        ControlOperation::Pause {
            boundary: PauseBoundary::FinishModelResponse,
        },
    );
    assert!(!store.schedule_ready("main", &occurrence).unwrap());
    assert!(
        store.work_step(admitted.token.expose(), Step::Tool).is_ok(),
        "definition-only pause lets admitted work finish"
    );
    store
        .register_schedule("main", &id, 2, Some("cli:main".into()))
        .unwrap();
    assert!(
        store
            .work_step(admitted.token.expose(), Step::Tool)
            .is_err(),
        "changed scope revokes old occurrence authority"
    );
    assert!(
        !store
            .schedule_ready(
                "main",
                &ScheduledOccurrence {
                    revision: 2,
                    ..occurrence
                }
            )
            .unwrap(),
        "definition changes preserve owner restrictions"
    );
}

#[test]
fn task_deletion_requires_each_source_acknowledgement_and_never_removes_siblings() {
    let dir = link_test_kit::TempDir::new("task-deletion");
    let store = Store::at(&*dir);
    let _main = admit(&store, "cli:main", "one", None);
    let child = admit(&store, "task-job", "child", Some("one"));
    store
        .finish_work(child.token.expose(), Some("progress".into()))
        .unwrap();
    let deleted = control(
        &store,
        &child.state.work,
        "delete",
        ControlOperation::Delete {
            scope: DeleteScope::TaskRecord,
        },
    );
    let member = &deleted.members[0];
    store.deleted_work(member).unwrap();
    assert_eq!(
        store.control_status(&deleted.control.id).unwrap().stage,
        ReceiptStage::Accepted
    );
    Store::at(&*dir)
        .deletion_applied(member, "scheduler")
        .unwrap();
    assert_eq!(
        store.control_status(&deleted.control.id).unwrap().stage,
        ReceiptStage::Accepted
    );
    Store::at(&*dir).deletion_applied(member, "bridge").unwrap();
    assert_eq!(
        store.control_status(&deleted.control.id).unwrap().stage,
        ReceiptStage::Applied
    );
    assert_eq!(store.works("main").unwrap().len(), 1);
    assert_eq!(
        store.works("main").unwrap()[0].state.work.session,
        "cli:main"
    );
}

#[test]
fn a_steer_received_while_paused_becomes_queued_input_for_the_next_admitted_attempt() {
    let dir = link_test_kit::TempDir::new("paused-steer");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let pause = control(
        &store,
        &main.state.work,
        "pause",
        ControlOperation::Pause {
            boundary: PauseBoundary::FinishModelResponse,
        },
    );
    store
        .finish_work(main.token.expose(), Some("progress".into()))
        .unwrap();
    let paused = store.works("main").unwrap().remove(0);
    let input = UserInput {
        id: InputId::new("owner-input").unwrap(),
        actor: sources::owner(),
        target: paused.state.work.clone(),
        content: TextEvidence {
            text: "Use the new brief".into(),
            supplied_by: sources::owner(),
            input_kind: InputKind::Typed,
        },
        admission: Admission::Steer,
        transport_event: TransportEventId::new("input").unwrap(),
        submitted_at: 100,
    };
    assert_eq!(
        store
            .admit_input(input.clone(), &paused.state, 100)
            .unwrap()
            .stage,
        ReceiptStage::Queued
    );
    control(
        &store,
        &paused.state.work,
        "resume",
        ControlOperation::Resume {
            reasons: BTreeSet::from([pause.members[0].id.clone()]),
        },
    );
    let next = admit(&store, "cli:main", "two", None);
    assert!(
        store
            .waiting_inputs(next.token.expose(), false)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.waiting_inputs(next.token.expose(), true).unwrap(),
        [input]
    );
}

#[test]
fn a_bulk_control_keeps_recorded_members_and_reports_partial_source_completion() {
    let dir = link_test_kit::TempDir::new("bulk-partial");
    let store = Store::at(&*dir);
    let main = admit(&store, "cli:main", "one", None);
    let child = admit(&store, "task-job", "child", Some("one"));
    store
        .finish_work(child.token.expose(), Some("progress".into()))
        .unwrap();
    let request = WorkControl {
        id: ControlId::new("bulk-delete").unwrap(),
        actor: sources::owner(),
        selector: ControlSelector::Tree {
            root: main.state.work.clone(),
            scope: TreeScope::Subtree,
        },
        operation: ControlOperation::Delete {
            scope: DeleteScope::TaskRecord,
        },
        transport_event: TransportEventId::new("bulk").unwrap(),
    };
    let batch = store
        .control_work(request.clone(), &store.lineage().unwrap(), 100)
        .unwrap();
    assert_eq!(batch.members.len(), 2);
    assert_eq!(
        store.control_status(&request.id).unwrap().stage,
        ReceiptStage::Partial
    );
    let target = batch
        .members
        .iter()
        .find(|m| m.work.task.is_some())
        .unwrap();
    store.deleted_work(target).unwrap();
    store.deletion_applied(target, "scheduler").unwrap();
    store.deletion_applied(target, "scheduler").unwrap();
    store.deletion_applied(target, "bridge").unwrap();
    let sibling = admit(&store, "task-other", "sibling", Some("one"));
    let replay = Store::at(&*dir)
        .control_work(request.clone(), &store.lineage().unwrap(), 101)
        .unwrap();
    assert_eq!(replay, batch);
    assert!(store.work_step(sibling.token.expose(), Step::Model).is_ok());
    assert_eq!(
        store.control_status(&request.id).unwrap().stage,
        ReceiptStage::Partial
    );
}

/// A store no source shares learns a subagent's lineage from its parent
/// conversation, named with the firing: before it, the subagent's work
/// is refused; after it, admitted as the parent's child; recorded again
/// it's unchanged, and a parent with no work here is refused.
#[test]
fn a_subagent_is_admitted_once_its_parent_conversation_is_recorded() {
    let dir = link_test_kit::TempDir::new("record-child");
    let store = Store::at(&*dir);
    let parent = admit(&store, "link:main", "chat", None);
    store
        .finish_work(parent.token.expose(), Some("progress".into()))
        .unwrap();
    let child = |attempt: &str| WorkAdmission {
        owner_route: None,
        kind: WorkKind::Child { id: "abc".into() },
        owner: sources::owner(),
        agent: "main".into(),
        routing_agent: "main".into(),
        session: "task-abc".into(),
        attempt: attempt.into(),
        parent_attempt: None,
        checkpoint: None,
    };
    assert!(
        store
            .register_work(child("refused"))
            .unwrap_err()
            .to_string()
            .contains("lineage")
    );
    assert!(store.record_child("main", "link:other", "abc").is_err());
    store.record_child("main", "link:main", "abc").unwrap();
    store.record_child("main", "link:main", "abc").unwrap();
    let admitted = store.register_work(child("first")).unwrap();
    assert_eq!(admitted.parent_attempt.as_deref(), Some("chat"));
}
