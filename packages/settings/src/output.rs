//! What every command prints, as typed blocks ([`link_format`]), with
//! this machine's `[output]` settings and its folder of records.

use link_format::*;

use std::process::ExitCode;

/// Runs a command, as [`link_format::run`] does, on this machine:
/// `[output]` from its settings under the variables, and a failure's
/// record in `~/Link/Logs`. Settings that can't be read leave the
/// defaults: output never fails on them.
pub fn run(command: impl FnOnce(&mut Out) -> Result<Done, Report>) -> ExitCode {
    link_format::run_with(machine(), command)
}

/// A `main`'s exit, for one that doesn't print blocks: an error made a
/// report, as `?` makes one in a block command, at the `main` that called
/// this, rendered with its record, and its exit code (4 for a module not
/// installed).
#[track_caller]
pub fn exit(result: anyhow::Result<()>) -> ExitCode {
    let at = std::panic::Location::caller();
    run(|_| match result {
        Ok(()) => Ok(Done::shown()),
        Err(err) => {
            let mut report = Report::from(err);
            if let Some(step) = report.trace.steps.first_mut() {
                step.at = Some(format!("{}:{}", at.file(), at.line()));
            }
            Err(report)
        }
    })
}

/// This machine, as a run reads it.
pub fn machine() -> Machine {
    let output = crate::folders::Settings::load()
        .map(|s| s.output)
        .unwrap_or_default();
    let mut machine = Machine::new(output, records());
    machine.prepare_records = Some(prepare_records);
    machine
}

/// This machine's possible folder of records, without creating it.
/// A moved or missing recorded Home never supplies a destination.
pub fn records() -> Option<std::path::PathBuf> {
    records_in(
        &link_files::try_home_dir().ok()?,
        &crate::folders::try_installation_dir().ok()?,
    )
    .ok()
}

fn records_in(
    user: &std::path::Path,
    machine: &std::path::Path,
) -> anyhow::Result<std::path::PathBuf> {
    crate::folders::record_home_in(user, machine)?;
    let locations = link_files::layout::map::load_cached(user, machine)?;
    Ok(locations.path(
        link_files::layout::map::Entry::Home(link_files::layout::Home::Logs),
        &link_files::layout::map::Roots {
            user_home: user.into(),
            hidden: machine.into(),
        },
    ))
}

fn prepare_records(dir: &std::path::Path) -> anyhow::Result<()> {
    prepare_records_in(
        &link_files::try_home_dir()?,
        &crate::folders::try_installation_dir()?,
        dir,
    )
}

