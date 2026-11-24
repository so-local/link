//! Search by words ([`Words`]): Tantivy's inverted index, in a folder
//! beside the index's SQLite file, keyed by passage and storing no text.
//! It ranks passages by BM25 with block-max pruning, so a search reads
//! only the postings that can still reach its best, however many
//! passages hold a word.
//!
//! The index's one writer writes it with SQLite, in the same batch
//! ([`Words::commit`]): SQLite commits first, then Tantivy, its commit
//! naming the batch's generation (`meta.words`). A crash between them
//! leaves Tantivy a generation behind, found at the next start and set
//! right ([`Words::reconcile`]).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};
use tantivy::query::{
    Bm25StatisticsProvider, BooleanQuery, EnableScoring, Occur, Query, TermQuery, Weight,
};
use tantivy::schema::{
    FAST, Field, INDEXED, IndexRecordOption, STRING, Schema, TextFieldIndexing, TextOptions,
};
use tantivy::tokenizer::{
    AsciiFoldingFilter, Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer,
    TextAnalyzer,
};
use tantivy::{
    Index, IndexReader, IndexWriter, ReloadPolicy, Score, Searcher, TantivyDocument, Term,
};

/// The name the index's analyzer is registered by: text split at what
/// isn't a letter or a digit, lowercased, its accents dropped, and each
/// word stemmed as English.
const ANALYZER: &str = "link";

/// The file naming what Tantivy has committed.
const META: &str = "meta.json";

/// The longest word kept, in bytes: a longer token is no word.
const LONGEST_WORD: usize = 40;

/// How many passages a search scores between looks at its deadline.
const SCORED_BETWEEN_LOOKS: u32 = 256;

/// What the folder of an index's words is called: its SQLite file's
/// name and this.
const FOLDER: &str = ".words";

/// The folder of the words of the index `file`.
pub(crate) fn folder_of(file: &Path) -> PathBuf {
    let mut path = file.as_os_str().to_owned();
    path.push(FOLDER);
    PathBuf::from(path)
}

/// What each kind of entry is, by its `tag`: a passage's words, or a
/// document's title.
#[derive(Clone, Copy)]
enum Tag {
    Passage,
    Title,
}

impl Tag {
    fn as_str(self) -> &'static str {
        match self {
            Tag::Passage => "passage",
            Tag::Title => "title",
        }
    }
}

/// The fields of the words' schema.
#[derive(Clone, Copy)]
struct Fields {
    /// What the entry is ([`Tag`]).
    tag: Field,
    /// A passage's ID, in the SQLite file.
    passage: Field,
    /// The document a passage or a title is of.
    document: Field,
    /// The document whose title it is, on a title alone: what a title is
    /// replaced by.
    titled: Field,
    /// A passage's words.
    body: Field,
    /// A title's words.
    title: Field,
}

fn schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let words = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(ANALYZER)
            .set_index_option(IndexRecordOption::WithFreqs),
    );
    let fields = Fields {
        tag: builder.add_text_field("tag", STRING),
        passage: builder.add_u64_field("passage", INDEXED | FAST),
        document: builder.add_u64_field("document", INDEXED | FAST),
        titled: builder.add_u64_field("titled", INDEXED),
        body: builder.add_text_field("body", words.clone()),
        title: builder.add_text_field("title", words),
    };
    (builder.build(), fields)
}

fn analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(LONGEST_WORD))
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter)
        .filter(Stemmer::new(Language::English))
        .build()
}

/// A change to the words, made with SQLite's in one batch and applied in
/// order ([`Words::commit`]).
pub(crate) enum Change {
    /// A passage written, with its text.
    Passage {
        passage: i64,
        document: i64,
        text: String,
    },
    /// A passage gone.
    PassageGone(i64),
    /// A document's title, in place of the one it had.
    Title { document: i64, title: String },
    /// A document gone: its passages and its title.
    DocumentGone(i64),
    /// Every title gone, to be written again.
    TitlesGone,
}

