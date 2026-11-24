//! Call identities within one model response and result matching.

use super::Call;
use anyhow::{Context, bail, ensure};

/// The longest provider-safe call ID.
pub const CALL_ID_MAX: usize = 64;

/// Normalize provider IDs and name omitted IDs from the response's
/// place. Repeated IDs within this batch are refused, never aliased.
pub fn settle_ids(calls: &mut [Call], seq: u64) -> anyhow::Result<()> {
    for (n, call) in calls.iter_mut().enumerate() {
        let plain: String = call
            .id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(CALL_ID_MAX)
            .collect();
        call.id = if plain.is_empty() {
            format!("call-{seq}-{}", n + 1)
        } else {
            plain
        };
    }
    check_ids(calls)
}

pub fn check_ids(calls: &[Call]) -> anyhow::Result<()> {
    for (n, call) in calls.iter().enumerate() {
        ensure!(
            !call.id.is_empty()
                && call.id.len() <= CALL_ID_MAX
                && call
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "the model made a call with an invalid ID"
        );
        ensure!(
            !calls[..n].iter().any(|before| before.id == call.id),
            "the model made two calls with the ID {}",
            call.id
        );
    }
    Ok(())
}

pub trait Matched {
    fn call(&self) -> &str;
}

impl Matched for super::Answer {
    fn call(&self) -> &str {
        &self.call
    }
}

/// A result for every originating call, in call order. Missing,
/// repeated, and unasked-for results are refused at this one seam.
pub fn in_call_order<R: Matched>(calls: &[Call], results: Vec<R>) -> anyhow::Result<Vec<R>> {
    let mut slots: Vec<Option<R>> = calls.iter().map(|_| None).collect();
    for result in results {
        let Some(at) = calls.iter().position(|call| call.id == result.call()) else {
            bail!("a tool answered {}, which wasn't called", result.call());
        };
        ensure!(
            slots[at].is_none(),
            "a tool answered {} twice",
            result.call()
        );
        slots[at] = Some(result);
    }
    slots
        .into_iter()
        .zip(calls)
        .map(|(slot, call)| slot.with_context(|| format!("no tool answered {}", call.id)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_after_normalization_are_refused_and_missing_ids_are_named() {
        let mut calls = vec![
            Call {
                id: "same!".into(),
                ..Default::default()
            },
            Call {
                id: "same".into(),
                ..Default::default()
            },
        ];
        assert!(settle_ids(&mut calls, 4).is_err());
        calls[1].id.clear();
        settle_ids(&mut calls, 4).unwrap();
        assert_eq!(calls[1].id, "call-4-2");
    }
}
