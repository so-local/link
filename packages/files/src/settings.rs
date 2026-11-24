//! Trusted, typed TOML settings and their stable writer lock.

use std::path::Path;

mod constants;
use constants::MAX_BYTES;

fn document(path: &Path) -> anyhow::Result<Option<String>> {
    Ok(crate::fs::read_control_optional(path, MAX_BYTES, "settings'")?.map(|read| read.text))
}

/// A control document's text, trusted as settings are, or `None` when
/// it's missing.
pub fn text_optional(path: &Path) -> anyhow::Result<Option<String>> {
    document(path)
}

/// Read settings without relaxing their ownership, ancestor or symlink checks.
pub fn read<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    crate::fs::from_toml(&document(path)?.unwrap_or_default(), path)
}

/// Read an existing typed document, distinguishing a missing trusted file.
pub fn read_optional<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    document(path)?
        .map(|text| crate::fs::from_toml(&text, path))
        .transpose()
}

/// The stable sidecar lock shared by all writers of one settings document.
/// Check trust before creating the lock: making a directory private must never
/// turn an untrusted missing policy into an allowed default.
pub fn lock(path: &Path) -> anyhow::Result<crate::saved::Held> {
    // Check the folder before lock_file can make it private (saved::lock
    // checks it first). Read the file only after locking: another
    // legitimate writer can replace it meanwhile.
    let lock = crate::saved::lock(path)?;
    read_optional::<toml::Table>(path)?;
    Ok(lock)
}

#[cfg(test)]
mod tests;