/// The changes of one batch, in order.
#[derive(Default)]
pub(crate) struct Changes(Vec<Change>);

impl Changes {
    pub(crate) fn push(&mut self, change: Change) {
        self.0.push(change);
    }
}

/// What the writer of the words holds: Tantivy's writer, taken at the
/// first write, and what it may spend.
struct Writing {
    writer: Option<IndexWriter>,
    /// The bytes its indexing threads buffer before they write a segment.
    budget: usize,
    threads: usize,
}

/// An index's words: Tantivy's index, a reader every search shares, and
/// its one writer.
pub(crate) struct Words {
    index: Index,
    reader: IndexReader,
    /// Its `meta.json` as the reader last loaded it: when it changed and
    /// its length.
    seen: Mutex<Option<(Option<std::time::SystemTime>, u64)>>,
    fields: Fields,
    analyzer: TextAnalyzer,
    writing: Mutex<Writing>,
    /// Whether the words may differ from SQLite's passages: a commit of
    /// Tantivy's was lost, or the folder is new beside an index that
    /// holds passages. The writer sets them right at its next turn.
    behind: AtomicBool,
    /// When a commit, SQLite's or Tantivy's, last found the disk full
    /// ([`Words::full`]).
    pub(crate) filled: Mutex<Option<Instant>>,
    /// Whether the next Tantivy commit fails as on a full disk, in a test.
    #[cfg(test)]
    pub(crate) fail_commit: AtomicBool,
    folder: PathBuf,
}

/// The least a Tantivy indexing thread buffers, in bytes: Tantivy's own
/// least.
const LEAST_BUDGET: usize = 15_000_000;

impl Words {
    /// The words of the index `file`, whose SQLite connection `db` says
    /// what generation it holds: opened, or made where there are none or
    /// they can't be read (moved aside, as an index of another schema is).
    pub(crate) fn open(file: &Path, db: &Connection) -> Result<Words> {
        let folder = folder_of(file);
        let (schema, fields) = schema();
        let index = match open_index(&folder, &schema) {
            Ok(index) => index,
            Err(err) => {
                if folder.exists() {
                    crate::warn(
                        link_format::sentence!("The index's words are made again."),
                        &err,
                    );
                    set_aside(&folder)?;
                }
                link_files::create_private_dir(&folder)?;
                Index::create_in_dir(&folder, schema.clone())
                    .with_context(|| format!("making {}", folder.display()))?
            }
        };
        index.tokenizers().register(ANALYZER, analyzer());
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let payload = index.load_metas()?.payload;
        let committed: i64 = payload.and_then(|p| p.parse().ok()).unwrap_or(0);
        let words = Words {
            index,
            reader,
            fields,
            analyzer: analyzer(),
            writing: Mutex::new(Writing {
                writer: None,
                budget: LEAST_BUDGET * 4,
                threads: 1,
            }),
            behind: AtomicBool::new(false),
            filled: Mutex::new(None),
            #[cfg(test)]
            fail_commit: AtomicBool::new(false),
            folder,
            seen: Mutex::new(None),
        };
        words
            .behind
            .store(committed != generation(db)?, Ordering::SeqCst);
        Ok(words)
    }

    /// Its writer spends `budget` bytes over `threads` threads, from the
    /// next it takes.
    pub(crate) fn spend(&self, budget: u64, threads: usize) {
        let mut writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        writing.threads = threads.max(1);
        writing.budget = (budget as usize).max(LEAST_BUDGET * writing.threads);
    }

