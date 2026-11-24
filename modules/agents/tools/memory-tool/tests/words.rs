//! Search by words over passages: those holding every word first, then
//! those holding the rarer words, a deadline it answers by with what it
//! has, and a status read from counters, never by counting rows; a
//! common word read in bounded time, and the words set right after a
//! crash between their commit and SQLite's.

use std::time::Duration;

use link_memory_tool::Index;
use link_memory_tool::opinions::Opinions;

fn write(root: &std::path::Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// A passage holding every word ranks before one holding only the rarer
/// words, however short; and a common word alone finds nothing more
/// when the query has rarer ones.
#[test]
fn every_word_first_then_the_rarer_words() {
    let root = link_test_kit::TempDir::new("words-order");
    write(
        &root,
        "memory/all.md",
        "The zebra has stripes, and the long tail of a horse.\n",
    );
    write(&root, "memory/rare.md", "Zebra.\n");
    for n in 0..20 {
        write(
            &root,
            &format!("memory/filler-{n}.md"),
            &format!("The day {n} was fine.\n"),
        );
    }
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let hits = index.search("the zebra stripes", None, 10).unwrap();
    let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    // `the` is in nearly every passage: left out once too few hold every
    // word, so no filler comes back.
    assert_eq!(paths, ["memory/all.md", "memory/rare.md"]);
    // Alone, a common word is all there is to rank by.
    assert_eq!(index.search("the", None, 50).unwrap().len(), 21);
}

/// A search past its deadline answers with the best of what it ranked
/// before it, and says it stopped.
#[test]
fn a_search_past_its_deadline_answers_with_what_it_has() {
    let root = link_test_kit::TempDir::new("words-deadline");
    for n in 0..400 {
        write(
            &root,
            &format!("memory/note-{n}.md"),
            &format!("# Note {n}\n\nThe garden, row {n}.\n"),
        );
    }
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let ranked = |index: &Index| index.reader().search_from("garden", None, 0, 50).unwrap();
    let whole = ranked(&index);
    assert!(!whole.stopped);
    assert_eq!(whole.hits.len(), 50);
    index
        .spend(Opinions {
            deadline: Duration::ZERO,
            ..Opinions::default()
        })
        .unwrap();
    let cut = ranked(&index);
    assert!(cut.stopped);
    // What it read before its first look at the clock, ranked.
    assert!(
        !cut.hits.is_empty() && cut.hits.len() < 400,
        "{}",
        cut.hits.len()
    );
}

/// An index of `low` notes that each hold a word once, among others, and
/// `high` that hold it often: the passages holding the word, and what one
/// search for it scored.
fn scored_for(name: &str, low: usize, high: usize) -> (usize, u64) {
    let root = link_test_kit::TempDir::new(name);
    for n in 0..low {
        write(
            &root,
            &format!("memory/low-{n}.md"),
            "The garden by the long wall of the old house, past the gate.\n",
        );
    }
    for n in 0..high {
        write(
            &root,
            &format!("memory/high-{n}.md"),
            "Garden, garden, garden: the garden.\n",
        );
    }
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let before = index.scored();
    let hits = index.search("garden", None, 10).unwrap();
    assert!(hits.iter().all(|h| h.path.contains("high-")), "{hits:?}");
    (low + high, index.scored() - before)
}

/// A common word's search scores no more passages however many hold it:
/// Tantivy skips the blocks of its postings that can't reach the best so
/// far, so ten times the passages holding it scores about as many.
#[test]
fn a_common_words_search_reads_a_bounded_number_of_postings() {
    let (few, scored_few) = scored_for("words-bounded-few", 300, 60);
    let (many, scored_many) = scored_for("words-bounded-many", 3000, 60);
    assert!(many > few * 8);
    // Each block Tantivy reads is at most 128 passages; the many differ
    // from the few by the blocks the best fall in, never by the rest.
    assert!(
        scored_many <= scored_few + 2 * 128,
        "{scored_many} scored of {many}, {scored_few} of {few}"
    );
    assert!(
        (scored_many as usize) < many / 4,
        "{scored_many} scored of {many}"
    );
}

/// The words' passages, as Tantivy keeps them in the index `file`'s
/// folder, beside SQLite's: they agree when they're the same count and a
/// search finds what each holds.
fn passages_in_both(file: &std::path::Path) -> (u64, u64) {
    use tantivy::collector::Count;
    use tantivy::query::TermQuery;
    let mut folder = file.as_os_str().to_owned();
    folder.push(".words");
    let words = tantivy::Index::open_in_dir(std::path::PathBuf::from(folder)).unwrap();
    let tag = words.schema().get_field("tag").unwrap();
    let searcher = words.reader().unwrap().searcher();
    let query = TermQuery::new(
        tantivy::Term::from_field_text(tag, "passage"),
        tantivy::schema::IndexRecordOption::Basic,
    );
    let in_words = searcher.search(&query, &Count).unwrap() as u64;
    let db = rusqlite::Connection::open(file).unwrap();
    let in_sqlite: i64 = db
        .query_row("SELECT count(*) FROM passages", [], |row| row.get(0))
        .unwrap();
    (in_words, in_sqlite as u64)
}

/// A crash after SQLite commits a batch and before Tantivy does leaves the
/// words a batch behind: the next start finds it by the generation each
/// names, and sets them right, so a file the batch wrote is found, one it
/// removed isn't, and the two hold the same passages again.
#[test]
fn a_crash_between_the_two_commits_leaves_them_agreeing_at_the_next_start() {
    let root = link_test_kit::TempDir::new("words-crash");
    let file = root.join(".index.sqlite");
    let words = root.join(".index.sqlite.words");
    let saved = root.join("saved-words");
    write(&root, "memory/kept.md", "The kept note names a heron.\n");
    write(&root, "memory/gone.md", "The gone note names a kestrel.\n");
    Index::open(&root, Some(&file)).unwrap().refresh().unwrap();
    // The words as they were before the next batch.
    std::fs::create_dir_all(&saved).unwrap();
    for entry in std::fs::read_dir(&words).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), saved.join(entry.file_name())).unwrap();
    }
    std::fs::remove_file(root.join("memory/gone.md")).unwrap();
    write(&root, "memory/new.md", "The new note names an osprey.\n");
    Index::open(&root, Some(&file)).unwrap().refresh().unwrap();
    // As a crash between the commits leaves them: SQLite's batch
    // committed, Tantivy's not.
    std::fs::remove_dir_all(&words).unwrap();
    std::fs::rename(&saved, &words).unwrap();
    let found = |index: &Index, word: &str| -> Vec<String> {
        index
            .search(word, None, 10)
            .unwrap()
            .into_iter()
            .map(|h| h.path)
            .collect()
    };
    let mut index = Index::open(&root, Some(&file)).unwrap();
    // Opened, before its writer's first turn: the new note's words
    // aren't there.
    assert!(found(&index, "osprey").is_empty());
    index.refresh().unwrap();
    assert_eq!(found(&index, "osprey"), ["memory/new.md"]);
    assert_eq!(found(&index, "heron"), ["memory/kept.md"]);
    assert!(found(&index, "kestrel").is_empty());
    drop(index);
    let (in_words, in_sqlite) = passages_in_both(&file);
    assert_eq!(in_words, in_sqlite);
}

