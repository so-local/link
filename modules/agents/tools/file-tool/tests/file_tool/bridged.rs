//! Link file on Home through the files bridge (`lnk-file-tool bridge`),
//! where Home is kept in a bucket, and on the agent's instructions and
//! skills through the storage bridge: the same calls, each proved as the
//! agent, its writes naming the version they read.

use crate::{answered, finds, numbered, said};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use link_file_tool::{Bridges, Offer, Seen, answer_with};
use link_sandbox_binding::Binding;
use link_sandbox_binding::Identity;
use link_sandbox_binding::Keyring;
use link_store_contract::files::{
    self, Crypt, DirStore, Expect, Home, Homes, Object, Store, TeamFolder, TeamRef, Version,
};
use link_test_kit::TempDir;
use serde_json::{Value, json};

/// A store that, once, changes a file just before the bridge writes it:
/// another unit of the agent writing it meanwhile.
struct Racing {
    store: DirStore,
    raced: AtomicBool,
    cipher_name: Mutex<Option<String>>,
    /// The objects read, each time one is.
    gets: AtomicUsize,
}

impl Store for Racing {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.store.get(name)
    }
    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        let racing = self.cipher_name.lock().unwrap().clone();
        if racing.as_deref() == Some(name) && !self.raced.swap(true, Ordering::SeqCst) {
            let meanwhile = files::crypt::Cipher::new(KEY)?.encrypt(b"meanwhile")?;
            self.store.put(name, &meanwhile, &Expect::Any)?;
        }
        self.store.put(name, data, expect)
    }
    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        self.store.delete(name, expect)
    }
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        self.store.list(prefix)
    }
}

struct Bucket {
    store: Arc<dyn Store>,
}

impl Homes for Bucket {
    fn home(&self, _who: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: self.store.clone(),
            cap: None,
            write: vec!["work".into(), "memory".into()],
        })
    }
}

const KEY: &str = "the agent's key for Home";

/// A files bridge in `dir` over a bucket there, its objects encrypted;
/// the agent's binding, and the store raced on `race` (a Home path).
fn bridge(dir: &Path, race: &str) -> (Session, Arc<Racing>) {
    let binding = link_store_contract::testing::bound(&dir.join("keys"), "agent0000001");
    let racing = Arc::new(Racing {
        store: DirStore::new(&dir.join("bucket")).unwrap(),
        // Armed by a test, once.
        raced: AtomicBool::new(true),
        cipher_name: Mutex::new(None),
        gets: AtomicUsize::new(0),
    });
    let crypt = Crypt::new(racing.clone(), KEY).unwrap();
    *racing.cipher_name.lock().unwrap() = Some(encrypted_name(&format!("home/{race}")));
    let socket = dir.join("files.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    let homes = Arc::new(Bucket {
        store: Arc::new(crypt),
    });
    std::thread::spawn(move || files::serve(listener, keys, homes));
    (Session::of(files::Client { socket, binding }), racing)
}

/// A session of Link File's on the bridge: its client, its storage
/// bridge where it has one, and the versions it saw.
struct Session {
    client: files::Client,
    store: Option<link_store_contract::store::Bridged>,
    seen: Seen,
}

impl Session {
    fn of(client: files::Client) -> Self {
        Self {
            client,
            store: None,
            seen: Seen::default(),
        }
    }
}

impl std::ops::Deref for Session {
    type Target = files::Client;
    fn deref(&self) -> &files::Client {
        &self.client
    }
}

fn encrypted_name(name: &str) -> String {
    files::crypt::Cipher::new(KEY).unwrap().encrypt_name(name)
}

fn call(session: &Session, name: &str, arguments: Value) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}});
    let write = ["work".to_string(), "skills".to_string()];
    let bridges = Bridges {
        files: &session.client,
        store: session.store.as_ref(),
    };
    let run = |name: &str, arguments: &Value| {
        link_file_tool::bridged(&bridges, &write, &session.seen, name, arguments)
    };
    answer_with(&run, &Offer::default(), &message.to_string()).unwrap()["result"].clone()
}

fn refused(result: &Value) -> String {
    assert_eq!(result["isError"], true, "{result}");
    said(result).to_string()
}

