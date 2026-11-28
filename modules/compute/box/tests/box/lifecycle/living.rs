//! A box's life: made, set up, stopped, started and removed; boxes kept,
//! found in the cloud, and owned when signed out.

use super::*;

/// Two `lnk box create` at once, each saving what it knows while the
/// other makes its box: both boxes are kept, under names of their own.
#[test]
fn two_boxes_started_at_once_are_both_kept() {
    let Some(world) = World::new("race", true) else {
        return;
    };
    std::fs::write(world.dir.join("slow"), "").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                world.lnk_box(&[
                    "box",
                    "create",
                    "--cloud",
                    "acct",
                    "--yes",
                    "--lnk.login=false",
                ])
            });
        }
    });
    let boxes: toml::Value = toml::from_str(
        &std::fs::read_to_string(world.home.join(".lnk/safe/boxes/boxes.toml")).unwrap(),
    )
    .unwrap();
    let boxes = boxes["box"].as_array().unwrap();
    let names: Vec<_> = boxes.iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["acct-1", "acct-2"]);
    assert_ne!(boxes[0]["id"], boxes[1]["id"]);
    assert!(boxes.iter().all(|b| b["state"].as_str() == Some("running")));
    // Each name was checked against the boxes the cloud has where the
    // box goes, never every place.
    let adapter = world.read("adapter.log");
    let asked: Vec<_> = adapter.lines().filter(|l| l.starts_with("list ")).collect();
    assert_eq!(asked.len(), 2, "{adapter}");
    assert!(
        asked.iter().all(|l| l.contains(r#""places":["moon-1"]"#)),
        "{adapter}"
    );
}

/// Signed-out boxes use the persisted local installation identity for both
/// launch and discovery, and never start an interactive relay login.
#[test]
fn signed_out_boxes_use_the_local_owner_for_creation_and_discovery() {
    let Some(world) = World::new("local-owner-discovery", true) else {
        return;
    };
    let config = world.home.join(".lnk");
    std::fs::remove_file(config.join("safe/accounts/accounts.toml")).unwrap();
    link_files::write_private(
        &link_files::layout::Hidden::Installation.at(&config),
        b"schema = 1\nid = \"abcdefabcdef\"\nname = \"qa-host\"\n",
    )
    .unwrap();
    let lnk = world.bin.join("lnk");
    let fake = std::fs::read_to_string(&lnk)
        .unwrap()
        .replace("\"username\": \"me\"", "\"username\": null")
        .replace("\"owner\": \"github-7\"", "\"owner\": null");
    script(&lnk, fake.strip_prefix("#!/bin/sh\n").unwrap());
    world.lnk_box(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "local-box",
        "--yes",
    ]);
    world.lnk_box(&["box", "list", "--json"]);
    let requests: Vec<(String, serde_json::Value)> = world
        .read("adapter.log")
        .lines()
        .map(|line| {
            let (name, request) = line.split_once(' ').unwrap();
            (name.into(), serde_json::from_str(request).unwrap())
        })
        .collect();
    let launch = &requests
        .iter()
        .find(|(name, _)| name == "launch")
        .unwrap()
        .1;
    let local = link_machine::owner::local_owner_of(
        &link_machine::machine::user_name().unwrap(),
        "qa-host",
    );
    let owner = format!("local-{}", &link_digest::sha256(local.as_bytes())[..56]);
    assert_eq!(launch["spec"]["tags"]["lnk-owner"], owner);
    assert!(owner.len() <= 63);
    assert!(
        owner
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    );
    assert!(
        requests
            .iter()
            .any(|(name, request)| name == "list" && request["owner"] == owner)
    );
    let ssh = world.read("ssh.log");
    assert!(!ssh.contains("auth login"), "{ssh}");
    assert!(!ssh.contains("auth key"), "{ssh}");
    assert_eq!(
        world
            .lnk_box(&["box", "ssh", "local-box", "--", "true"])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn a_box_is_made_set_up_stopped_started_and_removed() {
    let Some(world) = World::new("life", true) else {
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
    let stdout = String::from_utf8_lossy(&made.stdout);
    assert!(
        stdout.contains("Your box b1 is up at 192.0.2.10, a machine of type tiny in moon-1."),
        "{stdout}"
    );

    let adapter = world.read("adapter.log");
    let launch = adapter.lines().find(|l| l.starts_with("launch ")).unwrap();
    let request: serde_json::Value = serde_json::from_str(&launch["launch ".len()..]).unwrap();
    assert_eq!(request["account"]["env"]["FAKE_KEY"], "k");
    assert_eq!(request["place"], "moon-1");
    assert_eq!(request["spec"]["name"], "b1");
    assert_eq!(request["spec"]["size"], "tiny");
    // A installation ID of its own, which names its machine, and its tags.
    let id = request["spec"]["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 12);
    assert_eq!(
        request["spec"]["tags"],
        serde_json::json!({"lnk-box": "b1", "lnk-machine": id, "lnk-owner": "github-7"})
    );
    assert!(
        request["spec"]["user_data"]
            .as_str()
            .unwrap()
            .starts_with("#cloud-config")
    );
    // No key: its first boot trusts this computer's certificate
    // authority, for certificates naming this box alone.
    assert!(
        request["spec"]
            .get("public_key")
            .is_none_or(|key| key == ""),
        "{request}"
    );
    let authority = std::fs::read_to_string(world.home.join(".lnk/safe/boxes/ssh-ca.pub")).unwrap();
    let user_data = request["spec"]["user_data"].as_str().unwrap();
    assert!(
        user_data.contains(&format!("      {}\n", authority.trim())),
        "{user_data}"
    );
    assert!(
        user_data.contains(&format!("      lnk-box-{id}\n")),
        "{user_data}"
    );

    // Link's own key and known hosts; the install from the same relay.
    let ssh = world.read("ssh.log");
    assert!(ssh.contains("ubuntu@192.0.2.10"), "{ssh}");
    // Its keys pinned from its console: checked strictly, never trusted anew.
    assert!(ssh.contains("StrictHostKeyChecking=yes"), "{ssh}");
    assert!(!ssh.contains("StrictHostKeyChecking=accept-new"), "{ssh}");
    // ssh's options come before the host: after it, they'd run on the box.
    assert!(
        ssh.contains("-o BatchMode=yes -- ubuntu@192.0.2.10 true"),
        "{ssh}"
    );
    assert!(
        ssh.contains("-o BatchMode=yes -- ubuntu@192.0.2.10 sh -s"),
        "{ssh}"
    );
    assert!(ssh.contains(".lnk/safe/boxes/id_ed25519"), "{ssh}");
    // The release's install.sh, checked here by this lnk's keys, run on
    // the box as it was checked: no script fetched there.
    assert!(ssh.contains("sh <<'LNK_CHECKED_INSTALL_SH'\n"), "{ssh}");
    assert!(ssh.contains(release::INSTALL), "{ssh}");
    assert!(!ssh.contains("curl"), "{ssh}");
    // The box knows its ID and name before lnk first runs there.
    assert!(ssh.contains(&format!("|| id={id}\n")), "{ssh}");
    // Its host key, from its console, pinned before the first contact,
    // under the box's own name, whatever its address.
    let known = world.home.join(".lnk/safe/boxes/known_hosts");
    let pinned = format!("lnk-fake-m-1 ssh-ed25519 {KEY}\n");
    assert!(ssh.contains("-o HostKeyAlias=lnk-fake-m-1 "), "{ssh}");
    assert_eq!(std::fs::read_to_string(&known).unwrap(), pinned);
    let key = world.home.join(".lnk/safe/boxes/id_ed25519");
    let mode = std::fs::metadata(&key).unwrap().permissions();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o077, 0);
    // Each connection with a certificate for this box, signed here by
    // the authority, whose secret only this user reads; reused while
    // it's fresh.
    let cert = world
        .home
        .join(format!(".lnk/safe/boxes/certs/{id}-cert.pub"));
    assert!(
        ssh.contains(&format!("-o CertificateFile=\"{}\"", cert.display())),
        "{ssh}"
    );
    let ca = world.home.join(".lnk/safe/boxes/ssh-ca");
    let mode = std::fs::metadata(&ca).unwrap().permissions();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o077, 0);
    #[expect(
        clippy::disallowed_methods,
        reason = "reads the certificate with ssh-keygen"
    )]
    let shown = std::process::Command::new("ssh-keygen")
        .args(["-L", "-f"])
        .arg(&cert)
        .output()
        .unwrap();
    let shown = String::from_utf8_lossy(&shown.stdout);
    assert!(shown.contains(&format!("lnk-box-{id}")), "{shown}");
    assert!(shown.contains("permit-pty"), "{shown}");
    assert!(!shown.contains("permit-agent-forwarding"), "{shown}");
    assert!(!shown.contains("permit-X11-forwarding"), "{shown}");
    let signed = std::fs::metadata(&cert).unwrap().modified().unwrap();

    let listed = world.lnk_box(&["box", "list", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    link_test_kit::ids::assert_named(&listed);
    assert_eq!(listed[0]["name"], "b1");
    assert_eq!(listed[0]["id"], "m-1");
    assert_eq!(listed[0]["resource"], format!("box:{id}"));
    assert_eq!(listed[0]["state"], "running");
    assert_eq!(listed[0]["found"], "here");

    world.lnk_box(&["box", "stop", "b1"]);
    let listed = world.lnk_box(&["box", "list", "--json"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("\"stopped\""));
    world.lnk_box(&["box", "start", "b1"]);
    assert_eq!(std::fs::read_to_string(&known).unwrap(), pinned);
    world.lnk_box(&["box", "ssh", "b1", "--", "uptime"]);
    // Fresh still: the same certificate, signed once.
    assert_eq!(
        std::fs::metadata(&cert).unwrap().modified().unwrap(),
        signed
    );
    // Made with certificates, it's never given them again.
    assert!(!world.read("ssh.log").contains("user-cas.pub"));
    world.lnk_box(&["box", "run", "b1", "--", "agent", "show", "it's"]);
    assert!(
        world.read("ssh.log").lines().any(|l| l
            .ends_with(&format!(
                r#"ubuntu@192.0.2.10 LNK_RELEASES_URL='{}' "$HOME/.lnk/exec/bin/lnk" 'agent' 'show' 'it'\''s'"#,
                world.releases.url
            ))),
        "{}",
        world.read("ssh.log")
    );
    assert!(
        world
            .read("ssh.log")
            .lines()
            .any(|l| l.ends_with("ubuntu@192.0.2.10 uptime"))
    );

    // Without `--proxy-port` it refuses rather than open ssh's own SOCKS (this
    // computer and its LAN).
    let before = world.read("ssh.log");
    let refused = world.lnk_box_fails(&["box", "forward", "b1", "--port", "40780"]);
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--proxy-port"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_eq!(world.read("ssh.log"), before, "no ssh without --to");
    // A box with users of its own: a port on its loopback would be
    // theirs too, so only a socket is forwarded.
    std::fs::write(world.dir.join("users"), "").unwrap();
    let shared = world.lnk_box_fails(&[
        "box",
        "forward",
        "b1",
        "--port",
        "40780",
        "--proxy-port",
        "5555",
    ]);
    let said = String::from_utf8_lossy(&shared.stderr);
    assert!(said.contains("has users of its own"), "{said}");
    assert!(
        !world.read("ssh.log").contains("-R 127.0.0.1:40780"),
        "no port forwarded"
    );
    std::fs::remove_file(world.dir.join("users")).unwrap();
    world.lnk_box(&[
        "box",
        "forward",
        "b1",
        "--port",
        "40780",
        "--proxy-port",
        "5555",
    ]);
    assert!(
        world.read("ssh.log").lines().any(|l| l.contains("-N")
            && l.ends_with("-R 127.0.0.1:40780:127.0.0.1:5555 -- ubuntu@192.0.2.10")),
        "{}",
        world.read("ssh.log")
    );
    // Whatever `~/.ssh/config` says, no connection hands the box this
    // machine's agent or X11, or runs a command here; and only the
    // forward's forwards a port.
    let ssh = world.read("ssh.log");
    let connections: Vec<&str> = ssh.lines().filter(|l| l.contains("ubuntu@")).collect();
    assert!(connections.len() > 3, "{ssh}");
    for line in &connections {
        for option in [
            "-o ForwardAgent=no ",
            "-o ForwardX11=no ",
            "-o PermitLocalCommand=no ",
        ] {
            assert!(line.contains(option), "{option}: {line}");
        }
        let forward = line.contains(" -R ");
        assert_eq!(
            !line.contains("-o ClearAllForwardings=yes "),
            forward,
            "{line}"
        );
    }
    world.lnk_box(&["box", "delete", "b1", "--yes"]);
    let adapter = world.read("adapter.log");
    let commands: Vec<_> = adapter
        .lines()
        .map(|l| l.split(' ').next().unwrap())
        .filter(|c| *c != "list")
        .collect();
    assert_eq!(
        commands,
        [
            "defaults",
            "launch",
            "wait",
            "hostkeys",
            "save-hostkeys",
            "stop",
            "start",
            "remove",
            "cleanup"
        ]
    );
    // The host keys pinned here are kept in the cloud, for another computer.
    let saved = adapter
        .lines()
        .find(|l| l.starts_with("save-hostkeys "))
        .unwrap();
    assert!(
        saved.contains(&format!("\"host_keys\":[\"ssh-ed25519 {KEY}\"]")),
        "{saved}"
    );
    // Its name was checked against the boxes the cloud has.
    let asked = adapter.lines().find(|l| l.starts_with("list ")).unwrap();
    assert!(asked.contains("\"owner\":\"github-7\""), "{asked}");
    let listed = world.lnk_box(&["box", "list", "--json"]);
    assert_eq!(String::from_utf8_lossy(&listed.stdout).trim(), "[]");
}

/// A box made on another computer is found in the cloud by its tags; the
/// first SSH there pins its host keys from the cloud and adds this
/// computer's key through it. A box without tags is tagged.
#[test]
fn boxes_are_found_in_the_cloud_and_untagged_ones_tagged() {
    let Some(world) = World::new("found", true) else {
        return;
    };
    std::fs::write(
        world.dir.join("cloud.json"),
        r#"{"boxes": [
            {"id": "m-2", "name": "found-box", "machine_id": "k3v9q2x7m1ab", "place": "moon-2",
             "state": "running", "ip": "192.0.2.20", "size": "tiny"},
            {"id": "m-3", "name": "Not A Name", "place": "moon-2", "state": "running"},
            {"id": "--zone=x", "name": "sneaky", "place": "moon-2", "state": "running"}
        ]}"#,
    )
    .unwrap();
    let boxes = world.home.join(".lnk/safe/boxes/boxes.toml");
    std::fs::create_dir_all(boxes.parent().unwrap()).unwrap();
    std::fs::write(
        &boxes,
        "schema = 1\n[[box]]\nname = \"acct-1\"\naccount = \"acct\"\nkind = \"fake\"\nplace = \"moon-1\"\n\
         id = \"m-1\"\nsize = \"tiny\"\nip = \"192.0.2.10\"\nstate = \"running\"\ncreated = 1\n",
    )
    .unwrap();

    let listed = world.lnk_box(&["box", "list", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let names: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["name"].as_str().unwrap(), b["found"].as_str().unwrap()))
        .collect();
    // The ones the cloud names badly are skipped.
    assert_eq!(names, [("acct-1", "here"), ("found-box", "cloud")]);
    link_test_kit::ids::assert_named(&listed);
    // The old one learned its ID from the box, and got its tags.
    assert_eq!(listed[0]["resource"], "box:abcdefabcdef");
    let adapter = world.read("adapter.log");
    let tag = adapter.lines().find(|l| l.starts_with("tag ")).unwrap();
    assert!(
        tag.contains(
            r#""tags":{"lnk-box":"acct-1","lnk-machine":"abcdefabcdef","lnk-owner":"github-7"}"#
        ),
        "{tag}"
    );
    // Asked where its boxes are known to be.
    let asked = adapter.lines().find(|l| l.starts_with("list ")).unwrap();
    assert!(asked.contains(r#""places":["moon-1"]"#), "{asked}");
    // Found again, it's one of these now.
    let again = world.lnk_box(&["box", "list", "--json"]);
    assert!(!String::from_utf8_lossy(&again.stdout).contains("\"cloud\""));
    // A refresh asks the state of only the box the cloud didn't list.
    world.lnk_box(&["box", "list", "--refresh", "--json"]);
    let adapter = world.read("adapter.log");
    let states: Vec<_> = adapter
        .lines()
        .filter(|l| l.starts_with("state "))
        .collect();
    assert_eq!(states.len(), 1, "{adapter}");
    assert!(states[0].contains(r#""id":"m-1""#), "{adapter}");

    // The first SSH to it: host keys from the cloud, then this computer's
    // key added through it (for a minute), appended meanwhile.
    world.lnk_box(&["box", "ssh", "found-box", "--", "uptime"]);
    let known = std::fs::read_to_string(world.home.join(".lnk/safe/boxes/known_hosts")).unwrap();
    assert!(
        known.contains(&format!("lnk-fake-m-2 ssh-ed25519 {KEY}")),
        "{known}"
    );
    let adapter = world.read("adapter.log");
    let added = adapter.lines().find(|l| l.starts_with("add-key ")).unwrap();
    assert!(added.contains(r#""public_key":"ssh-ed25519 "#), "{added}");
    let ssh = world.read("ssh.log");
    assert!(ssh.contains("authorized_keys"), "{ssh}");
    // Then it trusts this computer's certificates, for its own principal,
    // and Link's plain keys go: from the cloud's, then from the box.
    let authority = std::fs::read_to_string(world.home.join(".lnk/safe/boxes/ssh-ca.pub")).unwrap();
    assert!(ssh.contains(authority.trim()), "{ssh}");
    assert!(ssh.contains("lnk-box-k3v9q2x7m1ab"), "{ssh}");
    assert!(ssh.contains("TrustedUserCAKeys"), "{ssh}");
    let trusted = ssh.find("user-cas.pub").unwrap();
    let checked = ssh[trusted..].find("cert-v01@openssh.com").unwrap() + trusted;
    let dropped = ssh.rfind("c ~ /^lnk-box-/").unwrap();
    assert!(checked < dropped, "{ssh}");
    let adapter = world.read("adapter.log");
    assert!(
        adapter
            .lines()
            .any(|l| l.starts_with("remove-keys ") && l.contains(r#""id":"m-2""#)),
        "{adapter}"
    );
    let boxes = std::fs::read_to_string(world.home.join(".lnk/safe/boxes/boxes.toml")).unwrap();
    assert!(boxes.contains("certified = true"), "{boxes}");
    // Once in, never again.
    world.lnk_box(&["box", "ssh", "found-box", "--", "uptime"]);
    assert_eq!(
        world
            .read("adapter.log")
            .lines()
            .filter(|l| l.starts_with("add-key "))
            .count(),
        1
    );
}

/// Signed-out creation uses the same portable local owner on subsequent calls,
/// and does not try the browser login that requires an account.
#[test]
fn signed_out_boxes_keep_a_stable_local_owner_without_logging_in() {
    let Some(world) = World::new("local-owner", true) else {
        return;
    };
    std::fs::remove_file(world.home.join(".lnk/safe/accounts/accounts.toml")).unwrap();
    for name in ["local-one", "local-two"] {
        world.lnk_box(&["box", "create", "--cloud", "acct", "--name", name, "--yes"]);
    }
    let log = std::fs::read_to_string(world.dir.join("adapter.log")).unwrap();
    let owners: Vec<String> = log
        .lines()
        .filter_map(|line| {
            let request: serde_json::Value =
                serde_json::from_str(line.strip_prefix("launch ")?).unwrap();
            request["spec"]["tags"]["lnk-owner"]
                .as_str()
                .map(str::to_string)
        })
        .collect();
    assert_eq!(owners.len(), 2, "{log}");
    assert!(owners[0].starts_with("local-"), "{owners:?}");
    assert_eq!(owners[0], owners[1]);
    let ssh = std::fs::read_to_string(world.dir.join("ssh.log")).unwrap();
    assert!(!ssh.contains("auth login"), "{ssh}");
    // Signed out, it keeps no login: no accounts module, which a release
    // may not have.
    assert!(ssh.contains("module add secret sandbox"), "{ssh}");
    assert!(!ssh.contains("module add auth"), "{ssh}");
}

/// A Keychain that can't keep the box certificate authority's secret
/// (locked, or asking for a person) fails `lnk box create` before any
/// box is made, and the secret is in no file.
#[test]
fn a_failing_keychain_fails_create_and_keeps_no_file() {
    let Some(world) = World::new("refusing-keychain", true) else {
        return;
    };
    let refusing = link_test_kit::keychain::refusing(&world.dir);
    let out = world.run_with(
        &[
            "box",
            "create",
            "--cloud",
            "acct",
            "--yes",
            "--lnk.login=false",
        ],
        &[("LNK_SECURITY", refusing.to_str().unwrap())],
    );
    assert!(!out.status.success(), "lnk box create succeeded");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("Keychain"), "{said}");
    let boxes = world.home.join(".lnk/safe/boxes");
    assert!(!boxes.join("ssh-ca").exists());
    assert!(!boxes.join("ssh-ca.pub").exists());
    let adapter = world.read("adapter.log");
    assert!(
        !adapter.lines().any(|l| l.starts_with("create")),
        "{adapter}"
    );
}

/// A box deleted from its cloud asks each module that gave it something
/// (a key to its storage) to take it back, naming the box by its name
/// and installation ID, and says what one couldn't; the box is gone either way.
#[test]
fn a_deleted_box_asks_what_it_was_given_back() {
    let Some(world) = World::new("removed-asked", true) else {
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
    assert!(world.read("removed.log").is_empty());
    std::fs::write(world.dir.join("holder-keeps"), "").unwrap();
    let deleted = world.lnk_box(&["box", "delete", "b1", "--yes"]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&deleted.stdout),
        String::from_utf8_lossy(&deleted.stderr)
    );
    assert!(said.contains("The key b1 had may still work."), "{said}");
    let asked = world.read("removed.log");
    let asked: serde_json::Value = serde_json::from_str(asked.trim()).unwrap();
    assert_eq!(asked["question"]["name"], "b1", "{asked}");
    let listed = world.lnk_box(&["box", "list", "--here", "--json"]);
    assert!(
        !String::from_utf8_lossy(&listed.stdout).contains("\"b1\""),
        "{said}"
    );
}
