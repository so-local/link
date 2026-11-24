//! `types.toml`: each top-level table one named shape.

use std::collections::BTreeMap;

use anyhow::{Context, bail, ensure};
use serde::Deserialize;

use crate::schema::Schema;

/// One named shape as declared, its names not yet resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Declared {
    pub schema: Schema,
    /// `x-link-spec`: the spec kind it is, unique across Link.
    pub spec: Option<String>,
    /// `x-link-resource`: the property that is its ID.
    pub resource: Option<String>,
    /// `x-link-checks`: the hooks its value is checked by too, each a
    /// function of its own crate's `checks` module, wherever it's read.
    pub checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    id: String,
}

pub(crate) fn read(text: &str, module: &str) -> anyhow::Result<BTreeMap<String, Declared>> {
    let table = super::versioned(text)?;
    let mut out = BTreeMap::new();
    for (name, value) in table {
        ensure!(
            crate::names::is_name(&name),
            "shape {name:?}: a shape's name is a lowercase name"
        );
        let declared = shape(value, module).with_context(|| format!("shape {name}"))?;
        out.insert(name, declared);
    }
    Ok(out)
}

fn shape(value: toml::Value, module: &str) -> anyhow::Result<Declared> {
    let toml::Value::Table(mut table) = value else {
        bail!("a shape is a table")
    };
    let spec = table
        .remove("x-link-spec")
        .map(toml::Value::try_into::<Spec>)
        .transpose()
        .context("x-link-spec")?
        .map(|spec| spec.kind);
    let checks = table
        .remove("x-link-checks")
        .map(toml::Value::try_into::<Vec<String>>)
        .transpose()
        .context("x-link-checks lists hooks by name")?
        .unwrap_or_default();
    let resource = table
        .remove("x-link-resource")
        .map(toml::Value::try_into::<Resource>)
        .transpose()
        .context("x-link-resource")?
        .map(|resource| resource.id);
    if let Some(kind) = &spec {
        ensure!(
            crate::names::is_name(kind),
            "spec kind {kind:?} is a lowercase name"
        );
    }
    let schema = super::schema::schema(&toml::Value::Table(table), module, 0)?;
    Ok(Declared {
        schema,
        spec,
        resource,
        checks,
    })
}
