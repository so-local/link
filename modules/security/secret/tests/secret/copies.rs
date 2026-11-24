//! The copies a move gave of an agent's own secret: reached from here by
//! `lnk secret delete`, and removed on a box by the agent's ID and use.

use super::*;

/// An agent's key a move gave to boxes and a cloud account, as the move
/// noted it: `remove` asks the module whose moves made them to reach
/// each, all at once, says what it found of each, and keeps a copy it
/// couldn't reach for the next `remove`, failing until one reaches it. A
/// copy no module here reaches stays noted.
#[test]
fn remove_reaches_each_copy_through_the_module_whose_move_made_it() {
    let world = World::new("copies");
    std::fs::create_dir_all(world.store().parent().unwrap()).unwrap();
    let key = "k1m2n3p4q5r6";
    std::fs::write(
        world.store(),
        format!(
            "[entry.{key}]\nkind = \"own\"\nvalue = \"{VALUE}\"\n\
             to = [{{ principal = \"service:{MAIN}\", action = \"secret.secret.read\", resource = \"secret:{key}\", workload = \"{MAIN_WORKLOAD}\", name = \"main\", as = {{ kind = \"own\", purpose = \"model\" }} }}]\n\
             copies = [{{ name = \"aws-1\" }}, {{ name = \"gcp-1\" }}, {{ name = \"old-1\" }}, \
             {{ cloud = true, name = \"cloud-1\" }}, {{ cloud = true, name = \"cloud-2\" }}]\n"
        ),
    )
    .unwrap();
    let listed = world.run(&["list"]);
    assert!(
        text(&listed.stdout).contains("copies on aws-1, gcp-1, old-1, cloud-1, cloud-2"),
        "{}{}",
        text(&listed.stdout),
        text(&listed.stderr)
    );
    // An agent's own: listed by its kind, whose it is and its use, never a
    // name.
    let own = &world.listed()[0];
    assert_eq!(own["kind"], "own");
    assert_eq!(own["name"], serde_json::Value::Null);
    assert_eq!(
        own["owner"],
        serde_json::json!({"principal": format!("service:{MAIN}"), "as": {"kind": "own", "purpose": "model"}})
    );
    // In that order: an account it couldn't ask holds its copy with the
    // agent, as one that says so does.
    let reached = |json: &str| std::fs::write(world.dir.join("reached.json"), json).unwrap();
    reached(
        r#"["removed", {"unreached": "gcp-1 is stopped: lnk box start gcp-1"}, "removed", "with_service", {"unreached": "down"}]"#,
    );
    let by_id = format!("secret:{key}");
    let out = world.run(&["delete", &by_id]);
    assert!(!out.status.success(), "{}", text(&out.stdout));
    // A failure keeps stdout empty: its report says what is left, and the
    // store what was removed.
    let err = text(&out.stderr);
    assert!(
        !std::fs::read_to_string(world.store())
            .unwrap_or_default()
            .contains(VALUE)
    );
    for account in ["cloud-1", "cloud-2"] {
        assert!(
            err.contains(&format!(
                "left on {account}: removed with its service there"
            )),
            "{err}"
        );
    }
    assert!(
        err.contains("gcp-1 (gcp-1 is stopped: lnk box start gcp-1)"),
        "{err}"
    );
    // Each copy asked by whose own it is and for what, as a box finds it.
    let asked: serde_json::Value =
        serde_json::from_slice(&std::fs::read(world.dir.join("asked.json")).unwrap()).unwrap();
    assert_eq!(asked["protocol"], 2, "{asked}");
    let asked = &asked["question"];
    assert_eq!(asked["copies"].as_array().unwrap().len(), 5);
    assert_eq!(
        asked["copies"][0],
        serde_json::json!({
            "owner": {"principal": format!("service:{MAIN}"), "as": {"kind": "own", "purpose": "model"}},
            "place": {"name": "aws-1"},
        })
    );
    let store = std::fs::read_to_string(world.store()).unwrap();
    assert!(!store.contains(VALUE), "{store}");
    assert!(store.contains(&format!("retired = [\"{key}\"]")), "{store}");
    let place = |name: &str, cloud: bool| serde_json::json!({"name": name, "cloud": cloud});
    assert_eq!(
        world.listed()[0]["copies"],
        serde_json::json!([
            place("gcp-1", false),
            place("cloud-1", true),
            place("cloud-2", true),
        ])
    );

    // No module here reaches them: each stays noted.
    std::fs::write(world.dir.join("no-agent"), "").unwrap();
    let out = world.run(&["delete", &by_id]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("gcp-1 (no module here reaches it"),
        "{}",
        text(&out.stderr)
    );
    std::fs::remove_file(world.dir.join("no-agent")).unwrap();

    // Up again: the next remove reaches it; the accounts' stay, said,
    // and fail nothing.
    reached(r#"["removed", "with_service", "with_service"]"#);
    let out = world.run(&["delete", &by_id]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let said = text(&out.stdout);
    let noted = text(&out.stderr);
    assert!(said.contains("removed from gcp-1"), "{said}");
    assert!(noted.contains("left on cloud-1"), "{noted}");
    assert!(
        !said.contains("aws-1") && !noted.contains("aws-1"),
        "{said}{noted}"
    );
    assert_eq!(
        world.listed()[0]["copies"],
        serde_json::json!([place("cloud-1", true), place("cloud-2", true)])
    );

    // On a box, `--here`: this machine alone, and nothing there is no error.
    let out = world.run(&["delete", "NOT_KEPT", "--here"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("nothing to remove"));
    let out = world.run(&["delete", "NOT_KEPT"]);
    assert!(!out.status.success());
}

/// On a box, `--here --service service:<id> --own <use>` removes that
/// agent's own secret alone: another agent's there (main made again, or
/// another box's agent) is left as it is.
#[test]
fn remove_here_for_an_agent_leaves_another_agents_key() {
    let world = World::new("for-agent");
    std::fs::create_dir_all(world.store().parent().unwrap()).unwrap();
    std::fs::write(
        world.store(),
        format!(
            "[entry.k1m2n3p4q5r6]\nkind = \"own\"\nvalue = \"{VALUE}\"\n\
             to = [{{ principal = \"service:z9y8x7w6v5u4\", action = \"secret.secret.read\", resource = \"secret:k1m2n3p4q5r6\", workload = \"{MAIN_WORKLOAD}\", name = \"main\", as = {{ kind = \"own\", purpose = \"model\" }} }}]\n"
        ),
    )
    .unwrap();
    let own = |agent: &str, r#use: &str| {
        world.run(&["delete", "--here", "--service", agent, "--own", r#use])
    };
    let kept = || {
        std::fs::read_to_string(world.store())
            .unwrap()
            .contains(VALUE)
    };
    let out = own(&format!("service:{MAIN}"), "model");
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("nothing to remove"),
        "{}",
        text(&out.stderr)
    );
    assert!(kept());
    // Its own agent's, for another use: left too.
    let out = own("service:z9y8x7w6v5u4", "fallback");
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(kept());
    // Its own agent's, for its use: removed.
    let out = own("service:z9y8x7w6v5u4", "model");
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!kept());
    // --service is an agent's service, for --here and --own alone.
    for args in [
        &[
            "delete",
            "--service",
            "service:z9y8x7w6v5u4",
            "--own",
            "model",
        ][..],
        &["delete", "--here", "--service", "service:z9y8x7w6v5u4"],
        &[
            "delete",
            "--here",
            "--service",
            "z9y8x7w6v5u4",
            "--own",
            "model",
        ],
        &[
            "delete",
            "X",
            "--here",
            "--service",
            "service:z9y8x7w6v5u4",
            "--own",
            "model",
        ],
    ] {
        assert!(!world.run(args).status.success(), "{args:?}");
    }
}

/// `lnk uninstall` removes the copies a move left before it forgets your
/// secrets: one it can't reach refuses the uninstall, the store and its
/// note of that copy kept, so a key on a box is never forgotten; once it
/// reaches it, your secrets go.
#[test]
fn uninstall_reaches_the_copies_a_move_left_first() {
    let world = World::new("uninstall-copies");
    std::fs::create_dir_all(world.store().parent().unwrap()).unwrap();
    let key = "k1m2n3p4q5r6";
    std::fs::write(
        world.store(),
        format!(
            "[entry.{key}]\nkind = \"own\"\nvalue = \"{VALUE}\"\n\
             to = [{{ principal = \"service:{MAIN}\", action = \"secret.secret.read\", resource = \"secret:{key}\", workload = \"{MAIN_WORKLOAD}\", name = \"main\", as = {{ kind = \"own\", purpose = \"model\" }} }}]\n\
             copies = [{{ name = \"aws-1\" }}, {{ name = \"gcp-1\" }}]\n"
        ),
    )
    .unwrap();
    let reached = |json: &str| std::fs::write(world.dir.join("reached.json"), json).unwrap();
    reached(r#"["removed", {"unreached": "gcp-1 is stopped: lnk box start gcp-1"}]"#);
    let out = world.run(&["uninstall", "--delete", "secrets"]);
    assert!(!out.status.success(), "{}", text(&out.stdout));
    let err = text(&out.stderr);
    assert!(err.contains("gcp-1 is stopped"), "{err}");
    // The value is gone, the copy it couldn't reach still noted, the one
    // it reached not.
    let store = std::fs::read_to_string(world.store()).unwrap();
    assert!(!store.contains(VALUE), "{store}");
    assert!(
        store.contains("gcp-1") && !store.contains("aws-1"),
        "{store}"
    );
    // Reached on the next try: the uninstall goes on, and the store goes.
    reached(r#"["removed"]"#);
    let out = world.run(&["uninstall", "--delete", "secrets"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!world.store().exists());
}
