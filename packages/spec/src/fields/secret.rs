//! Whether a place in a spec's shape holds a secret, which a field flag
//! never takes.

use serde_json::Value;

use crate::constants::MAX_DEPTH;

/// Whether `node`, or any schema below it, its `$ref`s followed, is a
/// secret's (`x-link-sensitive`).
pub(super) fn holds_sensitive(root: &Value, node: &Value, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    if node.get("x-link-sensitive") == Some(&Value::Bool(true)) {
        return true;
    }
    let below = |value: &Value| holds_sensitive(root, value, depth + 1);
    let referenced = node
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/"))
        .and_then(|path| path.split('/').try_fold(root, |at, key| at.get(key)));
    referenced.is_some_and(below)
        || node
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.values().any(below))
        || node.get("items").is_some_and(below)
        || node
            .get("additionalProperties")
            .is_some_and(|values| values.is_object() && below(values))
        || ["oneOf", "anyOf", "allOf"].iter().any(|key| {
            node.get(*key)
                .and_then(Value::as_array)
                .is_some_and(|choices| choices.iter().any(below))
        })
}
