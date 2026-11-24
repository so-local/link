//! The change journal: calls never list the folder, and a file written
//! is noted by the folder's watch and found by the next search, with no
//! walk.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use link_memory_tool::index::{Journal, Watch};
use link_memory_tool::serve::Served;
use link_memory_tool::tree::{self, Files, Found, Read, Tree};
use link_memory_tool::{Index, mcp};
use serde_json::{Value, json};

/// A folder whose listings are counted.
struct Counted {
    folder: tree::Folder,
    listed: Arc<AtomicUsize>,
}

impl Tree for Counted {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> anyhow::Result<BTreeMap<String, Found>> {
        self.listed.fetch_add(1, Ordering::SeqCst);
        self.folder.list(known)
    }

    fn read(&mut self, path: &str, found: &Found) -> anyhow::Result<Option<Read>> {
        self.folder.read(path, found)
    }

    fn files(&self) -> Arc<dyn Files> {
        self.folder.files()
    }

    fn shown(&self) -> &std::path::Path {
        self.folder.shown()
    }

    fn found(&mut self, path: &str) -> Option<anyhow::Result<Option<Found>>> {
        self.folder.found(path)
    }

    fn watch(&self, journal: Arc<Journal>) -> Option<Watch> {
        self.folder.watch(journal)
    }
}

/// The structured answer to tool call `name` with `arguments`.
fn call(session: &mut BufReader<UnixStream>, id: u64, name: &str, arguments: Value) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                         "params": {"name": name, "arguments": arguments}});
    writeln!(session.get_mut(), "{message}").unwrap();
    let mut line = String::new();
    session.read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    reply["result"]["structuredContent"].clone()
}

/// A hundred calls list the folder never; a file written is found by
/// the next search within two seconds, noted by the watch, not walked.
#[test]
fn calls_never_list_and_a_file_written_is_found_soon() {
    let root = link_test_kit::TempDir::new("journal");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    std::fs::write(root.join("memory/boat.md"), "# Boat\n\nNew sails.\n").unwrap();
    let listed = Arc::new(AtomicUsize::new(0));
    let tree = Counted {
        folder: tree::Folder::new(
            link_agent_contract::folder::Folder::flat(&root),
            link_store_contract::store::Agent {
                id: "main".into(),
                owner: String::new(),
            },
        ),
        listed: listed.clone(),
    };
    let index = Index::open_tree(Box::new(tree), Some(&root.join("index.sqlite"))).unwrap();
    let served = Served::start(index, mcp::Options::default());
    let (ours, theirs) = UnixStream::pair().unwrap();
    ours.set_read_timeout(Some(link_test_kit::wait::scaled(Duration::from_secs(10))))
        .unwrap();
    std::thread::scope(|scope| {
        let session = scope.spawn(|| {
            let output = theirs.try_clone().unwrap();
            served.session(BufReader::new(theirs), output)
        });
        let mut session_end = BufReader::new(ours);
        // Its first walk, in the background.
        link_test_kit::wait::eventually("the first walk", Duration::from_secs(10), || {
            let found = call(&mut session_end, 1, "search", json!({"query": "sails"}));
            found["indexing"].is_null().then_some(())
        });
        let walks = listed.load(Ordering::SeqCst);
        assert_eq!(walks, 1);
        for n in 0..100 {
            let (name, arguments) = match n % 3 {
                0 => ("search", json!({"query": "sails"})),
                1 => ("read", json!({"path": "memory/boat.md"})),
                _ => ("list", json!({})),
            };
            let answer = call(&mut session_end, n + 2, name, arguments);
            assert!(answer.is_object(), "{name}: {answer}");
        }
        assert_eq!(listed.load(Ordering::SeqCst), walks, "a call listed");
        std::fs::write(root.join("memory/later.md"), "We fly by zeppelin.\n").unwrap();
        link_test_kit::wait::eventually("the file written found", Duration::from_secs(2), || {
            let found = call(
                &mut session_end,
                200,
                "search",
                json!({"query": "zeppelin"}),
            );
            (found["results"][0]["path"] == "memory/later.md").then_some(())
        });
        assert_eq!(listed.load(Ordering::SeqCst), walks, "found by a walk");
        session_end
            .get_ref()
            .shutdown(std::net::Shutdown::Write)
            .unwrap();
        session.join().unwrap().unwrap();
    });
}

