//! What a tool of an agent's was last defined from, kept beside its
//! settings (`~/.lnk/safe/agents/<name>/<tool>.json`) while it's defined:
//! a start that would define it the same leaves it as it is. Each record
//! names its schema, declared beside its type.

use std::path::Path;

use link_files::schema::Specification;
use link_format::WithinExt;
use serde::{Deserialize, Serialize};

mod catalogue;
pub(crate) use catalogue::{DefinedTool, list_tools};

/// A tool served by a program of Link's (its scheduler, `run`, `file`,
/// `code`, the browser and the desktop): its command line, when its
/// program last changed, and the Link that defined it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub argv: Vec<String>,
    /// The complete explicit environment's digest; never its values.
    pub environment: String,
    /// When its program last changed, in nanoseconds since 1970.
    pub modified: Option<u64>,
    /// The scheduler's alone: whether the agent is sandboxed, and whether
    /// its calls ask first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<bool>,
    /// The scheduler's alone: the calls that ask by an argument, as the
    /// sandbox takes them (`--ask-if`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ask_if: Vec<String>,
    /// The version of the Link that defined it.
    pub lnk: String,
}

link_files::specification!(Tool, include_str!("defined/tool.schema.toml"));

/// Each agent tool's record, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Tool>()
}

impl Tool {
    /// A tool served as `argv`, whose program is `program`.
    pub fn new(argv: Vec<String>, program: &Path) -> Tool {
        Tool {
            argv,
            environment: link_digest::sha256(b"{}"),
            modified: modified(program),
            sandbox: None,
            ask: None,
            ask_if: Vec::new(),
            lnk: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub fn environment(
        &mut self,
        values: &std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        self.environment = link_digest::sha256(&serde_json::to_vec(values)?);
        Ok(())
    }
}

/// When `program` last changed, in nanoseconds since 1970: none when it
/// can't be read.
pub fn modified(program: &Path) -> Option<u64> {
    std::fs::metadata(program)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_nanos()).ok())
}

/// The record kept at `path`, its schema advanced: none when there's none
/// or it can't be read, so the tool is defined again.
pub fn kept<T: Specification>(path: &Path) -> Option<T> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| link_files::saved::parse_json(&bytes).ok())
}

/// Keeps `record` at `path`, naming its schema.
pub fn keep<T: Specification>(path: &Path, record: &T) -> anyhow::Result<()> {
    link_files::write_private(path, &link_files::saved::print_json(record)?)
        .within(format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_full_tool_environment_participates_in_its_cache_key_without_retaining_values() {
        let mut one = Tool::new(vec!["serve".into()], std::path::Path::new("/missing"));
        let initial = one.clone();
        let mut env = std::collections::BTreeMap::from([(
            "LNK_RUN_CONTEXT".into(),
            "one-private-context".into(),
        )]);
        one.environment(&env).unwrap();
        assert_ne!(one, initial);
        let mut two = one.clone();
        env.insert("LNK_RUN_CONTEXT".into(), "another-private-context".into());
        two.environment(&env).unwrap();
        assert_ne!(one, two);
        assert!(
            !serde_json::to_string(&one)
                .unwrap()
                .contains("one-private-context")
        );
    }

    /// A record kept is read back as it was, naming its schema; one of a
    /// newer schema isn't, so its tool is defined again.
    #[test]
    fn a_record_is_kept_naming_its_schema() {
        let dir = link_test_kit::TempDir::new("defined");
        let path = dir.join("run.json");
        assert_eq!(kept::<Tool>(&path), None);
        let tool = Tool::new(vec!["lnk-run-tool".into(), "serve".into()], &path);
        keep(&path, &tool).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"schema\":1"), "{text}");
        assert_eq!(kept::<Tool>(&path), Some(tool));
        std::fs::write(&path, text.replace("\"schema\":1", "\"schema\":2")).unwrap();
        assert_eq!(kept::<Tool>(&path), None);
    }
}
