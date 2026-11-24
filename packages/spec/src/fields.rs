//! A spec kind's fields as command-line flags, generated from its declared
//! shape ([`crate::schema`]): every field's flag is its TOML key path as
//! the file writes it, `--os` or `--model.thinking`, and only the spec's
//! own properties (its `name`, `version` and what it `extends`) take
//! `--spec.<key>`. A flag whose first word is a top-level field the
//! command's runtime flags don't claim is a field flag, and a runtime
//! flag never shares a field's name. A field added to the type has its
//! flag in the same build, with no table of flags to keep.
//!
//! [`split`] takes the field flags out of a command line before clap
//! sees the rest, [`read_with`] reads them over a spec file through the
//! kind's strict reader, [`help`] lists them, and [`show`] is the
//! `spec show` command every kind has.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::constants::{
    FREE_KEY, MAX_DEPTH, PATH_FORMAT, RESERVED, SHORTHAND, SPEC_OWN, TICK, TOP,
};

mod choices;
mod given;
mod leaf;
#[cfg(test)]
mod resolving;
mod show;
#[cfg(test)]
pub(crate) mod tests;
pub use given::{
    FieldFlags, Made, Trailing, clashes, flags_of, named_schema, over, read_text_at,
    read_text_with, read_with, split, split_claimed,
};

/// The spec kind a command binds: its shape, whose fields are its field
/// flags, and its format, whose steps answer a field's old flag.
pub struct Binding {
    pub schema: Value,
    pub format: crate::schema::Format,
}

impl Binding {
    /// The binding of kind `T`.
    pub fn of<T: crate::Kind>() -> Self {
        Binding {
            schema: crate::schema::<T>(),
            format: T::format().clone(),
        }
    }
}

mod nearest;
mod retired;
mod secret;
use nearest::unknown;
pub use retired::RetiredFlag;
pub use show::{Show, SpecCommand, show};

/// One place in a spec's shape: every schema a value there may take,
/// `$ref`s followed and `null` dropped (an optional field is its type).
#[derive(Clone)]
pub(crate) struct At<'a> {
    root: &'a Value,
    nodes: Vec<&'a Value>,
    /// The definitions followed from the top to here, so a type inside
    /// itself is listed once rather than walked again.
    chain: Vec<&'a str>,
    /// Whether a definition here is one of its own ancestors.
    recursive: bool,
    /// Where this place's own definitions start in `chain`: those before
    /// are its ancestors'.
    own: usize,
    doc: Option<&'a str>,
    /// The doc comment written on this place itself, not its type's.
    own_doc: Option<&'a str>,
    default: Option<&'a Value>,
    /// The first example its schema gives, a variant's definition's too.
    example: Option<&'a Value>,
    /// The first example written on this place itself, not its type's.
    own_example: Option<&'a Value>,
    /// Extra constraints make finite scalar candidates a review concern.
    plain_choices: bool,
    leaf_plain: bool,
    samples: Vec<&'a Value>,
}

impl<'a> At<'a> {
    /// The top of `root`, a whole JSON Schema with its `$defs`.
    pub(crate) fn top(root: &'a Value) -> Self {
        // `#` is the top's own name, which a type inside itself refers to.
        Self::of(root, &[root], vec!["#"])
    }

