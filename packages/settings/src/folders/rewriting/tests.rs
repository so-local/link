use super::*;

#[test]
fn linked_relocation_roots_cannot_rewrite_control() {
    let home = link_test_kit::TempDir::new("relocation-root-link");
    let safe = home.join("Safe");
    std::fs::create_dir(&safe).unwrap();
    let control = safe.join("settings.toml");
    std::fs::write(&control, "/before/private").unwrap();
    let root = home.join("harness");
    std::os::unix::fs::symlink(&safe, &root).unwrap();
    assert!(super::super::rewrite_tree(&root, Path::new("/before"), Path::new("/after")).is_err());
    assert_eq!(
        std::fs::read_to_string(&control).unwrap(),
        "/before/private"
    );
}

#[test]
fn a_swapped_parent_cannot_redirect_atomic_relocation_or_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let home = link_test_kit::TempDir::new("relocation-parent-swap");
    let data = home.join("data");
    let safe = home.join("Safe");
    std::fs::create_dir(&data).unwrap();
    std::fs::create_dir(&safe).unwrap();
    let name = OsStr::new("state.toml");
    std::fs::write(data.join(name), "/before/data").unwrap();
    std::fs::set_permissions(data.join(name), std::fs::Permissions::from_mode(0o640)).unwrap();
    std::fs::write(safe.join(name), "/before/control").unwrap();
    std::fs::set_permissions(safe.join(name), std::fs::Permissions::from_mode(0o600)).unwrap();
    let held = parent(&data.join(name)).unwrap();
    let detached = home.join("detached");
    std::fs::rename(&data, &detached).unwrap();
    std::os::unix::fs::symlink(&safe, &data).unwrap();
    assert!(rewrite(&held, name, &|text| text.replace("/before", "/after")).unwrap());
    assert_eq!(
        std::fs::read_to_string(detached.join(name)).unwrap(),
        "/after/data"
    );
    assert_eq!(
        std::fs::metadata(detached.join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        std::fs::read_to_string(safe.join(name)).unwrap(),
        "/before/control"
    );
    assert_eq!(
        std::fs::metadata(safe.join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn child_links_are_skipped_and_arrived_paths_use_the_same_boundary() {
    let home = link_test_kit::TempDir::new("relocation-child-link");
    let data = home.join("data");
    let safe = home.join("Safe");
    std::fs::create_dir(&data).unwrap();
    std::fs::create_dir(&safe).unwrap();
    std::fs::write(safe.join("state"), "/old/harness").unwrap();
    std::os::unix::fs::symlink(&safe, data.join("linked-folder")).unwrap();
    std::os::unix::fs::symlink(safe.join("state"), data.join("linked-file")).unwrap();
    std::fs::write(data.join("state"), "'/old/harness/session'").unwrap();
    assert_eq!(
        rewrite_arrived(&data, "/harness", Path::new("/new/harness")).unwrap(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(data.join("state")).unwrap(),
        "'/new/harness/session'"
    );
    assert_eq!(
        std::fs::read_to_string(safe.join("state")).unwrap(),
        "/old/harness"
    );
    assert!(
        !rewrite_paths(
            &data.join("linked-file"),
            Path::new("/old"),
            Path::new("/new")
        )
        .unwrap()
    );
}

#[test]
fn oversized_files_never_become_rewrite_inputs() {
    let home = link_test_kit::TempDir::new("relocation-input-bounds");
    let data = home.join("data");
    std::fs::create_dir(&data).unwrap();
    let path = data.join("large");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(FORMAT.max_text_bytes + 1).unwrap();
    assert!(!rewrite_paths(&path, Path::new("/before"), Path::new("/after")).unwrap());
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        FORMAT.max_text_bytes + 1
    );
}

#[test]
fn shipped_relocation_limits_are_typed_and_refuse_unbounded_reads() {
    FORMAT.validate().unwrap();
    let mut format: Format = toml::from_str(include_str!("../../../relocation.toml")).unwrap();
    format.max_text_bytes = 0;
    assert!(format.validate().is_err());
    format.max_text_bytes = u64::MAX;
    assert!(format.validate().is_err());
}
