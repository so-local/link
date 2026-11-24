//! A Rust client for the API, for programs that act as the app does, such
//! as simulated customers and tests: a type for every shape an API action
//! reaches, and for each action its arguments and output and a function
//! building its request. A request carries what its effect derives, its
//! method, its query and its body, and reads its answer, or each event of
//! a stream, as its declared output. Sending it is the caller's, so the
//! client holds no transport of its own. Beside it, for a server, a
//! handler trait with a method per action, which a server implements for
//! the actions it serves, and the dispatch reading a request's inputs into
//! each action's arguments and calling its method.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use anyhow::{Context, bail};
use link_actions::{
    Action, Catalog, Surface, TypeName,
    schema::{Meta, Node, Schema, Told},
};

use crate::{Files, config::SETTINGS, shapes::pascal};

/// `types.rs`, `client.rs` and `handler.rs`, each included by the crate
/// that holds the client.
pub fn generate(catalog: &Catalog) -> anyhow::Result<Files> {
    let mut writer = Writer::new(catalog);
    for action in crate::offered(catalog, Surface::Rest) {
        writer.action(action)?;
    }
    while let Some(name) = writer.queued.pop_first() {
        writer.shape(&name)?;
    }
    let mut handler = header();
    handler.push_str(
        "\n/// Each API action's handler: a server implements the actions it\n\
         /// serves, and every other one throws `not-found`, as a path naming\n\
         /// no action does.\n\
         pub trait Handler: Send + Sync {\n    \
         /// Who calls: what the server checked of the request, such as\n    \
         /// the caller's token, which each method passes on.\n    \
         type Caller: Send + Sync;\n",
    );
    handler.push_str(&writer.methods);
    handler.push_str(
        "}\n\n/// Calls `handler`'s method for `action`, its inputs read into\n\
         /// the action's arguments, and its spec where it binds one.\n\
         pub async fn handle<H: Handler>(\n    handler: &H,\n    caller: &H::Caller,\n    \
         action: &str,\n    input: crate::Input,\n) -> Result<crate::Answer, ::link_types::Thrown> {\n    \
         match action {\n",
    );
    handler.push_str(&writer.arms);
    handler.push_str("        _ => Err(crate::unserved(action)),\n    }\n}\n");
    handler.push_str("\n/// Every API action: its ID, path, method, and stream tag.\npub const ACTIONS: &[crate::Served] = &[\n");
    handler.push_str(&writer.table);
    handler.push_str("];\n");
    let mut files = Files::new();
    files.insert(&SETTINGS.rust.types, writer.types)?;
    files.insert(&SETTINGS.rust.client, writer.client)?;
    files.insert(&SETTINGS.rust.handler, handler)?;
    Ok(files)
}

fn header() -> String {
    format!("// {}\n", crate::marker())
}

/// What's written so far, and the named shapes still to write.
struct Writer<'a> {
    catalog: &'a Catalog,
    types: String,
    client: String,
    /// The handler trait's methods, its dispatch's arms, and the table of
    /// actions.
    methods: String,
    arms: String,
    table: String,
    /// Every Rust type declared, by its name, and what it is.
    taken: BTreeMap<String, String>,
    written: BTreeSet<TypeName>,
    queued: BTreeSet<TypeName>,
}

impl<'a> Writer<'a> {
    fn new(catalog: &'a Catalog) -> Self {
        Self {
            catalog,
            types: header(),
            client: header(),
            methods: String::new(),
            arms: String::new(),
            table: String::new(),
            taken: BTreeMap::new(),
            written: BTreeSet::new(),
            queued: BTreeSet::new(),
        }
    }

    /// Takes a Rust name for `what`, refused when another has it.
    fn take(&mut self, name: &str, what: &str) -> anyhow::Result<()> {
        if let Some(other) = self.taken.insert(name.to_string(), what.to_string()) {
            bail!("{what} and {other} would both be Rust's {name}");
        }
        Ok(())
    }

    /// A named shape's type, written once it's reached.
    fn named(&mut self, name: &TypeName) -> String {
        if !self.written.contains(name) {
            self.queued.insert(name.clone());
        }
        pascal(&name.to_string())
    }

