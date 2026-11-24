//! Harness configuration builders.
use super::*;

impl Harness {
    /// A first model found off left alone for `lasts` (tests).
    pub fn with_off_for(mut self, lasts: std::time::Duration) -> Self {
        self.off = crate::fallback::Off::new(lasts);
        self
    }

    /// Each turn's measures to `path` (tests).
    pub fn with_measures(mut self, path: std::path::PathBuf) -> Self {
        self.measures = Some(path);
        self
    }

    /// A smaller window (tests).
    pub fn with_budget(mut self, budget: usize) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Less time for a turn to wait for a tool to start and list its
    /// tools, and for its start in all (tests).
    pub fn with_open_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.open_timeout = timeout;
        self.start_limit = timeout;
        self
    }

    /// More time for a tool's start in all than a turn waits for it
    /// (tests): a start a turn stopped waiting for goes on.
    pub fn with_start_limit(mut self, limit: std::time::Duration) -> Self {
        self.start_limit = limit.max(self.open_timeout);
        self
    }
}
