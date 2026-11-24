//! An [`Index`] opened on a folder or a tree, what it extracts and embeds
//! with, its readers, and when it's due to refresh.

use super::*;

impl Index {
    /// Folder `folder`'s index, kept in `file`, or in memory.
    pub fn open(folder: &Path, file: Option<&Path>) -> Result<Index> {
        Self::open_with(&link_agent_contract::folder::Folder::flat(folder), file)
    }

    /// The index of an agent's `folder`, its Home and its own folder,
    /// where its conversations and skills are, kept in `file`, or in
    /// memory.
    pub fn open_with(
        folder: &link_agent_contract::folder::Folder,
        file: Option<&Path>,
    ) -> Result<Index> {
        let home = folder
            .home
            .canonicalize()
            .with_context(|| format!("no folder {}", folder.home.display()))?;
        if !home.is_dir() {
            bail!("{} isn't a folder", home.display());
        }
        let whose = link_store_contract::store::Agent {
            id: "main".into(),
            owner: String::new(),
        };
        let folder = link_agent_contract::folder::Folder {
            home,
            // Not made yet, it holds nothing: found when it is.
            agent: folder
                .agent
                .canonicalize()
                .unwrap_or_else(|_| folder.agent.clone()),
        };
        Self::open_tree(Box::new(tree::Folder::new(folder, whose)), file)
    }

