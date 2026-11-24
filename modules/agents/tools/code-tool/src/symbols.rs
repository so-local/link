//! Symbols found by name (`workspace/symbol`), for a definition or
//! references call that names a symbol rather than a place.

use std::sync::Arc;
use std::time::Instant;

use anyhow::bail;
use serde_json::{Value, json};

use crate::limits::limits;
use crate::lsp::{self, Lsp, capped};
use crate::{Code, chars_of, text_of};

/// A symbol found: the server that knows it, its file's URI and the
/// position of its name.
pub(crate) type Found = (Arc<Lsp>, String, Value);

/// Every symbol named `symbol`, asked of `lsps` (`workspace/symbol`), and
/// whether each server had settled; none is an error naming close ones.
pub(crate) fn named(
    code: &Code,
    lsps: &[Arc<Lsp>],
    symbol: &str,
    deadline: Instant,
) -> anyhow::Result<(Vec<Found>, bool)> {
    let mut close = Vec::new();
    let mut named: Vec<Found> = Vec::new();
    let mut settled = true;
    for lsp in lsps {
        settled &= lsp.settle(None, limits().settle_by(deadline));
        let found = lsp.request("workspace/symbol", json!({"query": symbol}), deadline)?;
        for item in found.as_array().into_iter().flatten() {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let (Some(uri), Some(start)) = (
                item.pointer("/location/uri").and_then(Value::as_str),
                item.pointer("/location/range/start"),
            ) else {
                continue;
            };
            if name != symbol {
                let name = capped(name, limits().line_max);
                if close.len() < limits().close_names_max && !close.contains(&name) {
                    close.push(name);
                }
                continue;
            }
            let position = name_at(code, uri, start, symbol).unwrap_or_else(|| start.clone());
            let seen = named
                .iter()
                .any(|(_, u, p)| u.as_str() == uri && *p == position);
            if !seen {
                named.push((lsp.clone(), uri.to_string(), position));
            }
        }
    }
    if !named.is_empty() {
        return Ok((named, settled));
    }
    match close.is_empty() {
        true => bail!("no symbol named {symbol} found"),
        false => bail!(
            "no symbol named {symbol} found; close ones:\n{}",
            lsp::server_said(&close.join(", "))
        ),
    }
}

/// The position of `name` itself, at or after `start` in `uri`'s file
/// (a declaration's range starts at its keyword): only in the files
/// folder.
fn name_at(code: &Code, uri: &str, start: &Value, name: &str) -> Option<Value> {
    let path = lsp::path_of(uri)?;
    let text = text_of(code, &path)?;
    let first = start.get("line")?.as_u64()? as usize;
    let character = start.get("character")?.as_u64()?;
    for (n, row) in text
        .lines()
        .enumerate()
        .skip(first)
        .take(limits().name_lines)
    {
        let from = match n == first {
            true => chars_of(row, character) as usize,
            false => 0,
        };
        let from_byte = row.char_indices().nth(from).map_or(row.len(), |(i, _)| i);
        let mut search = from_byte;
        while let Some(at) = row[search..].find(name) {
            let byte = search + at;
            let before = row[..byte].chars().next_back();
            let after = row[byte + name.len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !word(before) && !word(after) {
                let units: usize = row[..byte].chars().map(char::len_utf16).sum();
                return Some(json!({"line": n, "character": units}));
            }
            search = byte + name.len();
        }
    }
    None
}
