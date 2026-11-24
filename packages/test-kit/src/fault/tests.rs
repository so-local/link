use super::*;

#[test]
fn before_and_after_distinguish_refusal_from_a_lost_successful_reply() {
    for (moment, changed) in [(Moment::Before, 0), (Moment::After, 1)] {
        let mut fault = Once::new(moment);
        let mut effects = 0;
        let mut operation = || {
            effects += 1;
            Ok::<_, &str>(effects)
        };
        assert_eq!(
            fault.run(&mut operation, || "unavailable"),
            Err("unavailable")
        );
        fault.assert_injected();
        assert_eq!(fault.run(&mut operation, || "unavailable"), Ok(changed + 1));
        assert_eq!(effects, changed + 1);
    }
}

#[test]
fn an_operation_failure_cannot_be_counted_as_an_injected_lost_reply() {
    let mut fault = Once::new(Moment::After);
    assert_eq!(
        fault.run(|| Err::<(), _>("operation failed"), || "lost"),
        Err("operation failed")
    );
    assert!(!fault.injected());
    assert_eq!(fault.run(|| Ok(()), || "lost"), Err("lost"));
    fault.assert_injected();
}

#[test]
#[should_panic(expected = "fault was never injected")]
fn an_unreached_fault_is_not_a_passing_experiment() {
    Once::new(Moment::Before).assert_injected();
}

#[test]
fn file_damage_reports_observed_effects_and_does_not_restore_them() {
    let root = TempDir::new("fault-files");
    std::fs::create_dir(root.join(".lnk")).unwrap();
    let relative = Path::new(".lnk/state");
    let path = root.join(relative);
    std::fs::write(&path, b"abcdef").unwrap();
    let effect = FileFault::Truncate { bytes: 2 }
        .apply(&root, relative)
        .unwrap();
    assert_eq!(effect.before, 6);
    assert_eq!(effect.after, Some(2));
    assert_eq!(std::fs::read(&path).unwrap(), b"ab");
    let effect = FileFault::Remove.apply(&root, relative).unwrap();
    assert_eq!(effect.before, 2);
    assert_eq!(effect.after, None);
    assert!(!path.exists());
    assert!(
        FileFault::Remove.apply(&root, relative).is_err(),
        "absence isn't an injection"
    );
}

#[test]
fn invalid_paths_and_noop_truncations_leave_files_unchanged() {
    let root = TempDir::new("fault-refused");
    std::fs::write(root.join("state"), b"kept").unwrap();
    for path in [
        Path::new(""),
        Path::new(".."),
        Path::new("../state"),
        root.path(),
    ] {
        assert!(FileFault::Remove.apply(&root, path).is_err());
    }
    for bytes in [4, 5] {
        assert!(FileFault::Truncate { bytes }.apply(&root, "state").is_err());
    }
    assert!(FileFault::Remove.apply(&root, ".").is_err());
    assert_eq!(std::fs::read(root.join("state")).unwrap(), b"kept");
}

#[cfg(unix)]
#[test]
fn symlinks_and_shared_files_cannot_damage_another_fixture() {
    let root = TempDir::new("fault-owned");
    let other = TempDir::new("fault-other");
    std::fs::write(other.join("kept"), b"another owner").unwrap();
    std::os::unix::fs::symlink(other.path(), root.join("linked")).unwrap();
    std::os::unix::fs::symlink(other.join("kept"), root.join("file-link")).unwrap();
    std::fs::hard_link(other.join("kept"), root.join("shared")).unwrap();
    for path in ["linked/kept", "file-link", "shared"] {
        assert!(FileFault::Remove.apply(&root, path).is_err());
        assert!(FileFault::Truncate { bytes: 1 }.apply(&root, path).is_err());
    }
    assert_eq!(std::fs::read(other.join("kept")).unwrap(), b"another owner");
}
