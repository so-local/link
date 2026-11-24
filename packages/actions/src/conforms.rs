//! Whether a value written in a declaration, a default, is one its own
//! shape takes, so a declared default never fails a run that leaves its
//! field out.

use anyhow::{Context, bail, ensure};
use serde_json::Value;

use crate::{
    Catalog,
    schema::{Node, Schema},
};

impl Catalog {
    /// `value`, read from elsewhere (a file another release wrote, or one
    /// stepped to the current schema), is one the shape `name` takes, as
    /// its declaration says.
    pub fn takes(&self, name: &crate::TypeName, value: &Value) -> anyhow::Result<()> {
        let declared = self
            .shape(name)
            .with_context(|| format!("{name} isn't declared"))?;
        self.conforms(&declared.schema, value)
    }

    /// `value` is one `schema` takes, by its type, choices, bounds,
    /// pattern, length, required fields and closed records.
    pub(crate) fn conforms(&self, schema: &Schema, value: &Value) -> anyhow::Result<()> {
        let schema = self.resolve(schema)?;
        match (&schema.node, value) {
            (Node::Record(record), Value::Object(object)) => {
                for name in &record.required {
                    ensure!(
                        object.contains_key(name),
                        "it has no {name}, which is required"
                    );
                }
                for (name, value) in object {
                    let field = record
                        .properties
                        .get(name)
                        .or(record.others.as_deref())
                        .with_context(|| format!("{name} isn't a field"))?;
                    self.conforms(field, value)
                        .with_context(|| format!("its {name}"))?;
                }
            }
            (Node::Map(map), Value::Object(object)) => {
                for (key, value) in object {
                    if let Some(keys) = &map.names {
                        self.conforms(keys, &Value::String(key.clone()))
                            .with_context(|| format!("its key {key:?}"))?;
                    }
                    self.conforms(&map.values, value)
                        .with_context(|| format!("its {key}"))?;
                }
            }
            (Node::List(list), Value::Array(items)) => {
                let n = items.len() as u64;
                ensure!(
                    list.min_items.is_none_or(|min| n >= min),
                    "it lists too few"
                );
                ensure!(
                    list.max_items.is_none_or(|max| n <= max),
                    "it lists too many"
                );
                for item in items {
                    self.conforms(&list.items, item)?;
                }
            }
            (Node::Enum(values), value) => {
                ensure!(values.contains(value), "{value} isn't one of its choices");
            }
            (Node::Const(expected), value) => ensure!(expected == value, "it isn't {expected}"),
            (Node::OneOf(one), value) => match &one.told {
                crate::schema::Told::Markers(markers) => {
                    let chosen = markers
                        .iter()
                        .position(|marker| value.get(marker).is_some())
                        .with_context(|| format!("it holds none of {}", markers.join(", ")))?;
                    self.conforms(&one.variants[chosen], value)?;
                }
                crate::schema::Told::Tag(_) | crate::schema::Told::Value => ensure!(
                    one.variants.iter().any(|v| self.conforms(v, value).is_ok()),
                    "it's none of its choices"
                ),
            },
            (Node::String(text), Value::String(s)) => {
                let n = s.chars().count() as u64;
                ensure!(text.min_length.is_none_or(|min| n >= min), "it's too short");
                ensure!(text.max_length.is_none_or(|max| n <= max), "it's too long");
                if let Some(pattern) = &text.pattern {
                    ensure!(
                        regex::Regex::new(pattern)?.is_match(s),
                        "it doesn't match {pattern}"
                    );
                }
                match text.format {
                    Some(crate::schema::Format::Duration) => {
                        s.parse::<crate::value::Duration>()
                            .map_err(anyhow::Error::msg)?;
                    }
                    Some(crate::schema::Format::Size) => {
                        s.parse::<crate::value::Size>()
                            .map_err(anyhow::Error::msg)?;
                    }
                    Some(crate::schema::Format::Rate) => {
                        s.parse::<crate::value::Rate>()
                            .map_err(anyhow::Error::msg)?;
                    }
                    Some(crate::schema::Format::SealTo) => {
                        s.parse::<link_secretive::SealTo>()?;
                    }
                    _ => {}
                }
            }
            (Node::Integer(bounds), Value::Number(n)) if n.is_i64() => {
                let n = n.as_i64().unwrap_or_default();
                ensure!(
                    bounds.minimum.is_none_or(|min| n >= min),
                    "{n} is below its least"
                );
                ensure!(
                    bounds.maximum.is_none_or(|max| n <= max),
                    "{n} is above its most"
                );
            }
            (Node::Number(bounds), Value::Number(n)) => {
                let n = n.as_f64().unwrap_or_default();
                ensure!(
                    bounds.minimum.is_none_or(|min| n >= min),
                    "{n} is below its least"
                );
                ensure!(
                    bounds.maximum.is_none_or(|max| n <= max),
                    "{n} is above its most"
                );
            }
            (Node::Boolean, Value::Bool(_)) => {}
            (Node::Any, _) => {}
            (_, value) => bail!("{value} isn't of its type"),
        }
        Ok(())
    }
}
