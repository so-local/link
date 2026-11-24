//! link-signer's limits, read once from `limits.toml`.

use std::sync::LazyLock;

use serde::Deserialize;

/// link-signer's limits, as `limits.toml` sets them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The longest a signature lives, `iat` to `exp`, in seconds.
    pub signature_life: u64,
    /// The longest compact JWS read, in bytes.
    pub jws_max: usize,
    /// How many signatures a replay store keeps at once.
    pub seen_max: usize,
    /// How far ahead of this machine's clock a statement's `iat` may be,
    /// in seconds.
    pub clock_skew: u64,
}

/// link-signer's limits.
pub fn limits() -> &'static Limits {
    static LIMITS: LazyLock<Limits> = LazyLock::new(|| {
        let limits: Limits =
            toml::from_str(include_str!("limits.toml")).expect("link-signer's limits");
        limits.check().expect("link-signer's limits");
        limits
    });
    &LIMITS
}

impl Limits {
    fn check(&self) -> Result<(), String> {
        if !(1..=300).contains(&self.signature_life) {
            return Err("a signature lives 1 to 300 seconds".into());
        }
        if !(1024..=65536).contains(&self.jws_max) {
            return Err("a JWS is read up to 1 to 64 KiB".into());
        }
        if self.seen_max == 0 {
            return Err("a replay store keeps at least one signature".into());
        }
        if self.clock_skew > 60 {
            return Err("clocks are at most a minute apart".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_limits_read() {
        let limits = super::limits();
        assert_eq!(limits.signature_life, 60);
        assert!(limits.jws_max >= 4096);
    }
}
