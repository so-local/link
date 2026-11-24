//! Staging policy uses an explicit clock and isolated folders, never elapsed waits.

use super::*;

const CHILD_HOME: &str = "LNK_TEST_WRITE_HOME";

#[test]
fn a_private_write_completes_under_its_callers_directory_lock() {
    let root = link_test_kit::TempDir::new("write-directory-lock");
    let child = link_test_kit::process::ChildGroup::spawn(
        link_test_kit::env::command(std::env::current_exe().unwrap(), &root)
            .args([
                "--exact",
                "maintenance::tests::directory_lock_child",
                "--ignored",
            ])
            .env(CHILD_HOME, root.path())
            .stdout(std::process::Stdio::null()),
    )
    .unwrap();
    let status = link_test_kit::wait::eventually(
        "private write while its caller holds the parent directory",
        std::time::Duration::from_secs(10),
        || child.try_status().unwrap(),
    );
    assert!(status.success(), "{status}");
    assert_eq!(
        std::fs::read(root.join("settings.toml")).unwrap(),
        b"committed"
    );
}

#[test]
#[ignore = "subprocess entry for the directory-lock regression"]
fn directory_lock_child() {
    let root =
        PathBuf::from(std::env::var_os(CHILD_HOME).expect("the parent supplies its fixture"));
    let dir = Dir::open_unlinked(&root).unwrap();
    let _held = dir.lock(libc::LOCK_EX).unwrap();
    crate::write_private(&root.join("settings.toml"), b"committed").unwrap();
}

fn staged(root: &Path, modified: u64) -> PathBuf {
    std::fs::create_dir_all(root).unwrap();
    let path = root.join(layout::temporary_name());
    let file = std::fs::File::create_new(&path).unwrap();
    file.set_times(
        std::fs::FileTimes::new()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(modified)),
    )
    .unwrap();
    path
}

#[test]
fn expired_staging_is_removed_only_from_owned_safes_without_following_links() {
    let root = link_test_kit::TempDir::new("staging-policy");
    let safe = root.join("safe");
    crate::create_private_dir(&safe).unwrap();
    let data = root.join("data");
    let now = layout::LAYOUT.temporary.max_age_seconds + 100;
    let old = staged(&safe, 1);
    let recent = staged(&safe, now);
    let data_old = staged(&data, 1);
    let target = root.join("outside");
    let target_old = staged(&target, 1);
    std::os::unix::fs::symlink(&target, safe.join("linked")).unwrap();
    let saved = safe.join("settings.toml");
    std::fs::write(&saved, "kept").unwrap();
    let mut scan = Scan::new(now, true);
    visit_root(&safe, &mut scan);
    assert!(scan.unswept.is_empty(), "{:?}", scan.unswept);
    assert!(!old.exists());
    for path in [&recent, &data_old, &target_old, &saved] {
        assert!(path.exists(), "{}", path.display());
    }
    let mut scan = Scan::new(now, false);
    visit_root(&data, &mut scan);
    assert_eq!(scan.found, std::slice::from_ref(&data_old));
    assert!(data_old.exists());
}

#[test]
fn a_sweep_skips_active_writes_and_obeys_its_persisted_period() {
    let root = link_test_kit::TempDir::new("staging-cadence");
    let safe = root.join("safe");
    crate::create_private_dir(&safe).unwrap();
    let state = root.join("maintenance.toml");
    let now = layout::LAYOUT.temporary.max_age_seconds + 100;
    let old = staged(&safe, 1);
    let writer = std::fs::File::open(&old).unwrap();
    crate::fs::lock(&writer).unwrap();
    visit_root(&safe, &mut Scan::new(now, true));
    assert!(old.exists(), "an active write was removed");
    drop(writer);
    sweep_in(std::slice::from_ref(&safe), &state, now).unwrap();
    assert!(!old.exists());
    let another = staged(&safe, 1);
    let period = layout::LAYOUT.temporary.sweep_period_seconds;
    sweep_in(std::slice::from_ref(&safe), &state, now + period - 1).unwrap();
    assert!(another.exists());
    sweep_in(&[safe], &state, now + period).unwrap();
    assert!(!another.exists());
}

/// A folder deeper than the sweep walks, or a record it can't read, is
/// left and reported: what else is expired still goes, and the sweep is
/// recorded, so it isn't met again until its period has passed.
#[test]
fn a_sweep_reports_what_it_cannot_walk_and_still_records_its_run() {
    let root = link_test_kit::TempDir::new("staging-best-effort");
    let safe = root.join("safe");
    crate::create_private_dir(&safe).unwrap();
    let mut deep = safe.join("deep");
    for _ in 0..=nofollow::DEEPEST {
        deep.push("d");
    }
    crate::create_private_dir(&deep).unwrap();
    let now = layout::LAYOUT.temporary.max_age_seconds + 100;
    let old = staged(&safe, 1);
    let state = root.join("maintenance.toml");
    std::fs::write(&state, "not a record").unwrap();
    let unswept = sweep_in(std::slice::from_ref(&safe), &state, now).unwrap();
    assert!(!old.exists());
    assert_eq!(unswept.len(), 1, "{unswept:?}");
    assert!(
        unswept[0].path.starts_with(safe.join("deep")),
        "{unswept:?}"
    );
    let another = staged(&safe, 1);
    let again = sweep_in(std::slice::from_ref(&safe), &state, now + 1).unwrap();
    assert!(again.is_empty(), "{again:?}");
    assert!(another.exists(), "the sweep ran again within its period");
}

#[test]
fn a_durable_write_replaces_the_named_file_and_cleans_up_a_failed_publication() {
    let root = link_test_kit::TempDir::new("durable-write");
    let target = root.join("outside");
    std::fs::write(&target, "kept").unwrap();
    let path = root.join("settings.toml");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    crate::write_private(&path, b"published").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"published");
    assert_eq!(std::fs::read(&target).unwrap(), b"kept");
    assert!(!path.symlink_metadata().unwrap().file_type().is_symlink());
    let folder = root.join("folder");
    std::fs::create_dir(&folder).unwrap();
    assert!(crate::write_private(&folder, b"refused").is_err());
    assert!(folder.is_dir());
    assert!(
        std::fs::read_dir(&root)
            .unwrap()
            .all(|entry| !layout::is_temporary(&entry.unwrap().file_name().to_string_lossy()))
    );
}