    fn of(root: &'a Value, nodes: &[&'a Value], chain: Vec<&'a str>) -> Self {
        let mut at = At {
            root,
            nodes: Vec::new(),
            chain,
            recursive: false,
            own: 0,
            doc: None,
            own_doc: None,
            default: None,
            example: None,
            own_example: None,
            plain_choices: true,
            leaf_plain: true,
            samples: Vec::new(),
        };
        let above = at.chain.len();
        at.own = above;
        for node in nodes {
            if at.own_doc.is_none() {
                at.own_doc = node.get("description").and_then(Value::as_str);
            }
            if at.own_example.is_none() {
                at.own_example = node
                    .get("examples")
                    .and_then(Value::as_array)
                    .and_then(|e| e.first());
            }
            at.gather(node, 0, above);
        }
        at
    }

    fn gather(&mut self, node: &'a Value, depth: usize, above: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        self.plain_choices &= choices::plain(node);
        self.leaf_plain &= leaf::plain(node);
        self.samples.extend(
            node.get("examples")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
        if self.doc.is_none() {
            self.doc = node.get("description").and_then(Value::as_str);
        }
        if self.default.is_none() {
            self.default = node.get("default");
        }
        if self.example.is_none() {
            self.example = node
                .get("examples")
                .and_then(Value::as_array)
                .and_then(|e| e.first());
        }
        if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
            let target = match reference {
                "#" => Some(self.root),
                _ => reference
                    .strip_prefix("#/$defs/")
                    .and_then(|name| self.root.get("$defs")?.get(name)),
            };
            if self.chain[..above].contains(&reference) {
                self.recursive = true;
            }
            if let Some(target) = target {
                self.chain.push(reference);
                self.gather(target, depth + 1, above);
            }
        }
        for alternatives in ["anyOf", "oneOf", "allOf"] {
            for alternative in node
                .get(alternatives)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.gather(alternative, depth + 1, above);
            }
        }
        let says = [
            "type",
            "properties",
            "additionalProperties",
            "items",
            "enum",
            "const",
        ]
        .iter()
        .any(|k| node.get(*k).is_some());
        let only_null = node.get("type").and_then(Value::as_str) == Some("null");
        let refers = ["$ref", "anyOf", "oneOf", "allOf"]
            .iter()
            .any(|k| node.get(*k).is_some());
        if (says && !only_null) || (!says && !refers) {
            self.nodes.push(node);
        }
    }

    /// The place at `key` below this one: a declared key, or any key of a
    /// map whose keys the writer chooses.
    pub(crate) fn child(&self, key: &str) -> Option<At<'a>> {
        let declared: Vec<&Value> = self
            .nodes
            .iter()
            .filter_map(|n| n.get("properties")?.get(key))
            .collect();
        let found = match declared.is_empty() {
            false => declared,
            true => self.free(),
        };
        (!found.is_empty()).then(|| Self::of(self.root, &found, self.chain.clone()))
    }

    /// The property a bare value on this record's own flag sets
    /// (`x-link-shorthand`), and its place: `engine` takes `v8`
    /// as its `name`.
    pub(crate) fn shorthand(&self) -> Option<(&'a str, At<'a>)> {
        let key = self.nodes.iter().find_map(|n| n.get(SHORTHAND)?.as_str())?;
        Some((key, self.child(key)?))
    }

    /// The schemas of a free key's value, where keys are the writer's.
    fn free(&self) -> Vec<&'a Value> {
        self.nodes
            .iter()
            .filter_map(|n| open_or(n.get("additionalProperties")?))
            .collect()
    }

    /// The declared keys below, in order.
    pub(crate) fn keys(&self) -> BTreeSet<&'a str> {
        self.nodes
            .iter()
            .filter_map(|n| n.get("properties").and_then(Value::as_object))
            .flat_map(|p| p.keys().map(String::as_str))
            .collect()
    }

    /// An unconditional required key of one concrete object shape. Flattened
    /// alternative objects and obligations on other nodes stay outside this proof.
    pub(crate) fn required(&self, key: &str) -> bool {
        self.nodes.len() == 1
            && self.nodes[0]
                .get("properties")
                .is_some_and(|p| p.get(key).is_some())
            && self.nodes[0]
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|keys| keys.iter().any(|name| name.as_str() == Some(key)))
    }

    /// The JSON types a value here may have; an open schema (`{}`) any.
    pub(crate) fn types(&self) -> Option<BTreeSet<&'a str>> {
        let mut out = BTreeSet::new();
        for node in &self.nodes {
            let said = types_of(node);
            if said.is_empty() {
                return None;
            }
            out.extend(said);
        }
        Some(out)
    }

    /// Whether a value here may be `value`, by its type and any closed
    /// list of values.
    pub(crate) fn accepts(&self, value: &toml::Value) -> bool {
        let ty = match value {
            toml::Value::String(_) => "string",
            // No shape takes a TOML date, so date-like text is text.
            toml::Value::Datetime(_) => "datetime",
            toml::Value::Integer(_) => "integer",
            toml::Value::Float(_) => "number",
            toml::Value::Boolean(_) => "boolean",
            toml::Value::Array(_) => "array",
            toml::Value::Table(_) => "object",
        };
        let typed = self.nodes.iter().any(|node| {
            let said = types_of(node);
            let typed = said.is_empty()
                || said.contains(ty)
                || (ty == "integer" && said.contains("number"));
            typed && closed(node).is_none_or(|values| values.iter().any(|v| same(v, value)))
        });
        typed
            && match value {
                toml::Value::Array(items) => self
                    .element()
                    .is_none_or(|element| items.iter().all(|item| element.accepts(item))),
                _ => true,
            }
    }

    /// Whether bare text is a value here: a string, or one of its words.
    pub(crate) fn takes_text(&self, text: &str) -> bool {
        self.accepts(&toml::Value::String(text.to_string()))
    }

    /// Whether a value here is a secret's (`x-link-sensitive`), which a
    /// field flag never takes.
    /// A table or list given whole holds every value below it, so a
    /// secret anywhere there counts.
    pub(crate) fn sensitive(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| secret::holds_sensitive(self.root, node, 0))
    }

    /// Whether this is a switch, a flag given alone meaning `true`.
    pub(crate) fn switch(&self) -> bool {
        self.types()
            .is_some_and(|t| t.len() == 1 && t.contains("boolean"))
    }

    /// The place of a list's elements, where a value here is a list.
    pub(crate) fn element(&self) -> Option<At<'a>> {
        let items: Vec<&Value> = self
            .nodes
            .iter()
            .filter_map(|n| open_or(n.get("items")?))
            .collect();
        (!items.is_empty()).then(|| Self::of(self.root, &items, self.chain.clone()))
    }

    /// Whether a value here is a table with keys of its own.
    fn table(&self) -> bool {
        !self.keys().is_empty() || !self.free().is_empty()
    }

    /// Whether a value here may be other than a table of named keys.
    fn value(&self) -> bool {
        match self.types() {
            None => true,
            Some(types) => types.iter().any(|t| *t != "object") || !self.table(),
        }
    }

    /// Whether a value here is a path, resolved from its own source.
    pub(crate) fn path(&self) -> bool {
        self.nodes.iter().any(|n| {
            matches!(
                n.get("format").and_then(Value::as_str),
                Some(PATH_FORMAT | crate::CONFINED_PATH_FORMAT)
            )
        })
    }

    pub(crate) fn confined(&self) -> bool {
        self.nodes.iter().any(|node| {
            node.get("format").and_then(Value::as_str) == Some(crate::CONFINED_PATH_FORMAT)
        })
    }

    /// Its doc comment, whole.
    pub(crate) fn doc(&self) -> Option<&'a str> {
        self.doc
    }

    /// Whether a definition here is one of its own ancestors.
    pub(crate) fn recursive(&self) -> bool {
        self.recursive
    }

    /// Why a value here is refused in this build, where its schema says
    /// (`x-link-unavailable`): it's in the shape, and no example holds it.
    pub(crate) fn unavailable(&self) -> Option<&'a str> {
        self.nodes
            .iter()
            .find_map(|n| n.get("x-link-unavailable").and_then(Value::as_str))
    }

    /// The definitions (`$defs`) this place itself is, by name, not its
    /// ancestors'.
    pub(crate) fn definitions(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.chain[self.own.min(self.chain.len())..]
            .iter()
            .filter_map(|r| r.strip_prefix("#/$defs/"))
    }

    /// The definitions this place itself is, each its schema.
    pub(crate) fn definition_nodes(&self) -> impl Iterator<Item = &'a Value> + '_ {
        let defs = self.root.get("$defs");
        self.definitions()
            .filter_map(move |name| defs.and_then(|d| d.get(name)))
    }

    /// The place of a free key's value, where the writer chooses keys.
    pub(crate) fn free_at(&self) -> Option<At<'a>> {
        let free = self.free();
        (!free.is_empty()).then(|| Self::of(self.root, &free, self.chain.clone()))
    }

    /// Its default, where its schema gives one.
    pub(crate) fn default_value(&self) -> Option<&'a Value> {
        self.default
    }

    /// The first example its schema gives.
    pub(crate) fn example_value(&self) -> Option<&'a Value> {
        self.example
    }

    pub(crate) fn sample_values(&self) -> &[&'a Value] {
        &self.samples
    }

    /// The first example written on this place itself, where it has one:
    /// a field's, never only its type's.
    pub(crate) fn own_example(&self) -> Option<&'a Value> {
        self.own_example
    }

    /// The doc comment written on this place itself, where it has one:
    /// a field's, never only its type's.
    pub(crate) fn own_doc(&self) -> Option<&'a str> {
        self.own_doc
    }

    /// Its variants, where a value here is one of several tables (a
    /// tagged enum), each alone; none where it is one table.
    pub(crate) fn variants(&self) -> Vec<At<'a>> {
        let tables: Vec<&'a Value> = self
            .nodes
            .iter()
            .copied()
            .filter(|n| n.get("properties").is_some())
            .collect();
        match tables.len() {
            0 | 1 => Vec::new(),
            _ => tables
                .into_iter()
                .map(|n| Self::of(self.root, &[n], self.chain.clone()))
                .collect(),
        }
    }

    /// Whether a value here is a table with keys of its own.
    pub(crate) fn is_table(&self) -> bool {
        self.table()
    }

    /// A value of its type, for an example where its schema gives none:
    /// its first word, its least number, `true`, or text.
    pub(crate) fn first_value(&self) -> Option<toml::Value> {
        if let Some(word) = self
            .nodes
            .iter()
            .filter_map(|n| closed(n))
            .flatten()
            .find_map(|v| toml::Value::try_from(v).ok())
        {
            return Some(word);
        }
        let types = self.types()?;
        let least = self
            .nodes
            .iter()
            .find_map(|n| n.get("minimum").and_then(Value::as_i64))
            .unwrap_or(1)
            .max(1);
        if types.contains("string") {
            Some(toml::Value::String("example".into()))
        } else if types.contains("integer") {
            Some(toml::Value::Integer(least))
        } else if types.contains("number") {
            Some(toml::Value::Float(least as f64))
        } else if types.contains("boolean") {
            Some(toml::Value::Boolean(true))
        } else {
            None
        }
    }

    /// What a value here is, in words a reference page shows.
    pub(crate) fn described(&self) -> String {
        if self.switch() {
            return format!("{TICK}true{TICK} or {TICK}false{TICK}");
        }
        if let Some(element) = self.element() {
            return format!("a list, each {}", element.described());
        }
        let words: Vec<String> = self
            .nodes
            .iter()
            .filter_map(|n| closed(n))
            .flatten()
            .filter_map(|v| v.as_str().map(|w| format!("{TICK}{w}{TICK}")))
            .collect();
        let mut forms = Vec::new();
        if !words.is_empty() {
            forms.push(format!("one of {}", words.join(", ")));
        }
        let Some(types) = self.types() else {
            return "any value".into();
        };
        let open_text = self
            .nodes
            .iter()
            .any(|n| closed(n).is_none() && types_of(n).contains("string"));
        if types.contains("string") && (words.is_empty() || open_text) {
            forms.push("text".into());
        }
        if types.contains("integer") {
            forms.push(
                match self
                    .nodes
                    .iter()
                    .find_map(|n| n.get("minimum").and_then(Value::as_i64))
                {
                    Some(least) => format!("a whole number from {least}"),
                    None => "a whole number".into(),
                },
            );
        } else if types.contains("number") {
            forms.push("a number".into());
        }
        if types.contains("boolean") {
            forms.push(format!("{TICK}true{TICK} or {TICK}false{TICK}"));
        }
        if types.contains("object") {
            forms.push(match self.free_at() {
                Some(free) if self.keys().is_empty() => {
                    format!("a table by name, each {}", free.described())
                }
                _ => "a table".into(),
            });
        }
        match forms.is_empty() {
            true => "any value".into(),
            false => forms.join(", or "),
        }
    }

    /// What each of its words means, in declared order, where its schema
    /// says (`x-link-values`): a list's, its elements' words.
    pub(crate) fn meanings(&self) -> Vec<(&'a str, &'a str)> {
        if let Some(element) = self.element() {
            return element.meanings();
        }
        let mut out = Vec::new();
        for node in self.nodes.iter().copied() {
            let Some(values) = node.get("x-link-values").and_then(Value::as_object) else {
                continue;
            };
            for word in closed(node).into_iter().flatten().filter_map(Value::as_str) {
                if let Some(meaning) = values.get(word).and_then(Value::as_str) {
                    out.push((word, meaning));
                }
            }
        }
        out
    }

    /// Its doc comment's first paragraph, its help line.
    fn summary(&self) -> Option<String> {
        let doc = self.doc?;
        let first = doc.split("\n\n").next().unwrap_or(doc);
        Some(first.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    /// How a value is written, for help: `<off|low>`, `<text>`,
    /// `'{ … }'`, nothing for a switch.
    fn hint(&self) -> String {
        if self.switch() {
            return String::new();
        }
        if let Some(element) = self.element() {
            return match element.element() {
                Some(_) => "'[ … ]'...".into(),
                None => format!("{}...", element.hint()),
            };
        }
        let words: Vec<String> = self
            .nodes
            .iter()
            .filter_map(|n| closed(n))
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let Some(types) = self.types() else {
            return "<value>".into();
        };
        let mut forms = Vec::new();
        if !words.is_empty() {
            forms.push(format!("<{}>", words.join("|")));
        }
        for (ty, form) in [
            ("string", "<text>"),
            ("integer", "<integer>"),
            ("number", "<number>"),
            ("boolean", "<true|false>"),
            ("object", "'{ … }'"),
        ] {
            let worded = ty == "string" && !words.is_empty();
            let open_string = ty == "string"
                && self
                    .nodes
                    .iter()
                    .any(|n| closed(n).is_none() && types_of(n).contains("string"));
            if types.contains(ty) && (!worded || open_string) {
                forms.push(form.into());
            }
        }
        match forms.is_empty() {
            true => "<value>".into(),
            false => forms.join("|"),
        }
    }
}

/// Any value at all, the schema `true` is.
static OPEN: std::sync::LazyLock<Value> =
    std::sync::LazyLock::new(|| Value::Object(Default::default()));

/// A schema given where JSON Schema takes `true` for any value: `true` as
/// the open schema, a schema as itself, `false` as none.
fn open_or(node: &Value) -> Option<&Value> {
    match node {
        Value::Bool(true) => Some(&OPEN),
        Value::Object(_) => Some(node),
        _ => None,
    }
}

/// The JSON types one schema says, from its `type` or what it holds.
fn types_of(node: &Value) -> BTreeSet<&str> {
    let mut out: BTreeSet<&str> = match node.get("type") {
        Some(Value::String(t)) => [t.as_str()].into(),
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => BTreeSet::new(),
    };
    out.remove("null");
    if out.is_empty() {
        if node.get("properties").is_some() || node.get("additionalProperties").is_some() {
            out.insert("object");
        }
        if node.get("items").is_some() {
            out.insert("array");
        }
        for value in closed(node).unwrap_or_default() {
            out.insert(match value {
                Value::String(_) => "string",
                Value::Bool(_) => "boolean",
                Value::Number(n) if n.is_f64() => "number",
                Value::Number(_) => "integer",
                _ => continue,
            });
        }
    }
    out
}

/// A schema's closed list of values: its `enum`, or its one `const`.
fn closed(node: &Value) -> Option<Vec<&Value>> {
    if let Some(values) = node.get("enum").and_then(Value::as_array) {
        return Some(values.iter().filter(|v| !v.is_null()).collect());
    }
    node.get("const").map(|v| vec![v])
}

/// Whether a schema's JSON value is the TOML value given.
fn same(json: &Value, value: &toml::Value) -> bool {
    match (json, value) {
        (Value::String(a), toml::Value::String(b)) => a == b,
        (Value::Bool(a), toml::Value::Boolean(b)) => a == b,
        (Value::Number(a), toml::Value::Integer(b)) => a.as_i64() == Some(*b),
        (Value::Number(a), toml::Value::Float(b)) => a.as_f64() == Some(*b),
        _ => false,
    }
}

/// One field's flag, as help and suggestions list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// The key path, a free key written `<name>`.
    pub path: Vec<String>,
    /// The flag without its dashes: `os`, `model.thinking`, `spec.version`.
    pub flag: String,
    /// How its value is written: `<off|low>`, `'{ … }'...`.
    pub hint: String,
    /// Its doc comment's first paragraph.
    pub summary: Option<String>,
}

