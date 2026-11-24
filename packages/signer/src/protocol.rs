//! A protocol: one way of making a signature and checking it.

use std::fmt;
use std::str::FromStr;

/// One way of making a signature and checking it, named for itself. Each
/// signer speaks one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Protocol {
    /// An Ed25519 SSH key, signing with no network.
    Ssh,
}

impl Protocol {
    /// Every protocol, as each is written.
    pub const ALL: [Self; 1] = [Self::Ssh];

    /// As it's written.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Protocol {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|p| p.as_str() == text)
            .ok_or_else(|| format!("{text:?} isn't a protocol: ssh"))
    }
}