    /// Its folder.
    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }

    /// Why it can't be written now, if it can't: another process writes
    /// this index.
    pub(crate) fn unwritable(&self) -> Option<String> {
        let mut writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        writing
            .writer(&self.index)
            .err()
            .map(|_| "indexing is paused: another process writes this index".to_string())
    }

    /// Lets Tantivy's writer go, with its lock, once its merges end: the
    /// index has nothing left to write, or its writer is gone, so another
    /// writer of the same file, in this process or another, may write it.
    pub(crate) fn release(&self) {
        let mut writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(writer) = writing.writer.take()
            && let Err(err) = writer.wait_merging_threads()
        {
            crate::warn(
                link_format::sentence!("The index's words aren't merged."),
                &err.into(),
            );
        }
    }

    /// A commit found the disk full: the writer pauses a while.
    pub(crate) fn filled(&self) {
        *self.filled.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }

    /// Whether a commit found the disk full within the last
    /// [`crate::limits::Limits::full_retry_secs`]: the writer waits.
    pub(crate) fn full(&self) -> bool {
        let wait = std::time::Duration::from_secs(crate::limits::limits().full_retry_secs);
        self.filled
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|at| at.elapsed() < wait)
    }

    /// Whether the words may differ from SQLite's passages, for the
    /// writer to set right ([`Words::reconcile`]).
    pub(crate) fn behind(&self) -> bool {
        self.behind.load(Ordering::SeqCst)
    }

    /// Commits `tx` with `changes`: the generation it holds raised, the
    /// changes given to Tantivy's writer, SQLite committed, then Tantivy,
    /// its commit naming the generation. A transaction SQLite refuses
    /// leaves Tantivy as it was; a commit Tantivy then loses leaves it
    /// [behind](Words::behind), set right at the writer's next turn.
    pub(crate) fn commit(&self, tx: Transaction<'_>, changes: Changes) -> Result<()> {
        if changes.0.is_empty() {
            tx.commit()?;
            return Ok(());
        }
        let mut writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let writer = writing.writer(&self.index)?;
        let generation: i64 = tx.query_row(
            "INSERT INTO meta (key, value) VALUES ('words', 1)
             ON CONFLICT (key) DO UPDATE SET value = value + 1 RETURNING value",
            [],
            |row| row.get(0),
        )?;
        let f = self.fields;
        for change in changes.0 {
            match change {
                Change::Passage {
                    passage,
                    document,
                    text,
                } => {
                    let mut doc = TantivyDocument::new();
                    doc.add_text(f.tag, Tag::Passage.as_str());
                    doc.add_u64(f.passage, passage as u64);
                    doc.add_u64(f.document, document as u64);
                    doc.add_text(f.body, &text);
                    writer.add_document(doc)?;
                }
                Change::PassageGone(passage) => {
                    writer.delete_term(Term::from_field_u64(f.passage, passage as u64));
                }
                Change::Title { document, title } => {
                    writer.delete_term(Term::from_field_u64(f.titled, document as u64));
                    let mut doc = TantivyDocument::new();
                    doc.add_text(f.tag, Tag::Title.as_str());
                    doc.add_u64(f.document, document as u64);
                    doc.add_u64(f.titled, document as u64);
                    doc.add_text(f.title, &title);
                    writer.add_document(doc)?;
                }
                Change::DocumentGone(document) => {
                    writer.delete_term(Term::from_field_u64(f.document, document as u64));
                }
                Change::TitlesGone => {
                    writer.delete_term(Term::from_field_text(f.tag, Tag::Title.as_str()));
                }
            }
        }
        if let Err(err) = tx.commit() {
            writer.rollback()?;
            return Err(err.into());
        }
        let committed = (|| -> tantivy::Result<()> {
            #[cfg(test)]
            if self.fail_commit.swap(false, Ordering::SeqCst) {
                let full = std::io::Error::from_raw_os_error(libc::ENOSPC);
                return Err(tantivy::TantivyError::IoError(std::sync::Arc::new(full)));
            }
            let mut prepared = writer.prepare_commit()?;
            prepared.set_payload(&generation.to_string());
            prepared.commit()?;
            Ok(())
        })();
        if let Err(err) = committed {
            self.behind.store(true, Ordering::SeqCst);
            if disk_full(&err) {
                self.filled();
            }
            let _ = writer.rollback();
            crate::warn(
                link_format::sentence!("The index's words aren't committed."),
                &err.into(),
            );
            return Ok(());
        }
        drop(writing);
        self.reader.reload()?;
        Ok(())
    }

    /// Sets the words right by SQLite's passages, after a crash between
    /// the two commits or beside a copy whose words came from elsewhere:
    /// every title written again, a passage Tantivy holds that SQLite
    /// doesn't dropped, and one SQLite holds that Tantivy doesn't dropped
    /// from SQLite too, its document to be read again. The paths to read
    /// again.
    pub(crate) fn reconcile(&self, db: &mut Connection) -> Result<Vec<String>> {
        self.reader.reload()?;
        let searcher = self.reader.searcher();
        // Tantivy's passages, as a set of IDs.
        let mut held = Bits::default();
        for segment in searcher.segment_readers() {
            let ids = segment.fast_fields().u64("passage")?;
            let alive = segment.alive_bitset();
            for doc in 0..segment.max_doc() {
                if alive.is_some_and(|a| a.is_deleted(doc)) {
                    continue;
                }
                if let Some(id) = ids.first(doc) {
                    held.set(id);
                }
            }
        }
        let tx = db.transaction()?;
        let mut changes = Changes::default();
        changes.push(Change::TitlesGone);
        {
            let mut titles = tx.prepare("SELECT id, title FROM documents")?;
            for row in titles.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
                let (document, title) = row?;
                changes.push(Change::Title { document, title });
            }
        }
        let mut seen = Bits::default();
        let mut missing: BTreeSet<i64> = BTreeSet::new();
        {
            let mut passages = tx.prepare("SELECT id, document FROM passages")?;
            let mut rows = passages.query([])?;
            while let Some(row) = rows.next()? {
                let (id, document): (i64, i64) = (row.get(0)?, row.get(1)?);
                seen.set(id as u64);
                if !held.has(id as u64) {
                    missing.insert(document);
                }
            }
        }
        for extra in held.minus(&seen) {
            changes.push(Change::PassageGone(extra as i64));
        }
        let mut paths = Vec::new();
        for document in missing {
            let ids: Vec<i64> = tx
                .prepare_cached("SELECT id FROM passages WHERE document = ?1")?
                .query_map([document], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in ids.into_iter().filter(|id| !held.has(*id as u64)) {
                tx.execute("DELETE FROM passages WHERE id = ?1", [id])?;
            }
            // Read again whole: its size and time never match what's
            // there, and a conversation's next read starts at its start.
            tx.execute(
                "UPDATE documents SET modified = -1, since_kind = NULL, since = NULL
                 WHERE id = ?1",
                [document],
            )?;
            if let Some(path) = tx
                .query_row(
                    "SELECT path FROM documents WHERE id = ?1",
                    [document],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            {
                paths.push(path);
            }
        }
        self.behind.store(false, Ordering::SeqCst);
        self.commit(tx, changes)?;
        Ok(paths)
    }

    /// A searcher of what's committed, here or by another writer of the
    /// same file: reloaded when its `meta.json`, which every commit and
    /// merge replaces, has changed since.
    pub(crate) fn searcher(&self) -> Searcher {
        let now = std::fs::metadata(self.folder.join(META))
            .ok()
            .map(|m| (m.modified().ok(), m.len()));
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if *seen != now && self.reader.reload().is_ok() {
            *seen = now;
        }
        self.reader.searcher()
    }

    /// `query`'s words as the index keeps them, each once, in the order
    /// first written.
    pub(crate) fn terms(&self, query: &str) -> Vec<String> {
        let mut analyzer = self.analyzer.clone();
        let mut stream = analyzer.token_stream(query);
        let mut terms: Vec<String> = Vec::new();
        while stream.advance() {
            let text = &stream.token().text;
            if !terms.contains(text) {
                terms.push(text.clone());
            }
        }
        terms
    }

    /// How many passages hold `term`, by its postings' count: one lookup.
    pub(crate) fn holding(&self, searcher: &Searcher, term: &str) -> Result<u64> {
        Ok(searcher.doc_freq(&Term::from_field_text(self.fields.body, term))?)
    }

    /// How many entries of `tag` the index holds, by the postings' count
    /// of the tag.
    fn count(&self, searcher: &Searcher, tag: Tag) -> Result<u64> {
        Ok(searcher.doc_freq(&Term::from_field_text(self.fields.tag, tag.as_str()))?)
    }

    /// The `most` documents whose passages best match `terms`, each by its
    /// best passage, best first: holding every word (`every`), else any;
    /// only documents `keep` lets through. Pruned by the best so far, so a
    /// common word's search reads only the blocks of its postings that can
    /// still reach them; stopped at `deadline` with what it has.
    pub(crate) fn best(
        &self,
        searcher: &Searcher,
        terms: &[String],
        every: bool,
        keep: &dyn Fn(i64) -> bool,
        most: usize,
        deadline: Instant,
    ) -> Result<Best> {
        let passages = self.count(searcher, Tag::Passage)?;
        let stats = Stats { searcher, passages };
        let query = query(self.fields.body, terms, every);
        let weight = query.weight(EnableScoring::enabled_from_statistics_provider(
            &stats, searcher,
        ))?;
        rank(
            searcher,
            weight.as_ref(),
            Tag::Passage,
            keep,
            most,
            deadline,
        )
    }

    /// The `most` documents whose titles best match `terms`, holding any,
    /// best first, only those `keep` lets through, stopped at `deadline`.
    pub(crate) fn titled(
        &self,
        searcher: &Searcher,
        terms: &[String],
        keep: &dyn Fn(i64) -> bool,
        most: usize,
        deadline: Instant,
    ) -> Result<Best> {
        let query = query(self.fields.title, terms, false);
        let weight = query.weight(EnableScoring::enabled_from_searcher(searcher))?;
        rank(searcher, weight.as_ref(), Tag::Title, keep, most, deadline)
    }

    /// Which of `terms` `title` holds, split as the index splits it.
    pub(crate) fn in_title(&self, title: &str, terms: &[String]) -> Vec<String> {
        let words = self.terms(title);
        terms
            .iter()
            .filter(|term| words.contains(term))
            .cloned()
            .collect()
    }

    /// How rare a word held by `holding` of the index's passages is:
    /// BM25's inverse document frequency, as Tantivy reckons it.
    pub(crate) fn rarity(&self, searcher: &Searcher, holding: u64) -> Result<f64> {
        let all = self.count(searcher, Tag::Passage)?.max(holding) as f64;
        let n = holding as f64;
        Ok((1.0 + (all - n + 0.5) / (n + 0.5)).ln())
    }

    /// Copies what's committed of the words into the folder `to`, made
    /// new, each file linked where it can be, else copied; then runs
    /// `also`, a copy of the SQLite file, before any batch commits, so the
    /// two agree. Merges go on meanwhile, so a file gone as it's linked
    /// starts it over, and what's linked is opened to prove it whole.
    pub(crate) fn snapshot(&self, to: &Path, also: impl FnOnce() -> Result<()>) -> Result<()> {
        let _writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut tries = 0;
        loop {
            tries += 1;
            let _ = std::fs::remove_dir_all(to);
            link_files::create_private_dir(to)?;
            match link_files_in(&self.folder, to).and_then(|()| {
                let (schema, _) = schema();
                let index = open_index(to, &schema)?;
                let _reader: IndexReader = index
                    .reader_builder()
                    .reload_policy(ReloadPolicy::Manual)
                    .try_into()?;
                Ok(())
            }) {
                Ok(()) => return also(),
                Err(err) if tries >= SNAPSHOT_TRIES => {
                    let _ = std::fs::remove_dir_all(to);
                    return Err(err.context("copying the index's words"));
                }
                Err(_) => {}
            }
        }
    }
}