/// The flag of the field at `path`: its key path, quoted where TOML
/// quotes a key, and `spec.` before the spec's own properties.
pub fn flag_of(path: &[String]) -> String {
    let keys: Vec<String> = path
        .iter()
        .map(|k| match k.as_str() {
            FREE_KEY => k.clone(),
            _ => toml_edit::Key::new(k.as_str()).display_repr().into_owned(),
        })
        .collect();
    match keys.as_slice() {
        [only] if SPEC_OWN.contains(&only.as_str()) => format!("{TOP}.{only}"),
        _ => keys.join("."),
    }
}

/// Every field flag of the kind whose shape is `schema`: each value a
/// file can hold, by its key path, a free key's as `<name>`. A list's
/// elements take no flags of their own, since they have no stable names:
/// its flag takes whole elements.
pub fn fields(schema: &Value) -> Vec<Field> {
    let mut out = Vec::new();
    walk(&At::top(schema), &mut Vec::new(), None, &mut out);
    out
}

/// `inherited` is the doc comment of the map a free key's value is in,
/// which the value says where it says nothing of its own.
fn walk(at: &At<'_>, path: &mut Vec<String>, inherited: Option<String>, out: &mut Vec<Field>) {
    if path.len() > MAX_DEPTH {
        return;
    }
    let reserved = path.len() == 1 && RESERVED.contains(&path[0].as_str());
    if !path.is_empty() && !reserved && (at.value() || at.recursive) {
        out.push(Field {
            path: path.clone(),
            flag: flag_of(path),
            hint: at.hint(),
            summary: at.summary().or(inherited),
        });
    } else if !path.is_empty()
        && !reserved
        && let Some((_, short)) = at.shorthand()
    {
        // A record's own flag takes the bare value its shorthand names.
        out.push(Field {
            path: path.clone(),
            flag: flag_of(path),
            hint: short.hint(),
            summary: at.summary().or(short.summary()).or(inherited),
        });
    }
    if at.recursive || reserved || at.element().is_some() {
        return;
    }
    for key in at.keys() {
        if let Some(child) = at.child(key) {
            path.push(key.to_string());
            walk(&child, path, None, out);
            path.pop();
        }
    }
    let free = at.free();
    if !free.is_empty() {
        let child = At::of(at.root, &free, at.chain.clone());
        path.push(FREE_KEY.to_string());
        walk(&child, path, at.summary(), out);
        path.pop();
    }
}

