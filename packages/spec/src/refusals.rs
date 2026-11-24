//! Bounded wrong-kind checks on fields retained by the actual Every Option reader.
use crate::{Binding, Exported, FieldFlags, Kind};
use anyhow::{Context, ensure};
use std::path::PathBuf;
mod active;
mod structural;
pub use structural::{
    StructuralExclusion, StructuralType, StructuralTypes, check_structural_types, structural_types,
};
mod examples;
pub use examples::{
    ExampleExclusion, ExampleSample, ExampleSamples, check_example_readers, example_readers,
};
mod choices;
pub use choices::{ScalarChoice, ScalarChoices, check_scalar_choices, scalar_choices};
mod omissions;
pub use omissions::{Omission, OmissionState, Omissions, check_omissions, omissions};
mod required;
pub use required::{RequiredRefusal, RequiredRefusals, check_required_refusals, required_refusals};

/// One actual owning-reader refusal, at a concrete sampled field.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TypeRefusal {
    pub field: String,
    pub pointer: String,
    pub declared: String,
    pub rejected: String,
    /// The exact TOML literal refused; no claim about every value of its kind.
    pub counterexample: String,
}

/// A sampled field outside this primitive guard's proof.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TypeExclusion {
    pub field: String,
    pub pointer: String,
    pub reason: String,
}

/// Witnesses and explicit exclusions; never a claim about unvisited arms.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct TypeRefusals {
    pub witnessed: Vec<TypeRefusal>,
    pub excluded: Vec<TypeExclusion>,
}

/// Problems found by the bounded retained-field wrong-kind guard.
pub fn check_type_refusals<T: Kind>() -> Vec<String> {
    type_refusals::<T>().err().map_or_else(Vec::new, |error| {
        vec![format!("spec input type refusal: {error:#}")]
    })
}

/// Witness wrong TOML kinds only for actual primitive fields retained by
/// Every Option, typed serialization and flags. Each isolated mutation goes
/// through the real owning reader; no schema validator substitutes for it.
/// Open/mixed kinds, containers and unretained fields are excluded explicitly.
pub fn type_refusals<T: Kind>() -> anyhow::Result<TypeRefusals> {
    checked_with::<T>(crate::flags_of::<T>)
}

/// Problems refusing wrong TOML kinds for retained active array/object fields.
pub fn check_container_type_refusals<T: Kind>() -> Vec<String> {
    container_type_refusals::<T>()
        .err()
        .map_or_else(Vec::new, |error| {
            vec![format!("spec container type refusal: {error:#}")]
        })
}

/// Arrays and objects retained by typed serialization, with a valid same-folder
/// file/flag baseline. A container need not have an independent emitted flag:
/// its actual owning file reader must refuse a boolean in that field instead.
pub fn container_type_refusals<T: Kind>() -> anyhow::Result<TypeRefusals> {
    checked_in::<T>(crate::flags_of::<T>, active::Scope::Container)
}

fn checked_with<T: Kind>(
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<TypeRefusals> {
    checked_in::<T>(emit, active::Scope::Primitive)
}

fn checked_in<T: Kind>(
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
    scope: active::Scope,
) -> anyhow::Result<TypeRefusals> {
    let base = baseline::<T>(emit)?;
    checked_base::<T>(&base, scope)
}
fn checked_base<T: Kind>(base: &Baseline, scope: active::Scope) -> anyhow::Result<TypeRefusals> {
    let mut report = TypeRefusals::default();
    for mut candidate in active::candidates(base, &mut report, scope)? {
        let mut input = base.input.clone();
        let wrong = if candidate.witness.declared == "boolean" {
            toml::Value::Integer(0)
        } else {
            toml::Value::Boolean(false)
        };
        candidate.witness.counterexample = wrong.to_string();
        active::replace(&mut input, &candidate.steps, wrong);
        let text = toml::to_string(&input)?;
        ensure!(
            crate::read_text_at::<T>(&text, &base.dir, &FieldFlags::default()).is_err(),
            "{} at {}: the owning reader accepted a {} for its declared {} field",
            candidate.witness.field,
            candidate.witness.pointer,
            candidate.witness.rejected,
            candidate.witness.declared
        );
        report.witnessed.push(candidate.witness);
    }
    Ok(report)
}

struct Baseline {
    dir: PathBuf,
    input: toml::Value,
    typed: serde_json::Value,
    flags: toml::Value,
    schema: serde_json::Value,
    focus: Option<String>,
}
fn baseline<T: Kind>(emit: fn(&T) -> anyhow::Result<Vec<String>>) -> anyhow::Result<Baseline> {
    let exported = Exported::of::<T>();
    let text = crate::reference::every_option_text(&exported);
    baseline_text::<T>(&text, emit)
}
fn baseline_text<T: Kind>(
    text: &str,
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<Baseline> {
    let dir = std::env::current_dir().context("reading the spec's source folder")?;
    let value = crate::read_text_at::<T>(text, &dir, &FieldFlags::default())
        .context("reading unmutated Every Option")?;
    let typed = serde_json::to_value(&value)?;
    let input: toml::Value = toml::from_str(text)?;
    let normalized =
        crate::read_text_at::<T>(&toml::to_string(&input)?, &dir, &FieldFlags::default())
            .context("reading unmutated normalized Every Option")?;
    ensure!(
        serde_json::to_value(normalized)? == typed,
        "normalizing the baseline changes its typed configuration"
    );
    let binding = Binding::of::<T>();
    let (flags, rest) = crate::split(
        emit(&value)?.into_iter().map(Into::into),
        Some(&binding),
        None,
    )?;
    ensure!(rest.is_empty(), "the baseline emits runtime arguments");
    let flagged =
        crate::read_text_at::<T>("", &dir, &flags).context("reading unmutated field flags")?;
    ensure!(
        serde_json::to_value(flagged)? == typed,
        "file and flag baselines differ"
    );
    Ok(Baseline {
        dir,
        input,
        typed,
        flags: toml::Value::Table(flags.table().clone()),
        schema: crate::schema::<T>(),
        focus: None,
    })
}

#[cfg(test)]
mod tests;
