//! Each shape as a Rust type that enforces what its shape says: a record a
//! struct closed to other fields, a choice of words a closed enum, a
//! choice of records a tagged enum, an ID a typed ID, and every bound,
//! pattern and length checked by its `Validate`.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use anyhow::{Context, bail, ensure};
use link_actions::{
    Catalog, Module, TypeName,
    schema::{Format, Node, OneOf, Record, Schema, Told},
};

use crate::names::{field, pascal};

pub(crate) struct Types<'a> {
    pub catalog: &'a Catalog,
    pub module: &'a Module,
    pub out: String,
    /// Each Rust name written, with the shape it was written for.
    emitted: BTreeMap<String, Schema>,
    /// Each spec kind's check hooks, by its Rust name: those the actions
    /// binding it name, run on every read of it.
    hooks: BTreeMap<String, BTreeSet<String>>,
    /// Whether these are core's shared shapes, each given its JSON Schema
    /// and checked as it's read.
    pub shared: bool,
    /// The crate whose generated shapes this module's names mean, when
    /// another crate owns them: a program over its contract.
    pub shapes: Option<String>,
    /// The crate each other module's shapes are generated in, by the
    /// module's ID: a contract this one implements, or a spec kind's owner.
    pub owners: BTreeMap<String, String>,
    /// The records being written, innermost last: a reference back to one
    /// is boxed, so a shape may hold its own kind.
    enclosing: Vec<String>,
    /// The Rust names of this module's text shapes some map is keyed by
    /// (`propertyNames`): each ordered and shown as its text.
    property_names: BTreeSet<String>,
}

/// The field of an open record holding the keys it doesn't name.
const OTHERS: &str = "others";

/// A secret's value, as generated: held in link-secretive's type, never printed.
const SECRET: &str = "::link_secretive::Secret";

const DERIVE: &str = "#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]";

impl<'a> Types<'a> {
    pub(crate) fn new(catalog: &'a Catalog, module: &'a Module) -> Self {
        Self {
            catalog,
            module,
            out: String::new(),
            emitted: BTreeMap::new(),
            hooks: module
                .actions
                .values()
                .filter_map(|a| {
                    let spec = a.spec.as_ref()?;
                    // Another module's spec is checked where it's generated.
                    (spec.module() == module.identity.id)
                        .then(|| (pascal(spec.name()), a.checks.clone()))
                })
                // A shape's own hooks, wherever it's read.
                .chain(
                    module
                        .types
                        .iter()
                        .map(|(name, declared)| (pascal(name), declared.checks.clone())),
                )
                .fold(BTreeMap::new(), |mut hooks, (name, checks)| {
                    hooks
                        .entry(name)
                        .or_insert_with(BTreeSet::new)
                        .extend(checks);
                    hooks
                }),
            shared: false,
            shapes: None,
            owners: BTreeMap::new(),
            enclosing: Vec::new(),
            property_names: keyed(catalog, module),
        }
    }

    /// Every shape the module declares.
    pub(crate) fn all(&mut self) -> anyhow::Result<()> {
        if self.shapes.is_some() {
            return Ok(());
        }
        for (name, declared) in &self.module.types {
            self.named(&pascal(name), &declared.schema)
                .with_context(|| format!("shape {name}"))?;
            if self.shared {
                self.json_schema(name, &declared.schema)?;
            }
        }
        Ok(())
    }

    /// A shared shape's JSON Schema, for a hand-written spec holding it.
    fn json_schema(&mut self, name: &str, schema: &Schema) -> anyhow::Result<()> {
        ensure!(
            schema.names().is_empty(),
            "core's shape {name} names another: a shared shape stands alone"
        );
        let rust = pascal(name);
        let json = serde_json::to_string(&self.catalog.json(schema))?;
        let _ = write!(
            self.out,
            "impl ::schemars::JsonSchema for {rust} {{\n    \
             fn schema_name() -> ::std::borrow::Cow<'static, str> {{\n        {rust:?}.into()\n    }}\n    \
             fn json_schema(_: &mut ::schemars::SchemaGenerator) -> ::schemars::Schema {{\n        \
             ::serde_json::from_str({json:?}).expect(\"the catalog's JSON Schema\")\n    }}\n}}\n\n"
        );
        Ok(())
    }

