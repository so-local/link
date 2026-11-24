use super::*;
use link_harness_contract::asp::schedules::{Destination, Origin, ScheduleId, When};
use link_harness_contract::asp::schedules::{DestinationOwner, WhenCron};
use link_test_kit::TempDir;
use std::cell::RefCell;

fn definition(id: &str, zone: &str) -> Schedule {
    Schedule {
        id: ScheduleId(id.into()),
        name: "Morning inbox".into(),
        when: When::Cron(WhenCron {
            cron: "30 8 * * 1-5".into(),
            zone: zone.into(),
        }),
        message: "Summarise client mail".into(),
        to: Destination::Owner(DestinationOwner {
            connection: Some("telegram".into()),
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
fn returns_and_repeated_imports_keep_identity_zone_destination_and_owner_edits() {
    let source = definition("openclaw:inbox", "Europe/Lisbon");
    let mut definitions = merge(&[], std::slice::from_ref(&source)).unwrap();
    definitions[0].message = "The owner changed this schedule".into();
    definitions[0].revision += 1;
    // OpenClaw -> Link -> Hermes -> OpenClaw -> Link, with each native
    // source reporting the same already-imported definition on retries.
    let retained = definitions.clone();
    for _ in ["link", "hermes", "openclaw", "link"] {
        definitions = merge(&definitions, std::slice::from_ref(&source)).unwrap();
        assert_eq!(definitions, retained);
    }
    let mut conflict = source.clone();
    conflict.id = ScheduleId("another-identity".into());
    assert!(merge(&definitions, &[conflict]).is_err());
}

#[test]
fn interruptions_at_each_persisted_stage_resume_without_two_firing_owners() {
    // Failure before recording, after recording but before its stage save,
    // before clearing, and after clearing but before its stage save.
    for failure in 0..4 {
        let root = TempDir::new("schedule-stage");
        let journal = root.join("handoff.json");
        let native = root.join("jobs.json");
        let archive = root.join(".lnk/imports");
        let bytes = b"{\"jobs\":[{\"id\":\"inbox\"}]}";
        std::fs::write(&native, bytes).unwrap();
        let schedules = Schedules {
            carried: vec![definition("openclaw:inbox", "Europe/Lisbon")],
            source_revision: Some(link_digest::sha256(bytes)),
            ..Default::default()
        };
        let handoff = Handoff {
            harness: "openclaw".into(),
            root: root.to_path_buf(),
            schedules,
            stage: Stage::Prepared,
            superseded: Vec::new(),
        };
        let definitions = RefCell::new(Vec::<Schedule>::new());
        link_files::write_private(&journal, &link_files::saved::print_json(&handoff).unwrap())
            .unwrap();
        let failed = RefCell::new(false);
        for attempt in 0..2 {
            let mut handoff: Handoff =
                link_files::saved::parse_json(&std::fs::read(&journal).unwrap()).unwrap();
            let stop = |point| -> anyhow::Result<()> {
                if point == failure && attempt == 0 {
                    *failed.borrow_mut() = true;
                    anyhow::bail!("interrupted");
                }
                Ok(())
            };
            let result = advance(
                &mut handoff,
                |h| {
                    stop(0)?;
                    let merged = merge(&definitions.borrow(), &h.schedules.carried)?;
                    *definitions.borrow_mut() = merged;
                    Ok(())
                },
                |h| {
                    stop(2)?;
                    assert_eq!(definitions.borrow().len(), 1);
                    link_harness_contract::harness::schedules::clear(
                        &native,
                        &archive,
                        h.schedules.source_revision.as_deref().unwrap(),
                        b"{\"jobs\":[]}",
                    )
                },
                |h| {
                    stop(if h.stage == Stage::Recorded { 1 } else { 3 })?;
                    Ok(link_files::write_private(
                        &journal,
                        &link_files::saved::print_json(h)?,
                    )?)
                },
            );
            if attempt == 0 {
                assert!(result.is_err());
            } else {
                result.unwrap();
                assert_eq!(handoff.stage, Stage::NativeCleared);
            }
            let saved: Handoff =
                link_files::saved::parse_json(&std::fs::read(&journal).unwrap()).unwrap();
            // No dispatch projection exists before the durable clear stage.
            if saved.stage == Stage::NativeCleared {
                assert_eq!(std::fs::read(&native).unwrap(), b"{\"jobs\":[]}");
            }
        }
        assert!(*failed.borrow());
        assert_eq!(definitions.borrow().len(), 1);
        assert_eq!(
            definitions.borrow()[0].when,
            When::Cron(WhenCron {
                cron: "30 8 * * 1-5".into(),
                zone: "Europe/Lisbon".into()
            })
        );
    }
}

/// A spec's schedule takes its identity from the record by its name, a
/// newer revision once its definition changes, and one the record lacks
/// is named for the spec at revision 1.
#[test]
fn the_record_gives_a_spec_schedule_its_identity_and_revision() {
    let native = definition("openclaw:inbox", "Europe/Lisbon");
    let spec = link_agent_spec::Schedule::of(&native);
    let record = vec![Kept {
        id: native.id.clone(),
        revision: 3,
        origin: native.origin.clone(),
        definition: spec.clone(),
    }];
    let same = identified(std::slice::from_ref(&spec), &record);
    assert_eq!(
        (same[0].id.clone(), same[0].revision),
        (native.id.clone(), 3)
    );
    assert_eq!(same[0].origin, native.origin);
    let mut edited = spec.clone();
    edited.message = "Summarise it all".into();
    assert_eq!(
        identified(std::slice::from_ref(&edited), &record)[0].revision,
        4
    );
    let mut new = spec;
    new.name = "Evening".into();
    let fresh = identified(&[new], &record);
    assert_eq!(
        (
            fresh[0].id.0.as_str(),
            fresh[0].revision,
            fresh[0].origin.is_none()
        ),
        ("spec:Evening", 1, true)
    );
}
