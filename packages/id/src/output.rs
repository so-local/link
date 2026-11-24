//! An ID in output: `<kind>:<id>`, under a field naming its axis
//! (`output.toml`'s `axes`), never bare.

use std::sync::LazyLock;

use serde::Deserialize;

/// The output form, as `output.toml` sets it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    /// The fields an ID is written under, each an axis's name.
    axes: Vec<String>,
}

fn output() -> &'static Output {
    static OUTPUT: LazyLock<Output> = LazyLock::new(|| {
        let output: Output =
            toml::from_str(include_str!("output.toml")).expect("link-id's output form");
        assert!(
            output.axes.iter().all(|axis| crate::is_kind_name(axis)),
            "link-id's axes are names"
        );
        output
    });
    &OUTPUT
}

/// The fields an ID is written under in output, each naming its axis.
pub fn axes() -> &'static [String] {
    &output().axes
}

/// Whether an ID may be written under `field`: it names an axis.
pub fn is_axis(field: &str) -> bool {
    axes().iter().any(|axis| axis == field)
}

/// The candidate id `text` writes as `<kind>:<id>` of `kind`: none when
/// it's bare, of another kind, or not a candidate ([`crate::is_candidate`]).
pub fn candidate_of<'a>(kind: &str, text: &'a str) -> Option<&'a str> {
    text.strip_prefix(kind)?
        .strip_prefix(':')
        .filter(|id| crate::is_candidate(id))
}
