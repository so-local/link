//! This workspace's root `Cargo.toml` keeps the order
//! `manifest-order.toml` gives (the rule is
//! `link_repository_check::manifest_order`, which the private workspace
//! runs on its own roots too).

use std::path::Path;

use link_repository_check::manifest_order::{npm_problems, npm_version, order, problems, top_keys};

#[test]
fn the_root_manifest_keeps_its_order() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let found = link_repository_check::manifest_order::check(&root);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn every_package_json_opens_with_its_name_and_the_workspace_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let version = link_repository_check::manifest_order::workspace_version(&root);
    let found = link_repository_check::manifest_order::npm(&root, &[], &version);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn only_an_objects_own_keys_count() {
    let text = r#"{ "name": "a", "scripts": { "x": "{\"y\": 1}" }, "list": [{ "k": 1 }], "version": "1" }"#;
    assert_eq!(top_keys(text), ["name", "scripts", "list", "version"]);
}

#[test]
fn a_package_json_out_of_order_is_refused_by_name() {
    let text = r#"{ "version": "0.1.0", "private": true, "name": "a" }"#;
    let found = npm_problems("package.json", text, &order()).join("\n");
    assert!(
        found.contains("opens with version, private: open it with name, version"),
        "{found}"
    );
    let text = r#"{ "name": "a", "version": "0.1.0", "private": true }"#;
    assert!(npm_problems("package.json", text, &order()).is_empty());
}

#[test]
fn every_place_names_a_table_or_a_family() {
    for place in order().places {
        assert!(
            place.table.is_some() != place.family.is_some(),
            "a place names one table or one family: {place:?}"
        );
    }
}

#[test]
fn a_manifest_out_of_order_is_refused_by_name() {
    let text = r#"
[workspace]
members = ["b", "a"]

[workspace.package]
edition = "2024"
version = "0.1.0"

[workspace.dependencies]
zeta = "1"
alpha = "1"

[workspace.dependencies.beta]
workspace = true
"#;
    let found = problems(text, &order()).join("\n");
    for expected in [
        "[workspace.package] after [workspace]",
        "opens with edition: open it with version",
        "alpha after zeta",
        "write beta as one line",
        "member a after b",
    ] {
        assert!(
            found.contains(expected),
            "missing {expected:?} in:\n{found}"
        );
    }
}

#[test]
fn a_manifest_in_order_passes() {
    let text = r#"
[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace]
resolver = "3"
members = ["a", "b"]

[workspace.lints.clippy]
todo = "deny"

[workspace.dependencies]
alpha = "1"
beta = { path = "b" }

[profile.dev]
debug = 1

[workspace.metadata.notes]
x = 1
"#;
    assert_eq!(problems(text, &order()), Vec::<String>::new());
}

#[test]
fn a_package_json_off_the_workspace_version_is_refused() {
    let text = r#"{ "name": "a", "version": "0.1.0" }"#;
    let found = npm_version("package.json", text, "0.0.0").join("\n");
    assert!(
        found.contains("is version 0.1.0: set it to the workspace's, 0.0.0"),
        "{found}"
    );
    assert!(
        npm_version(
            "package.json",
            r#"{ "name": "a", "version": "0.0.0" }"#,
            "0.0.0"
        )
        .is_empty()
    );
}
