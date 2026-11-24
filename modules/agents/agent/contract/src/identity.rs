//! Who an agent is, and whose: its ID ([`link_id::candidate`]),
//! its owner, and the teams it's on. A name is for people; Link keys
//! everything of an agent's on its ID and its owner's, so two owners'
//! `main` never meet. An owner is every machine's
//! ([`link_machine::owner`]); the owner comes from the agent's settings
//! on the machine, never from a request.

/// What each of an agent's own secrets is for, as the secrets' store
/// keeps it.
pub use link_agent_identity::own::OwnSecret;

/// The agent's ID, in an adapter's environment beside `LNK_AGENT` (its
/// name).
pub use link_agent_identity::owner::AGENT_ID_ENV;

/// The agent's owner, in an adapter's environment.
pub use link_agent_identity::owner::OWNER_ENV;

/// The service the agent acts as, in an adapter's environment.
pub use link_agent_identity::owner::SERVICE_ENV;

pub use link_agent_identity::owner::Identity;
pub use link_machine::owner::*;
