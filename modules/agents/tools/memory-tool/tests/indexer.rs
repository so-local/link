//! The background indexer: small transactions that stay committed when
//! it's killed, a pause while the disk is short of what's kept free, and
//! an index of another schema moved aside without waiting.

use std::collections::BTreeMap;

use link_memory_tool::opinions::Opinions;
use link_memory_tool::tree::{self, Files, Found, Read, Tree};
use link_memory_tool::{Filter, Index};

/// An agent's folder with `n` notes, each a word of its own.
fn notes(name: &str, n: usize) -> link_test_kit::TempDir {
    let root = link_test_kit::TempDir::new(name);
    std::fs::create_dir_all(root.join("memory")).unwrap();
    for i in 0..n {
        std::fs::write(
            root.join(format!("memory/note-{i}.md")),
            format!("# Note {i}\n\nThe word is word{i}.\n"),
        )
        .unwrap();
    }
    root
}

fn folder(root: &std::path::Path) -> tree::Folder {
    tree::Folder::new(
        link_agent_contract::folder::Folder::flat(root),
        link_store_contract::store::Agent {
            id: "main".into(),
            owner: String::new(),
        },
    )
}

/// A folder each of whose files is listed as a whole batch's worth, so
/// each is a batch of its own, and whose reads past `reads` die, as a
/// process killed mid-batch.
struct Dies {
    folder: tree::Folder,
    reads: usize,
}

impl Tree for Dies {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> anyhow::Result<BTreeMap<String, Found>> {
        let mut found = self.folder.list(known)?;
        for file in found.values_mut() {
            file.size = link_memory_tool::limits::limits().batch_bytes as i64;
        }
        Ok(found)
    }

    fn read(&mut self, path: &str, found: &Found) -> anyhow::Result<Option<Read>> {
        assert!(self.reads > 0, "killed mid-batch");
        self.reads -= 1;
        self.folder.read(path, found)
    }

    fn files(&self) -> std::sync::Arc<dyn Files> {
        self.folder.files()
    }

    fn shown(&self) -> &std::path::Path {
        self.folder.shown()
    }
}

/// A folder each of whose `notes` is listed as a whole batch's worth, so
/// a build writes one a batch, as a large corpus is written; a file
/// written later is listed as it is.
struct Large {
    folder: tree::Folder,
}

impl Large {
    fn padded(path: &str, mut file: Found) -> Found {
        if path.contains("/note-") {
            file.size = link_memory_tool::limits::limits().batch_bytes as i64;
        }
        file
    }
}

impl Tree for Large {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> anyhow::Result<BTreeMap<String, Found>> {
        let found = self.folder.list(known)?;
        Ok(found
            .into_iter()
            .map(|(path, file)| {
                let file = Self::padded(&path, file);
                (path, file)
            })
            .collect())
    }

    fn read(&mut self, path: &str, found: &Found) -> anyhow::Result<Option<Read>> {
        self.folder.read(path, found)
    }

    fn found(&mut self, path: &str) -> Option<anyhow::Result<Option<Found>>> {
        let found = self.folder.found(path)?;
        Some(found.map(|file| file.map(|file| Self::padded(path, file))))
    }

    fn files(&self) -> std::sync::Arc<dyn Files> {
        self.folder.files()
    }

    fn shown(&self) -> &std::path::Path {
        self.folder.shown()
    }
}

/// A folder whose first answer about a path it's asked of alone fails,
/// as a file system that answers again later.
struct Unreached {
    folder: tree::Folder,
    failed: bool,
}

impl Tree for Unreached {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> anyhow::Result<BTreeMap<String, Found>> {
        self.folder.list(known)
    }

    fn read(&mut self, path: &str, found: &Found) -> anyhow::Result<Option<Read>> {
        self.folder.read(path, found)
    }

    fn found(&mut self, path: &str) -> Option<anyhow::Result<Option<Found>>> {
        if !std::mem::replace(&mut self.failed, true) {
            return Some(Err(anyhow::anyhow!("not reached now")));
        }
        self.folder.found(path)
    }

    fn files(&self) -> std::sync::Arc<dyn Files> {
        self.folder.files()
    }

    fn shown(&self) -> &std::path::Path {
        self.folder.shown()
    }
}

/// How many documents the index at `file` holds, as another process
/// opening it reads them.
fn indexed(root: &std::path::Path, file: &std::path::Path) -> usize {
    Index::open(root, Some(file))
        .unwrap()
        .list(&Filter::default(), 1)
        .unwrap()
        .total
}

