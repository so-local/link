//! The connection contract's values: read here, used by their one name.

use std::time::Duration;

/// How long a harness a switch started has to take in the conversations
/// from elsewhere, from when it first answers (its install and start
/// count against none of it), and so the longest a message waits for
/// them ([`crate::arriving`]).
pub const ARRIVING: Duration = Duration::from_secs(300);

/// How often a waiting message looks whether they've arrived.
pub const ARRIVING_LOOK: Duration = Duration::from_millis(250);

/// How long an agent woken from idle may take to answer: its door holds a
/// connection that long while it wakes the agent, then closes it, so a
/// bridge's challenge on that connection waits as long for its answer
/// ([`crate::connection`]'s proof).
pub const WAKING: Duration = Duration::from_secs(120);

/// The most characters of a tool call's line an answer being written
/// shows (`› run command cargo test`): a longer title is cut with `…`.
pub const CALL_LINE_MAX: usize = 200;
