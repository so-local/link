//! Every program this workspace builds is `lnk` or `lnk-<name>`: the
//! rule is `link_repository_check::program_names`, which the private
//! workspace runs on its own roots too.

use std::path::Path;

#[test]
fn every_program_is_named_lnk() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let problems = link_repository_check::program_names::problems(&root, &[]);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
