//! Command-backed tools, projected from the component's action declaration.
use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Command, OptionDeclaration, ValueType, actions::Limits};

/// The execution and approval metadata retained in an installed descriptor.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandTool {
    pub read_only: bool,
    pub json: bool,
    pub limits: Limits,
}

/// One explicitly exposed command, with its declaration's argument shapes.
#[derive(Clone, Debug)]
pub struct Tool {
    pub path: String,
    pub command: Command,
}

impl Tool {
    pub fn name(&self) -> String {
        self.path.replace(' ', link_spec::follow::TOOL_SEPARATOR)
    }

    /// Transport and approval flags belong to the caller, never the model.
    fn inputs(&self, agent: bool) -> impl Iterator<Item = (&String, &OptionDeclaration)> {
        self.command.options.iter().filter(move |(name, arg)| {
            !arg.derived
                && !matches!(arg.long.as_deref(), Some("json" | "request" | "yes"))
                && !matches!(name.as_str(), "json" | "request" | "yes")
                && !(agent && name.as_str() == "agent")
        })
    }

    pub fn offered(&self, agent: bool) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for (name, arg) in self.inputs(agent) {
            let mut schema = json!({"type": match arg.r#type {
                ValueType::String => "string", ValueType::Boolean => "boolean", ValueType::Integer => "integer"
            }});
            if let Some(values) = &arg.values {
                schema["enum"] = serde_json::to_value(values).expect("argument values serialize");
            }
            if arg.optional_value {
                schema = json!({"anyOf":[schema,{"type":"null"}]});
            }
            if arg.multiple {
                schema = json!({"type":"array", "items":schema});
                if arg.required {
                    schema["minItems"] = json!(1);
                }
            }
            if let Some(help) = self.command.flags.get(&crate::actions::spelling(name, arg)) {
                schema["description"] = json!(help);
            }
            if arg.required {
                required.push(name);
            }
            properties.insert(name.clone(), schema);
        }
        let policy = self
            .command
            .tool
            .as_ref()
            .expect("a tool has execution metadata");
        json!({"name":self.name(), "description":self.command.help,
            "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
            "annotations":{"readOnlyHint":policy.read_only,"destructiveHint":!policy.read_only,"openWorldHint":true}})
    }

    /// Validate every value before producing argv. No shell interprets these strings.
    pub fn arguments(&self, input: &Value, agent: Option<&str>) -> anyhow::Result<Vec<String>> {
        let input = input.as_object().context("arguments must be an object")?;
        let allowed: BTreeMap<_, _> = self.inputs(agent.is_some()).collect();
        ensure!(
            input.keys().all(|name| allowed.contains_key(name)),
            "unknown or caller-owned argument"
        );
        let mut flags = Vec::new();
        let mut positionals = BTreeMap::new();
        for (name, arg) in allowed {
            let Some(value) = input.get(name) else {
                ensure!(!arg.required, "missing argument {name}");
                continue;
            };
            if arg.optional_value && value.is_null() {
                flags.push(format!(
                    "--{}",
                    arg.long.as_ref().context("optional values need a flag")?
                ));
                continue;
            }
            let values = if arg.multiple {
                value
                    .as_array()
                    .context("repeatable argument must be an array")?
                    .iter()
                    .collect::<Vec<_>>()
            } else {
                vec![value]
            };
            ensure!(
                !arg.required || !values.is_empty(),
                "empty required argument {name}"
            );
            let mut words = Vec::new();
            for value in values {
                let valid = match arg.r#type {
                    ValueType::String => value.is_string(),
                    ValueType::Boolean => value.is_boolean(),
                    ValueType::Integer => value.as_i64().is_some(),
                };
                ensure!(valid, "invalid type for {name}");
                if let Some(choices) = &arg.values {
                    ensure!(choices.iter().any(|choice| serde_json::to_value(choice).ok().as_ref() == Some(value)), "invalid choice for {name}");
                }
                let text = value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string());
                ensure!(!text.contains('\0'), "null byte in {name}");
                if arg.position.is_some() {
                    words.push(text);
                } else {
                    let flag = arg.long.as_ref().context("tool flags need a long name")?;
                    match arg.r#type {
                        ValueType::Boolean => {
                            if value == true {
                                words.push(format!("--{flag}"));
                            }
                        }
                        _ => words.push(format!("--{flag}={text}")),
                    }
                }
            }
            if let Some(position) = arg.position {
                positionals.insert(position, words);
            } else {
                flags.extend(words);
            }
        }
        let mut args: Vec<String> = self.path.split(' ').map(str::to_string).collect();
        if let Some(agent) = agent
            && self.command.options.contains_key("agent")
        {
            args.push(format!("--agent={agent}"));
        }
        if self.command.tool.as_ref().context("not a tool")?.json {
            flags.push("--json".into());
        }
        args.extend(flags);
        // End option parsing before any model-supplied positional, including '-...'.
        if !positionals.is_empty() {
            let positions: Vec<_> = positionals.keys().copied().collect();
            ensure!(
                positions.iter().copied().eq(1..=positions.len()),
                "positional arguments must be contiguous"
            );
            args.push("--".into());
            args.extend(positionals.into_values().flatten());
        }
        Ok(args)
    }
}

/// Explicit tool exposure survives installation. No hidden command is offered.
pub fn commands(commands: &BTreeMap<String, Command>) -> Vec<Tool> {
    commands
        .iter()
        .filter(|(_, command)| !command.hidden && command.tool.is_some())
        .map(|(path, command)| Tool {
            path: path.clone(),
            command: command.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