/// The `most` documents `weight` scores best, of entries of `tag` (a
/// passage names itself; a title, no passage), each by its best, only those
/// `keep` lets through. Tantivy calls back only with what beats the
/// threshold it's given, the last kept's score, skipping blocks of postings
/// that can't. The clock is looked at with the first entry scored, then
/// every [`SCORED_BETWEEN_LOOKS`], and before each segment past the first:
/// past `deadline` it stops with what it has, the first scored kept.
fn rank(
    searcher: &Searcher,
    weight: &dyn Weight,
    tag: Tag,
    keep: &dyn Fn(i64) -> bool,
    most: usize,
    deadline: Instant,
) -> Result<Best> {
    let mut top = Top::new(most);
    let mut stopped = false;
    let mut scored = 0u64;
    for (n, segment) in searcher.segment_readers().iter().enumerate() {
        if n > 0 && Instant::now() >= deadline {
            stopped = true;
            break;
        }
        let ids = match tag {
            Tag::Passage => Some(segment.fast_fields().u64("passage")?),
            Tag::Title => None,
        };
        let documents = segment.fast_fields().u64("document")?;
        let alive = segment.alive_bitset();
        weight.for_each_pruning(top.threshold(), segment, &mut |doc, score| {
            if stopped {
                return Score::MAX;
            }
            if !alive.is_some_and(|a| a.is_deleted(doc))
                && let Some(document) = documents.first(doc)
                && keep(document as i64)
            {
                let passage = ids.as_ref().and_then(|ids| ids.first(doc));
                if ids.is_none() || passage.is_some() {
                    top.offer(document as i64, passage.unwrap_or(0) as i64, score);
                }
            }
            if scored.is_multiple_of(u64::from(SCORED_BETWEEN_LOOKS)) && Instant::now() >= deadline
            {
                stopped = true;
            }
            scored += 1;
            match stopped {
                true => Score::MAX,
                false => top.threshold(),
            }
        })?;
        if stopped {
            break;
        }
    }
    Ok(top.into_best(stopped, scored))
}