/// Whether `flag`, as `--os`, `--machine.gcp.size` or `--spec.name`, names a
/// field of `schema`, and if so whether it takes a value, as the field
/// flags' reader resolves it. It reaches every key a file can hold,
/// below a type within itself too, which [`fields`] lists only as far as
/// that type's own flag.
pub fn flag_takes(schema: &Value, flag: &str) -> Option<bool> {
    let keys: Vec<String> = flag
        .strip_prefix("--")?
        .split('.')
        .map(str::to_string)
        .collect();
    let path = match keys.as_slice() {
        [top, only] if top == TOP && SPEC_OWN.contains(&only.as_str()) => vec![only.clone()],
        [top, ..] if top == TOP => return None,
        [only] if SPEC_OWN.contains(&only.as_str()) => return None,
        _ => keys,
    };
    let at = resolve(schema, &path).ok()?;
    let at = match at.shorthand() {
        Some((_, short)) if !at.value() => short,
        _ => at,
    };
    Some(!at.hint().is_empty())
}

/// The field flags of `schema` as help lists them after the runtime
/// flags: each flag, how its value is written and its doc comment.
pub fn help(schema: &Value) -> String {
    let mut out = String::from("Spec fields, each its key path in the spec file:\n");
    for field in fields(schema) {
        let flag = match field.hint.as_str() {
            "" => format!("--{}", field.flag),
            hint => format!("--{} {hint}", field.flag),
        };
        out.push_str(&format!("  {flag}\n"));
        if let Some(summary) = field.summary {
            out.push_str(&format!("          {summary}\n"));
        }
    }
    out
}

