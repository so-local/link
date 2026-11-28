//! Search by meaning, against a fake embedding model: a note found by a
//! question that shares none of its words; only what changed embedded
//! again, and everything when the model changes; how far embedding got;
//! and the program over stdio, with a fake embeddings server and a fake
//! `lnk model serve` that has no such model.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use link_memory_tool::Index;
use link_memory_tool::semantic::{Embedder, Source};
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// Words of one meaning share a dimension: a garden's, money's.
const MEANINGS: &[&[&str]] = &[
    &[
        "plant", "planted", "tomatoes", "tomato", "garden", "spring", "april", "bed", "sow",
    ],
    &["tax", "taxes", "return", "accountant", "receipts", "money"],
];

fn vector(text: &str) -> Vec<f32> {
    let mut v = vec![0.0; MEANINGS.len()];
    for word in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if let Some(i) = MEANINGS.iter().position(|m| m.contains(&word)) {
            v[i] += 1.0;
        }
    }
    v
}

struct Fake {
    model: String,
    texts: AtomicUsize,
}

impl Fake {
    fn new(model: &str) -> Arc<Self> {
        Arc::new(Self {
            model: model.into(),
            texts: AtomicUsize::new(0),
        })
    }
}

impl Embedder for Fake {
    fn model(&self) -> &str {
        &self.model
    }
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.texts.fetch_add(texts.len(), Ordering::SeqCst);
        Ok(texts.iter().map(|t| vector(t)).collect())
    }
}

fn folder(name: &str) -> TempDir {
    let dir = TempDir::new(&format!("semantic-{name}"));
    std::fs::create_dir_all(dir.join("memory/notes")).unwrap();
    std::fs::write(
        dir.join("memory/notes/beds.md"),
        "# Beds\n\nThe back bed:\ntomatoes, April.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("memory/notes/money.md"),
        "# Money\n\nThe tax return, last year.\n",
    )
    .unwrap();
    dir
}

const QUESTION: &str = "what did I plant last spring";

/// A file whose title is only its name: found by what the name means,
/// when nothing in it matches.
#[test]
fn a_title_counts_by_meaning() {
    let root = folder("title");
    std::fs::write(
        root.join("memory/notes/taxes.txt"),
        "Due on the fifteenth.\n",
    )
    .unwrap();
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    index.embed_with(Fake::new("fake")).unwrap();
    while index.embed_pending(8).unwrap() > 0 {}
    let hits = index.search("money owed", None, 10).unwrap();
    let taxes = hits
        .iter()
        .find(|h| h.path == "memory/notes/taxes.txt")
        .unwrap_or_else(|| panic!("{hits:?}"));
    assert_eq!(taxes.matched, "meaning");
}

