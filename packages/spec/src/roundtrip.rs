//! Check serialization and field flags through the registered kind's reader.

use crate::{Binding, Exported, FieldFlags, Kind};

/// Every Option, printed and expressed as field flags, must read as the
/// same configuration. This checks the serializer as well as the shape:
/// an invocation-only field emitted by a serializer is refused by the
/// strict reader even when the declared schema remains correct.
pub fn check_roundtrip<T: Kind>() -> Vec<String> {
    checked::<T>().err().map_or_else(Vec::new, |error| {
        vec![format!("spec input equivalence: {error:#}")]
    })
}

fn checked<T: Kind>() -> anyhow::Result<()> {
    checked_with::<T>(crate::flags_of::<T>)
}

fn checked_with<T: Kind>(emit: fn(&T) -> anyhow::Result<Vec<String>>) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    let dir = std::env::current_dir().context("reading the spec's source folder")?;
    let exported = Exported::of::<T>();
    let text = crate::reference::every_option_text(&exported);
    let spec = crate::read_text_at::<T>(&text, &dir, &FieldFlags::default())
        .context("reading Every Option")?;
    let printed = T::format().print(&spec).context("printing the spec")?;
    let again = crate::read_text_at::<T>(&printed, &dir, &FieldFlags::default())
        .context("reading the printed spec")?;
    let expected = serde_json::to_value(&spec)?;
    ensure!(
        serde_json::to_value(&again)? == expected,
        "printing changes the configuration"
    );
    // The same source folder gives file and flag paths the same meaning.
    let binding = Binding::of::<T>();
    let words = emit(&spec)?.into_iter().map(Into::into);
    let (flags, rest) = crate::split(words, Some(&binding), None)?;
    ensure!(rest.is_empty(), "a spec emits runtime arguments: {rest:?}");
    let from_flags = crate::read_text_at::<T>("", &dir, &flags).context("reading field flags")?;
    ensure!(
        serde_json::to_value(&from_flags)? == expected,
        "field flags change the typed configuration"
    );
    let (file_flags, _) = crate::split(
        emit(&again)?.into_iter().map(Into::into),
        Some(&binding),
        None,
    )?;
    ensure!(
        flags.table() == file_flags.table(),
        "printing changes field flags"
    );
    let flags_again = emit(&from_flags)?;
    let (normalized, _) = crate::split(
        flags_again.into_iter().map(Into::into),
        Some(&binding),
        None,
    )?;
    ensure!(
        flags.table() == normalized.table(),
        "field flags change the configuration"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