/// `command` with its spec's field flags listed after its own, below
/// any help it has there already.
pub fn with_field_help(command: clap::Command, schema: &Value) -> clap::Command {
    let fields = help(schema);
    let after = match command.get_after_help() {
        Some(own) => format!("{own}\n\n{fields}"),
        None => fields,
    };
    command.after_help(after)
}

/// The place `path` names in `schema`, or why it names none, with the
/// nearest flag there is.
pub(crate) fn resolve<'a>(schema: &'a Value, path: &[String]) -> Result<At<'a>, String> {
    let shown = flag_of(path);
    if path.len() == 1 && RESERVED.contains(&path[0].as_str()) {
        return Err(format!(
            "--{shown}: {} is the file's format, never a field, so it takes no flag",
            path[0]
        ));
    }
    let mut at = At::top(schema);
    for key in path {
        at = match at.child(key) {
            Some(child) => child,
            None => return Err(unknown(schema, path)),
        };
    }
    Ok(at)
}

/// Every problem with how `schema` spells its fields as flags: a
/// top-level field named `spec`, which would make that prefix
/// ambiguous, or `kind` or `schema`, the file's format.
pub fn check_spelling(schema: &Value) -> Vec<String> {
    At::top(schema)
        .keys()
        .into_iter()
        .filter(|k| *k == TOP || RESERVED.contains(k))
        .map(|k| format!("the top-level field {k} is reserved: rename it"))
        .collect()
}

