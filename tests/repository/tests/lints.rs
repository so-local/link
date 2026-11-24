//! Every package of this workspace takes its lint levels (the rule is
//! `link_repository_check::lints`, which the private workspace runs on
//! its own root too).

use std::path::Path;

#[test]
fn every_package_takes_the_workspace_lint_levels() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let found = link_repository_check::lints::check(&root);
    assert!(found.is_empty(), "{}", found.join("\n"));
}
