//! Every package takes its workspace's lint levels: the workspace's
//! `Cargo.toml` sets them under `[workspace.lints]`, and each member's
//! manifest says `[lints] workspace = true`, so a new package can't leave
//! them out. It reads only manifests, so each workspace runs it on its
//! own root.

use std::path::Path;

/// The members the workspace manifest `text` lists, or what's wrong with
/// it: it sets no `[workspace.lints]`, or names a member by a pattern,
/// which this rule doesn't expand.
pub fn members(text: &str) -> Result<Vec<String>, Vec<String>> {
    let manifest: toml::Table =
        toml::from_str(text).map_err(|e| vec![format!("Cargo.toml: {e}")])?;
    let workspace = manifest.get("workspace").and_then(toml::Value::as_table);
    let mut problems = Vec::new();
    if workspace.and_then(|w| w.get("lints")).is_none() {
        problems.push(
            "Cargo.toml sets no [workspace.lints]: give the workspace its lint levels".to_string(),
        );
    }
    let members: Vec<String> = workspace
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .map(|m| {
            m.iter()
                .filter_map(|m| m.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for member in &members {
        if member.contains(['*', '?', '[']) {
            problems.push(format!(
                "Cargo.toml's member {member} is a pattern: list each package by its path"
            ));
        }
    }
    if problems.is_empty() {
        Ok(members)
    } else {
        Err(problems)
    }
}

/// What's wrong with the package manifest `text`, shown as `file`: none
/// when it says `[lints] workspace = true`.
pub fn package(file: &str, text: &str) -> Option<String> {
    let manifest: toml::Table = match toml::from_str(text) {
        Ok(manifest) => manifest,
        Err(e) => return Some(format!("{file}: {e}")),
    };
    let lints = manifest.get("lints").and_then(toml::Value::as_table);
    let inherits = lints
        .and_then(|l| l.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    match lints {
        Some(lints) if inherits && lints.len() == 1 => None,
        Some(_) if inherits => Some(format!(
            "{file}: sets lints of its own beside the workspace's: set them in the workspace's [workspace.lints], or suppress one where it fires with #[expect(lint, reason = \"...\")]"
        )),
        _ => Some(format!(
            "{file}: takes no lint levels: add [lints] workspace = true"
        )),
    }
}

/// Every problem in the workspace at `root`: its manifest's, then each
/// member's that doesn't take the workspace's lint levels.
pub fn check(root: &Path) -> Vec<String> {
    let read = |path: &Path| {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    };
    let members = match members(&read(&root.join("Cargo.toml"))) {
        Ok(members) => members,
        Err(problems) => return problems,
    };
    members
        .iter()
        .filter_map(|member| {
            let file = format!("{member}/Cargo.toml");
            package(&file, &read(&root.join(&file)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_takes_the_workspace_levels_or_is_named() {
        let taking = "[package]\nname = \"a\"\n\n[lints]\nworkspace = true\n";
        assert_eq!(package("a/Cargo.toml", taking), None);
        let dotted = "lints.workspace = true\n\n[package]\nname = \"a\"\n";
        assert_eq!(package("a/Cargo.toml", dotted), None);
        for wrong in [
            "[package]\nname = \"a\"\n",
            "[package]\nname = \"a\"\n\n[lints]\nworkspace = false\n",
            "[package]\nname = \"a\"\n\n[lints.clippy]\ntodo = \"allow\"\n",
        ] {
            let said = package("a/Cargo.toml", wrong).expect(wrong);
            assert!(said.contains("[lints] workspace = true"), "{said}");
        }
        let beside = "[lints]\nworkspace = true\n\n[lints.rust]\nunused = \"allow\"\n";
        assert!(
            package("a/Cargo.toml", beside)
                .unwrap()
                .contains("of its own")
        );
    }

    #[test]
    fn a_workspace_sets_its_levels_and_lists_its_members() {
        let set = "[workspace]\nmembers = [\"a\", \"b/c\"]\n\n[workspace.lints.clippy]\ntodo = \"deny\"\n";
        assert_eq!(members(set).unwrap(), ["a", "b/c"]);
        let unset = "[workspace]\nmembers = [\"a\"]\n";
        assert!(members(unset).unwrap_err()[0].contains("[workspace.lints]"));
        let pattern =
            "[workspace]\nmembers = [\"modules/*\"]\n\n[workspace.lints.clippy]\ntodo = \"deny\"\n";
        assert!(members(pattern).unwrap_err()[0].contains("pattern"));
    }
}
