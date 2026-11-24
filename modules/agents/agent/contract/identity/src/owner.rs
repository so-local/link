//! Who an agent is, and the variables an adapter is told it by. An
//! owner's grammar is every machine's ([`link_machine::owner`]).

pub use link_machine::owner::{OWNER_MAX, check_owner, is_local};

/// Who an agent is: its ID, its service, its owner, and its teams' IDs.
pub use link_sandbox_bridge::Identity;

/// Agent id in an adapter process.
pub const AGENT_ID_ENV: &str = "LNK_AGENT_ID";
/// Owner in an adapter process.
pub const OWNER_ENV: &str = "LNK_OWNER";
/// The service the agent acts as, its bare ID, in an adapter process.
pub const SERVICE_ENV: &str = "LNK_AGENT_SERVICE";
