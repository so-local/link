//! One meaning per flag: the rule is `flags.toml`, read by
//! `link_repository_check::flags`, which the private workspace runs on its
//! own root too.

use std::path::Path;

#[test]
fn flags_keep_to_one_meaning_each() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = link_repository_check::flags::check(&root, &here.join("flags.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