/// How many times a snapshot of the words is tried while merges remove
/// what it links.
const SNAPSHOT_TRIES: usize = 8;

/// Links, or copies, each file of `from` into `to`: meta.json first, so
/// every file it names was there as it was read; one gone meanwhile is an
/// error.
fn link_files_in(from: &Path, to: &Path) -> Result<()> {
    link_or_copy(&from.join(META), &to.join(META))?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Its locks and its own list of files are its writer's, and
        // meta.json is linked.
        if name == META || name.starts_with('.') || !entry.file_type()?.is_file() {
            continue;
        }
        link_or_copy(&entry.path(), &to.join(name))?;
    }
    Ok(())
}

fn link_or_copy(from: &Path, to: &Path) -> Result<()> {
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    }
    Ok(())
}

/// The words in `folder`, of `schema`: an error where there are none,
/// they can't be read, or they're of another schema.
pub(crate) fn open_index(folder: &Path, schema: &Schema) -> Result<Index> {
    let index = Index::open_in_dir(folder)
        .with_context(|| format!("opening the words in {}", folder.display()))?;
    anyhow::ensure!(
        index.schema() == *schema,
        "the words in {} are of another schema",
        folder.display()
    );
    Ok(index)
}

/// Whether the words in `folder` are memory's and whole: of its schema,
/// every file as its checksum says.
pub(crate) fn check_copy(folder: &Path) -> Result<()> {
    let (schema, _) = schema();
    let index = open_index(folder, &schema)?;
    let damaged = index.validate_checksum()?;
    anyhow::ensure!(
        damaged.is_empty(),
        "the index's words aren't whole: {} files differ from their checksums",
        damaged.len()
    );
    Ok(())
}

