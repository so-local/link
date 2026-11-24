use super::*;

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    unsandboxed: Option<bool>,
}

#[test]
fn typed_settings_distinguish_missing_invalid_and_untrusted_documents() {
    use std::os::unix::fs::PermissionsExt;
    let dir = link_test_kit::TempDir::under(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
        "typed-settings",
    );
    let path = dir.join("settings.toml");
    if !link_test_kit::gate::settings(&path) {
        return;
    }
    assert!(read_optional::<Settings>(&path).unwrap().is_none());
    assert_eq!(read::<Settings>(&path).unwrap().unsandboxed, None);
    crate::write_private(&path, b"unsandboxed = false\n").unwrap();
    assert_eq!(read::<Settings>(&path).unwrap().unsandboxed, Some(false));
    for invalid in ["unsandboxed = 'false'\n", "unknown = false\n", "not toml"] {
        crate::write_private(&path, invalid.as_bytes()).unwrap();
        assert!(read::<Settings>(&path).is_err());
    }
    crate::write_private(&path, b"unsandboxed = true\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(read::<Settings>(&path).is_err());
    assert!(lock(&path).is_err());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o666
    );
    std::fs::remove_file(&path).unwrap();
    let others_write = std::fs::metadata(&*dir).unwrap().permissions().mode() | 0o022;
    std::fs::set_permissions(&*dir, std::fs::Permissions::from_mode(others_write)).unwrap();
    assert!(read::<Settings>(&path).is_err());
    assert!(lock(&path).is_err());
}

#[test]
fn control_settings_cannot_follow_data_files_or_directories() {
    let dir = link_test_kit::TempDir::under(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
        "settings-data-links",
    );
    let safe = dir.join("Safe");
    let data = dir.join("data");
    crate::create_private_dir(&safe).unwrap();
    crate::create_private_dir(&data).unwrap();
    if !link_test_kit::gate::settings(&safe.join("settings.toml")) {
        return;
    }
    let target = data.join("settings.toml");
    crate::write_private(&target, b"unsandboxed = true\n").unwrap();
    let linked = safe.join("settings.toml");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    assert!(read::<Settings>(&linked).is_err());
    assert!(lock(&linked).is_err());
    let parent = safe.join("agent");
    std::os::unix::fs::symlink(&data, &parent).unwrap();
    assert!(read::<Settings>(&parent.join("settings.toml")).is_err());
    assert!(read_optional::<Settings>(&parent.join("missing.toml")).is_err());
    assert!(lock(&parent.join("missing.toml")).is_err());
    assert!(!data.join("missing.toml.lock").exists());
    let hard = safe.join("hard.toml");
    std::fs::hard_link(&target, &hard).unwrap();
    assert!(read::<Settings>(&hard).is_err());
    assert!(lock(&hard).is_err());
    assert!(read::<Settings>(&target).is_err());
    std::fs::remove_file(&hard).unwrap();
    assert_eq!(read::<Settings>(&target).unwrap().unsandboxed, Some(true));
    crate::create_private_dir(&data.join("child")).unwrap();
    assert_eq!(
        read::<Settings>(&data.join("child/../settings.toml"))
            .unwrap()
            .unsandboxed,
        Some(true)
    );
    assert!(read::<Settings>(&safe.join("agent/../settings.toml")).is_err());
}

const LINKED_HOME: &str = "LNK_TEST_LINKED_HOME_REAL";

/// A home reached through a link, as `/home` is `var/home` on Fedora
/// Atomic, has its settings read where `HOME` names them: the link above
/// it is resolved once, every folder on the real way still checked, and a
/// link inside its Safe still refused.
#[test]
fn settings_are_read_in_a_home_reached_through_a_link() {
    let dir = link_test_kit::TempDir::under(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
        "settings-linked-home",
    );
    let real = dir.join("real");
    let safe = real.join("home/.lnk/safe");
    crate::create_private_dir(&safe).unwrap();
    if !link_test_kit::gate::settings(&safe.join("home.toml")) {
        return;
    }
    crate::write_private(&safe.join("home.toml"), b"unsandboxed = false\n").unwrap();
    std::os::unix::fs::symlink(&real, dir.join("linked")).unwrap();
    let home = dir.join("linked/home");
    let child = link_test_kit::process::ChildGroup::spawn(
        link_test_kit::env::command(std::env::current_exe().unwrap(), &home)
            .args(["--exact", "settings::tests::linked_home_child", "--ignored"])
            .env(LINKED_HOME, &real)
            .stdout(std::process::Stdio::null()),
    )
    .unwrap();
    let status = link_test_kit::wait::eventually(
        "settings read in a linked home",
        std::time::Duration::from_secs(10),
        || child.try_status().unwrap(),
    );
    assert!(status.success(), "{status}");
}

#[test]
#[ignore = "subprocess entry for the linked-home regression"]
fn linked_home_child() {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("the parent sets HOME"));
    let safe = home.join(".lnk/safe");
    let read_here = read::<Settings>(&safe.join("home.toml")).unwrap();
    assert_eq!(read_here.unsandboxed, Some(false));
    assert!(lock(&safe.join("home.toml")).is_ok());
    // Below the home, a link is refused as before.
    let real = std::path::PathBuf::from(std::env::var_os(LINKED_HOME).expect("the parent sets it"));
    let data = real.join("home/.lnk/data");
    crate::create_private_dir(&data).unwrap();
    crate::write_private(&data.join("home.toml"), b"unsandboxed = true\n").unwrap();
    std::os::unix::fs::symlink(&data, home.join(".lnk/safe/linked")).unwrap();
    assert!(read::<Settings>(&home.join(".lnk/safe/linked/home.toml")).is_err());
}
