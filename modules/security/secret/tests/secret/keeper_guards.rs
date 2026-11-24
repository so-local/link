//! A keeper install must refuse an existing user store before privilege escalation.
use super::*;

#[test]
fn a_nonempty_user_store_refuses_keeper_before_privilege() {
    let mut world = World::new("keeper-existing-file");
    std::fs::create_dir_all(world.store().parent().unwrap()).unwrap();
    let contents = format!(
        "[entry.k1m2n3p4q5r6]\nkind = \"named\"\nname = \"QA_KEPT\"\nvalue = \"{VALUE}\"\nto = []\n"
    );
    std::fs::write(world.store(), &contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(world.store(), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let sudo = world.dir.join("forbidden-sudo");
    let called = world.dir.join("privilege-was-requested");
    link_test_kit::write_program(
        &sudo,
        "#!/bin/sh\nprintf called >\"$QA_KEEPER_PRIVILEGE_MARKER\"\nexit 99\n",
    )
    .unwrap();
    world.env = vec![
        ("LNK_SUDO", sudo),
        ("QA_KEEPER_PRIVILEGE_MARKER", called.clone()),
    ];
    let out = world.run(&["keeper", "create"]);
    assert!(!out.status.success(), "keeper accepted the existing store");
    assert!(
        text(&out.stderr).contains("remove the file"),
        "{}",
        text(&out.stderr)
    );
    assert!(!called.exists(), "keeper requested privileged setup");
    assert_eq!(std::fs::read_to_string(world.store()).unwrap(), contents);
    for output in [&out.stdout, &out.stderr] {
        assert!(!text(output).contains(VALUE), "keeper exposed the value");
    }
}
