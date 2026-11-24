//! What's embedded next ([`embed_batch`]), and the one thread that
//! embeds for every index a process serves ([`Embedding`]).
//!
//! Every passage not embedded yet waits in the index's `queue`, ordered
//! by its priority: conversations and notes first, then documents whose
//! text was extracted, each newest first, a document read lately moved up
//! beside the newest. Triggers keep it as passages are written and
//! embedded, so picking a batch reads that many rows of the queue's index
//! ([`PICK`]), however many wait. One thread takes a batch from each index
//! in turn, so one agent's large corpus doesn't hold up another's new
//! note.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::semantic::{Embedder, Shared, hash, normalized, title_words};

/// The passages a batch takes, best first: read from the queue's own
/// index (`queue_order`), so it reads as many rows as it takes.
pub const PICK: &str = "SELECT passage FROM queue INDEXED BY queue_order
     ORDER BY priority, passage LIMIT ?1";

/// Where in the queue a passage whose file no longer holds what the index
/// says waits: after every other, until the index writes it again.
pub(crate) const DEFERRED: i64 = 8_000_000_000_000_000_000;

/// The priority a passage of a document read lately takes: beside the
/// newest conversation's, as of when it was read.
fn read_lately() -> i64 {
    -std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64)
}

/// Embeds up to `limit` passages from the front of the queue, each read
/// from its file, first moving up those of the documents `shared` says
/// were read lately. How many it took off the front: embedded, or put
/// back as its file no longer holds what the index says.
pub fn embed_batch(
    db: &Connection,
    files: &dyn crate::tree::Files,
    embedder: &dyn Embedder,
    kept: &crate::vectors::Kept,
    shared: Option<&Shared>,
    limit: usize,
) -> Result<usize> {
    if let Some(shared) = shared {
        let read = shared.take_read();
        if !read.is_empty() {
            let mut moved = db.prepare_cached(
                "UPDATE queue SET priority = min(priority, ?2)
                 WHERE passage IN (SELECT id FROM passages WHERE document = ?1)",
            )?;
            for document in read {
                moved.execute(params![document, read_lately()])?;
            }
        }
    }
    let picked: Vec<i64> = db
        .prepare_cached(PICK)?
        .query_map([limit as i64], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut pending = Vec::new();
    let mut put_back = Vec::new();
    for passage in picked {
        let found: Option<(i64, i64, i64, i64, String, String, i64)> = db
            .prepare_cached(
                "SELECT p.document, p.start, p.end, p.hash, d.title, d.path, q.priority
                 FROM passages p JOIN documents d ON d.id = p.document
                 JOIN queue q ON q.passage = p.id WHERE p.id = ?1",
            )?
            .query_row([passage], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .optional()?;
        let Some((document, start, end, passage_hash, title, path, priority)) = found else {
            continue;
        };
        // Read from its file: one changed since it was indexed waits at the
        // back until the index writes it again.
        let bytes = crate::source::range(db, files, document, &path, start as u64, end as u64)?;
        match bytes.filter(|bytes| hash(bytes) == passage_hash) {
            Some(bytes) => {
                let text = format!(
                    "{}\n\n{}",
                    title_words(&title),
                    String::from_utf8_lossy(&bytes)
                );
                pending.push((passage, document, passage_hash, title, text));
            }
            None if priority < DEFERRED => put_back.push(passage),
            None => {}
        }
    }
    for passage in &put_back {
        db.execute(
            "UPDATE queue SET priority = ?2 WHERE passage = ?1",
            params![passage, DEFERRED],
        )?;
    }
    if pending.is_empty() {
        return Ok(put_back.len());
    }
    let texts: Vec<String> = pending.iter().map(|(.., text)| text.clone()).collect();
    let vectors = embedder.embed(&texts)?;
    let tx = db.unchecked_transaction()?;
    for ((id, document, passage_hash, title, _), vector) in pending.iter().zip(vectors) {
        // Its vectors at a slot of their own, written before the index
        // names it: a crash between leaves the slot to be taken again.
        let slot = crate::vectors::take_slot(&tx)?;
        kept.write(slot, *id, &kept.reduce(&normalized(vector)))?;
        // A passage rewritten meanwhile is gone, or another under its id
        // now, with other bytes or another title: left to embed again.
        let landed = tx.execute(
            "UPDATE passages SET slot = ?2
             WHERE id = ?1 AND document = ?3 AND hash = ?4
               AND (SELECT title FROM documents WHERE id = ?3) = ?5",
            params![id, slot as i64, document, passage_hash, title],
        )?;
        if landed == 0 {
            tx.execute("INSERT INTO free_slots (slot) VALUES (?1)", [slot as i64])?;
        }
    }
    tx.commit()?;
    Ok(pending.len() + put_back.len())
}

/// Passages embedded in one request.
pub(crate) const BATCH: usize = 16;

/// How long the thread waits, with nothing to embed, before it looks
/// again unwoken: another server over the same file may have added
/// passages.
const IDLE: Duration = Duration::from_secs(30);

/// How long an index whose embedding failed waits before it's tried again.
const AGAIN: Duration = Duration::from_secs(5);

/// An index whose model answers, as the embedding thread embeds it: its
/// own connection to the index's file, what reads its files, its model,
/// and what it shares with its searches.
pub(crate) struct Job {
    pub(crate) db: Connection,
    pub(crate) files: Arc<dyn crate::tree::Files>,
    pub(crate) embedder: Arc<dyn Embedder>,
    pub(crate) kept: crate::vectors::Kept,
    pub(crate) shared: Arc<Shared>,
    /// The model's server, kept running while it's embedded with.
    pub(crate) _served: Option<std::process::Child>,
}

/// The one embedding thread of a process: the indexes it embeds, taken
/// in turn, and what wakes it.
pub struct Embedding {
    jobs: Mutex<Vec<(Job, Option<Instant>)>>,
    woken: Mutex<bool>,
    wake: Condvar,
}

impl Embedding {
    /// The process's, its thread started the first time it's asked for.
    pub fn of_process() -> &'static Embedding {
        static ONE: std::sync::LazyLock<&'static Embedding> = std::sync::LazyLock::new(|| {
            let embedding: &'static Embedding = Box::leak(Box::new(Embedding {
                jobs: Mutex::new(Vec::new()),
                woken: Mutex::new(false),
                wake: Condvar::new(),
            }));
            std::thread::spawn(move || embedding.run());
            embedding
        });
        &ONE
    }

    /// Embeds `job`'s index from now on, in turn with the rest.
    pub(crate) fn add(&self, job: Job) {
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((job, None));
        self.woken();
    }

    /// Its next turn now.
    pub(crate) fn woken(&self) {
        *self.woken.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_all();
    }

    /// Takes a batch from each index in turn, dropping one whose index
    /// closed, and sleeps when none has one.
    fn run(&self) {
        loop {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            jobs.retain(|(job, _)| !job.shared.stopped());
            let mut busy = false;
            for (job, again) in jobs.iter_mut() {
                if again.is_some_and(|at| Instant::now() < at) {
                    continue;
                }
                match embed_batch(
                    &job.db,
                    job.files.as_ref(),
                    job.embedder.as_ref(),
                    &job.kept,
                    Some(&job.shared),
                    BATCH,
                ) {
                    Ok(taken) => {
                        job.shared.fail(None);
                        *again = None;
                        busy |= taken > 0;
                    }
                    Err(err) => {
                        job.shared.fail(Some(format!("{err:#}")));
                        *again = Some(Instant::now() + AGAIN);
                    }
                }
            }
            let waiting = jobs.iter().filter_map(|(_, again)| *again).min();
            drop(jobs);
            if busy {
                continue;
            }
            let longest = waiting.map_or(IDLE, |at| at.saturating_duration_since(Instant::now()));
            let woken = self.woken.lock().unwrap_or_else(|e| e.into_inner());
            let (mut woken, _) = self
                .wake
                .wait_timeout_while(woken, longest.min(IDLE), |woken| !*woken)
                .unwrap_or_else(|e| e.into_inner());
            *woken = false;
        }
    }
}
