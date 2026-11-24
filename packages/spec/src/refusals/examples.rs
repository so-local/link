//! Maintained example fragments qualify only in complete real reader contexts.
use super::*;

#[derive(Debug, serde::Serialize)]
pub struct ExampleSample {
    pub field: String,
    pub pointer: String,
    pub index: usize,
    pub value: serde_json::Value,
    pub input: serde_json::Value,
    pub typed: serde_json::Value,
    pub flags: serde_json::Value,
    pub types: TypeRefusals,
    pub containers: TypeRefusals,
    pub choices: ScalarChoices,
    /// Accepted concrete omissions and serialized replay; no hidden default.
    pub omissions: Omissions,
}
#[derive(Debug, serde::Serialize)]
pub struct ExampleExclusion {
    pub field: String,
    pub pointer: String,
    pub index: usize,
    pub value: serde_json::Value,
    pub reason: String,
}
/// Complete valid literal example contexts and incompatible fragment exclusions.
#[derive(Debug, Default, serde::Serialize)]
pub struct ExampleSamples {
    pub witnessed: Vec<ExampleSample>,
    pub excluded: Vec<ExampleExclusion>,
}
/// Problems in valid maintained example contexts; refused file contexts excluded.
pub fn check_example_readers<T: Kind>() -> Vec<String> {
    example_readers::<T>().err().map_or_else(Vec::new, |error| {
        vec![format!("spec maintained examples: {error:#}")]
    })
}
/// Inject only maintained literal examples at existing Every Option pointers.
/// Each complete file must pass the owning reader and Kind check before its
/// typed file/flag equality or focused type/choice assertions supply any proof.
pub fn example_readers<T: Kind>() -> anyhow::Result<ExampleSamples> {
    checked_with::<T>(crate::flags_of::<T>)
}
fn checked_with<T: Kind>(
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<ExampleSamples> {
    let base = baseline::<T>(emit)?;
    let mut report = ExampleSamples::default();
    let mut seen = std::collections::BTreeSet::new();
    for example in active::examples(&base)? {
        let attempt = (|| -> anyhow::Result<String> {
            let mut input = base.input.clone();
            active::replace(
                &mut input,
                &example.steps,
                toml::Value::try_from(&example.value)?,
            );
            let text = toml::to_string(&input)?;
            // A valid fragment may be incompatible with its surrounding fields.
            // That is an exclusion; an accepted file with broken flag replay is not.
            crate::read_text_at::<T>(&text, &base.dir, &FieldFlags::default())?;
            Ok(text)
        })();
        let text = match attempt {
            Ok(text) => text,
            Err(error) => {
                report.excluded.push(ExampleExclusion {
                    field: example.field,
                    pointer: example.pointer,
                    index: example.index,
                    value: example.value,
                    reason: format!("{error:#}"),
                });
                continue;
            }
        };
        if !seen.insert(text.clone()) {
            continue;
        }
        let mut sample = baseline_text::<T>(&text, emit)?;
        sample.focus = Some(example.pointer.clone());
        let types = checked_base::<T>(&sample, active::Scope::Primitive)?;
        let containers = checked_base::<T>(&sample, active::Scope::Container)?;
        let choices = choices::checked_base::<T>(&sample, emit, crate::read_text_at::<T>)?;
        let omissions = omissions::checked_base::<T>(&sample, emit)?;
        report.witnessed.push(ExampleSample {
            field: example.field,
            pointer: example.pointer,
            index: example.index,
            value: example.value,
            input: serde_json::to_value(sample.input)?,
            typed: sample.typed,
            flags: serde_json::to_value(sample.flags)?,
            types,
            containers,
            choices,
            omissions,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
