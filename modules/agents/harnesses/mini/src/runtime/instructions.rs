//! One instruction file, read once before a turn. There is no instruction discovery.

use std::path::Path;

/// Read the instruction file verbatim. A missing file means no instruction.
/// Other errors are reported instead of silently dropping the user's instruction.
pub fn read_instructions(path: &Path) -> anyhow::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(anyhow::Error::new(error).context("reading Mini's instruction file")),
    }
}

#[cfg(test)]
#[path = "../../tests/runtime/instructions.rs"]
mod tests;