    fn shape(&mut self, name: &TypeName) -> anyhow::Result<()> {
        if !self.written.insert(name.clone()) {
            return Ok(());
        }
        let declared = self
            .catalog
            .shape(name)
            .with_context(|| format!("{name} isn't declared"))?;
        let rust = pascal(&name.to_string());
        self.take(&rust, &format!("shape {name}"))?;
        let mut out = String::new();
        self.declare(&mut out, &rust, &declared.schema.clone())?;
        self.types.push_str(&out);
        Ok(())
    }

    /// Declares type `rust` as `schema`, into `out`: a struct for a
    /// record, an enum for words or a tagged choice, else an alias.
    fn declare(&mut self, out: &mut String, rust: &str, schema: &Schema) -> anyhow::Result<()> {
        out.push('\n');
        doc(out, &schema.meta, "");
        match &schema.node {
            Node::Record(record) => {
                writeln!(
                    out,
                    "#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]"
                )?;
                if record.others.is_none() {
                    writeln!(out, "#[serde(deny_unknown_fields)]")?;
                }
                writeln!(out, "pub struct {rust} {{")?;
                let mut fields = BTreeSet::new();
                let mut nested = Vec::new();
                for (key, property) in &record.properties {
                    let field = field(key);
                    if !fields.insert(field.clone()) {
                        bail!("{rust}: two properties would both be Rust's field {field}");
                    }
                    let ty = self.ty(property, &format!("{rust}{}", variant(key)), &mut nested)?;
                    // A record holding itself holds it boxed.
                    let ty = match ty == rust {
                        true => format!("Box<{ty}>"),
                        false => ty,
                    };
                    doc(out, &property.meta, "    ");
                    writeln!(out, "    #[serde(rename = {key:?})]")?;
                    match record.required.contains(key) {
                        true => writeln!(out, "    pub {field}: {ty},")?,
                        false => {
                            writeln!(
                                out,
                                "    #[serde(default, skip_serializing_if = \"Option::is_none\")]"
                            )?;
                            writeln!(out, "    pub {field}: Option<{ty}>,")?;
                        }
                    }
                }
                if let Some(others) = &record.others {
                    let ty = self.ty(others, &format!("{rust}Other"), &mut nested)?;
                    writeln!(out, "    #[serde(flatten)]")?;
                    writeln!(
                        out,
                        "    pub others: ::std::collections::BTreeMap<String, {ty}>,"
                    )?;
                }
                writeln!(out, "}}")?;
                for (name, schema) in nested {
                    self.declare(out, &name, &schema)?;
                }
            }
            Node::Enum(values) if values.iter().all(serde_json::Value::is_string) => {
                writeln!(
                    out,
                    "#[derive(Clone, Copy, Debug, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]"
                )?;
                writeln!(out, "pub enum {rust} {{")?;
                let mut variants = BTreeSet::new();
                for value in values.iter().filter_map(serde_json::Value::as_str) {
                    let variant = variant(value);
                    if !variants.insert(variant.clone()) {
                        bail!("{rust}: two words would both be Rust's {variant}");
                    }
                    writeln!(out, "    #[serde(rename = {value:?})]\n    {variant},")?;
                }
                writeln!(out, "}}")?;
            }
            Node::OneOf(choice) if matches!(choice.told, Told::Tag(_)) => {
                let Told::Tag(tag) = &choice.told else {
                    unreachable!()
                };
                writeln!(
                    out,
                    "#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]"
                )?;
                writeln!(out, "#[serde(tag = {tag:?})]")?;
                writeln!(out, "pub enum {rust} {{")?;
                let mut nested = Vec::new();
                for choice in &choice.variants {
                    let resolved = self.catalog.resolve(choice)?;
                    let Node::Record(record) = &resolved.node else {
                        bail!("{rust}: each choice of a tagged union is a record");
                    };
                    let word = match record.properties.get(tag).map(|p| &p.node) {
                        Some(Node::Const(serde_json::Value::String(word))) => word.clone(),
                        _ => bail!("{rust}: a choice names its kind by {tag}"),
                    };
                    let name = format!("{rust}{}", variant(&word));
                    let mut own = record.clone();
                    own.properties.remove(tag);
                    own.required.remove(tag);
                    let schema = Schema {
                        node: Node::Record(own),
                        meta: resolved.meta.clone(),
                    };
                    self.take(&name, &format!("{rust}'s {word}"))?;
                    nested.push((name.clone(), schema));
                    writeln!(out, "    #[serde(rename = {word:?})]")?;
                    writeln!(out, "    {}({name}),", variant(&word))?;
                }
                writeln!(out, "}}")?;
                for (name, schema) in nested {
                    self.declare(out, &name, &schema)?;
                }
            }
            _ => {
                let mut nested = Vec::new();
                let ty = self.ty(schema, &format!("{rust}Item"), &mut nested)?;
                writeln!(out, "pub type {rust} = {ty};")?;
                for (name, schema) in nested {
                    self.declare(out, &name, &schema)?;
                }
            }
        }
        Ok(())
    }

