//! One agent served as its own process (`bridge`): answered over its
//! input and output, its index written back beside Home when its input
//! ends; or, once, made current and written back with nothing served.

use super::*;

fn bound(client: &files::Client) -> Bound {
    Bound {
        index: link_memory_tool_contract::IndexAt::Home,
        owner: client.binding.identity.owner.clone(),
        service: client.binding.identity.service.clone(),
        key: client.binding.key.duplicate(),
        files: Some(client.socket.clone()),
        storage: None,
        notes: None,
    }
}

fn one(client: &files::Client, cache: &Path, once: bool) -> service::One {
    service::One {
        agent: AGENT.into(),
        bound: bound(client),
        team: None,
        cache: cache.to_path_buf(),
        once,
    }
}

/// What `one` answers to a search for `query` once its Home is indexed,
/// in the background, its input then ended.
fn searched(one: service::One, query: &str) -> Vec<String> {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let serving = std::thread::spawn(move || {
        let output = theirs.try_clone().unwrap();
        service::serve_one(one, BufReader::new(theirs), output)
    });
    let mut session = BufReader::new(ours);
    let answer = search(&mut session, query);
    session
        .get_ref()
        .shutdown(std::net::Shutdown::Write)
        .unwrap();
    serving.join().unwrap().unwrap();
    found(&answer)
}

/// Served from its Home, its index is written back beside it when its
/// input ends, so the next wake fetches it.
#[test]
fn an_agent_served_alone_writes_its_index_back_when_its_input_ends() {
    let dir = TempDir::short("one-served");
    let (client, _) = files_bridge(&dir);
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    assert!(cache::manifest(&client).unwrap().is_none());
    let found = searched(one(&client, &dir.join("cache"), false), "sails");
    assert_eq!(found, ["memory/boat.md"]);
    assert!(cache::manifest(&client).unwrap().is_some(), "written back");
}

/// Once, it lists what changed in its Home, writes the index back and
/// removes its cache, serving nothing; a later start finds what it
/// indexed.
#[test]
fn once_it_writes_a_current_index_back_and_leaves_no_cache() {
    let dir = TempDir::short("one-once");
    let (client, _) = files_bridge(&dir);
    client
        .write(
            "memory/kite.md",
            b"The kite needs a new tail.\n",
            Expect::Absent,
        )
        .unwrap();
    let cache = dir.join("pass");
    let mut output = Vec::new();
    service::serve_one(one(&client, &cache, true), &b""[..], &mut output).unwrap();
    assert!(output.is_empty(), "nothing served");
    assert!(!cache.exists(), "its cache removed");
    let (manifest, _) = cache::manifest(&client).unwrap().unwrap();
    assert!(!manifest.segments.is_empty(), "{manifest:?}");
    let found = searched(one(&client, &dir.join("wake"), false), "tail");
    assert_eq!(found, ["memory/kite.md"]);
}

/// A key the bridge doesn't take is refused before anything is served.
#[test]
fn a_key_the_bridge_refuses_serves_nothing() {
    let dir = TempDir::short("one-refused");
    let (client, _) = files_bridge(&dir);
    let mut wrong = one(&client, &dir.join("cache"), false);
    wrong.bound.key = link_secretive::Secret::from("not the key".to_string());
    let mut output = Vec::new();
    assert!(service::serve_one(wrong, &b""[..], &mut output).is_err());
    assert!(output.is_empty());
}
