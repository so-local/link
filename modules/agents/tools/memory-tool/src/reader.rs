//! What a call reads through ([`Reader`]): a connection of its own to
//! the index's file, never the writer's, so no call waits for indexing;
//! and what an index shares with every call ([`Common`]).
//!
//! SQLite's write-ahead log lets a reader read the last committed state
//! while one writer writes: a call answers from what's indexed so far.

use super::*;
use std::sync::Mutex;

/// What an index shares with every call made of it, whichever
/// connection reads: its file, search by meaning, the agent's files, the
/// vectors kept between searches, and how far indexing has got.
pub struct Common {
    /// The index's file: the one given, or a temporary one.
    pub(crate) file: PathBuf,
    pub(crate) semantic: Arc<Shared>,
    /// What reads the agent's files: an indexed document's text, and a
    /// skill's file, never indexed.
    pub(crate) files: Arc<dyn tree::Files>,
    /// How far indexing has got, as the writer says it.
    pub(crate) progress: Mutex<Progress>,
    /// The bytes of page cache each connection holds ([`crate::opinions::Opinions::memory`]).
    pub(crate) memory: std::sync::atomic::AtomicU64,
    /// The largest file indexed, in bytes ([`crate::opinions::Opinions::file`]):
    /// a call reads one this large.
    pub(crate) file_most: std::sync::atomic::AtomicU64,
    /// How long a search ranks, in milliseconds
    /// ([`crate::opinions::Opinions::deadline`]).
    pub(crate) deadline_ms: std::sync::atomic::AtomicU64,
    /// How many of the model's dimensions vectors keep
    /// ([`crate::opinions::Opinions::dimensions`]).
    pub(crate) dimensions: std::sync::atomic::AtomicU32,
    /// The bytes of small vectors searches by meaning have read, to
    /// rescore their best ([`crate::vectors`]).
    pub(crate) small_read: std::sync::atomic::AtomicU64,
    /// The passages searches by words have scored: what Tantivy read of
    /// their words' postings, past what it skipped ([`crate::index::words`]).
    pub(crate) scored: std::sync::atomic::AtomicU64,
    /// The paths that may have changed, which the writer drains.
    pub(crate) journal: Arc<crate::index::Journal>,
    /// Which passages hold each word: search by words.
    pub(crate) words: Arc<crate::index::words::Words>,
}

/// How far indexing has got: what a call's answer says of what isn't
/// found yet.
#[derive(Clone, Debug, Default)]
pub(crate) struct Progress {
    /// Whether the folder has been read whole once.
    pub(crate) walked: bool,
    /// Why indexing is paused, while it is: the disk short of its reserve.
    pub(crate) paused: Option<String>,
}

impl Common {
    /// Something of what it indexes changed, it's not known what: its
    /// folder is walked soon. Takes nothing the writer holds.
    pub fn changed(&self) {
        self.journal.walk();
    }

    /// `path` (from the folder served) may have changed: indexed soon.
    /// Takes nothing the writer holds.
    pub fn noted(&self, path: &str) {
        self.journal.note(path);
    }

    /// The paths that may have changed.
    pub fn journal(&self) -> &Arc<crate::index::Journal> {
        &self.journal
    }

    pub(crate) fn progress(&self) -> Progress {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn set_progress(&self, change: impl FnOnce(&mut Progress)) {
        change(&mut self.progress.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// A call's way into an index: its own connection, read-only, and what
/// the index shares ([`Common`]).
pub struct Reader {
    pub(crate) db: Connection,
    pub(crate) common: Arc<Common>,
}

impl Reader {
    /// A reader of the index `common` describes: opening reads nothing of
    /// it, so it never waits on the writer.
    pub fn open(common: Arc<Common>) -> Result<Reader> {
        let db = open_file(&common.file)?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        // A call only reads: refused any write, whatever it runs.
        db.execute_batch("PRAGMA query_only = 1;")?;
        crate::opening::cache(
            &db,
            common.memory.load(std::sync::atomic::Ordering::Relaxed) / READER_SHARE,
        )?;
        Ok(Reader { db, common })
    }

    /// Whether it reads the index `common` describes: one replaced (a
    /// bound agent's, fetched again) needs a reader of its own.
    pub fn reads(&self, common: &Arc<Common>) -> bool {
        Arc::ptr_eq(&self.common, common)
    }

    /// Whether search is by meaning too, and how far embedding and
    /// indexing have got; what memory has to say of its documents.
    pub fn status(&self) -> Result<Status> {
        let mut status = self.embedding()?;
        status.indexing = self.indexing();
        if let Some(why) = self.common.journal.unwatched() {
            status.notices.push(format!(
                "what changes in memory's folder is found by a walk: {why}"
            ));
        }
        let mut unread = self
            .db
            .prepare("SELECT path, why FROM unread ORDER BY path LIMIT 20")?;
        for row in unread.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (path, why) = row?;
            status
                .notices
                .push(format!("{path} is found by its name only: {why}"));
        }
        Ok(status)
    }

    /// What isn't indexed yet, as the writer last said: none once
    /// everything it was told of is.
    fn indexing(&self) -> Option<semantic::Indexing> {
        let progress = self.common.progress();
        let waiting = self.common.journal.waiting();
        (!progress.walked || waiting > 0 || progress.paused.is_some()).then_some(
            semantic::Indexing {
                walking: !progress.walked,
                waiting,
                paused: progress.paused.clone(),
            },
        )
    }

    fn embedding(&self) -> Result<Status> {
        let model = match self.common.semantic.state() {
            State::On(embedder) => embedder.model().to_string(),
            State::Off(why) => {
                return Ok(Status {
                    semantic: Some(format!("Search is by words only: {why}.")),
                    ..Status::default()
                });
            }
            State::Starting => {
                return Ok(Status {
                    semantic: Some(
                        "Search is by words only while the embedding model starts.".into(),
                    ),
                    ..Status::default()
                });
            }
        };
        // Other models' vectors are forgotten when this one starts, so what
        // is embedded is this model's: counted as it's written, never
        // counted here.
        let (done, total): (i64, i64) = self.db.query_row(
            "SELECT coalesce((SELECT value FROM meta WHERE key = 'embedded'), 0),
                    coalesce((SELECT value FROM meta WHERE key = 'passages'), 0)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let (done, total) = (done.clamp(0, total.max(0)), total.max(0));
        let mut notices = Vec::new();
        if let Some(kept) = crate::vectors::Kept::held(&self.db, &self.common.file, &model)?
            && kept.cut == crate::vectors::Cut::Rotated
        {
            notices.push(format!(
                "{model} isn't known to be trained to be cut to fewer dimensions: its vectors \
                 are turned by a fixed random rotation before {} of them are kept",
                kept.dimensions
            ));
        }
        Ok(Status {
            notices,
            embedded: (done < total).then_some(Embedded {
                done: done as usize,
                total: total as usize,
            }),
            semantic: self
                .common
                .semantic
                .failing()
                .map(|why| format!("Embedding is failing: {why}.")),
            ..Status::default()
        })
    }
}
