//! The app's TypeScript: a type for every shape and for each API action's
//! arguments and output, and a typed client whose methods follow the
//! action IDs, `client.env.approval.add(args)`. Each call carries what its
//! effect derives, its method and where each input goes, and a streaming
//! action's method yields its events. The client sends each call through
//! a transport the app gives it, so it holds no behavior of its own.

use std::{collections::BTreeMap, fmt::Write as _};

use anyhow::{Context, bail};
use link_actions::{
    Action, Catalog, Surface, TypeName,
    schema::{Meta, Node, Schema},
};

use crate::{Files, config::SETTINGS, shapes::pascal};

/// `types.ts` and `client.ts`.
pub fn generate(catalog: &Catalog) -> anyhow::Result<Files> {
    let names = Names::of(catalog)?;
    let mut files = Files::new();
    files.insert(&SETTINGS.typescript.types, types(catalog, &names)?)?;
    files.insert(&SETTINGS.typescript.client, client(catalog, &names)?)?;
    Ok(files)
}

/// Every TypeScript name the files declare, each from what it names, and
/// refused when two would read alike.
struct Names {
    shapes: BTreeMap<TypeName, String>,
}

impl Names {
    fn of(catalog: &Catalog) -> anyhow::Result<Self> {
        let mut shapes = BTreeMap::new();
        let mut taken: BTreeMap<String, String> = BTreeMap::new();
        let mut take = |ts: String, what: String| -> anyhow::Result<String> {
            if let Some(other) = taken.insert(ts.clone(), what.clone()) {
                bail!("{what} and {other} would both be TypeScript's {ts}");
            }
            Ok(ts)
        };
        for module in catalog.modules() {
            for name in module.types.keys() {
                let name = TypeName::new(&module.identity.id, name)?;
                let ts = take(pascal(&name.to_string()), format!("shape {name}"))?;
                shapes.insert(name, ts);
            }
        }
        for action in crate::offered(catalog, Surface::Rest) {
            for suffix in ["Args", "Output"] {
                take(
                    format!("{}{suffix}", pascal(action.id.as_str())),
                    format!("action {}'s {suffix}", action.id),
                )?;
            }
        }
        Ok(Self { shapes })
    }

    fn shape(&self, name: &TypeName) -> anyhow::Result<&str> {
        self.shapes
            .get(name)
            .map(String::as_str)
            .with_context(|| format!("{name} isn't declared"))
    }
}

fn header() -> String {
    format!("// {}\n", crate::marker())
}

fn types(catalog: &Catalog, names: &Names) -> anyhow::Result<String> {
    let mut out = header();
    for module in catalog.modules() {
        for (name, declared) in &module.types {
            let name = TypeName::new(&module.identity.id, name)?;
            out.push('\n');
            doc(&mut out, &declared.schema.meta, "");
            let ts = names.shape(&name)?;
            writeln!(
                out,
                "export type {ts} = {};",
                ty(&declared.schema, names, "")?
            )?;
        }
    }
    for action in crate::offered(catalog, Surface::Rest) {
        let base = pascal(action.id.as_str());
        out.push('\n');
        writeln!(out, "/** `{}`'s runtime arguments. */", action.id)?;
        writeln!(out, "export type {base}Args = {};", args(action, names)?)?;
        writeln!(out, "/** What `{}` answers. */", action.id)?;
        writeln!(
            out,
            "export type {base}Output = {};",
            ty(&action.output, names, "")?
        )?;
    }
    if !out.contains("export ") {
        // A file with no declaration is still a module others import.
        out.push_str("\nexport {};\n");
    }
    Ok(out)
}

fn args(action: &Action, names: &Names) -> anyhow::Result<String> {
    let mut fields = Vec::new();
    for (name, arg) in crate::rest_args(action) {
        let optional = if arg.required { "" } else { "?" };
        fields.push(format!(
            "  {}{optional}: {};",
            key(name),
            ty(&arg.schema, names, "  ")?
        ));
    }
    Ok(if fields.is_empty() {
        "Record<string, never>".into()
    } else {
        format!("{{\n{}\n}}", fields.join("\n"))
    })
}

