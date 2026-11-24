//! Bind reference pages to their actual released types, never copied field lists.

use std::collections::BTreeMap;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use super::{
    fields::{self, Binding, Fields},
    source,
};

pub struct Reference {
    pub page: &'static str,
    pub owner: &'static str,
    pub fields: Fields,
    pub defaults: Value,
    pub schema: Option<&'static link_spec::schema::Format>,
}
impl Reference {
    pub fn render(&self) -> String {
        let scope = if self.owner.contains("TeamFile") {
            " The list covers the team head; shared opinion tables use the Agent Spec."
        } else if self.owner.contains("Machine, Kind") {
            " These are identity and root-owned kind records. Other switches belong to their feature consumers, and variant fields depend on kind."
        } else {
            ""
        };
        let mut text = format!(
            "<!-- spec-reference:start -->\n\n## Checked Reference\n\nThese fields are checked against the released type. An unset optional\nvalue waits for its feature to resolve it.{scope}\n\n```text\n",
        );
        for (path, keys) in &self.fields {
            text += &format!(
                "{}: {}\n",
                if path.is_empty() { "root" } else { path },
                keys.join(", ")
            );
        }
        text += "```\n";
        if let Some(schema) = self.schema {
            text += &format!(
                "\n### `kind` and `schema`\n\nThe optional `kind` names the kind, `{}`, and the optional positive\ninteger `schema` its format, now `{}`: omission means current. Supported\nprevious formats are transformed first, and a future one asks for `lnk upgrade`.\n",
                schema.kind, schema.current
            );
            // Its print command, where a published module answers it.
            let printed = schema.command.as_deref().filter(|printed| {
                let group = printed.split_whitespace().nth(1).unwrap_or_default();
                crate::parsers::commands().iter().any(|command| {
                    command
                        .get_subcommands()
                        .any(|command| command.get_name() == group)
                })
            });
            if let Some(printed) = printed {
                // A kind's `spec show` takes its file as `--spec`.
                let print = match printed.ends_with(" spec show") {
                    true => format!("{printed} --spec"),
                    false => printed.to_string(),
                };
                text += &format!(
                    "`{print} <file>` prints the current form, and `lnk spec convert <file>`\nrewrites the source, preserving comments and order.\n",
                );
            } else {
                text += "The owning library parses and prints this format.\n";
            }
        }
        if let Some(defaults) = self
            .defaults
            .as_object()
            .filter(|defaults| !defaults.is_empty())
        {
            text += "\n### Checked Defaults\n\nThese values come from the built-in defaults, including nested settings.\nOmitted optional values have no stored default at this layer.\n\n```text\n";
            for (key, value) in defaults {
                text += &format!("{key} = {value}\n");
            }
            text += "```\n";
        }
        text += "\n<!-- spec-reference:end -->";
        text
    }
}

fn discover<T: DeserializeOwned>() -> Result<Fields, String> {
    fields::discover::<T>(&BTreeMap::new())
}
/// The protocol's declarations, where a schedule's `to` and `when` are
/// declared.
const PROTOCOL_TYPES: &str = include_str!(
    "../../../../../modules/agents/harnesses/portability/agent-state-protocol/types.toml"
);

fn schedule_destination() -> Result<Fields, String> {
    declared_choice(PROTOCOL_TYPES, "destination")
}
fn schedule_when() -> Result<Fields, String> {
    declared_choice(PROTOCOL_TYPES, "when")
}

