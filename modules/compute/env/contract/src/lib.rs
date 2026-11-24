//! Environment file and process-client contract. Execution belongs to lnk-env.

pub mod approvals;
mod checks;
pub mod cli;
pub mod declared;
pub mod environment;
pub mod file;
pub mod here;
pub mod profile;
pub mod remote;
pub mod stage;
pub mod values;
pub mod workloads;
/// Quote a word for a POSIX shell.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
link_test_kit::program!(unit);
