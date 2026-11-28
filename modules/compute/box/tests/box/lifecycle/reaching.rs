//! A box reached: one SSH connection shared, its cloud's tunnel, its keys
//! kept at a new address or trusted on first contact.

use super::*;

/// The several `lnk box run` a move makes share one SSH connection, which
/// closes with the box.
#[test]
fn runs_on_a_box_share_one_connection() {
    // The home plus the mux directory, SSH's hash and its temporary
    // suffix must fit, even when the suite's state folder is longer.
    let temp = TempDir::short_with_limit("", 23);
    let home = temp.to_path_buf();
    let Some(world) = World::at(temp, home, true) else {
        return;
    };
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "b1",
        "--yes",
        "--lnk.login=false",
    ]);
    for _ in 0..3 {
        world.lnk_box(&["box", "run", "b1", "--", "--version"]);
    }
    let ssh = world.read("ssh.log");
    let opened: Vec<_> = ssh
        .lines()
        .filter(|l| l.contains("ControlMaster=yes"))
        .collect();
    assert_eq!(opened.len(), 1, "{ssh}");
    assert!(opened[0].contains("ControlPersist=60"), "{ssh}");
    assert!(opened[0].contains("-N -f"), "{ssh}");
    let runs: Vec<_> = ssh
        .lines()
        .filter(|l| l.ends_with(r#""$HOME/.lnk/exec/bin/lnk" '--version'"#))
        .collect();
    assert_eq!(runs.len(), 3, "{ssh}");
    let mux = world.home.join(".lnk/safe/boxes/mux");
    // In quotes, one value to ssh whatever the path holds.
    let socket = format!("ControlPath=\"{}/%C\"", mux.display());
    assert!(
        runs.iter()
            .all(|l| l.contains("ControlMaster=no") && l.contains(&socket)),
        "{ssh}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&mux).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    world.lnk_box(&["box", "delete", "b1", "--yes"]);
    assert!(
        world
            .read("ssh.log")
            .contains("-O exit -- ubuntu@192.0.2.10")
    );
    assert!(!world.dir.join("master").exists());
}

/// With `LNK_BOX_SSH=tunnel` (a cloud session, which can't reach port
/// 22), every connection to a box goes through its cloud's tunnel: its
/// adapter's `tunnel`, as ssh's ProxyCommand, and its keys stay pinned
/// under its name, its cloud letting nothing in. Any other is refused.
#[test]
fn a_box_is_reached_through_its_clouds_tunnel() {
    let Some(world) = World::new("tunnel", true) else {
        return;
    };
    let tunnel = [("LNK_BOX_SSH", "tunnel")];
    let b1: Vec<_> = "box create --cloud acct --name b1 --yes --lnk.login=false"
        .split(' ')
        .collect();
    assert!(world.run_with(&b1, &tunnel).status.success());
    assert!(world.read("adapter.log").contains(r#""tunnel":true"#));
    let ran = world.run_with(&["box", "ssh", "b1", "--", "true"], &tunnel);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let ssh = world.read("ssh.log");
    // The last connection that ran `true` there: `box ssh`'s.
    let ran = |ssh: &str| {
        ssh.lines()
            .rfind(|l| l.ends_with("-- ubuntu@192.0.2.10 true"))
            .unwrap()
            .to_string()
    };
    let last = ran(&ssh);
    let proxy = format!(
        "-o ProxyCommand='{}' 'tunnel' '--cloud' 'acct' '--zone' 'moon-1' '--id' 'm-1' ",
        world.bin.join("lnk-fake").display()
    );
    assert!(last.contains(&proxy), "{ssh}");
    assert!(last.contains("-o HostKeyAlias=lnk-fake-m-1 "), "{ssh}");
    // Without it, straight to port 22.
    world.lnk_box(&["box", "ssh", "b1", "--", "true"]);
    assert!(!ran(&world.read("ssh.log")).contains("ProxyCommand"));
    let refused = world.run_with(
        &["box", "ssh", "b1", "--", "true"],
        &[("LNK_BOX_SSH", "ssm")],
    );
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("direct (port 22) or tunnel"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    // A second box given the same address takes it: the first, stopped
    // since as far as Link knows, has none until its cloud says, so a
    // tunnel never goes to the wrong one.
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "b2",
        "--yes",
        "--lnk.login=false",
    ]);
    // b2, made without it, is a box its cloud lets SSH in to.
    let tunnels = world
        .read("adapter.log")
        .matches(r#""tunnel":true"#)
        .count();
    assert_eq!(tunnels, 1);
    let two = world.run_with(&["box", "ssh", "b1", "--", "true"], &tunnel);
    assert!(!two.status.success());
    assert!(
        String::from_utf8_lossy(&two.stderr).contains("b1, which has no address yet"),
        "{}",
        String::from_utf8_lossy(&two.stderr)
    );
}

/// A box whose address changed (restarted from the cloud's console, or
/// another computer) is still checked against the keys it had: they're
/// kept under its name, not its address. Keys kept under an address
/// before are kept under its name too.
#[test]
fn a_box_at_a_new_address_keeps_its_keys() {
    let Some(world) = World::new("moved", true) else {
        return;
    };
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "b1",
        "--yes",
        "--lnk.login=false",
    ]);
    let known = world.home.join(".lnk/safe/boxes/known_hosts");
    let pinned = std::fs::read_to_string(&known).unwrap();
    // The cloud says it's somewhere else now.
    std::fs::write(
        world.dir.join("cloud.json"),
        r#"{"boxes": [{"id": "m-1", "name": "b1", "place": "moon-1", "state": "running",
            "ip": "192.0.2.30", "size": "tiny"}]}"#,
    )
    .unwrap();
    world.lnk_box(&["box", "list", "--json"]);
    world.lnk_box(&["box", "ssh", "b1", "--", "uptime"]);
    let last = world.read("ssh.log");
    let last = last.lines().rfind(|l| l.contains("uptime")).unwrap();
    assert!(
        last.contains("-o HostKeyAlias=lnk-fake-m-1 ") && last.contains("ubuntu@192.0.2.30"),
        "{last}"
    );
    assert_eq!(std::fs::read_to_string(&known).unwrap(), pinned);

    // Pinned by address, as Link did before: kept under its name too.
    std::fs::write(&known, format!("192.0.2.30 ssh-ed25519 {KEY}\n")).unwrap();
    world.lnk_box(&["box", "ssh", "b1", "--", "uptime"]);
    let now = std::fs::read_to_string(&known).unwrap();
    assert!(
        now.contains(&format!("lnk-fake-m-1 ssh-ed25519 {KEY}")),
        "{now}"
    );
}

