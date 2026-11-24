//! Agents' tools: one per action offered as a tool, its name the action's
//! ID with its words joined by `_`, its description the action's, and its
//! input one JSON Schema object holding the action's runtime arguments
//! and, under `spec`, its spec, with no `x-link-*` keyword left in. An
//! action with a `tool` table is its program's own call, which only its
//! program offers, from its [`template`], so `tools.json` leaves it out.

use anyhow::{Context, bail, ensure};
use link_actions::tools::wire_name;
use link_actions::{Action, Catalog, Surface};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{Files, config::SETTINGS};

/// One tool, as a model's tool definition takes it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Every tool, as one file: `tools.json`.
pub fn generate(catalog: &Catalog) -> anyhow::Result<Files> {
    let document = crate::files::stamp(json!({"tools": tools(catalog)?}));
    let mut files = Files::new();
    files.insert(&SETTINGS.tools.file, crate::shapes::json_text(document)?)?;
    Ok(files)
}

/// Every action offered as a tool, in ID order, but a program's own calls
/// (those with a `tool` table), which its [`template`] offers.
pub fn tools(catalog: &Catalog) -> anyhow::Result<Vec<Tool>> {
    crate::offered(catalog, Surface::Tool)
        .filter(|action| action.tool.is_none())
        .map(|action| tool(catalog, action))
        .collect()
}

/// A tool's name: `env_approval_add`.
pub fn name(action: &Action) -> String {
    let id = &action.id;
    [id.namespace(), id.resource(), id.verb()].join(&SETTINGS.tools.separator)
}

fn tool(catalog: &Catalog, action: &Action) -> anyhow::Result<Tool> {
    let name = name(action);
    ensure!(
        name.len() <= SETTINGS.tools.max_name,
        "{}: its tool's name {name} is longer than {} characters",
        action.id,
        SETTINGS.tools.max_name
    );
    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut reached = Vec::new();
    for (arg, declared) in crate::tool_args(action) {
        properties.insert(arg.clone(), catalog.json(&declared.schema));
        reached.push(&declared.schema);
        if declared.required {
            required.push(arg.clone());
        }
    }
    let spec_schema;
    if let Some(spec) = &action.spec {
        let key = &SETTINGS.tools.spec;
        if properties.contains_key(key) {
            bail!(
                "{}: an argument is named {key}, where its tool holds its spec",
                action.id
            );
        }
        spec_schema =
            link_actions::schema::Schema::new(link_actions::schema::Node::Named(spec.clone()));
        properties.insert(key.clone(), catalog.json(&spec_schema));
        reached.push(&spec_schema);
        required.push(key.clone());
    }
    let mut defs = std::collections::BTreeMap::new();
    for schema in reached {
        defs.extend(catalog.reached(schema)?);
    }
    let mut input = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        input["required"] = json!(required);
    }
    if !defs.is_empty() {
        input["$defs"] = Value::Object(defs.into_iter().collect());
    }
    strip(&mut input);
    Ok(Tool {
        name,
        description: action.description.clone(),
        input_schema: input,
    })
}

/// Module `module`'s tool actions, each with its `tool` table, as the
/// template its program fills at runtime ([`link_actions::tools::offer`]):
/// its arguments' JSON Schema, as [`tools`] gives it; the arguments its title shows; the
/// choices and condition the program gives.
pub fn template(catalog: &Catalog, module: &str) -> anyhow::Result<link_actions::tools::Template> {
    use link_actions::tools::{Template, TemplateTool, Uses};
    let declared = catalog
        .module(module)
        .with_context(|| format!("module {module} isn't loaded"))?;
    let derived: std::collections::BTreeSet<String> =
        tools(catalog)?.into_iter().map(|tool| tool.name).collect();
    let mut fills = std::collections::BTreeMap::new();
    let mut tools = Vec::new();
    for action in declared.actions.values() {
        let Some(table) = &action.tool else { continue };
        ensure!(
            !derived.contains(&table.name),
            "{}: its tool name {} is another action's tool name",
            action.id,
            table.name
        );
        let input = wired(tool(catalog, action)?.input_schema);
        let mut summary: Vec<(u32, &String)> = Vec::new();
        let mut chosen = std::collections::BTreeMap::new();
        let mut asks = Vec::new();
        for (arg, declared) in &action.args {
            if declared.schema.meta.asks {
                ensure!(
                    matches!(declared.schema.node, link_actions::schema::Node::Boolean),
                    "{}: x-link-asks is a boolean argument's, not {arg}'s",
                    action.id
                );
                asks.push(wire_name(arg));
            }
            if let Some(place) = declared.schema.meta.summary {
                summary.push((place, arg));
            }
            if let Some(fill) = &declared.schema.meta.fill {
                chosen.insert(wire_name(arg), fill.clone());
                fills.insert(fill.clone(), Uses::Words);
            }
        }
        summary.sort();
        let mut texts = vec![action.description.as_str()];
        texts.extend(
            action
                .args
                .values()
                .filter_map(|a| a.schema.meta.description.as_deref()),
        );
        for text in texts {
            for fill in link_actions::tools::placeholders(text)? {
                fills.insert(fill.to_string(), Uses::Text);
            }
        }
        tools.push(TemplateTool {
            name: table.name.clone(),
            description: action.description.clone(),
            input_schema: input,
            annotations: annotations(action.effect, table),
            effect: action.effect,
            summary: summary.into_iter().map(|(_, arg)| wire_name(arg)).collect(),
            when: table.when.clone(),
            chosen,
            asks,
        });
    }
    // By the name a model calls each, whatever its action's ID.
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Template {
        fills,
        conditions: declared.tools.conditions.clone(),
        tools,
    })
}

/// A program's own call's input, its arguments named as a model calls
/// them ([`wire_name`]): what its handler reads, and its `--request`.
fn wired(mut input: Value) -> Value {
    if let Some(properties) = input["properties"].as_object_mut() {
        *properties = std::mem::take(properties)
            .into_iter()
            .map(|(arg, schema)| (wire_name(&arg), schema))
            .collect();
    }
    // `get_mut`, never `input["required"]`: indexing to change a missing
    // key writes it as null, and MCP clients refuse `"required": null`.
    if let Some(required) = input.get_mut("required").and_then(Value::as_array_mut) {
        for arg in required.iter_mut() {
            if let Some(name) = arg.as_str() {
                *arg = json!(wire_name(name));
            }
        }
    }
    input
}

/// A call's MCP annotations: the hints its effect gives, whether it only
/// reads, and where it doesn't, whether it may destroy and whether calling
/// it again changes more, and whether it reaches beyond the machine where
/// its `tool` table says, MCP's default holding where it doesn't.
fn annotations(effect: link_actions::Effect, table: &link_actions::Tool) -> Value {
    let mut hints = Map::new();
    for (hint, declared) in [
        ("readOnlyHint", Some(effect.reads_only())),
        ("destructiveHint", effect.destructive()),
        ("idempotentHint", effect.idempotent()),
        ("openWorldHint", table.open_world),
    ] {
        if let Some(value) = declared {
            hints.insert(hint.into(), json!(value));
        }
    }
    Value::Object(hints)
}

/// `schema` with every `x-link-*` keyword taken out, at any depth.
fn strip(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            map.retain(|key, _| !key.starts_with("x-link-"));
            map.values_mut().for_each(strip);
        }
        Value::Array(items) => items.iter_mut().for_each(strip),
        _ => {}
    }
}
