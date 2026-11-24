//! Which tracked files are production source discovery reads.
use super::*;

/// Production Rust and Cargo manifests outside test, fixture, data and
/// agent-session folders. Library code inside verification packages still
/// defines the typed catalogue.
pub(super) fn source_input(path: &Path) -> bool {
    let parts: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    if parts.iter().any(|part| {
        matches!(
            part.as_ref(),
            ".agents"
                | ".codex"
                | ".claude"
                | "target"
                | "vendor"
                | "node_modules"
                | "fixtures"
                | "data"
        )
    }) {
        return false;
    }
    if path
        .file_name()
        .is_some_and(|name| name == "Cargo.toml" || name == "Cargo.lock")
    {
        return true;
    }
    if path.extension().is_none_or(|ext| ext != "rs")
        || path.file_name().is_some_and(|name| name == "tests.rs")
    {
        return false;
    }
    if path.file_name().is_some_and(|name| name == "build.rs") {
        return true;
    }
    let Some(src) = parts.iter().rposition(|part| part == "src") else {
        return false;
    };
    !parts[src + 1..].iter().any(|part| part == "tests")
}
