//! `flakes.toml`: each flaky test a session met and didn't fix names a test
//! of a package here, with when, on which commit, how it ran and what it said
//! (`link_repository_check::flakes`).

use std::{collections::BTreeSet, path::Path};

#[test]
fn every_flake_written_down_names_a_test_here() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = link_test_kit::cargo()
        .current_dir(here.join("../.."))
        .args([
            "metadata",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages: BTreeSet<String> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    let text = std::fs::read_to_string(here.join("flakes.toml")).unwrap();
    let problems = link_repository_check::flakes::problems(&text, &packages);
    assert!(problems.is_empty(), "flakes.toml:\n{}", problems.join("\n"));
}
