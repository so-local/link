//! Each struct of runtime arguments the generator writes, checked as
//! it's read, and its rows for the parser's table.

use std::{collections::BTreeSet, fmt::Write as _};

use anyhow::{bail, ensure};
use link_actions::{
    Arg,
    schema::{Node, Schema},
};

use super::{Commands, doc};
use crate::names::{field, pascal};

impl Commands<'_, '_> {
    /// `write`'s struct of runtime arguments, `name`, and its rows; over
    /// a contract's arguments ([`Commands::over`]), the rows alone, and
    /// the contract's struct and every type it made named in its place,
    /// so each is one Rust type.
    pub(super) fn reused(
        &mut self,
        name: &str,
        write: impl FnOnce(&mut Self) -> anyhow::Result<String>,
    ) -> anyhow::Result<String> {
        let Some(owner) = self.over.clone() else {
            return write(self);
        };
        let before: BTreeSet<String> = self.types.emitted().cloned().collect();
        let (out, types) = (self.out.len(), self.types.out.len());
        let rows = write(self)?;
        self.out.truncate(out);
        self.types.out.truncate(types);
        let made = self
            .types
            .emitted()
            .filter(|made| !before.contains(*made))
            .cloned()
            .collect::<Vec<_>>();
        for made in std::iter::once(name.to_string()).chain(made) {
            let _ = writeln!(self.out, "pub use {owner}::{made};");
        }
        Ok(rows)
    }

