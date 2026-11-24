//! A field flag resolved as the reader resolves it, below a type within
//! itself too.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

/// A table whose free keys hold the table itself, as a box's
/// `[machine.<cloud>]` holds `[machine]`'s keys.
#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Nested {
    /// Its name.
    pub name: Option<String>,
    pub machine: Option<Machine>,
}

#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// Its size, as its cloud names one.
    pub size: Option<String>,
    /// Each cloud's own table, by its kind.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cloud: BTreeMap<String, Machine>,
}

#[test]
fn a_flag_resolves_as_the_reader_resolves_it_below_a_type_within_itself() {
    let nested = crate::schema::<Nested>();
    assert_eq!(flag_takes(&nested, "--spec.name"), Some(true));
    assert_eq!(flag_takes(&nested, "--nickname"), None);
    assert_eq!(flag_takes(&nested, "--name"), None);
    assert_eq!(flag_takes(&nested, "spec.name"), None);
    assert_eq!(flag_takes(&nested, "--machine.size"), Some(true));
    assert_eq!(flag_takes(&nested, "--machine.cloud.gcp.size"), Some(true));
    assert_eq!(
        flag_takes(&nested, "--machine.cloud.gcp.cloud.aws.size"),
        Some(true)
    );
    assert_eq!(flag_takes(&nested, "--machine.cloud.gcp.colour"), None);
}
