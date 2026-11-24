//! Concrete retained leaves, using the same schema traversal as field flags.
use super::*;
use crate::fields::At;

#[derive(Clone)]
pub(super) enum Step {
    Key(String),
    Index(usize),
}
pub(super) struct Candidate {
    pub steps: Vec<Step>,
    pub witness: TypeRefusal,
    pub choices: Option<Vec<serde_json::Value>>,
    pub required: bool,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Scope {
    Primitive,
    Container,
    Examples,
    Omissions,
}
pub(super) struct Example {
    pub steps: Vec<Step>,
    pub field: String,
    pub pointer: String,
    pub index: usize,
    pub value: serde_json::Value,
}
pub(super) fn candidates(
    base: &Baseline,
    report: &mut TypeRefusals,
    scope: Scope,
) -> anyhow::Result<Vec<Candidate>> {
    Ok(sampled(base, report, scope)?.0)
}
pub(super) fn examples(base: &Baseline) -> anyhow::Result<Vec<Example>> {
    Ok(sampled(base, &mut TypeRefusals::default(), Scope::Examples)?.1)
}
fn sampled(
    base: &Baseline,
    report: &mut TypeRefusals,
    scope: Scope,
) -> anyhow::Result<(Vec<Candidate>, Vec<Example>)> {
    let mut scan = Active {
        base,
        report,
        found: Vec::new(),
        visited: 0,
        scope,
        examples: Vec::new(),
    };
    scan.walk(
        &At::top(&base.schema),
        &base.input,
        &mut Vec::new(),
        &mut Vec::new(),
        false,
        false,
    )?;
    Ok((scan.found, scan.examples))
}
struct Active<'a> {
    base: &'a Baseline,
    report: &'a mut TypeRefusals,
    found: Vec<Candidate>,
    visited: usize,
    scope: Scope,
    examples: Vec<Example>,
}
impl Active<'_> {
    fn walk(
        &mut self,
        at: &At<'_>,
        value: &toml::Value,
        steps: &mut Vec<Step>,
        field: &mut Vec<String>,
        is_field: bool,
        required: bool,
    ) -> anyhow::Result<()> {
        self.visited += 1;
        ensure!(
            self.visited <= crate::constants::MAX_PROBES
                && field.len() <= crate::constants::MAX_DEPTH,
            "the active sample exceeds the bounded field traversal"
        );
        let pointer = pointer(steps);
        if self.scope == Scope::Examples && !steps.is_empty() {
            for (index, sample) in at.sample_values().iter().enumerate() {
                if serde_json::to_value(value)? == **sample {
                    continue;
                }
                ensure!(
                    self.examples.len() < crate::constants::MAX_PROBES,
                    "too many maintained example samples"
                );
                self.examples.push(Example {
                    steps: steps.clone(),
                    field: field.join("."),
                    pointer: pointer.clone(),
                    index,
                    value: (*sample).clone(),
                });
            }
        }
        let focused = self.base.focus.as_ref().is_none_or(|focus| {
            pointer == *focus
                || pointer
                    .strip_prefix(focus)
                    .is_some_and(|rest| rest.starts_with('/'))
        });
        if is_field && focused && self.scope != Scope::Examples {
            let types = at.types();
            let declared = if self.scope == Scope::Omissions {
                Some("observable")
            } else {
                types
                    .as_ref()
                    .filter(|types| types.len() == 1)
                    .and_then(|types| types.first().copied())
                    .filter(|kind| match self.scope {
                        Scope::Primitive => {
                            matches!(*kind, "string" | "boolean" | "integer" | "number")
                        }
                        Scope::Container => matches!(*kind, "array" | "object"),
                        Scope::Examples | Scope::Omissions => false,
                    })
            };
            let reason = match declared {
                None => Some("schema kind is open, mixed, or outside this check's scope"),
                Some(kind) if self.scope != Scope::Omissions && !toml_kind(value, kind) => {
                    Some("the active input does not have the declared kind")
                }
                Some(kind)
                    if self.scope != Scope::Omissions
                        && !self
                            .base
                            .typed
                            .pointer(&pointer)
                            .is_some_and(|v| json_kind(v, kind)) =>
                {
                    Some("the field is not retained in typed serialization")
                }
                Some(kind)
                    if self.scope == Scope::Primitive
                        && !get(&self.base.flags, steps).is_some_and(|v| toml_kind(v, kind)) =>
                {
                    Some("the primitive field is not retained in the emitted flag table")
                }
                Some(_)
                    if self.scope == Scope::Omissions
                        && self.base.typed.pointer(&pointer).is_none() =>
                {
                    Some("the active field is not retained in typed serialization")
                }
                Some(kind) => {
                    self.found.push(Candidate {
                        steps: steps.clone(),
                        choices: at.scalar_choices(),
                        required,
                        witness: TypeRefusal {
                            field: field.join("."),
                            pointer: pointer.clone(),
                            declared: kind.into(),
                            rejected: if kind == "boolean" {
                                "integer"
                            } else {
                                "boolean"
                            }
                            .into(),
                            counterexample: String::new(),
                        },
                    });
                    None
                }
            };
            if let Some(reason) = reason {
                self.report.excluded.push(TypeExclusion {
                    field: field.join("."),
                    pointer,
                    reason: reason.into(),
                });
            }
        }
        match value {
            toml::Value::Table(table) => {
                for (key, inner) in table {
                    if field.is_empty() && crate::constants::RESERVED.contains(&key.as_str()) {
                        continue;
                    }
                    if let Some(child) = at.child(key) {
                        steps.push(Step::Key(key.clone()));
                        field.push(if at.keys().contains(key.as_str()) {
                            key.clone()
                        } else {
                            "*".into()
                        });
                        self.walk(&child, inner, steps, field, true, at.required(key))?;
                        field.pop();
                        steps.pop();
                    }
                }
            }
            toml::Value::Array(items) => {
                if let Some(child) = at.element() {
                    for (index, item) in items.iter().enumerate() {
                        steps.push(Step::Index(index));
                        // A primitive list item is not a separately declared field.
                        self.walk(&child, item, steps, field, false, false)?;
                        steps.pop();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
fn toml_kind(value: &toml::Value, kind: &str) -> bool {
    matches!(
        (value, kind),
        (toml::Value::String(_), "string")
            | (toml::Value::Boolean(_), "boolean")
            | (toml::Value::Integer(_), "integer" | "number")
            | (toml::Value::Float(_), "number")
            | (toml::Value::Array(_), "array")
            | (toml::Value::Table(_), "object")
    )
}
fn json_kind(value: &serde_json::Value, kind: &str) -> bool {
    match kind {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_number().is_some_and(|n| n.is_i64() || n.is_u64()),
        "number" => value.is_number(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    }
}
fn pointer(steps: &[Step]) -> String {
    steps
        .iter()
        .map(|step| match step {
            Step::Key(key) => format!("/{}", key.replace('~', "~0").replace('/', "~1")),
            Step::Index(index) => format!("/{index}"),
        })
        .collect()
}
pub(super) fn get<'a>(value: &'a toml::Value, steps: &[Step]) -> Option<&'a toml::Value> {
    let (first, rest) = match steps.split_first() {
        Some(parts) => parts,
        None => return Some(value),
    };
    get(
        match first {
            Step::Key(key) => value.get(key)?,
            Step::Index(index) => value.get(*index)?,
        },
        rest,
    )
}
pub(super) fn replace(value: &mut toml::Value, steps: &[Step], changed: toml::Value) {
    let Some((first, rest)) = steps.split_first() else {
        *value = changed;
        return;
    };
    let inner = match first {
        Step::Key(key) => value.get_mut(key),
        Step::Index(index) => value.get_mut(*index),
    }
    .expect("an active path in the cloned baseline");
    replace(inner, rest, changed);
}
