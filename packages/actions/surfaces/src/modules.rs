//! Which modules a workspace's generators read: every member of its Cargo
//! workspace whose folder holds a `module.toml`, but those still held,
//! whose declarations aren't in the current format yet.

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};

/// The module folders of the Cargo workspace at `workspace`, in member
/// order, leaving out the members `held` names by their path from it. A
/// held member that isn't a module is refused, so the list only shrinks.
pub fn discover(workspace: &Path, held: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let manifest = workspace.join("Cargo.toml");
    let text =
        std::fs::read_to_string(&manifest).with_context(|| manifest.display().to_string())?;
    let table: toml::Table =
        toml::from_str(&text).with_context(|| manifest.display().to_string())?;
    let members: Vec<String> = table
        .get("workspace")
        .and_then(|w| w.get("members"))
        .cloned()
        .context("a [workspace] with its members")?
        .try_into()
        .context("members lists folders")?;
    if let Some(pattern) = members.iter().find(|m| m.contains(['*', '?', '['])) {
        anyhow::bail!("member {pattern} is a pattern: list each member's folder");
    }
    ensure!(
        table
            .get("workspace")
            .and_then(|w| w.get("exclude"))
            .is_none(),
        "the workspace excludes folders: list only its members"
    );
    let file = link_actions::file_names()[0];
    let modules: Vec<&String> = members
        .iter()
        .filter(|member| workspace.join(member).join(file).is_file())
        .collect();
    for member in held {
        ensure!(
            modules.contains(&member),
            "held module {member} isn't a member holding a {file}: take it off the list"
        );
    }
    Ok(modules
        .into_iter()
        .filter(|member| !held.contains(member))
        .map(|member| workspace.join(member))
        .collect())
}

#[cfg(test)]
mod tests {
    use link_test_kit::temp::TempDir;

    #[test]
    fn members_holding_a_module_toml_are_modules_but_those_held() {
        let root = TempDir::new("surfaces-discover");
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\n",
        )
        .unwrap();
        for member in ["a", "b"] {
            std::fs::create_dir_all(root.join(member)).unwrap();
            std::fs::write(root.join(member).join("module.toml"), "").unwrap();
        }
        let found = super::discover(&root, &["b".into()]).unwrap();
        assert_eq!(found, [root.join("a")]);
        let refused = super::discover(&root, &["c".into()])
            .unwrap_err()
            .to_string();
        assert!(refused.contains("take it off"), "{refused}");
    }
}
