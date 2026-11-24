//! Accepted contrasting scalar pairs through one actual isolated field flag.
use super::*;
use active::Step;

pub(super) fn check<T: Kind>(
    base: &Baseline,
    candidate: &active::Candidate,
    emit: fn(&T) -> anyhow::Result<Vec<String>>,
    overlay: fn(&str, &std::path::Path, &FieldFlags) -> anyhow::Result<T>,
    report: &mut ScalarChoices,
) -> anyhow::Result<Option<ScalarChoice>> {
    let field = &candidate.witness.field;
    let pointer = &candidate.witness.pointer;
    let Some(before) = base.typed.pointer(pointer) else {
        exclude(report, candidate, "no retained baseline scalar target");
        return Ok(None);
    };
    let Some(raw) = active::get(&base.input, &candidate.steps).and_then(contrast) else {
        exclude(
            report,
            candidate,
            "no finite different scalar contrast candidate",
        );
        return Ok(None);
    };
    let raw = serde_json::to_value(raw)?;
    let text = changed(base, candidate, &raw)?;
    let spec = match crate::read_text_at::<T>(&text, &base.dir, &FieldFlags::default()) {
        Ok(spec) => spec,
        Err(error) => {
            exclude(
                report,
                candidate,
                &format!("contrasting full file rejected: {error:#}"),
            );
            return Ok(None);
        }
    };
    let typed = serde_json::to_value(&spec)?;
    let Some(after) = typed.pointer(pointer).filter(|value| *value != before) else {
        exclude(
            report,
            candidate,
            "contrast is elided or not observably different at its typed target",
        );
        return Ok(None);
    };
    let binding = Binding::of::<T>();
    let emitted = match emit(&spec)
        .and_then(|words| crate::split(words.into_iter().map(Into::into), Some(&binding), None))
    {
        Ok((flags, rest)) if rest.is_empty() => flags,
        _ => {
            exclude(
                report,
                candidate,
                "contrast cannot be emitted as field flags alone",
            );
            return Ok(None);
        }
    };
    if crate::read_text_at::<T>("", &base.dir, &emitted)
        .and_then(|value| serde_json::to_value(value).map_err(Into::into))
        .ok()
        .as_ref()
        != Some(&typed)
    {
        exclude(
            report,
            candidate,
            "contrasting full flag replay is rejected or changes the serialized configuration",
        );
        return Ok(None);
    }
    let emitted_json = serde_json::to_value(emitted.table())?;
    if emitted_json.pointer(pointer) != Some(after) {
        exclude(
            report,
            candidate,
            "contrast absent from the actual emitted field flag table",
        );
        return Ok(None);
    }
    // List elements have no independent flags. Their enclosing collection's
    // real flag can carry the changed record; other keys use their exact path.
    let keys: Vec<_> = candidate
        .steps
        .iter()
        .take_while(|step| matches!(step, Step::Key(_)))
        .filter_map(|step| match step {
            Step::Key(key) => Some(key.as_str()),
            Step::Index(_) => None,
        })
        .collect();
    let Some(value) = emitted.get(&keys).filter(|_| !keys.is_empty()) else {
        exclude(
            report,
            candidate,
            "contrast has no independently targetable field flag",
        );
        return Ok(None);
    };
    let path: Vec<_> = keys.iter().map(|key| key.to_string()).collect();
    let word = format!("--{}={value}", crate::fields::flag_of(&path));
    let isolated = match crate::split([word.clone().into()], Some(&binding), None) {
        Ok((flags, rest)) if rest.is_empty() => flags,
        _ => {
            exclude(
                report,
                candidate,
                "isolated explicit contrast flag is not accepted",
            );
            return Ok(None);
        }
    };
    // The other required fields remain in the context. Remove only this real
    // flag scope so acceptance cannot be credited to an already-present B.
    let mut context = base.input.clone();
    remove_scope(&mut context, &keys)?;
    let control = toml::to_string(&context)?;
    if overlay(&control, &base.dir, &isolated)
        .and_then(|value| serde_json::to_value(value).map_err(Into::into))
        .ok()
        .as_ref()
        != Some(&typed)
    {
        exclude(
            report,
            candidate,
            "isolated explicit contrast flag control is rejected or does not match full file B",
        );
        return Ok(None);
    }
    let combined = overlay(&toml::to_string(&base.input)?, &base.dir, &isolated)
        .with_context(|| format!("{field} at {pointer}: open scalar precedence combined input"))?;
    ensure!(
        serde_json::to_value(combined)? == typed,
        "{field} at {pointer}: open scalar precedence failed for actual file target {before} and isolated flag target {after}"
    );
    Ok(Some(ScalarChoice {
        field: field.clone(),
        pointer: pointer.clone(),
        value: after.clone(),
        rejected: None,
        precedence: Some(before.clone()),
        open_sample: Some(raw),
        isolated_flag: Some(word),
    }))
}
fn contrast(value: &toml::Value) -> Option<toml::Value> {
    Some(match value {
        toml::Value::Boolean(value) => toml::Value::Boolean(!value),
        toml::Value::Integer(value) => {
            toml::Value::Integer(value.checked_add(1).or_else(|| value.checked_sub(1))?)
        }
        toml::Value::Float(value) => {
            let next = if *value == 0.0 { 1.0 } else { *value / 2.0 };
            if !next.is_finite() || next == *value {
                return None;
            }
            toml::Value::Float(next)
        }
        toml::Value::String(value) => toml::Value::String(format!("{value}-contrast")),
        _ => return None,
    })
}
fn remove_scope(input: &mut toml::Value, keys: &[&str]) -> anyhow::Result<()> {
    let (last, parents) = keys.split_last().context("an isolated flag has a path")?;
    let mut at = input;
    for key in parents {
        at = at
            .get_mut(*key)
            .context("flag scope parent exists in baseline")?;
    }
    ensure!(
        at.as_table_mut()
            .context("flag scope parent is an object")?
            .remove(*last)
            .is_some(),
        "flag scope exists in baseline"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
