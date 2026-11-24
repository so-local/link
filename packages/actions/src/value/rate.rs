//! A rate as a declaration's `format = "rate"` writes it: how many in
//! each period, a whole number above none, a slash and `s`, `m` or `h`:
//! `10/s`, `600/m`.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How many in each period.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rate {
    count: u32,
    seconds: u64,
}

const PERIODS: [(&str, u64); 3] = [("s", 1), ("m", 60), ("h", 3600)];

impl Rate {
    /// How many in each period.
    pub const fn count(self) -> u32 {
        self.count
    }

    /// The period: a second, a minute or an hour.
    pub const fn per(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.seconds)
    }
}

impl FromStr for Rate {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let invalid = || format!("{text:?} isn't a rate, such as \"10/s\"");
        let (count, period) = text.trim().split_once('/').ok_or_else(invalid)?;
        let count: u32 = count
            .trim()
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(invalid)?;
        let (_, seconds) = PERIODS
            .iter()
            .find(|(name, _)| *name == period.trim())
            .ok_or_else(|| format!("{text:?} isn't a rate: /s, /m or /h after the number"))?;
        Ok(Self {
            count,
            seconds: *seconds,
        })
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = PERIODS
            .iter()
            .find(|(_, s)| *s == self.seconds)
            .map_or("s", |(name, _)| name);
        write!(f, "{}/{name}", self.count)
    }
}

impl Serialize for Rate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
