//! [`Index::refresh`]: the index brought up to the folder, each document's
//! title, chunks and links written, in small transactions, the newest
//! first: each one committed stays, so a crash or a kill loses at most
//! the batch it was writing, and the next start's walk finds the rest.

use super::*;
use crate::writing::{Batch, grow, write_body};
use link_format::{Foreign, sentence};
use std::sync::atomic::{AtomicBool, Ordering};

impl Index {
    /// Brings the index up to the folder at once: reads what's new or
    /// changed (by size and time), drops what's gone, in as many small
    /// transactions as it takes. The writer does the same a turn at a
    /// time ([`Index::work`]).
    pub fn refresh(&mut self) -> Result<Refreshed> {
        self.reconcile_words()?;
        let mut refreshed = Refreshed::default();
        if self.queue.is_none() {
            self.walk()?;
        }
        while self.queue.is_some() {
            if !self.batch(&mut refreshed)? {
                // Paused: the disk is short of its reserve.
                break;
            }
        }
        refreshed.total = self.listed_total;
        self.common.words.release();
        Ok(refreshed)
    }

    /// The writer's turn: a batch of what's queued, the paths its journal
    /// noted first; with none queued, a walk when one's due. Whether more
    /// waits at once.
    pub fn turn(&mut self) -> Result<bool> {
        let more = self.take_turn();
        // Nothing left to write: the words' writer lets go, so another
        // writer of the same file may write it.
        if !matches!(more, Ok(true)) {
            self.common.words.release();
        }
        more
    }

    fn take_turn(&mut self) -> Result<bool> {
        self.reconcile_words()?;
        if self.queue.is_none() && (self.common.journal.take_walk() || self.due()) {
            self.walk()?;
        } else {
            // Ahead of what a walk queued: a file written while the index
            // builds is found by the next batch, not after the build.
            self.drain()?;
        }
        if self.queue.is_none() {
            return Ok(false);
        }
        let mut refreshed = Refreshed::default();
        Ok(self.batch(&mut refreshed)? && self.queue.is_some())
    }