/// Every key `schema` declares that no field flag reaches: none, since
/// flags are generated from the same shape. A key inside a list of
/// records is reached by its list's flag, which takes whole elements, and
/// one inside a type within itself by that type's flag. This holds the
/// flag walk to the shape walk ([`crate::declared`]) for every kind.
pub fn uncovered(schema: &Value) -> Vec<String> {
    // Paths compared as the shape walk writes them, a free key as `*`.
    let flags: Vec<String> = fields(schema)
        .into_iter()
        .map(|f| {
            f.path
                .iter()
                .map(|k| if k == FREE_KEY { "*" } else { k.as_str() })
                .collect::<Vec<_>>()
                .join(".")
        })
        .collect();
    let under = |path: &str, above: &str| path == above || path.starts_with(&format!("{above}."));
    let declared = match crate::declared(schema) {
        Ok(declared) => declared,
        Err(e) => return vec![e],
    };
    let mut out = Vec::new();
    // Every flag listed is one the split takes, a free key's with a key.
    for field in fields(schema) {
        let path: Vec<String> = field
            .path
            .iter()
            .map(|k| match k.as_str() {
                FREE_KEY => "k".to_string(),
                k => k.to_string(),
            })
            .collect();
        if let Err(e) = resolve(schema, &path) {
            out.push(format!("--{} is listed, but {e}", field.flag));
        }
    }
    for (table, keys) in &declared.fields {
        for key in keys {
            let path = match table.as_str() {
                "" => key.clone(),
                table => format!("{table}.{key}"),
            };
            let reserved = table.is_empty() && RESERVED.contains(&key.as_str());
            let reached = flags.iter().any(|f| under(&path, f) || under(f, &path));
            if !reached && !reserved {
                out.push(format!("{path} has no flag"));
            }
        }
    }
    out
}

