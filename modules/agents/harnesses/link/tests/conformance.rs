//! Link Harness passes the conformance kit (`link_harness_conformance`,
//! in `link-core` beside this folder), the checks every harness's tests
//! run: it answers the adapter contract as a harness must.

#[test]
fn link_harness_conforms() {
    link_harness_conformance::check(std::path::Path::new(env!("CARGO_BIN_EXE_lnk-harness")));
}