    /// Sets the words right by the passages when they may differ
    /// ([`crate::index::words::Words::reconcile`]), unless indexing is
    /// paused ([`Index::pause`]): what it reads again noted in the journal.
    fn reconcile_words(&mut self) -> Result<()> {
        let words = self.common.words.clone();
        if !words.behind() || self.pause() {
            return Ok(());
        }
        match words.reconcile(&mut self.db) {
            Ok(paths) => {
                for path in paths {
                    self.common.journal.note(&path);
                }
                Ok(())
            }
            Err(err) if disk_full(&err) => {
                words.filled();
                self.pause();
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Whether indexing pauses now, said in its progress: the disk is short
    /// of its reserve, a commit found it full lately, or another process
    /// writes the index.
    fn pause(&self) -> bool {
        let words = &self.common.words;
        let why = self
            .short_of_room()
            .or_else(|| words.full().then(|| PAUSED_FULL.to_string()))
            .or_else(|| words.unwritable());
        let paused = why.is_some();
        if paused {
            self.common.set_progress(|progress| progress.paused = why);
        }
        paused
    }

    /// Queues what the journal noted at the queue's front, the newest
    /// first: each path asked of the tree, and written if it changed,
    /// dropped if it's gone, ahead of what a walk queued for it before,
    /// in batches of its own, so it waits for none of the walk's. A
    /// path the tree can't say alone (a folder, a conversation a driver
    /// keeps, a tree reached through a bridge) is found by a walk instead:
    /// at once for a folder here, or once the queue is written while one
    /// is, and [`SETTLE`] after the last change for a tree listed only on
    /// a change, so a burst of writes is listed once.
    fn drain(&mut self) -> Result<()> {
        let journal = self.common.journal.clone();
        if journal.waiting() == 0 {
            return Ok(());
        }
        if self.tree.on_change() {
            if self.queue.is_none() && journal.last().is_some_and(|at| at.elapsed() >= SETTLE) {
                self.walk()?;
            }
            return Ok(());
        }
        let paths = journal.take(crate::limits::limits().journal_batch);
        let mut asked = std::collections::HashSet::new();
        let mut changed = Vec::new();
        let mut removed = Vec::new();
        for path in paths {
            let known: Option<(i64, i64, i64)> = self
                .db
                .query_row(
                    "SELECT id, size, modified FROM documents WHERE path = ?1",
                    [&path],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            match self.tree.found(&path) {
                // Not said alone: a walk finds it, after the queue.
                None if self.queue.is_some() => {
                    journal.walk();
                    continue;
                }
                None => return self.walk(),
                // Tried again at the writer's next turn.
                Some(Err(err)) => {
                    crate::warn(sentence!("{} isn't reached.", Foreign::new(&path)), &err);
                    journal.retry(&path);
                    continue;
                }
                Some(Ok(None)) => {
                    if let Some((id, _, _)) = known {
                        removed.push((path.clone(), id));
                    }
                }
                Some(Ok(Some(file))) => match known {
                    Some((_, size, modified)) if (size, modified) == (file.size, file.modified) => {
                    }
                    _ => changed.push((path.clone(), file)),
                },
            }
            asked.insert(path);
        }
        changed.sort_by(|a, b| a.1.modified.cmp(&b.1.modified).then(b.0.cmp(&a.0)));
        match &mut self.queue {
            // What a walk found of these before is as they are now.
            Some(queue) => {
                queue.changed.retain(|(path, ..)| !asked.contains(path));
                queue.noted.retain(|(path, ..)| !asked.contains(path));
                queue.removed.retain(|(path, _)| !asked.contains(path));
                queue.noted.extend(changed);
                queue.removed.extend(removed);
            }
            None if changed.is_empty() && removed.is_empty() => {}
            None => {
                self.queue = Some(Queue {
                    changed: Vec::new(),
                    noted: changed,
                    removed,
                    walk: false,
                    walked: true,
                    now: now_nanos(),
                    indexed: 0,
                    removed_any: false,
                });
            }
        }
        Ok(())
    }

    /// Whether what the last walk found waits to be indexed.
    pub fn pending(&self) -> bool {
        self.queue.is_some()
    }

    /// Whether it has work under way: indexing left, unless it's paused
    /// for want of disk, or documents' text being extracted.
    pub fn busy(&self) -> bool {
        self.extracting() || (self.pending() && self.common.progress().paused.is_none())
    }

    /// Lists the folder and notes what to read again (new or changed,
    /// the newest first) and what's gone, for the batches that follow.
    fn walk(&mut self) -> Result<()> {
        // What was noted until now is listed by this: noted meanwhile,
        // it's indexed after.
        self.common.journal.clear();
        // Documents read by their name alone while Link Document Converter wasn't
        // installed: read again, now that it is.
        if self.extractor.available() {
            self.db.execute(
                "UPDATE documents SET modified = -1
                 WHERE path IN (SELECT path FROM unread WHERE why = ?1)",
                [extract::not_installed()],
            )?;
        }
        let db = &self.db;
        let known_of = |path: &str| -> Option<(i64, i64)> {
            db.query_row(
                "SELECT size, modified FROM documents WHERE path = ?1",
                [path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .ok()
            .flatten()
        };
        let found = self.tree.list(&known_of)?;
        // The first walk finds what was there before memory: first seen
        // when last changed, so none of it is new.
        let walked: bool = self.db.query_row(
            "SELECT count(*) FROM meta WHERE key = 'walked'",
            [],
            |row| row.get::<_, i64>(0),
        )? > 0;
        let known: HashMap<String, (i64, i64, i64)> = {
            let mut statement = self
                .db
                .prepare("SELECT path, id, size, modified FROM documents")?;
            statement
                .query_map([], |row| {
                    Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?)))
                })?
                .collect::<rusqlite::Result<_>>()?
        };
        self.listed_total = found.len();
        let removed: Vec<(String, i64)> = known
            .iter()
            .filter(|(path, _)| !found.contains_key(*path))
            .map(|(path, (id, _, _))| (path.clone(), *id))
            .collect();
        let mut changed: Vec<(String, tree::Found)> = found
            .into_iter()
            .filter_map(|(path, file)| match known.get(&path) {
                Some((_, size, modified)) if (*size, *modified) == (file.size, file.modified) => {
                    None
                }
                _ => Some((path, file)),
            })
            .collect();
        if walked && removed.is_empty() && changed.is_empty() {
            // Nothing to write.
            self.listed();
            self.extract_pending()?;
            return Ok(());
        }
        // The newest last, taken first.
        changed.sort_by(|a, b| a.1.modified.cmp(&b.1.modified).then(b.0.cmp(&a.0)));
        self.queue = Some(Queue {
            changed,
            noted: Vec::new(),
            removed,
            walk: true,
            walked,
            now: now_nanos(),
            indexed: 0,
            removed_any: false,
        });
        Ok(())
    }

    /// One batch of the queue, in a transaction of its own: what's gone
    /// dropped, up to [`crate::limits::Limits::removed_batch`] of it,
    /// then what's new or changed read, the newest first, until
    /// [`crate::limits::Limits::batch_bytes`] of text. Paused, nothing
    /// written, while the disk is short of its reserve. Whether it wrote.
    fn batch(&mut self, refreshed: &mut Refreshed) -> Result<bool> {
        if self.pause() {
            return Ok(false);
        }
        let Some(mut queue) = self.queue.take() else {
            return Ok(false);
        };
        let most_removed = crate::limits::limits().removed_batch;
        let removed = queue
            .removed
            .split_off(queue.removed.len().saturating_sub(most_removed));
        // What the journal noted first, in a batch of its own: it waits
        // for none of what a walk queued.
        let noted = !queue.noted.is_empty();
        let mut taken = Vec::new();
        let limits = crate::limits::limits();
        let most = batch_bytes(self.speed, limits.batch_millis, limits.batch_bytes);
        let mut bytes = 0u64;
        // At least one file, however large.
        while (taken.is_empty() || bytes < most)
            && let Some(next) = match noted {
                true => queue.noted.pop(),
                false => queue.changed.pop(),
            }
        {
            bytes += next.1.size.max(0) as u64;
            taken.push(next);
        }
        let put_back = |queue: &mut Queue, rest: Vec<(String, tree::Found)>| match noted {
            true => queue.noted.extend(rest.into_iter().rev()),
            false => queue.changed.extend(rest.into_iter().rev()),
        };
        let started = std::time::Instant::now();
        match self.write_batch(&queue, &removed, &taken) {
            Ok((indexed, read)) => {
                // Stopped at a change noted: the rest waits behind it.
                put_back(&mut queue, taken.split_off(read));
                // How fast a walk's files go, to size the next batch by.
                if !noted && !taken.is_empty() {
                    let written: u64 = taken.iter().map(|(_, f)| f.size.max(0) as u64).sum();
                    let took = started.elapsed().as_secs_f64().max(f64::EPSILON);
                    self.speed = Some(written as f64 / took);
                }
                queue.indexed += indexed;
                queue.removed_any |= !removed.is_empty();
                refreshed.indexed += indexed;
                refreshed.removed += removed.len();
                // Tantivy's commit may have found the disk full.
                let full = self.common.words.full().then(|| PAUSED_FULL.to_string());
                self.common.set_progress(|progress| progress.paused = full);
            }
            Err(err) => {
                // Nothing of it written: taken again at the next turn.
                queue.removed.extend(removed);
                put_back(&mut queue, taken);
                self.queue = Some(queue);
                if disk_full(&err) {
                    self.common.words.filled();
                    self.pause();
                    return Ok(false);
                }
                return Err(err);
            }
        }
        if queue.indexed > 0 {
            self.common.semantic.wake();
        }
        if queue.changed.is_empty() && queue.noted.is_empty() && queue.removed.is_empty() {
            self.finish(&queue)?;
        } else {
            self.queue = Some(queue);
        }
        Ok(true)
    }

    /// Writes one batch: `removed` dropped and `taken` read, each into
    /// the index, until a change is noted ([`crate::index::Journal::notes`]):
    /// the batch is committed at the file it's on, at least the first, so
    /// what was noted is written next, never after the rest. How many it
    /// read again, and how many of `taken` it went through.
    fn write_batch(
        &mut self,
        queue: &Queue,
        removed: &[(String, i64)],
        taken: &[(String, tree::Found)],
    ) -> Result<(usize, usize)> {
        let notes = self.common.journal.notes();
        let mut read_through = taken.len();
        let mut indexed = 0;
        let words = self.common.words.clone();
        let mut changes = crate::index::words::Changes::default();
        let tx = self.db.transaction()?;
        for (path, id) in removed {
            tx.execute(
                "INSERT OR REPLACE INTO removed (path, kind, title, at)
                 SELECT path, kind, title, ?2 FROM documents WHERE id = ?1",
                params![id, queue.now],
            )?;
            tx.execute("DELETE FROM documents WHERE id = ?1", [id])?;
            tx.execute("DELETE FROM passages WHERE document = ?1", [id])?;
            changes.push(crate::index::words::Change::DocumentGone(*id));
            tx.execute("DELETE FROM derived WHERE document = ?1", [id])?;
            tx.execute("DELETE FROM links WHERE document = ?1", [id])?;
            tx.execute("DELETE FROM unread WHERE path = ?1", [path])?;
        }
        for (at, (path, file)) in taken.iter().enumerate() {
            if at > 0 && self.common.journal.notes() != notes {
                read_through = at;
                break;
            }
            // As the index holds it now: another process serving the same
            // file may have written it since the walk.
            let held: Option<(i64, i64, i64)> = tx
                .query_row(
                    "SELECT id, size, modified FROM documents WHERE path = ?1",
                    [path],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let id = match held {
                Some((_, size, modified)) if (size, modified) == (file.size, file.modified) => {
                    continue;
                }
                held => held.map(|(id, _, _)| id),
            };
            let id = id.as_ref();
            // A conversation grown since it was indexed: only what it grew
            // by is read and written.
            if let Some(id) = id
                && file.kind == Kind::Conversation
            {
                match grow(
                    &mut Batch {
                        db: &tx,
                        words: &mut changes,
                    },
                    self.tree.as_mut(),
                    *id,
                    path,
                    file,
                ) {
                    Ok(true) => {
                        indexed += 1;
                        continue;
                    }
                    Ok(false) => {}
                    // Read whole instead.
                    Err(err) => crate::warn(sentence!("{} isn't read.", Foreign::new(path)), &err),
                }
            }
            // Gone, grown or a link since the walk: the next walk finds it.
            // Not reached now: tried again at the writer's next turn.
            let read = match self.tree.read(path, file) {
                Ok(read) => read,
                Err(err) => {
                    crate::warn(sentence!("{} isn't read.", Foreign::new(path)), &err);
                    self.common.journal.retry(path);
                    None
                }
            };
            let document = extract::format_of(path).filter(|_| file.kind == Kind::File);
            let most = match (document, file.kind) {
                (Some(_), _) => extract::document_limits().document_max_bytes,
                // A conversation of any length: all of it.
                (None, Kind::Conversation) => u64::MAX,
                (None, Kind::File) => self.opinions.file,
            };
            let Some(read) = read.filter(|r| r.bytes.len() as u64 <= most) else {
                continue;
            };
            // A document's text, extracted in the background, by its name
            // until then; one that can't be read kept by its name with why,
            // so the next refresh needn't try it again until it changes.
            let mut titled = Titled::ByHeading;
            let body: std::borrow::Cow<[u8]> = match (&read.unreadable, document, file.kind) {
                (Some(why), _, _) => {
                    tx.execute(
                        "INSERT OR REPLACE INTO unread (path, why) VALUES (?1, ?2)",
                        params![path, why],
                    )?;
                    titled = Titled::ByName;
                    Default::default()
                }
                (None, Some(_), _) if !self.extractor.available() => {
                    tx.execute(
                        "INSERT OR REPLACE INTO unread (path, why) VALUES (?1, ?2)",
                        params![path, extract::not_installed()],
                    )?;
                    Default::default()
                }
                (None, Some(_), _) => {
                    tx.execute(
                        "INSERT OR REPLACE INTO unread (path, why) VALUES (?1, ?2)",
                        params![path, extract::PENDING],
                    )?;
                    Default::default()
                }
                (None, None, Kind::Conversation) => {
                    tx.execute("DELETE FROM unread WHERE path = ?1", [path])?;
                    (&read.bytes[..]).into()
                }
                (None, None, Kind::File) => (&read.bytes[..]).into(),
            };
            let text = String::from_utf8_lossy(&body);
            let id = match id {
                Some(id) => {
                    tx.execute(
                        "UPDATE documents SET size = ?2, modified = ?3 WHERE id = ?1",
                        params![id, file.size, file.modified],
                    )?;
                    *id
                }
                None => {
                    let added = if queue.walked {
                        queue.now
                    } else {
                        file.modified
                    };
                    tx.execute(
                        "INSERT INTO documents (path, kind, title, size, modified, added)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            path,
                            file.kind.as_str(),
                            title(path, file.kind, &text, titled),
                            file.size,
                            file.modified,
                            added
                        ],
                    )?;
                    // Back, it isn't removed.
                    tx.execute("DELETE FROM removed WHERE path = ?1", [path])?;
                    tx.last_insert_rowid()
                }
            };
            write_body(
                &mut Batch {
                    db: &tx,
                    words: &mut changes,
                },
                id,
                path,
                file.kind,
                &body,
                read.derived,
                titled,
            )?;
            // Where a conversation's next read starts.
            let (kind, since) = read.since.map(tree::Since::kept).unzip();
            tx.execute(
                "UPDATE documents SET since_kind = ?2, since = ?3 WHERE id = ?1",
                params![id, kind, since],
            )?;
            indexed += 1;
        }
        words.commit(tx, changes)?;
        Ok((indexed, read_through))
    }

    /// The queue all written: what's removed trimmed, and a walk's end
    /// noted.
    fn finish(&mut self, queue: &Queue) -> Result<()> {
        if queue.removed_any {
            self.db.execute(
                "DELETE FROM removed WHERE path NOT IN
                 (SELECT path FROM removed ORDER BY at DESC LIMIT ?1)",
                [crate::limits::limits().removed_max],
            )?;
        }
        self.db.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('walked', 1)",
            [],
        )?;
        if queue.walk {
            self.listed();
        }
        self.extract_pending()?;
        Ok(())
    }

    /// Why indexing pauses now, if it does: the disk the index is on has
    /// less free than its reserve.
    fn short_of_room(&self) -> Option<String> {
        let free = free_bytes(&self.common.file)?;
        (free < self.opinions.reserve).then(|| {
            format!(
                "indexing is paused: the disk has {} MiB free, under the {} MiB kept free",
                free >> 20,
                self.opinions.reserve >> 20
            )
        })
    }

    /// Notes a walk finished.
    fn listed(&mut self) {
        self.refreshed = Some(std::time::Instant::now());
        self.common.set_progress(|progress| progress.walked = true);
    }

    /// Extracts the text of the documents waiting for it
    /// ([`extract::PENDING`]), never in a refresh's transaction: a few
    /// at a time, their bytes read now, extracted by a thread of their
    /// own over the index's file, which writes each one's text as it
    /// comes, so no call waits on a parser. One batch at a time: the next
    /// starts at the first refresh after it ends. An index in memory
    /// extracts them here, after its refresh.
    pub(super) fn extract_pending(&mut self) -> Result<()> {
        if self.extracting() {
            return Ok(());
        }
        if let Some(extraction) = self.extracting.take() {
            extraction.stop();
        }
        // The fewest tried first, each counted as it's taken: one a
        // process ended on before it was read waits behind the rest.
        let pending: Vec<(String, i64, i64)> = {
            let mut statement = self.db.prepare(
                "SELECT d.path, d.size, d.modified FROM unread u JOIN documents d ON d.path = u.path
                 WHERE u.why = ?1 ORDER BY u.attempts, d.path LIMIT ?2",
            )?;
            statement
                .query_map(
                    params![extract::PENDING, crate::limits::limits().extract_batch],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?
                .collect::<rusqlite::Result<_>>()?
        };
        for (path, _, _) in &pending {
            self.db.execute(
                "UPDATE unread SET attempts = attempts + 1 WHERE path = ?1",
                [path],
            )?;
        }
        let mut jobs = Vec::new();
        for (path, size, modified) in pending {
            let found = tree::Found {
                kind: Kind::File,
                size,
                modified,
                copy: None,
                thread: None,
            };
            let format = extract::format_of(&path);
            match (format, self.tree.read(&path, &found)) {
                (Some(format), Ok(Some(read))) => jobs.push(Extracting {
                    path,
                    size,
                    modified,
                    format,
                    bytes: read.bytes,
                }),
                // Gone or changed since: the next refresh finds it.
                (_, Ok(None)) | (None, _) => {
                    self.db
                        .execute("DELETE FROM unread WHERE path = ?1", [&path])?;
                }
                (_, Err(err)) => {
                    self.db.execute(
                        "INSERT OR REPLACE INTO unread (path, why) VALUES (?1, ?2)",
                        params![path, format!("{err:#}")],
                    )?;
                }
            }
        }
        if jobs.is_empty() {
            return Ok(());
        }
        let extractor = self.extractor.clone();
        let semantic = self.common.semantic.clone();
        match &self.file {
            None => {
                for job in jobs {
                    let text = extractor.extract(job.format, &job.bytes);
                    write_extracted(&mut self.db, &self.common.words, &job, text)?;
                }
                semantic.wake();
            }
            Some(file) => {
                let file = file.clone();
                let words = self.common.words.clone();
                let stopping = Arc::new(AtomicBool::new(false));
                let stop = stopping.clone();
                let thread = std::thread::spawn(move || {
                    let Ok(mut db) = open_file(&file) else {
                        return;
                    };
                    let _ = db.busy_timeout(std::time::Duration::from_secs(10));
                    for job in jobs {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let text = extractor.extract(job.format, &job.bytes);
                        if let Err(err) = write_extracted(&mut db, &words, &job, text) {
                            crate::warn(sentence!("{} isn't read.", Foreign::new(&job.path)), &err);
                        }
                        semantic.wake();
                    }
                });
                self.extracting = Some(Extraction { thread, stopping });
            }
        }
        Ok(())
    }

    /// Whether documents' text is being extracted in the background
    /// ([`Index::extract_pending`]).
    pub fn extracting(&self) -> bool {
        self.extracting
            .as_ref()
            .is_some_and(|extraction| !extraction.thread.is_finished())
    }
}

/// A batch of documents being extracted in the background, and what
/// tells it to stop.
pub(super) struct Extraction {
    thread: std::thread::JoinHandle<()>,
    stopping: Arc<AtomicBool>,
}

impl Extraction {
    /// Told to stop after the document it's on, and waited for: never
    /// left running past its index.
    pub(super) fn stop(self) {
        self.stopping.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

/// A document waiting for its text, as read when its batch started.
struct Extracting {
    path: String,
    size: i64,
    modified: i64,
    format: extract::Format,
    bytes: Vec<u8>,
}

/// Writes `text`, extracted from `job`'s document, in a short
/// transaction of its own: its body and chunks, or why it has none. Left
/// for the next refresh when the document changed since it was read.
fn write_extracted(
    db: &mut Connection,
    words: &crate::index::words::Words,
    job: &Extracting,
    text: extract::Outcome,
) -> Result<()> {
    let mut changes = crate::index::words::Changes::default();
    // Immediate: the write lock taken before the read, so a refresh
    // committing between them waits, never fails the snapshot.
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let id: Option<i64> = tx
        .query_row(
            "SELECT id FROM documents WHERE path = ?1 AND size = ?2 AND modified = ?3",
            params![job.path, job.size, job.modified],
            |row| row.get(0),
        )
        .optional()?;
    let Some(id) = id else {
        return Ok(());
    };
    match text {
        extract::Outcome::Text(text) => {
            tx.execute("DELETE FROM unread WHERE path = ?1", [&job.path])?;
            write_body(
                &mut Batch {
                    db: &tx,
                    words: &mut changes,
                },
                id,
                &job.path,
                Kind::File,
                text.as_bytes(),
                true,
                Titled::ByHeading,
            )?;
        }
        extract::Outcome::Unread(why) => {
            tx.execute(
                "INSERT OR REPLACE INTO unread (path, why) VALUES (?1, ?2)",
                params![job.path, why],
            )?;
        }
        // Still waiting, tried once more: the sandbox may start next time.
        extract::Outcome::Retry(_) => {}
    }
    words.commit(tx, changes)
}

/// A title's most characters; the rest is dropped.
const MAX_TITLE: usize = 200;

/// A document's title: its first heading (`# ...`), else its
/// conversation's folder or its file's name, at most [`MAX_TITLE`]
/// characters.
pub(super) fn title(path: &str, kind: Kind, body: &str, titled: Titled) -> String {
    whole_title(path, kind, body, titled)
        .chars()
        .take(MAX_TITLE)
        .collect()
}

fn whole_title(path: &str, kind: Kind, body: &str, titled: Titled) -> String {
    let heading = match titled {
        Titled::ByHeading => body
            .lines()
            .take(40)
            .find_map(|line| line.strip_prefix("# "))
            .map(str::trim)
            .filter(|h| !h.is_empty()),
        Titled::ByName => None,
    };
    if let Some(heading) = heading {
        return heading.to_string();
    }
    let parts: Vec<&str> = path.split('/').collect();
    match kind {
        Kind::Conversation if parts.len() > 2 => parts[parts.len() - 2].to_string(),
        // Every skill's file has one name: a skill is called by its folder.
        _ if parts.len() > 1 && parts[parts.len() - 1] == link_harness_wire::skill::FILE => {
            parts[parts.len() - 2].to_string()
        }
        _ => parts[parts.len() - 1].to_string(),
    }
}

/// Where a document's title comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Titled {
    /// Its first heading, else its name.
    ByHeading,
    /// Its name alone: its text is unread, so a heading in it may be a
    /// message's, not the document's.
    ByName,
}

/// What a walk found to write, taken a batch at a time.
pub(super) struct Queue {
    /// What's new or changed, the newest last: its ID, if it has one, read
    /// as the batch writes it.
    changed: Vec<(String, tree::Found)>,
    /// What the journal noted of it since, the newest last: written ahead
    /// of `changed`, in batches of its own ([`Index::drain`]).
    noted: Vec<(String, tree::Found)>,
    /// What's gone, by path and ID: dropped first, at most
    /// [`crate::limits::Limits::removed_batch`] a batch.
    removed: Vec<(String, i64)>,
    /// Whether a walk queued it: else the journal's paths.
    walk: bool,
    /// Whether a walk had finished before this one: else what it finds was
    /// there before memory.
    walked: bool,
    /// When the walk began, as the index keeps times.
    now: i64,
    /// How many it's read again so far.
    indexed: usize,
    /// Whether it dropped any.
    removed_any: bool,
}

/// The bytes a batch reads, written at `speed` bytes a second, to take
/// about `millis` milliseconds, at most `most`; none before a speed is
/// known, so the first batch is one file, and those after grow to it.
fn batch_bytes(speed: Option<f64>, millis: u64, most: u64) -> u64 {
    let took = std::time::Duration::from_millis(millis).as_secs_f64();
    speed.map_or(0, |speed| ((speed * took) as u64).min(most))
}

/// What memory's status says while SQLite finds the disk full.
const PAUSED_FULL: &str = "indexing is paused: the disk is full";

/// Whether SQLite's `err` is the disk being full.
fn disk_full(err: &anyhow::Error) -> bool {
    err.downcast_ref::<rusqlite::Error>()
        .is_some_and(|err| err.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull))
}

/// The bytes free to this user on the file system holding `file`; none
/// when it can't be told.
fn free_bytes(file: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let dir = file.parent().filter(|d| !d.as_os_str().is_empty())?;
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: a NUL-ended path and a statvfs this function owns.
        if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        None
    }
}

/// Now, in nanoseconds since 1970, as the index keeps times.
fn now_nanos() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Tantivy commit that finds the disk full pauses indexing, said in
    /// its status: the writer neither sets the words right nor writes
    /// again until it's waited, then catches up.
    #[test]
    fn a_full_disk_pauses_without_setting_the_words_right() {
        let root = link_test_kit::TempDir::new("memory-full");
        std::fs::create_dir_all(root.join("memory")).unwrap();
        std::fs::write(root.join("memory/boat.md"), "New sails.\n").unwrap();
        let mut index = Index::open(&root, Some(&root.join("index.sqlite"))).unwrap();
        index.refresh().unwrap();
        let words = index.common.words.clone();
        words.fail_commit.store(true, Ordering::SeqCst);
        std::fs::write(root.join("memory/later.md"), "We fly by zeppelin.\n").unwrap();
        index.common.noted("memory/later.md");
        index.turn().unwrap();
        assert!(words.behind() && words.full());
        let paused = index.common.progress().paused;
        assert_eq!(paused.as_deref(), Some(PAUSED_FULL));
        // Its next turns wait: the words aren't set right meanwhile.
        assert!(!index.turn().unwrap());
        assert!(words.behind(), "set right while the disk is full");
        assert_eq!(index.common.progress().paused.as_deref(), Some(PAUSED_FULL));
        // Waited: the words set right, and the file found.
        *words.filled.lock().unwrap() = None;
        while index.turn().unwrap() {}
        assert!(!words.behind());
        assert_eq!(index.search("zeppelin", None, 10).unwrap().len(), 1);
        assert_eq!(index.common.progress().paused, None);
    }

    /// A batch reads what its last one's speed says fits its time, at
    /// most its bytes; the first, its speed unknown, a file.
    #[test]
    fn a_batch_is_sized_by_the_last_ones_speed() {
        assert_eq!(batch_bytes(None, 1000, 8 << 20), 0);
        assert_eq!(batch_bytes(Some(2_000_000.0), 500, 8 << 20), 1_000_000);
        assert_eq!(batch_bytes(Some(1e12), 1000, 8 << 20), 8 << 20);
    }

    /// What's gone is dropped a bounded batch at a time, as what's
    /// changed is read: never all of it in one transaction.
    #[test]
    fn what_is_gone_is_dropped_in_bounded_batches() {
        let root = link_test_kit::TempDir::new("memory-removed");
        std::fs::create_dir_all(root.join("memory")).unwrap();
        let mut index = Index::open(&root, Some(&root.join("index.sqlite"))).unwrap();
        index.refresh().unwrap();
        let most = crate::limits::limits().removed_batch;
        let gone = (0..=most as i64).map(|id| (format!("memory/{id}.md"), id));
        index.queue = Some(Queue {
            changed: Vec::new(),
            noted: Vec::new(),
            removed: gone.collect(),
            walk: false,
            walked: true,
            now: now_nanos(),
            indexed: 0,
            removed_any: false,
        });
        assert!(index.turn().unwrap(), "one batch drops them all");
        assert_eq!(index.queue.as_ref().map(|q| q.removed.len()), Some(1));
        assert!(!index.turn().unwrap());
        assert!(index.queue.is_none());
    }

    /// Another process serving the same file indexes what this one's
    /// walk queued: this one's batch writes it as it's held now, never
    /// failing on the path being in the index already.
    #[test]
    fn another_writer_of_the_same_file_writes_in_turn() {
        let root = link_test_kit::TempDir::new("memory-two");
        std::fs::create_dir_all(root.join("memory")).unwrap();
        std::fs::write(root.join("memory/boat.md"), "New sails.\n").unwrap();
        let file = root.join("index.sqlite");
        let mut ours = Index::open(&root, Some(&file)).unwrap();
        let mut theirs = Index::open(&root, Some(&file)).unwrap();
        // Ours queues it new; theirs writes it first.
        ours.walk().unwrap();
        theirs.refresh().unwrap();
        while ours.turn().unwrap() {}
        assert!(ours.queue.is_none());
        assert_eq!(ours.search("sails", None, 10).unwrap().len(), 1);
        assert_eq!(ours.list(&crate::Filter::default(), 10).unwrap().total, 1);
    }

    #[test]
    fn a_title_is_the_heading_else_the_name() {
        let by = Titled::ByHeading;
        assert_eq!(
            title("a/b.md", Kind::File, "text\n# Plans \nmore", by),
            "Plans"
        );
        assert_eq!(title("a/b.md", Kind::File, "no heading", by), "b.md");
        assert_eq!(
            title("chats/t42/conversation.md", Kind::Conversation, "", by),
            "t42"
        );
    }
}
