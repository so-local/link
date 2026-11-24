//! A duration as a declaration's `format = "duration"` writes it: a
//! whole number and a unit, `500ms`, `30s`, `10m`, `2h` or `1d`.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration {
    value: u64,
    unit: Unit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Unit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl Unit {
    const ALL: [(Unit, &'static str, u64); 5] = [
        (Unit::Milliseconds, "ms", 1),
        (Unit::Seconds, "s", 1_000),
        (Unit::Minutes, "m", 60_000),
        (Unit::Hours, "h", 3_600_000),
        (Unit::Days, "d", 86_400_000),
    ];

    fn spelling(self) -> (&'static str, u64) {
        Self::ALL
            .iter()
            .find(|(unit, ..)| *unit == self)
            .map(|(_, word, ms)| (*word, *ms))
            .unwrap_or(("ms", 1))
    }
}

impl Duration {
    /// `n` seconds, written `<n>s`.
    pub const fn seconds(n: u64) -> Self {
        Self {
            value: n,
            unit: Unit::Seconds,
        }
    }

    /// `n` minutes, written `<n>m`.
    pub const fn minutes(n: u64) -> Self {
        Self {
            value: n,
            unit: Unit::Minutes,
        }
    }

    /// `duration`, in the largest unit that holds it whole.
    pub fn from_std(duration: std::time::Duration) -> Self {
        let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        let (unit, per) = Unit::ALL
            .iter()
            .rev()
            .find(|(_, _, per)| ms % per == 0 && ms >= *per)
            .map_or((Unit::Milliseconds, 1), |(unit, _, per)| (*unit, *per));
        Self {
            value: ms / per,
            unit,
        }
    }

    /// `n` hours, written `<n>h`.
    pub const fn hours(n: u64) -> Self {
        Self {
            value: n,
            unit: Unit::Hours,
        }
    }

    /// How long it is.
    pub fn get(self) -> std::time::Duration {
        let (_, ms) = self.unit.spelling();
        std::time::Duration::from_millis(self.value.saturating_mul(ms))
    }
}

impl FromStr for Duration {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let digits = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (number, word) = text.split_at(digits);
        let bad =
            || format!("{text:?} isn't a duration: a whole number and ms, s, m, h or d, as 30s");
        let value: u64 = number.parse().map_err(|_| bad())?;
        let (unit, ..) = Unit::ALL
            .iter()
            .find(|(_, w, _)| *w == word)
            .ok_or_else(bad)?;
        value
            .checked_mul(unit.spelling().1)
            .ok_or_else(|| format!("{text:?} is too long a duration"))?;
        Ok(Self { value, unit: *unit })
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.value, self.unit.spelling().0)
    }
}

impl Serialize for Duration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
