//! What several generators share about shapes: references moved to where
//! a document keeps its shapes, and names as TypeScript writes them.

use serde_json::Value;

/// Where `link-actions` renders a name's reference.
const DEFS: &str = "#/$defs/";

/// `value` with each reference to `#/$defs/<name>` pointing to
/// `<to><name>` instead, for a document that keeps its shapes elsewhere.
pub(crate) fn rebase(mut value: Value, to: &str) -> Value {
    fn walk(value: &mut Value, to: &str) {
        match value {
            Value::Object(object) => {
                if let Some(Value::String(target)) = object.get_mut("$ref")
                    && let Some(name) = target.strip_prefix(DEFS)
                {
                    *target = format!("{to}{name}");
                }
                object.values_mut().for_each(|v| walk(v, to));
            }
            Value::Array(items) => items.iter_mut().for_each(|v| walk(v, to)),
            _ => {}
        }
    }
    walk(&mut value, to);
    value
}

/// `value` as a file's text: every object's keys sorted, so the file reads
/// the same whether or not a workspace keeps JSON's insertion order.
pub(crate) fn json_text(mut value: Value) -> anyhow::Result<String> {
    value.sort_all_objects();
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

/// A lowercase dashed name in PascalCase: `cloud-account` is `CloudAccount`.
pub(crate) fn pascal(name: &str) -> String {
    name.split(['-', '.'])
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn references_move_and_names_read_as_pascal_case() {
        assert_eq!(
            super::rebase(
                json!({"items": {"$ref": "#/$defs/env.stage"}, "x": [{"$ref": "#/$defs/box.id"}]}),
                "#/components/schemas/"
            ),
            json!({"items": {"$ref": "#/components/schemas/env.stage"}, "x": [{"$ref": "#/components/schemas/box.id"}]})
        );
        assert_eq!(super::pascal("env.cloud-account"), "EnvCloudAccount");
    }
}
