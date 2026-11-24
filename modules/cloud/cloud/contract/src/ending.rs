//! A disposable machine's first-boot ending policy, read by `lnk box end-here`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub credential_url: String,
    pub credential_headers: BTreeMap<String, String>,
    pub remove_url: String,
}
