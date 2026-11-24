//! The five files as written, each read strictly in the current format:
//! a key the format doesn't read is refused by name.

pub(crate) mod actions;
pub(crate) mod module;
pub(crate) mod schema;
pub(crate) mod types;
pub(crate) mod variables;

use anyhow::{Context, ensure};
use std::path::Path;

/// A file's text, or none where the module has no such file.
pub(crate) fn text(directory: &Path, name: &str) -> anyhow::Result<Option<String>> {
    let path = directory.join(name);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    ensure!(metadata.is_file(), "{} is a regular file", path.display());
    ensure!(
        metadata.len() <= crate::config::SETTINGS.files.max_bytes,
        "{} is over the format's file limit",
        path.display()
    );
    std::fs::read_to_string(&path)
        .map(Some)
        .with_context(|| format!("reading {}", path.display()))
}

/// A file's table with its `schema` line taken out and checked.
pub(crate) fn versioned(text: &str) -> anyhow::Result<toml::Table> {
    let mut table: toml::Table = toml::from_str(text)?;
    let schema = table
        .remove("schema")
        .context("its first line names its format: schema = 1")?;
    let current = crate::config::SETTINGS.format.schema;
    ensure!(
        schema.as_integer() == Some(i64::from(current)),
        "schema {schema} isn't the format this build reads, schema = {current}"
    );
    Ok(table)
}
