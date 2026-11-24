//! A JSON answer, as a person reads it: the answer another program gave
//! (an agent's action, a model's route) shown as blocks, each member by
//! its name, so only `--json` prints the JSON itself.

use serde_json::{Map, Value as Json};

use crate::{Align, Block, Column, Count, Fields, Foreign, List, Phrase, Section, Table, Value};
use crate::{Span, Verbatim, sentence};

/// The most items of a list of plain values shown in one field; a longer
/// one, such as an embedding's numbers, shows how many it holds.
const LISTED_MAX: usize = 16;

/// `answer` as blocks: an object's plain members as fields, its text of
/// more than one line, objects and lists of objects each under its name;
/// a list of flat objects as a table. Absent members (`null`) show
/// nothing.
pub fn blocks(answer: &Json) -> Vec<Block> {
    match answer {
        Json::Object(members) => object(members),
        Json::Array(items) => items_of(items),
        Json::String(text) => vec![lines(text).into()],
        Json::Null => Vec::new(),
        other => vec![Verbatim::new().line(plain(other)).into()],
    }
}

fn object(members: &Map<String, Json>) -> Vec<Block> {
    let mut fields = Fields::new();
    let mut sections = Vec::new();
    // By name, whatever order the map keeps: serde_json keeps its
    // input's order only when a crate in the build turns on
    // `preserve_order`, so the order is decided here.
    let mut members: Vec<(&String, &Json)> = members.iter().collect();
    members.sort_by_key(|(name, _)| *name);
    for (name, member) in members {
        match member {
            Json::Null => {}
            Json::String(text) if text.contains('\n') => {
                sections.push(Section::new(label(name)).with(lines(text)).into());
            }
            Json::Object(inner) if !inner.is_empty() => {
                sections.push(within(name, object(inner)));
            }
            Json::Array(items) if !items.iter().all(flat) => {
                sections.push(within(name, items_of(items)));
            }
            _ => fields = fields.row(label(name), cell(member)),
        }
    }
    let mut blocks = Vec::new();
    if !fields.is_empty() {
        blocks.push(fields.into());
    }
    blocks.extend(sections);
    blocks
}

/// A list of objects, as a table when each is flat; else each by its
/// place, from 1.
fn items_of(items: &[Json]) -> Vec<Block> {
    if let Some(table) = table(items) {
        return vec![table.into()];
    }
    if items.iter().all(flat) {
        let list = items
            .iter()
            .fold(List::new(sentence!("None.")), |list, i| list.value(cell(i)));
        return vec![list.into()];
    }
    items
        .iter()
        .enumerate()
        .map(|(n, item)| within(&(n + 1).to_string(), blocks(item)))
        .collect()
}

/// Flat objects, one a row, a column for each member any of them has.
fn table(items: &[Json]) -> Option<Table> {
    let rows: Vec<&Map<String, Json>> = items.iter().map(Json::as_object).collect::<Option<_>>()?;
    let one_line = |v: &Json| flat(v) && v.as_str().is_none_or(|text| !text.contains('\n'));
    if rows.is_empty() || !rows.iter().all(|row| row.values().all(one_line)) {
        return None;
    }
    let mut names: Vec<&String> = Vec::new();
    for row in &rows {
        for name in row.keys() {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    let columns = names
        .iter()
        .map(|name| Column {
            header: words(name),
            align: match rows
                .iter()
                .all(|r| r.get(*name).is_none_or(Json::is_number))
            {
                true => Align::Right,
                false => Align::Left,
            },
        })
        .collect();
    let rows = rows
        .iter()
        .map(|row| {
            names
                .iter()
                .map(|name| row.get(*name).map_or(Value::Text(String::new()), cell))
                .collect()
        })
        .collect();
    Some(Table {
        columns,
        rows,
        empty: sentence!("None."),
    })
}

/// What a field or a cell holds: a plain value, or a list of them (or of
/// lists of them, such as an embedding's vectors).
fn flat(value: &Json) -> bool {
    match value {
        Json::Object(members) => members.is_empty(),
        Json::Array(items) => items
            .iter()
            .all(|i| !i.is_object() && (!i.is_array() || flat(i))),
        _ => true,
    }
}

fn within(name: &str, blocks: Vec<Block>) -> Block {
    blocks
        .into_iter()
        .fold(Section::new(label(name)), Section::with)
        .into()
}

/// A plain value as a field or a cell shows it: text as it came, cleaned.
fn cell(value: &Json) -> Value {
    match value {
        Json::String(text) => Foreign::new(text).into(),
        Json::Array(items) if items.is_empty() => Value::Text("none".into()),
        Json::Array(items) if items.len() > LISTED_MAX => {
            Count::new(items.len() as u64, "item", "items").into()
        }
        Json::Array(items) => Value::List(items.iter().map(cell).collect()),
        Json::Object(members) if members.is_empty() => Value::Text("none".into()),
        other => Value::Text(plain(other)),
    }
}

fn plain(value: &Json) -> String {
    match value {
        Json::Bool(true) => "yes".into(),
        Json::Bool(false) => "no".into(),
        Json::Null => String::new(),
        other => other.to_string(),
    }
}

/// Text of its own, a line at a time, as it came.
fn lines(text: &str) -> Verbatim {
    text.trim_end_matches('\n')
        .split('\n')
        .fold(Verbatim::new(), |lines, line| lines.line(line))
}

/// A member's name as a label: `base_url` is `Base url`.
fn label(name: &str) -> Phrase {
    Phrase(vec![Span::Value(Value::Foreign(Foreign::new(&words(
        name,
    ))))])
}

fn words(name: &str) -> String {
    let spaced = name.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(answer: Json) -> String {
        blocks(&answer)
            .iter()
            .map(|b| crate::render::block(b, &crate::render::Ctx::plain()))
            .collect()
    }

    #[test]
    fn an_object_shows_its_members_by_name() {
        let said = shown(serde_json::json!({
            "ready": true, "loaded": ["qwen3:8b"], "base_url": "http://x", "gone": null,
        }));
        assert_eq!(
            said,
            "  Base url  http://x\n  Loaded    qwen3:8b\n  Ready     yes\n"
        );
    }

    #[test]
    fn nested_members_and_flat_lists_take_their_own_blocks() {
        let said = shown(serde_json::json!({
            "tokens": {"input": 3, "output": 4},
            "sent": [{"to": "owner", "id": "a1"}, {"to": "team", "id": "a2"}],
            "text": "one\ntwo",
        }));
        for part in [
            "Tokens", "Input", "Sent", "TO", "owner", "team", "Text", "one\n", "two\n",
        ] {
            assert!(said.contains(part), "{part}: {said}");
        }
        assert!(!said.contains('{'), "{said}");
    }

    #[test]
    fn a_long_list_shows_its_count() {
        let said = shown(serde_json::json!({"vectors": [vec![0.5; 40]]}));
        assert!(said.contains("40 items"), "{said}");
    }

    #[test]
    fn text_from_elsewhere_is_cleaned() {
        let said = shown(serde_json::json!({"said": "a\u{1b}[31mred"}));
        assert!(!said.contains('\u{1b}'), "{said}");
    }
}
