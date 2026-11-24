//! No other company's credential, real or made up, in a tracked file
//! (the rule is `link_repository_check::credentials`).

use std::path::Path;

#[test]
fn no_credential_is_published() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = link_repository_check::credentials::check(&root, &here.join("credentials.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
