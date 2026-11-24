//! The box spec's check hook (`actions.toml`'s `checks`), run on every
//! read of the spec, from a file, its flags or a call alike.

use crate::spec::BoxSpec;

/// What the spec's shape alone can't refuse ([`crate::spec`]).
pub(crate) fn spec(spec: &BoxSpec) -> Result<(), String> {
    crate::spec::checked(spec).map_err(|e| format!("{e:#}"))
}
