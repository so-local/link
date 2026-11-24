//! One protected transaction boundary for questions, approvals and controls.

pub use link_interactions_contract::*;
pub mod capabilities;
pub mod chains;
mod constants;
pub mod control;
pub mod forms;
mod host;
mod lifecycle;
pub mod native;
pub mod owner;
pub mod presence;
pub mod render;
pub mod reply;
mod retention;
pub mod runtime;
pub mod sources;
mod store;
mod validation;
pub use constants::STORE_VAR;
pub use host::Host;
pub use store::{Snapshot, Store, SubmitDisposition};

#[cfg(test)]
mod tests;

/// Wall-clock deadlines include delivery and queueing time.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
link_test_kit::program!(unit);
