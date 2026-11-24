//! Link folder layout, persisted application settings and command output setup.

pub mod execution;
pub mod folders;
pub mod output;
pub use folders::{link_dir, try_link_dir};

#[cfg(test)]
link_test_kit::program!(unit);
