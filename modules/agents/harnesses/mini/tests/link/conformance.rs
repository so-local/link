//! The mini harness passes the conformance kit
//! (`link_harness_conformance`), the checks every harness's tests run:
//! the core's fixture answers the contract as a harness must.

#![cfg(unix)]

link_test_kit::program!(unit);

#[test]
fn the_mini_harness_conforms() {
    link_harness_conformance::check(std::path::Path::new(env!("CARGO_BIN_EXE_lnk-mini-harness")));
}
