//! A schema as a declaration writes it, read strictly into the catalog's
//! shape: the subset that maps to one type, every other keyword refused
//! by name, and a name standing where a schema goes.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail, ensure};
use serde_json::Value;
use toml::Value as Toml;

use crate::{
    names::TypeName,
    schema::{
        Bounds, Display, Format, List, Meta, Node, OneOf, PathKind, Record, Schema, Text, Told,
    },
};

/// Keywords of JSON Schema the subset never reads, each refused by name.
const REFUSED: &[&str] = &[
    "$ref",
    "$defs",
    "allOf",
    "anyOf",
    "not",
    "if",
    "then",
    "else",
    "patternProperties",
    "dependentSchemas",
    "unevaluatedProperties",
];

/// Reads `value`, written inside `module`, `depth` levels down.
pub(crate) fn schema(value: &Toml, module: &str, depth: usize) -> anyhow::Result<Schema> {
    ensure!(
        depth <= crate::config::SETTINGS.files.max_depth,
        "a shape nests deeper than the format's limit"
    );
    let table = match value {
        Toml::String(name) => {
            return Ok(Schema::new(Node::Named(TypeName::written(name, module)?)));
        }
        Toml::Table(table) => table,
        other => bail!(
            "a schema is a table or a shape's name, not {}",
            other.type_str()
        ),
    };
    let mut keys = Keys::new(table);
    for keyword in REFUSED {
        ensure!(
            !table.contains_key(*keyword),
            "{keyword} isn't read: name a shape instead, or use the subset's keywords"
        );
    }
    let meta = Meta {
        description: keys.string("description")?,
        default: keys.take("default").map(json),
        examples: match keys.take("examples") {
            Some(Toml::Array(values)) => values.iter().map(json).collect(),
            Some(_) => bail!("examples is a list"),
            None => Vec::new(),
        },
        sensitive: keys.flag("x-link-sensitive")?,
        display: keys
            .take("x-link-display")
            .map(|v| v.clone().try_into::<Display>())
            .transpose()
            .context("x-link-display")?,
        unavailable: keys.string("x-link-unavailable")?,
        summary: keys
            .integer("x-link-summary")?
            .map(|n| u32::try_from(n).ok().filter(|n| *n > 0))
            .map(|n| n.context("x-link-summary is a place, from 1"))
            .transpose()?,
        fill: keys.string("x-link-fill")?,
        asks: keys.flag("x-link-asks")?,
        values: keys
            .take("x-link-values")
            .map(|v| v.clone().try_into::<BTreeMap<String, String>>())
            .transpose()
            .context("x-link-values says each word's meaning as text")?
            .unwrap_or_default(),
        rust: keys.string("x-link-rust")?,
    };
    if let Some(why) = &meta.unavailable {
        ensure!(
            !why.trim().is_empty(),
            "x-link-unavailable says why a run refuses it"
        );
    }
    // A shape named with its own description, examples or default, where
    // it stands: `{ shape = "core.duration", default = "1m" }`.
    if let Some(name) = keys.take("shape") {
        let Toml::String(name) = name else {
            bail!("shape names a shape")
        };
        ensure!(
            meta.values.is_empty(),
            "x-link-values is an enum's: what each of its words means"
        );
        let node = Node::Named(TypeName::written(name, module)?);
        keys.done()
            .context("a named shape carries only its description, examples and default")?;
        return Ok(Schema { node, meta });
    }
    let any = keys.flag("x-link-any")?;
    if any || meta.rust.is_some() {
        ensure!(
            !(any && meta.rust.is_some()),
            "x-link-rust names its type alone, without x-link-any"
        );
        ensure!(
            meta.rust
                .as_deref()
                .is_none_or(|rust| rust.starts_with("::")),
            "x-link-rust names its type by its whole path, ::<crate>::<type>"
        );
        ensure!(
            meta.values.is_empty(),
            "x-link-values is an enum's: what each of its words means"
        );
        keys.done()
            .context("any value carries only its description and examples")?;
        return Ok(Schema {
            node: Node::Any,
            meta,
        });
    }
    let kind = keys.string("type")?;
    let node = if let Some(values) = keys.take("enum") {
        let Toml::Array(values) = values else {
            bail!("enum is a list")
        };
        ensure!(!values.is_empty(), "enum lists at least one value");
        ensure!(
            values.iter().all(|v| v.is_str()),
            "enum lists strings: a closed choice of words"
        );
        ensure!(
            matches!(kind.as_deref(), None | Some("string")),
            "an enum is of strings"
        );
        let distinct: BTreeSet<_> = values.iter().filter_map(Toml::as_str).collect();
        ensure!(distinct.len() == values.len(), "enum repeats a value");
        ensure!(
            meta.values.is_empty()
                || meta
                    .values
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
                    == distinct,
            "x-link-values says the meaning of each of the enum's words, and only them"
        );
        Node::Enum(values.iter().map(json).collect())
    } else if let Some(value) = keys.take("const") {
        Node::Const(json(value))
    } else if let Some(choices) = keys.take("oneOf") {
        let markers = keys.take("x-link-markers");
        one_of(choices, markers, module, depth)?
    } else {
        match kind
            .as_deref()
            .context("a schema says its type, names a shape, or is an enum, const or oneOf")?
        {
            "object" => object(&mut keys, module, depth)?,
            "array" => Node::List(List {
                items: Box::new(schema(
                    keys.take("items").context("an array says its items")?,
                    module,
                    depth + 1,
                )?),
                min_items: keys.count("minItems")?,
                max_items: keys.count("maxItems")?,
            }),
            "string" => Node::String(Text {
                format: keys
                    .take("format")
                    .map(|v| v.clone().try_into::<Format>())
                    .transpose()
                    .context("format names a standard format or link-id")?,
                pattern: keys.string("pattern")?,
                min_length: keys.count("minLength")?,
                max_length: keys.count("maxLength")?,
                kind: keys.string("x-link-kind")?,
                reference: keys
                    .string("x-link-ref")?
                    .map(|word| word.parse())
                    .transpose()?,
                path: match keys.take("x-link-path") {
                    None | Some(Toml::Boolean(false)) => None,
                    Some(Toml::Boolean(true)) => Some(PathKind::Plain),
                    Some(Toml::String(word)) if word == "confined" => Some(PathKind::Confined),
                    Some(_) => bail!(
                        "x-link-path is true, or \"confined\" for a path kept inside the spec's own folder"
                    ),
                },
            }),
            "integer" => Node::Integer(Bounds {
                minimum: keys.integer("minimum")?,
                maximum: keys.integer("maximum")?,
            }),
            "number" => Node::Number(Bounds {
                minimum: keys.number("minimum")?,
                maximum: keys.number("maximum")?,
            }),
            "boolean" => Node::Boolean,
            other => bail!(
                "type {other:?} isn't one of the subset's: object, array, string, integer, number or boolean"
            ),
        }
    };
    keys.done()?;
    check(&node)?;
    ensure!(
        meta.values.is_empty() || matches!(node, Node::Enum(_)),
        "x-link-values is an enum's: what each of its words means"
    );
    Ok(Schema { node, meta })
}