/// Moves the words' `folder` aside and deletes it in the background.
pub(crate) fn set_aside(folder: &Path) -> Result<()> {
    if !folder.exists() {
        return Ok(());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let aside = folder.with_file_name(format!("aside-{nanos}-{name}"));
    std::fs::rename(folder, &aside)
        .with_context(|| format!("moving {} aside", folder.display()))?;
    std::thread::spawn(move || {
        let _ = std::fs::remove_dir_all(aside);
    });
    Ok(())
}

/// The generation of the words the index `db` holds: raised by every
/// batch that changes them.
pub(crate) fn generation(db: &Connection) -> Result<i64> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key = 'words'", [], |row| {
            row.get(0)
        })
        .optional()?
        .unwrap_or(0))
}

/// Whether Tantivy's `err` is the disk being full.
fn disk_full(err: &tantivy::TantivyError) -> bool {
    use tantivy::directory::error::OpenWriteError;
    let (tantivy::TantivyError::IoError(io)
    | tantivy::TantivyError::OpenWriteError(OpenWriteError::IoError { io_error: io, .. })) = err
    else {
        return false;
    };
    io.kind() == std::io::ErrorKind::StorageFull || io.raw_os_error() == Some(libc::ENOSPC)
}

impl Writing {
    /// Tantivy's writer, taken now if it isn't yet: an error while another
    /// process holds it.
    fn writer(&mut self, index: &Index) -> Result<&mut IndexWriter> {
        if self.writer.is_none() {
            let options = tantivy::indexer::IndexWriterOptions::builder()
                .num_worker_threads(self.threads)
                .memory_budget_per_thread(self.budget / self.threads)
                .num_merge_threads(1)
                .build();
            let writer = index
                .writer_with_options(options)
                .context("another process writes this index")?;
            self.writer = Some(writer);
        }
        Ok(self.writer.as_mut().expect("taken"))
    }
}

