//! The model module's fixed values.

use std::time::Duration;

/// How long `lnk model serve --json` answers the requests it already took
/// once its owner closes its stdin, its port already closed: then it ends.
pub const ANSWERING_AFTER_OWNER: Duration = Duration::from_secs(10);
