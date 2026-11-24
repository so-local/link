//! What marks a file as a script: the two bytes a Unix kernel reads to
//! start an interpreter in its place.
pub(crate) const SCRIPT_MARK: &[u8] = b"#!";
