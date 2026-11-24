//! Source and documentation measures shared by the survey and ratio check.

use super::held;
use crate::{Finding, Tree, fail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A package's code and its docs, in lines with words on them: the Rust
/// in its `src/`, and its README and `docs/` pages, without the
/// packages inside it or what the docs writer generates.
#[derive(Debug, Clone, Default)]
pub struct Measure {
    pub package: PathBuf,
    pub rust: usize,
    pub docs: usize,
    pub pages: usize,
    pub production: usize,
    pub tests: usize,
    pub generated: usize,
    pub words: usize,
    pub terminal_examples: usize,
    pub supporting_pages: usize,
    pub problems: Vec<String>,
    pub example_problems: Vec<String>,
}

impl Measure {
    /// The docs lines its Rust predicts: its lines raised to the curve's
    /// exponent.
    pub fn expected(&self, exponent: f64) -> f64 {
        (self.rust as f64).powf(exponent)
    }

    /// How many times more, or fewer, docs lines it has than its Rust
    /// predicts: never less than 1.
    pub fn off_by(&self, exponent: f64) -> f64 {
        let expected = self.expected(exponent);
        let docs = self.docs.max(1) as f64;
        (docs / expected).max(expected / docs)
    }
}

/// The workspace's packages, those under a `fixtures/` folder left out.
pub fn packages(tree: &Tree) -> Vec<PathBuf> {
    let mut packages: Vec<PathBuf> = crate::shape::packages(tree)
        .into_iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "fixtures"))
        .collect();
    packages.extend(tree.files.iter().filter_map(|(path, text)| {
        if path.file_name().is_none_or(|name| name != "package.json")
            || path.components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some("fixtures" | "node_modules" | "vendor")
                )
            })
        {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(text)
            .ok()?
            .get("name")?
            .as_str()?;
        Some(path.parent()?.to_path_buf())
    }));
    packages.sort();
    packages.dedup();
    packages
}

/// Each package's [`Measure`].
pub fn measures(tree: &Tree) -> Vec<Measure> {
    let packages = packages(tree);
    let owner = |path: &Path| {
        packages
            .iter()
            .filter(|p| path.starts_with(p))
            .max_by_key(|p| p.as_os_str().len())
    };
    let mut out: BTreeMap<&PathBuf, Measure> = packages
        .iter()
        .map(|p| {
            let measure = Measure {
                package: p.clone(),
                ..Measure::default()
            };
            (p, measure)
        })
        .collect();
    let lines = |text: &str| text.lines().filter(|l| !l.trim().is_empty()).count();
    for (path, text) in &tree.files {
        let Some(package) = owner(path) else {
            continue;
        };
        let measure = out.get_mut(package).expect("a package's measure");
        let rel = path.strip_prefix(package).expect("in its package");
        if path.extension().is_some_and(|e| e == "rs")
            && (rel.starts_with("src") || rel.starts_with("tests"))
        {
            if tree.limits.generated.contains(&tree.shown(path))
                || text
                    .lines()
                    .next()
                    .is_some_and(|line| line.starts_with("// @generated"))
            {
                measure.generated += lines(text);
                continue;
            }
            let test_file = rel.starts_with("tests")
                || rel.components().any(|part| part.as_os_str() == "tests")
                || path
                    .file_stem()
                    .is_some_and(|name| name.to_string_lossy().ends_with("_tests"));
            match crate::rust_source::lines(text, test_file) {
                Ok(count) => {
                    measure.production += count.production;
                    measure.tests += count.tests;
                    if rel.starts_with("src") {
                        measure.rust += count.source;
                    }
                }
                Err(error) => measure
                    .problems
                    .push(format!("{}: {error}", tree.shown(path))),
            }
        } else if ["actions.toml", "types.toml", "variables.toml"]
            .iter()
            .any(|name| rel == Path::new(name))
            && text.lines().any(|line| line.trim() == "schema = 1")
        {
            // A module's version 1 declarations are its source: its
            // command line and shapes are generated from them.
            let declared = text
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .count();
            measure.production += declared;
            measure.rust += declared;
        } else if (rel == Path::new("README.md")
            || (rel.starts_with("docs") && path.extension().is_some_and(|e| e == "md")))
            // A generated reference is its declarations' text, counted there.
            && !crate::generated_reference(text)
        {
            let text = crate::navigation::explanations(text);
            measure.docs += lines(&text);
            measure.words += text
                .split_whitespace()
                .filter(|word| word.chars().any(char::is_alphanumeric))
                .count();
            let examples = crate::examples::terminal_commands(&text);
            measure.terminal_examples += examples.invocations.len();
            measure.example_problems.extend(
                examples
                    .problems
                    .into_iter()
                    .map(|problem| format!("{}: {problem}", tree.shown(path))),
            );
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            measure.supporting_pages += usize::from(
                ["security", "decisions", "troubleshooting"]
                    .iter()
                    .any(|kind| name == *kind || name.ends_with(&format!("-{kind}"))),
            );
            measure.pages += 1;
        }
    }
    out.into_values()
        .map(|mut measure| {
            // A test-only package's implementation is its maintained test program,
            // not a crate-level comment in an otherwise empty library stub.
            if measure.production == 0 {
                measure.rust = measure.tests;
            }
            measure
        })
        .collect()
}