/// A shape as a TypeScript type, `indent` the depth it is written at.
fn ty(schema: &Schema, names: &Names, indent: &str) -> anyhow::Result<String> {
    Ok(match &schema.node {
        Node::Named(name) => names.shape(name)?.to_string(),
        Node::Record(record) => {
            // An open record: its own keys, and any other of one shape.
            let others = match &record.others {
                Some(others) => format!(" & {{ [key: string]: {} }}", ty(others, names, indent)?),
                None => String::new(),
            };
            if record.properties.is_empty() {
                return Ok(match others.is_empty() {
                    true => "Record<string, never>".into(),
                    false => others.trim_start_matches(" & ").to_string(),
                });
            }
            let inner = format!("{indent}  ");
            let mut out = String::from("{\n");
            for (name, property) in &record.properties {
                doc(&mut out, &property.meta, &inner);
                let optional = if record.required.contains(name) {
                    ""
                } else {
                    "?"
                };
                writeln!(
                    out,
                    "{inner}{}{optional}: {};",
                    key(name),
                    ty(property, names, &inner)?
                )?;
            }
            out.push_str(indent);
            out.push('}');
            out.push_str(&others);
            out
        }
        Node::Map(map) => format!("{{ [key: string]: {} }}", ty(&map.values, names, indent)?),
        Node::List(list) => format!("Array<{}>", ty(&list.items, names, indent)?),
        Node::Enum(values) => values
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join(" | "),
        Node::Const(value) => serde_json::to_string(value)?,
        Node::OneOf(one) => one
            .variants
            .iter()
            .map(|v| ty(v, names, indent))
            .collect::<anyhow::Result<Vec<_>>>()?
            .join(" | "),
        Node::String(_) => "string".into(),
        Node::Integer(_) | Node::Number(_) => "number".into(),
        Node::Boolean => "boolean".into(),
        Node::Any => "unknown".into(),
    })
}

/// A property's name, quoted as JSON writes it, which TypeScript reads.
fn key(name: &str) -> String {
    serde_json::to_string(name).expect("a string")
}

fn doc(out: &mut String, meta: &Meta, indent: &str) {
    if let Some(description) = &meta.description {
        let text = description.trim().replace("*/", "*\\/");
        let _ = writeln!(out, "{indent}/** {} */", text.replace('\n', " "));
    }
}

fn client(catalog: &Catalog, names: &Names) -> anyhow::Result<String> {
    let transport = link_actions::transport::transport();
    let mut out = header();
    out.push_str(
        "import type * as T from \"./types.ts\";\n\n\
         /** One call, as its action's effect sends it: its method and path, its\n \
         * query parameters, its body where its effect takes one, and the tag\n \
         * naming each event where it streams. */\n\
         export type Call = {\n  action: string;\n  path: string;\n  method: \"GET\" | \"POST\" | \"PUT\" | \"DELETE\";\n  \
         query: Record<string, unknown>;\n  body?: unknown;\n  stream?: string;\n};\n\n\
         /** Sends a call: `send` answers its output, and `stream` yields each\n \
         * event of a streaming one, in order. */\n\
         export type Transport = {\n  send: (call: Call) => Promise<unknown>;\n  \
         stream: (call: Call) => AsyncIterable<unknown>;\n};\n\n",
    );
    writeln!(
        out,
        "/** Each named error's HTTP status, by its name. */\nexport const statuses = {{"
    )?;
    for (name, status) in &transport.status {
        writeln!(out, "  {}: {status},", key(name))?;
    }
    out.push_str(
        "} as const;\n\n/** A named error. */\nexport type ErrorName = keyof typeof statuses;\n\n",
    );
    out.push_str(
        "/** Every API action, by its ID's words, each sent through `transport`. */\n\
         export function client(transport: Transport) {\n  return ",
    );
    let mut tree: BTreeMap<&str, BTreeMap<&str, Vec<&Action>>> = BTreeMap::new();
    for action in crate::offered(catalog, Surface::Rest) {
        tree.entry(action.id.namespace())
            .or_default()
            .entry(action.id.resource())
            .or_default()
            .push(action);
    }
    if tree.is_empty() {
        out.push_str("{};\n}\n");
        return Ok(out);
    }
    out.push_str("{\n");
    for (namespace, resources) in tree {
        writeln!(out, "    {}: {{", key(namespace))?;
        for (resource, actions) in resources {
            writeln!(out, "      {}: {{", key(resource))?;
            for action in actions {
                method(&mut out, catalog, names, action)?;
            }
            out.push_str("      },\n");
        }
        out.push_str("    },\n");
    }
    out.push_str("  };\n}\n");
    Ok(out)
}

