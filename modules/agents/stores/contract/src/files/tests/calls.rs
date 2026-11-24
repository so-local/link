//! Calls through the bridge: reads, writes, Home's cap and the index.

use super::*;

/// A bucket that counts the objects its listings return, and says
/// sizes only by listing (as Cloud Storage does).
struct Counted {
    store: DirStore,
    listed: std::sync::atomic::AtomicUsize,
}

impl Store for Counted {
    fn get(&self, name: &str) -> anyhow::Result<Option<(Vec<u8>, Version)>> {
        self.store.get(name)
    }
    fn put(&self, name: &str, data: &[u8], expect: &Expect) -> anyhow::Result<Version> {
        self.store.put(name, data, expect)
    }
    fn delete(&self, name: &str, expect: &Expect) -> anyhow::Result<()> {
        self.store.delete(name, expect)
    }
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<Object>> {
        let found = self.store.list(prefix)?;
        self.listed
            .fetch_add(found.len(), std::sync::atomic::Ordering::SeqCst);
        Ok(found)
    }
}

#[test]
fn a_read_lists_its_own_object_never_its_folder() {
    let dir = TempDir::new("files-read-listed");
    let counted = Arc::new(Counted {
        store: DirStore::new(&dir.join("s")).unwrap(),
        listed: Default::default(),
    });
    let store = Arc::new(Crypt::new(counted.clone(), "a key").unwrap());
    for n in 0..200 {
        let name = format!("home/memory/note {n}.md");
        store.put(&name, b"a note", &Expect::Absent).unwrap();
    }
    let home = Home {
        store: store.clone(),
        cap: None,
        write: vec!["memory".into()],
    };
    counted.listed.store(0, std::sync::atomic::Ordering::SeqCst);
    for n in [0, 7, 199] {
        let name = format!("home/memory/note {n}.md");
        match get(&home, &name, &name).unwrap() {
            Answer::Got { .. } => {}
            other => panic!("{other:?}"),
        }
    }
    assert!(matches!(
        get(&home, "home/memory/none.md", "none").unwrap(),
        Answer::Missing { .. }
    ));
    // Three reads and a miss: the objects named, at most, not the
    // folder's 200 each time.
    let listed = counted.listed.load(std::sync::atomic::Ordering::SeqCst);
    assert!(listed <= 3, "{listed} objects listed for 4 reads");
    assert_eq!(store.size("home/memory/note 7.md").unwrap(), Some(6));
    assert_eq!(store.size("home/memory/none.md").unwrap(), None);
}

#[test]
fn a_read_past_the_bridges_most_is_refused_by_its_size() {
    let dir = TempDir::new("files-read-most");
    let store = Arc::new(Crypt::new(DirStore::new(&dir.join("s")).unwrap(), "a key").unwrap());
    let big = vec![0u8; READ_MAX as usize + 1];
    store.put("home/work/big", &big, &Expect::Absent).unwrap();
    let home = Home {
        store,
        cap: None,
        write: vec!["work".into()],
    };
    let err = get(&home, "home/work/big", "work/big").unwrap_err();
    assert!(
        format!("{err:#}").contains("larger than the bridge reads"),
        "{err:#}"
    );
}

#[test]
fn the_bridge_serves_only_the_agent_proved() {
    let dir = TempDir::short("files-bridge");
    let ours = crate::testing::bound(&dir.join("keys"), "ours00000001");
    // Another agent, bound with a key of its own.
    crate::bridge::Keyring::at(&dir.join("keys"))
        .bind(
            &who("theirs000001"),
            &link_secretive::Secret::from("1".repeat(64)),
        )
        .unwrap();
    let (socket, homes) = bridge(&dir, None);
    let client = Client {
        socket: socket.clone(),
        binding: ours.duplicate(),
    };
    assert_eq!(client.read("work/a.md").unwrap(), None);
    let v = client.write("work/a.md", b"one", Expect::Absent).unwrap();
    let (data, read) = client.read("work/a.md").unwrap().unwrap();
    assert_eq!((data.as_slice(), &read), (b"one".as_slice(), &v));
    // Two writes from one version: one is refused.
    client
        .write("work/a.md", b"two", Expect::Version(v.clone()))
        .unwrap();
    let refused = client.write("work/a.md", b"three", Expect::Version(v));
    assert!(is_conflict(&refused.unwrap_err()));
    // Its grants: not instructions/, not a hidden path, not outside.
    assert!(
        client
            .write("instructions/x.md", b"x", Expect::Any)
            .is_err()
    );
    assert!(client.write("work/.x", b"x", Expect::Any).is_err());
    assert!(client.read("../theirs000001/work/a.md").is_err());
    assert!(client.read("/etc/passwd").is_err());
    client
        .write("memory/notes.md", b"n", Expect::Absent)
        .unwrap();
    assert_eq!(
        homes.changed.lock().unwrap().last().unwrap(),
        &("ours00000001".to_string(), "memory/notes.md".to_string())
    );
    let listed = client.list("memory/").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, "memory/notes.md");
    // Its index, beside Home.
    client
        .index_put("link-memory/manifest.json", b"{}", Expect::Absent)
        .unwrap();
    assert_eq!(
        client
            .index_get("link-memory/manifest.json")
            .unwrap()
            .unwrap()
            .0,
        b"{}"
    );
    assert!(client.index_get("../home/work/a.md").is_err());
    // A call naming another agent, proved with this agent's key.
    let forged = Client {
        socket: socket.clone(),
        binding: Binding {
            identity: who("theirs000001"),
            key: ours.key.duplicate(),
        },
    };
    let err = forged.read("work/a.md").unwrap_err();
    assert!(format!("{err:#}").contains("proof"), "{err:#}");
    // An agent not bound here.
    let stranger = Client {
        socket,
        binding: Binding {
            identity: who("nobody000001"),
            key: ours.key,
        },
    };
    assert!(stranger.read("work/a.md").is_err());
}