    /// The index of what `tree` holds, kept in `file`, or in a temporary
    /// file removed with it: calls read through connections of their own,
    /// which only a file gives.
    pub fn open_tree(tree: Box<dyn tree::Tree>, file: Option<&Path>) -> Result<Index> {
        let at = match file {
            Some(file) => file.to_path_buf(),
            None => temporary()?,
        };
        let mut db = open_file(&at)?;
        // Each session the tool bridge starts is its own server, over one file.
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        let (version, tables): (i64, i64) = db.query_row(
            "SELECT (SELECT user_version FROM pragma_user_version),
                    (SELECT count(*) FROM sqlite_master)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if version != SCHEMA && tables > 0 {
            // Another schema's: moved aside and deleted in the background,
            // never dropped table by table while a start waits; a new one
            // is built in its place.
            drop(db);
            set_aside(&at)?;
            crate::index::words::set_aside(&crate::index::words::folder_of(&at))?;
            db = open_file(&at)?;
            db.busy_timeout(std::time::Duration::from_secs(10))?;
        }
        create(&db)?;
        // Calls and the embedding thread read while the writer writes, and
        // the log is cut back once checkpointed.
        db.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        db.query_row(
            &format!(
                "PRAGMA journal_size_limit = {}",
                crate::limits::limits().wal_max_bytes
            ),
            [],
            |_| Ok(()),
        )?;
        let opinions = opinions::Opinions::default();
        cache(&db, opinions.memory / WRITER_SHARE)?;
        let words = Arc::new(crate::index::words::Words::open(&at, &db)?);
        words.spend(opinions.memory / WORDS_SHARE, opinions.threads);
        let common = Arc::new(Common {
            file: at,
            semantic: Shared::new(State::Off(
                "search by meaning isn't on ('--embed <model>')".into(),
            )),
            files: tree.files(),
            progress: std::sync::Mutex::new(reader::Progress::default()),
            memory: std::sync::atomic::AtomicU64::new(opinions.memory),
            file_most: std::sync::atomic::AtomicU64::new(opinions.file),
            deadline_ms: std::sync::atomic::AtomicU64::new(opinions.deadline.as_millis() as u64),
            dimensions: std::sync::atomic::AtomicU32::new(opinions.dimensions),
            small_read: std::sync::atomic::AtomicU64::new(0),
            scored: std::sync::atomic::AtomicU64::new(0),
            journal: Arc::new(crate::index::Journal::default()),
            words,
        });
        let reader = Reader::open(common.clone())?;
        Ok(Index {
            db,
            tree,
            file: file.map(Path::to_path_buf),
            common,
            reader,
            refreshed: None,
            queue: None,
            listed_total: 0,
            watch: None,
            opinions,
            converter: extract::Extractor::installed().available(),
            extractor: extract::Extractor::installed(),
            extracting: None,
            speed: None,
        })
    }

    /// Spends what `opinions` say: its tree's folders and largest file,
    /// its page cache, and the disk it leaves free.
    pub fn spend(&mut self, opinions: opinions::Opinions) -> Result<()> {
        self.tree.spend(&opinions);
        cache(&self.db, opinions.memory / WRITER_SHARE)?;
        self.common
            .words
            .spend(opinions.memory / WORDS_SHARE, opinions.threads);
        self.common
            .memory
            .store(opinions.memory, std::sync::atomic::Ordering::Relaxed);
        self.common
            .file_most
            .store(opinions.file, std::sync::atomic::Ordering::Relaxed);
        self.common.deadline_ms.store(
            opinions.deadline.as_millis() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.common
            .dimensions
            .store(opinions.dimensions, std::sync::atomic::Ordering::Relaxed);
        self.opinions = opinions;
        Ok(())
    }

    /// What it spends.
    pub fn opinions(&self) -> &opinions::Opinions {
        &self.opinions
    }

    /// What it shares with every call: a [`Reader`] of it is opened from
    /// this.
    pub fn common(&self) -> Arc<Common> {
        self.common.clone()
    }

    /// Its own reader, for the calls made of it directly.
    pub fn reader(&self) -> &Reader {
        &self.reader
    }

    /// [`Reader::search`], through its own reader.
    pub fn search(&self, query: &str, filter: impl Into<Filter>, limit: usize) -> Result<Vec<Hit>> {
        self.reader.search(query, filter, limit)
    }

    /// [`Reader::read`], through its own reader.
    pub fn read(
        &self,
        path: &str,
        from: usize,
        lines: Option<usize>,
        tokens: Option<usize>,
    ) -> Result<Option<Document>> {
        self.reader.read(path, from, lines, tokens)
    }

    /// [`Reader::links`], through its own reader.
    pub fn links(&self, path: &str) -> Result<Option<Links>> {
        self.reader.links(path)
    }

    /// [`Reader::list`], through its own reader.
    pub fn list(&self, filter: &Filter, limit: usize) -> Result<Listing> {
        self.reader.list(filter, limit)
    }

    /// [`Reader::list_after`], through its own reader.
    pub fn list_after(
        &self,
        filter: &Filter,
        after: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Listing> {
        self.reader.list_after(filter, after, limit)
    }

    /// Extracts documents' text with `extractor`, not Link Document Converter
    /// where it's installed: a test's.
    pub fn extract_with(&mut self, extractor: extract::Extractor) {
        self.converter = extractor.available();
        self.extractor = extractor;
    }

    /// Asks a walk when Link Document Converter has appeared since the
    /// writer's last turn, so what was found by its name alone is read.
    pub(crate) fn notice_converter(&mut self) {
        let now = self.extractor.available();
        // Appeared, or there while a document is still marked as waiting
        // for it (it went and came back while that document was read).
        let waiting = || {
            self.db
                .query_row(
                    "SELECT count(*) FROM unread WHERE why = ?1",
                    [extract::not_installed()],
                    |row| row.get::<_, i64>(0),
                )
                .is_ok_and(|n| n > 0)
        };
        if now && (!self.converter || (!self.pending() && waiting())) {
            self.common.journal.walk();
        }
        self.converter = now;
        // A document its parser didn't start for, tried again a few times,
        // at the writer's turns: not only after the next walk.
        let retried = || {
            self.db
                .query_row(
                    "SELECT count(*) FROM unread WHERE why = ?1 AND attempts BETWEEN 1 AND ?2",
                    rusqlite::params![extract::PENDING, crate::limits::limits().extract_tries],
                    |row| row.get::<_, i64>(0),
                )
                .is_ok_and(|n| n > 0)
        };
        if now
            && !self.extracting()
            && !self.pending()
            && retried()
            && let Err(err) = self.extract_pending()
        {
            crate::warn(link_format::sentence!("Documents aren't read."), &err);
        }
    }

    /// The agent whose folder it is, as a store driver keeping its
    /// conversations names it: its ID and owner, as Link Harness's are
    /// (`LNK_AGENT_ID`, `LNK_OWNER`; else its name and no owner).
    pub fn for_agent(&mut self, whose: link_store_contract::store::Agent) {
        self.tree.for_agent(whose);
    }

    /// The store driver the agent's spec keeps its conversations by
    /// (`LNK_MEMORY_DRIVER`), the only one it runs to read one; none,
    /// it runs none, and a conversation a driver keeps isn't indexed.
    pub fn driven_by(&mut self, driver: Option<String>) {
        self.tree.driven_by(driver);
    }

    /// Searches by meaning too, with the model `source` names, embedding
    /// in the background. Needs the index in a file, which the embedding
    /// thread opens too.
    pub fn embed(&mut self, source: semantic::Source) -> Result<()> {
        let Some(file) = self.file.clone() else {
            bail!("--embed needs --index: the embedding thread shares the index's file");
        };
        // One that couldn't be served is off from the first search, nothing
        // started; any other is found in the background, a model served
        // for it once it answers, and searches are by words until then.
        let state = match &source {
            semantic::Source::Missing(model) => {
                self.common
                    .semantic
                    .set(State::Off(semantic::no_such_model(model)));
                return Ok(());
            }
            _ => State::Starting,
        };
        self.common.semantic.set(state);
        semantic::start(
            file,
            self.common.files.clone(),
            source,
            self.common.semantic.clone(),
            self.common
                .dimensions
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        Ok(())
    }

    /// Searches by meaning with `embedder`, embedding only when told
    /// ([`Index::embed_pending`]): for tests.
    pub fn embed_with(&mut self, embedder: Arc<dyn semantic::Embedder>) -> Result<()> {
        self.kept(embedder.model()).take_over(&self.db)?;
        self.common.semantic.set(State::On(embedder));
        Ok(())
    }

    /// Embeds up to `limit` passages not embedded yet; how many.
    pub fn embed_pending(&self, limit: usize) -> Result<usize> {
        match self.common.semantic.state() {
            State::On(embedder) => crate::queue::embed_batch(
                &self.db,
                self.common.files.as_ref(),
                embedder.as_ref(),
                &self.kept(embedder.model()),
                Some(&self.common.semantic),
                limit,
            ),
            _ => Ok(0),
        }
    }

    /// The vectors it keeps of `model`, as many dimensions as it's told.
    fn kept(&self, model: &str) -> crate::vectors::Kept {
        crate::vectors::Kept::new(
            &self.common.file,
            model,
            self.common
                .dimensions
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// The bytes of small vectors its searches by meaning have read, to
    /// rescore their best: what a search by meaning reads beside the codes
    /// it maps.
    pub fn small_read(&self) -> u64 {
        self.common
            .small_read
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The passages its searches by words have scored, of all those
    /// holding their words: the rest were skipped unread.
    pub fn scored(&self) -> u64 {
        self.common
            .scored
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether search is by meaning too, and how far embedding and
    /// indexing have got ([`Reader::status`]).
    pub fn status(&self) -> Result<Status> {
        self.reader.status()
    }

    /// The folder it indexes.
    pub fn folder(&self) -> &Path {
        self.tree.shown()
    }

    /// Watches its folder, where its tree can, noting each change in its
    /// journal: what the writer of a served index does, so no call and no
    /// timer walks the folder for it.
    pub fn watch(&mut self) {
        self.watch = self.tree.watch(self.common.journal.clone());
    }

    /// Whether a change it was told of waits to be indexed.
    pub fn waiting(&self) -> bool {
        self.common.journal.waiting() > 0
    }

    /// Whether its folder is walked now: before it ever was, when a walk
    /// is asked, and, for a folder on this machine, each
    /// [`crate::limits::Limits::walk_every_secs`], or each
    /// [`crate::limits::Limits::walk_unwatched_secs`] where no watch
    /// reaches all of it. A tree listed only on a change is walked
    /// otherwise only when told.
    pub fn due(&self) -> bool {
        let Some(at) = self.refreshed else {
            return true;
        };
        if self.tree.on_change() {
            return false;
        }
        let limits = crate::limits::limits();
        let every = match self.watch.is_some() && self.common.journal.watched() {
            true => limits.walk_every_secs,
            false => limits.walk_unwatched_secs,
        };
        at.elapsed() >= std::time::Duration::from_secs(every)
    }

    /// What's changed in the index, as a mark: a mark that differs from
    /// an earlier one means it changed since (here, or by the embedding
    /// thread).
    pub fn stamp(&self) -> Result<(i64, u64)> {
        let others: i64 = self
            .db
            .query_row("PRAGMA data_version", [], |row| row.get(0))?;
        Ok((others, self.db.total_changes()))
    }

    /// Writes the index back as its copy ([`cache::write_back`]),
    /// through the files bridge `files`, if the copy is still `read`'s
    /// version; `scratch` is a folder it may use meanwhile. Past its
    /// opinions' `kept_off`, nothing is written: search is by words from
    /// then on, and its status says why.
    pub fn write_back(
        &self,
        files: &dyn link_store_contract::files::Store,
        read: Option<&link_store_contract::files::Version>,
        scratch: &Path,
    ) -> Result<cache::Written> {
        let written = cache::write_back(
            &self.db,
            &self.common.words,
            &self.common.file,
            files,
            read,
            scratch,
            self.opinions.kept_off,
        )?;
        if let cache::Written::TooLarge { size } = written {
            self.common.semantic.stop();
            self.common.semantic.set(State::Off(format!(
                "the index, {} MiB, is past the {} MiB kept off this machine \
                 ([tool.memory] kept_off): it isn't written back, and isn't embedded",
                size >> 20,
                self.opinions.kept_off >> 20
            )));
        }
        Ok(written)
    }
}

/// A temporary file for an index given none, private to this user, its
/// name never another's: removed with its index.
fn temporary() -> Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let dir = std::env::temp_dir();
    let file = dir.join(format!(
        "lnk-memory-{}-{n}-{nanos}.sqlite",
        std::process::id()
    ));
    // Made now, its owner's alone, so no one else's file is opened there.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode_private()
        .open(&file)
        .with_context(|| format!("making the index {}", file.display()))?;
    Ok(file)
}

/// Opening a file its owner's alone.
trait Private {
    fn mode_private(&mut self) -> &mut Self;
}

impl Private for std::fs::OpenOptions {
    #[cfg(unix)]
    fn mode_private(&mut self) -> &mut Self {
        std::os::unix::fs::OpenOptionsExt::mode(self, 0o600)
    }
    #[cfg(not(unix))]
    fn mode_private(&mut self) -> &mut Self {
        self
    }
}

/// Holds `db`'s page cache to `memory` bytes, its share of what the
/// service holds for the index.
pub(crate) fn cache(db: &Connection, memory: u64) -> Result<()> {
    // Negative: in KiB, not pages.
    let kib = -i64::try_from(memory >> 10).unwrap_or(i64::MAX);
    db.execute_batch(&format!("PRAGMA cache_size = {kib};"))?;
    Ok(())
}

/// Moves the index `file` aside, with its log, and deletes it in the
/// background: an old schema's index may be large, and its start waits
/// for none of it.
fn set_aside(file: &Path) -> Result<()> {
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut moved = Vec::new();
    for end in ["", "-wal", "-shm"] {
        let mut from = file.as_os_str().to_owned();
        from.push(end);
        // Named apart from the index's own: a move carries
        // only what's named after it.
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let to = file.with_file_name(format!("aside-{nanos}-{name}{end}"));
        match std::fs::rename(&from, &to) {
            Ok(()) => moved.push(to),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("moving {} aside", file.display()));
            }
        }
    }
    std::thread::spawn(move || {
        for file in moved {
            let _ = std::fs::remove_file(file);
        }
    });
    Ok(())
}