#[test]
fn it_reads_writes_replaces_and_finds_through_the_bridge() {
    let dir = TempDir::short("file-bridged");
    let (bridge, _) = bridge(&dir, "work/race.md");
    let wrote = call(
        &bridge,
        "write",
        json!({"file_path": "work/plan.md", "content": "one\ntwo\n"}),
    );
    assert_eq!(wrote["structuredContent"]["created"], true, "{wrote}");
    assert_eq!(
        answered(&wrote)["diff"],
        "--- /dev/null\n+++ b/work/plan.md\n@@ -0,0 +1,2 @@\n+one\n+two\n"
    );
    let read = call(&bridge, "read", json!({"file_path": "work/plan.md"}));
    assert_eq!(said(&read), numbered("one\ntwo\n"));
    let from = call(
        &bridge,
        "read",
        json!({"file_path": "work/plan.md", "offset": 2}),
    );
    assert!(said(&from).ends_with("two\n"));
    let replaced = call(
        &bridge,
        "edit",
        json!({"file_path": "work/plan.md", "old_string": "two", "new_string": "three"}),
    );
    assert_eq!(replaced["structuredContent"]["replaced"], 1, "{replaced}");
    assert_eq!(
        answered(&replaced)["diff"],
        "--- a/work/plan.md\n+++ b/work/plan.md\n@@ -1,2 +1,2 @@\n one\n-two\n+three\n"
    );
    let again = call(
        &bridge,
        "write",
        json!({"file_path": "work/plan.md", "content": "x\n"}),
    );
    assert_eq!(again["structuredContent"]["created"], false);
    assert_eq!(
        answered(&again)["diff"],
        "--- a/work/plan.md\n+++ b/work/plan.md\n@@ -1,2 +1 @@\n-one\n-three\n+x\n"
    );
    // The owner's documents, read, never written.
    bridge
        .write("memory/taxes.md", b"refund due in May\n", Expect::Absent)
        .unwrap();
    let found = call(&bridge, "grep", json!({"pattern": "refund"}));
    assert_eq!(finds(&found)[0]["path"], "memory/taxes.md");
    // `.` is Home, as the local tool has it.
    let here = call(&bridge, "grep", json!({"pattern": "refund", "path": "."}));
    assert_eq!(here["structuredContent"], found["structuredContent"]);
    let named = call(
        &bridge,
        "glob",
        json!({"pattern": "PLAN.*", "path": "work"}),
    );
    assert_eq!(finds(&named)[0]["path"], "work/plan.md");
    let why = refused(&call(
        &bridge,
        "write",
        json!({"file_path": "memory/x.md", "content": "x"}),
    ));
    assert!(why.contains("only in work/"), "{why}");
    let why = refused(&call(
        &bridge,
        "write",
        json!({"file_path": "chats/x", "content": "x"}),
    ));
    assert!(why.contains("may not write chats/"), "{why}");
    // A team it isn't on, the bridge refuses; nothing outside Home.
    let why = refused(&call(
        &bridge,
        "read",
        json!({"file_path": "teams/ops/x.md"}),
    ));
    assert!(why.contains("aren't on a team ops"), "{why}");
    refused(&call(
        &bridge,
        "read",
        json!({"file_path": "../agent0000002/work/plan.md"}),
    ));
    refused(&call(
        &bridge,
        "read",
        json!({"file_path": "work/missing.md"}),
    ));
}

