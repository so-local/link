//! A spec kind's declared shape: the JSON Schema its type derives with
//! `schemars`, walked into the same dotted tables the serde probe finds
//! ([`crate::probe`]), and the check that the two agree.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::probe::{self, Expand, Fields};

/// A spec kind Link reads from a file and a command line: a
/// [`Specification`](crate::Specification) whose shape is declared, by
/// deriving `schemars::JsonSchema` or by hand for a type read by hand.
/// [`crate::specification!`] registers one, with `check = <fn>` for what
/// its type alone can't refuse.
pub trait Kind: crate::Specification + schemars::JsonSchema {
    /// What the type alone can't refuse, such as a name's form or values
    /// held to each other: every read of the kind with its field flags
    /// (`read_with`, `read_text_with`, `over`) and every `spec show`
    /// applies it, so a printed spec is one its commands take back.
    fn check(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A spec kind as the module owning it exports it in its description:
/// its name, current schema, print command, steps and shape. Every
/// installed module's kinds are what `lnk spec` lists, shows and
/// converts, with no code from the module.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exported {
    pub kind: String,
    pub schema: u32,
    /// The oldest schema its steps read, 1 unless steps were retired.
    #[serde(default = "first", skip_serializing_if = "is_first")]
    pub oldest: u32,
    /// The command printing it in its current schema, where one comes
    /// with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// What the kind configures, its reference page's opening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<crate::schema::Step>,
    /// Its shapes that predate Link 0.0.1, each refused by name.
    #[serde(default, skip_serializing_if = "crate::schema::Before::is_empty")]
    pub before: crate::schema::Before,
    /// Its JSON Schema, without the format envelope ([`schema`]).
    pub shape: Value,
}

impl Exported {
    /// Kind `T`, as its module exports it.
    pub fn of<T: Kind>() -> Self {
        let format = T::format();
        Exported {
            kind: format.kind.clone(),
            schema: format.current,
            oldest: format.oldest,
            command: format.command.clone(),
            about: format.about.clone(),
            steps: format.steps.clone(),
            before: format.before.clone(),
            shape: schema::<T>(),
        }
    }

    /// Its format, whose steps read its older files.
    pub fn format(&self) -> crate::schema::Format {
        crate::schema::Format {
            kind: self.kind.clone(),
            current: self.schema,
            oldest: self.oldest,
            command: self.command.clone(),
            schema: crate::schema::DECLARATION,
            steps: self.steps.clone(),
            about: self.about.clone(),
            before: self.before.clone(),
            // A spec is a person's: never converted where it lies, and
            // never given records by a transfer.
            holds: Default::default(),
            files: Vec::new(),
            transfers: Vec::new(),
            named: true,
            migrations: &[],
            references: crate::follow::sites(&self.shape),
        }
    }
}

fn first() -> u32 {
    1
}

fn is_first(schema: &u32) -> bool {
    *schema == 1
}

/// Gives each variant's `kind` tag, in a tagged enum's schema, `kinds`, the
/// enum's doc of its kinds: a transform (`#[schemars(transform = ...)]`)
/// for a field whose variants' docs don't say what its tag is.
pub fn kind_doc(schema: &mut schemars::Schema, kinds: &str) {
    for list in ["oneOf", "anyOf"] {
        let Some(variants) = schema.get_mut(list).and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for variant in variants {
            if let Some(kind) = variant
                .get_mut("properties")
                .and_then(|p| p.get_mut("kind"))
                .and_then(|k| k.as_object_mut())
            {
                kind.insert("description".into(), kinds.into());
            }
        }
    }
}

/// The JSON Schema of `T`'s configurable type, without the format
/// marker: what flags, help and references are made from.
pub fn schema<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("a schema is JSON")
}

/// The tables a schema declares, by the paths [`probe::discover`] uses,
/// and every schema node met at each path (a list's and its element's
/// both at the list's path), where a hand-declared schema's examples are.
#[derive(Debug, Default)]
pub struct Declared<'a> {
    pub fields: Fields,
    pub nodes: BTreeMap<String, Vec<&'a Value>>,
}

/// Walk `root`, a whole JSON Schema with its `$defs`.
pub fn declared(root: &Value) -> Result<Declared<'_>, String> {
    let mut out = Declared::default();
    // The top is `#`, the name a type inside itself refers to it by.
    walk(root, root, &mut Vec::new(), &mut vec!["#".into()], &mut out)?;
    for keys in out.fields.values_mut() {
        keys.sort();
        keys.dedup();
    }
    Ok(out)
}

