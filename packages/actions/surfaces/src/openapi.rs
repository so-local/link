//! The API: one OpenAPI 3.1 document. Each action offered on the API is a
//! request to `/_link/<namespace>/<resource>/<verb>`, whose operation ID
//! is its action ID. How it travels is derived from its effect
//! (`link-actions`' `transport.toml`): its method, and where its inputs
//! go. A find or a delete takes its runtime arguments as query
//! parameters; an insert or an update takes the argument naming what it
//! acts on as one, and every other input, its spec among them, in a typed
//! body. Its output is the answer, or each event of a stream, and any
//! failure is core's error, its status the error's name's. Every shape is
//! a component named `<module>.<name>`.

use anyhow::ensure;
use link_actions::{Action, Catalog, Surface, schema::Node, transport::transport};
use serde_json::{Map, Value, json};

use crate::{
    Files,
    config::SETTINGS,
    shapes::{json_text, rebase},
};

/// Where the document keeps its shapes.
const COMPONENTS: &str = "#/components/schemas/";

/// The document, as one file.
pub fn generate(catalog: &Catalog) -> anyhow::Result<Files> {
    let mut files = Files::new();
    files.insert(&SETTINGS.openapi.file, json_text(document(catalog)?)?)?;
    Ok(files)
}

/// The document as JSON.
pub fn document(catalog: &Catalog) -> anyhow::Result<Value> {
    let settings = &SETTINGS.openapi;
    let mut paths = Map::new();
    let mut tags = Vec::new();
    for module in catalog.modules() {
        if module.actions.values().any(|a| a.offered_on(Surface::Rest)) {
            tags.push(json!({"name": module.identity.id}));
        }
    }
    for action in crate::offered(catalog, Surface::Rest) {
        let method = transport().method(action.effect).method.to_lowercase();
        paths.insert(path(action), json!({ method: operation(catalog, action)? }));
    }
    let mut schemas = Map::new();
    for module in catalog.modules() {
        for (name, declared) in &module.types {
            schemas.insert(
                format!("{}.{name}", module.identity.id),
                rebase(catalog.json(&declared.schema), COMPONENTS),
            );
        }
    }
    Ok(crate::files::stamp(json!({
        "openapi": settings.version,
        "info": {"title": settings.title, "version": settings.api_version},
        "jsonSchemaDialect": link_actions::dialect(),
        "tags": tags,
        "paths": paths,
        "components": {"schemas": schemas},
    })))
}

/// An action's path: `/_link/env/approval/add`.
pub fn path(action: &Action) -> String {
    let id = &action.id;
    format!(
        "{}/{}/{}/{}",
        SETTINGS.openapi.prefix,
        id.namespace(),
        id.resource(),
        id.verb()
    )
}

/// The arguments an action takes in its query: every one for an effect
/// that takes no body, and otherwise only the one naming what it acts on.
pub fn query(action: &Action) -> impl Iterator<Item = (&String, &link_actions::Arg)> {
    let body = transport().method(action.effect).body;
    crate::rest_args(action)
        .filter(move |(name, _)| !body || action.resource.as_deref() == Some(name.as_str()))
}

/// The arguments an action takes in its body, beside its spec: those not
/// in its query.
pub fn body(action: &Action) -> impl Iterator<Item = (&String, &link_actions::Arg)> {
    let body = transport().method(action.effect).body;
    crate::rest_args(action)
        .filter(move |(name, _)| body && action.resource.as_deref() != Some(name.as_str()))
}

/// A stream's tag: the property naming each of its events.
pub fn tag(catalog: &Catalog, action: &Action) -> anyhow::Result<Option<String>> {
    if !action.stream {
        return Ok(None);
    }
    match &catalog.resolve(&action.output)?.node {
        Node::OneOf(choice) => match &choice.told {
            link_actions::schema::Told::Tag(tag) => Ok(Some(tag.clone())),
            _ => anyhow::bail!("{}'s events are told apart by a tag", action.id),
        },
        _ => anyhow::bail!("{}'s events are a union", action.id),
    }
}

fn operation(catalog: &Catalog, action: &Action) -> anyhow::Result<Value> {
    let transport = transport();
    let media = &transport.formats.media[&transport.formats.default];
    let schema = |schema| rebase(catalog.json(schema), COMPONENTS);
    let parameters: Vec<Value> = query(action)
        .map(|(name, arg)| {
            let mut parameter = json!({
                "name": name,
                "in": "query",
                "required": arg.required,
                "schema": schema(&arg.schema),
            });
            if let Some(description) = &arg.schema.meta.description {
                parameter["description"] = json!(description);
            }
            if arg.cli != link_actions::Cli::default() {
                parameter["x-link-cli"] = json!(arg.cli);
            }
            parameter
        })
        .collect();
    let error = json!({
        "description": "A named error: its name gives its status",
        "content": {media.as_str(): {"schema": {"$ref": format!("{COMPONENTS}core.error")}}},
    });
    let answer = match tag(catalog, action)? {
        Some(tag) => json!({
            "description": action.summary(),
            "content": {transport.streams.media.as_str(): {
                "schema": schema(&action.output),
                "x-link-event": tag,
            }},
        }),
        None => {
            let mut answer = json!({
                "description": action.summary(),
                "content": {media.as_str(): {"schema": schema(&action.output)}},
            });
            if let Some(cache) = transport.cache_control(action.effect, action.keeps.as_ref()) {
                answer["headers"] = json!({"Cache-Control": {
                    "schema": {"type": "string", "const": cache},
                }});
            }
            answer
        }
    };
    let mut operation = json!({
        "operationId": action.id.as_str(),
        "summary": action.summary(),
        "description": action.description,
        "tags": [action.id.namespace()],
        "x-link-command": action.id.command(),
        "x-link-role": action.role,
        "x-link-effect": action.effect,
        "responses": {"200": answer, "4XX": error, "5XX": error},
    });
    if !parameters.is_empty() {
        operation["parameters"] = Value::Array(parameters);
    }
    // The body: its spec, alone where it takes no other input, as an
    // effect without a body still does until the API takes only what it
    // accepts; else a record of its other inputs and its spec.
    let others: Vec<_> = body(action).collect();
    let spec = action
        .spec
        .as_ref()
        .map(|spec| json!({"$ref": format!("{COMPONENTS}{spec}")}));
    let content = match (others.is_empty(), spec) {
        (true, None) => None,
        (true, Some(spec)) => Some(spec),
        (false, spec) => {
            let mut properties = Map::new();
            let mut required = Vec::new();
            for (name, arg) in others {
                properties.insert(name.clone(), schema(&arg.schema));
                if arg.required {
                    required.push(json!(name));
                }
            }
            if let Some(spec) = spec {
                let key = &SETTINGS.tools.spec;
                ensure!(
                    !properties.contains_key(key),
                    "{}: an argument named {key} would be its spec's place in the body",
                    action.id
                );
                properties.insert(key.clone(), spec);
                required.push(json!(key));
            }
            Some(json!({
                "type": "object",
                "additionalProperties": false,
                "required": required,
                "properties": properties,
            }))
        }
    };
    if let Some(content) = content {
        operation["requestBody"] = json!({
            "required": true,
            "content": {media.as_str(): {"schema": content}},
        });
    }
    Ok(operation)
}
