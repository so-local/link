//! Cargo invocation without per-test package variables. The caller supplies
//! its environment-filtered command constructor.
use std::process::Command;

pub fn for_test(new: impl FnOnce(std::ffi::OsString) -> Command) -> Command {
    let mut command = new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(set_for_tests) {
            command.env_remove(name);
        }
    }
    command
}

pub fn set_for_tests(name: &str) -> bool {
    ["CARGO_MANIFEST_", "CARGO_PKG_", "CARGO_BIN_EXE_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || [
            "CARGO_CRATE_NAME",
            "CARGO_BIN_NAME",
            "CARGO_PRIMARY_PACKAGE",
            "CARGO_TARGET_TMPDIR",
            "CARGO_RUSTC_CURRENT_DIR",
        ]
        .contains(&name)
}
