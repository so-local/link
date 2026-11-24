use super::*;
fn work(id: &str) -> WorkRef {
    WorkRef {
        kind: WorkKind::Conversation,
        owner: link_interactions_contract::Actor::Owner,
        agent: "agent-id".into(),
        session: id.into(),
        task: Some(id.into()),
        attempt: id.into(),
        generation: 1,
        control_revision: 1,
    }
}
fn state(id: &str) -> WorkState {
    WorkState {
        work: work(id),
        lifecycle: Lifecycle::Open,
        pause_reasons: BTreeSet::new(),
        observed: Execution::Running,
        checkpoint: Some("kept-progress".into()),
        in_flight_effects: BTreeSet::new(),
    }
}
fn control(state: &WorkState, id: &str, operation: ControlOperation) -> AddressedControl {
    AddressedControl {
        id: ControlId::new(id).unwrap(),
        batch: BatchId::new("batch").unwrap(),
        work: state.work.clone(),
        operation,
    }
}
#[test]
fn recursive_selectors_capture_members_and_deduplicate_selected_work() {
    let tree = LineageSnapshot {
        revision: 7,
        members: vec![
            LineageMember {
                work: work("root"),
                parent_attempt: None,
            },
            LineageMember {
                work: work("a"),
                parent_attempt: Some("root".into()),
            },
            LineageMember {
                work: work("b"),
                parent_attempt: Some("a".into()),
            },
            LineageMember {
                work: work("peer"),
                parent_attempt: None,
            },
        ],
    };
    let owner = work("root").owner;
    for (scope, expected) in [
        (TreeScope::SelfOnly, vec!["root"]),
        (TreeScope::Descendants, vec!["a", "b"]),
        (TreeScope::Subtree, vec!["a", "b", "root"]),
    ] {
        let selected = select(
            &owner,
            &ControlSelector::Tree {
                root: work("root"),
                scope,
            },
            &tree,
        )
        .unwrap();
        let mut actual: Vec<_> = selected.iter().map(|w| w.attempt.as_str()).collect();
        actual.sort();
        assert_eq!(actual, expected);
    }
    let selector = ControlSelector::Selected {
        members: vec![work("b"), work("a"), work("a")],
    };
    assert_eq!(select(&owner, &selector, &tree).unwrap().len(), 2);
    let mut stale = work("a");
    stale.generation += 1;
    assert!(select(&owner, &ControlSelector::Exact { work: stale }, &tree).is_err());
    assert!(select(&"user:o1t2h3e4r5u6".parse().unwrap(), &selector, &tree).is_err());
}
#[test]
fn independent_pauses_preserve_progress_and_resume_only_named_reasons() {
    let original = state("a");
    let first = transition(
        &original,
        &control(
            &original,
            "first",
            ControlOperation::Pause {
                boundary: PauseBoundary::FinishModelResponse,
            },
        ),
    )
    .unwrap();
    assert!(!can_progress(&first));
    assert_eq!(first.observed, Execution::Pausing);
    assert_eq!(first.checkpoint, original.checkpoint);
    assert_eq!(first.lifecycle, Lifecycle::Open);
    let second = transition(
        &first,
        &control(
            &first,
            "second",
            ControlOperation::Pause {
                boundary: PauseBoundary::InterruptCurrentStep,
            },
        ),
    )
    .unwrap();
    let resumed = transition(
        &second,
        &control(
            &second,
            "resume",
            ControlOperation::Resume {
                reasons: BTreeSet::from([ControlId::new("first").unwrap()]),
            },
        ),
    )
    .unwrap();
    assert_eq!(
        resumed.pause_reasons,
        BTreeSet::from([ControlId::new("second").unwrap()])
    );
    assert!(!can_progress(&resumed));
}
#[test]
fn holds_survive_attempt_changes_and_release_does_not_resume_work() {
    let mut host = state("parent");
    let hold = Hold {
        id: HoldId::new("hold").unwrap(),
        imposed_by: host.work.owner.clone(),
        coordinator: host.work.clone(),
        terms: HoldTerms {
            children: None,
            blocked_actions: BTreeSet::from([
                DelegationAction::SpawnChild,
                DelegationAction::StartChild,
                DelegationAction::DispatchChildMessage,
            ]),
        },
        released: false,
    };
    host.work.attempt = "replacement".into();
    host.work.generation += 1;
    assert_eq!(
        blocked_by(
            std::slice::from_ref(&hold),
            &host.work,
            Some("child"),
            &DelegationAction::StartChild
        ),
        vec![hold.id.clone()]
    );
    assert!(release(&hold, &"service:p1a2r3e4n5t6".parse().unwrap()).is_err());
    let next = release(&hold, &host.work.owner).unwrap();
    host.pause_reasons.insert(ControlId::new("pause").unwrap());
    assert!(
        blocked_by(
            &[next],
            &host.work,
            Some("child"),
            &DelegationAction::StartChild
        )
        .is_empty()
    );
    assert!(!can_progress(&host));
    assert!(!hold.released);
}
#[test]
fn archive_unarchive_and_delete_are_distinct_and_do_not_clear_other_restrictions() {
    let original = state("a");
    let mut archived = transition(
        &original,
        &control(&original, "archive", ControlOperation::Archive),
    )
    .unwrap();
    assert_eq!(archived.lifecycle, Lifecycle::Archiving);
    assert!(!can_progress(&archived));
    assert!(
        transition(
            &archived,
            &control(&archived, "return", ControlOperation::Unarchive)
        )
        .is_err()
    );
    archived.lifecycle = Lifecycle::Archived;
    archived.observed = Execution::Stopped;
    let returned = transition(
        &archived,
        &control(&archived, "return", ControlOperation::Unarchive),
    )
    .unwrap();
    assert_eq!(returned.lifecycle, Lifecycle::Open);
    assert_eq!(returned.observed, Execution::Paused);
    assert_eq!(returned.checkpoint, original.checkpoint);
    assert!(!returned.pause_reasons.is_empty());
    let deleted = transition(
        &returned,
        &control(
            &returned,
            "delete",
            ControlOperation::Delete {
                scope: DeleteScope::Conversation,
            },
        ),
    )
    .unwrap();
    assert_eq!(deleted.lifecycle, Lifecycle::Deleting);
    let mut done = deleted;
    done.lifecycle = Lifecycle::Deleted;
    assert!(
        transition(
            &done,
            &control(&done, "return", ControlOperation::Unarchive)
        )
        .is_err()
    );
}
