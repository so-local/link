//! What memory holds to, read once from `limits.toml`.

use serde::Deserialize;

/// The bounds on extraction, on what's indexed and given, on a
/// conversation's copy, and on the service's sessions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    /// The most documents one background batch extracts.
    pub extract_batch: u32,
    /// The most a compact conversation's frames are read to, in bytes.
    pub frames_max_bytes: u64,
    /// The longest line of a service session past its hello, in bytes.
    pub message_max_bytes: u64,
    /// The most sessions the service serves at once.
    pub sessions_max: usize,
    /// The largest file indexed, in bytes.
    pub file_max_bytes: u64,
    /// The most removed documents remembered.
    pub removed_max: i64,
    /// The most documents `links` gives each way.
    pub links_max: usize,
    /// How long the document converter may take over a document, in
    /// seconds ([`Limits::extract_deadline`]).
    pub extract_deadline_secs: u64,
    /// The most memory the document converter may map, in bytes.
    pub extract_memory_max_bytes: u64,
    /// The most text one indexing batch reads before it commits, in bytes.
    pub batch_bytes: u64,
    /// About how long one indexing batch takes, reading and committing,
    /// in milliseconds: what it reads is sized to it by how fast the last
    /// batch went.
    pub batch_millis: u64,
    /// What the write-ahead log is cut back to once checkpointed, in bytes.
    pub wal_max_bytes: i64,
    /// How often a watched folder is walked anyway, in seconds.
    pub walk_every_secs: u64,
    /// How often a folder no watch reaches whole is walked, in seconds.
    pub walk_unwatched_secs: u64,
    /// How long the writer waits after a commit found the disk full, in
    /// seconds.
    pub full_retry_secs: u64,
    /// The most removed documents one indexing batch drops.
    pub removed_batch: usize,
    /// The most noted paths one writer's turn takes from the journal.
    pub journal_batch: usize,
    /// How many times the writer tries again a document whose parser
    /// didn't start.
    pub extract_tries: i64,
    /// The most of a file a call reads to answer, in bytes.
    pub source_max_bytes: u64,
    /// How much a word in a title counts, beside one in a passage.
    pub rank_title: f64,
    /// The share of the passages past which a word is common.
    pub rank_common: f64,
    /// How many passages a search by meaning rescores with their small
    /// vectors.
    pub rescore: usize,
}

impl Limits {
    /// How long the document converter may take over a document, from
    /// its start: then it's killed.
    pub fn extract_deadline(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.extract_deadline_secs)
    }
}

/// The limits, from `limits.toml`.
pub fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("limits.toml")).expect("limits.toml is memory's limits")
    });
    &LIMITS
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_limits_read() {
        let limits = super::limits();
        assert!(limits.extract_batch > 0 && limits.sessions_max > 0);
        assert!(limits.file_max_bytes > 0 && limits.removed_max > 0 && limits.links_max > 0);
        // The converter is given time and memory enough for a document.
        assert!(limits.extract_deadline() >= std::time::Duration::from_secs(1));
        assert!(
            limits.extract_memory_max_bytes >= crate::extract::document_limits().document_max_bytes
        );
        assert!(limits.frames_max_bytes >= limits.file_max_bytes);
        assert!(limits.message_max_bytes >= 64 << 10);
        // A batch is small beside the log it's held to.
        assert!(limits.batch_bytes > 0 && (limits.batch_bytes as i64) < limits.wal_max_bytes);
        // A batch under way leaves a file written time to be found within
        // the 2 s memory's docs promise.
        assert!(limits.batch_millis > 0 && limits.batch_millis <= 1000);
        // A folder no watch reaches is walked more often than one it does.
        assert!(
            0 < limits.walk_unwatched_secs && limits.walk_unwatched_secs <= limits.walk_every_secs
        );
        assert!(limits.journal_batch > 0 && limits.full_retry_secs > 0);
        assert!(limits.removed_batch > 0);
        // A call reads any file the index holds.
        assert!(limits.source_max_bytes >= limits.frames_max_bytes);
        assert!(limits.rescore > 0);
        assert!(limits.rank_title >= 0.0 && (0.0..1.0).contains(&limits.rank_common));
    }
}
