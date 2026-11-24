//! The Keychain's limits, each named once.

use std::time::Duration;

/// How long `security` has to answer. It answers in well under a second
/// unless it is showing a person a dialog, which Link never waits on.
pub const ANSWERS_WITHIN: Duration = Duration::from_secs(15);
