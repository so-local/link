//! What `lnk secret create` and `delete` answer: their `--json`, and a
//! Keychain item a replaced secret leaves.

use super::*;

/// `lnk secret create` and `delete` answer `--json` with the secret, as
/// `secret:<id>` under `resource`.
#[test]
fn create_and_delete_answer_json_with_the_secrets_id() {
    let world = World::new("json");
    let out = world.run_with(&["create", "NPM_TOKEN", "--json"], &format!("{VALUE}\n"));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let created: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    link_test_kit::ids::assert_named(&created);
    let id = world.first_id();
    assert_eq!(
        created,
        serde_json::json!({"resource": format!("secret:{id}")})
    );
    let out = world.run(&["delete", "NPM_TOKEN", "--json"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let deleted: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    link_test_kit::ids::assert_named(&deleted);
    assert_eq!(
        deleted,
        serde_json::json!({
            "resource": format!("secret:{id}"),
            "removed": true,
            "copies": [],
            "left": [],
        })
    );
}

/// A secret made again replaces the first, whose Keychain item a Keychain
/// that won't let go of it leaves: the new one is kept all the same, and
/// the item left is named, never a failed create.
#[test]
fn a_replaced_secrets_item_the_keychain_keeps_is_a_warning() {
    let mut world = World::with_keychain("replaced");
    let out = world.add(&["NPM_TOKEN"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let first = world.first_id();
    // The stand-in, but deleting nothing.
    let keeping = world.dir.join("security-keeping");
    link_test_kit::write_program(
        &keeping,
        format!(
            "#!/bin/sh\n[ \"$1\" = delete-generic-password ] && {{ echo 'security: The \
             item could not be deleted.' >&2; exit 1; }}\nexec {:?} \"$@\"\n",
            world.security.as_ref().unwrap()
        ),
    )
    .unwrap();
    world.security = Some(keeping);
    let out = world.add(&["NPM_TOKEN"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let said = text(&out.stderr);
    assert!(said.contains(&format!("link:secret/{first}")), "{said}");
    let second = world.first_id();
    assert_ne!(first, second);
    assert!(world.dir.join(format!("items/secret/{first}")).exists());
    assert_eq!(
        std::fs::read_to_string(world.dir.join(format!("items/secret/{second}")))
            .unwrap()
            .trim(),
        VALUE
    );
}
