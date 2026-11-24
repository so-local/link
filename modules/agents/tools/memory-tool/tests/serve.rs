//! An index served: a session's handshake and its calls never wait on
//! the index's writer, whatever it's in the middle of.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;

use link_memory_tool::serve::Served;
use link_memory_tool::tree::{self, Files, Found, Read, Tree};
use link_memory_tool::{Index, mcp};
use serde_json::{Value, json};

/// How long a test waits on any one step before it fails, naming it.
const STEP: Duration = Duration::from_secs(10);

/// A folder whose listings after the first are held: each says it's
/// begun, then waits to be let go, as a long batch holds the writer.
struct Held {
    folder: tree::Folder,
    listed: usize,
    begun: mpsc::Sender<()>,
    go: mpsc::Receiver<()>,
}

impl Tree for Held {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> anyhow::Result<BTreeMap<String, Found>> {
        self.listed += 1;
        if self.listed > 1 {
            let _ = self.begun.send(());
            let _ = self.go.recv_timeout(link_test_kit::wait::scaled(STEP * 3));
        }
        self.folder.list(known)
    }

    fn read(&mut self, path: &str, found: &Found) -> anyhow::Result<Option<Read>> {
        self.folder.read(path, found)
    }

    fn files(&self) -> std::sync::Arc<dyn Files> {
        self.folder.files()
    }

    fn shown(&self) -> &std::path::Path {
        self.folder.shown()
    }
}

/// The reply to `message`, sent on `session`, within the step's deadline.
fn ask(session: &mut BufReader<UnixStream>, message: Value) -> Value {
    writeln!(session.get_mut(), "{message}").unwrap();
    let mut line = String::new();
    session
        .read_line(&mut line)
        .unwrap_or_else(|err| panic!("no reply to {message}: {err}"));
    serde_json::from_str(&line).unwrap_or_else(|_| panic!("{line:?}"))
}

/// With the writer held in a long batch, a new session's `initialize`,
/// `tools/list` and a `read` are answered while it's still held: from
/// what was committed, never waiting for it.
#[test]
fn a_session_never_waits_on_the_writer() {
    let root = link_test_kit::TempDir::new("serve-held");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    std::fs::write(root.join("memory/boat.md"), "# Boat\n\nNew sails.\n").unwrap();
    let (begun, begins) = mpsc::channel();
    let (go, goes) = mpsc::channel();
    let folder = tree::Folder::new(
        link_agent_contract::folder::Folder::flat(&root),
        link_store_contract::store::Agent {
            id: "main".into(),
            owner: String::new(),
        },
    );
    let held = Held {
        folder,
        listed: 0,
        begun,
        go: goes,
    };
    let mut index = Index::open_tree(Box::new(held), Some(&root.join("index.sqlite"))).unwrap();
    // The first listing, at once: what a call then reads is committed.
    index.refresh().unwrap();
    let served = Served::start(index, mcp::Options::default());
    // A walk asked: the writer's next turn lists again, and is held.
    link_memory_tool::serve::Answers::common(&served).changed();
    begins
        .recv_timeout(link_test_kit::wait::scaled(STEP))
        .expect("the writer's next turn");

    let (ours, theirs) = UnixStream::pair().unwrap();
    ours.set_read_timeout(Some(link_test_kit::wait::scaled(STEP)))
        .unwrap();
    std::thread::scope(|scope| {
        let session = scope.spawn(|| {
            let output = theirs.try_clone().unwrap();
            served.session(BufReader::new(theirs), output)
        });
        let mut session_end = BufReader::new(ours);
        let init = ask(
            &mut session_end,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2025-06-18"}}),
        );
        assert_eq!(
            init["result"]["serverInfo"]["name"], "link-memory",
            "{init}"
        );
        let tools = ask(
            &mut session_end,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        );
        assert!(tools.to_string().contains("\"read\""), "{tools}");
        let read = ask(
            &mut session_end,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "read", "arguments": {"path": "memory/boat.md"}}}),
        );
        assert!(
            crate::document(&read)["text"]
                .as_str()
                .is_some_and(|text| text.contains("New sails")),
            "{read}"
        );
        // All answered while the writer is still held: let it go.
        assert!(begins.try_recv().is_err(), "the writer turned again");
        go.send(()).unwrap();
        session_end
            .get_ref()
            .shutdown(std::net::Shutdown::Write)
            .unwrap();
        session.join().unwrap().unwrap();
    });
}
