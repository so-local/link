//! The flag nearest one given that names no field, which a refusal
//! offers in its place.

use serde_json::Value;

use super::{At, Field, fields, flag_of};
use crate::constants::FREE_KEY;

pub(super) fn unknown(schema: &Value, path: &[String]) -> String {
    let shown = flag_of(path);
    // A free key's path, with the key given in its place.
    let given = |field: &Field| -> Vec<String> {
        field
            .path
            .iter()
            .enumerate()
            .map(|(i, k)| match (k.as_str(), path.get(i)) {
                (FREE_KEY, Some(key)) => key.clone(),
                _ => k.clone(),
            })
            .collect()
    };
    // Only a flag the schema takes is offered: a free key given as a
    // declared one's name (`machine.disk` for `machine.<cloud>`) leads
    // nowhere, and the flag given is never its own nearest.
    let reaches = |keys: &[String]| {
        keys.iter()
            .try_fold(At::top(schema), |at, key| at.child(key))
            .is_some()
    };
    let nearest = fields(schema)
        .iter()
        .map(given)
        .filter(|keys| reaches(keys))
        .map(|keys| flag_of(&keys))
        .filter(|flag| *flag != shown)
        .map(|flag| (distance(&flag, &shown), flag))
        .min();
    match nearest {
        Some((_, flag)) => format!("--{shown} is no field of this spec; the nearest is --{flag}"),
        None => format!("--{shown}: this spec has no fields"),
    }
}

/// The edits between two words: how near a mistyped flag is to one.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut last = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (row[j + 1] + 1)
                .min(row[j] + 1)
                .min(last + usize::from(ca != *cb));
            last = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}
