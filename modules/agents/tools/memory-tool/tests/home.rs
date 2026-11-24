//! Memory over Home: only `memory/` and the conversations; PDFs and Word
//! documents by their text; Home kept in a bucket, reached through the
//! files bridge as its agent, with the index a cache of a copy beside it
//! (`service`, an agent bound); an index moved with Home used as it is.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
#[cfg(target_os = "linux")]
use link_memory_tool::extract::Format;
use link_memory_tool::extract::{self, Extractor};
use link_memory_tool::semantic::Embedder;
use link_memory_tool::{Index, cache, service, tree};
use link_memory_tool_contract::{Bound, Control, Hello};
use link_sandbox_binding::Identity;
use link_sandbox_binding::Keyring;
use link_secretive::Duplicate;
use link_store_contract::files::{
    self, Crypt, DirStore, Expect, Home, Homes, Object, Store, Version,
};
use link_test_kit::TempDir;
use serde_json::{Value, json};

mod bindings;
mod bridged;
mod copies;
mod documents;
mod one;
mod serving;
mod teams;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// Link Document Converter as the workspace built it, extracting documents' text
/// for an index a test opens: none is installed beside the test's own
/// program.
fn extractor() -> Extractor {
    Extractor::program(documents_program(), extract::deadline())
}

/// `lnk-document-converter`, as `cargo build --workspace --bins` built it.
fn documents_program() -> PathBuf {
    link_test_kit::programs().join("lnk-document-converter")
}

fn paths(index: &Index, query: &str) -> Vec<String> {
    index
        .search(query, None, 10)
        .unwrap()
        .into_iter()
        .map(|hit| hit.path)
        .collect()
}

/// A store that logs what's read of it, by the names it's given.
struct Logged {
    store: Arc<dyn Store>,
    read: Mutex<Vec<String>>,
}

impl Store for Logged {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.read.lock().unwrap().push(name.to_string());
        self.store.get(name)
    }
    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        self.store.put(name, data, expect)
    }
    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        self.store.delete(name, expect)
    }
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        self.store.list(prefix)
    }
}

/// One agent's Home, in a bucket, encrypted, its reads logged.
struct Bucket {
    store: Arc<Logged>,
}

impl Homes for Bucket {
    fn home(&self, _: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: self.store.clone(),
            cap: None,
            write: vec![".".into()],
        })
    }
}

const AGENT: &str = "agent0000001";

/// A files bridge at `dir/files.sock` over a bucket at `dir/bucket`,
/// taking agent [`AGENT`]'s calls; how it calls it, and its bucket's log.
fn files_bridge(dir: &Path) -> (files::Client, Arc<Logged>) {
    files_bridge_of(dir, "me@box")
}

/// [`files_bridge`], agent [`AGENT`] `owner`'s.
fn files_bridge_of(dir: &Path, owner: &str) -> (files::Client, Arc<Logged>) {
    std::fs::create_dir_all(dir).unwrap();
    let binding = Keyring::at(&dir.join("keys"))
        .bind(
            &Identity {
                agent: AGENT.into(),
                service: link_store_contract::testing::TEST_SERVICE.into(),
                owner: owner.into(),
                teams: Vec::new(),
            },
            &link_secretive::Secret::from(link_store_contract::testing::TEST_KEY.to_string()),
        )
        .unwrap();
    let store = Arc::new(Logged {
        store: Arc::new(
            Crypt::new(DirStore::new(&dir.join("bucket")).unwrap(), "Home's key").unwrap(),
        ),
        read: Mutex::new(Vec::new()),
    });
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    let homes = Arc::new(Bucket {
        store: store.clone(),
    });
    std::thread::spawn(move || files::serve(listener, keys, homes));
    (files::Client { socket, binding }, store)
}

/// Each owner's Home, a bucket of its own.
struct ByOwner(std::collections::HashMap<String, Arc<dyn Store>>);

impl Homes for ByOwner {
    fn home(&self, who: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: self.0.get(&who.owner).context("no Home")?.clone(),
            cap: None,
            write: vec![".".into()],
        })
    }
}

/// Each binding's key, by its agent and owner both: two owners' agents of
/// one ID, as a machine's keyring never holds at once, for memory to keep
/// apart.
struct ByIdentity(Vec<link_sandbox_binding::Binding>);