/// `glob` and `grep` through the bridge are the same as on disk: a glob
/// of a file's name, or of its path below `path` when it holds `/`, any
/// case; `grep`'s `pattern` a regular expression, its `glob` narrowing
/// the files it reads.
#[test]
fn it_finds_by_a_glob_and_a_regular_expression_through_the_bridge() {
    let dir = TempDir::short("file-bridged-globs");
    let (bridge, _) = bridge(&dir, "work/race.md");
    for (path, text) in [
        ("work/src/main.rs", "fn main() {\n    run(1);\n}\n"),
        ("work/src/lib/Test_Lib.PY", "def test_lib(): pass\n"),
        ("work/src/test_top.py", "def test_top(): pass\n"),
        ("work/tests/test_other.py", "x = 1\n"),
    ] {
        bridge.write(path, text.as_bytes(), Expect::Absent).unwrap();
    }
    let paths = |arguments: Value| -> Vec<String> {
        let found = call(&bridge, "glob", arguments);
        finds(&found)
            .as_array()
            .unwrap_or_else(|| panic!("{found}"))
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(paths(json!({"pattern": "*.RS"})), ["work/src/main.rs"]);
    assert_eq!(
        paths(json!({"pattern": "src/**/test_*.py", "path": "work"})),
        ["work/src/lib/Test_Lib.PY", "work/src/test_top.py"]
    );
    assert_eq!(
        paths(json!({"pattern": "src/*.py", "path": "work"})),
        ["work/src/test_top.py"]
    );
    let lines = call(
        &bridge,
        "grep",
        json!({"pattern": r"^\s+run\(\d\);$", "output_mode": "content"}),
    );
    assert_eq!(
        finds(&lines),
        json!([{"path": "work/src/main.rs", "line": 2}])
    );
    assert_eq!(said(&lines), "work/src/main.rs:2:    run(1);");
    let narrowed = call(
        &bridge,
        "grep",
        json!({"pattern": "def test_", "glob": "test_top.*", "path": "work", "output_mode": "count"}),
    );
    assert_eq!(
        finds(&narrowed),
        json!([{"path": "work/src/test_top.py", "count": 1}])
    );
    assert_eq!(said(&narrowed), "work/src/test_top.py:1");
    let why = refused(&call(&bridge, "grep", json!({"pattern": "run("})));
    assert!(why.contains("isn't a regular expression"), "{why}");
}

/// A change past the most a diff answers with answers `diff_cut` and the
/// new text's SHA-256 in its place.
#[test]
fn a_large_change_answers_its_sha256_through_the_bridge() {
    let dir = TempDir::short("file-bridged-large");
    let (bridge, _) = bridge(&dir, "work/race.md");
    let long: String = (1..=4000).map(|n| format!("line {n}\n")).collect();
    let wrote = call(
        &bridge,
        "write",
        json!({"file_path": "work/long.md", "content": long}),
    );
    assert_eq!(wrote["structuredContent"]["diff_cut"], true, "{wrote}");
    assert_eq!(
        wrote["structuredContent"]["sha256"],
        link_digest::sha256(long.as_bytes())
    );
    let replaced = call(
        &bridge,
        "edit",
        json!({"file_path": "work/long.md", "old_string": "line", "new_string": "row", "replace_all": true}),
    );
    assert_eq!(
        replaced["structuredContent"]["diff_cut"], true,
        "{replaced}"
    );
    assert_eq!(
        replaced["structuredContent"]["sha256"],
        link_digest::sha256(long.replace("line", "row").as_bytes())
    );
}

/// A file read on from a later line is paged from the text read from its
/// first, never read whole again for each page; read from the first line
/// again, or written, it's read anew.
#[test]
fn a_files_pages_are_read_once() {
    let dir = TempDir::short("file-bridged-pages");
    let (bridge, racing) = bridge(&dir, "work/race.md");
    // Lines of 1 KB: about 24 to a page.
    let text: String = (1..=100).map(|n| format!("{n:<1023}\n")).collect();
    call(
        &bridge,
        "write",
        json!({"file_path": "work/long.md", "content": text}),
    );
    let first = call(&bridge, "read", json!({"file_path": "work/long.md"}));
    let read = racing.gets.load(Ordering::SeqCst);
    let mut from = first["structuredContent"]["to"].as_u64().unwrap() + 1;
    let mut pages = 1;
    while from <= 100 {
        let page = call(
            &bridge,
            "read",
            json!({"file_path": "work/long.md", "offset": from}),
        );
        assert!(
            said(&page).contains(&format!("{from:>6}→{from} ")),
            "{page}"
        );
        from = page["structuredContent"]["to"].as_u64().unwrap() + 1;
        pages += 1;
    }
    assert!(pages > 3, "{pages} pages");
    assert_eq!(
        racing.gets.load(Ordering::SeqCst),
        read,
        "a page read the file again"
    );

    // Changed by another unit meanwhile: its pages are still the version
    // read; from the first line, the new one.
    let (_, version) = bridge.read("work/long.md").unwrap().unwrap();
    bridge
        .write("work/long.md", b"changed\n", Expect::Version(version))
        .unwrap();
    let page = call(
        &bridge,
        "read",
        json!({"file_path": "work/long.md", "offset": 50}),
    );
    assert_eq!(page["structuredContent"]["lines"], 100, "{page}");
    let read = call(&bridge, "read", json!({"file_path": "work/long.md"}));
    assert_eq!(said(&read), numbered("changed\n"));

    // Written by the session, a later line is read from what's there.
    call(
        &bridge,
        "write",
        json!({"file_path": "work/long.md", "content": "a\nb\n"}),
    );
    let page = call(
        &bridge,
        "read",
        json!({"file_path": "work/long.md", "offset": 2}),
    );
    assert_eq!(page["structuredContent"]["lines"], 2, "{page}");
}

#[test]
fn a_file_changed_since_it_was_read_is_refused() {
    let dir = TempDir::short("file-bridged-race");
    let (bridge, racing) = bridge(&dir, "work/race.md");
    call(
        &bridge,
        "write",
        json!({"file_path": "work/race.md", "content": "hello\n"}),
    );
    racing.raced.store(false, Ordering::SeqCst);
    let why = refused(&call(
        &bridge,
        "edit",
        json!({"file_path": "work/race.md", "old_string": "hello", "new_string": "bye"}),
    ));
    assert!(why.contains("changed since it was read"), "{why}");
    // What the other wrote is kept; read again, the call goes through.
    let raw = racing
        .store
        .get(&encrypted_name("home/work/race.md"))
        .unwrap();
    let cipher = files::crypt::Cipher::new(KEY).unwrap();
    assert_eq!(cipher.decrypt(&raw.unwrap().0).unwrap(), b"meanwhile");
    call(&bridge, "read", json!({"file_path": "work/race.md"}));
    call(
        &bridge,
        "write",
        json!({"file_path": "work/race.md", "content": "hello\n"}),
    );
    let replaced = call(
        &bridge,
        "edit",
        json!({"file_path": "work/race.md", "old_string": "hello", "new_string": "bye"}),
    );
    assert_eq!(replaced["structuredContent"]["replaced"], 1, "{replaced}");
}

#[test]
fn a_write_of_a_file_changed_since_it_was_read_is_refused() {
    let dir = TempDir::short("file-bridged-seen");
    let (bridge, _) = bridge(&dir, "work/race.md");
    call(
        &bridge,
        "write",
        json!({"file_path": "work/plan.md", "content": "ours\n"}),
    );
    call(&bridge, "read", json!({"file_path": "work/plan.md"}));
    // Another of the agent's units writes it meanwhile.
    let (_, version) = bridge.read("work/plan.md").unwrap().unwrap();
    bridge
        .write("work/plan.md", b"theirs\n", Expect::Version(version))
        .unwrap();
    let why = refused(&call(
        &bridge,
        "write",
        json!({"file_path": "work/plan.md", "content": "mine\n"}),
    ));
    assert!(why.contains("changed since it was read"), "{why}");
    assert_eq!(bridge.read("work/plan.md").unwrap().unwrap().0, b"theirs\n");
    // Read again, it's written; and written, written again.
    call(&bridge, "read", json!({"file_path": "work/plan.md"}));
    for text in ["mine\n", "mine, again\n"] {
        let wrote = call(
            &bridge,
            "write",
            json!({"file_path": "work/plan.md", "content": text}),
        );
        assert_eq!(wrote["structuredContent"]["created"], false, "{wrote}");
    }
    assert_eq!(
        bridge.read("work/plan.md").unwrap().unwrap().0,
        b"mine, again\n"
    );
}

/// An edit of a file another wrote since the session read it is refused,
/// as a write is, though the text it replaces is still there.
#[test]
fn an_edit_of_a_file_changed_since_it_was_read_is_refused() {
    let dir = TempDir::short("file-bridged-edit");
    let (bridge, _) = bridge(&dir, "work/race.md");
    call(
        &bridge,
        "write",
        json!({"file_path": "work/plan.md", "content": "ours\n"}),
    );
    call(&bridge, "read", json!({"file_path": "work/plan.md"}));
    let (_, version) = bridge.read("work/plan.md").unwrap().unwrap();
    bridge
        .write("work/plan.md", b"ours\ntheirs\n", Expect::Version(version))
        .unwrap();
    let edit = json!({"file_path": "work/plan.md", "old_string": "ours", "new_string": "mine"});
    let why = refused(&call(&bridge, "edit", edit.clone()));
    assert!(why.contains("changed since it was read"), "{why}");
    assert_eq!(
        bridge.read("work/plan.md").unwrap().unwrap().0,
        b"ours\ntheirs\n"
    );
    // Read again, it's edited.
    call(&bridge, "read", json!({"file_path": "work/plan.md"}));
    let replaced = call(&bridge, "edit", edit);
    assert_eq!(replaced["structuredContent"]["replaced"], 1, "{replaced}");
    assert_eq!(
        bridge.read("work/plan.md").unwrap().unwrap().0,
        b"mine\ntheirs\n"
    );
}

#[test]
fn a_call_naming_another_agent_is_refused() {
    let dir = TempDir::short("file-bridged-other");
    let (bridge, _) = bridge(&dir, "work/race.md");
    let forged = Session::of(files::Client {
        socket: bridge.socket.clone(),
        binding: Binding {
            identity: Identity {
                agent: "agent0000002".into(),
                service: String::new(),
                owner: "me@box".into(),
                teams: Vec::new(),
            },
            key: bridge.binding.key.duplicate(),
        },
    });
    let why = refused(&call(&forged, "read", json!({"file_path": "work/plan.md"})));
    assert!(why.contains("isn't bound here"), "{why}");
}

/// The agent's Home, and two teams' folders as the bridge says it's on
/// them: its owner's `research`, `notes` written and `sources` read, and
/// another account's `design`, read whole.
struct Teamed {
    dir: std::path::PathBuf,
}

const OTHER: &str = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e";

impl Homes for Teamed {
    fn home(&self, _who: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: Arc::new(DirStore::new(&self.dir.join("home"))?),
            cap: None,
            write: vec!["work".into()],
        })
    }
    fn team(&self, who: &Identity, team: &TeamRef) -> anyhow::Result<Option<TeamFolder>> {
        let (folder, write, read) = match (team.owner.as_deref(), team.name.as_str()) {
            (None, "research") => ("research", vec!["notes".into()], vec!["sources".into()]),
            (Some(OTHER), "design") => ("design", vec![], vec![".".into()]),
            _ => return Ok(None),
        };
        Ok(Some(TeamFolder {
            store: Arc::new(DirStore::new(&self.dir.join(folder))?),
            id: "t4m9q2x7k1ab".into(),
            owner: team.owner.clone().unwrap_or_else(|| who.owner.clone()),
            folders: link_harness_contract::harness::TeamFolders { write, read },
            keeps_index: true,
        }))
    }
}

