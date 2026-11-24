//! The settings' `name`, taken as `machine`, made the one entry of
//! `machines`.

use anyhow::Context;
use serde_json::json;

use crate::schema::Body;

pub(super) fn run(mut taken: Body) -> anyhow::Result<Body> {
    let machine = taken.remove("machine").context("a taken machine")?;
    let mut made = Body::new();
    made.insert("machines".into(), json!([machine]));
    Ok(made)
}
