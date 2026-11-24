//! The website's runtime JSON schemas, read from their one TypeScript
//! source in the workspace that holds the website being checked.

use std::path::PathBuf;

use serde_json::Value;

/// Where a website that reads these lists keeps its schemas, in its app.
const SOURCE: &str = "src/catalog-schemas.ts";
const PREFIX: &str = "export const catalogSchemas = ";
const SUFFIX: &str = " as const;";

fn extract(source: &str) -> Result<Value, String> {
    let literal = source
        .split_once(PREFIX)
        .and_then(|(_, after)| after.split_once(SUFFIX))
        .map(|(literal, _)| literal)
        .ok_or("website catalog schema declaration is missing")?;
    let table: Value = serde_json::from_str(literal).map_err(|err| err.to_string())?;
    let schemas = table.as_object().ok_or("catalog schemas must be a table")?;
    for (name, schema) in schemas {
        shape(schema).map_err(|why| format!("catalog schema {name}: {why}"))?;
    }
    Ok(table)
}

/// Refuse unsupported schema features, so a frontend contract change can
/// never silently weaken the documentation check.
fn shape(schema: &Value) -> Result<(), String> {
    let object = schema.as_object().ok_or("a schema must be an object")?;
    for (key, value) in object {
        let valid = match key.as_str() {
            "type" => matches!(value.as_str(), Some("object" | "array" | "string")),
            "additionalProperties" => value.is_boolean(),
            "required" => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
            "minLength" | "maxLength" => value.as_u64().is_some(),
            "pattern" => value
                .as_str()
                .is_some_and(|pattern| regex::Regex::new(pattern).is_ok()),
            "properties" => {
                let properties = value.as_object().ok_or("properties must be an object")?;
                for child in properties.values() {
                    shape(child)?;
                }
                true
            }
            "items" => {
                shape(value)?;
                true
            }
            _ => return Err(format!("unsupported schema keyword {key:?}")),
        };
        if !valid {
            return Err(format!("invalid schema keyword {key:?}"));
        }
    }
    if !object.contains_key("type") {
        return Err("a schema needs its type".into());
    }
    Ok(())
}

pub(super) fn check(apps: &[PathBuf], name: &str, text: &str) -> Result<(), String> {
    let source = apps
        .iter()
        .find_map(|app| std::fs::read_to_string(app.join(SOURCE)).ok())
        .ok_or_else(|| format!("website JSON contract {name:?} needs a website app's {SOURCE}"))?;
    check_in(&extract(&source)?, name, text)
}

fn check_in(schemas: &Value, name: &str, text: &str) -> Result<(), String> {
    let schema = schemas
        .get(name)
        .ok_or_else(|| format!("unregistered website JSON contract {name:?}"))?;
    let value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    walk(schema, &value, "$")
}

fn walk(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    match schema["type"].as_str() {
        Some("object") => {
            let object = value
                .as_object()
                .ok_or_else(|| format!("{path} must be an object"))?;
            if let Some(required) = schema["required"].as_array() {
                for key in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(key) {
                        return Err(format!("{path} is missing required field {key:?}"));
                    }
                }
            }
            for (key, value) in object {
                if let Some(child) = schema["properties"].get(key) {
                    walk(child, value, &format!("{path}.{key}"))?;
                } else if schema["additionalProperties"] == false {
                    return Err(format!("{path} has unknown field {key:?}"));
                }
            }
        }
        Some("array") => {
            let array = value
                .as_array()
                .ok_or_else(|| format!("{path} must be an array"))?;
            if let Some(items) = schema.get("items") {
                for (index, value) in array.iter().enumerate() {
                    walk(items, value, &format!("{path}[{index}]"))?;
                }
            }
        }
        Some("string") => {
            let text = value
                .as_str()
                .ok_or_else(|| format!("{path} must be a string"))?;
            let size = text.chars().count() as u64;
            if schema["minLength"].as_u64().is_some_and(|min| size < min)
                || schema["maxLength"].as_u64().is_some_and(|max| size > max)
            {
                return Err(format!("{path} does not meet its string length limits"));
            }
            if let Some(pattern) = schema["pattern"].as_str()
                && !regex::Regex::new(pattern)
                    .map_err(|err| err.to_string())?
                    .is_match(text)
            {
                return Err(format!("{path} does not match its pattern"));
            }
        }
        _ => return Err(format!("{path} has an unsupported schema type")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schema table of the shapes the lists have, as the source declares it.
    fn schemas() -> Value {
        let source = format!(
            "{PREFIX}{}{SUFFIX}",
            serde_json::json!({
                "offers": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["offers"],
                    "properties": {"offers": {"type": "array", "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["name", "what", "price"],
                        "properties": {
                            "name": {"type": "string", "minLength": 1},
                            "what": {"type": "string", "minLength": 1},
                            "price": {"type": "string", "minLength": 1}
                        }
                    }}}
                },
                "sponsors": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {"sponsors": {"type": "array", "items": {
                        "type": "object",
                        "required": ["since"],
                        "properties": {"since": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"}}
                    }}}
                }
            })
        );
        extract(&source).unwrap()
    }

    #[test]
    fn runtime_schemas_reject_changed_types_missing_fields_and_unknown_fields() {
        let table = schemas();
        let valid = r#"{"offers":[{"name":"Setup","what":"On your machine","price":"$500"}]}"#;
        assert!(check_in(&table, "offers", valid).is_ok());
        for invalid in [
            valid.replace("\"name\":\"Setup\",", ""),
            valid.replace("\"price\":\"$500\"", "\"price\":500"),
            valid.replace("\"what\":", "\"unknown\":true,\"what\":"),
            valid.replace("\"Setup\"", "\"\""),
        ] {
            assert!(check_in(&table, "offers", &invalid).is_err(), "{invalid}");
        }
        assert!(check_in(&table, "unknown", valid).is_err());
        let sponsor = r#"{"sponsors":[{"since":"2026-10-12"}]}"#;
        assert!(check_in(&table, "sponsors", sponsor).is_ok());
        assert!(
            check_in(
                &table,
                "sponsors",
                &sponsor.replace("2026-10-12", "tomorrow")
            )
            .is_err()
        );
    }

    #[test]
    fn a_workspace_without_the_source_cannot_use_its_contracts() {
        let nowhere = [PathBuf::from("/nonexistent/link-docs")];
        assert!(
            check(&nowhere, "offers", "{}")
                .unwrap_err()
                .contains(SOURCE)
        );
    }

    #[test]
    fn new_schema_keywords_fail_closed() {
        let Value::Object(mut table) = schemas() else {
            panic!("schema table");
        };
        table.get_mut("offers").unwrap()["required"] = serde_json::json!(["new_required"]);
        assert!(walk(&table["offers"], &serde_json::json!({"offers": []}), "$").is_err());
        table.get_mut("offers").unwrap()["unevaluatedProperties"] = false.into();
        let changed = format!("{PREFIX}{}{SUFFIX}", Value::Object(table));
        assert!(
            extract(&changed)
                .unwrap_err()
                .contains("unsupported schema keyword")
        );
        assert!(extract("no declaration").is_err());
    }
}