#[test]
fn without_its_console_a_box_is_trusted_on_first_contact() {
    let Some(world) = World::new("console", false) else {
        return;
    };
    let made = world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "b1",
        "--yes",
        "--lnk.login=false",
    ]);
    let stderr = String::from_utf8_lossy(&made.stderr);
    assert!(
        stderr.contains("Its host key is trusted on first contact instead"),
        "{stderr}"
    );
    assert!(String::from_utf8_lossy(&made.stdout).contains("Your box b1 is up"));
    assert!(!world.home.join(".lnk/safe/boxes/known_hosts").exists());
}

/// A box made here before certificates is given them the next time Link
/// reaches it, in with this computer's key: it trusts the authority for
/// its own principal, a certificate alone gets in, and then Link's plain
/// keys go. One with no installation ID yet keeps the key.
#[test]
fn a_box_from_before_certificates_is_given_them_then_the_key_goes() {
    let Some(world) = World::new("certify", true) else {
        return;
    };
    let boxes = world.home.join(".lnk/safe/boxes/boxes.toml");
    std::fs::create_dir_all(boxes.parent().unwrap()).unwrap();
    std::fs::write(
        &boxes,
        "schema = 1\n[[box]]\nname = \"old-1\"\nmachine_id = \"k3v9q2x7m1ab\"\naccount = \"acct\"\n\
         kind = \"fake\"\nplace = \"moon-1\"\nid = \"m-1\"\nsize = \"tiny\"\n\
         ip = \"192.0.2.10\"\nstate = \"running\"\ncreated = 1\ntagged = true\n\
         [[box]]\nname = \"older-1\"\naccount = \"acct\"\nkind = \"fake\"\nplace = \"moon-1\"\n\
         id = \"m-9\"\nsize = \"tiny\"\nip = \"192.0.2.11\"\nstate = \"running\"\ncreated = 1\n",
    )
    .unwrap();
    world.lnk_box(&["box", "ssh", "older-1", "--", "true"]);
    assert!(!world.read("ssh.log").contains("user-cas.pub"));
    assert!(!world.read("adapter.log").contains("remove-keys"));

    world.lnk_box(&["box", "ssh", "old-1", "--", "uptime"]);
    let ssh = world.read("ssh.log");
    // Its key got in: nothing added through the cloud.
    assert!(!world.read("adapter.log").contains("add-key"));
    let trusted = ssh.find("user-cas.pub").unwrap();
    assert!(ssh[trusted..].contains("lnk-box-k3v9q2x7m1ab"), "{ssh}");
    assert!(ssh[trusted..].contains("systemctl reload ssh"), "{ssh}");
    let checked = ssh[trusted..].find("cert-v01@openssh.com").unwrap() + trusted;
    assert!(ssh[checked..].contains("c ~ /^lnk-box-/"), "{ssh}");
    assert!(world.read("adapter.log").contains("remove-keys "));
    assert!(
        std::fs::read_to_string(&boxes)
            .unwrap()
            .contains("certified = true")
    );
    // Certified, it's never given them again.
    world.lnk_box(&["box", "ssh", "old-1", "--", "uptime"]);
    assert_eq!(
        world.read("ssh.log").matches("user-cas.pub").count(),
        ssh.matches("user-cas.pub").count()
    );
}

/// `lnk box ssh` with no box named and none kept says it can't pick one
/// and how to make one, as its declared failures allow.
#[test]
fn ssh_with_no_box_says_how_to_make_one() {
    let Some(world) = World::new("ssh-no-box", true) else {
        return;
    };
    let output = world.run(&["box", "ssh"]);
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(
        said.contains("Couldn't pick a box, since you have none."),
        "{said}"
    );
    assert!(said.contains("lnk box create <cloud>"), "{said}");
}
