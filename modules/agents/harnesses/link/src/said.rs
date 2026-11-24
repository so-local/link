//! What the harness says about its run while it serves, written outside
//! any command's `Out` through `link-format`'s log ([`link_format::log`]):
//! a record a line, with its time and the IDs that place it, where its
//! stderr is the agent's log; else a line on its terminal.

pub use link_format::log::Ids;
use link_format::log::{Level, say};

/// A line about the run, placed by `ids`.
pub fn note(ids: Ids, text: &str) {
    say(Level::Info, text, ids);
}

/// Something that went wrong that the harness goes on after, placed by
/// `ids`.
pub fn warning(ids: Ids, text: &str) {
    say(Level::Warning, text, ids);
}

/// Its stderr made its log, when it's no terminal: its agent's when it
/// serves one (`agent`), the rest naming their own.
pub fn to_log(agent: Option<&str>) {
    if !link_format::streams::stderr_is_terminal() {
        link_format::log::to_stderr(agent);
    }
}

/// [`note`], formatted as `format!` does, perhaps placed by IDs first
/// (`note!(ids => "...")`).
#[macro_export]
macro_rules! note {
    ($ids:expr => $($t:tt)*) => { $crate::said::note($ids, &format!($($t)*)) };
    ($($t:tt)*) => { $crate::said::note($crate::said::Ids::default(), &format!($($t)*)) };
}

/// [`warning`], formatted as `format!` does, perhaps placed by IDs first
/// (`warn!(ids => "...")`).
#[macro_export]
macro_rules! warn {
    ($ids:expr => $($t:tt)*) => { $crate::said::warning($ids, &format!($($t)*)) };
    ($($t:tt)*) => { $crate::said::warning($crate::said::Ids::default(), &format!($($t)*)) };
}

/// The IDs that place a line about `agent`'s conversation `thread`.
pub fn in_chat(agent: &crate::agent::Agent, thread: &crate::agent::Thread) -> Ids {
    Ids::agent(agent.id()).chat(thread.name())
}