/// A package's docs lines are within `docs-ratio`'s factor of what its
/// Rust predicts, either way.
pub(super) fn docs_ratio(tree: &Tree) -> Vec<Finding> {
    let limit = &tree.limits.limit["docs-ratio"];
    let exponent = limit.exponent.expect("docs-ratio's exponent");
    let factor = limit.at_most;
    let mut found = Vec::new();
    for measure in measures(tree) {
        if !measure.problems.is_empty() {
            found.extend(measure.problems.into_iter().map(fail));
            continue;
        }
        if measure.rust == 0 {
            continue;
        }
        let off = measure.off_by(exponent);
        let ok = off <= factor;
        found.extend(held(
            tree,
            "docs-ratio",
            &tree.shown(&measure.package),
            ok,
            || {
                format!(
                    "{}: {} docs lines for {} Rust lines, {off:.1} times off the {:.0} expected",
                    tree.shown(&measure.package),
                    measure.docs,
                    measure.rust,
                    measure.expected(exponent)
                )
            },
        ));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_programs_supply_the_ratio_without_counting_nested_owners_twice() {
        let root = link_test_kit::TempDir::new("owner-measures");
        for (name, text) in [
            ("large.toml", "[lines]\n"),
            (
                "suite/Cargo.toml",
                "[package]\nname='suite'\nversion='0.1.0'\n",
            ),
            ("suite/src/lib.rs", "//! Test program.\n"),
            ("suite/tests/check.rs", "#[test]\nfn checks() {}\n"),
            (
                "suite/child/Cargo.toml",
                "[package]\nname='child'\nversion='0.1.0'\n",
            ),
            ("suite/child/src/lib.rs", "pub fn child() {}\n"),
            ("site/package.json", "{\"name\":\"site\"}"),
            ("site/README.md", "# Site\n\nThe reader's guide.\n"),
        ] {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let tree = Tree::read(&root, false, &root.join("large.toml"));
        let measures = measures(&tree);
        let suite = measures
            .iter()
            .find(|m| m.package.ends_with("suite"))
            .unwrap();
        assert_eq!((suite.rust, suite.production, suite.tests), (2, 0, 2));
        let child = measures
            .iter()
            .find(|m| m.package.ends_with("child"))
            .unwrap();
        assert_eq!((child.rust, child.production, child.tests), (1, 1, 0));
        let site = measures
            .iter()
            .find(|m| m.package.ends_with("site"))
            .unwrap();
        assert_eq!((site.rust, site.pages), (0, 1));
    }

    /// A reference written from a module's declarations is counted as
    /// those declarations, never again as its docs.
    #[test]
    fn a_generated_reference_is_no_docs_lines() {
        let root = link_test_kit::TempDir::new("generated-reference");
        let reference = format!(
            "<!-- {} -->\n# Mod Commands\n\nEvery command.\n",
            link_actions_surfaces::marker()
        );
        for (name, text) in [
            ("large.toml", "[lines]\n".to_string()),
            (
                "module/Cargo.toml",
                "[package]\nname='module'\nversion='0.1.0'\n".to_string(),
            ),
            ("module/src/lib.rs", "pub fn module() {}\n".to_string()),
            (
                "module/README.md",
                "# Module\n\nWhat it does.\n".to_string(),
            ),
            ("module/docs/more/reference/modules/module.md", reference),
        ] {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let tree = Tree::read(&root, false, &root.join("large.toml"));
        let module = measures(&tree)
            .into_iter()
            .find(|m| m.package.ends_with("module"))
            .unwrap();
        assert_eq!((module.docs, module.pages), (2, 1));
    }
}
