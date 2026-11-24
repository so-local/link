//! Memory over Home in a bucket, through the files bridge: only
//! `memory/` read, and the index written back whole.

use super::*;

/// Memory over Home in a bucket reads only `memory/`, through the bridge.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn memory_over_the_bridge_reads_only_memory() {
    let dir = TempDir::short("home-bridged");
    let (client, bucket) = files_bridge(&dir);
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    client
        .write("work/boat.md", b"The boat in work.\n", Expect::Absent)
        .unwrap();
    std::fs::copy(
        Path::new(FIXTURES).join("refund.pdf"),
        dir.join("refund.pdf"),
    )
    .unwrap();
    client
        .write(
            "memory/refund.pdf",
            &std::fs::read(dir.join("refund.pdf")).unwrap(),
            Expect::Absent,
        )
        .unwrap();
    bucket.read.lock().unwrap().clear();
    let mut index =
        Index::open_tree(Box::new(tree::Bridged::new(client.duplicate(), None)), None).unwrap();
    index.extract_with(extractor());
    index.refresh().unwrap();
    assert_eq!(paths(&index, "sails"), ["memory/boat.md"]);
    assert_eq!(paths(&index, "accountant"), ["memory/refund.pdf"]);
    let read = bucket.read.lock().unwrap().clone();
    assert!(!read.is_empty());
    assert!(
        read.iter().all(|name| name.starts_with("home/memory/")),
        "{read:?}"
    );
    // Listed again only when told something changed.
    client
        .write("memory/later.md", b"The anchor chain.\n", Expect::Absent)
        .unwrap();
    index.turn().unwrap();
    assert!(paths(&index, "anchor").is_empty());
    // The bridge's notice names the path: listed once the changes settle.
    index.common().noted("memory/later.md");
    index.turn().unwrap();
    assert!(
        paths(&index, "anchor").is_empty(),
        "listed before it settled"
    );
    std::thread::sleep(link_memory_tool::SETTLE);
    index.turn().unwrap();
    assert_eq!(paths(&index, "anchor"), ["memory/later.md"]);
}

/// A skill in Home in a bucket is read by its path through the bridge,
/// never listed, indexed or found; nothing else outside `memory/` is.
#[test]
fn a_skill_over_the_bridge_is_read_never_indexed() {
    let dir = TempDir::short("home-skill");
    let (client, bucket) = files_bridge(&dir);
    let skill = "---\nname: knots\ndescription: Tie knots.\n---\n\nA bowline first.\n";
    client
        .write("skills/knots/SKILL.md", skill.as_bytes(), Expect::Absent)
        .unwrap();
    client
        .write("work/knots.md", b"A bowline in work.\n", Expect::Absent)
        .unwrap();
    let big = "bowline ".repeat(link_memory_tool::limits::limits().file_max_bytes as usize / 8 + 1);
    client
        .write("skills/big/SKILL.md", big.as_bytes(), Expect::Absent)
        .unwrap();
    bucket.read.lock().unwrap().clear();
    let mut index =
        Index::open_tree(Box::new(tree::Bridged::new(client.duplicate(), None)), None).unwrap();
    index.refresh().unwrap();
    assert!(paths(&index, "bowline").is_empty());
    assert!(
        bucket
            .read
            .lock()
            .unwrap()
            .iter()
            .all(|name| name.starts_with("home/memory/"))
    );
    let read = index
        .read("skills/knots/SKILL.md", 1, None, None)
        .unwrap()
        .unwrap();
    assert_eq!((read.title.as_str(), read.lines), ("knots", 6));
    assert!(read.text.ends_with("A bowline first."), "{}", read.text);
    for path in [
        "skills/big/SKILL.md",
        "skills/../work/knots.md",
        "work/knots.md",
        "skills/none/SKILL.md",
    ] {
        assert!(index.read(path, 1, None, None).unwrap().is_none(), "{path}");
    }
    // Read, never indexed.
    index.common().changed();
    index.turn().unwrap();
    assert!(paths(&index, "bowline").is_empty());
}

/// The index written back as segments and a manifest, fetched again
/// whole; a writer behind another refused.
#[test]
fn the_index_is_written_back_and_fetched_whole() {
    let dir = TempDir::short("home-copy");
    let (client, _) = files_bridge(&dir);
    client
        .write(
            "memory/boat.md",
            b"The boat needs new sails.\n",
            Expect::Absent,
        )
        .unwrap();
    let file = dir.join("cache/index.sqlite");
    std::fs::create_dir_all(dir.join("cache")).unwrap();
    assert_eq!(cache::fetch(&client, &file).unwrap(), None);
    let mut index = Index::open_tree(
        Box::new(tree::Bridged::new(client.duplicate(), None)),
        Some(&file),
    )
    .unwrap();
    index.refresh().unwrap();
    let first = index
        .write_back(&client, None, &dir.join("cache/scratch"))
        .unwrap()
        .copy()
        .unwrap();
    // A second writer from no copy, and then from the first's: one refused.
    let behind = index.write_back(&client, None, &dir.join("cache/scratch"));
    assert!(files::is_conflict(&behind.unwrap_err()));
    let second = index
        .write_back(&client, Some(&first), &dir.join("cache/scratch"))
        .unwrap()
        .copy()
        .unwrap();
    // Unchanged, no segment is sent again: only the manifest moved.
    let segments: Vec<_> = client
        .index_list(cache::ENGINE)
        .unwrap()
        .into_iter()
        .filter(|o| o.path.contains("/seg-"))
        .collect();
    let (manifest, _) = cache::manifest(&client).unwrap().unwrap();
    assert_eq!(segments.len(), manifest.segments.len());
    drop(index);
    let fetched = dir.join("fetched/index.sqlite");
    assert_eq!(cache::fetch(&client, &fetched).unwrap(), Some(second));
    // Its words came with it, in their folder beside it.
    assert!(dir.join("fetched/index.sqlite.words/meta.json").exists());
    assert!(manifest.size > std::fs::metadata(&fetched).unwrap().len());
    let again =
        Index::open_tree(Box::new(tree::Bridged::new(client, None)), Some(&fetched)).unwrap();
    assert_eq!(paths(&again, "sails"), ["memory/boat.md"]);
}
