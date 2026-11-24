//! A size as a declaration's `format = "size"` writes it: a whole number
//! and a unit, `16 MB`, the space between optional, each unit a power of
//! two: `B`, `KB`, `MB` or `GB`.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How many bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Size(u64);

const UNITS: [(&str, u64); 4] = [("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10), ("B", 1)];

impl Size {
    /// `n` bytes.
    pub const fn bytes(n: u64) -> Self {
        Self(n)
    }

    /// How many bytes it is.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl FromStr for Size {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (number, unit) = super::number_and_word(text);
        let number: u64 = number
            .parse()
            .map_err(|_| format!("{text:?} isn't a size, such as \"16 MB\""))?;
        let (_, times) = UNITS
            .iter()
            .find(|(name, _)| *name == unit)
            .ok_or_else(|| format!("{text:?} isn't a size: B, KB, MB or GB after the number"))?;
        number
            .checked_mul(*times)
            .map(Self)
            .ok_or_else(|| format!("{text:?} is too large a size"))
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, times) = UNITS
            .iter()
            .find(|(_, times)| self.0.is_multiple_of(*times) && self.0 >= *times)
            .unwrap_or(&("B", 1));
        write!(f, "{} {name}", self.0 / times)
    }
}

impl Serialize for Size {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