#[test]
fn a_teams_folder_is_reached_at_teams_through_the_bridge() {
    let dir = TempDir::short("file-teams");
    let binding = link_store_contract::testing::bound(&dir.join("keys"), "agent0000001");
    let socket = dir.join("files.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    let homes = Arc::new(Teamed {
        dir: dir.join("bucket"),
    });
    std::thread::spawn(move || files::serve(listener, keys, homes));
    let session = Session::of(files::Client { socket, binding });
    // Its own team's: written where it writes, from the version read.
    let wrote = call(
        &session,
        "write",
        json!({"file_path": "teams/research/notes/plan.md", "content": "the plan"}),
    );
    assert_ne!(wrote["isError"], true, "{wrote}");
    let read = call(
        &session,
        "read",
        json!({"file_path": "teams/research/notes/plan.md"}),
    );
    assert!(read.to_string().contains("the plan"), "{read}");
    let replaced = call(
        &session,
        "edit",
        json!({"file_path": "teams/research/notes/plan.md", "old_string": "the", "new_string": "our"}),
    );
    assert_ne!(replaced["isError"], true, "{replaced}");
    let why = refused(&call(
        &session,
        "write",
        json!({"file_path": "teams/research/sources/s.md", "content": "x"}),
    ));
    assert!(why.contains("you may write only in"), "{why}");
    // Kept in the team's folder, not Home.
    let shared = DirStore::new(&dir.join("bucket/research")).unwrap();
    assert_eq!(
        shared.get("home/notes/plan.md").unwrap().unwrap().0,
        b"our plan"
    );
    assert!(
        DirStore::new(&dir.join("bucket/home"))
            .unwrap()
            .list("")
            .unwrap()
            .is_empty()
    );
    // Found as its paths are given.
    let found = call(
        &session,
        "grep",
        json!({"path": "teams/research", "pattern": "plan"}),
    );
    assert!(
        found.to_string().contains("teams/research/notes/plan.md"),
        "{found}"
    );
    // Another account's team, by its account: read, never written.
    DirStore::new(&dir.join("bucket/design"))
        .unwrap()
        .put("home/brief.md", b"the brief", &Expect::Absent)
        .unwrap();
    let read = call(
        &session,
        "read",
        json!({"file_path": format!("teams/{OTHER}/design/brief.md")}),
    );
    assert!(read.to_string().contains("the brief"), "{read}");
    refused(&call(
        &session,
        "write",
        json!({"file_path": format!("teams/{OTHER}/design/brief.md"), "content": "x"}),
    ));
    // A team it isn't on, by either form: refused.
    refused(&call(
        &session,
        "read",
        json!({"file_path": "teams/design/brief.md"}),
    ));
    refused(&call(
        &session,
        "read",
        json!({"file_path": format!("teams/{OTHER}/research/notes/plan.md")}),
    ));
}