#[test]
fn a_note_is_found_by_its_meaning() {
    let root = folder("meaning");
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    // By words only, the question finds only the note sharing "last".
    let hits = index.search(QUESTION, None, 10).unwrap();
    let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(paths, ["memory/notes/money.md"]);
    assert!(
        index
            .status()
            .unwrap()
            .semantic
            .unwrap()
            .contains("words only")
    );

    let fake = Fake::new("fake");
    index.embed_with(fake.clone()).unwrap();
    // Nothing embedded yet: by words, and it says how far it got.
    let status = index.status().unwrap();
    assert_eq!(
        status.embedded.as_ref().map(|e| (e.done, e.total)),
        Some((0, 2))
    );
    while index.embed_pending(1).unwrap() > 0 {}
    assert_eq!(index.status().unwrap().embedded, None);

    let hits = index.search(QUESTION, None, 10).unwrap();
    let beds = hits
        .iter()
        .find(|h| h.path == "memory/notes/beds.md")
        .unwrap_or_else(|| panic!("{hits:?}"));
    assert_eq!(beds.matched, "meaning");
    assert_eq!(beds.line, 1);
    assert!(beds.snippet.contains("tomatoes, April"), "{beds:?}");
    let money = hits
        .iter()
        .find(|h| h.path == "memory/notes/money.md")
        .unwrap();
    assert_eq!(money.matched, "words");
    assert_eq!(money.line, 3);
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn only_what_changed_is_embedded_again_and_everything_for_a_new_model() {
    let root = folder("again");
    let file = root.join(".index.sqlite");
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let fake = Fake::new("fake");
    index.embed_with(fake.clone()).unwrap();
    while index.embed_pending(16).unwrap() > 0 {}
    assert_eq!(fake.texts.load(Ordering::SeqCst), 2);

    // A new session over the same file: nothing to embed.
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    index.embed_with(fake.clone()).unwrap();
    assert_eq!(index.embed_pending(16).unwrap(), 0);

    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(
        root.join("memory/notes/beds.md"),
        "# Beds\n\nGarlic in the front bed.\n",
    )
    .unwrap();
    index.refresh().unwrap();
    assert_eq!(index.embed_pending(16).unwrap(), 1);

    let other = Fake::new("other");
    index.embed_with(other.clone()).unwrap();
    assert_eq!(index.status().unwrap().embedded.unwrap().done, 0);
    while index.embed_pending(16).unwrap() > 0 {}
    assert_eq!(other.texts.load(Ordering::SeqCst), 2);
}

/// A conversation that grows: only its new passage is embedded, and the
/// passages still to embed are taken from the queue's own index.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_grown_file_embeds_only_its_new_passages() {
    let root = folder("grown");
    let file = root.join(".index.sqlite");
    let talk = root.join("memory/notes/talk.md");
    std::fs::write(&talk, "# Talk\n\n## You\n\nThe back bed.\n").unwrap();
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let fake = Fake::new("fake");
    index.embed_with(fake.clone()).unwrap();
    while index.embed_pending(16).unwrap() > 0 {}
    let before = fake.texts.load(Ordering::SeqCst);

    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(
        &talk,
        "# Talk\n\n## You\n\nThe back bed.\n\n## Agent\n\nTomatoes in April.\n",
    )
    .unwrap();
    index.refresh().unwrap();
    while index.embed_pending(16).unwrap() > 0 {}
    assert_eq!(fake.texts.load(Ordering::SeqCst) - before, 1);
    let found = index.search(QUESTION, None, 10).unwrap();
    assert!(
        found.iter().any(|h| h.path == "memory/notes/talk.md"),
        "{found:?}"
    );

    let db = rusqlite::Connection::open(&file).unwrap();
    let plan: Vec<String> = db
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            link_memory_tool::queue::PICK
        ))
        .unwrap()
        .query_map([16], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        plan.iter().any(|step| step.contains("queue_order"))
            && !plan.iter().any(|step| step.contains("TEMP B-TREE")),
        "{plan:?}"
    );
}

/// A fake `/v1/embeddings`, on a port of its own.
fn embeddings_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let asked: Value = serde_json::from_slice(&body).unwrap();
            let data: Vec<Value> = asked["input"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, t)| json!({"index": i, "embedding": vector(t.as_str().unwrap())}))
                .collect();
            let reply = json!({"data": data}).to_string();
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    url
}

/// `lnk-memory-tool serve` with `args`, asked to search for `QUESTION`
/// until `done` says its reply will do; the last reply.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn search_until(args: &[&str], env: &[(&str, &Path)], done: impl Fn(&Value) -> bool) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lnk-memory-tool"))
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut reply = Value::Null;
    for id in 0..100 {
        let call = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "search", "arguments": {"query": QUESTION}}});
        writeln!(input, "{call}").unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        reply =
            serde_json::from_str::<Value>(&line).unwrap()["result"]["structuredContent"].clone();
        if done(&reply) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    drop(input);
    let _ = child.wait();
    reply
}