/// A query for `terms` in `field`: one term alone, or all of them
/// (`every`), or any.
fn query(field: Field, terms: &[String], every: bool) -> Box<dyn Query> {
    let term = |t: &String| {
        Box::new(TermQuery::new(
            Term::from_field_text(field, t),
            IndexRecordOption::WithFreqs,
        )) as Box<dyn Query>
    };
    match terms {
        [one] => term(one),
        _ => {
            let occur = match every {
                true => Occur::Must,
                false => Occur::Should,
            };
            Box::new(BooleanQuery::new(
                terms.iter().map(|t| (occur, term(t))).collect(),
            ))
        }
    }
}

/// What BM25 weighs a passage's words by: how many passages hold each,
/// beside how many passages there are (not titles), and the average
/// passage's length.
struct Stats<'a> {
    searcher: &'a Searcher,
    passages: u64,
}

impl Bm25StatisticsProvider for Stats<'_> {
    fn total_num_tokens(&self, field: Field) -> tantivy::Result<u64> {
        self.searcher.total_num_tokens(field)
    }

    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Ok(self.passages.max(1))
    }

    fn doc_freq(&self, term: &Term) -> tantivy::Result<u64> {
        self.searcher.doc_freq(term)
    }
}

/// What a search by words found: each document by its best passage, its
/// score, best first; whether it stopped at its deadline; whether more
/// documents matched than it keeps; and how many passages it scored.
#[derive(Debug, Default)]
pub(crate) struct Best {
    pub(crate) found: Vec<Found>,
    pub(crate) stopped: bool,
    pub(crate) beyond: bool,
    pub(crate) scored: u64,
}

/// A document found, by its best passage.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Found {
    pub(crate) document: i64,
    pub(crate) passage: i64,
    pub(crate) score: f64,
}

/// The best documents so far, each by its best passage: the threshold a
/// passage must pass to change them is the last kept's score. One more
/// than asked is kept, to know whether more match.
struct Top {
    most: usize,
    keep: usize,
    best: HashMap<i64, (Score, i64)>,
    /// The kept, by score (as its bits, which order as the scores do:
    /// BM25's are never negative), then document.
    order: BTreeSet<(u32, std::cmp::Reverse<i64>)>,
    beyond: bool,
}

impl Top {
    fn new(most: usize) -> Top {
        Top {
            most: most.max(1),
            keep: most.max(1) + 1,
            best: HashMap::new(),
            order: BTreeSet::new(),
            beyond: false,
        }
    }

