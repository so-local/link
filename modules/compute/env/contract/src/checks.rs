//! The environment spec's check hooks (`actions.toml`'s `checks`), run on
//! every read of the spec, from a file, its flags or a call alike.

use crate::file::Env;

/// What the file alone can't refuse: what it names on disk is checked
/// where it's read from its folder (`file::read`).
pub(crate) fn input(file: &Env) -> Result<(), String> {
    crate::file::check_input(file).map_err(|e| format!("{e:#}"))
}