/// Killed mid-build, the indexer's batches committed before stay: the
/// next start finds them and indexes only the rest.
#[test]
fn a_build_killed_midway_resumes_from_what_was_committed() {
    let root = notes("indexer-killed", 4);
    let file = root.join("index.sqlite");
    let killed = std::panic::catch_unwind(|| {
        let tree = Dies {
            folder: folder(&root),
            reads: 2,
        };
        let mut index = Index::open_tree(Box::new(tree), Some(&file)).unwrap();
        // Two batches written, each in a transaction of its own.
        assert!(index.turn().unwrap());
        assert!(index.turn().unwrap());
        // The third dies while it's written.
        let _ = index.turn();
    });
    assert!(killed.is_err(), "the third batch dies");
    assert_eq!(indexed(&root, &file), 2, "what was committed stays");
    // The next start resumes: what's left, the newest first.
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    assert_eq!(index.list(&Filter::default(), 10).unwrap().total, 4);
    for i in 0..4 {
        assert_eq!(
            index.search(&format!("word{i}"), None, 10).unwrap().len(),
            1
        );
    }
}

/// A file noted while a build's batches are written goes ahead of
/// them: the next batch writes it, so a search finds it while the rest
/// is still being indexed.
#[test]
fn a_file_written_during_a_build_is_written_next() {
    let root = notes("indexer-noted", 4);
    let tree = Large {
        folder: folder(&root),
    };
    let mut index = Index::open_tree(Box::new(tree), Some(&root.join("index.sqlite"))).unwrap();
    // The walk, and its first batch: three notes still wait.
    assert!(index.turn().unwrap());
    std::fs::write(root.join("memory/later.md"), "We fly by zeppelin.\n").unwrap();
    index.common().noted("memory/later.md");
    assert!(index.turn().unwrap(), "the build's notes still wait");
    let found = index.search("zeppelin", None, 10).unwrap();
    assert_eq!(
        found.len(),
        1,
        "the file written is found by the next batch"
    );
    assert!(index.pending(), "found before the build ends");
    // The build goes on, and ends with every note.
    while index.turn().unwrap() {}
    assert_eq!(index.list(&Filter::default(), 10).unwrap().total, 5);
}

/// A path noted that can't be reached now is tried again at the next
/// turn, never dropped until a walk.
#[test]
fn a_noted_path_not_reached_is_tried_again() {
    let root = notes("indexer-unreached", 1);
    let tree = Unreached {
        folder: folder(&root),
        failed: false,
    };
    let mut index = Index::open_tree(Box::new(tree), Some(&root.join("index.sqlite"))).unwrap();
    while index.turn().unwrap() {}
    std::fs::write(root.join("memory/later.md"), "We fly by zeppelin.\n").unwrap();
    index.common().noted("memory/later.md");
    index.turn().unwrap();
    assert!(index.waiting(), "kept to be tried again");
    index.turn().unwrap();
    assert_eq!(index.search("zeppelin", None, 10).unwrap().len(), 1);
}

/// Short of the disk it keeps free, indexing pauses: calls go on
/// answering from what's indexed, its status says why, and it resumes
/// once there's room.
#[test]
fn indexing_pauses_while_the_disk_is_short_of_its_reserve() {
    let root = notes("indexer-reserve", 2);
    let mut index = Index::open(&root, Some(&root.join("index.sqlite"))).unwrap();
    // A reserve past any disk's room: as a disk filled to it.
    index
        .spend(Opinions {
            reserve: u64::MAX,
            ..Opinions::default()
        })
        .unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 0);
    assert!(index.pending());
    assert!(!index.busy(), "a paused index holds up no service's end");
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    let paused = status["indexing"]["paused"].as_str().unwrap_or_default();
    assert!(paused.contains("indexing is paused"), "{status}");
    // Calls answer meanwhile.
    assert!(index.search("word0", None, 10).unwrap().is_empty());
    // Room again: it resumes where it was.
    index
        .spend(Opinions {
            reserve: 0,
            ..Opinions::default()
        })
        .unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 2);
    assert_eq!(index.search("word1", None, 10).unwrap().len(), 1);
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    assert!(status.get("indexing").is_none(), "{status}");
}

/// An index of another schema is moved aside and a new one built: never
/// dropped table by table while the start waits, so a writer still
/// holding the old file's lock holds up nothing.
#[test]
fn an_older_schema_opens_without_waiting() {
    let root = notes("indexer-schema", 1);
    let file = root.join("index.sqlite");
    let old = rusqlite::Connection::open(&file).unwrap();
    old.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE documents (id INTEGER PRIMARY KEY, path TEXT);
         INSERT INTO documents (path) VALUES ('memory/old.md');
         PRAGMA user_version = 6;",
    )
    .unwrap();
    // Its writer, mid-transaction: dropping its tables would wait on it.
    old.execute_batch("BEGIN IMMEDIATE; INSERT INTO documents (path) VALUES ('x');")
        .unwrap();
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let paths: Vec<String> = index
        .list(&Filter::default(), 10)
        .unwrap()
        .documents
        .into_iter()
        .map(|d| d.path)
        .collect();
    assert_eq!(paths, ["memory/note-0.md"]);
    old.execute_batch("ROLLBACK").unwrap();
    drop(old);
    // The old one goes in the background.
    link_test_kit::wait::eventually(
        "the old index removed",
        std::time::Duration::from_secs(10),
        || {
            std::fs::read_dir(&*root)
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().contains("aside-"))
        },
    );
}