fn object(keys: &mut Keys, module: &str, depth: usize) -> anyhow::Result<Node> {
    let properties = keys.take("properties");
    let required = keys.take("required");
    let shorthand = keys.string("x-link-shorthand")?;
    let mut record = Record::default();
    match keys.take("additionalProperties") {
        Some(Toml::Boolean(false)) => {}
        Some(Toml::Boolean(true)) => bail!("additionalProperties is false or a schema, never true"),
        // A map, or a record open to keys it doesn't name, each of this
        // shape.
        Some(values) if properties.is_none() => {
            ensure!(
                required.is_none(),
                "a map has no named properties to require"
            );
            // Its keys, where they're a text shape of their own.
            let names = keys
                .take("propertyNames")
                .map(|names| schema(names, module, depth + 1))
                .transpose()
                .context("propertyNames")?;
            if let Some(names) = &names {
                ensure!(
                    matches!(names.node, Node::String(_) | Node::Named(_)),
                    "propertyNames is a text shape, or names one: a map's keys are text"
                );
            }
            return Ok(Node::Map(crate::schema::Map {
                names: names.map(Box::new),
                values: Box::new(schema(values, module, depth + 1)?),
            }));
        }
        Some(values) => {
            record.others = Some(Box::new(schema(values, module, depth + 1)?));
        }
        None => bail!(
            "an object closes its properties (additionalProperties = false) or is a map of a schema"
        ),
    }
    if let Some(properties) = properties {
        let Toml::Table(properties) = properties else {
            bail!("properties is a table")
        };
        for (name, value) in properties {
            let child =
                schema(value, module, depth + 1).with_context(|| format!("property {name}"))?;
            record.properties.insert(name.clone(), child);
        }
    }
    if let Some(required) = required {
        let Toml::Array(required) = required else {
            bail!("required is a list")
        };
        for name in required {
            let name = name.as_str().context("required lists property names")?;
            ensure!(
                record.properties.contains_key(name),
                "required names {name:?}, which isn't a property"
            );
            ensure!(
                record.required.insert(name.to_string()),
                "required repeats {name:?}"
            );
        }
    }
    if let Some(name) = &shorthand {
        ensure!(
            record.properties.contains_key(name),
            "x-link-shorthand names {name:?}, which isn't a property"
        );
    }
    record.shorthand = shorthand;
    Ok(Node::Record(record))
}

