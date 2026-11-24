//! The steps' edits on a saved JSON object: paths of object keys, as a
//! TOML document's are of table keys ([`super::path`]).

use anyhow::Context;
use serde_json::{Map, Value};

use super::path::split_first;

pub(super) fn lookup<'a>(object: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    match split_first(path)? {
        (leaf, None) => object.get(leaf),
        (head, Some(rest)) => lookup(object.get(head)?.as_object()?, rest),
    }
}

pub(super) fn take(object: &mut Map<String, Value>, path: &str) -> Option<Value> {
    match split_first(path)? {
        (leaf, None) => object.remove(leaf),
        (head, Some(rest)) => take(object.get_mut(head)?.as_object_mut()?, rest),
    }
}

pub(super) fn put(object: &mut Map<String, Value>, path: &str, value: Value) -> anyhow::Result<()> {
    match split_first(path).with_context(|| format!("{path} isn't a path"))? {
        (leaf, None) => {
            object.insert(leaf.into(), value);
            Ok(())
        }
        (head, Some(rest)) => {
            let inner = object
                .entry(head)
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .with_context(|| format!("{head} is not an object"))?;
            put(inner, rest, value)
        }
    }
}
