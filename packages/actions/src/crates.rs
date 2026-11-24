//! Every crate's `variables.toml`, read into one catalog: each variable's
//! one definition and every crate's use of it, checked so a variable has
//! one home and every use names a defined variable.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::read::variables::{Definition, Variables};

/// The crates' variables, by each crate's folder.
#[derive(Clone, Debug, Default)]
pub struct Crates {
    pub crates: BTreeMap<PathBuf, Variables>,
}

/// Folders no crate's declarations are in.
const SKIPPED: [&str; 3] = ["target", "node_modules", "fixtures"];

impl Crates {
    /// Every crate under `roots` with a `variables.toml` beside its
    /// `Cargo.toml`.
    pub fn load(roots: &[&Path]) -> anyhow::Result<Self> {
        let mut crates = Self::default();
        for root in roots {
            crates.walk(root)?;
        }
        Ok(crates)
    }

    fn walk(&mut self, directory: &Path) -> anyhow::Result<()> {
        let name = &crate::config::SETTINGS.files.variables;
        if directory.join("Cargo.toml").is_file()
            && let Some(text) = crate::read::text(directory, name)?
        {
            self.add(directory, &text)?;
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
            .with_context(|| format!("reading {}", directory.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if path.is_symlink()
                || !path.is_dir()
                || file.starts_with('.')
                || SKIPPED.contains(&file)
            {
                continue;
            }
            self.walk(&path)?;
        }
        Ok(())
    }

    /// The crate in `directory`, its `variables.toml` as `text`.
    pub fn add(&mut self, directory: &Path, text: &str) -> anyhow::Result<()> {
        let variables = crate::read::variables::read(text)
            .with_context(|| format!("{}", directory.join("variables.toml").display()))?;
        self.crates.insert(directory.to_path_buf(), variables);
        Ok(())
    }

    /// A variable's one definition, and the crate it's in.
    pub fn definition(&self, name: &str) -> Option<(&Path, &Definition)> {
        self.crates.iter().find_map(|(directory, variables)| {
            variables
                .defining(name)
                .map(|definition| (directory.as_path(), definition))
        })
    }

    /// Every variable defined, with the crate defining it.
    pub fn defined(&self) -> BTreeMap<&str, &Path> {
        let mut all = BTreeMap::new();
        for (directory, variables) in &self.crates {
            for name in variables.defines.keys() {
                all.entry(name.as_str()).or_insert(directory.as_path());
            }
        }
        all
    }

    /// Each problem: a variable two crates define, and a use of one no
    /// crate defines.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut first: BTreeMap<&str, &Path> = BTreeMap::new();
        for (directory, variables) in &self.crates {
            for name in variables.defines.keys() {
                if let Some(other) = first.insert(name, directory) {
                    problems.push(format!(
                        "{name} is defined by both {} and {}: one crate defines it, and the other declares its use",
                        other.display(),
                        directory.display()
                    ));
                }
            }
        }
        for (directory, variables) in &self.crates {
            for one in &variables.uses {
                if !first.contains_key(one.name.as_str()) && self.definition(&one.name).is_none() {
                    problems.push(format!(
                        "{}: {} is used, and no crate's variables.toml defines it",
                        directory.display(),
                        one.name
                    ));
                }
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crates(files: &[(&str, &str)]) -> Crates {
        let mut crates = Crates::default();
        for (directory, text) in files {
            crates.add(Path::new(directory), text).unwrap();
        }
        crates
    }

    const DEFINES: &str = "schema = 1\n[defines.LNK_A]\ntype = \"string\"\ndescription = \"A\"\n";
    const USES: &str =
        "schema = 1\n[[uses]]\nname = \"LNK_A\"\naccess = [\"read\"]\ncontext = \"runtime\"\n";

    #[test]
    fn a_use_names_a_variable_another_crate_defines() {
        let crates = crates(&[("a", DEFINES), ("b", USES)]);
        assert!(crates.problems().is_empty());
        assert_eq!(crates.definition("LNK_A").unwrap().0, Path::new("a"));
    }

    /// A family of variables, `LNK_TOOL_*`, is defined once, and two
    /// crates use it, by the family or by one member.
    #[test]
    fn a_family_is_defined_once_for_every_member() {
        let family = "schema = 1\n[defines.\"LNK_TOOL_*\"]\ntype = \"string\"\n\
                      description = \"A tool's command\"\nsuffix = \"The tool's name in capitals\"\n";
        let link = "schema = 1\n[[uses]]\nname = \"LNK_TOOL_*\"\naccess = [\"read\"]\ncontext = \"runtime\"\n";
        let mini = "schema = 1\n[[uses]]\nname = \"LNK_TOOL_WEB\"\naccess = [\"read\"]\ncontext = \"runtime\"\n";
        let crates = crates(&[("agent", family), ("link", link), ("mini", mini)]);
        assert!(crates.problems().is_empty(), "{:?}", crates.problems());
        let (home, definition) = crates.definition("LNK_TOOL_WEB").unwrap();
        assert_eq!(home, Path::new("agent"));
        assert_eq!(
            definition.suffix.as_deref(),
            Some("The tool's name in capitals")
        );
        assert!(
            crates.definition("LNK_TOOL_").is_none(),
            "a member names something"
        );
        let unsaid =
            "schema = 1\n[defines.\"LNK_TOOL_*\"]\ntype = \"string\"\ndescription = \"x\"\n";
        let refused = format!("{:#}", crate::read::variables::read(unsaid).unwrap_err());
        assert!(refused.contains("says what its * names"), "{refused}");
    }

    #[test]
    fn a_variable_has_one_home_and_every_use_a_definition() {
        let problems = crates(&[("a", DEFINES), ("b", DEFINES)]).problems();
        assert!(
            problems[0].contains("defined by both a and b"),
            "{problems:?}"
        );
        let problems = crates(&[("b", USES)]).problems();
        assert!(
            problems[0].contains("no crate's variables.toml defines it"),
            "{problems:?}"
        );
    }

    #[test]
    fn namespaces_and_readers_say_what_they_read() {
        let mut crates = Crates::default();
        for text in [
            "schema = 1\n[namespaces]\nLNK_A = \"no star\"\n",
            "schema = 1\n[namespaces]\n\"LNK_*\" = \"\"\n",
            "schema = 1\n[namespaces]\n\"*_*\" = \"Every name\"\n",
            "schema = 1\n[readers]\n\"a b\" = \"Reads\"\n",
            "schema = 1\n[defines.LNK_A]\ntype = \"string\"\ndescription = \" \"\n",
            "schema = 1\n[defines.lnk_a]\ntype = \"string\"\ndescription = \"A\"\n",
        ] {
            assert!(crates.add(Path::new("a"), text).is_err(), "{text}");
        }
        crates
            .add(
                Path::new("a"),
                "schema = 1\n[namespaces]\n\"LNK_*\" = \"A program's name\"\n[readers]\n\"ask::given\" = \"A question's variable\"\n",
            )
            .unwrap();
    }
}
