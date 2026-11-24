//! No code kept for an older version: a source file that says one of
//! legacy's words is listed in `legacy.toml`, which may shrink, never grow (the rule
//! is `link_repository_check::legacy`, which the private workspace runs on
//! its own root too).

use std::path::Path;

#[test]
fn no_new_code_for_an_older_version() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = link_repository_check::legacy::check(&root, &here.join("legacy.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