    /// The Rust type `schema` is where it stands; a record, words or a
    /// tagged choice in place becomes a type of its own, named `context`,
    /// declared after the one holding it.
    fn ty(
        &mut self,
        schema: &Schema,
        context: &str,
        nested: &mut Vec<(String, Schema)>,
    ) -> anyhow::Result<String> {
        Ok(match &schema.node {
            Node::Named(name) => self.named(name),
            Node::Record(_) => self.own(schema, context, nested)?,
            Node::Enum(values) if values.iter().all(serde_json::Value::is_string) => {
                self.own(schema, context, nested)?
            }
            Node::OneOf(choice) if matches!(choice.told, Told::Tag(_)) => {
                self.own(schema, context, nested)?
            }
            Node::Map(map) => format!(
                "::std::collections::BTreeMap<String, {}>",
                self.ty(&map.values, &format!("{context}Value"), nested)?
            ),
            Node::List(list) => format!(
                "Vec<{}>",
                self.ty(&list.items, &format!("{context}Item"), nested)?
            ),
            Node::Const(serde_json::Value::String(_)) | Node::String(_) => "String".into(),
            Node::Integer(_) => "i64".into(),
            Node::Number(_) => "f64".into(),
            Node::Boolean => "bool".into(),
            // Values told apart by their value alone, mixed words, and any
            // value are kept as JSON, which the server checks.
            Node::Enum(_) | Node::Const(_) | Node::OneOf(_) | Node::Any => {
                "::serde_json::Value".into()
            }
        })
    }

    fn own(
        &mut self,
        schema: &Schema,
        context: &str,
        nested: &mut Vec<(String, Schema)>,
    ) -> anyhow::Result<String> {
        self.take(context, &format!("a shape in place, {context}"))?;
        nested.push((context.to_string(), schema.clone()));
        Ok(context.to_string())
    }