    /// A struct of runtime arguments, `name`, checked as it's read, and
    /// its rows for the parser's table. A `joined` struct, an argument
    /// group, keeps whether each argument was given, so two can be laid
    /// one over the other: an argument with a declared default is
    /// optional, its accessor of the same name giving the default when
    /// it's left out, and its flag shows the default without setting it.
    pub(super) fn shape(
        &mut self,
        name: &str,
        label: &str,
        args: &[(String, Arg)],
        mut checks: String,
        joined: bool,
    ) -> anyhow::Result<String> {
        let mut accessors = String::new();
        let mut fields = String::new();
        let mut rows = String::new();
        // Each argument group taken, one flattened field of its own type.
        let mut groups = BTreeSet::new();
        for (arg, declared) in args {
            let mut grouped_fields = String::new();
            let mut grouped_checks = String::new();
            let (fields, checks): (&mut String, &mut String) = match &declared.group {
                Some(group) => {
                    groups.insert(group.clone());
                    (&mut grouped_fields, &mut grouped_checks)
                }
                None => (&mut fields, &mut checks),
            };
            let resolved = self.types.catalog.resolve(&declared.schema)?.clone();
            let rust = self
                .types
                .rust(&declared.schema, &format!("{name}{}", pascal(arg)))?;
            let ident = field(arg);
            let at = format!("{arg:?}");
            let items = match &resolved.node {
                Node::List(list) => Some(self.types.catalog.resolve(&list.items)?.clone()),
                _ => None,
            };
            let kind = kind(&resolved, items.as_ref(), declared)?;
            ensure!(
                declared.cli.default_missing.is_none() || kind == "Text",
                "argument {arg}: default-missing is a text flag's"
            );
            let _ = writeln!(
                fields,
                "{}    #[serde(rename = {arg:?})]",
                doc(&declared.schema)
            );
            let joined = joined || declared.group.is_some();
            if let (Some(default), true) = (&declared.schema.meta.default, joined) {
                // Left out, it's its default, which a join never sees.
                let literal = serde_json::to_string(default)?;
                let _ = writeln!(
                    fields,
                    "    #[serde(default)]\n    pub {ident}: Option<{rust}>,"
                );
                // A grouped argument's accessor is its group's.
                let accessors = match declared.group {
                    Some(_) => &mut String::new(),
                    None => &mut accessors,
                };
                let _ = write!(
                    accessors,
                    "    /// `{arg}`, its declared default when it's left out.\n    \
                     pub fn {ident}(&self) -> {rust} {{\n        \
                     self.{ident}.as_ref().cloned().unwrap_or_else(|| ::serde_json::from_str({literal:?}).expect(\"its declared default\"))\n    }}\n\n"
                );
                let inner = self.types.checks("value", &declared.schema, &at)?;
                if !inner.is_empty() {
                    let _ = writeln!(
                        checks,
                        "        if let Some(value) = &self.{ident} {{\n{inner}        }}"
                    );
                }
            } else if let Some(default) = &declared.schema.meta.default {
                // Its absence means its default, on every surface.
                let function = format!(
                    "{}_{}_default",
                    crate::names::field(name)
                        .trim_start_matches("r#")
                        .to_lowercase(),
                    ident.trim_start_matches("r#")
                );
                let literal = serde_json::to_string(default)?;
                let _ = writeln!(
                    fields,
                    "    #[serde(default = {function:?})]\n    pub {ident}: {rust},"
                );
                let _ = writeln!(
                    self.out,
                    "fn {function}() -> {rust} {{\n    \
                     ::serde_json::from_str({literal:?}).expect(\"its declared default\")\n}}\n"
                );
                *checks += &self
                    .types
                    .checks(&format!("self.{ident}"), &declared.schema, &at)?;
            } else if declared.required {
                let _ = writeln!(fields, "    pub {ident}: {rust},");
                *checks += &self
                    .types
                    .checks(&format!("self.{ident}"), &declared.schema, &at)?;
            } else if matches!(kind, "Switch" | "Texts" | "Integers" | "Rest") {
                let _ = writeln!(fields, "    #[serde(default)]\n    pub {ident}: {rust},");
                *checks += &self
                    .types
                    .checks(&format!("self.{ident}"), &declared.schema, &at)?;
            } else {
                let _ = writeln!(
                    fields,
                    "    #[serde(default)]\n    pub {ident}: Option<{rust}>,"
                );
                let inner = self.types.checks("value", &declared.schema, &at)?;
                if !inner.is_empty() {
                    let _ = writeln!(
                        checks,
                        "        if let Some(value) = &self.{ident} {{\n{inner}        }}"
                    );
                }
            }
            let values: Vec<&str> = match &resolved.node {
                Node::Enum(values) => values.iter().filter_map(|v| v.as_str()).collect(),
                _ => Vec::new(),
            };
            // A whole number's bounds, the parser's to refuse by its flag.
            let range = match (&resolved.node, items.as_ref().map(|i| &i.node)) {
                (Node::Integer(b), _) | (_, Some(Node::Integer(b))) => {
                    format!("({:?}, {:?})", b.minimum, b.maximum)
                }
                _ => "(None, None)".to_string(),
            };
            // The kind of Link ID it takes, whose names the command line
            // looks up.
            let id_kind = match &resolved.node {
                Node::String(text) if text.format == Some(link_actions::schema::Format::LinkId) => {
                    text.kind.as_deref()
                }
                _ => None,
            };
            rows += &row(arg, declared, kind, &values, &range, joined, id_kind);
        }
        for group in &groups {
            let _ = writeln!(
                fields,
                "    /// Its {group} arguments, the module's group.\n    #[serde(flatten)]\n    pub {}: {},",
                field(group),
                pascal(group)
            );
            let _ = writeln!(
                checks,
                "        ::link_actions::value::Validate::validate(&self.{}, at)?;",
                field(group)
            );
        }
        // A struct taking a group refuses an unknown key itself, since a
        // flattened field takes whatever its holder doesn't name.
        let (closed, derive, read) = match groups.is_empty() {
            true => (
                "#[serde(deny_unknown_fields)]\n",
                "::serde::Deserialize",
                String::new(),
            ),
            false => {
                let known: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                let moved: Vec<String> = args
                    .iter()
                    .filter(|(_, a)| a.group.is_none())
                    .map(|(n, _)| field(n))
                    .chain(groups.iter().map(|g| field(g)))
                    .map(|f| format!("{f}: read.{f}"))
                    .collect();
                (
                    "",
                    "",
                    format!(
                        "impl<'de> ::serde::Deserialize<'de> for {name} {{\n    \
                         fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {{\n        \
                         const KNOWN: &[&str] = &{known:?};\n        \
                         let given = <::serde_json::Map<String, ::serde_json::Value> as ::serde::Deserialize>::deserialize(deserializer)?;\n        \
                         if let Some(key) = given.keys().find(|key| !KNOWN.contains(&key.as_str())) {{\n            \
                         return Err(<D::Error as ::serde::de::Error>::unknown_field(key, KNOWN));\n        }}\n        \
                         #[derive(::serde::Deserialize)]\n        \
                         struct Read {{\n{fields}        }}\n        \
                         let read: Read = ::serde_json::from_value(::serde_json::Value::Object(given)).map_err(<D::Error as ::serde::de::Error>::custom)?;\n        \
                         Ok({name} {{ {} }})\n    }}\n}}\n\n",
                        moved.join(", ")
                    ),
                )
            }
        };
        // Arguments none of which must be given, and none of which has a
        // declared default outside a join, have an empty value.
        let default = if args
            .iter()
            .any(|(_, a)| a.required || (!joined && a.schema.meta.default.is_some()))
        {
            ""
        } else {
            "Default, "
        };
        let _ = write!(
            self.out,
            "/// `{}`'s runtime arguments.\n\
             #[derive({default}Clone, Debug, PartialEq, ::serde::Serialize, {derive})]\n\
             {closed}pub struct {name} {{\n{fields}}}\n\n{read}\
             impl ::link_actions::value::Validate for {name} {{\n    \
             #[allow(unused_variables, reason = \"an action may have nothing to check\")]\n    \
             #[allow(clippy::needless_borrow, reason = \"a list's or map's items are checked as references\")]\n    \
             fn validate(&self, at: &str) -> Result<(), ::link_actions::value::Refusal> {{\n\
             {checks}        Ok(())\n    }}\n}}\n\n",
            label,
        );
        if !accessors.is_empty() {
            let _ = write!(self.out, "impl {name} {{\n{accessors}}}\n\n");
        }
        Ok(rows)
    }
}