impl link_sandbox_binding::Keys for ByIdentity {
    fn key_of(&self, who: &Identity) -> Option<link_secretive::Secret> {
        self.0
            .iter()
            .find(|b| b.identity.agent == who.agent && b.identity.owner == who.owner)
            .map(|b| b.key.duplicate())
    }
}

/// One files bridge at `dir/files.sock`, as a machine has one, taking
/// agent [`AGENT`]'s calls for two owners, each its own Home: how each
/// calls it.
fn files_bridge_two_owners(dir: &Path) -> (files::Client, files::Client) {
    let binding = |owner: &str| link_sandbox_binding::Binding {
        identity: Identity {
            agent: AGENT.into(),
            service: link_store_contract::testing::TEST_SERVICE.into(),
            owner: owner.into(),
            teams: Vec::new(),
        },
        key: link_secretive::Secret::from(link_store_contract::testing::TEST_KEY.to_string()),
    };
    let (mine, theirs) = (binding("me@box"), binding("you@box"));
    let home = |name: &str| -> Arc<dyn Store> {
        Arc::new(Crypt::new(DirStore::new(&dir.join(name)).unwrap(), "Home's key").unwrap())
    };
    let homes = Arc::new(ByOwner(
        [
            ("me@box".to_string(), home("mine")),
            ("you@box".to_string(), home("theirs")),
        ]
        .into(),
    ));
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(ByIdentity(vec![mine.duplicate(), theirs.duplicate()]));
    std::thread::spawn(move || files::serve(listener, keys, homes));
    (
        files::Client {
            socket: socket.clone(),
            binding: mine,
        },
        files::Client {
            socket,
            binding: theirs,
        },
    )
}

/// A session of the service at `socket`: its first line, and the
/// answer to it.
fn first(socket: &Path, line: &Value) -> (Value, BufReader<UnixStream>) {
    let mut stream = UnixStream::connect(socket).unwrap();
    writeln!(stream, "{line}").unwrap();
    let mut reader = BufReader::new(stream);
    let mut answer = String::new();
    reader.read_line(&mut answer).unwrap();
    (serde_json::from_str(&answer).unwrap(), reader)
}

/// A search's answer in an open session, once it says nothing waits to
/// be indexed: memory reads its folder in the background.
fn search(session: &mut BufReader<UnixStream>, query: &str) -> Value {
    link_test_kit::wait::eventually("the folder indexed", Duration::from_secs(10), || {
        let answer = search_once(session, query);
        let content = &answer["result"]["structuredContent"];
        match content.is_object() && content["indexing"].is_null() {
            true => Ok(answer),
            false => Err(answer),
        }
    })
}

/// A search's answer in an open session, at once.
fn search_once(session: &mut BufReader<UnixStream>, query: &str) -> Value {
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search", "arguments": {"query": query}}});
    writeln!(session.get_mut(), "{call}").unwrap();
    let mut answer = String::new();
    session.read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap()
}

fn found(answer: &Value) -> Vec<String> {
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("{}");
    let results: Value =
        serde_json::from_str(&link_untrusted::unfenced(text)).unwrap_or(Value::Null);
    results["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|hit| hit["path"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// The service binding a machine's agents, at `dir/<name>.sock`, its
/// caches in `dir/<name>`, naming no agent of its own, the machine's files
/// and storage bridges at `dir/files.sock` and `dir/store.sock`.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn cache_service(dir: &Path, name: &str) -> PathBuf {
    let socket = dir.join(format!("{name}.sock"));
    let config = service::Config {
        cache: Some(dir.join(name)),
        write_every: Duration::from_secs(3600),
        // This machine's bridges: a bound agent's must be these.
        files_bridge: Some(dir.join("files.sock")),
        storage_bridge: Some(dir.join("store.sock")),
        ..service::Config::default()
    };
    let serving = socket.clone();
    std::thread::spawn(move || {
        service::serve_with(
            &serving,
            link_memory_tool_contract::declared::Agents(Default::default()),
            config,
        )
    });
    let started = Instant::now();
    while !socket.exists() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    socket
}

fn hello(client: &files::Client) -> Value {
    serde_json::to_value(Hello {
        agent: AGENT.into(),
        bound: Some(Bound {
            index: Default::default(),
            owner: client.binding.identity.owner.clone(),
            service: client.binding.identity.service.clone(),
            key: client.binding.key.duplicate(),
            files: Some(client.socket.clone()),
            storage: None,
            notes: None,
        }),
        team: None,
    })
    .unwrap()
}
