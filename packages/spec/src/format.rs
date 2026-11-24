//! The checks of a spec kind's `schema.toml`: its kind's name, its steps'
//! and its `[before]`'s releases, and `[before]`'s keys against the shape.

use serde_json::Value;

use crate::schema;

/// Every problem with a spec kind's declaration: its kind is one
/// lowercase word, and neither a step nor its `[before]` has reached the
/// release that deletes it.
pub fn check_format(format: &schema::Format) -> Vec<String> {
    check_format_at(format, &link_version::version())
}

fn check_format_at(format: &schema::Format, release: &link_version::Version) -> Vec<String> {
    let mut problems = Vec::new();
    if format.kind.is_empty() || !format.kind.bytes().all(|b| b.is_ascii_lowercase()) {
        problems.push(format!(
            "the kind {:?} isn't one lowercase word",
            format.kind
        ));
    }
    for step in &format.steps {
        if &step.since > release {
            problems.push(format!(
                "schema step {} comes at {}, after this release {release}",
                step.from, step.since
            ));
        }
        // A format of history keeps its steps, which name no until.
        let Some(until) = step.until.as_ref() else {
            continue;
        };
        if until <= release {
            problems.push(format!(
                "schema step {} is deleted at {until}, which this release {release} has reached: \
                 delete the step{} and read from the schema after it (oldest)",
                step.from,
                match &step.migration {
                    Some(named) => format!(" and its code, {named},"),
                    None => String::new(),
                }
            ));
        }
    }
    for transfer in &format.transfers {
        if &transfer.since > release {
            problems.push(format!(
                "the transfer from {} comes at {}, after this release {release}",
                transfer.from, transfer.since
            ));
        }
        if &transfer.until <= release {
            problems.push(format!(
                "the transfer from {} is deleted at {}, which this release {release} has \
                 reached: delete it{}",
                transfer.from,
                transfer.until,
                match &transfer.migration {
                    Some(named) => format!(" and its code, {named}"),
                    None => String::new(),
                }
            ));
        }
    }
    let before = &format.before;
    if let Some(since) = before.since.as_ref().filter(|s| *s > release) {
        problems.push(format!(
            "[before] comes at {since}, after this release {release}"
        ));
    }
    if let Some(until) = before.until.as_ref().filter(|u| *u <= release) {
        problems.push(format!(
            "[before] is deleted at {until}, which this release {release} has reached: \
             delete it, and such a file is refused as unknown"
        ));
    }
    problems
}

/// Every `[before.keys]` path the current shape, the kind's JSON Schema
/// `shape`, still declares: such a path would refuse every valid file.
pub fn check_before(format: &schema::Format, shape: &Value) -> Vec<String> {
    format
        .before
        .keys
        .keys()
        .filter(|path| {
            let parts = schema::path::keys(path).unwrap_or_default();
            declares(shape, shape, &parts)
        })
        .map(|path| format!("[before.keys] {path} is a key the current shape declares"))
        .collect()
}

/// Whether `schema`, inside `root`, declares the path `parts`, a `*`
/// matching any declared key or list entry.
fn declares(root: &Value, schema: &Value, parts: &[&str]) -> bool {
    let Some((head, rest)) = parts.split_first() else {
        return true;
    };
    let schema = resolved(root, schema);
    let variants = ["anyOf", "oneOf", "allOf"]
        .iter()
        .filter_map(|k| schema.get(*k).and_then(Value::as_array))
        .flatten();
    if variants
        .clone()
        .any(|variant| declares(root, variant, parts))
    {
        return true;
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    if *head == "*" {
        let inner = [schema.get("items"), schema.get("additionalProperties")]
            .into_iter()
            .flatten()
            .filter(|v| v.is_object())
            .any(|inner| declares(root, inner, rest));
        return inner
            || properties.is_some_and(|p| p.values().any(|inner| declares(root, inner, rest)));
    }
    if let Some(inner) = properties.and_then(|p| p.get(*head)) {
        return declares(root, inner, rest);
    }
    schema
        .get("additionalProperties")
        .filter(|v| v.is_object())
        .is_some_and(|inner| declares(root, inner, rest))
}

/// `schema` with a local `$ref` followed, once per hop.
fn resolved<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    let mut schema = schema;
    for _ in 0..16 {
        let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
            break;
        };
        let Some(pointer) = reference.strip_prefix('#') else {
            break;
        };
        match root.pointer(pointer) {
            Some(target) => schema = target,
            None => break,
        }
    }
    schema
}

#[cfg(test)]
mod tests;