/// One argument's row in a parser table.
/// A `joined` argument's default is told in its help, never set: a
/// join sees only what's given.
fn row(
    name: &str,
    arg: &Arg,
    kind: &str,
    values: &[&str],
    range: &str,
    joined: bool,
    id_kind: Option<&str>,
) -> String {
    let default = arg.schema.meta.default.as_ref().map(|d| match d {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    });
    let description = arg.schema.meta.description.clone().unwrap_or_default();
    let (help, default) = match (default, joined) {
        (Some(default), true) => (format!("{description} [default: {default}]"), None),
        (default, _) => (description, default),
    };
    format!(
        "    ::link_actions::cli::Arg {{ name: {name:?}, kind: ::link_actions::cli::Kind::{kind}, help: {:?}, \
         required: {}, position: {:?}, short: {:?}, env: {:?}, values: &{values:?}, default: {:?}, range: {range}, last: {}, default_missing: {:?}, \
         conflicts: &{:?}, requires: &{:?}, hidden: {}, global: {}, id_kind: {:?} }},\n",
        help,
        arg.required,
        arg.cli.position.map(usize::from),
        arg.cli.short,
        arg.cli.env,
        default,
        arg.cli.last,
        arg.cli.default_missing,
        arg.cli.conflicts,
        arg.cli.requires,
        arg.cli.hidden,
        arg.every,
        id_kind,
    )
}

/// How the parser types an argument's value.
fn kind(resolved: &Schema, items: Option<&Schema>, arg: &Arg) -> anyhow::Result<&'static str> {
    if arg.cli.rest {
        return Ok("Rest");
    }
    Ok(match &resolved.node {
        Node::Boolean => "Switch",
        Node::Integer(_) => "Integer",
        Node::Number(_) => "Number",
        Node::String(_) | Node::Enum(_) | Node::Const(_) => "Text",
        Node::List(_) => match items.map(|items| &items.node) {
            Some(Node::Integer(_)) => "Integers",
            Some(Node::String(_) | Node::Enum(_)) => "Texts",
            _ => bail!("a list argument lists text or whole numbers"),
        },
        _ => bail!("an argument is a scalar, a list or a choice"),
    })
}
