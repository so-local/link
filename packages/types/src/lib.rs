//! Core's shapes every module shares, generated from core's declarations
//! (`link-actions`' `core/types.toml`): an OS, a processor, a box's user and a
//! duration. A declaration names one as `core.<name>`, and a module's
//! generated code holds this crate's type.

include!(concat!(env!("OUT_DIR"), "/core.rs"));

mod arch;
mod os;
mod thrown;
mod user;

pub use thrown::Thrown;

impl Duration {
    /// `n` seconds.
    pub const fn seconds(n: u64) -> Self {
        Self(link_actions::value::Duration::seconds(n))
    }

    /// `n` minutes.
    pub const fn minutes(n: u64) -> Self {
        Self(link_actions::value::Duration::minutes(n))
    }

    /// `n` hours.
    pub const fn hours(n: u64) -> Self {
        Self(link_actions::value::Duration::hours(n))
    }

    /// `duration`, in the largest unit that holds it whole.
    pub fn from_std(duration: std::time::Duration) -> Self {
        Self(link_actions::value::Duration::from_std(duration))
    }

    /// How long it is.
    pub fn get(self) -> std::time::Duration {
        self.0.get()
    }
}

impl Copy for Duration {}

impl Eq for Duration {}

impl std::fmt::Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for Duration {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        text.parse().map(Self)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);
