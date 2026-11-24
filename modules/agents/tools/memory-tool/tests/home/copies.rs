//! A copy of the index that can't be used as it is: made again.

use super::*;

/// The copy, written back from an index of Home made through the bridge.
fn written_copy(dir: &Path, client: &files::Client) -> Version {
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    std::fs::create_dir_all(dir.join("made")).unwrap();
    let mut index = Index::open_tree(
        Box::new(tree::Bridged::new(client.duplicate(), None)),
        Some(&dir.join("made/index.sqlite")),
    )
    .unwrap();
    index.refresh().unwrap();
    index
        .write_back(client, None, &dir.join("made/scratch"))
        .unwrap()
        .copy()
        .unwrap()
}

/// The copy changed as its agent may change it: `change` given its
/// SQLite file, the rest of its archive as it was; the copy's new version.
fn change_copy(
    dir: &Path,
    client: &files::Client,
    read: &Version,
    change: impl FnOnce(&Path),
) -> Version {
    use std::io::Read;
    let (bytes, _) = files::index::read(client, cache::ENGINE).unwrap().unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in tar::Archive::new(&bytes[..]).entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        parts.push((name, data));
    }
    let (_, sqlite) = parts
        .iter_mut()
        .find(|(name, _)| name == "index.sqlite")
        .unwrap();
    let file = dir.join("changing.sqlite");
    std::fs::write(&file, &sqlite).unwrap();
    change(&file);
    *sqlite = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    let mut archive = tar::Builder::new(Vec::new());
    for (name, data) in &parts {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        archive.append_data(&mut header, name, &data[..]).unwrap();
    }
    let bytes = archive.into_inner().unwrap();
    files::index::write(client, cache::ENGINE, &bytes, Some(read)).unwrap()
}

/// Runs `sql` on the index `file`, as plain SQLite does.
fn run_sql(file: &Path, sql: &str) {
    let db = rusqlite::Connection::open(file).unwrap();
    db.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))
        .unwrap();
    db.execute_batch(sql).unwrap();
}

/// Whether the index `file` holds a schema object named `name`.
fn holds(file: &Path, name: &str) -> bool {
    let db = rusqlite::Connection::open(file).unwrap();
    db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

/// A copy holding a view or a trigger the index doesn't make isn't
/// used: memory makes the index again from Home.
#[test]
fn a_copy_with_a_schema_of_its_own_is_made_again() {
    let dir = TempDir::short("home-schema");
    let (client, _) = files_bridge(&dir);
    let mut version = written_copy(&dir, &client);
    for (case, sql) in [
        ("view", "CREATE VIEW extra AS SELECT path FROM documents"),
        (
            "trigger",
            "CREATE TRIGGER extra AFTER INSERT ON documents BEGIN DELETE FROM meta; END",
        ),
    ] {
        version = change_copy(&dir, &client, &version, |file| run_sql(file, sql));
        let fetched = dir.join(case).join("index.sqlite");
        assert_eq!(
            cache::fetch(&client, &fetched).unwrap(),
            Some(version.clone())
        );
        assert!(!fetched.exists(), "the copy with a {case} was used");
    }
    // Bound, it's served from the index made again, without the trigger.
    let socket = cache_service(&dir, "cache");
    let (answer, mut session) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut session, "sails")), ["memory/boat.md"]);
    let cached = dir.join("cache/me@box").join(AGENT).join("index.sqlite");
    assert!(!holds(&cached, "extra"));
}

/// A copy that isn't a whole SQLite file isn't used: memory makes the
/// index again from Home.
#[test]
fn a_copy_not_whole_is_made_again() {
    let dir = TempDir::short("home-corrupt");
    let (client, _) = files_bridge(&dir);
    let version = written_copy(&dir, &client);
    let version = change_copy(&dir, &client, &version, |file| {
        // Every page but the first written over.
        let mut bytes = std::fs::read(file).unwrap();
        assert!(bytes.len() > 4096);
        bytes[4096..].fill(0x55);
        std::fs::write(file, bytes).unwrap();
    });
    let fetched = dir.join("fetched/index.sqlite");
    assert_eq!(cache::fetch(&client, &fetched).unwrap(), Some(version));
    assert!(!fetched.exists(), "the copy not whole was used");
    let socket = cache_service(&dir, "cache");
    let (answer, mut session) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut session, "sails")), ["memory/boat.md"]);
}

/// A copy whose segment is gone (another writer removed it) or isn't
/// what its manifest says doesn't fail every binding after: the index is
/// made again from Home, and written back over it.
#[test]
fn a_copy_missing_a_segment_is_made_again() {
    let dir = TempDir::short("home-segment");
    let (client, _) = files_bridge(&dir);
    let version = written_copy(&dir, &client);
    let segments: Vec<String> = client
        .index_list(cache::ENGINE)
        .unwrap()
        .into_iter()
        .map(|o| o.path)
        .filter(|path| path.contains("/seg-"))
        .collect();
    // One changed: fetched, it isn't used.
    client
        .index_put(&segments[0], b"not what it was", Expect::Any)
        .unwrap();
    let fetched = dir.join("fetched/index.sqlite");
    assert_eq!(
        cache::fetch(&client, &fetched).unwrap(),
        Some(version.clone())
    );
    assert!(!fetched.exists());
    // Gone: bound, it's served, and the copy written back whole.
    client.index_delete(&segments[0], Expect::Any).unwrap();
    let socket = cache_service(&dir, "cache");
    let (answer, mut session) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut session, "sails")), ["memory/boat.md"]);
    let flush = serde_json::to_value(Control::Flush {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&socket, &flush).0["ok"], true);
    let again = dir.join("again/index.sqlite");
    assert_ne!(cache::fetch(&client, &again).unwrap(), Some(version));
    assert!(again.exists());
}

/// A cache is served only once the files bridge takes the binding's key:
/// a key it doesn't take, or one unbound since, is refused, cache or not.
#[test]
fn a_cache_is_served_only_once_the_bridge_takes_its_key() {
    let dir = TempDir::short("home-key");
    let (client, _) = files_bridge(&dir);
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    let socket = cache_service(&dir, "cache");
    let (answer, mut session) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(found(&search(&mut session, "sails")), ["memory/boat.md"]);
    let end = serde_json::to_value(Control::End {
        owner: "me@box".into(),
        agent: AGENT.into(),
    })
    .unwrap();
    assert_eq!(first(&socket, &end).0["ok"], true);
    // Its cache stays, but a key the bridge doesn't take isn't served it.
    let mut forged = client.duplicate();
    forged.binding.key = "not its key".to_string().into();
    let (answer, _) = first(&socket, &hello(&forged));
    assert_ne!(answer["ok"], true, "{answer}");
    // Bound again, then unbound at the bridge: the same binding refused.
    let (answer, _) = first(&socket, &hello(&client));
    assert_eq!(answer["ok"], true, "{answer}");
    Keyring::at(&dir.join("keys")).unbind(AGENT, &link_secret_contract::keeper::RouteRevoker);
    let (answer, _) = first(&socket, &hello(&client));
    assert_ne!(answer["ok"], true, "{answer}");
}
