//! How long lnk-agent waits on an agent's harness: to stop, and for one
//! run of its health check (`harness_waits.toml`).

use std::time::Duration;

/// `harness_waits.toml`, typed.
#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessWaits {
    stop_secs: u64,
    health_secs: u64,
}

impl HarnessWaits {
    /// `harness_waits.toml`, read once.
    pub fn get() -> HarnessWaits {
        static READ: std::sync::OnceLock<HarnessWaits> = std::sync::OnceLock::new();
        *READ.get_or_init(|| {
            toml::from_str(include_str!("harness_waits.toml")).expect("harness_waits.toml reads")
        })
    }

    /// How long a harness asked to stop has to end.
    pub fn stop(&self) -> Duration {
        Duration::from_secs(self.stop_secs)
    }

    /// How long one run of a harness's health check has.
    pub fn health(&self) -> Duration {
        Duration::from_secs(self.health_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stop waits longer than one health check, and both are bounded.
    #[test]
    fn the_waits_read() {
        let waits = HarnessWaits::get();
        assert!(waits.health() > Duration::ZERO);
        assert!(waits.stop() > waits.health());
        assert!(waits.stop() <= Duration::from_secs(120));
    }
}
