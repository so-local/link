//! link-token's limits, read once from `limits.toml`.

use std::sync::LazyLock;

use serde::Deserialize;

/// link-token's limits, as `limits.toml` sets them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// A token's life unless its issuer's settings say less, in seconds.
    pub token_life: u64,
    /// The longest any token may live, in seconds.
    pub token_life_max: u64,
}

/// link-token's limits.
pub fn limits() -> &'static Limits {
    static LIMITS: LazyLock<Limits> = LazyLock::new(|| {
        let limits: Limits =
            toml::from_str(include_str!("limits.toml")).expect("link-token's limits");
        limits.check().expect("link-token's limits");
        limits
    });
    &LIMITS
}

impl Limits {
    fn check(&self) -> Result<(), String> {
        if self.token_life == 0 || self.token_life > self.token_life_max {
            return Err("a token lives at least a second, and no longer than the longest".into());
        }
        if self.token_life_max > 24 * 3600 {
            return Err("no token lives longer than a day".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_limits_read() {
        assert_eq!(super::limits().token_life, 300);
    }
}