fn walk<'a>(
    root: &'a Value,
    node: &'a Value,
    path: &mut Vec<String>,
    ancestors: &mut Vec<String>,
    out: &mut Declared<'a>,
) -> Result<(), String> {
    if path.len() > crate::constants::MAX_DEPTH {
        return Err(format!("{}: the schema nests too deep", path.join(".")));
    }
    let at = path.join(".");
    out.nodes.entry(at.clone()).or_default().push(node);
    if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
        let (name, target) = match reference {
            "#" => ("#", Some(root)),
            _ => {
                let name = reference
                    .strip_prefix("#/$defs/")
                    .ok_or_else(|| format!("{at}: {reference} isn't a local definition"))?;
                (name, root.get("$defs").and_then(|defs| defs.get(name)))
            }
        };
        let target = target.ok_or_else(|| format!("{at}: {reference} isn't defined"))?;
        if ancestors.iter().any(|a| a == name) {
            // A type inside itself: its keys listed, not expanded again,
            // as the probe does.
            if let Some(properties) = target.get("properties").and_then(Value::as_object) {
                out.fields
                    .entry(at)
                    .or_default()
                    .extend(properties.keys().cloned());
            }
            return Ok(());
        }
        ancestors.push(name.to_string());
        let walked = walk(root, target, path, ancestors, out);
        ancestors.pop();
        walked?;
    }
    if let Some(properties) = node.get("properties").and_then(Value::as_object) {
        out.fields
            .entry(at.clone())
            .or_default()
            .extend(properties.keys().cloned());
        for (key, child) in properties {
            path.push(key.clone());
            let walked = walk(root, child, path, ancestors, out);
            path.pop();
            walked?;
        }
    }
    if let Some(free) = node.get("additionalProperties").filter(|v| v.is_object()) {
        path.push("*".into());
        let walked = walk(root, free, path, ancestors, out);
        path.pop();
        walked?;
    }
    if let Some(items) = node.get("items").filter(|v| v.is_object()) {
        walk(root, items, path, ancestors, out)?;
    }
    for alternatives in ["anyOf", "oneOf", "allOf"] {
        for alternative in node
            .get(alternatives)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match variant(alternative) {
                Some((name, inner)) => {
                    path.push(name.clone());
                    let walked = walk(root, inner, path, ancestors, out);
                    path.pop();
                    walked?;
                }
                None => walk(root, alternative, path, ancestors, out)?,
            }
        }
    }
    Ok(())
}

/// An externally tagged variant as `schemars` writes one: a table of
/// exactly the variant's name, holding its value. An internally tagged
/// unit variant looks the same but holds only its tag's constant, which
/// is a value, not a variant.
fn variant(alternative: &Value) -> Option<(&String, &Value)> {
    let properties = alternative.get("properties")?.as_object()?;
    let required = alternative.get("required")?.as_array()?;
    let closed = alternative.get("additionalProperties") == Some(&Value::Bool(false));
    match (
        properties.iter().next(),
        properties.len(),
        required.as_slice(),
    ) {
        (Some((name, inner)), 1, [only])
            if closed && only == name && inner.get("const").is_none() =>
        {
            Some((name, inner))
        }
        _ => None,
    }
}

/// Whether `example` holds the key at `rest`, the parts of a declared
/// key's path below the value read by hand: through any element of a
/// list, and any key for a `*`.
fn holds(example: &Value, rest: &[&str]) -> bool {
    let Some((first, more)) = rest.split_first() else {
        return true;
    };
    match example {
        Value::Array(items) => items.iter().any(|item| holds(item, rest)),
        Value::Object(map) if *first == "*" => map.values().any(|v| holds(v, more)),
        Value::Object(map) => map.get(*first).is_some_and(|v| holds(v, more)),
        _ => false,
    }
}

/// Whether `path` is `opaque` or inside it.
fn within(path: &str, opaque: &str) -> bool {
    opaque.is_empty() || path == opaque || path.starts_with(&format!("{opaque}."))
}