fn prepare_records_in(
    user: &std::path::Path,
    machine: &std::path::Path,
    dir: &std::path::Path,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        records_in(user, machine)? == dir,
        "the failure record destination changed while preparing it"
    );
    match crate::folders::record_home_in(user, machine)? {
        crate::folders::RecordHome::DamagedLocator(home) => {
            anyhow::ensure!(home.is_dir(), "the selected Link folder is missing");
        }
        crate::folders::RecordHome::Need(expected) => {
            anyhow::ensure!(
                crate::folders::need_in(user, machine)? == expected,
                "the selected Link folder changed"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_files::layout::{Hidden, Home};

    #[test]
    fn logging_a_restored_marked_home_remembers_its_identity_before_it_moves() {
        for locator in [None, Some("schema = 1\n")] {
            let user = link_test_kit::TempDir::new("restored-home-records");
            let machine = user.join(".lnk");
            let home = user.join("Link");
            let marker = Home::Marker.at(&home);
            if !link_test_kit::gate::settings(&marker) {
                return;
            }
            let identity = link_files::layout::Marker {
                id: "restored-folder".into(),
            };
            link_files::saved::write(&marker, &identity).unwrap();
            if let Some(text) = locator {
                link_files::write_private(&Hidden::Settings.at(&machine), text.as_bytes()).unwrap();
            }
            let logs = Home::Logs.at(&home);
            prepare_records_in(&user, &machine, &logs).unwrap();
            let bug = Report::bug(sentence!("An unexpected failure."));
            assert!(
                link_format::Record::new(&bug, cmd!("lnk up"))
                    .write(&logs)
                    .unwrap()
                    .is_file()
            );
            assert_eq!(
                link_files::settings::read::<toml::Table>(&Hidden::Settings.at(&machine)).unwrap()
                    ["id"]
                    .as_str(),
                Some(identity.id.as_str())
            );
            let moved = user.join("Moved");
            std::fs::rename(&home, &moved).unwrap();
            assert!(records_in(&user, &machine).is_err());
            assert!(prepare_records_in(&user, &machine, &logs).is_err());
            assert!(!home.exists());
            assert_eq!(
                link_files::layout::read_marker(&moved).unwrap().unwrap().id,
                identity.id
            );
        }
    }

    #[test]
    fn a_damaged_locator_logs_only_in_the_existing_marked_mapped_home() {
        use link_files::layout::map::{self, Entry};
        let user = link_test_kit::TempDir::new("damaged-record-locator");
        let machine = user.join(".lnk");
        let original = user.join("Link");
        if !link_test_kit::gate::settings(&Home::Marker.at(&original)) {
            return;
        }
        crate::folders::need_in(&user, &machine).unwrap();
        let moved = user.join("Moved");
        std::fs::rename(&original, &moved).unwrap();
        map::set(&user, &machine, Entry::Link, &moved).unwrap();
        let locator = Hidden::Settings.at(&machine);
        let valid = std::fs::read(&locator).unwrap();
        let damaged = b"not = [valid TOML";
        std::fs::write(&locator, damaged).unwrap();
        let marker = Home::Marker.at(&moved);
        let marker_bytes = std::fs::read(&marker).unwrap();
        let map_path = Hidden::Map.at_default(&machine);
        let map_bytes = std::fs::read(&map_path).unwrap();
        let logs = Home::Logs.at(&moved);
        assert_eq!(records_in(&user, &machine).unwrap(), logs);
        prepare_records_in(&user, &machine, &logs).unwrap();
        let bug = Report::bug(sentence!("An unexpected failure."));
        assert!(
            link_format::Record::new(&bug, cmd!("lnk up"))
                .write(&logs)
                .unwrap()
                .is_file()
        );
        assert_eq!(std::fs::read(&locator).unwrap(), damaged);
        assert_eq!(std::fs::read(&marker).unwrap(), marker_bytes);
        assert_eq!(std::fs::read(&map_path).unwrap(), map_bytes);
        assert!(!original.exists());
        std::fs::remove_file(&marker).unwrap();
        assert!(records_in(&user, &machine).is_err());
        assert!(prepare_records_in(&user, &machine, &logs).is_err());
        assert!(!marker.exists());
        std::fs::write(&marker, &marker_bytes).unwrap();
        std::fs::write(&map_path, damaged).unwrap();
        assert!(records_in(&user, &machine).is_err());
        assert!(prepare_records_in(&user, &machine, &logs).is_err());
        assert_eq!(std::fs::read(&map_path).unwrap(), damaged);
        std::fs::write(&map_path, &map_bytes).unwrap();
        std::fs::write(&locator, &valid).unwrap();
        let replacement = link_files::layout::Marker {
            id: "another-folder".into(),
        };
        link_files::saved::write(&marker, &replacement).unwrap();
        assert!(records_in(&user, &machine).is_err());
        assert!(prepare_records_in(&user, &machine, &logs).is_err());
        assert_eq!(std::fs::read(&locator).unwrap(), valid);
        assert!(!original.exists());
        std::fs::write(&marker, &marker_bytes).unwrap();
        let outside = user.join("outside-locator.toml");
        std::fs::write(&outside, damaged).unwrap();
        std::fs::remove_file(&locator).unwrap();
        std::os::unix::fs::symlink(&outside, &locator).unwrap();
        assert!(records_in(&user, &machine).is_err());
        assert!(prepare_records_in(&user, &machine, &logs).is_err());
        assert!(locator.is_symlink());
        assert_eq!(std::fs::read(&outside).unwrap(), damaged);
    }

    #[test]
    fn the_first_failure_is_recorded_and_a_moved_home_is_never_recreated() {
        let user = link_test_kit::TempDir::new("first-failure");
        let machine = user.join(".lnk");
        let home = user.join("Link");
        let logs = Home::Logs.at(&home);
        if !link_test_kit::gate::settings(&Home::Marker.at(&home)) {
            return;
        }
        assert!(!home.exists());
        prepare_records_in(&user, &machine, &logs).unwrap();
        let report = Report::bug(sentence!("An unexpected failure."));
        let record = link_format::Record::new(&report, cmd!("lnk module remove"))
            .write(&logs)
            .unwrap();
        assert!(record.is_file());
        assert!(Home::Marker.at(&home).is_file());
        assert!(Hidden::Settings.at(&machine).is_file());
        let moved = user.join("Moved");
        std::fs::rename(&home, &moved).unwrap();
        assert!(prepare_records_in(&user, &machine, &logs).is_err());
        assert!(!home.exists());
        assert!(Home::Logs.at(&moved).is_dir());
    }
}