    /// The typed ID of `text`'s kind, `box` as `BoxId`, written once.
    fn id_type(&mut self, text: &link_actions::schema::Text) -> anyhow::Result<String> {
        let kind = text.kind.clone().context("an ID names its kind")?;
        let name = format!("{}Id", pascal(&kind));
        let canonical = Schema::new(Node::String(link_actions::schema::Text {
            format: Some(Format::LinkId),
            kind: Some(kind.clone()),
            ..Default::default()
        }));
        if self.claim(&name, &canonical)? {
            let _ = write!(
                self.out,
                "/// A {kind} ID, `{kind}:<id>`.\n\
                 #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Serialize, ::serde::Deserialize)]\n\
                 #[serde(transparent)]\npub struct {name}(pub String);\n\n\
                 impl {name} {{\n    pub fn as_str(&self) -> &str {{\n        &self.0\n    }}\n}}\n\n\
                 impl ::std::fmt::Display for {name} {{\n    \
                 fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        f.write_str(&self.0)\n    }}\n}}\n\n\
                 impl ::link_actions::value::Validate for {name} {{\n    \
                 #[allow(clippy::needless_borrow, reason = \"a list's or map's items are checked as references\")]\n    \
                 fn validate(&self, at: &str) -> Result<(), ::link_actions::value::Refusal> {{\n        \
                 ::link_actions::value::id(at, &self.0, {kind:?})\n    }}\n}}\n\n"
            );
        }
        Ok(name)
    }

    /// The Rust type of the spec `action` reads: its declared kind's, or
    /// a held kind's hand-written type (`held.toml`).
    pub(crate) fn spec_type(
        &self,
        action: &link_actions::Action,
    ) -> anyhow::Result<Option<String>> {
        if let Some(spec) = &action.spec {
            return self.named_type(spec).map(Some);
        }
        Ok(action
            .held
            .as_deref()
            .and_then(link_actions::held_spec)
            .map(|held| held.rust.clone()))
    }

    /// Every Rust type name written so far.
    pub(crate) fn emitted(&self) -> impl Iterator<Item = &String> {
        self.emitted.keys()
    }

    /// Takes the Rust `name` for `schema`: true when it's newly taken,
    /// false when it was written for this same shape already, refused when
    /// another shape has it, so two shapes never share one type.
    fn claim(&mut self, name: &str, schema: &Schema) -> anyhow::Result<bool> {
        match self.emitted.get(name) {
            None => {
                self.emitted.insert(name.to_string(), schema.clone());
                Ok(true)
            }
            Some(taken) if taken == schema => Ok(false),
            Some(_) => bail!(
                "two shapes would both be the Rust type {name}: name the inline one as a shape of its own"
            ),
        }
    }

    /// The Rust type a declared name means: this module's, or core's in
    /// `link-types`.
    pub(crate) fn named_type(&self, name: &TypeName) -> anyhow::Result<String> {
        let core = link_actions::core();
        if name.module() == core.module && self.module.identity.id != core.module {
            return Ok(format!("::{}::{}", core.types, pascal(name.name())));
        }
        if name.module() != self.module.identity.id {
            let owner = self.owners.get(name.module()).with_context(|| {
                format!(
                    "{name} is module {}'s: the build names the crate its shapes are generated in",
                    name.module()
                )
            })?;
            return Ok(format!("{owner}::{}", pascal(name.name())));
        }
        Ok(match &self.shapes {
            Some(owner) => format!("{owner}::{}", pascal(name.name())),
            None => pascal(name.name()),
        })
    }

    /// A named shape: its own type, whatever it is.
    fn named(&mut self, name: &str, schema: &Schema) -> anyhow::Result<()> {
        match &schema.node {
            Node::Record(record) => self.record(name, schema, record, None),
            Node::Enum(_) => self.enumeration(name, schema),
            Node::OneOf(one) => self.one_of(name, schema, one),
            _ => {
                if !self.claim(name, schema)? {
                    return Ok(());
                }
                let inner = self.rust(schema, &format!("{name}Value"))?;
                let mut checks = self.checks("self.0", schema, "at")?;
                for hook in self.hooks.get(name).into_iter().flatten() {
                    let _ = writeln!(
                        checks,
                        "        crate::checks::{hook}(self).map_err(|why| ::link_actions::value::Refusal {{ at: at.to_string(), why }})?;"
                    );
                }
                let mut text = doc(schema);
                // A shared shape is checked as it's read, since a
                // hand-written spec holding it calls no `Validate`.
                let (derive, read) = if self.shared {
                    (
                        "#[derive(Clone, Debug, PartialEq, ::serde::Serialize)]".to_string(),
                        format!(
                            "impl<'de> ::serde::Deserialize<'de> for {name} {{\n    \
                             fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {{\n        \
                             let value = {name}(<{inner} as ::serde::Deserialize>::deserialize(deserializer)?);\n        \
                             ::link_actions::value::Validate::validate(&value, \"\").map_err(|r| ::serde::de::Error::custom(r.why))?;\n        \
                             Ok(value)\n    }}\n}}\n\n"
                        ),
                    )
                } else {
                    (self.derive(schema).to_string(), String::new())
                };
                // A plain text is ordered and shown as itself, so it can key
                // a map (`propertyNames`).
                let text_only = inner == "String" && self.property_names.contains(name);
                let derive = match text_only {
                    true => {
                        derive.replacen("PartialEq,", "PartialEq, Eq, PartialOrd, Ord, Hash,", 1)
                    }
                    false => derive,
                };
                let shown = match text_only {
                    true => format!(
                        "impl ::std::fmt::Display for {name} {{\n    \
                         fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        \
                         f.write_str(&self.0)\n    }}\n}}\n\n"
                    ),
                    false => String::new(),
                };
                let read = format!("{read}{shown}");
                let bare = match inner.contains(SECRET) {
                    true => "#[serde(with = \"::link_secretive::bare\")] ",
                    false => "",
                };
                let _ = write!(
                    text,
                    "{derive}\n#[serde(transparent)]\npub struct {name}({bare}pub {inner});\n\n{read}\
                     impl ::link_actions::value::Validate for {name} {{\n    \
                     #[allow(unused_variables, reason = \"a shape may have nothing to check\")]\n    \
                     #[allow(clippy::needless_borrow, reason = \"a list's or map's items are checked as references\")]\n    \
                     fn validate(&self, at: &str) -> Result<(), ::link_actions::value::Refusal> {{\n\
                     {checks}        Ok(())\n    }}\n}}\n\n"
                );
                self.out.push_str(&text);
                Ok(())
            }
        }
    }