/// The index as segments and a manifest: written, read back whole,
/// written again sending only what changed, a writer behind refused.
#[test]
fn the_index_is_kept_as_segments_and_a_manifest_written_last() {
    let dir = TempDir::new("files-index");
    let store = DirStore::new(&dir.join("s")).unwrap();
    assert!(index::read(&store, "link-memory").unwrap().is_none());
    let mut bytes = vec![7u8; index::SEGMENT + 10];
    let first = index::write(&store, "link-memory", &bytes, None).unwrap();
    assert_eq!(store.list("index/link-memory/seg-").unwrap().len(), 2);
    bytes[index::SEGMENT + 1] = 8;
    let second = index::write(&store, "link-memory", &bytes, Some(&first)).unwrap();
    // The unchanged first segment kept, the old second gone.
    assert_eq!(store.list("index/link-memory/seg-").unwrap().len(), 2);
    assert!(is_conflict(
        &index::write(&store, "link-memory", &bytes, Some(&first)).unwrap_err()
    ));
    let (read, version) = index::read(&store, "link-memory").unwrap().unwrap();
    assert_eq!((read, version), (bytes, second));
    index::remove(&store, "link-memory").unwrap();
    assert!(store.list("index/").unwrap().is_empty());
}

#[test]
fn a_write_past_homes_cap_is_refused() {
    let dir = TempDir::short("files-cap");
    let ours = crate::testing::bound(&dir.join("keys"), "ours00000001");
    let (socket, _) = bridge(&dir, Some(10));
    let client = Client {
        socket,
        binding: ours,
    };
    let v = client.write("work/a", b"12345678", Expect::Absent).unwrap();
    let err = client.write("work/b", b"123", Expect::Absent).unwrap_err();
    assert!(format!("{err:#}").contains("Home is full"), "{err:#}");
    // Smaller is always let through.
    client.write("work/a", b"1", Expect::Version(v)).unwrap();
    client.write("work/b", b"123", Expect::Absent).unwrap();
}

#[test]
fn writes_at_once_never_pass_homes_cap_together() {
    let dir = TempDir::short("files-cap-race");
    crate::testing::bound(&dir.join("keys"), "ours00000001");
    let store = Arc::new(DirStore::new(&dir.join("bucket")).unwrap());
    let homes = Plain {
        store: store.clone(),
        cap: Some(10),
    };
    let bridge = Bridge::new(
        Arc::new(crate::bridge::Keyring::at(&dir.join("keys"))),
        Arc::new(homes),
    );
    let home = bridge.homes.home(&who("ours00000001")).unwrap();
    // Eight writes of 4 bytes at once: two fit in 10.
    let start = std::sync::Barrier::new(8);
    let written = std::thread::scope(|s| {
        let writes: Vec<_> = (0..8)
            .map(|n| {
                let (bridge, home, start) = (&bridge, &home, &start);
                s.spawn(move || {
                    start.wait();
                    let name = format!("{HOME}work/{n}");
                    bridge.put(&who("ours00000001"), home, &name, b"1234", &Expect::Absent)
                })
            })
            .collect();
        writes
            .into_iter()
            .map(|w| w.join().unwrap())
            .filter(Result::is_ok)
            .count()
    });
    assert_eq!(written, 2);
    let held: u64 = store.list(HOME).unwrap().iter().map(|o| o.size).sum();
    assert_eq!(held, 8);
}

#[test]
fn homes_use_is_counted_again_once_its_agent_is_bound_anew() {
    let dir = TempDir::short("files-cap-bind");
    let keyring = crate::bridge::Keyring::at(&dir.join("keys"));
    crate::testing::bound(&dir.join("keys"), "ours00000001");
    let store = Arc::new(DirStore::new(&dir.join("bucket")).unwrap());
    let bridge = Bridge::new(
        Arc::new(keyring.clone()),
        Arc::new(Plain {
            store: store.clone(),
            cap: Some(10),
        }),
    );
    let ours = who("ours00000001");
    let home = bridge.homes.home(&ours).unwrap();
    bridge
        .put(&ours, &home, "home/work/a", b"1", &Expect::Absent)
        .unwrap();
    // Another bridge writes meanwhile; the agent sleeps and is bound
    // again, with a new key.
    store
        .put("home/work/b", b"12345678", &Expect::Absent)
        .unwrap();
    keyring
        .bind(&ours, &link_secretive::Secret::from("2".repeat(64)))
        .unwrap();
    let err = bridge
        .put(&ours, &home, "home/work/c", b"1234", &Expect::Absent)
        .unwrap_err();
    assert!(format!("{err:#}").contains("Home is full"), "{err:#}");
    bridge
        .put(&ours, &home, "home/work/c", b"1", &Expect::Absent)
        .unwrap();
}
