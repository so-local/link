use super::*;
use crate::tests::{CAPS, EVERY_DRIVER, fire_at, job, recording, store};
use link_agent_state_protocol::schedules::WhenEvery;
use link_agent_state_protocol::schedules::{Origin, ScheduleId};

fn definition(id: &str, when: When) -> Schedule {
    Schedule {
        id: ScheduleId(id.into()),
        name: "Morning inbox".into(),
        when,
        message: "Summarise new client mail.".into(),
        to: Destination::Thread(DestinationThread {
            thread: "telegram:-42.7".into(),
        }),
        enabled: true,
        revision: 1,
        origin: Some(Origin {
            adapter: "openclaw".into(),
            job: id.into(),
        }),
    }
}

#[test]
fn imported_occurrences_fire_once_per_due_instant_on_every_driver_and_restart() {
    for wakes in EVERY_DRIVER {
        let (_dir, store) = store("portable-fire");
        let definitions = vec![definition(
            "inbox",
            When::Every(WhenEvery {
                seconds: 600,
                anchor: 617,
            }),
        )];
        install(&store, &definitions, 600).unwrap();
        install(&store, &definitions, 601).unwrap();
        assert_eq!(store.waiting(), 1);
        assert_eq!(store.jobs()[0].due, 617);
        let (driver, seen) = recording();
        assert!(fire_at(wakes, &store, &driver, &CAPS, 616).fired.is_empty());
        assert_eq!(fire_at(wakes, &store, &driver, &CAPS, 617).fired.len(), 1);
        assert!(fire_at(wakes, &store, &driver, &CAPS, 617).fired.is_empty());
        install(&store, &definitions, 618).unwrap();
        assert_eq!(store.jobs()[0].due, 1217);
        assert_eq!(fire_at(wakes, &store, &driver, &CAPS, 1217).fired.len(), 1);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].thread.starts_with("schedule-"));
        assert_ne!(seen[0].thread, seen[1].thread);
        assert_eq!(
            seen[0].answer,
            link_connection_contract::AnswerTo::OwnerThread("telegram:-42.7".into())
        );
        assert!(seen[0].from.starts_with("schedule:"));
        assert_eq!(seen[0].occurrence.as_ref().unwrap().scheduled_at, 617);
        assert_eq!(seen[1].occurrence.as_ref().unwrap().scheduled_at, 1217);
        assert_ne!(seen[0].occurrence, seen[1].occurrence);
    }
}

#[test]
fn failed_delivery_retries_the_same_occurrence_instead_of_changing_authority() {
    let (_dir, store) = store("portable-retry");
    let definitions = vec![definition(
        "inbox",
        When::Every(WhenEvery {
            seconds: 600,
            anchor: 617,
        }),
    )];
    install(&store, &definitions, 600).unwrap();
    let (driver, asked) = crate::tests::failing(1);
    fire_at(crate::tests::Wakes::Here, &store, &driver, &CAPS, 617);
    let job = store.jobs()[0].clone();
    assert_eq!(job.tries, 1);
    assert_eq!(job.portable.as_ref().unwrap().occurrence.scheduled_at, 617);
    fire_at(crate::tests::Wakes::Here, &store, &driver, &CAPS, job.due);
    assert_eq!(*asked.lock().unwrap(), 2);
}

#[test]
fn a_completed_or_cancelled_import_is_not_recreated_on_another_start() {
    let (_dir, store) = store("portable-once");
    let definitions = vec![definition("once", When::At(WhenAt { at: 617 }))];
    install(&store, &definitions, 600).unwrap();
    let (driver, _) = recording();
    fire_at(crate::tests::Wakes::Here, &store, &driver, &CAPS, 617);
    install(&store, &definitions, 618).unwrap();
    assert!(store.jobs().is_empty());
    let mut later = definitions.clone();
    later[0].revision += 1;
    later[0].when = When::At(WhenAt { at: 900 });
    install(&store, &later, 700).unwrap();
    let id = store.jobs()[0].id.clone();
    store.cancel(&id).unwrap();
    install(&store, &later, 701).unwrap();
    assert!(store.jobs().is_empty());
}

#[test]
fn pending_activation_does_not_fire_and_a_retry_recovers_it() {
    let (_dir, store) = store("portable-pending");
    let definition = definition(
        "inbox",
        When::Every(WhenEvery {
            seconds: 600,
            anchor: 617,
        }),
    );
    install(&store, std::slice::from_ref(&definition), 600).unwrap();
    let job = store.jobs()[0].clone();
    let pending = Activation {
        definition: definition.clone(),
        ready: false,
    };
    link_files::write_private(
        &path(&store, &job.id),
        &serde_json::to_vec(&pending).unwrap(),
    )
    .unwrap();
    let (driver, seen) = recording();
    assert!(
        fire_at(crate::tests::Wakes::Here, &store, &driver, &CAPS, 617)
            .fired
            .is_empty()
    );
    install(&store, &[definition], 600).unwrap();
    assert_eq!(
        fire_at(crate::tests::Wakes::Here, &store, &driver, &CAPS, 617)
            .fired
            .len(),
        1
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn invalid_revision_or_corrupt_activation_fails_without_overwriting_jobs() {
    let (_dir, store) = store("portable-invalid");
    let definition = definition(
        "inbox",
        When::Every(WhenEvery {
            seconds: 600,
            anchor: 617,
        }),
    );
    install(&store, std::slice::from_ref(&definition), 600).unwrap();
    let jobs = store.jobs();
    let mut changed = definition.clone();
    changed.message = "a changed message".into();
    assert!(install(&store, &[changed], 600).is_err());
    assert_eq!(store.jobs(), jobs);
    std::fs::write(path(&store, &jobs[0].id), b"{broken").unwrap();
    assert!(install(&store, &[definition], 600).is_err());
    let (driver, seen) = recording();
    fire_at(crate::tests::Wakes::Here, &store, &driver, &CAPS, 617);
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn deleting_a_definition_cancels_only_its_projected_jobs() {
    let (_dir, store) = store("portable-delete");
    install(
        &store,
        &[definition(
            "inbox",
            When::Every(WhenEvery {
                seconds: 600,
                anchor: 617,
            }),
        )],
        600,
    )
    .unwrap();
    store.put(&job("aaaaaaaaaaxx", 700)).unwrap();
    install(&store, &[], 600).unwrap();
    assert_eq!(store.jobs().len(), 1);
    assert!(store.jobs()[0].portable.is_none());
}