    /// The Rust type `schema` reads into, writing a type for an inline
    /// record or choice under `hint`.
    pub(crate) fn rust(&mut self, schema: &Schema, hint: &str) -> anyhow::Result<String> {
        Ok(match &schema.node {
            Node::Named(name) => self.named_type(name)?,
            Node::Record(_) | Node::Enum(_) | Node::OneOf(_) => {
                self.named(hint, schema)?;
                hint.to_string()
            }
            Node::Map(map) => {
                let key = match &map.names {
                    Some(keys) => self.rust(keys, &format!("{hint}Key"))?,
                    None => "String".into(),
                };
                format!(
                    "::std::collections::BTreeMap<{key}, {}>",
                    self.rust(&map.values, &format!("{hint}Value"))?
                )
            }
            Node::List(list) => format!("Vec<{}>", self.rust(&list.items, &format!("{hint}Item"))?),
            Node::Const(value) if value.is_string() => "String".into(),
            Node::Const(_) => "::serde_json::Value".into(),
            Node::String(text) if text.kind.is_some() => self.id_type(text)?,
            Node::String(text) => match (text.format, text.path) {
                (Some(Format::Duration), _) => "::link_actions::value::Duration".into(),
                (Some(Format::Size), _) => "::link_actions::value::Size".into(),
                (Some(Format::Rate), _) => "::link_actions::value::Rate".into(),
                (Some(Format::SealTo), _) => "::link_secretive::SealTo".into(),
                (_, Some(_)) => "::std::path::PathBuf".into(),
                // A secret's value is held in link-secretive's type, never printed.
                _ if schema.meta.sensitive => "::link_secretive::Secret".into(),
                _ => "String".into(),
            },
            Node::Integer(bounds) => integer(bounds).into(),
            Node::Number(_) => "f64".into(),
            Node::Boolean => "bool".into(),
            Node::Any => match &schema.meta.rust {
                Some(rust) => rust.clone(),
                None => "::serde_json::Value".into(),
            },
        })
    }

    fn record(
        &mut self,
        name: &str,
        schema: &Schema,
        record: &Record,
        tag: Option<&str>,
    ) -> anyhow::Result<()> {
        if !self.claim(name, schema)? {
            return Ok(());
        }
        self.enclosing.push(name.to_string());
        let written = self.record_body(name, schema, record, tag);
        self.enclosing.pop();
        written
    }

