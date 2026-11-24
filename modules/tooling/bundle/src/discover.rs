//! Which workspace builds each program: every binary of every package of
//! each source, as `cargo metadata` lists them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::Value;

/// A program a source builds.
#[derive(Clone, Debug)]
pub struct Found {
    /// The package that builds it.
    pub package: String,
    /// The package's manifest.
    pub manifest: PathBuf,
    /// The workspace's target folder.
    pub target_dir: PathBuf,
    /// The source it was found in.
    pub source: PathBuf,
    /// Whether it's a module: a package's one `lnk-` program, not a fixture.
    pub module: bool,
    /// Whether a release includes it (`released = false` keeps it out).
    pub released: bool,
}

/// Every program the sources build, by its file name. A name two sources
/// build is refused, since a bundle holds one of each.
pub fn programs(sources: &[PathBuf]) -> anyhow::Result<BTreeMap<String, Found>> {
    let mut found: BTreeMap<String, Found> = BTreeMap::new();
    for source in sources {
        for (name, program) in of(source)? {
            if let Some(other) = found.get(&name) {
                bail!(
                    "{name} is built by both {} and {}: a bundle holds one",
                    other.source.display(),
                    source.display()
                );
            }
            found.insert(name, program);
        }
    }
    Ok(found)
}

fn of(source: &Path) -> anyhow::Result<Vec<(String, Found)>> {
    let manifest = source.join("Cargo.toml");
    let text = crate::run(
        "cargo",
        &[
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            &manifest.to_string_lossy(),
        ],
        source,
    )?;
    let metadata: Value = serde_json::from_str(&text).context("reading cargo metadata")?;
    let target_dir = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .context("cargo metadata names no target folder")?,
    );
    let mut found = Vec::new();
    for package in metadata["packages"].as_array().into_iter().flatten() {
        let bins: Vec<&str> = package["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                t["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|k| k == "bin"))
            })
            .filter_map(|t| t["name"].as_str())
            .collect();
        let link = &package["metadata"]["link"];
        let fixture = link["role"] == "fixture";
        let modules = bins.iter().filter(|b| b.starts_with("lnk-")).count();
        for bin in &bins {
            found.push((
                bin.to_string(),
                Found {
                    package: package["name"].as_str().unwrap_or_default().to_string(),
                    manifest: PathBuf::from(package["manifest_path"].as_str().unwrap_or_default()),
                    target_dir: target_dir.clone(),
                    source: source.to_path_buf(),
                    module: !fixture && modules == 1 && bin.starts_with("lnk-"),
                    released: link["released"] != false,
                },
            ));
        }
    }
    Ok(found)
}

/// The programs a bundle holds when none are named: `lnk` and every module
/// of the first source, leaving out what a release leaves out.
pub fn default_programs(
    found: &BTreeMap<String, Found>,
    first: &Path,
    release: bool,
) -> Vec<String> {
    found
        .iter()
        .filter(|(name, f)| {
            f.source == first && (*name == "lnk" || (f.module && (!release || f.released)))
        })
        .map(|(name, _)| name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_leaves_out_what_isnt_released() {
        let source = PathBuf::from("/w");
        let program = |module: bool, released: bool| Found {
            package: String::new(),
            manifest: PathBuf::new(),
            target_dir: PathBuf::new(),
            source: source.clone(),
            module,
            released,
        };
        let found = BTreeMap::from([
            ("lnk".to_string(), program(false, true)),
            ("lnk-agent".to_string(), program(true, true)),
            ("lnk-box".to_string(), program(true, false)),
            ("lnk-fixture".to_string(), program(false, true)),
        ]);
        assert_eq!(
            default_programs(&found, &source, true),
            ["lnk", "lnk-agent"]
        );
        assert_eq!(
            default_programs(&found, &source, false),
            ["lnk", "lnk-agent", "lnk-box"]
        );
        assert!(default_programs(&found, Path::new("/other"), false).is_empty());
    }
}
