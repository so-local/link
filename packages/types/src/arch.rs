//! What a processor says of itself beyond its shape: its word and the
//! one a machine has by default.

use crate::Arch;

impl Arch {
    /// Its word, `x86_64`.
    pub fn word(self) -> &'static str {
        link_actions::value::Words::word(self)
    }
}

/// A machine's processor when nothing names one.
impl Default for Arch {
    fn default() -> Self {
        Arch::X8664
    }
}