/// Every disagreement between `T`'s declared shape and the tables its
/// reader takes, each naming its path, or none. Where the reader is
/// written by hand, the schema there declares `examples`, the reader
/// takes each one at that path, and every key the schema declares there
/// is in one of them, so a key the reader no longer takes fails.
pub fn check_shape<T: schemars::JsonSchema + serde::de::DeserializeOwned>() -> Vec<String> {
    let schema = schema::<T>();
    let mut problems = Vec::new();
    let declared = match declared(&schema) {
        Ok(declared) => declared,
        Err(e) => return vec![e],
    };
    let found = match probe::discover::<T>(&BTreeMap::new(), Expand::Everywhere) {
        Ok(found) => found,
        Err(e) => return vec![e],
    };
    let shown = |path: &str| match path {
        "" => "the top".to_string(),
        _ => path.to_string(),
    };
    for (path, keys) in &found.fields {
        match declared.fields.get(path) {
            Some(declared) if declared == keys => {}
            Some(declared) => problems.push(format!(
                "at {} the reader takes {keys:?}, but the schema declares {declared:?}",
                shown(path)
            )),
            None => problems.push(format!(
                "at {} the reader takes {keys:?}, which the schema doesn't declare",
                shown(path)
            )),
        }
    }
    for (path, keys) in &declared.fields {
        let read_by_hand = found.opaque.keys().any(|o| within(path, o));
        if !found.fields.contains_key(path) && !read_by_hand {
            problems.push(format!(
                "at {} the schema declares {keys:?}, which the reader doesn't take",
                shown(path)
            ));
        }
    }
    for (path, why) in &found.opaque {
        let examples: Vec<&Value> = declared
            .nodes
            .get(path)
            .into_iter()
            .flatten()
            .filter_map(|node| node.get("examples").and_then(Value::as_array))
            .flatten()
            .collect();
        if examples.is_empty() {
            problems.push(format!(
                "at {}, {why}: declare its schema with examples it reads",
                shown(path)
            ));
        }
        for example in &examples {
            if let Err(e) = probe::read_at::<T>(path, example) {
                problems.push(format!(
                    "at {} the schema's example {example} is refused by the reader: {e}",
                    shown(path)
                ));
            }
        }
        if examples.is_empty() {
            continue;
        }
        let depth = match path.as_str() {
            "" => 0,
            path => path.split('.').count(),
        };
        for (inner, keys) in declared.fields.iter().filter(|(p, _)| within(p, path)) {
            let below: Vec<&str> = match inner.as_str() {
                "" => Vec::new(),
                inner => inner.split('.').skip(depth).collect(),
            };
            for key in keys {
                let mut rest = below.clone();
                rest.push(key);
                if !examples.iter().any(|example| holds(example, &rest)) {
                    problems.push(format!(
                        "at {}, {why}: no example holds the declared {}",
                        shown(path),
                        rest.join(".")
                    ));
                }
            }
        }
    }
    let unique: BTreeSet<String> = problems.into_iter().collect();
    unique.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
    use serde::Deserialize;
    use std::borrow::Cow;

    #[expect(dead_code, reason = "only deserialized, for the shape it declares")]
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Derived {
        name: Option<String>,
        files: Option<Files>,
        #[serde(default)]
        stages: Vec<Files>,
        #[serde(default)]
        tool: BTreeMap<String, Files>,
        mode: Option<Mode>,
        inner: Option<Box<Derived>>,
    }

    #[expect(dead_code, reason = "only deserialized, for the shape it declares")]
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Files {
        #[serde(default)]
        write: Vec<String>,
    }

    #[expect(dead_code, reason = "only deserialized, for the shape it declares")]
    #[derive(Deserialize, JsonSchema)]
    #[serde(rename_all = "lowercase")]
    enum Mode {
        Off,
        Limited { hosts: Vec<String> },
    }

    #[test]
    fn a_derived_shape_agrees_with_its_reader_at_every_path() {
        assert_eq!(check_shape::<Derived>(), Vec::<String>::new());
        let schema = schema::<Derived>();
        let fields = declared(&schema).unwrap().fields;
        assert_eq!(fields["tool.*"], ["write"]);
        assert_eq!(fields["mode.limited"], ["hosts"]);
        // A type inside itself is listed, not expanded again.
        assert!(fields.contains_key("inner") && !fields.contains_key("inner.inner"));
    }

    /// A schema written by hand for a type whose reader is derived.
    #[expect(dead_code, reason = "only deserialized, for the shape it declares")]
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Stale {
        name: String,
        added: Option<bool>,
    }
    impl JsonSchema for Stale {
        fn schema_name() -> Cow<'static, str> {
            "Stale".into()
        }
        fn json_schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({"type": "object", "properties": {"name": {"type": "string"}, "gone": {"type": "string"}}})
        }
    }

    #[test]
    fn a_hand_declared_schema_that_disagrees_with_its_reader_fails() {
        let problems = check_shape::<Stale>();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains(
                r#"reader takes ["added", "name"], but the schema declares ["gone", "name"]"#
            ),
            "{problems:?}"
        );
    }

    /// A value read by hand from its text form, `"2..4"`.
    #[derive(Debug)]
    struct Range;
    impl<'de> Deserialize<'de> for Range {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let text = serde_json::Value::deserialize(d)?;
            match text.as_str().and_then(|t| t.split_once("..")) {
                Some(_) => Ok(Range),
                None => Err(serde::de::Error::custom("a range is low..high")),
            }
        }
    }
    struct Examples(&'static [&'static str]);
    impl Examples {
        fn schema(&self) -> Schema {
            match self.0 {
                [] => json_schema!({"type": "string"}),
                examples => json_schema!({"type": "string", "examples": examples}),
            }
        }
    }
    thread_local! {
        static EXAMPLES: std::cell::Cell<&'static [&'static str]> = const { std::cell::Cell::new(&[]) };
    }
    impl JsonSchema for Range {
        fn schema_name() -> Cow<'static, str> {
            "Range".into()
        }
        fn json_schema(_: &mut SchemaGenerator) -> Schema {
            Examples(EXAMPLES.get()).schema()
        }
    }
    #[expect(dead_code, reason = "only deserialized, for the shape it declares")]
    #[derive(Deserialize, JsonSchema)]
    struct Machine {
        cpus: Option<Range>,
        #[serde(default)]
        steps: Vec<Range>,
    }

    #[test]
    fn a_value_read_by_hand_declares_examples_its_reader_takes() {
        EXAMPLES.set(&["2..4"]);
        assert_eq!(check_shape::<Machine>(), Vec::<String>::new());
        EXAMPLES.set(&[]);
        let missing = check_shape::<Machine>();
        assert!(
            missing
                .iter()
                .any(|p| p.contains("at cpus,") && p.contains("declare its schema with examples")),
            "{missing:?}"
        );
        EXAMPLES.set(&["2..4", "four"]);
        let refused = check_shape::<Machine>();
        assert!(
            refused
                .iter()
                .any(|p| p.contains("at steps the") && p.contains("\"four\" is refused")),
            "{refused:?}"
        );
    }

    /// A table read by hand: its schema's keys are proved by examples.
    #[derive(Debug)]
    struct Window;
    impl<'de> Deserialize<'de> for Window {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let table = serde_json::Value::deserialize(d)?;
            let keys = table.as_object().into_iter().flat_map(|t| t.keys());
            match keys.into_iter().all(|k| k == "max" || k == "min") {
                true => Ok(Window),
                false => Err(serde::de::Error::custom("a window takes min and max")),
            }
        }
    }
    impl JsonSchema for Window {
        fn schema_name() -> Cow<'static, str> {
            "Window".into()
        }
        fn json_schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({
                "type": "object",
                "properties": {"min": {}, "max": {}, "gone": {}},
                "examples": tables()
            })
        }
    }
    thread_local! {
        static EXAMPLE_TABLES: std::cell::RefCell<Value> = const { std::cell::RefCell::new(Value::Null) };
    }
    fn tables() -> Value {
        EXAMPLE_TABLES.with(|v| v.borrow().clone())
    }
    #[expect(dead_code, reason = "only deserialized, for the shape it declares")]
    #[derive(Deserialize, JsonSchema)]
    struct Windowed {
        window: Option<Window>,
    }

    #[test]
    fn a_key_only_the_schema_has_inside_a_value_read_by_hand_fails() {
        EXAMPLE_TABLES.with(|v| *v.borrow_mut() = serde_json::json!([{"min": 1, "max": 2}]));
        let problems = check_shape::<Windowed>();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].starts_with("at window,")
                && problems[0].ends_with("no example holds the declared gone"),
            "{problems:?}"
        );
        EXAMPLE_TABLES.with(|v| *v.borrow_mut() = serde_json::json!([{"min": 1, "gone": 2}]));
        let refused = check_shape::<Windowed>();
        assert!(
            refused
                .iter()
                .any(|p| p.contains("is refused by the reader")),
            "{refused:?}"
        );
    }

    #[test]
    fn an_internally_tagged_unit_variant_is_a_value_not_a_table() {
        let alternative = serde_json::json!({
            "type": "object",
            "properties": {"kind": {"type": "string", "const": "auto"}},
            "required": ["kind"],
            "additionalProperties": false
        });
        assert!(variant(&alternative).is_none());
        let external = serde_json::json!({
            "type": "object",
            "properties": {"limited": {"type": "object"}},
            "required": ["limited"],
            "additionalProperties": false
        });
        assert_eq!(
            variant(&external).map(|(name, _)| name.as_str()),
            Some("limited")
        );
    }

    /// A reader that takes a table its schema leaves out.
    #[expect(dead_code, reason = "only deserialized, for the shape it declares")]
    #[derive(Deserialize)]
    struct Wider {
        files: Option<Files>,
    }
    impl JsonSchema for Wider {
        fn schema_name() -> Cow<'static, str> {
            "Wider".into()
        }
        fn json_schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({"type": "object", "properties": {"files": {"type": "object", "properties": {"write": {}, "read": {}}}, "extra": {"type": "object", "properties": {"a": {}}}}})
        }
    }

    #[test]
    fn a_table_only_one_side_has_is_named() {
        let problems = check_shape::<Wider>();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("at extra the schema declares")),
            "{problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("at files the reader takes [\"write\"]")),
            "{problems:?}"
        );
    }
}
