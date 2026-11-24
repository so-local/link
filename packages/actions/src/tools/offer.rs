//! A module's tool calls as its program offers them: the template its
//! build renders from its declarations, filled with what the program
//! gives at runtime.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A module's tool calls as its build renders them: every `{fill}` and
/// filled choice still to give, each call's condition beside it. Built
/// into `OUT_DIR` with the program that reads it, so it names no format
/// number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// Each fill `[tools]` declares, by how its declarations use it.
    pub fills: BTreeMap<String, Uses>,
    /// The conditions `[tools]` declares.
    pub conditions: Vec<String>,
    pub tools: Vec<TemplateTool>,
}

/// How a fill is used, and so what the program gives for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Uses {
    /// Text, in a description: [`Fill::Text`].
    Text,
    /// An argument's choices: [`Fill::Words`].
    Words,
}

/// One call as its build renders it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateTool {
    pub name: String,
    pub description: String,
    /// Its arguments' JSON Schema, no `x-link-*` keyword in it.
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    pub annotations: Value,
    /// What it does to state, its action's effect.
    pub effect: crate::Effect,
    /// The arguments its title shows, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<String>,
    /// The condition it's offered on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// Each argument whose choices a fill gives, by the fill's name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chosen: BTreeMap<String, String>,
    /// Its boolean arguments whose call asks you first unless given false
    /// (`x-link-asks`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asks: Vec<String>,
}

/// What the program gives for one fill.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// The text a `{name}` in a description stands for.
    Text(String),
    /// An argument's choices, at least one.
    Words(Vec<String>),
    /// An argument whose choices are left open: any value, its schema
    /// naming none.
    Any,
}

/// What the program gives at runtime: each declared fill, and the
/// conditions that hold now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fills<'a> {
    pub values: BTreeMap<&'a str, Fill>,
    pub conditions: BTreeSet<&'a str>,
}

impl Template {
    /// The template a build rendered.
    pub fn parse(template: &str) -> anyhow::Result<Self> {
        serde_json::from_str(template).context("the tools template")
    }

    /// The calls offered while `conditions` hold: each with no condition,
    /// and each whose condition is among them. What's listed, what's
    /// checked for fills and what's filled are all these.
    pub fn offered<'a>(
        &'a self,
        conditions: &'a BTreeSet<&str>,
    ) -> impl Iterator<Item = &'a TemplateTool> {
        self.tools.iter().filter(|tool| {
            tool.when
                .as_deref()
                .is_none_or(|when| conditions.contains(when))
        })
    }

    /// Every call it declares, by the name a model calls it, whatever its
    /// condition: the one list of a tool's calls.
    pub fn calls(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.name.clone()).collect()
    }

    /// Each call's argument that asks you first unless given false
    /// (`x-link-asks`), as `<call>.<argument>`: what a sandbox asks by
    /// (`lnk sandbox tool add --ask-if`).
    pub fn asks(&self) -> Vec<String> {
        self.tools
            .iter()
            .flat_map(|tool| {
                tool.asks
                    .iter()
                    .map(move |arg| format!("{}.{arg}", tool.name))
            })
            .collect()
    }

    /// The calls that only read: those whose effect is `find`.
    pub fn reads(&self) -> Vec<String> {
        self.tools
            .iter()
            .filter(|tool| tool.effect.reads_only())
            .map(|tool| tool.name.clone())
            .collect()
    }
}

