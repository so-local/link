//! Whether a format's steps answer every field they take away: each code
//! step's flags a field of the schema it steps from, and every field of
//! the oldest schema reaching a field of the current one, or answered.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{Fate, Format};

/// How many times one shape may be entered on one path: itself, and
/// itself within itself.
const REENTRY: usize = 2;

/// Every field path a JSON Schema document declares, dotted as a flag
/// names it: each table and each value in it, through `$ref`s and each
/// variant's properties, never into a list's items or a map's values.
pub fn field_paths(document: &Value) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    walk(document, document, "", &mut paths, &mut Vec::new());
    paths
}

/// `node`'s paths under `at`; `within` the `$ref`s it is inside. A shape
/// that holds itself, as a section holds its OS's own keys, is walked
/// within itself once (`files.linux.write`), never deeper.
fn walk<'a>(
    document: &'a Value,
    node: &'a Value,
    at: &str,
    paths: &mut BTreeSet<String>,
    within: &mut Vec<&'a str>,
) {
    let Some(object) = node.as_object() else {
        return;
    };
    let entered =
        |reference: &str, within: &[&str]| within.iter().filter(|r| **r == reference).count();
    if let Some(reference) = object.get("$ref").and_then(Value::as_str)
        && entered(reference, within) < REENTRY
        && let Some(target) = resolve(document, reference)
    {
        within.push(reference);
        walk(document, target, at, paths, within);
        within.pop();
    }
    for variants in ["allOf", "anyOf", "oneOf"] {
        let variants = object.get(variants).and_then(Value::as_array);
        for variant in variants.into_iter().flatten() {
            walk(document, variant, at, paths, within);
        }
    }
    let properties = object.get("properties").and_then(Value::as_object);
    for (name, property) in properties.into_iter().flatten() {
        if let Some(reference) = property.get("$ref").and_then(Value::as_str)
            && entered(reference, within) >= REENTRY
        {
            continue;
        }
        let path = match at {
            "" => name.clone(),
            at => format!("{at}.{name}"),
        };
        paths.insert(path.clone());
        walk(document, property, &path, paths, within);
    }
}

/// `reference`, a pointer into `document` (`#/$defs/<name>`).
fn resolve<'a>(document: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    let pointer = pointer.replace("~1", "/").replace("~0", "~");
    document.pointer(&pointer).or_else(|| {
        // A name holding a `/`, as a qualified shape's does, kept whole.
        let (defs, name) = pointer.strip_prefix('/')?.split_once('/')?;
        document
            .get(defs)?
            .as_object()
            .and_then(|defs: &Map<String, Value>| defs.get(name))
    })
}

impl Format {
    /// Each field of schema `oldest` (`from`, its paths) the steps take
    /// away unanswered, and each code step's flag naming no field of the
    /// schema it steps from, by its path: none when the steps answer
    /// every field they take. `current` is the current schema's paths.
    pub fn unanswered(&self, from: &BTreeSet<String>, current: &BTreeSet<String>) -> Vec<String> {
        let mut problems = Vec::new();
        for step in self.steps.iter().filter(|step| step.from == self.oldest) {
            for flag in step.flags.keys().filter(|flag| !from.contains(*flag)) {
                problems.push(format!(
                    "{flag}, a flag of step {}, is no field of schema {}",
                    step.from, step.from
                ));
            }
        }
        for path in from {
            if let Fate::Renamed(now) = self.fate(self.oldest, path)
                && !current.contains(&now)
            {
                problems.push(format!(
                    "{path} of schema {} is no field of schema {} ({now}), and no step answers it",
                    self.oldest, self.current
                ));
            }
        }
        problems
    }
}
