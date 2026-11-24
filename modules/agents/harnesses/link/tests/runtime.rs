//! Confined comparison cases over the same first-party fixture as Mini.

use link_harness_conformance::runtime::{self, Mode};

async fn checked(harness: &str, count: usize) {
    let report = runtime::run(&link_test_kit::programs(), harness, Mode::Confined, count).await;
    println!("{report}");
    assert!(
        matches!(report["outcome"].as_str(), Some("passed" | "skipped")),
        "{report}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mini_confined_control() {
    checked("mini", 1).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn full_link_confined_protected_cold_and_warm_tools() {
    checked("link", 1).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_confined_agents_keep_their_files_and_cleanup_separate() {
    checked("link", 2).await;
}
