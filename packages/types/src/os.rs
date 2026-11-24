//! What an OS says of itself beyond its shape: its word, the one a box
//! has by default, and its place on a command line.

use crate::Os;

impl Os {
    /// Its word, `ubuntu-24.04`.
    pub const fn word(self) -> &'static str {
        match self {
            Os::Ubuntu2404 => "ubuntu-24.04",
            Os::Ubuntu2204 => "ubuntu-22.04",
            Os::Debian13 => "debian-13",
            Os::Macos => "macos",
        }
    }
}

/// A box's OS when nothing names one.
impl Default for Os {
    fn default() -> Self {
        Os::Ubuntu2404
    }
}

impl std::fmt::Display for Os {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.word())
    }
}
