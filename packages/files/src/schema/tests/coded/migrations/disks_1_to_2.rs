//! Disks 1 to 2: schema 1's `disk = "20G"`, its size and unit in one
//! string, split by its value into schema 2's `[disk]` with `size` and
//! `unit`.

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::schema::Body;

pub(super) fn run(mut body: Body) -> anyhow::Result<Body> {
    let Some(disk) = body.remove("disk") else {
        return Ok(body);
    };
    let Value::String(disk) = disk else {
        bail!("disk is a size and unit, as \"20G\"");
    };
    let at = disk
        .find(|c: char| !c.is_ascii_digit())
        .with_context(|| format!("disk {disk:?} has no unit: write it as \"20G\""))?;
    let (size, unit) = disk.split_at(at);
    let size: u32 = size.parse().with_context(|| {
        format!("disk {disk:?} doesn't start with its size: write it as \"20G\"")
    })?;
    body.insert("disk".into(), json!({ "size": size, "unit": unit }));
    Ok(body)
}
