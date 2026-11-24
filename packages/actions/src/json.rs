//! Shapes as standard JSON Schema 2020-12, the version OpenAPI 3.1 uses:
//! each name becomes a reference to `#/$defs/<module>.<name>`, the one
//! translation a generator needs.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{
    Catalog,
    names::TypeName,
    schema::{Node, PathKind, Schema},
};

impl Catalog {
    /// `name`'s shape as a whole JSON Schema document: the shape at the
    /// top, every shape it reaches under `$defs`.
    pub fn json_schema(&self, name: &TypeName) -> anyhow::Result<Value> {
        let top = self
            .shape(name)
            .ok_or_else(|| anyhow::anyhow!("{name} isn't declared"))?;
        let mut document = self.json(&top.schema);
        let defs = self.reached(&top.schema)?;
        if let Value::Object(object) = &mut document {
            object.insert(
                "$schema".into(),
                json!(crate::config::SETTINGS.format.dialect),
            );
            if !defs.is_empty() {
                object.insert("$defs".into(), Value::Object(defs.into_iter().collect()));
            }
        }
        Ok(document)
    }

    /// Every named shape `schema` reaches, as JSON Schema, by its
    /// qualified name.
    pub fn reached(&self, schema: &Schema) -> anyhow::Result<BTreeMap<String, Value>> {
        let mut out = BTreeMap::new();
        let mut todo: Vec<TypeName> = schema.names().into_iter().cloned().collect();
        while let Some(name) = todo.pop() {
            let key = name.to_string();
            if out.contains_key(&key) {
                continue;
            }
            let declared = self
                .shape(&name)
                .ok_or_else(|| anyhow::anyhow!("{name} isn't declared"))?;
            out.insert(key, self.json(&declared.schema));
            todo.extend(declared.schema.names().into_iter().cloned());
        }
        Ok(out)
    }

    /// One schema as JSON Schema, its names as references.
    pub fn json(&self, schema: &Schema) -> Value {
        let mut out = Map::new();
        match &schema.node {
            Node::Named(name) => {
                out.insert("$ref".into(), json!(format!("#/$defs/{name}")));
                // Where a name stands, it says what its shape says of
                // itself, as a property written whole would.
                if schema.meta.description.is_none()
                    && let Some(said) = self
                        .shape(name)
                        .and_then(|d| d.schema.meta.description.clone())
                {
                    out.insert("description".into(), json!(said));
                }
            }
            Node::Record(record) => {
                out.insert("type".into(), json!("object"));
                let properties: Map<String, Value> = record
                    .properties
                    .iter()
                    .map(|(name, schema)| (name.clone(), self.json(schema)))
                    .collect();
                out.insert("properties".into(), Value::Object(properties));
                if !record.required.is_empty() {
                    out.insert("required".into(), json!(record.required));
                }
                let others = match &record.others {
                    Some(others) => self.json(others),
                    None => json!(false),
                };
                out.insert("additionalProperties".into(), others);
                insert(&mut out, "x-link-shorthand", record.shorthand.clone());
            }
            Node::Map(map) => {
                out.insert("type".into(), json!("object"));
                if let Some(keys) = &map.names {
                    out.insert("propertyNames".into(), self.json(keys));
                }
                out.insert("additionalProperties".into(), self.json(&map.values));
            }
            Node::List(list) => {
                out.insert("type".into(), json!("array"));
                out.insert("items".into(), self.json(&list.items));
                insert(&mut out, "minItems", list.min_items);
                insert(&mut out, "maxItems", list.max_items);
            }
            Node::Enum(values) => {
                out.insert("type".into(), json!("string"));
                out.insert("enum".into(), Value::Array(values.clone()));
            }
            Node::Const(value) => {
                out.insert("const".into(), value.clone());
            }
            Node::OneOf(one) => {
                out.insert(
                    "oneOf".into(),
                    Value::Array(one.variants.iter().map(|v| self.json(v)).collect()),
                );
                if let crate::schema::Told::Markers(markers) = &one.told {
                    out.insert("x-link-markers".into(), json!(markers));
                }
            }
            Node::String(text) => {
                out.insert("type".into(), json!("string"));
                // A path's format is link-spec's: `path` resolves it from the
                // file's folder or the current one, and `confined-path` keeps
                // it inside the spec's own folder too.
                let format = match (text.format, text.path) {
                    (Some(format), _) => Some(format.as_str()),
                    (None, Some(PathKind::Plain)) => Some("path"),
                    (None, Some(PathKind::Confined)) => Some("confined-path"),
                    (None, None) => None,
                };
                insert(&mut out, "format", format);
                insert(&mut out, "pattern", text.pattern.clone());
                insert(&mut out, "minLength", text.min_length);
                insert(&mut out, "maxLength", text.max_length);
                insert(&mut out, "x-link-kind", text.kind.clone());
                insert(&mut out, "x-link-ref", text.reference.map(|r| r.word()));
                match text.path {
                    Some(PathKind::Plain) => {
                        out.insert("x-link-path".into(), json!(true));
                    }
                    Some(PathKind::Confined) => {
                        out.insert("x-link-path".into(), json!("confined"));
                    }
                    None => {}
                }
            }
            Node::Integer(bounds) => {
                out.insert("type".into(), json!("integer"));
                insert(&mut out, "minimum", bounds.minimum);
                insert(&mut out, "maximum", bounds.maximum);
            }
            Node::Number(bounds) => {
                out.insert("type".into(), json!("number"));
                insert(&mut out, "minimum", bounds.minimum);
                insert(&mut out, "maximum", bounds.maximum);
            }
            Node::Boolean => {
                out.insert("type".into(), json!("boolean"));
            }
            // Any value: JSON Schema's empty schema, its description alone.
            Node::Any => {}
        }
        let meta = &schema.meta;
        insert(&mut out, "description", meta.description.clone());
        insert(&mut out, "default", meta.default.clone());
        if !meta.examples.is_empty() {
            out.insert("examples".into(), Value::Array(meta.examples.clone()));
        }
        if meta.sensitive {
            out.insert("x-link-sensitive".into(), json!(true));
        }
        insert(&mut out, "x-link-unavailable", meta.unavailable.clone());
        insert(&mut out, "x-link-summary", meta.summary);
        insert(&mut out, "x-link-fill", meta.fill.clone());
        if meta.asks {
            out.insert("x-link-asks".into(), json!(true));
        }
        if !meta.values.is_empty() {
            out.insert("x-link-values".into(), json!(meta.values));
        }
        if let Some(display) = &meta.display {
            out.insert("x-link-display".into(), json!(display));
        }
        Value::Object(out)
    }
}

fn insert<T: serde::Serialize>(out: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(value) = value {
        out.insert(key.into(), json!(value));
    }
}