    /// One action's arguments, its output, and its request's function.
    fn action(&mut self, action: &Action) -> anyhow::Result<()> {
        let base = pascal(action.id.as_str());
        let args = format!("{base}Args");
        let output = format!("{base}Output");
        self.take(&args, &format!("action {}'s arguments", action.id))?;
        self.take(&output, &format!("action {}'s output", action.id))?;
        let mut out = String::new();
        let mut record = link_actions::schema::Record::default();
        for (name, arg) in crate::rest_args(action) {
            record.properties.insert(name.clone(), arg.schema.clone());
            if arg.required {
                record.required.insert(name.clone());
            }
        }
        let mut nested = Vec::new();
        writeln!(out, "\n/// `{}`'s runtime arguments.", action.id)?;
        writeln!(
            out,
            "#[derive(Clone, Debug, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]"
        )?;
        writeln!(out, "pub struct {args} {{")?;
        for (key, property) in &record.properties {
            let ty = self.ty(property, &format!("{args}{}", variant(key)), &mut nested)?;
            doc(&mut out, &property.meta, "    ");
            writeln!(
                out,
                "    #[serde(rename = {key:?}, default, skip_serializing_if = \"Option::is_none\")]"
            )?;
            writeln!(out, "    pub {}: Option<{ty}>,", field(key))?;
        }
        writeln!(out, "}}")?;
        let mut types = String::new();
        for (name, schema) in nested {
            self.declare(&mut types, &name, &schema)?;
        }
        let mut nested = Vec::new();
        let answer = self.ty(&action.output, &format!("{output}Item"), &mut nested)?;
        writeln!(out, "/// What `{}` answers.", action.id)?;
        writeln!(out, "pub type {output} = {answer};")?;
        for (name, schema) in nested {
            self.declare(&mut types, &name, &schema)?;
        }
        self.types.push_str(&types);
        // Its request: each query argument by its name, and its body.
        let required: Vec<&String> = record.required.iter().collect();
        let spec = action.spec.as_ref().map(|spec| self.named(spec));
        let spec_param = spec
            .as_ref()
            .map(|ty| format!(", spec: &{ty}"))
            .unwrap_or_default();
        let query: Vec<String> = crate::openapi::query(action)
            .map(|(n, _)| format!("{n:?}"))
            .collect();
        let body: Vec<String> = crate::openapi::body(action)
            .map(|(n, _)| format!("{n:?}"))
            .collect();
        let method = &link_actions::transport::transport()
            .method(action.effect)
            .method;
        let stream = crate::openapi::tag(self.catalog, action)?;
        writeln!(out, "/// {}", action.summary())?;
        writeln!(
            out,
            "pub fn {}(args: &{args}{spec_param}) -> crate::Request<{output}> {{",
            field(&action.id.as_str().replace('.', "_"))
        )?;
        for name in required {
            let field = field(name);
            writeln!(
                out,
                "    debug_assert!(args.{field}.is_some(), \"{} takes {name}\");",
                action.id
            )?;
        }
        writeln!(
            out,
            "    crate::Request::new({:?}, {:?}, crate::Method::{}, args, &[{}], &[{}], {}, {})",
            action.id.as_str(),
            crate::openapi::path(action),
            variant(&method.to_lowercase()),
            query.join(", "),
            body.join(", "),
            match spec {
                Some(_) => "Some(::serde_json::to_value(spec).expect(\"a spec is JSON\"))",
                None => "None",
            },
            match &stream {
                Some(tag) => format!("Some({tag:?})"),
                None => "None".into(),
            },
        )?;
        writeln!(out, "}}")?;
        self.client.push_str(&out);
        self.serve(
            action,
            &args,
            &output,
            spec.as_deref(),
            stream.as_deref(),
            &record,
        )
    }

    /// One action's handler method, its dispatch arm, and its row.
    fn serve(
        &mut self,
        action: &Action,
        args: &str,
        output: &str,
        spec: Option<&str>,
        stream: Option<&str>,
        record: &link_actions::schema::Record,
    ) -> anyhow::Result<()> {
        let id = action.id.as_str();
        let name = field(&id.replace('.', "_"));
        let spec_param = spec.map(|ty| format!(", spec: {ty}")).unwrap_or_default();
        let answer = match stream {
            Some(_) => format!("crate::Events<{output}>"),
            None => output.to_string(),
        };
        let mut methods = String::new();
        writeln!(methods, "    /// `{id}`: {}", action.summary())?;
        writeln!(
            methods,
            "    fn {name}(&self, caller: &Self::Caller, args: {args}{spec_param}) -> impl ::std::future::Future<Output = Result<{answer}, ::link_types::Thrown>> + Send {{"
        )?;
        let unused = match spec {
            Some(_) => "(caller, args, spec)",
            None => "(caller, args)",
        };
        writeln!(methods, "        let _ = {unused};")?;
        writeln!(methods, "        async {{ Err(crate::unserved({id:?})) }}")?;
        writeln!(methods, "    }}")?;
        self.methods.push_str(&methods);
        let query: Vec<String> = crate::openapi::query(action)
            .map(|(n, arg)| Ok(format!("({n:?}, {})", self.kind(&arg.schema)?)))
            .collect::<anyhow::Result<_>>()?;
        let body: Vec<String> = crate::openapi::body(action)
            .map(|(n, _)| format!("{n:?}"))
            .collect();
        let required: Vec<String> = record.required.iter().map(|n| format!("{n:?}")).collect();
        let mut arm = String::new();
        writeln!(arm, "        {id:?} => {{")?;
        writeln!(
            arm,
            "            let (args, {}) = input.read::<{args}>(&[{}], &[{}], &[{}], {})?;",
            if spec.is_some() { "spec" } else { "_" },
            query.join(", "),
            body.join(", "),
            required.join(", "),
            spec.is_some()
        )?;
        let call = match spec {
            Some(_) => format!("handler.{name}(caller, args, crate::Input::spec(spec)?)"),
            None => format!("handler.{name}(caller, args)"),
        };
        match stream {
            Some(_) => writeln!(arm, "            Ok(crate::Answer::events({call}.await?))")?,
            None => writeln!(arm, "            crate::Answer::json(&{call}.await?)")?,
        }
        writeln!(arm, "        }}")?;
        self.arms.push_str(&arm);
        let method = &link_actions::transport::transport()
            .method(action.effect)
            .method;
        let cache = match stream {
            Some(_) => None,
            None => link_actions::transport::transport()
                .cache_control(action.effect, action.keeps.as_ref()),
        };
        writeln!(
            self.table,
            "    crate::Served {{ action: {id:?}, path: {:?}, method: crate::Method::{}, stream: {}, cache: {} }},",
            crate::openapi::path(action),
            variant(&method.to_lowercase()),
            match stream {
                Some(tag) => format!("Some({tag:?})"),
                None => "None".into(),
            },
            match cache {
                Some(cache) => format!("Some({cache:?})"),
                None => "None".into(),
            }
        )?;
        Ok(())
    }

