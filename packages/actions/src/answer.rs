//! An action's answer as its declared output: the value a handler built,
//! read as the output's generated type, which refuses what its shape
//! refuses, and written as that type writes it. The declaration, not the
//! handler's own type, is what goes on the wire. A field the value holds
//! as null is absent, since no output declares a field null.

use anyhow::Context;
use link_format::{Out, Report, WithinExt};
use serde::{Serialize, de::DeserializeOwned};

use crate::value::Validate;

/// `value` as `T`, its declared output, checked: an error for a value
/// its shape refuses. A null field is left out, never written as null.
pub fn output<T: DeserializeOwned + Validate>(value: &impl Serialize) -> anyhow::Result<T> {
    let mut json = serde_json::to_value(value).context("writing the answer")?;
    absent(&mut json);
    let typed: T =
        serde_json::from_value(json).context("reading the answer as its declared output")?;
    typed
        .validate("")
        .map_err(|refusal| anyhow::anyhow!("the answer isn't its declared output: {refusal}"))?;
    Ok(typed)
}

/// Replies `value` as `T`, its declared output: what `--json` prints.
pub fn reply<T: DeserializeOwned + Serialize + Validate>(
    out: &mut Out,
    value: &impl Serialize,
) -> Result<(), Report> {
    let typed: T = output(value)?;
    out.reply(&typed);
    Ok(())
}

/// Replies `item`, one line of a reply read a line at a time, as `T`,
/// its declared output.
pub fn line<T: DeserializeOwned + Serialize + Validate>(
    out: &mut Out,
    item: &impl Serialize,
) -> Result<(), Report> {
    let typed: T = output(item)?;
    out.reply_line(&typed);
    Ok(())
}

/// Answers `value`, another program's answer, as `T`, its declared
/// output: with `--json` the reply, for a person its members as blocks.
pub fn answer<T: DeserializeOwned + Serialize + Validate>(
    out: &mut Out,
    value: &impl Serialize,
) -> Result<(), Report> {
    let typed: T = output(value)?;
    let json = serde_json::to_value(&typed).within("writing the answer")?;
    out.answer(&json);
    Ok(())
}

/// `item` as `T`, its declared output, written as one line of JSON: a
/// line of a protocol a program reads on standard output.
pub fn json_line<T: DeserializeOwned + Serialize + Validate>(
    item: &impl Serialize,
) -> anyhow::Result<Vec<u8>> {
    let typed: T = output(item)?;
    let mut line = serde_json::to_vec(&typed).context("writing the answer")?;
    line.push(b'\n');
    Ok(line)
}

/// `value` with each null field of each object it holds left out.
fn absent(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            fields.retain(|_, field| !field.is_null());
            fields.values_mut().for_each(absent);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(absent),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A null field is left out; a null in a list stays.
    #[test]
    fn a_null_field_is_absent() {
        let mut value = serde_json::json!({"a": null, "b": {"c": null, "d": 1}, "e": [null]});
        absent(&mut value);
        assert_eq!(value, serde_json::json!({"b": {"d": 1}, "e": [null]}));
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Declared {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
    }

    impl Validate for Declared {
        fn validate(&self, at: &str) -> Result<(), crate::value::Refusal> {
            crate::value::length(at, &self.name, Some(1), None)
        }
    }

    /// What the handler built is written as the declaration writes it,
    /// and a field it doesn't declare, or a value it refuses, is an error.
    #[test]
    fn an_answer_is_its_declared_output() {
        let written = json_line::<Declared>(&serde_json::json!({"name": "a"})).unwrap();
        assert_eq!(written, b"{\"name\":\"a\"}\n");
        assert!(output::<Declared>(&serde_json::json!({"name": "a", "extra": 1})).is_err());
        assert!(output::<Declared>(&serde_json::json!({"name": ""})).is_err());
        let written = json_line::<Declared>(&serde_json::json!({"name": "a", "size": null}));
        assert_eq!(
            written.unwrap(),
            b"{\"name\":\"a\"}\n",
            "an absent key is never null"
        );
    }
}