    fn record_body(
        &mut self,
        name: &str,
        schema: &Schema,
        record: &Record,
        tag: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut idents = BTreeSet::new();
        let mut fields = String::new();
        let mut checks = String::new();
        let mut defaults = String::new();
        // Whether every declared default is its type's empty value, so the
        // record's empty value is what an empty file reads.
        let mut empty_defaults = true;
        for (property, child) in &record.properties {
            if Some(property.as_str()) == tag {
                continue;
            }
            let mut rust = self.rust(child, &format!("{name}{}", pascal(property)))?;
            // A reference back to a record being written holds it boxed.
            if matches!(child.node, Node::Named(_)) && self.enclosing.contains(&rust) {
                rust = format!("Box<{rust}>");
            }
            let ident = field(property);
            ensure!(
                idents.insert(ident.clone()),
                "{name}: two properties are both the Rust field {ident}"
            );
            let _ = writeln!(
                fields,
                "{}    #[serde(rename = {property:?})]",
                doc_indented(child)
            );
            // A secret is written as its value only here, on the wire.
            if rust.contains(SECRET) {
                let _ = writeln!(fields, "    #[serde(with = \"::link_secretive::bare\")]");
            }
            let at = format!("&::link_actions::value::field(at, {property:?})");
            let resolved = self.catalog.resolve(child)?;
            // An optional list stays optional: left out and empty differ.
            let empty = matches!(resolved.node, Node::Map(_));
            if record.required.contains(property) {
                let _ = writeln!(fields, "    pub {ident}: {rust},");
                checks += &self.checks(&format!("self.{ident}"), child, &at)?;
            } else if let Some(default) = &child.meta.default {
                let function = format!(
                    "{}_{}_default",
                    field(name).trim_start_matches("r#"),
                    ident.trim_start_matches("r#")
                );
                let literal = serde_json::to_string(default)?;
                // A default of nothing given, an empty list or a record
                // read from nothing, is never written: a value left as
                // its default reads back the same.
                let list = default.as_array().is_some_and(Vec::is_empty);
                let record = default.as_object().is_some_and(|o| o.is_empty());
                empty_defaults &= list || record;
                let unchanged = format!("{function}_is");
                let skip = match (list, record) {
                    (true, _) => ", skip_serializing_if = \"Vec::is_empty\"".to_string(),
                    (_, true) => format!(", skip_serializing_if = {unchanged:?}"),
                    _ => String::new(),
                };
                let _ = writeln!(
                    fields,
                    "    #[serde(default = {function:?}{skip})]\n    pub {ident}: {rust},"
                );
                let _ = writeln!(
                    defaults,
                    "#[allow(non_snake_case, reason = \"named for its type and field\")]\n\
                     fn {function}() -> {rust} {{\n    \
                     ::serde_json::from_str({literal:?}).expect(\"its declared default\")\n}}\n"
                );
                if record {
                    let _ = writeln!(
                        defaults,
                        "#[allow(non_snake_case, reason = \"named for its type and field\")]\n\
                         fn {unchanged}(value: &{rust}) -> bool {{\n    \
                         *value == {function}()\n}}\n"
                    );
                }
                checks += &self.checks(&format!("self.{ident}"), child, &at)?;
            } else if empty {
                let test = "::std::collections::BTreeMap::is_empty";
                let _ = writeln!(
                    fields,
                    "    #[serde(default, skip_serializing_if = {:?})]\n    pub {ident}: {rust},",
                    if matches!(child.node, Node::Named(_)) {
                        "Option::is_none"
                    } else {
                        test
                    }
                );
                if matches!(child.node, Node::Named(_)) {
                    bail!(
                        "property {property}: an optional named map is written inline or required"
                    );
                }
                checks += &self.checks(&format!("self.{ident}"), child, &at)?;
            } else {
                let _ = writeln!(
                    fields,
                    "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n    pub {ident}: Option<{rust}>,"
                );
                let inner = self.checks("value", child, &at)?;
                if !inner.is_empty() {
                    let _ = writeln!(
                        checks,
                        "        if let Some(value) = &self.{ident} {{\n{inner}        }}"
                    );
                }
            }
        }
        // A record open to keys it doesn't name holds each in a map
        // beside its named fields, checked as its shape says.
        let closed = match &record.others {
            Some(others) => {
                let rust = self.rust(others, &format!("{name}Other"))?;
                ensure!(
                    idents.insert(OTHERS.to_string()),
                    "{name}: a property is the Rust field {OTHERS}, which holds its other keys"
                );
                let _ = writeln!(
                    fields,
                    "    /// Every other key, as written.\n    #[serde(flatten)]\n    pub {OTHERS}: ::std::collections::BTreeMap<String, {rust}>,"
                );
                let inner = self.checks("item", others, "&item_at")?;
                if !inner.is_empty() {
                    let _ = writeln!(
                        checks,
                        "        for (key, item) in self.{OTHERS}.iter() {{\n            \
                         let item_at = ::link_actions::value::field(at, key);\n{inner}        }}"
                    );
                }
                ""
            }
            None => "#[serde(deny_unknown_fields)]\n",
        };
        for hook in self.hooks.get(name).into_iter().flatten() {
            let _ = writeln!(
                checks,
                "        crate::checks::{hook}(self).map_err(|why| ::link_actions::value::Refusal {{ at: at.to_string(), why }})?;"
            );
        }
        let derive = self.derive(schema);
        let mut text = doc(schema);
        // A record that needs nothing given has an empty value: every
        // field left out.
        let mut empty_value = String::new();
        if record.required.is_empty() && (defaults.is_empty() || empty_defaults) {
            text += "#[derive(Default)]\n";
        } else if record.required.is_empty() {
            // Its fields' declared defaults, as reading `{}` gives them.
            empty_value = format!(
                "impl Default for {name} {{\n    fn default() -> Self {{\n        \
                 ::serde_json::from_value(::serde_json::json!({{}})).expect(\"every field of it may be left out\")\n    }}\n}}\n\n"
            );
        }
        let _ = write!(
            text,
            "{derive}\n{closed}pub struct {name} {{\n{fields}}}\n\n{defaults}{empty_value}\
             impl ::link_actions::value::Validate for {name} {{\n    \
             #[allow(unused_variables, reason = \"a record may have nothing to check\")]\n    \
             #[allow(clippy::needless_borrow, reason = \"a list's or map's items are checked as references\")]\n    \
             fn validate(&self, at: &str) -> Result<(), ::link_actions::value::Refusal> {{\n\
             {checks}        Ok(())\n    }}\n}}\n\n"
        );
        self.out.push_str(&text);
        Ok(())
    }