fn one_of(
    choices: &Toml,
    markers: Option<&Toml>,
    module: &str,
    depth: usize,
) -> anyhow::Result<Node> {
    let Toml::Array(choices) = choices else {
        bail!("oneOf is a list")
    };
    ensure!(choices.len() >= 2, "oneOf offers at least two shapes");
    let variants = choices
        .iter()
        .enumerate()
        .map(|(i, v)| schema(v, module, depth + 1).with_context(|| format!("oneOf choice {i}")))
        .collect::<anyhow::Result<Vec<_>>>()?;
    if let Some(markers) = markers {
        let markers: Vec<String> = markers
            .clone()
            .try_into()
            .context("x-link-markers lists each choice's marking key")?;
        ensure!(
            markers.len() == variants.len(),
            "x-link-markers names one marking key for each oneOf choice, in order"
        );
        return Ok(Node::OneOf(OneOf {
            told: Told::Markers(markers),
            variants,
        }));
    }
    // The tag is settled once names resolve, since a choice may be a
    // named record (`Catalog::load`).
    Ok(Node::OneOf(OneOf {
        told: Told::Tag(String::new()),
        variants,
    }))
}

/// Bounds that can't hold anything are refused.
fn check(node: &Node) -> anyhow::Result<()> {
    let ordered = |low: Option<f64>, high: Option<f64>, what: &str| match (low, high) {
        (Some(low), Some(high)) if low > high => bail!("{what}'s least is above its most"),
        _ => Ok(()),
    };
    match node {
        Node::String(text) => {
            ordered(
                text.min_length.map(|n| n as f64),
                text.max_length.map(|n| n as f64),
                "length",
            )?;
            if let Some(pattern) = &text.pattern {
                regex::Regex::new(pattern).with_context(|| format!("pattern {pattern:?}"))?;
            }
            ensure!(
                text.kind.is_none() || text.format == Some(Format::LinkId),
                "x-link-kind names the kind of a link-id"
            );
            if let Some(kind) = &text.kind {
                ensure!(
                    crate::names::is_name(kind),
                    "x-link-kind {kind:?} is a kind's name"
                );
            }
        }
        Node::Integer(b) => ordered(
            b.minimum.map(|n| n as f64),
            b.maximum.map(|n| n as f64),
            "integer",
        )?,
        Node::Number(b) => ordered(b.minimum, b.maximum, "number")?,
        Node::List(l) => ordered(
            l.min_items.map(|n| n as f64),
            l.max_items.map(|n| n as f64),
            "list",
        )?,
        _ => {}
    }
    Ok(())
}

/// A TOML value as JSON, which every generator reads.
pub(crate) fn json(value: &Toml) -> Value {
    match value {
        Toml::String(s) => Value::String(s.clone()),
        Toml::Integer(i) => Value::from(*i),
        Toml::Float(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
        Toml::Boolean(b) => Value::Bool(*b),
        Toml::Datetime(d) => Value::String(d.to_string()),
        Toml::Array(a) => Value::Array(a.iter().map(json).collect()),
        Toml::Table(t) => Value::Object(t.iter().map(|(k, v)| (k.clone(), json(v))).collect()),
    }
}

/// A table's keys, each taken once, the rest refused when done.
pub(crate) struct Keys<'a> {
    table: &'a toml::Table,
    taken: BTreeMap<&'a str, ()>,
}

impl<'a> Keys<'a> {
    pub(crate) fn new(table: &'a toml::Table) -> Self {
        Self {
            table,
            taken: BTreeMap::new(),
        }
    }

    pub(crate) fn take(&mut self, key: &str) -> Option<&'a Toml> {
        let (key, value) = self.table.get_key_value(key)?;
        self.taken.insert(key.as_str(), ());
        Some(value)
    }

    pub(crate) fn string(&mut self, key: &str) -> anyhow::Result<Option<String>> {
        self.take(key)
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .with_context(|| format!("{key} is text"))
            })
            .transpose()
    }

    pub(crate) fn flag(&mut self, key: &str) -> anyhow::Result<bool> {
        Ok(self
            .take(key)
            .map(|v| {
                v.as_bool()
                    .with_context(|| format!("{key} is true or false"))
            })
            .transpose()?
            .unwrap_or(false))
    }

    fn integer(&mut self, key: &str) -> anyhow::Result<Option<i64>> {
        self.take(key)
            .map(|v| {
                v.as_integer()
                    .with_context(|| format!("{key} is a whole number"))
            })
            .transpose()
    }

    fn number(&mut self, key: &str) -> anyhow::Result<Option<f64>> {
        self.take(key)
            .map(|v| match v {
                Toml::Integer(i) => Ok(*i as f64),
                Toml::Float(f) => Ok(*f),
                _ => bail!("{key} is a number"),
            })
            .transpose()
    }

    fn count(&mut self, key: &str) -> anyhow::Result<Option<u64>> {
        self.integer(key)?
            .map(|n| u64::try_from(n).with_context(|| format!("{key} is zero or more")))
            .transpose()
    }

    /// Refuses whatever key wasn't taken, by name.
    pub(crate) fn done(self) -> anyhow::Result<()> {
        match self
            .table
            .keys()
            .find(|k| !self.taken.contains_key(k.as_str()))
        {
            Some(key) => bail!("{key:?} isn't a keyword this format reads here"),
            None => Ok(()),
        }
    }
}
