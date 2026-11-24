//! The env module: `lnk env`. An environment is one file saying what a
//! machine needs before work starts: its setup in stages, each in its own
//! sandbox, variables and services. What runs in
//! it is an agent, a person over SSH, or stages handed to it to run to
//! their end (`lnk env here`'s run), such as a job's.
//!
//! What runs on the machine is this module there (`lnk env here`), asked
//! on its input. A box is `lnk box`'s, reached by running `lnk`, as a
//! sandbox is `lnk sandbox`'s; `lnk env create` makes one from an
//! environment through it.

pub mod carried;
pub mod cli;
pub use link_env_contract::approvals;
pub use link_env_contract::environment;
pub use link_env_contract::file;
pub mod here;
pub use link_env_contract::remote;
pub use link_env_contract::stage;
pub use link_env_contract::values;
pub use link_env_contract::workloads;

/// `text` quoted for a POSIX shell.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
link_test_kit::program!(unit);