    fn enumeration(&mut self, name: &str, schema: &Schema) -> anyhow::Result<()> {
        let Node::Enum(values) = &schema.node else {
            unreachable!("an enum")
        };
        if !self.claim(name, schema)? {
            return Ok(());
        }
        let mut variants = String::new();
        let mut words = String::new();
        let mut meanings = String::new();
        let mut seen = BTreeSet::new();
        for value in values {
            let text = value.as_str().context("an enum of words")?;
            let variant = pascal(text);
            ensure!(
                seen.insert(variant.clone()),
                "enum values {text:?} and another are one Rust name"
            );
            let _ = writeln!(variants, "    #[serde(rename = {text:?})]\n    {variant},");
            let _ = writeln!(words, "        (Self::{variant}, {text:?}),");
            if let Some(meaning) = schema.meta.values.get(text) {
                let _ = writeln!(meanings, "        (Self::{variant}, {meaning:?}),");
            }
        }
        let meanings = match meanings.is_empty() {
            true => String::new(),
            false => format!(
                "    const MEANINGS: &'static [(Self, &'static str)] = &[\n{meanings}    ];\n"
            ),
        };
        let mut text = doc(schema);
        let _ = write!(
            text,
            "#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Serialize, ::serde::Deserialize)]\n\
             pub enum {name} {{\n{variants}}}\n\n\
             impl ::link_actions::value::Words for {name} {{\n    \
             const WORDS: &'static [(Self, &'static str)] = &[\n{words}    ];\n{meanings}}}\n\n\
             impl ::link_actions::value::Validate for {name} {{\n    \
             fn validate(&self, _: &str) -> Result<(), ::link_actions::value::Refusal> {{\n        Ok(())\n    }}\n}}\n\n"
        );
        self.out.push_str(&text);
        Ok(())
    }

    fn one_of(&mut self, name: &str, schema: &Schema, one: &OneOf) -> anyhow::Result<()> {
        if !self.claim(name, schema)? {
            return Ok(());
        }
        match &one.told {
            Told::Tag(tag) => self.tagged(name, schema, one, tag),
            Told::Markers(markers) => self.marked(name, schema, one, markers),
            Told::Value => self.by_value(name, schema, one),
        }
    }

    /// A choice told apart by its tag, the default.
    fn tagged(
        &mut self,
        name: &str,
        schema: &Schema,
        one: &OneOf,
        tag: &str,
    ) -> anyhow::Result<()> {
        let mut seen = BTreeSet::new();
        let mut variants = String::new();
        let mut arms = String::new();
        for choice in &one.variants {
            let resolved = self.catalog.resolve(choice)?;
            let Node::Record(record) = &resolved.node else {
                bail!("a oneOf choice is a record")
            };
            let Some(Node::Const(value)) = record.properties.get(tag).map(|s| &s.node) else {
                bail!("a oneOf choice holds its tag as a const");
            };
            let value = value.as_str().context("a oneOf tag is a word")?;
            let variant = pascal(value);
            ensure!(
                seen.insert(variant.clone()),
                "{name}: two choices are both the variant {variant}"
            );
            let inner = format!("{name}{variant}");
            self.record(&inner, resolved, record, Some(tag))?;
            let _ = writeln!(
                variants,
                "    #[serde(rename = {value:?})]\n    {variant}({inner}),"
            );
            let _ = writeln!(
                arms,
                "            Self::{variant}(inner) => ::link_actions::value::Validate::validate(inner, at),"
            );
        }
        let derive = self.derive(schema);
        let mut text = doc(schema);
        let _ = write!(
            text,
            "{derive}\n#[allow(clippy::large_enum_variant, reason = \"a declared choice is the shape its declaration gives, each variant held as it is read\")]\n#[serde(tag = {tag:?})]\npub enum {name} {{\n{variants}}}\n\n\
             impl ::link_actions::value::Validate for {name} {{\n    \
             #[allow(clippy::needless_borrow, reason = \"a list's or map's items are checked as references\")]\n    \
             fn validate(&self, at: &str) -> Result<(), ::link_actions::value::Refusal> {{\n        \
             match self {{\n{arms}        }}\n    }}\n}}\n\n"
        );
        self.out.push_str(&text);
        Ok(())
    }