/// One action's method: its arguments and spec split into its query and
/// body as its effect says, sent, or for a stream, its events yielded.
fn method(
    out: &mut String,
    catalog: &Catalog,
    names: &Names,
    action: &Action,
) -> anyhow::Result<()> {
    let base = pascal(action.id.as_str());
    let optional = !crate::rest_args(action).any(|(_, arg)| arg.required);
    let args = if optional {
        format!("args: T.{base}Args = {{}}")
    } else {
        format!("args: T.{base}Args")
    };
    let spec_param = match &action.spec {
        Some(spec) => format!(", spec: T.{}", names.shape(spec)?),
        None => String::new(),
    };
    let pick = |names: Vec<&String>| -> String {
        names
            .iter()
            .map(|name| format!("{}: args[{}]", key(name), key(name)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let query = match pick(
        crate::openapi::query(action)
            .map(|(name, _)| name)
            .collect(),
    ) {
        fields if fields.is_empty() => "{}".to_string(),
        fields => format!("{{ {fields} }}"),
    };
    let others: Vec<&String> = crate::openapi::body(action).map(|(name, _)| name).collect();
    // A body: its other inputs and its spec, or its spec alone, as the
    // OpenAPI document declares it.
    let body = match (others.is_empty(), action.spec.is_some()) {
        (true, false) => None,
        (true, true) => Some("spec".to_string()),
        (false, spec) => {
            let mut fields = pick(others);
            if spec {
                fields.push_str(&format!(
                    ", {}: spec",
                    key(&crate::config::SETTINGS.tools.spec)
                ));
            }
            Some(format!("{{ {fields} }}"))
        }
    };
    let http = &link_actions::transport::transport()
        .method(action.effect)
        .method;
    let mut call = format!(
        "{{ action: {}, path: {}, method: {}, query: {query}",
        key(action.id.as_str()),
        key(&crate::openapi::path(action)),
        key(http),
    );
    if let Some(body) = body {
        call.push_str(&format!(", body: {body}"));
    }
    let stream = crate::openapi::tag(catalog, action)?;
    if let Some(tag) = &stream {
        call.push_str(&format!(", stream: {}", key(tag)));
    }
    call.push_str(" }");
    writeln!(
        out,
        "        /** {} */",
        action.summary().replace("*/", "*\\/")
    )?;
    match stream {
        Some(_) => {
            writeln!(
                out,
                "        {}: ({args}{spec_param}): AsyncIterable<T.{base}Output> =>",
                key(action.id.verb())
            )?;
            writeln!(
                out,
                "          transport.stream({call}) as AsyncIterable<T.{base}Output>,"
            )?;
        }
        None => {
            writeln!(
                out,
                "        {}: ({args}{spec_param}): Promise<T.{base}Output> =>",
                key(action.id.verb())
            )?;
            writeln!(
                out,
                "          transport.send({call}) as Promise<T.{base}Output>,"
            )?;
        }
    }
    Ok(())
}
