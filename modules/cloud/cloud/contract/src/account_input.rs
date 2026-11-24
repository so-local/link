//! Exact connected-account transfer between controlled execution contexts.
//! The input is credential-bearing and belongs only on a guarded stdin pipe.

use serde::{Deserialize, Serialize};

use crate::Account;

pub const SCHEMA_ARGUMENT: &str = "account-input-schema";
pub const IMPORT_ARGUMENT: &str = "account-input";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub version: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub version: u32,
    pub account: Account,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accepted {
    pub version: u32,
    pub name: String,
    pub kind: String,
}