    /// A choice told apart by its value: a whole number, or one of its
    /// words, read as whichever it is.
    fn by_value(&mut self, name: &str, schema: &Schema, one: &OneOf) -> anyhow::Result<()> {
        // Each JSON type its own variant: a number, a record, a text or
        // its words, read untagged in that order.
        let mut variants = String::new();
        let mut arms = String::new();
        let mut words = Vec::new();
        for choice in &one.variants {
            let resolved = self.catalog.resolve(choice)?.clone();
            let (variant, said) = match &resolved.node {
                Node::Integer(_) => ("Number", "A whole number"),
                Node::Number(_) => ("Number", "A number"),
                Node::Record(_) => ("Record", "A record"),
                Node::String(_) => ("Text", "A text"),
                Node::Const(value) => {
                    words.push(value.as_str().context("a word")?.to_string());
                    continue;
                }
                _ => bail!("a choice told by its value is a number, a word, a text or a record"),
            };
            let inner = self.rust(choice, &format!("{name}{variant}"))?;
            let checks = self.checks("value", choice, "at")?;
            let _ = writeln!(variants, "    /// {said}.\n    {variant}({inner}),");
            let _ = writeln!(
                arms,
                "            Self::{variant}({bound}) => {{\n{checks}                Ok(())\n            }}",
                bound = if checks.is_empty() { "_" } else { "value" }
            );
        }
        // Its words are read first, and only they: any other text is the
        // text's, so `"none"` beside a size is the word, never a bad size.
        let mut helper = String::new();
        let texts = variants.contains("    Text(");
        if !words.is_empty() && !texts {
            // With no text beside them, any text is a word, refused by its
            // check naming where it is.
            let listed = words
                .iter()
                .map(|w| format!("{w:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                variants,
                "    /// One of its words: {listed}.\n    Word(String),"
            );
            let _ = writeln!(
                arms,
                "            Self::Word(word) => ::link_actions::value::one_of(at, word, &[{listed}]),"
            );
        } else if !words.is_empty() {
            let listed = words
                .iter()
                .map(|w| format!("{w:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            let read = format!("{}_word", crate::names::field(name));
            variants = format!(
                "    /// One of its words: {listed}.\n    #[serde(deserialize_with = \"{read}\")]\n    Word(String),\n{variants}"
            );
            let _ = writeln!(
                arms,
                "            Self::Word(word) => ::link_actions::value::one_of(at, word, &[{listed}]),"
            );
            let _ = write!(
                helper,
                "/// One of [`{name}`]'s words, and no other text.\n\
                 fn {read}<'de, D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {{\n    \
                 ::link_actions::value::word(deserializer, &[{listed}])\n}}\n\n"
            );
        }
        let derive = self.derive(schema);
        let mut text = doc(schema);
        let _ = write!(
            text,
            "{derive}\n#[allow(clippy::large_enum_variant, reason = \"a declared choice is the shape its declaration gives, each variant held as it is read\")]\n#[serde(untagged)]\npub enum {name} {{\n{variants}}}\n\n\
             impl ::link_actions::value::Validate for {name} {{\n    \
             #[allow(clippy::needless_borrow, reason = \"each variant's checks read it as a field's\")]\n    \
             fn validate(&self, at: &str) -> Result<(), ::link_actions::value::Refusal> {{\n        \
             match self {{\n{arms}        }}\n    }}\n}}\n\n"
        );
        self.out.push_str(&text);
        self.out.push_str(&helper);
        Ok(())
    }

    /// A choice told apart by the marking key each choice alone requires:
    /// read by which key is there, written as the choice it is.
    fn marked(
        &mut self,
        name: &str,
        schema: &Schema,
        one: &OneOf,
        markers: &[String],
    ) -> anyhow::Result<()> {
        let mut seen = BTreeSet::new();
        let mut variants = String::new();
        let mut arms = String::new();
        let mut reads = String::new();
        for (choice, marker) in one.variants.iter().zip(markers) {
            let variant = pascal(marker);
            ensure!(
                seen.insert(variant.clone()),
                "{name}: two choices are both the variant {variant}"
            );
            let inner = self.rust(choice, &format!("{name}{variant}"))?;
            let _ = writeln!(variants, "    {variant}({inner}),");
            let _ = writeln!(
                arms,
                "            Self::{variant}(inner) => ::link_actions::value::Validate::validate(inner, at),"
            );
            let _ = writeln!(
                reads,
                "        if value.get({marker:?}).is_some() {{\n            \
                 return ::serde_json::from_value(value).map(Self::{variant}).map_err(::serde::de::Error::custom);\n        }}"
            );
        }
        let keys = markers.join(", ");
        let mut text = doc(schema);
        let _ = write!(
            text,
            "#[derive(Clone, Debug, PartialEq, ::serde::Serialize)]\n#[allow(clippy::large_enum_variant, reason = \"a declared choice is the shape its declaration gives, each variant held as it is read\")]\n#[serde(untagged)]\npub enum {name} {{\n{variants}}}\n\n\
             impl<'de> ::serde::Deserialize<'de> for {name} {{\n    \
             fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {{\n        \
             let value = <::serde_json::Value as ::serde::Deserialize>::deserialize(deserializer)?;\n\
             {reads}        Err(::serde::de::Error::custom(\"it holds none of {keys}, the keys that say which it is\"))\n    }}\n}}\n\n\
             impl ::link_actions::value::Validate for {name} {{\n    \
             #[allow(clippy::needless_borrow, reason = \"a list's or map's items are checked as references\")]\n    \
             fn validate(&self, at: &str) -> Result<(), ::link_actions::value::Refusal> {{\n        \
             match self {{\n{arms}        }}\n    }}\n}}\n\n"
        );
        self.out.push_str(&text);
        Ok(())
    }

    /// The statements refusing what `schema` refuses of `expr`, a value
    /// of its Rust type, found at the string expression `at`.
    /// Whether `schema` holds a secret's value, itself or in what it
    /// holds: its type then has no `Clone`, as a `Secret` hasn't.
    pub(crate) fn holds_secret(&self, schema: &Schema) -> bool {
        let mut seen = BTreeSet::new();
        self.secret_in(schema, &mut seen)
    }

    fn secret_in(&self, schema: &Schema, seen: &mut BTreeSet<String>) -> bool {
        match &schema.node {
            Node::Named(name) => {
                seen.insert(name.to_string())
                    && self
                        .catalog
                        .shape(name)
                        .is_some_and(|d| self.secret_in(&d.schema, seen))
            }
            Node::String(text) => {
                schema.meta.sensitive
                    && text.kind.is_none()
                    && text.path.is_none()
                    && !matches!(
                        text.format,
                        Some(Format::Duration | Format::Size | Format::Rate | Format::SealTo)
                    )
            }
            Node::Record(record) => {
                record.properties.values().any(|p| self.secret_in(p, seen))
                    || record
                        .others
                        .as_ref()
                        .is_some_and(|o| self.secret_in(o, seen))
            }
            Node::Map(map) => self.secret_in(&map.values, seen),
            Node::List(list) => self.secret_in(&list.items, seen),
            Node::OneOf(one) => one.variants.iter().any(|v| self.secret_in(v, seen)),
            // A type the format can't say may hold a secret, and may not
            // be Clone: a type holding it isn't either.
            Node::Any => schema.meta.rust.is_some(),
            _ => false,
        }
    }

    /// The derives of a type for `schema`: every one, but `Clone` for
    /// one holding a secret.
    fn derive(&self, schema: &Schema) -> &'static str {
        match self.holds_secret(schema) {
            true => "#[derive(Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]",
            false => DERIVE,
        }
    }

    pub(crate) fn checks(&self, expr: &str, schema: &Schema, at: &str) -> anyhow::Result<String> {
        let mut out = String::new();
        let v = "::link_actions::value";
        match &schema.node {
            Node::Named(_) | Node::Record(_) | Node::Enum(_) | Node::OneOf(_) => {
                let _ = writeln!(out, "        {v}::Validate::validate(&{expr}, {at})?;");
            }
            Node::List(list) => {
                let _ = writeln!(
                    out,
                    "        {v}::items({at}, {expr}.len(), {:?}, {:?})?;",
                    list.min_items, list.max_items
                );
                let inner = self.checks("item", &list.items, "&item_at")?;
                if !inner.is_empty() {
                    let _ = writeln!(
                        out,
                        "        let list_at: &str = {at};\n        for (i, item) in {expr}.iter().enumerate() {{\n            \
                         let item_at = format!(\"{{list_at}}[{{}}]\", i + 1);\n{inner}        }}"
                    );
                }
            }
            Node::Map(map) => {
                let inner = self.checks("item", &map.values, "&item_at")?;
                // A typed key is checked as its shape says, where it is.
                let keyed = match &map.names {
                    Some(keys) => self.checks("key", keys, "&item_at")?,
                    None => String::new(),
                };
                if !inner.is_empty() || !keyed.is_empty() {
                    let _ = writeln!(
                        out,
                        "        for (key, item) in {expr}.iter() {{\n            \
                         let item_at = {v}::field({at}, &key.to_string());\n{keyed}{inner}        }}"
                    );
                }
            }
            Node::Const(value) => {
                let literal = serde_json::to_string(value)?;
                if value.is_string() {
                    let _ = writeln!(
                        out,
                        "        {v}::constant({at}, &{expr}.as_str(), &{literal})?;"
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "        {v}::constant({at}, &{expr}, &::serde_json::json!({literal}))?;"
                    );
                }
            }
            Node::String(text) if text.kind.is_some() => {
                let _ = writeln!(out, "        {v}::Validate::validate(&{expr}, {at})?;");
            }
            Node::String(text) => {
                let s = if text.path.is_some() {
                    format!("{expr}.to_string_lossy()")
                } else if schema.meta.sensitive {
                    format!("{v}::secret_text(&{expr})")
                } else {
                    expr.to_string()
                };
                if let Some(pattern) = &text.pattern {
                    let said = match &schema.meta.description {
                        Some(said) => format!("Some({said:?})"),
                        None => "None".into(),
                    };
                    let _ = writeln!(
                        out,
                        "        {v}::described({at}, &{s}, {pattern:?}, {said})?;"
                    );
                }
                if text.min_length.is_some() || text.max_length.is_some() {
                    let _ = writeln!(
                        out,
                        "        {v}::length({at}, &{s}, {:?}, {:?})?;",
                        text.min_length, text.max_length
                    );
                }
                if text.format == Some(Format::LinkId) {
                    let _ = writeln!(out, "        {v}::link_id({at}, &{s})?;");
                }
            }
            Node::Integer(b) if b.minimum.is_some() || b.maximum.is_some() => {
                // Compared wide, whatever width its bounds give it.
                let _ = writeln!(
                    out,
                    "        {v}::bounds({at}, i128::from(*::std::borrow::Borrow::<{}>::borrow(&{expr})), {:?}, {:?})?;",
                    integer(b),
                    b.minimum,
                    b.maximum
                );
            }
            Node::Number(b) if b.minimum.is_some() || b.maximum.is_some() => {
                let _ = writeln!(
                    out,
                    "        {v}::bounds({at}, *::std::borrow::Borrow::<f64>::borrow(&{expr}), {:?}, {:?})?;",
                    b.minimum, b.maximum
                );
            }
            _ => {}
        }
        Ok(out)
    }
}

pub(crate) fn doc(schema: &Schema) -> String {
    schema
        .meta
        .description
        .as_deref()
        .map(|d| d.lines().map(|l| format!("/// {l}\n")).collect())
        .unwrap_or_default()
}

fn doc_indented(schema: &Schema) -> String {
    doc(schema).lines().map(|l| format!("    {l}\n")).collect()
}

/// The smallest Rust integer a whole number's bounds fit: unsigned from a
/// least of 0 (`u64` with no most), signed below it, and `i64` unbounded.
fn integer(bounds: &link_actions::schema::Bounds<i64>) -> &'static str {
    match (bounds.minimum, bounds.maximum) {
        (Some(min), max) if min >= 0 => match max {
            Some(max) if max <= i64::from(u8::MAX) => "u8",
            Some(max) if max <= i64::from(u16::MAX) => "u16",
            Some(max) if max <= i64::from(u32::MAX) => "u32",
            _ => "u64",
        },
        (Some(min), Some(max)) => {
            let fits = |low: i64, high: i64| min >= low && max <= high;
            if fits(i64::from(i8::MIN), i64::from(i8::MAX)) {
                "i8"
            } else if fits(i64::from(i16::MIN), i64::from(i16::MAX)) {
                "i16"
            } else if fits(i64::from(i32::MIN), i64::from(i32::MAX)) {
                "i32"
            } else {
                "i64"
            }
        }
        _ => "i64",
    }
}

/// The Rust names of `module`'s shapes that key a map anywhere in
/// `catalog` (`propertyNames`).
fn keyed(catalog: &Catalog, module: &Module) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut note = |schema: &Schema| {
        schema.walk(&mut |schema| {
            if let Node::Map(map) = &schema.node
                && let Some(Node::Named(name)) = map.names.as_deref().map(|k| &k.node)
                && name.module() == module.identity.id
            {
                keys.insert(pascal(name.name()));
            }
        });
    };
    for other in catalog.modules() {
        for declared in other.types.values() {
            note(&declared.schema);
        }
        for action in other.actions.values() {
            for arg in action.args.values() {
                note(&arg.schema);
            }
        }
    }
    keys
}