#[test]
fn the_server_embeds_in_the_background_and_searches_by_meaning() {
    let root = folder("server");
    let index = root.join(".index.sqlite");
    let url = embeddings_server();
    let (root_arg, index_arg) = (root.display().to_string(), index.display().to_string());
    let reply = search_until(
        &[
            "serve",
            &root_arg,
            "--index",
            &index_arg,
            "--embed",
            "fake",
            "--embed-url",
            &url,
        ],
        &[],
        |reply| {
            reply["results"]
                .as_array()
                .is_some_and(|r| r.iter().any(|h| h["matched"] == "meaning"))
        },
    );
    let beds = reply["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["path"] == "memory/notes/beds.md")
        .unwrap_or_else(|| panic!("{reply}"));
    assert_eq!(beds["matched"], "meaning", "{reply}");
    assert!(reply.get("semantic").is_none(), "{reply}");
}

#[test]
fn without_the_model_search_is_by_words_and_says_how_to_get_it() {
    let root = folder("missing");
    let index = root.join(".index.sqlite");
    // `lnk model serve` finding no runtime serving it.
    let lnk = root.join("lnk");
    link_test_kit::write_program(&lnk, "#!/bin/sh\nexit 2\n").unwrap();
    let (root_arg, index_arg) = (root.display().to_string(), index.display().to_string());
    let reply = search_until(
        &[
            "serve",
            &root_arg,
            "--index",
            &index_arg,
            "--embed",
            "qwen3-embedding",
        ],
        &[("LNK_EXE", &lnk)],
        |reply| {
            reply["semantic"]
                .as_str()
                .is_some_and(|s| s.contains("install"))
                && reply["indexing"].is_null()
        },
    );
    let said = reply["semantic"]
        .as_str()
        .unwrap_or_else(|| panic!("{reply}"));
    assert!(
        said.contains("'lnk model install qwen3-embedding'"),
        "{said}"
    );
    assert_eq!(
        reply["results"][0]["path"], "memory/notes/money.md",
        "{reply}"
    );

    // Served for it outside its sandbox, and couldn't be: the same, with
    // no `lnk` of its own run.
    let reply = search_until(
        &[
            "serve",
            &root_arg,
            "--index",
            &index_arg,
            "--embed",
            "qwen3-embedding",
        ],
        &[
            ("LNK_MEMORY_MODEL", Path::new("")),
            ("LNK_EXE", Path::new("/nonexistent/lnk")),
        ],
        |reply| {
            reply["semantic"]
                .as_str()
                .is_some_and(|s| s.contains("install"))
        },
    );
    let said = reply["semantic"]
        .as_str()
        .unwrap_or_else(|| panic!("{reply}"));
    assert!(
        said.contains("'lnk model install qwen3-embedding'"),
        "{said}"
    );

    // Without --embed at all, it says it's off too.
    let reply = search_until(&["serve", &root_arg], &[], |_| true);
    assert!(
        reply["semantic"].as_str().unwrap().contains("--embed"),
        "{reply}"
    );
}

/// A model served for it that couldn't be is said from the first search,
/// never as one starting: nothing is.
#[test]
fn a_model_that_couldnt_be_served_is_said_at_once() {
    let root = folder("missing-at-once");
    let mut index = Index::open(&root, Some(&root.join(".index.sqlite"))).unwrap();
    index
        .embed(Source::Missing("qwen3-embedding".into()))
        .unwrap();
    assert_eq!(
        index.status().unwrap().semantic.as_deref(),
        Some(
            "Search is by words only: no model runtime here serves qwen3-embedding: \
             'lnk model install qwen3-embedding' installs it."
        )
    );
}

/// The vectors kept between searches are read again when a file changes
/// and is embedded again, and when another server over the same file
/// does it: a search never ranks by a vector that's gone.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_search_after_a_change_ranks_by_the_new_vectors() {
    let root = folder("kept");
    let file = root.join(".index.sqlite");
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let fake = Fake::new("fake");
    index.embed_with(fake.clone()).unwrap();
    while index.embed_pending(16).unwrap() > 0 {}
    let by_meaning = |index: &Index| -> Vec<String> {
        index
            .search("garden", None, 10)
            .unwrap()
            .into_iter()
            .filter(|h| h.matched != "words")
            .map(|h| h.path)
            .collect()
    };
    assert_eq!(by_meaning(&index), ["memory/notes/beds.md"]);

    // The same chunk ids, their vectors changed: beds is about money now.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(
        root.join("memory/notes/beds.md"),
        "# Beds\n\nThe accountant.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("memory/notes/money.md"),
        "# Money\n\nSow in April.\n",
    )
    .unwrap();
    index.refresh().unwrap();
    while index.embed_pending(16).unwrap() > 0 {}
    assert_eq!(by_meaning(&index), ["memory/notes/money.md"]);

    // Another server over the same file changes it back.
    let mut other = Index::open(&root, Some(&file)).unwrap();
    other.embed_with(fake.clone()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(
        root.join("memory/notes/beds.md"),
        "# Beds\n\nSow in April.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("memory/notes/money.md"),
        "# Money\n\nThe accountant.\n",
    )
    .unwrap();
    other.refresh().unwrap();
    while other.embed_pending(16).unwrap() > 0 {}
    assert_eq!(by_meaning(&index), ["memory/notes/beds.md"]);

    // Gone there, gone here.
    std::fs::remove_file(root.join("memory/notes/beds.md")).unwrap();
    other.refresh().unwrap();
    assert!(by_meaning(&index).is_empty());
}