/// The fields of a declared choice `shape` in `types`: the tag and every
/// choice's own, as a tagged value is written.
fn declared_choice(types: &str, shape: &str) -> Result<Fields, String> {
    let table: toml::Table = toml::from_str(types).map_err(|e| e.to_string())?;
    let choices = table
        .get(shape)
        .and_then(|s| s.get("oneOf"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("types.toml declares no choice {shape}"))?;
    let mut keys = Vec::new();
    for choice in choices {
        let name = choice.as_str().ok_or("a choice names its shape")?;
        let properties = table
            .get(name)
            .and_then(|s| s.get("properties"))
            .and_then(toml::Value::as_table)
            .ok_or_else(|| format!("types.toml declares no record {name}"))?;
        keys.extend(properties.keys().cloned());
    }
    keys.sort();
    keys.dedup();
    let mut fields = Fields::new();
    fields.insert(String::new(), keys);
    Ok(fields)
}

fn schedule_reference() -> Result<Reference, String> {
    let fields = fields::discover::<link_agent_spec::Schedule>(&BTreeMap::from([
        ("to".into(), schedule_destination as Binding),
        ("when".into(), schedule_when as Binding),
    ]))?;
    Ok(Reference {
        page: "modules/agents/agent/contract/spec/docs/config/specs/guides/schedules.md",
        owner: "link_agent_spec::Schedule",
        fields,
        defaults: json!({}),
        schema: None,
    })
}

fn merge(target: &mut Fields, prefix: &str, fields: Fields) {
    for (path, keys) in fields {
        target.insert(
            if path.is_empty() {
                prefix.to_owned()
            } else {
                format!("{prefix}.{path}")
            },
            keys,
        );
    }
}

/// Read defaults through serde, whose field defaults can differ from Rust Default.
fn parsed<T: DeserializeOwned>(input: &str) -> Result<T, String> {
    toml::from_str(input).map_err(|e| format!("{} default input: {e}", std::any::type_name::<T>()))
}
fn omitted_defaults<T: DeserializeOwned + Serialize>(input: &str) -> Result<Value, String> {
    let mut value = serde_json::to_value(parsed::<T>(input)?).map_err(|e| e.to_string())?;
    let supplied: toml::Value = toml::from_str(input).map_err(|e| e.to_string())?;
    fn remove(value: &mut Value, supplied: &toml::Value) {
        if let (Some(value), Some(supplied)) = (value.as_object_mut(), supplied.as_table()) {
            for (key, supplied) in supplied {
                if supplied.is_table() {
                    if let Some(child) = value.get_mut(key) {
                        remove(child, supplied);
                    }
                } else {
                    value.remove(key);
                }
            }
        }
    }
    remove(&mut value, &supplied);
    Ok(value)
}

fn reference<T: DeserializeOwned + Serialize>(
    page: &'static str,
    owner: &'static str,
    input: &str,
    mut defaults: Value,
    bindings: &[(&str, Binding)],
) -> Result<Reference, String> {
    let mut omitted = omitted_defaults::<T>(input)?;
    let declared = defaults
        .as_object_mut()
        .ok_or("reference defaults must be an object")?;
    if let Some(omitted) = omitted.as_object_mut() {
        omitted.retain(|key, value| declared.get(key) != Some(value));
        if !omitted.is_empty() {
            declared.insert("omitted fields".into(), Value::Object(omitted.clone()));
        }
    }
    let fields = fields::discover::<T>(
        &bindings
            .iter()
            .map(|(key, binding)| ((*key).into(), *binding))
            .collect(),
    )?;
    Ok(Reference {
        page,
        owner,
        fields,
        defaults,
        schema: None,
    })
}

fn installation_identity() -> Result<Reference, String> {
    let mut fields = discover::<link_machine::installation::Installation>()?;
    merge(
        &mut fields,
        "/etc/lnk/kind.toml",
        source::tagged::<link_machine::machine::Kind>(include_str!(
            "../../../../../packages/machine/src/machine.rs"
        ))?,
    );
    Ok(Reference {
        page: "packages/machine/docs/config/installation.md",
        owner: "link_machine::{installation::Installation, machine::Kind}",
        fields,
        defaults: json!({}),
        schema: None,
    })
}

pub fn references() -> Result<Vec<Reference>, String> {
    Ok(vec![
        installation_identity()?,
        schedule_reference()?,
        reference::<link_agent_spec::access::Access>(
            "modules/agents/agent/contract/identity/docs/config/specs/access.md",
            "link_agent_spec::access::Access",
            "",
            {
                let defaults = parsed::<link_agent_spec::access::Access>("")?;
                json!({"message": defaults.message, "member": defaults.member})
            },
            &[],
        )?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seven() -> u32 {
        7
    }
    #[derive(Default, Serialize, serde::Deserialize)]
    struct Defaults {
        #[serde(default = "seven")]
        count: u32,
    }
    #[test]
    fn effective_field_defaults_come_from_serde_instead_of_rust_default() {
        assert_eq!(Defaults::default().count, 0);
        let reference =
            reference::<Defaults>("example.md", "Defaults", "", json!({}), &[]).unwrap();
        assert_eq!(reference.defaults["omitted fields"]["count"], 7);
        assert!(reference.render().contains("\"count\":7"));
    }
}
