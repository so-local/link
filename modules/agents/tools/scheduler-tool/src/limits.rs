//! Link scheduler's limits, read once from `limits.toml`.

use std::time::Duration;

use serde::Deserialize;

/// What the tool and its runner hold to, as `limits.toml` sets them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The longest message a job sends, in bytes.
    pub text_max: usize,
    /// The most of a job's message `list` shows, in characters.
    pub listed_text_max: usize,
    /// The longest description a subagent is given, in bytes.
    pub description_max: usize,
    /// The most jobs an agent keeps waiting.
    pub jobs_max: usize,
    /// The most jobs one subagent may wait on.
    pub after_max: usize,
    /// The most jobs `list` gives, the soonest first.
    pub list_max: usize,
    /// How late a firing may be before it's a missed one, in seconds.
    pub late_after_secs: u64,
    /// How many times a failed firing is tried again.
    pub retries: u32,
    /// The wait before a failed firing's first try again, in seconds,
    /// doubled each time.
    pub retry_after_secs: u64,
    /// How long an ended job is kept, in seconds, the newest
    /// [`Limits::ended_max`] at most.
    pub ended_keep_secs: u64,
    /// The most ended jobs kept: past it, the longest ended go first.
    pub ended_max: usize,
    /// The most of an ended job's answer kept, in bytes.
    pub result_max: usize,
    look_every_ms: u64,
    /// The longest the runner sleeps between two looks at every job, in
    /// seconds.
    pub look_at_most_secs: u64,
    /// The most of a relay's messages put as jobs at one look.
    pub pulls_at_a_look: usize,
    /// How long before its end a kept token is made again, in seconds.
    pub token_margin_secs: u64,
}

impl Limits {
    /// How often the runner looks whether the jobs changed: a job set for
    /// now fires within this.
    pub fn look_every(&self) -> Duration {
        Duration::from_millis(self.look_every_ms)
    }
}

/// [`Limits`], read once.
pub fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        let limits: Limits =
            toml::from_str(include_str!("limits.toml")).expect("Link scheduler's limits.toml");
        assert!(
            limits.list_max > 0
                && limits.jobs_max > 0
                && limits.look_every_ms > 0
                && limits.look_every_ms <= limits.look_at_most_secs * 1000
                && limits.retries < u64::BITS
        );
        limits
    });
    &LIMITS
}

#[cfg(test)]
mod tests {
    /// `limits.toml` reads, each limit within what the rest hold to: the
    /// runner looks at least once a minute and calls a firing missed only
    /// past that, a retried firing's last wait fits, and a subagent waits
    /// on fewer jobs than an agent keeps.
    #[test]
    fn the_limits_read() {
        let limits = super::limits();
        assert!(limits.look_every() <= std::time::Duration::from_secs(limits.look_at_most_secs));
        assert!(limits.late_after_secs > limits.look_at_most_secs);
        assert!(limits.after_max < limits.jobs_max && limits.list_max <= limits.jobs_max);
        assert!(limits.listed_text_max <= limits.text_max);
        assert!(limits.ended_max > 0 && limits.ended_keep_secs > 0 && limits.result_max > 0);
        assert!(limits.pulls_at_a_look > 0);
    }
}
