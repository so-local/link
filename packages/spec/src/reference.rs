//! The parts of a spec kind's reference page that come from its shape,
//! which `link-actions-surfaces` writes in its one layout: Every Option
//! as a checked example ([`every_option_of`]), then an H2 per top-level
//! key and an H3 per field ([`key_sections`]), each opening with a quoted
//! line of its type, values, default and flag, and its description below.

use serde_json::Value;

use crate::Exported;
use crate::constants::{FREE_KEY, RESERVED, TICK};
use crate::fields::{At, flag_of};

/// A kind another kind holds whole: its own shape, which says where the
/// holder holds it, the kind it is, and its own page.
pub struct Composed<'a> {
    /// The held kind's own JSON Schema: the holder holds it where a
    /// definition has its fields, whatever that definition is named.
    pub shape: &'a Value,
    /// The kind it is (`agent`).
    pub kind: &'a str,
    /// Its page, as a link from the holding kind's page.
    pub page: &'a str,
}

/// A schema's own fields, the kind and schema keys aside.
fn fields_of(schema: &Value) -> std::collections::BTreeSet<&str> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|p| p.keys().map(String::as_str))
        .filter(|key| !RESERVED.contains(key))
        .collect()
}

/// The composed kind `at` is, where it's one of `composed`: one of its
/// definitions has exactly that kind's fields. A definition's name is
/// the schema generator's to choose, so it's never what's matched.
fn composed_at<'a>(at: &At<'_>, composed: &'a [Composed<'a>]) -> Option<&'a Composed<'a>> {
    at.definition_nodes().find_map(|node| {
        let fields = fields_of(node);
        composed
            .iter()
            .find(|c| !fields.is_empty() && fields == fields_of(c.shape))
    })
}

/// The kind's name as a title, `Agent Specification`, each word of a
/// hyphenated name capitalized (`Action Server Specification`), and an
/// abbreviation in capitals (`VM Specification`).
pub fn title(kind: &str) -> String {
    let mut words: Vec<String> = kind
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|word| {
            if crate::constants::ABBREVIATIONS.contains(&word) {
                return word.to_uppercase();
            }
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect();
    words.push("Specification".into());
    words.join(" ")
}

/// Each top-level key of a kind's shape, by its key, with its section's
/// body in the page's one layout: the key's quoted line of type, values,
/// default and flag, its description, then each field below it under an
/// H3, deeper fields deeper. The key's own heading, an H2, is the
/// caller's, so a generator splitting a page by key writes it.
pub fn key_sections(schema: &Value, composed: &[Composed<'_>]) -> Vec<(String, String)> {
    let at = At::top(schema);
    let mut out = Vec::new();
    for key in at.keys() {
        if RESERVED.contains(&key) {
            continue;
        }
        if let Some(child) = at.child(key) {
            let mut text = String::new();
            section(&child, &mut vec![key.to_string()], composed, &mut text);
            // Its own heading, the section's first line, is the caller's.
            let body = text
                .trim_start()
                .split_once('\n')
                .map(|(_, rest)| rest.trim_start().to_string())
                .unwrap_or_default();
            out.push((key.to_string(), body));
        }
    }
    out
}

/// Every Option for a kind named `kind` at `schema`, its format's
/// version, whose shape is `shape`: as [`every_option_text`] writes it.
pub fn every_option_of(kind: &str, schema: u32, shape: &Value) -> String {
    let fields = toml::to_string_pretty(&every_option(shape)).unwrap_or_default();
    format!("kind = {kind:?}\nschema = {schema}\n{fields}")
}

/// The segment a list's element takes in a key path, `service.[].name`:
/// an element has no name, so its fields take no flag.
pub const ELEMENT: &str = "[]";

/// A heading for the field at `path` and its entry, then its own fields a
/// level deeper, H2 for a top-level key down to H6. Below H4 a heading
/// shows the whole key path, and from H6 every deeper field is an H6
/// named by its path.
fn section(at: &At<'_>, path: &mut Vec<String>, composed: &[Composed<'_>], out: &mut String) {
    let depth = path.iter().filter(|k| *k != ELEMENT).count();
    let level = (depth + 1).min(6);
    let shown = match level {
        2..=4 => path.last().cloned().unwrap_or_default(),
        _ => path.join("."),
    };
    out.push_str(&format!("\n{} {TICK}{shown}{TICK}\n\n", "#".repeat(level)));
    out.push_str(&format!("> {}\n", line(at, path)));
    if let Some(doc) = at.own_doc().or(at.doc()) {
        out.push_str(&format!("\n{}\n", plain(doc.trim())));
    }
    // Each of its words with what it means, where its schema says.
    let meanings = at.meanings();
    if !meanings.is_empty() {
        out.push('\n');
        for (word, meaning) in meanings {
            out.push_str(&format!(
                "- {TICK}{word}{TICK}: {}\n",
                plain(meaning.trim())
            ));
        }
    }
    // A kind held whole is its own page's, linked rather than inlined.
    if let Some(kind) = composed_at(at, composed) {
        out.push_str(&format!(
            "\nIts fields are the [{}]({})'s, each a key below this one.\n",
            title(kind.kind),
            kind.page
        ));
        return;
    }
    children(at, path, &mut |child, path| {
        section(child, path, composed, out)
    });
}

/// Each place below `at`, with its path: its keys, a free key's value as
/// `<name>`, and a list of tables' element fields under `[]`. A type
/// inside itself is listed once, not walked again.
fn children(at: &At<'_>, path: &mut Vec<String>, visit: &mut dyn FnMut(&At<'_>, &mut Vec<String>)) {
    if at.recursive() || path.len() > crate::constants::MAX_DEPTH {
        return;
    }
    if let Some(element) = at.element() {
        if element.is_table() {
            path.push(ELEMENT.to_string());
            children(&element, path, visit);
            path.pop();
        }
        return;
    }
    for key in at.keys() {
        if let Some(child) = at.child(key) {
            path.push(key.to_string());
            visit(&child, path);
            path.pop();
        }
    }
    if let Some(free) = at.free_at() {
        path.push(FREE_KEY.to_string());
        visit(&free, path);
        path.pop();
    }
}