    /// How a query parameter's text is read: as the scalar its schema is,
    /// each of a list's items so, or JSON.
    fn kind(&self, schema: &Schema) -> anyhow::Result<String> {
        Ok(match &self.catalog.resolve(schema)?.node {
            Node::String(_) | Node::Const(serde_json::Value::String(_)) => {
                "crate::Kind::Text".into()
            }
            Node::Enum(values) if values.iter().all(serde_json::Value::is_string) => {
                "crate::Kind::Text".into()
            }
            Node::Integer(_) => "crate::Kind::Integer".into(),
            Node::Number(_) => "crate::Kind::Number".into(),
            Node::Boolean => "crate::Kind::Boolean".into(),
            Node::List(list) => match self.kind(&list.items)?.as_str() {
                "crate::Kind::Text" => "crate::Kind::Texts".into(),
                "crate::Kind::Integer" => "crate::Kind::Integers".into(),
                _ => "crate::Kind::Json".into(),
            },
            _ => "crate::Kind::Json".into(),
        })
    }
}

/// A property's name as a Rust field: snake case, a keyword escaped.
fn field(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        match c {
            'A'..='Z' => {
                if i > 0 && !out.ends_with('_') {
                    out.push('_');
                }
                out.push(c.to_ascii_lowercase());
            }
            'a'..='z' | '0'..='9' | '_' => out.push(c),
            _ => {
                if !out.ends_with('_') {
                    out.push('_');
                }
            }
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    match KEYWORDS.contains(&out.as_str()) {
        true if out == "self" || out == "super" || out == "crate" => format!("{out}_"),
        true => format!("r#{out}"),
        false => out,
    }
}

/// A word as a Rust variant: PascalCase, digits kept.
fn variant(word: &str) -> String {
    let out: String = word
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    match out.starts_with(|c: char| c.is_ascii_digit()) || out.is_empty() {
        true => format!("V{out}"),
        false => out,
    }
}

const KEYWORDS: [&str; 51] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "yield", "gen", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual",
];

fn doc(out: &mut String, meta: &Meta, indent: &str) {
    if let Some(description) = &meta.description {
        for line in description.trim().lines() {
            let _ = writeln!(out, "{indent}/// {}", line.trim_end());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_read_as_rust() {
        assert_eq!(super::field("structuredContent"), "structured_content");
        assert_eq!(super::field("retry-after"), "retry_after");
        assert_eq!(super::field("type"), "r#type");
        assert_eq!(super::variant("ubuntu-24.04"), "Ubuntu2404");
        assert_eq!(super::variant("not-found"), "NotFound");
    }
}
