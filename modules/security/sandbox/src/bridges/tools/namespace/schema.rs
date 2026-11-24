//! The shapes a namespace offers and checks: one object schema for an
//! action several providers serve, a call's arguments against its
//! provider's schema, and the targets a call selects.

use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox_client::namespace::{Action, Target};
use serde_json::{Value, json};

/// One object schema for an action several providers serve: every
/// provider's properties, `kind` naming each, and required only what
/// every one requires. Models' APIs refuse a top-level `anyOf`; each call
/// is still checked against its own provider's schema.
pub(super) fn one_shape(variants: &[Value]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut kinds = Vec::new();
    for variant in variants {
        for (key, property) in variant["properties"].as_object().into_iter().flatten() {
            if key == "kind" {
                kinds.extend(property["enum"].as_array().into_iter().flatten().cloned());
            } else {
                properties
                    .entry(key.clone())
                    .or_insert_with(|| property.clone());
            }
        }
    }
    properties.insert("kind".into(), json!({"type": "string", "enum": kinds}));
    let required_by = |variant: &Value| -> Vec<Value> {
        variant["required"].as_array().cloned().unwrap_or_default()
    };
    let required: Vec<Value> = variants
        .split_first()
        .map(|(first, rest)| {
            required_by(first)
                .into_iter()
                .filter(|key| {
                    rest.iter()
                        .all(|variant| required_by(variant).contains(key))
                })
                .collect()
        })
        .unwrap_or_default();
    json!({"type": "object", "properties": properties, "required": required})
}

/// Namespace arguments follow the provider's discovered schema and reject
/// unlisted fields. A refusal names the argument, by its path in the
/// call, and the value it refused.
pub(super) fn check_arguments(input: &Value, schema: &Value) -> anyhow::Result<()> {
    check_at(input, schema, "")
}

/// A value as a refusal names it: text as itself, anything else as JSON.
fn shown(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn check_at(input: &Value, schema: &Value, at: &str) -> anyhow::Result<()> {
    let named = if at.is_empty() {
        "the arguments".to_owned()
    } else {
        format!("argument {at}")
    };
    if let Some(variants) = schema
        .get("anyOf")
        .or_else(|| schema.get("oneOf"))
        .and_then(Value::as_array)
    {
        ensure!(
            variants
                .iter()
                .any(|variant| check_at(input, variant, at).is_ok()),
            "{named} match no offered shape"
        );
        return Ok(());
    }
    let matches = |kind: &str| match kind {
        "object" => input.is_object(),
        "array" => input.is_array(),
        "string" => input.is_string(),
        "integer" => input.is_i64() || input.is_u64(),
        "number" => input.is_number(),
        "boolean" => input.is_boolean(),
        "null" => input.is_null(),
        _ => false,
    };
    if let Some(kind) = schema.get("type") {
        ensure!(
            kind.as_str().is_some_and(matches)
                || kind.as_array().is_some_and(|kinds| kinds
                    .iter()
                    .any(|kind| kind.as_str().is_some_and(matches))),
            "{named} has the wrong type: {} isn't {}",
            shown(input),
            shown(kind)
        );
    }
    if let Some(options) = schema["enum"].as_array() {
        ensure!(
            options.contains(input),
            "{named} can't be {}: it's one of {}",
            shown(input),
            options.iter().map(shown).collect::<Vec<_>>().join(", ")
        );
    }
    let below = |name: &str| {
        if at.is_empty() {
            name.to_owned()
        } else {
            format!("{at}.{name}")
        }
    };
    if let Some(object) = input.as_object() {
        for required in schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            ensure!(
                object.contains_key(required),
                "missing argument {}",
                below(required)
            );
        }
        for (name, value) in object {
            if let Some(property) = schema["properties"].get(name) {
                check_at(value, property, &below(name))?;
            } else {
                match schema.get("additionalProperties") {
                    Some(Value::Bool(true)) => {}
                    Some(property) if property.is_object() => {
                        check_at(value, property, &below(name))?;
                    }
                    _ => anyhow::bail!("unknown argument {}", below(name)),
                }
            }
        }
    }
    if let Some(items) = input.as_array()
        && let Some(schema) = schema.get("items")
    {
        for (i, item) in items.iter().enumerate() {
            check_at(item, schema, &format!("{at}[{i}]"))?;
        }
    }
    Ok(())
}

pub(super) fn selected<'a>(
    action: &'a Action,
    kind: Option<&str>,
) -> anyhow::Result<Vec<(&'a String, &'a Target)>> {
    match kind {
        Some(kind) => Ok(vec![
            action
                .targets
                .get_key_value(kind)
                .within("unknown task kind")?,
        ]),
        None if action.targets.len() == 1 || action.broadcast => {
            Ok(action.targets.iter().collect())
        }
        None => anyhow::bail!("kind is required for this action"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value outside an argument's offered ones is refused naming the
    /// argument, the value and what's offered, as a model needs to
    /// correct its call.
    #[test]
    fn a_refusal_names_the_argument_and_its_value() {
        let schema = json!({"type": "object", "properties": {
            "answer": {"type": "string", "enum": ["owner", "chat"]},
            "every": {"type": "object", "properties": {"minutes": {"type": "integer"}}},
            "tags": {"type": "array", "items": {"type": "string"}},
        }});
        let refused = |input: Value| check_arguments(&input, &schema).unwrap_err().to_string();
        assert_eq!(
            refused(json!({"answer": "whatsapp"})),
            "argument answer can't be whatsapp: it's one of owner, chat"
        );
        assert_eq!(
            refused(json!({"every": {"minutes": "5"}})),
            "argument every.minutes has the wrong type: 5 isn't integer"
        );
        assert_eq!(
            refused(json!({"tags": ["a", 2]})),
            "argument tags[1] has the wrong type: 2 isn't string"
        );
        assert_eq!(refused(json!({"to": "x"})), "unknown argument to");
        check_arguments(&json!({"answer": "chat", "tags": ["a"]}), &schema).unwrap();
    }
}
