//! Tests keep to the testing kit where Clippy can't see it, in this
//! workspace (the rules are `link_repository_check::test_kit`, which the
//! private workspace runs on its own root too).

use std::path::Path;

/// The workspace's root, from this package's.
const WORKSPACE: &str = "../..";

#[test]
fn tests_keep_to_the_testing_kit() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let listed = link_repository_check::test_kit::Listed::parse(
        &std::fs::read_to_string(here.join("test-kit.toml")).unwrap(),
    );
    let found = link_repository_check::test_kit::check(
        &root,
        &["core", "packages", "modules", "interop", "tests"],
        // The kit, and this rule's own examples of what it refuses.
        &["packages/test-kit", "tests/repository/src/test_kit.rs"],
        &listed,
    );
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// Every test program declares its kind, which gives it a home of its own,
/// and a unit program's tests start no program (`link_test_kit::program!`).
#[test]
fn every_test_program_declares_its_kind() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = here.join(WORKSPACE).canonicalize().unwrap();
    let output = link_test_kit::cargo()
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata = serde_json::from_slice(&output.stdout).unwrap();
    let programs = link_repository_check::test_kit::programs(&workspace, &metadata);
    let mut found = link_repository_check::test_kit::undeclared(&programs);
    found.extend(link_repository_check::test_kit::unit_starting(
        &workspace, &programs,
    ));
    assert!(found.is_empty(), "{}", found.join("\n"));
}