/// A folder whose notes are slow to read, as a large build's are: its
/// first note read says the build's first batch has begun, and each later
/// one is held until a change is noted while it's read, or until it's let
/// go. A file written meanwhile is read at once.
struct Slow {
    folder: tree::Folder,
    journal: std::sync::Mutex<Option<Arc<Journal>>>,
    begun: Arc<std::sync::atomic::AtomicBool>,
    let_go: Arc<std::sync::atomic::AtomicBool>,
}

impl Tree for Slow {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> anyhow::Result<BTreeMap<String, Found>> {
        self.folder.list(known)
    }

    fn read(&mut self, path: &str, found: &Found) -> anyhow::Result<Option<Read>> {
        if path.contains("/note-") && self.begun.swap(true, Ordering::SeqCst) {
            let journal = self.journal.lock().unwrap().clone();
            let before = journal.as_ref().map_or(0, |j| j.waiting());
            // Held: past its deadline it's read anyway, as the test has
            // failed by then.
            let _ = link_test_kit::wait::within("a change noted", Duration::from_secs(10), || {
                let noted = journal.as_ref().is_some_and(|j| j.waiting() > before);
                (noted || self.let_go.load(Ordering::SeqCst)).then_some(())
            });
        }
        self.folder.read(path, found)
    }

    fn files(&self) -> Arc<dyn Files> {
        self.folder.files()
    }

    fn shown(&self) -> &std::path::Path {
        self.folder.shown()
    }

    fn found(&mut self, path: &str) -> Option<anyhow::Result<Option<Found>>> {
        self.folder.found(path)
    }

    fn watch(&self, journal: Arc<Journal>) -> Option<Watch> {
        *self.journal.lock().unwrap() = Some(journal.clone());
        self.folder.watch(journal)
    }
}

/// Lets the slow folder's reads go when dropped, whatever the test did.
struct LetGo(Arc<std::sync::atomic::AtomicBool>);

impl Drop for LetGo {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A file written while a build's batch is being written is found by a
/// search within two seconds, however long the rest of that batch would
/// take: the batch stops at the file it's reading, and the file noted is
/// written next, in a batch of its own.
#[test]
fn a_file_written_during_a_long_batch_is_found_soon() {
    let root = link_test_kit::TempDir::new("journal-batch");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    for i in 0..20 {
        std::fs::write(
            root.join(format!("memory/note-{i}.md")),
            format!("# Note {i}\n\nThe word is word{i}.\n"),
        )
        .unwrap();
    }
    let begun = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let let_go = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tree = Slow {
        folder: tree::Folder::new(
            link_agent_contract::folder::Folder::flat(&root),
            link_store_contract::store::Agent {
                id: "main".into(),
                owner: String::new(),
            },
        ),
        journal: std::sync::Mutex::new(None),
        begun: begun.clone(),
        let_go: let_go.clone(),
    };
    let index = Index::open_tree(Box::new(tree), Some(&root.join("index.sqlite"))).unwrap();
    let served = Served::start(index, mcp::Options::default());
    let _let_go = LetGo(let_go);
    let (ours, theirs) = UnixStream::pair().unwrap();
    ours.set_read_timeout(Some(link_test_kit::wait::scaled(Duration::from_secs(10))))
        .unwrap();
    std::thread::scope(|scope| {
        let session = scope.spawn(|| {
            let output = theirs.try_clone().unwrap();
            served.session(BufReader::new(theirs), output)
        });
        let mut session_end = BufReader::new(ours);
        link_test_kit::wait::eventually("the build's first batch", Duration::from_secs(10), || {
            begun.load(Ordering::SeqCst).then_some(())
        });
        std::fs::write(root.join("memory/later.md"), "We fly by zeppelin.\n").unwrap();
        link_test_kit::wait::eventually("the file written found", Duration::from_secs(2), || {
            let found = call(&mut session_end, 1, "search", json!({"query": "zeppelin"}));
            (found["results"][0]["path"] == "memory/later.md").then_some(())
        });
        let listed = call(&mut session_end, 2, "list", json!({}));
        assert!(
            listed["total"].as_u64().is_some_and(|total| total < 21),
            "found before the build ends: {listed}"
        );
        drop(_let_go);
        session_end
            .get_ref()
            .shutdown(std::net::Shutdown::Write)
            .unwrap();
        session.join().unwrap().unwrap();
    });
}