/// A storage bridge in `dir` over the agent's folder there, as a cloud's
/// store keeps its instructions and skills, its calls proved with the
/// keys of `dir/keys` and answered for the agent the proof is of.
fn store_bridge(dir: &Path, binding: &Binding) -> link_store_contract::store::Bridged {
    let socket = dir.join("store.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    let kept = dir.join("store");
    std::thread::spawn(move || {
        link_sandbox_bridge::serve(
            listener,
            keys,
            &link_store_contract::store::BRIDGE_LIMITS,
            &AtomicBool::new(false),
            move |who: &Identity, request: link_store_contract::store::Request| {
                link_store_contract::store::file::Folder::at(&kept, &who.agent)?.answer(&request)
            },
        )
    });
    link_store_contract::store::Bridged {
        socket,
        binding: binding.duplicate(),
    }
}

/// Its own `instructions/` and `skills/` are the store's, not Home's: a
/// skill written where `[files]` says, from the version read, found by
/// `glob` and `grep` when named; its instructions read, never written, as
/// `[files]` doesn't name them; neither in Home's bucket.
#[test]
fn its_instructions_and_skills_are_reached_through_the_storage_bridge() {
    use link_store_contract::store::Store as _;
    let dir = TempDir::short("file-bridged-own");
    let (mut bridge, _) = bridge(&dir, "work/race.md");
    let store = store_bridge(&dir, &bridge.binding);
    let agent = link_store_contract::store::Agent {
        id: bridge.binding.identity.agent.clone(),
        owner: bridge.binding.identity.owner.clone(),
    };
    store
        .file_write(&agent, "instructions/instruction.md", b"Be brief.\n", 0)
        .unwrap();
    bridge.store = Some(store);
    let read = call(
        &bridge,
        "read",
        json!({"file_path": "instructions/instruction.md"}),
    );
    assert_eq!(said(&read), numbered("Be brief.\n"));
    let why = refused(&call(
        &bridge,
        "write",
        json!({"file_path": "instructions/instruction.md", "content": "x"}),
    ));
    assert!(why.contains("only in work/"), "{why}");
    let wrote = call(
        &bridge,
        "write",
        json!({"file_path": "skills/tea/SKILL.md", "content": "Steep three minutes.\n"}),
    );
    assert_eq!(wrote["structuredContent"]["created"], true, "{wrote}");
    let replaced = call(
        &bridge,
        "edit",
        json!({"file_path": "skills/tea/SKILL.md", "old_string": "three", "new_string": "four"}),
    );
    assert_eq!(replaced["structuredContent"]["replaced"], 1, "{replaced}");
    let kept = bridge.store.as_ref().unwrap();
    let (bytes, version) = kept
        .file_read(&agent, "skills/tea/SKILL.md")
        .unwrap()
        .unwrap();
    assert_eq!(bytes, b"Steep four minutes.\n");
    // Changed meanwhile, by another of the agent's units: refused.
    kept.file_write(&agent, "skills/tea/SKILL.md", b"Steep.\n", version)
        .unwrap();
    let why = refused(&call(
        &bridge,
        "write",
        json!({"file_path": "skills/tea/SKILL.md", "content": "x"}),
    ));
    assert!(why.contains("changed since it was read"), "{why}");
    let found = call(
        &bridge,
        "grep",
        json!({"pattern": "Steep", "path": "skills"}),
    );
    assert_eq!(finds(&found)[0]["path"], "skills/tea/SKILL.md");
    let named = call(
        &bridge,
        "glob",
        json!({"pattern": "**/*.md", "path": "skills"}),
    );
    assert_eq!(finds(&named)[0]["path"], "skills/tea/SKILL.md");
    // Neither in Home's bucket, nor found in Home.
    assert!(bridge.list("").unwrap().is_empty());
    let home = call(&bridge, "grep", json!({"pattern": "Steep"}));
    assert_eq!(finds(&home), json!([]), "{home}");
}
