//! The sandbox contract's values: read here, used by their one name.

/// The most of bubblewrap's report of its sandbox read
/// ([`crate::bubblewrap::read_info`]): it names a process and a few
/// namespaces.
pub const MOST_INFO: u64 = 4096;

/// Bubblewrap's program as the system names its processes
/// (`/proc/<pid>/comm`): what a process in front of a sandbox ends when
/// bubblewrap leaves its first process running
/// (`link_secretive::process::end_left`).
pub const BUBBLEWRAP: &str = "bwrap";
