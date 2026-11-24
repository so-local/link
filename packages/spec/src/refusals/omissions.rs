//! Concrete optional record properties through their owning file and flag reader.
use super::*;
use active::Step;
use serde_json::Value;

/// The observable serialized result only; absence asserts no hidden default.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum OmissionState {
    /// The actual result retains this value, including explicit JSON null.
    Retained { value: Value },
    /// The prior concrete pointer is absent. No None/default/internal-state
    /// claim is made; actual reader acceptance and serialized replay are checked.
    SerializedAbsent,
}

/// One accepted omission, only at this concrete sampled record property.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Omission {
    pub field: String,
    pub pointer: String,
    /// The exact active TOML value removed from the complete valid input.
    pub removed: String,
    /// The omitted configuration's observable state at the sampled pointer.
    pub result: OmissionState,
    /// Independently declared comparable literal, only when actually checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_default: Option<Value>,
}
/// Exact accepted omissions and exclusions; no all-keys or all-arms claim.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Omissions {
    pub witnessed: Vec<Omission>,
    pub excluded: Vec<TypeExclusion>,
}
/// Problems preserving accepted optional omissions through real field flags.
pub fn check_omissions<T: Kind>() -> Vec<String> {
    omissions::<T>().err().map_or_else(Vec::new, |error| {
        vec![format!("spec optional omissions: {error:#}")]
    })
}
/// Remove only independently optional active properties inside actual records.
/// Conditional rejection and newly empty parents are excluded. An absent
/// serialized result proves only accepted omission and serialized flag replay,
/// never a hidden None/default value. Defaults are claimed only for retained
/// values compared against independent declared literals.
pub fn omissions<T: Kind>() -> anyhow::Result<Omissions> {
    checked_with::<T>(crate::flags_of::<T>)
}
fn checked_with<T: Kind>(emit: fn(&T) -> anyhow::Result<Vec<String>>) -> anyhow::Result<Omissions> {
    let base = baseline::<T>(emit)?;
    checked_base::<T>(&base, emit)
}
pub(super) fn checked_base<T: Kind>(
    base: &Baseline,
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<Omissions> {
    let mut types = TypeRefusals::default();
    let candidates = active::candidates(base, &mut types, active::Scope::Omissions)?;
    let mut report = Omissions {
        excluded: types.excluded,
        ..Default::default()
    };
    let binding = Binding::of::<T>();
    for candidate in candidates {
        let field = &candidate.witness.field;
        let pointer = &candidate.witness.pointer;
        let declaration = match optional(base, &candidate.steps) {
            Ok(declaration) => declaration,
            Err(reason) => {
                exclude(&mut report, &candidate, reason);
                continue;
            }
        };
        let mut input = base.input.clone();
        let removed = match remove(&mut input, &candidate.steps) {
            Ok(value) => value,
            Err(reason) => {
                exclude(&mut report, &candidate, reason);
                continue;
            }
        };
        let text = toml::to_string(&input)?;
        let value = match crate::read_text_at::<T>(&text, &base.dir, &FieldFlags::default()) {
            Ok(value) => value,
            Err(error) => {
                exclude(
                    &mut report,
                    &candidate,
                    format!("owning reader rejects this optional omission: {error:#}"),
                );
                continue;
            }
        };
        let typed = serde_json::to_value(&value)?;
        let observed = typed.pointer(pointer);
        // A serialized-absent state cannot prove any literal default, even
        // when the schema declares one. Preserve that distinction in the API.
        let declared_default = declaration
            .default
            .filter(|_| declaration.comparable && observed.is_some())
            .cloned();
        if let Some(expected) = &declared_default {
            ensure!(
                observed == Some(expected),
                "{field} at {pointer}: omitted value {observed:?} disagrees with independently declared default {expected}"
            );
        }
        let result = match observed {
            Some(value) => OmissionState::Retained {
                value: value.clone(),
            },
            None => OmissionState::SerializedAbsent,
        };
        let (flags, rest) = crate::split(
            emit(&value)?.into_iter().map(Into::into),
            Some(&binding),
            None,
        )?;
        ensure!(
            rest.is_empty(),
            "{field} at {pointer}: omitted configuration emits runtime arguments"
        );
        let replay = crate::read_text_at::<T>("", &base.dir, &flags).with_context(|| {
            format!("{field} at {pointer}: reading omitted configuration flags")
        })?;
        ensure!(
            serde_json::to_value(replay)? == typed,
            "{field} at {pointer}: omitted file and emitted field flags change the typed configuration"
        );
        report.witnessed.push(Omission {
            field: field.clone(),
            pointer: pointer.clone(),
            removed: removed.to_string(),
            result,
            declared_default,
        });
    }
    Ok(report)
}
fn exclude(report: &mut Omissions, candidate: &active::Candidate, reason: impl Into<String>) {
    report.excluded.push(TypeExclusion {
        field: candidate.witness.field.clone(),
        pointer: candidate.witness.pointer.clone(),
        reason: reason.into(),
    });
}

struct Optional<'a> {
    default: Option<&'a Value>,
    comparable: bool,
}
// This is applicability, not a schema validator: only direct object required
// lists, bounded references, exact nullable wrappers and actual unique tags.
fn resolved<'a>(
    root: &'a Value,
    node: &'a Value,
    input: &toml::Value,
    depth: usize,
) -> Option<&'a Value> {
    if depth > crate::constants::MAX_DEPTH || !node.is_object() {
        return None;
    }
    if [
        "allOf",
        "if",
        "then",
        "else",
        "dependentRequired",
        "dependentSchemas",
        "dependencies",
        "not",
        "unevaluatedProperties",
    ]
    .iter()
    .any(|key| node.get(*key).is_some())
    {
        return None;
    }
    if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
        if node
            .as_object()?
            .keys()
            .any(|key| !annotation(key) && key != "$ref")
        {
            return None;
        }
        let target = root.pointer(reference.strip_prefix('#')?)?;
        return resolved(root, target, input, depth + 1);
    }
    for key in ["anyOf", "oneOf"] {
        let Some(arms) = node.get(key).and_then(Value::as_array) else {
            continue;
        };
        if node
            .as_object()?
            .keys()
            .any(|name| !annotation(name) && name != key)
        {
            return None;
        }
        let present: Vec<_> = arms
            .iter()
            .filter(|arm| **arm != serde_json::json!({"type":"null"}))
            .collect();
        if arms.len() == 2 && present.len() == 1 {
            return resolved(root, present[0], input, depth + 1);
        }
        let record = input.as_table()?;
        let mut matching = Vec::new();
        for arm in arms {
            let Some(object) = resolved(root, arm, input, depth + 1) else {
                continue;
            };
            let Some(properties) = object.get("properties").and_then(Value::as_object) else {
                continue;
            };
            let tags: Vec<_> = properties
                .iter()
                .filter_map(|(name, node)| node.get("const").map(|value| (name, value)))
                .collect();
            if !tags.is_empty()
                && tags.iter().all(|(name, value)| {
                    record.get(*name).is_some_and(|actual| {
                        serde_json::to_value(actual).ok().as_ref() == Some(*value)
                    })
                })
            {
                matching.push(object);
            }
        }
        return (matching.len() == 1).then(|| matching[0]);
    }
    Some(node)
}
/// Schema metadata does not constrain the sampled object shape.
fn annotation(key: &str) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Keywords {
        annotations: Vec<String>,
    }
    static KEYWORDS: std::sync::LazyLock<Keywords> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("annotations.toml"))
            .expect("the schema annotation keywords are typed data")
    });
    KEYWORDS.annotations.iter().any(|name| name == key)
}
fn optional<'a>(base: &'a Baseline, steps: &[Step]) -> Result<Optional<'a>, String> {
    let mut node = &base.schema;
    let mut input = &base.input;
    for (index, step) in steps.iter().enumerate() {
        node = resolved(&base.schema, node, input, 0)
            .ok_or_else(|| "no unambiguous concrete owning-object declaration".to_string())?;
        match step {
            Step::Index(item) => {
                node = node
                    .get("items")
                    .filter(|node| node.is_object())
                    .ok_or("no direct list item declaration")?;
                input = input.get(*item).ok_or("list item is not active")?;
            }
            Step::Key(key) => {
                let property = node.get("properties").and_then(|p| p.get(key));
                if index + 1 == steps.len() {
                    let property = property
                        .ok_or("a free entry itself has no named-property optional declaration")?;
                    if let Some(required) = node.get("required") {
                        let required = required
                            .as_array()
                            .ok_or("malformed required declaration")?;
                        if required.iter().any(|key| !key.is_string()) {
                            return Err("malformed required declaration".into());
                        }
                        if required.iter().any(|name| name.as_str() == Some(key)) {
                            return Err(
                                "the local object independently requires this property".into()
                            );
                        }
                    }
                    let value = input.get(key).ok_or("property is not active")?;
                    let expanded = resolved(&base.schema, property, value, 0);
                    let default = property
                        .get("default")
                        .or_else(|| expanded.and_then(|node| node.get("default")));
                    let comparable = default.is_some_and(|value| {
                        value.is_null()
                            || value.is_boolean()
                            || value.is_number()
                            || value.is_string()
                    }) && property.get("format").is_none()
                        && expanded.is_some_and(|node| node.get("format").is_none());
                    return Ok(Optional {
                        default,
                        comparable,
                    });
                }
                node = property
                    .or_else(|| node.get("additionalProperties"))
                    .filter(|node| node.is_object())
                    .ok_or("no direct property or record-value declaration")?;
                input = input.get(key).ok_or("property is not active")?;
            }
        }
    }
    Err("a list item itself is not an optional object property".into())
}
fn remove(input: &mut toml::Value, steps: &[Step]) -> Result<toml::Value, String> {
    let (last, parents) = steps.split_last().ok_or("property has no path")?;
    let mut at = input;
    for step in parents {
        at = match step {
            Step::Key(key) => at.get_mut(key),
            Step::Index(index) => at.get_mut(*index),
        }
        .ok_or("property parent is not active")?;
    }
    let Step::Key(key) = last else {
        return Err("a list item itself is not an optional object property".into());
    };
    let parent = at.as_table_mut().ok_or("property parent is not a record")?;
    if parent.len() <= 1
        || parents.is_empty()
            && parent
                .keys()
                .filter(|key| !crate::constants::RESERVED.contains(&key.as_str()))
                .count()
                <= 1
    {
        return Err("removal would leave an empty parent record".into());
    }
    parent
        .remove(key)
        .ok_or_else(|| "property is not active".into())
}

#[cfg(test)]
mod tests;