/// The tool calls `template` holds as MCP's `tools/list` gives them,
/// filled with `fills`: a call whose condition doesn't hold left out, and
/// a fill only such calls name may be left out with it. A
/// `{name}` is filled in the call's description and its top-level
/// argument descriptions alone, the texts its declarations' check reads;
/// a nested argument's description is never filled.
/// Refused: a fill declared and not given, given and not declared, or
/// given as the other kind; a condition not declared; no choice.
pub fn offer(template: &str, fills: &Fills) -> anyhow::Result<Value> {
    let template = Template::parse(template)?;
    // The fills the calls offered now name: one only a call left out
    // names may be left out too.
    let mut needed: BTreeSet<String> = BTreeSet::new();
    for tool in template.offered(&fills.conditions) {
        needed.extend(tool.chosen.values().cloned());
        let mut texts = vec![tool.description.clone()];
        if let Some(properties) = tool.input_schema["properties"].as_object() {
            texts.extend(
                properties
                    .values()
                    .filter_map(|p| p["description"].as_str().map(str::to_string)),
            );
        }
        for text in &texts {
            needed.extend(super::placeholders(text)?.into_iter().map(str::to_string));
        }
    }
    for (name, uses) in &template.fills {
        match (fills.values.get(name.as_str()), uses) {
            (None, _) if !needed.contains(name) => {}
            (None, _) => bail!("fill {name} is declared and not given"),
            (Some(Fill::Text(_)), Uses::Text) => {}
            (Some(Fill::Words(words)), Uses::Words) => {
                ensure!(!words.is_empty(), "fill {name} gives at least one choice");
            }
            (Some(Fill::Any), Uses::Words) => {}
            (Some(_), Uses::Text) => bail!("fill {name} is a description's text"),
            (Some(_), Uses::Words) => bail!("fill {name} is an argument's choices"),
        }
    }
    if let Some(name) = fills
        .values
        .keys()
        .find(|name| !template.fills.contains_key(**name))
    {
        bail!("fill {name} is given and [tools] declares no such fill");
    }
    if let Some(name) = fills
        .conditions
        .iter()
        .find(|name| !template.conditions.iter().any(|c| c == *name))
    {
        bail!("condition {name} is given and [tools] declares no such condition");
    }
    let text = |text: &str| -> String {
        let mut out = text.to_string();
        for (name, fill) in &fills.values {
            if let Fill::Text(value) = fill {
                out = out.replace(&format!("{{{name}}}"), value);
            }
        }
        out
    };
    let mut offered = Vec::new();
    for tool in template.offered(&fills.conditions) {
        let mut input = tool.input_schema.clone();
        if let Some(properties) = input["properties"].as_object_mut() {
            for (arg, schema) in properties.iter_mut() {
                if let Some(said) = schema["description"].as_str() {
                    schema["description"] = json!(text(said));
                }
                if let Some(fill) = tool.chosen.get(arg)
                    && let Some(Fill::Words(words)) = fills.values.get(fill.as_str())
                {
                    schema["enum"] = json!(words);
                }
            }
        }
        let mut call = json!({
            "name": &tool.name,
            "description": text(&tool.description),
            "inputSchema": input,
            "annotations": &tool.annotations,
        });
        if !tool.summary.is_empty() {
            call["_meta"] = json!({ super::SUMMARY_META: &tool.summary });
        }
        offered.push(call);
    }
    Ok(Value::Array(offered))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template() -> String {
        serde_json::to_string(&Template {
            fills: [
                ("programs".to_string(), Uses::Words),
                ("timeout".to_string(), Uses::Text),
            ]
            .into(),
            conditions: vec!["background".into()],
            tools: vec![
                TemplateTool {
                    name: "command".into(),
                    description: "Runs a program, stopped after {timeout}.".into(),
                    input_schema: json!({"type": "object", "properties": {
                        "program": {"type": "string", "description": "One of them."},
                        "args": {"type": "array", "items": {"type": "string"}},
                    }}),
                    annotations: json!({"readOnlyHint": false, "openWorldHint": false}),
                    effect: crate::Effect::Insert,
                    summary: vec!["program".into(), "args".into()],
                    when: None,
                    chosen: [("program".to_string(), "programs".to_string())].into(),
                    asks: Vec::new(),
                },
                TemplateTool {
                    name: "start".into(),
                    description: "Starts one in the background.".into(),
                    input_schema: json!({"type": "object", "properties": {}}),
                    annotations: json!({"readOnlyHint": true}),
                    effect: crate::Effect::Find,
                    summary: Vec::new(),
                    when: Some("background".into()),
                    chosen: BTreeMap::new(),
                    asks: vec!["recurring".into()],
                },
            ],
        })
        .unwrap()
    }

    fn fills<'a>(conditions: &[&'a str]) -> Fills<'a> {
        Fills {
            values: [
                ("programs", Fill::Words(vec!["git".into(), "cargo".into()])),
                ("timeout", Fill::Text("108s".into())),
            ]
            .into(),
            conditions: conditions.iter().copied().collect(),
        }
    }

    /// Every declared call, whatever its condition; those offered while
    /// conditions hold; and those whose effect only reads.
    #[test]
    fn a_templates_calls_are_listed() {
        let template = Template::parse(&template()).unwrap();
        assert_eq!(template.calls(), ["command", "start"]);
        let offered = |conditions: &BTreeSet<&str>| -> Vec<String> {
            template
                .offered(conditions)
                .map(|t| t.name.clone())
                .collect()
        };
        assert_eq!(offered(&BTreeSet::new()), ["command"]);
        assert_eq!(offered(&["background"].into()), ["command", "start"]);
        assert_eq!(template.reads(), ["start"]);
        // Each argument that asks unless given false, as a sandbox takes it.
        assert_eq!(template.asks(), ["start.recurring"]);
    }

    /// Each fill takes its place, the summary goes in `_meta`, and a call
    /// whose condition doesn't hold is left out.
    #[test]
    fn a_template_is_filled_as_the_program_says() {
        let offered = offer(&template(), &fills(&[])).unwrap();
        assert_eq!(offered.as_array().unwrap().len(), 1);
        let command = &offered[0];
        assert_eq!(
            command["description"],
            "Runs a program, stopped after 108s."
        );
        assert_eq!(
            command["inputSchema"]["properties"]["program"]["enum"],
            json!(["git", "cargo"])
        );
        assert_eq!(
            command["_meta"][super::super::SUMMARY_META],
            json!(["program", "args"])
        );
        // What only the template holds never reaches a model.
        for key in ["effect", "when", "summary", "chosen"] {
            assert!(command.get(key).is_none(), "{key} in {command}");
        }
        let both = offer(&template(), &fills(&["background"])).unwrap();
        assert_eq!(both[1]["name"], "start");
        assert!(both[1].get("_meta").is_none());
        for key in ["effect", "when", "summary", "chosen"] {
            assert!(both[1].get(key).is_none(), "{key} in {}", both[1]);
        }
    }

    /// A choices fill left open names no choices: its argument takes any
    /// value.
    #[test]
    fn an_open_fill_names_no_choices() {
        let mut open = fills(&[]);
        open.values.insert("programs", Fill::Any);
        let offered = offer(&template(), &open).unwrap();
        let program = &offered[0]["inputSchema"]["properties"]["program"];
        assert!(program.get("enum").is_none(), "{program}");
    }

    /// A fill only a call whose condition is off names may be left out,
    /// that call with it; once its condition holds, the fill is needed.
    #[test]
    fn a_fill_only_an_off_call_names_may_be_left_out() {
        // `start`, offered only in the background, alone names `most`.
        let mut template = Template::parse(&template()).unwrap();
        template.fills.insert("most".into(), Uses::Text);
        template.tools[1].description = "Starts one, at most {most}.".into();
        let template = serde_json::to_string(&template).unwrap();
        let off = offer(&template, &fills(&[])).unwrap();
        let names: Vec<&str> = off
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["command"]);
        let refused = offer(&template, &fills(&["background"])).unwrap_err();
        assert!(
            format!("{refused:#}").contains("fill most is declared and not given"),
            "{refused:#}"
        );
    }

    /// What the declarations and the program say must agree.
    #[test]
    fn a_fill_or_condition_out_of_place_is_refused() {
        let mut missing = fills(&[]);
        missing.values.remove("timeout");
        assert!(offer(&template(), &missing).is_err());
        let mut extra = fills(&[]);
        extra.values.insert("most", Fill::Text("4".into()));
        assert!(offer(&template(), &extra).is_err());
        let mut kind = fills(&[]);
        kind.values
            .insert("timeout", Fill::Words(vec!["1s".into()]));
        assert!(offer(&template(), &kind).is_err());
        let mut none = fills(&[]);
        none.values.insert("programs", Fill::Words(Vec::new()));
        assert!(offer(&template(), &none).is_err());
        assert!(offer(&template(), &fills(&["teammates"])).is_err());
    }
}