/// How far embedding has got is counted as each passage is written: rows
/// written behind the counters' back change nothing a status says.
#[test]
fn the_status_is_read_without_counting_rows() {
    let root = link_test_kit::TempDir::new("words-status");
    let file = root.join(".index.sqlite");
    for n in 0..3 {
        write(
            &root,
            &format!("memory/note-{n}.md"),
            &format!("Note {n}.\n"),
        );
    }
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    index.embed_with(std::sync::Arc::new(Constant)).unwrap();
    index.embed_pending(1).unwrap();
    let embedded =
        |index: &Index| serde_json::to_value(index.status().unwrap()).unwrap()["embedded"].clone();
    assert_eq!(embedded(&index), serde_json::json!({"done": 1, "total": 3}));
    // A thousand passages added with no trigger to count them.
    let db = rusqlite::Connection::open(&file).unwrap();
    db.execute_batch(
        "DROP TRIGGER passages_added;
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
         INSERT INTO passages (document, start, end, line, hash) SELECT 1, 0, 0, 1, i FROM n;",
    )
    .unwrap();
    assert_eq!(embedded(&index), serde_json::json!({"done": 1, "total": 3}));
}

/// An embedding model that gives every text the same vector.
struct Constant;

impl link_memory_tool::semantic::Embedder for Constant {
    fn model(&self) -> &str {
        "constant"
    }

    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
    }
}