/// Every key and value in `table` that `schema` doesn't take, each by its
/// path: what a file is checked against with no code from its owner,
/// as `lnk spec convert` checks what it writes. The owner's reader may
/// refuse more, such as a value out of its range.
pub fn check_value(schema: &Value, table: &toml::Table) -> Vec<String> {
    let mut body = table.clone();
    for key in RESERVED {
        body.remove(*key);
    }
    let mut out = Vec::new();
    check_at(
        &At::top(schema),
        &toml::Value::Table(body),
        &mut Vec::new(),
        &mut out,
    );
    out
}

fn check_at(at: &At<'_>, value: &toml::Value, path: &mut Vec<String>, out: &mut Vec<String>) {
    // Each schema a value may take, tried alone: it's good where one
    // takes it whole, and otherwise the nearest one says why not.
    let mut nearest: Option<Vec<String>> = None;
    for node in &at.nodes {
        let one = At::of(at.root, &[node], at.chain.clone());
        let mut problems = Vec::new();
        check_one(&one, node, value, path, &mut problems);
        if problems.is_empty() {
            return;
        }
        if nearest.as_ref().is_none_or(|n| problems.len() < n.len()) {
            nearest = Some(problems);
        }
    }
    out.extend(nearest.unwrap_or_default());
}

fn check_one(
    at: &At<'_>,
    node: &Value,
    value: &toml::Value,
    path: &mut Vec<String>,
    out: &mut Vec<String>,
) {
    let shown = |path: &[String]| path.join(".");
    if !at.accepts(value) {
        out.push(format!("{} can't be {value}", shown(path)));
        return;
    }
    match value {
        toml::Value::Table(table) => {
            for key in node
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !table.contains_key(key) {
                    path.push(key.to_string());
                    out.push(format!("{} is missing", shown(path)));
                    path.pop();
                }
            }
            let closed = node.get("additionalProperties") == Some(&Value::Bool(false));
            for (key, inner) in table {
                path.push(key.clone());
                match at.child(key) {
                    Some(child) => check_at(&child, inner, path, out),
                    None if closed || at.table() => {
                        out.push(format!("{} is no field of this spec", shown(path)))
                    }
                    None => {}
                }
                path.pop();
            }
        }
        toml::Value::Array(items) => {
            if let Some(element) = at.element() {
                for item in items {
                    check_at(&element, item, path, out);
                }
            }
        }
        toml::Value::Integer(_) | toml::Value::Float(_) => {
            let number = match value {
                toml::Value::Integer(n) => *n as f64,
                toml::Value::Float(f) => *f,
                _ => unreachable!("matched above"),
            };
            let below = node
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|m| number < m);
            let above = node
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|m| number > m);
            if below || above {
                out.push(format!("{} can't be {value}: out of range", shown(path)));
            }
        }
        _ => {}
    }
}
