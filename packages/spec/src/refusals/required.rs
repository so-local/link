//! Required scalar omissions checked against an independent object declaration.
use super::*;

/// One actual owning-reader refusal of an independently required scalar field.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RequiredRefusal {
    pub field: String,
    pub pointer: String,
}
/// Required scalar omissions and the active fields outside this check.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct RequiredRefusals {
    pub witnessed: Vec<RequiredRefusal>,
    pub excluded: Vec<TypeExclusion>,
}
/// Problems refusing omissions of retained scalars required by one object shape.
pub fn check_required_refusals<T: Kind>() -> Vec<String> {
    required_refusals::<T>()
        .err()
        .map_or_else(Vec::new, |error| {
            vec![format!("spec required scalar refusal: {error:#}")]
        })
}
/// The schema's unconditional object requirement supplies the expected refusal;
/// the actual reader decides whether removing the concrete field is refused.
/// Optional/defaulted fields and composed object requirements receive no proof.
pub fn required_refusals<T: Kind>() -> anyhow::Result<RequiredRefusals> {
    let base = baseline::<T>(crate::flags_of::<T>)?;
    let mut types = TypeRefusals::default();
    let candidates = active::candidates(&base, &mut types, active::Scope::Primitive)?;
    let mut report = RequiredRefusals {
        excluded: types.excluded,
        ..Default::default()
    };
    for candidate in candidates {
        if !candidate.required {
            report.excluded.push(TypeExclusion {
                field: candidate.witness.field,
                pointer: candidate.witness.pointer,
                reason: "no independent unconditional required key on one object shape".into(),
            });
            continue;
        }
        let mut input = base.input.clone();
        remove(&mut input, &candidate.steps)?;
        let text = toml::to_string(&input)?;
        ensure!(
            crate::read_text_at::<T>(&text, &base.dir, &FieldFlags::default()).is_err(),
            "{} at {}: the owning reader accepted omission of its independently required scalar",
            candidate.witness.field,
            candidate.witness.pointer
        );
        report.witnessed.push(RequiredRefusal {
            field: candidate.witness.field,
            pointer: candidate.witness.pointer,
        });
    }
    Ok(report)
}

fn remove(input: &mut toml::Value, steps: &[active::Step]) -> anyhow::Result<()> {
    let (last, parents) = steps.split_last().context("a required field has a path")?;
    let mut at = input;
    for step in parents {
        at = match step {
            active::Step::Key(key) => at.get_mut(key),
            active::Step::Index(index) => at.get_mut(*index),
        }
        .context("the required field's parent is in the baseline")?;
    }
    let active::Step::Key(key) = last else {
        anyhow::bail!("a list item is not a required object field")
    };
    ensure!(
        at.as_table_mut()
            .context("a required field's parent is an object")?
            .remove(key)
            .is_some(),
        "the required field is in the baseline"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
