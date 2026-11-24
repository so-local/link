//! What Link Code holds to, read once from `limits.toml`.

use std::time::Duration;

use serde::Deserialize;

/// The bounds on a server's messages and on an answer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    /// The most places or problems one answer lists.
    pub listed_max: usize,
    /// The most of a line shown beside a place, in characters.
    pub line_max: usize,
    /// The most of one diagnostic's message, in characters.
    pub message_max: usize,
    /// The most bytes of an answer's listing.
    pub answer_max: usize,
    /// The most bytes of a file read.
    pub file_max: u64,
    /// The protocol versions it speaks to a model's harness, newest first.
    pub versions: Vec<String>,
    /// The most bytes of one header line a server sends.
    pub header_max: usize,
    /// The most bytes of one message a server sends.
    pub body_max: usize,
    /// What's kept of a server's errors, in bytes.
    pub errors_kept: usize,
    /// The most pieces of work in progress followed at once.
    pub progress_max: usize,
    /// The most diagnostics kept of one file.
    pub diagnostics_max: usize,
    /// The most close names suggested for a name that matches no symbol.
    pub close_names_max: usize,
    /// How many lines from a symbol's range start are searched for its
    /// name.
    pub name_lines: usize,
    /// The most files under one root followed for changes.
    pub watched_max: usize,
    /// The folders, by name, never looked in for changes.
    pub unwatched: Vec<String>,
    /// The share of a call's time, in percent, waited for its server to
    /// settle before it's asked where a name is.
    pub settle_percent: u32,
    quiet_ms: u64,
    unpublished_ms: u64,
    errors_wait_ms: u64,
    stop_ms: u64,
    stop_poll_ms: u64,
}

impl Limits {
    /// When a call due by `deadline` stops waiting for its server to
    /// settle, its `settle-percent` of the time left.
    pub fn settle_by(&self, deadline: std::time::Instant) -> std::time::Instant {
        let now = std::time::Instant::now();
        now + deadline.saturating_duration_since(now) * self.settle_percent.min(100) / 100
    }

    /// How long a server is quiet before what it published is all it has.
    pub fn quiet(&self) -> Duration {
        Duration::from_millis(self.quiet_ms)
    }

    /// How long a file opened is waited on for diagnostics never published.
    pub fn unpublished(&self) -> Duration {
        Duration::from_millis(self.unpublished_ms)
    }

    /// How long an ended server's errors may stay open.
    pub fn errors_wait(&self) -> Duration {
        Duration::from_millis(self.errors_wait_ms)
    }

    /// How long a server is given to shut down.
    pub fn stop(&self) -> Duration {
        Duration::from_millis(self.stop_ms)
    }

    /// How long between looks whether a stopping server ended.
    pub fn stop_poll(&self) -> Duration {
        Duration::from_millis(self.stop_poll_ms)
    }
}

/// The limits, from `limits.toml`.
pub fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        let limits: Limits =
            toml::from_str(include_str!("limits.toml")).expect("limits.toml is Link Code's limits");
        assert!(
            !limits.versions.is_empty(),
            "Link Code speaks a protocol version"
        );
        limits
    });
    &LIMITS
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_limits_read() {
        let limits = super::limits();
        assert!(limits.listed_max > 0 && limits.answer_max > limits.message_max);
        assert!(limits.unpublished() > limits.quiet());
        assert!(!limits.versions.is_empty());
        assert!(limits.versions.windows(2).all(|pair| pair[0] > pair[1]));
    }
}