/// A doc comment as a page shows it: a rustdoc link, `` [`x`] `` with no
/// target, is its code alone.
fn plain(doc: &str) -> String {
    let mut out = String::new();
    let mut rest = doc;
    let (open, close) = (format!("[{TICK}"), format!("{TICK}]"));
    while let Some(at) = rest.find(&open) {
        let tail = &rest[at + 1..];
        match tail.find(&close) {
            Some(end) if !tail[end + 2..].starts_with('(') && !tail[end + 2..].starts_with('[') => {
                out.push_str(&rest[..at]);
                out.push_str(&tail[..end + 1]);
                rest = &tail[end + 2..];
            }
            _ => {
                out.push_str(&rest[..at + 2]);
                rest = &rest[at + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A field's quoted line: its type, its default where it has one, and its
/// flag.
fn line(at: &At<'_>, path: &[String]) -> String {
    let mut parts = vec![format!("**Type** {}", at.described())];
    if let Some(default) = at.default_value() {
        parts.push(format!("**Default** {TICK}{}{TICK}", toml_of(default)));
    }
    if let Some(why) = at.unavailable() {
        parts.push(format!("**Unavailable** {why}"));
    }
    match path.iter().position(|k| k == ELEMENT) {
        Some(list) => parts.push(format!(
            "**Flag** in {TICK}--{}{TICK}, whole elements",
            flag_of(&path[..list])
        )),
        None => parts.push(format!("**Flag** {TICK}--{}{TICK}", flag_of(path))),
    }
    parts.join(" · ")
}

/// A JSON default as TOML writes it.
fn toml_of(value: &Value) -> String {
    match toml::Value::try_from(value) {
        Ok(toml) => toml.to_string(),
        Err(_) => value.to_string(),
    }
}

/// Every Option as the file holds it: the kind and schema, then every
/// field with an example value.
pub fn every_option_text(kind: &Exported) -> String {
    every_option_of(&kind.kind, kind.schema, &kind.shape)
}

/// Every field of `schema` with an example value. A table holds each of
/// its fields; one of several tables (a variant) is its example where its
/// schema gives one, else its first variant; a value is its example, its
/// default, else its first value or a value of its type. A free key is
/// `name`, a list holds one element, and a type inside itself shows its
/// own fields once. A table whose field gives an example is that example.
pub fn every_option(schema: &Value) -> toml::Table {
    match example(&At::top(schema), 0, false) {
        Some(toml::Value::Table(table)) => table,
        _ => toml::Table::new(),
    }
}

fn given(value: Option<&Value>) -> Option<toml::Value> {
    value.and_then(|v| toml::Value::try_from(v).ok())
}

/// `inside` says the example is already within a type inside itself,
/// whose own fields it shows once, not again.
fn example(at: &At<'_>, depth: usize, inside: bool) -> Option<toml::Value> {
    if depth > crate::constants::MAX_DEPTH || (inside && at.recursive()) {
        return None;
    }
    let inside = inside || at.recursive();
    let variants = at.variants();
    if let Some(first) = variants.first() {
        return given(at.example_value()).or_else(|| example(first, depth, inside));
    }
    if at.is_table() {
        // A table whose keys depend on where it is (`[tools]` takes keys
        // `[tool.memory]` doesn't) is the example its field gives, whole.
        if let Some(whole) = given(at.own_example()) {
            return Some(whole);
        }
        let mut table = toml::Table::new();
        for key in at.keys() {
            if RESERVED.contains(&key) && depth == 0 {
                continue;
            }
            // A field this build refuses is in the shape, never in an
            // example its reader must take.
            if let Some(value) = at
                .child(key)
                .filter(|c| c.unavailable().is_none())
                .and_then(|c| example(&c, depth + 1, inside))
            {
                table.insert(key.to_string(), value);
            }
        }
        if let Some(value) = at.free_at().and_then(|f| example(&f, depth + 1, inside)) {
            table.insert("name".into(), value);
        }
        return (!table.is_empty()).then_some(toml::Value::Table(table));
    }
    if let Some(element) = at.element() {
        return given(at.example_value())
            .or_else(|| example(&element, depth + 1, inside).map(|v| toml::Value::Array(vec![v])));
    }
    given(at.example_value())
        .or_else(|| given(at.default_value()))
        .or_else(|| at.first_value())
}

#[cfg(test)]
mod tests;