/// Rewrites beds' passage under the same id while its first vector is
/// made, as a refresh may when SQLite gives a deleted passage's id again:
/// other bytes, so another hash.
struct Swapping {
    file: std::path::PathBuf,
    swapped: AtomicBool,
    texts: Mutex<Vec<String>>,
}

impl Embedder for Swapping {
    fn model(&self) -> &str {
        "fake"
    }
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if !self.swapped.swap(true, Ordering::SeqCst) {
            let db = rusqlite::Connection::open(&self.file).unwrap();
            db.execute_batch(
                "CREATE TEMP TABLE beds AS SELECT p.id, p.document FROM passages p
                     JOIN documents d ON d.id = p.document WHERE d.path = 'memory/notes/beds.md';
                 DELETE FROM passages WHERE id IN (SELECT id FROM beds);
                 INSERT INTO passages (id, document, start, end, line, hash)
                     SELECT id, document, 0, 0, 1, 7 FROM beds;",
            )
            .unwrap();
        }
        self.texts.lock().unwrap().extend(texts.iter().cloned());
        Ok(texts.iter().map(|t| vector(t)).collect())
    }
}

/// A vector is kept only for the passage it was made from: one whose id
/// now holds other bytes is left to embed again, however many batches
/// run. That it's embedded once its file is written again is
/// `only_what_changed_is_embedded_again_and_everything_for_a_new_model`'s.
#[test]
fn a_vector_lands_only_on_the_passage_it_was_made_from() {
    let root = folder("swapped");
    let file = root.join(".index.sqlite");
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let swapping = Arc::new(Swapping {
        file: file.clone(),
        swapped: AtomicBool::new(false),
        texts: Mutex::new(Vec::new()),
    });
    index.embed_with(swapping.clone()).unwrap();
    let db = rusqlite::Connection::open(&file).unwrap();
    let beds = |db: &rusqlite::Connection| -> (i64, bool) {
        db.query_row(
            "SELECT p.hash, p.slot IS NOT NULL FROM passages p
             JOIN documents d ON d.id = p.document WHERE d.path = 'memory/notes/beds.md'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    };
    index.embed_pending(16).unwrap();
    assert_eq!(beds(&db), (7, false));
    // Its file doesn't hold those bytes: never embedded from another's.
    while index.embed_pending(16).unwrap() > 0 {}
    assert_eq!(beds(&db), (7, false));
}

/// The rows SQLite steps through to pick a batch of 16 from an index with
/// `pending` passages waiting.
fn picking_costs(name: &str, pending: usize) -> i32 {
    let root = TempDir::new(&format!("semantic-{name}"));
    let file = root.join(".index.sqlite");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    std::fs::write(root.join("memory/one.md"), "One.\n").unwrap();
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let db = rusqlite::Connection::open(&file).unwrap();
    db.execute(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
         INSERT INTO passages (document, start, end, line, hash)
         SELECT (SELECT id FROM documents), 0, 0, 1, i FROM n",
        [pending as i64 - 1],
    )
    .unwrap();
    let queued: i64 = db
        .query_row("SELECT count(*) FROM queue", [], |row| row.get(0))
        .unwrap();
    assert_eq!(queued as usize, pending);
    let mut pick = db.prepare(link_memory_tool::queue::PICK).unwrap();
    let picked = pick
        .query_map([16], |row| row.get::<_, i64>(0))
        .unwrap()
        .count();
    assert_eq!(picked, 16);
    pick.get_status(rusqlite::StatementStatus::VmStep)
}

/// Picking the next batch to embed reads the queue's index, so it costs
/// the same with a thousand passages waiting as with a hundred thousand.
#[test]
fn picking_a_batch_costs_the_same_however_many_wait() {
    let few = picking_costs("pick-few", 1_000);
    let many = picking_costs("pick-many", 100_000);
    assert!(few > 0);
    assert_eq!(few, many, "{few} steps for 1,000, {many} for 100,000");
}