    fn threshold(&self) -> Score {
        match self.order.len() >= self.keep {
            true => self
                .order
                .first()
                .map_or(Score::MIN, |(bits, _)| Score::from_bits(*bits)),
            false => Score::MIN,
        }
    }

    fn offer(&mut self, document: i64, passage: i64, score: Score) {
        let score = score.max(0.0);
        let key = |score: Score| (score.to_bits(), std::cmp::Reverse(document));
        match self.best.get(&document).copied() {
            Some((held, at)) => {
                if score > held || (score == held && passage < at) {
                    self.order.remove(&key(held));
                    self.order.insert(key(score));
                    self.best.insert(document, (score, passage));
                }
            }
            None => {
                self.best.insert(document, (score, passage));
                self.order.insert(key(score));
                if self.order.len() > self.keep
                    && let Some((_, std::cmp::Reverse(last))) = self.order.pop_first()
                {
                    self.best.remove(&last);
                    self.beyond = true;
                }
            }
        }
    }

    fn into_best(self, stopped: bool, scored: u64) -> Best {
        let mut found: Vec<Found> = self
            .best
            .into_iter()
            .map(|(document, (score, passage))| Found {
                document,
                passage,
                score: f64::from(score),
            })
            .collect();
        found.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.document.cmp(&b.document))
        });
        let beyond = self.beyond || found.len() > self.most;
        found.truncate(self.most);
        Best {
            found,
            stopped,
            beyond,
            scored,
        }
    }
}

/// A set of IDs, a bit each.
#[derive(Default)]
struct Bits(Vec<u64>);

impl Bits {
    fn set(&mut self, id: u64) {
        let word = (id / 64) as usize;
        if self.0.len() <= word {
            self.0.resize(word + 1, 0);
        }
        self.0[word] |= 1 << (id % 64);
    }

    fn has(&self, id: u64) -> bool {
        self.0
            .get((id / 64) as usize)
            .is_some_and(|w| w & (1 << (id % 64)) != 0)
    }

    /// The IDs here that aren't in `other`.
    fn minus<'a>(&'a self, other: &'a Bits) -> impl Iterator<Item = u64> + 'a {
        self.0.iter().enumerate().flat_map(move |(n, word)| {
            let left = word & !other.0.get(n).copied().unwrap_or(0);
            (0..64)
                .filter(move |bit| left & (1 << bit) != 0)
                .map(move |bit| n as u64 * 64 + bit)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_querys_words_are_the_indexs() {
        let dir = link_test_kit::TempDir::new("words-terms");
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL)")
            .unwrap();
        let words = Words::open(&dir.join("index.sqlite"), &db).unwrap();
        let got = words.terms("Planting tomatoes? NEAR(a* \"plant\" Café");
        assert_eq!(got, ["plant", "tomato", "near", "a", "cafe"]);
        assert!(words.terms(" *()\" ").is_empty());
    }

    #[test]
    fn the_best_keeps_each_document_once_by_its_best() {
        let mut top = Top::new(2);
        top.offer(1, 10, 1.0);
        top.offer(1, 11, 3.0);
        top.offer(2, 20, 2.0);
        // One more than asked is kept, to know more match.
        assert_eq!(top.threshold(), Score::MIN);
        top.offer(3, 30, 2.5);
        assert_eq!(top.threshold(), 2.0);
        top.offer(4, 40, 2.2);
        assert_eq!(top.threshold(), 2.2);
        let best = top.into_best(false, 5);
        let kept: Vec<(i64, i64)> = best.found.iter().map(|f| (f.document, f.passage)).collect();
        assert_eq!(kept, [(1, 11), (3, 30)]);
        assert!(best.beyond);
    }

    #[test]
    fn bits_say_what_one_set_holds_and_another_doesnt() {
        let (mut a, mut b) = (Bits::default(), Bits::default());
        for id in [1, 64, 200] {
            a.set(id);
        }
        b.set(64);
        assert!(a.has(200) && !a.has(2));
        assert_eq!(a.minus(&b).collect::<Vec<_>>(), [1, 200]);
    }
}
