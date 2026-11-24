//! What the reader admits of an archive, read once from `limits.toml`.

use serde::Deserialize;

/// The limits an archive is read within.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    /// The most bytes its files hold together.
    pub max_bytes: u64,
    /// The most entries it holds.
    pub max_entries: u64,
    /// The longest path of an entry, in bytes.
    pub max_path_bytes: usize,
    /// The largest `archive.toml`.
    pub max_manifest_bytes: u64,
    /// The files it may hold at its root, beside its things' folders.
    pub root_files: Vec<String>,
    /// Its own record, its first entry.
    pub manifest: String,
}

/// The limits, from `limits.toml`.
pub fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("limits.toml")).expect("limits.toml is an archive's limits")
    });
    &LIMITS
}
