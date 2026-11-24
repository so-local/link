//! Finite scalar literals checked by the owning reader, not a schema validator.
use super::*;
use serde_json::Value;

/// One declared finite value or actually accepted scalar contrast sample.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ScalarChoice {
    pub field: String,
    pub pointer: String,
    pub value: Value,
    /// A same-kind value outside the closed domain, refused by the reader.
    pub rejected: Option<Value>,
    /// A different actually valid file literal overridden by this value's flags.
    pub precedence: Option<Value>,
    /// Generated raw contrast, only after actual file/flag controls accept it.
    /// Its presence marks an open sample, never a declared finite choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_sample: Option<Value>,
    /// One actual explicit field flag used by the open-sample precedence test.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolated_flag: Option<String>,
}

/// Exact finite samples/accepted contrasts and fields outside their proof.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct ScalarChoices {
    pub witnessed: Vec<ScalarChoice>,
    pub excluded: Vec<TypeExclusion>,
}

/// Problems preserving finite scalar choices through real file and flag readers.
pub fn check_scalar_choices<T: Kind>() -> Vec<String> {
    scalar_choices::<T>().err().map_or_else(Vec::new, |error| {
        vec![format!("spec scalar choices: {error:#}")]
    })
}

/// Check retained finite domains and one contrast for other active scalars.
/// Declared literals supply finite expectations; generated contrasts supply
/// none until actual full file and isolated flag controls accept them.
pub fn scalar_choices<T: Kind>() -> anyhow::Result<ScalarChoices> {
    checked_with::<T>(crate::flags_of::<T>)
}
fn checked_with<T: Kind>(
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<ScalarChoices> {
    checked_reader_with::<T>(emit, crate::read_text_at::<T>)
}
fn checked_reader_with<T: Kind>(
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
    overlay: fn(&str, &std::path::Path, &FieldFlags) -> anyhow::Result<T>,
) -> anyhow::Result<ScalarChoices> {
    let base = baseline::<T>(emit)?;
    checked_base::<T>(&base, emit, overlay)
}
pub(super) fn checked_base<T: Kind>(
    base: &Baseline,
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
    overlay: fn(&str, &std::path::Path, &FieldFlags) -> anyhow::Result<T>,
) -> anyhow::Result<ScalarChoices> {
    let mut types = TypeRefusals::default();
    let candidates = active::candidates(base, &mut types, active::Scope::Primitive)?;
    let mut report = ScalarChoices {
        excluded: types.excluded,
        ..Default::default()
    };
    let binding = Binding::of::<T>();
    let mut probes = 0;
    for candidate in candidates {
        let Some(values) = &candidate.choices else {
            probes += 1;
            ensure!(
                probes <= crate::constants::MAX_PROBES,
                "too many scalar probes"
            );
            if let Some(witness) =
                precedence::check::<T>(base, &candidate, emit, overlay, &mut report)?
            {
                report.witnessed.push(witness);
            }
            continue;
        };
        probes += values.len() + 1;
        ensure!(
            probes <= crate::constants::MAX_PROBES,
            "too many finite scalar probes"
        );
        let rejected = outside(values, &candidate.witness.declared);
        if let Some(wrong) = &rejected {
            let text = changed(base, &candidate, wrong)?;
            ensure!(
                crate::read_text_at::<T>(&text, &base.dir, &FieldFlags::default()).is_err(),
                "{} at {}: the owning reader accepted {wrong} outside its closed domain",
                candidate.witness.field,
                candidate.witness.pointer
            );
        }
        for value in values {
            let text = changed(base, &candidate, value)?;
            let spec = crate::read_text_at::<T>(&text, &base.dir, &FieldFlags::default())
                .with_context(|| {
                    format!(
                        "{}: reading declared choice {value}",
                        candidate.witness.field
                    )
                })?;
            let typed = serde_json::to_value(&spec)?;
            let Some(actual) = typed.pointer(&candidate.witness.pointer) else {
                exclude(
                    &mut report,
                    &candidate,
                    &format!("choice {value} absent from typed serialization"),
                );
                continue;
            };
            ensure!(
                actual == value,
                "{} at {}: the owning reader did not preserve choice {value}, got {actual}",
                candidate.witness.field,
                candidate.witness.pointer
            );
            let (flags, rest) = crate::split(
                emit(&spec)?.into_iter().map(Into::into),
                Some(&binding),
                None,
            )?;
            ensure!(rest.is_empty(), "a choice emits runtime arguments");
            let again = crate::read_text_at::<T>("", &base.dir, &flags)?;
            ensure!(
                serde_json::to_value(again)? == typed,
                "{}: field flags change the typed choice {value}",
                candidate.witness.field
            );
            let table = serde_json::to_value(flags.table())?;
            if table.pointer(&candidate.witness.pointer) != Some(value) {
                exclude(
                    &mut report,
                    &candidate,
                    &format!("choice {value} absent from emitted flag table"),
                );
                continue;
            }
            let mut precedence = None;
            if let Some(other) = values.iter().find(|other| *other != value) {
                let other_text = changed(base, &candidate, other)?;
                let other_spec =
                    crate::read_text_at::<T>(&other_text, &base.dir, &FieldFlags::default())?;
                // A raw spelling difference alone is not a conflicting typed file.
                if serde_json::to_value(other_spec)? != typed {
                    let combined = overlay(&other_text, &base.dir, &flags)?;
                    ensure!(
                        serde_json::to_value(combined)? == typed,
                        "{} at {}: precedence failed for file {other} and flag {value}",
                        candidate.witness.field,
                        candidate.witness.pointer
                    );
                    precedence = Some(other.clone());
                }
            }
            report.witnessed.push(ScalarChoice {
                field: candidate.witness.field.clone(),
                pointer: candidate.witness.pointer.clone(),
                value: value.clone(),
                rejected: rejected.clone(),
                precedence,
                open_sample: None,
                isolated_flag: None,
            });
        }
    }
    Ok(report)
}
fn exclude(report: &mut ScalarChoices, candidate: &active::Candidate, reason: &str) {
    report.excluded.push(TypeExclusion {
        field: candidate.witness.field.clone(),
        pointer: candidate.witness.pointer.clone(),
        reason: reason.into(),
    });
}
fn changed(
    base: &Baseline,
    candidate: &active::Candidate,
    value: &Value,
) -> anyhow::Result<String> {
    let mut input = base.input.clone();
    active::replace(&mut input, &candidate.steps, toml::Value::try_from(value)?);
    Ok(toml::to_string(&input)?)
}
fn outside(values: &[Value], kind: &str) -> Option<Value> {
    if kind == "boolean" {
        return [Value::Bool(false), Value::Bool(true)]
            .into_iter()
            .find(|v| !values.contains(v));
    }
    let mut word = "__link_spec_invalid_choice__".to_owned();
    while values.iter().any(|v| v.as_str() == Some(&word)) {
        word.push('_');
    }
    Some(word.into())
}

mod precedence;
#[cfg(test)]
mod tests;
