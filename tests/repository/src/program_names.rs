//! Every program Link builds is named `lnk` or `lnk-<name>`, whatever
//! builds it: a module, a service, a fixture or a tool run by hand. The
//! rule reads each binary target `cargo metadata` lists for a workspace;
//! a package keeps its own name, since only the program a person or a
//! script runs is named here.

use std::path::Path;

/// The core's own program, the one name without a suffix.
const CORE: &str = "lnk";

/// The prefix of every other program.
const PREFIX: &str = "lnk-";

/// Each problem with the programs the workspace at `root` builds: a
/// binary named outside the scheme and not on `held`, or a name on
/// `held` that is no longer built that way, since the list only shrinks.
pub fn problems(root: &Path, held: &[&str]) -> Vec<String> {
    let output = match link_test_kit::cargo()
        .current_dir(root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => return vec![String::from_utf8_lossy(&output.stderr).into_owned()],
        Err(error) => return vec![format!("running cargo metadata: {error}")],
    };
    let metadata: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(metadata) => metadata,
        Err(error) => return vec![format!("reading cargo metadata: {error}")],
    };
    let mut misnamed = Vec::new();
    for package in metadata["packages"].as_array().into_iter().flatten() {
        for target in package["targets"].as_array().into_iter().flatten() {
            let bin = target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k == "bin"));
            let name = target["name"].as_str().unwrap_or_default();
            if bin && name != CORE && !name.starts_with(PREFIX) {
                misnamed.push((
                    name.to_string(),
                    package["name"].as_str().unwrap_or_default().to_string(),
                ));
            }
        }
    }
    let mut problems: Vec<String> = misnamed
        .iter()
        .filter(|(name, _)| !held.contains(&name.as_str()))
        .map(|(name, package)| {
            format!(
                "{package} builds the program {name}: every program is {CORE} or {PREFIX}<name>"
            )
        })
        .collect();
    problems.extend(
        held.iter()
            .filter(|name| !misnamed.iter().any(|(m, _)| m == *name))
            .map(|name| format!("{name} is held but no longer built: take it off the list")),
    );
    problems
}
