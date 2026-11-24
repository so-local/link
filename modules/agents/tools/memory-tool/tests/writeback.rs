//! A bridged agent's index written back streamed, a segment at a time,
//! never read whole into memory; and past the most kept off the machine,
//! not written back at all, its search by words, saying why.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Mutex;

use link_memory_tool::Index;
use link_memory_tool::opinions::Opinions;
use link_store_contract::files::{self, Expect, Object, Store as _, Version};

/// The process's allocator, counting what each thread holds and the most
/// it has held: a test reads its own thread's, whatever runs beside it.
pub(crate) struct Counted;

thread_local! {
    static HELD: Cell<usize> = const { Cell::new(0) };
    static MOST: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: the system's allocator, with a count beside each call.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = HELD.try_with(|held| {
            held.set(held.get() + layout.size());
            let _ = MOST.try_with(|most| most.set(most.get().max(held.get())));
        });
        // SAFETY: as the caller asks of `alloc`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        let _ = HELD.try_with(|held| held.set(held.get().saturating_sub(layout.size())));
        // SAFETY: as the caller asks of `dealloc`.
        unsafe { System.dealloc(at, layout) }
    }
}

/// The most this thread held past what it held when `run` began.
fn most_held(run: impl FnOnce()) -> usize {
    let before = HELD.with(Cell::get);
    MOST.with(|most| most.set(before));
    run();
    MOST.with(Cell::get) - before
}

/// An object as [`Sizes`] keeps it: its size, and its bytes for the
/// manifest alone.
type Kept = (u64, Option<Vec<u8>>);

/// A bucket that keeps what it's sent only by its size, beside
/// the manifest: it holds none of a segment's bytes, so what a write back
/// holds is its own.
#[derive(Default)]
struct Sizes {
    objects: Mutex<BTreeMap<String, Kept>>,
    versions: Mutex<u64>,
}

impl files::Store for Sizes {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(name)
            .and_then(|(_, bytes)| bytes.clone())
            .map(|bytes| (bytes, "1".into())))
    }

    fn put(&self, name: &str, data: &[u8], _expect: &Expect) -> anyhow::Result<Version> {
        let kept = name.ends_with("manifest.json").then(|| data.to_vec());
        self.objects
            .lock()
            .unwrap()
            .insert(name.to_string(), (data.len() as u64, kept));
        let mut versions = self.versions.lock().unwrap();
        *versions += 1;
        Ok(versions.to_string())
    }

    fn delete(&self, name: &str, _expect: &Expect) -> anyhow::Result<()> {
        self.objects.lock().unwrap().remove(name);
        Ok(())
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, (size, _))| Object {
                path: name.clone(),
                size: *size,
                version: "1".into(),
                modified: 0,
            })
            .collect())
    }
}

/// `length` bytes no two segments of which are the same.
fn noise(length: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64 ^ length as u64;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// An index of `megabytes` MiB, its bytes random so no two segments are
/// the same.
fn large_index(name: &str, megabytes: i64) -> (link_test_kit::TempDir, Index) {
    let root = link_test_kit::TempDir::new(name);
    let file = root.join(".index.sqlite");
    let index = Index::open(&root, Some(&file)).unwrap();
    let db = rusqlite::Connection::open(&file).unwrap();
    db.execute(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
         INSERT INTO derived (document, body) SELECT i, randomblob(1048576) FROM n",
        [megabytes],
    )
    .unwrap();
    (root, index)
}

/// An index of 48 MiB, and 16 MiB of vectors beside it, is written back
/// holding less than 24 MiB of its own: one archive of its parts, a
/// segment at a time, every segment sent.
#[test]
fn an_index_larger_than_memory_allows_is_written_back_streamed() {
    let (root, index) = large_index("writeback-streamed", 48);
    for end in ["codes", "small"] {
        std::fs::write(root.join(format!(".index.sqlite.{end}")), noise(8 << 20)).unwrap();
    }
    let bucket = Sizes::default();
    let mut written = None;
    let most = most_held(|| {
        written = Some(
            index
                .write_back(&bucket, None, &root.join("scratch"))
                .unwrap(),
        );
    });
    assert!(written.unwrap().copy().is_some());
    assert!(most < 24 << 20, "held {} MiB", most >> 20);
    let sent: u64 = bucket
        .list("")
        .unwrap()
        .iter()
        .filter(|o| o.path.contains("seg-"))
        .map(|o| o.size)
        .sum();
    assert!(sent >= 64 << 20, "{sent} bytes sent");
}

/// Past the most kept off the machine, nothing is written back, search
/// is by words, and its status says why.
#[test]
fn an_index_past_its_ceiling_isnt_written_back() {
    let (root, mut index) = large_index("writeback-ceiling", 4);
    index
        .spend(Opinions {
            kept_off: 1 << 20,
            ..Opinions::default()
        })
        .unwrap();
    let bucket = Sizes::default();
    let written = index
        .write_back(&bucket, None, &root.join("scratch"))
        .unwrap();
    assert!(written.copy().is_none());
    assert!(bucket.list("").unwrap().is_empty());
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    assert!(
        status["semantic"].as_str().unwrap().contains("kept_off"),
        "{status}"
    );
}

/// Written back and fetched elsewhere, an index comes with its words and
/// its vectors: found by its words before any refresh, and nothing
/// embedded again.
#[test]
fn an_index_comes_back_with_its_words_and_vectors() {
    let root = link_test_kit::TempDir::new("writeback-parts");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    std::fs::write(
        root.join("memory/a.md"),
        "# Garden\n\nSow the beans in April.\n",
    )
    .unwrap();
    let mut index = Index::open(&root, Some(&root.join(".index.sqlite"))).unwrap();
    index.refresh().unwrap();
    index.embed_with(std::sync::Arc::new(Constant)).unwrap();
    while index.embed_pending(16).unwrap() > 0 {}
    let bucket = files::DirStore::new(&root.join("bucket")).unwrap();
    let written = index
        .write_back(&bucket, None, &root.join("scratch"))
        .unwrap();
    assert!(written.copy().is_some());
    drop(index);
    let there = root.join("there/index.sqlite");
    assert!(
        link_memory_tool::cache::fetch(&bucket, &there)
            .unwrap()
            .is_some()
    );
    let mut fetched = Index::open(&root, Some(&there)).unwrap();
    let hits = fetched.search("beans", None, 10).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    fetched.embed_with(std::sync::Arc::new(Constant)).unwrap();
    assert_eq!(fetched.embed_pending(16).unwrap(), 0);
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
