//! One verb per meaning: the rule is `verbs.toml`, read by
//! `link_repository_check::commands`, which the private workspace runs on
//! its own root too.

use std::path::Path;

#[test]
fn commands_keep_to_one_verb_per_meaning() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = link_repository_check::commands::check(&root, &here.join("verbs.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
