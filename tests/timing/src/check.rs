//! Whose each failure of a run is: the change's own, the base's, a known
//! flake's or the machine's, so a session fixes its own and reports the
//! rest without chasing it. Every failure stays listed and counted, and
//! none is passed off: this sorts them, it waives nothing.

use crate::Report;
use serde::Deserialize;
use std::collections::BTreeSet;

/// One kind of failure that is the machine's, not the change's: what its
/// message says, and what fixes it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    pub name: String,
    pub says: String,
    pub fix: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signatures {
    schema: u32,
    signature: Vec<Signature>,
}

/// The machine's signatures, from `signatures.toml` beside this crate.
pub fn signatures() -> Vec<Signature> {
    let read: Signatures =
        toml::from_str(include_str!("../signatures.toml")).expect("signatures.toml parses");
    assert_eq!(read.schema, 1, "signatures.toml's schema");
    read.signature
}

/// A test a workspace's `flakes.toml` lists: its package and whole path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Flake {
    pub package: String,
    pub test: String,
}

#[derive(Deserialize)]
struct Flakes {
    #[serde(default)]
    flake: Vec<Flake>,
}

/// The tests a `flakes.toml` lists, read for their package and path alone:
/// the repository check holds the rest of each entry.
pub fn flakes(text: &str) -> Result<Vec<Flake>, String> {
    toml::from_str::<Flakes>(text)
        .map(|read| read.flake)
        .map_err(|e| format!("flakes.toml: {e}"))
}

/// Whose a failure is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Whose {
    /// None of the others: the change's own, to fix.
    Yours,
    /// It failed on the base's run too.
    Base,
    /// Its workspace's `flakes.toml` lists it.
    Flake,
    /// Its message is the named signature's.
    Machine(String),
}

/// One failed test, sorted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sorted {
    pub package: Option<String>,
    pub test: String,
    pub said: Option<String>,
    pub whose: Whose,
}

/// Every failure of `run`, sorted against the base's run, the flakes and
/// the machine's signatures, in that order of precedence: the machine's
/// first, since a missing program fails tests that would pass, then a
/// known flake, then the base's.
pub fn sort(
    run: &Report,
    base: Option<&Report>,
    flakes: &[Flake],
    signatures: &[Signature],
) -> Vec<Sorted> {
    let failed = |report: &Report| -> BTreeSet<(Option<String>, String)> {
        report
            .programs
            .iter()
            .flat_map(|p| {
                p.failures
                    .iter()
                    .map(|f| (p.package.clone(), f.test.clone()))
            })
            .collect()
    };
    let on_base = base.map(failed).unwrap_or_default();
    run.programs
        .iter()
        .flat_map(|program| {
            program.failures.iter().map(|failure| {
                let package = program.package.clone();
                let machine = failure
                    .said
                    .as_deref()
                    .and_then(|said| signatures.iter().find(|s| said.contains(&s.says)));
                let whose = if let Some(signature) = machine {
                    Whose::Machine(signature.name.clone())
                } else if flakes
                    .iter()
                    .any(|f| Some(&f.package) == package.as_ref() && f.test == failure.test)
                {
                    Whose::Flake
                } else if on_base.contains(&(package.clone(), failure.test.clone())) {
                    Whose::Base
                } else {
                    Whose::Yours
                };
                Sorted {
                    package,
                    test: failure.test.clone(),
                    said: failure.said.clone(),
                    whose,
                }
            })
        })
        .collect()
}

/// What to do about a failure of the change's own: fix it, unless the
/// test is one the change didn't touch and a copy of it passes, which
/// makes it a new flake.
pub const YOURS: &str = "Fix each. One in a test this change didn't touch may be a flake: run `cargo test-copies -p <package> <test>`, and if any copy passes, write it in flakes.toml and go on.";

/// What to do about a failure the base's run had too.
pub const BASE: &str = "Not this change's: report them and go on.";

/// What to do about a known flake.
pub const FLAKE: &str = "Not this change's: report them and go on. A session given flakes.toml fixes each, proved by `cargo test-copies`.";

/// The sorted failures as a person reads them: a count of each group,
/// then each group's tests and what to do about them, the machine's
/// collapsed to one line per signature with its count and its fix.
pub fn text(sorted: &[Sorted], signatures: &[Signature]) -> String {
    let count = |whose: fn(&Whose) -> bool| sorted.iter().filter(|s| whose(&s.whose)).count();
    let yours = count(|w| *w == Whose::Yours);
    let base = count(|w| *w == Whose::Base);
    let flakes = count(|w| *w == Whose::Flake);
    let machine = count(|w| matches!(w, Whose::Machine(_)));
    let mut out = format!(
        "{} failed: {yours} yours, {base} the base's, {flakes} known flakes, {machine} the machine's\n",
        sorted.len()
    );
    let line = |s: &Sorted| {
        format!(
            "  {} {}{}\n",
            s.package.as_deref().unwrap_or("(unassigned)"),
            s.test,
            s.said
                .as_deref()
                .map(|said| format!(": {said}"))
                .unwrap_or_default()
        )
    };
    for (title, whose, next) in [
        ("Yours, to fix", Whose::Yours, YOURS),
        ("The base's, failing before this change", Whose::Base, BASE),
        ("Known flakes, in flakes.toml", Whose::Flake, FLAKE),
    ] {
        let group: Vec<_> = sorted.iter().filter(|s| s.whose == whose).collect();
        if !group.is_empty() {
            out.push_str(&format!("{title}:\n"));
            group.into_iter().for_each(|s| out.push_str(&line(s)));
            out.push_str(&format!("  {next}\n"));
        }
    }
    let mut first = true;
    for signature in signatures {
        let n = sorted
            .iter()
            .filter(|s| s.whose == Whose::Machine(signature.name.clone()))
            .count();
        if n > 0 {
            if first {
                out.push_str("The machine's:\n");
                first = false;
            }
            out.push_str(&format!(
                "  {} ({n} tests): {}\n",
                signature.name, signature.fix
            ));
        }
    }
    out
}
