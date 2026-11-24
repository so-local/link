//! Link Memory finds a conversation Link Harness keeps, as a
//! conversation. Memory never links a harness, so this writes the folder
//! as Link Harness's docs say it keeps one: an event per file, never
//! searched, and `conversation.md`, the copy for people and memory.
//! Link Harness's own tests search a conversation it wrote, through this
//! program.

use std::io::Write as _;

use link_memory_tool::{Index, Kind};

#[test]
fn memory_finds_a_harness_conversation_as_a_conversation() {
    let root = link_test_kit::TempDir::new("reads_conversations");
    let conversation = root.join("chats/telegram-42");
    std::fs::create_dir_all(&conversation).unwrap();
    let at = "2026-09-29T14:03:07Z";
    std::fs::write(
        conversation.join("00000001.json"),
        format!(r#"{{"seq":1,"role":"user","text":"My dog is called Biscuit.","at":"{at}"}}"#),
    )
    .unwrap();
    std::fs::write(
        conversation.join("00000002.json"),
        format!(r#"{{"seq":2,"role":"assistant","text":"Noted.","at":"{at}"}}"#),
    )
    .unwrap();
    std::fs::write(
        conversation.join("conversation.md"),
        format!(
            "# telegram:42\n\n## You, {at}\n\nMy dog is called Biscuit.\n\n\
             ## Agent, {at}\n\nNoted.\n"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("memory")).unwrap();
    std::fs::write(root.join("memory/plans.md"), "Ship the Biscuit feature.").unwrap();

    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let mut hits: Vec<(Kind, String)> = index
        .search("Biscuit", None, 10)
        .unwrap()
        .into_iter()
        .map(|hit| (hit.kind, hit.path))
        .collect();
    hits.sort_by(|a, b| a.1.cmp(&b.1));

    // The conversation's copy, as a conversation, and never its events;
    // the file dropped in, as a file.
    assert_eq!(
        hits,
        vec![
            (
                Kind::Conversation,
                "chats/telegram-42/conversation.md".to_string()
            ),
            (Kind::File, "memory/plans.md".to_string()),
        ]
    );
}

/// A conversation kept compact (`events.zst`, its events in zstd frames,
/// and those since in `events.jsonl`) has no copy on disk: memory makes
/// its own, found as the copy a log's would be, and made again as the
/// conversation grows.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn memory_makes_a_compact_conversations_copy() {
    use link_store_contract::store::compact;
    let root = link_test_kit::TempDir::new("reads_conversations-compact");
    let conversation = root.join("chats/telegram-42");
    std::fs::create_dir_all(&conversation).unwrap();
    let at = "2026-09-29T14:03:07Z";
    let line = |seq: u64, role: &str, text: &str| {
        format!(r#"{{"seq":{seq},"role":"{role}","text":"{text}","at":"{at}"}}"#) + "\n"
    };
    let mut frames = compact::named("telegram:42");
    let framed = line(1, "user", "My dog is called Biscuit.") + &line(2, "assistant", "Noted.");
    frames.extend(compact::encode(framed.as_bytes()).unwrap());
    std::fs::write(conversation.join(compact::FRAMES), &frames).unwrap();
    std::fs::write(
        conversation.join(compact::TAIL),
        line(3, "user", "And my cat?") + &line(4, "assistant", "Pickles, you said."),
    )
    .unwrap();

    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let path = "chats/telegram-42/conversation.md";
    for word in ["Biscuit", "Pickles"] {
        let hits = index.search(word, None, 10).unwrap();
        assert_eq!(hits.len(), 1, "{word}");
        assert_eq!(
            (hits[0].kind, hits[0].path.as_str()),
            (Kind::Conversation, path)
        );
    }
    let copy = index.read(path, 1, None, None).unwrap().unwrap();
    assert!(copy.text.starts_with("# telegram:42"), "{}", copy.text);
    // Its events never are.
    assert!(index.search("seq", None, 10).unwrap().is_empty());

    // Grown: made again.
    let mut tail = std::fs::OpenOptions::new()
        .append(true)
        .open(conversation.join(compact::TAIL))
        .unwrap();
    std::io::Write::write_all(
        &mut tail,
        (line(5, "user", "Rename the dog Waffles.") + &line(6, "assistant", "Done.")).as_bytes(),
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    index.refresh().unwrap();
    assert_eq!(index.search("Waffles", None, 10).unwrap().len(), 1);
}

/// A conversation folder naming a store driver (`store.<name>`, which
/// the agent can write) is read only by the driver the agent's spec
/// names: one naming another is never run, so the agent never picks
/// what memory runs.
#[test]
fn a_folder_never_picks_the_driver_memory_runs() {
    let root = link_test_kit::TempDir::new("reads_conversations-driver");
    let conversation = root.join("chats/telegram-42");
    std::fs::create_dir_all(&conversation).unwrap();
    std::fs::write(conversation.join("store.chosen"), b"").unwrap();
    let mut index = Index::open(&root, None).unwrap();
    for driver in [None, Some("sqlite")] {
        index.driven_by(driver.map(str::to_string));
        index.refresh().unwrap();
        assert_eq!(index.list(&Default::default(), 10).unwrap().total, 0);
    }
}

/// A conversation of 5 MB, its copy a file or one memory makes of a
/// compact one, is indexed whole: a word from its first turn is found as
/// one from its last. Indexed, it's kept, never read again while it's
/// as it was.
#[test]
fn a_long_conversations_whole_history_is_indexed() {
    use link_store_contract::store::compact;
    let root = link_test_kit::TempDir::new("reads_conversations-long");
    // A log's copy, on disk.
    let filler = "The weather was mild and nothing much happened. ".repeat(40) + "\n\n";
    let mut copy = String::from("# telegram:42\n\nThe first word was Aardvark.\n\n");
    while copy.len() < 5 << 20 {
        copy.push_str(&filler);
    }
    copy.push_str("The last word was Zebra.\n");
    let logged = root.join("chats/telegram-42");
    std::fs::create_dir_all(&logged).unwrap();
    std::fs::write(logged.join("conversation.md"), &copy).unwrap();
    // A compact one, its frames as long.
    let at = "2026-09-29T14:03:07Z";
    let line = |seq: u64, role: &str, text: &str| {
        format!(r#"{{"seq":{seq},"role":"{role}","text":"{text}","at":"{at}"}}"#) + "\n"
    };
    let compacted = root.join("chats/discord-7");
    std::fs::create_dir_all(&compacted).unwrap();
    let mut frames = compact::named("discord:7");
    let mut seq = 0;
    let mut frame = String::new();
    for n in 0..2500 {
        seq += 1;
        let (role, text) = match n {
            0 => ("user", "The first word was Armadillo.".to_string()),
            _ if n % 2 == 1 => ("assistant", filler.trim().to_string()),
            _ => ("user", filler.trim().to_string()),
        };
        frame.push_str(&line(seq, role, &text));
        if frame.len() as u64 >= compact::FRAME_BYTES {
            frames.extend(compact::encode(frame.as_bytes()).unwrap());
            frame.clear();
        }
    }
    frames.extend(compact::encode(frame.as_bytes()).unwrap());
    std::fs::write(compacted.join(compact::FRAMES), &frames).unwrap();
    std::fs::write(
        compacted.join(compact::TAIL),
        // The copy goes up to the agent's last answer.
        line(seq + 1, "user", "The last word was Yak.") + &line(seq + 2, "assistant", "Noted."),
    )
    .unwrap();

    let mut index = Index::open(&root, None).unwrap();
    let refreshed = index.refresh().unwrap();
    assert_eq!(refreshed.indexed, 2);
    let found = |word: &str| -> Vec<String> {
        index
            .search(word, None, 10)
            .unwrap()
            .into_iter()
            .map(|hit| hit.path)
            .collect()
    };
    assert_eq!(found("Zebra"), ["chats/telegram-42/conversation.md"]);
    assert_eq!(found("Aardvark"), ["chats/telegram-42/conversation.md"]);
    assert_eq!(found("Yak"), ["chats/discord-7/conversation.md"]);
    assert_eq!(found("Armadillo"), ["chats/discord-7/conversation.md"]);
    // Read as its file is, from its first line.
    let read = index
        .read("chats/telegram-42/conversation.md", 1, Some(3), None)
        .unwrap()
        .unwrap();
    assert_eq!(read.text, "# telegram:42\n\nThe first word was Aardvark.");
    // As it was: nothing read again.
    assert_eq!(index.refresh().unwrap().indexed, 0);
    assert_eq!(index.list(&Default::default(), 10).unwrap().total, 2);
}

/// A compact conversation whose events don't read is kept by its name,
/// status saying why, and isn't read again while it's as it was: the
/// next refresh finds nothing changed.
#[test]
fn a_conversation_that_doesnt_read_is_kept_by_its_name() {
    use link_store_contract::store::compact;
    let root = link_test_kit::TempDir::new("reads_conversations-unread");
    let conversation = root.join("chats/discord-9");
    std::fs::create_dir_all(&conversation).unwrap();
    std::fs::write(
        conversation.join(compact::FRAMES),
        compact::named("discord:9"),
    )
    .unwrap();
    // Its tail's line isn't an event.
    std::fs::write(conversation.join(compact::TAIL), "not an event\n").unwrap();
    let mut index = Index::open(&root, None).unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 1);
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    assert!(
        status["notices"]
            .to_string()
            .contains("chats/discord-9/conversation.md is found by its name only"),
        "{status}"
    );
    assert_eq!(index.refresh().unwrap().indexed, 0);
    let hits = index.search("discord-9", None, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "discord-9");
}

/// A folder whose reads are counted: whole, and of what a conversation
/// grew by, in bytes.
struct Counted {
    folder: link_memory_tool::tree::Folder,
    whole: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    grown: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl link_memory_tool::tree::Tree for Counted {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> anyhow::Result<std::collections::BTreeMap<String, link_memory_tool::tree::Found>> {
        self.folder.list(known)
    }

    fn read(
        &mut self,
        path: &str,
        found: &link_memory_tool::tree::Found,
    ) -> anyhow::Result<Option<link_memory_tool::tree::Read>> {
        let read = self.folder.read(path, found)?;
        let bytes = read.as_ref().map_or(0, |read| read.bytes.len());
        self.whole
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
        Ok(read)
    }

    fn read_since(
        &mut self,
        path: &str,
        found: &link_memory_tool::tree::Found,
        since: link_memory_tool::tree::Since,
    ) -> anyhow::Result<Option<link_memory_tool::tree::Read>> {
        let read = self.folder.read_since(path, found, since)?;
        let bytes = read.as_ref().map_or(0, |read| read.bytes.len());
        self.grown
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
        Ok(read)
    }

    fn files(&self) -> std::sync::Arc<dyn link_memory_tool::tree::Files> {
        self.folder.files()
    }

    fn shown(&self) -> &std::path::Path {
        self.folder.shown()
    }
}

/// A turn appended to a 5 MB conversation, its copy a file or one memory
/// makes of a compact one, reads only what it added, and the passage it
/// joins: never the conversation again. Its word is found, and the first
/// turn's still is.
#[test]
fn a_turn_appended_reads_only_what_it_added() {
    use link_store_contract::store::compact;
    use std::sync::atomic::Ordering;
    let root = link_test_kit::TempDir::new("reads_conversations-grown");
    let filler = "The weather was mild and nothing much happened. ".repeat(40) + "\n\n";
    let mut copy = String::from("# telegram:42\n\nThe first word was Aardvark.\n\n");
    while copy.len() < 5 << 20 {
        copy.push_str(&filler);
    }
    let logged = root.join("chats/telegram-42/conversation.md");
    std::fs::create_dir_all(logged.parent().unwrap()).unwrap();
    std::fs::write(&logged, &copy).unwrap();
    let at = "2026-09-29T14:03:07Z";
    let line = |seq: u64, role: &str, text: &str| {
        format!(r#"{{"seq":{seq},"role":"{role}","text":"{text}","at":"{at}"}}"#) + "\n"
    };
    let compacted = root.join("chats/discord-7");
    std::fs::create_dir_all(&compacted).unwrap();
    let mut frames = compact::named("discord:7");
    let mut frame = String::new();
    for seq in 1..=2500u64 {
        let (role, text) = match seq {
            1 => ("user", "The first word was Armadillo.".to_string()),
            _ if seq % 2 == 0 => ("assistant", filler.trim().to_string()),
            _ => ("user", filler.trim().to_string()),
        };
        frame.push_str(&line(seq, role, &text));
        if frame.len() as u64 >= compact::FRAME_BYTES {
            frames.extend(compact::encode(frame.as_bytes()).unwrap());
            frame.clear();
        }
    }
    frames.extend(compact::encode(frame.as_bytes()).unwrap());
    std::fs::write(compacted.join(compact::FRAMES), &frames).unwrap();
    std::fs::write(compacted.join(compact::TAIL), "").unwrap();

    let whole = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let grown = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Counted {
        folder: link_memory_tool::tree::Folder::new(
            link_agent_contract::folder::Folder::flat(&root),
            link_store_contract::store::Agent {
                id: "main".into(),
                owner: String::new(),
            },
        ),
        whole: whole.clone(),
        grown: grown.clone(),
    };
    let mut index = Index::open_tree(Box::new(counted), None).unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 2);
    // Both whole, 5 MB each.
    assert!(whole.load(Ordering::SeqCst) > 9 << 20);
    whole.store(0, Ordering::SeqCst);

    // A turn each.
    let turn = "## You, 2026-09-29T14:05:00Z\n\nThe new word is Quokka.\n";
    std::fs::OpenOptions::new()
        .append(true)
        .open(&logged)
        .unwrap()
        .write_all(turn.as_bytes())
        .unwrap();
    std::fs::write(
        compacted.join(compact::TAIL),
        line(2501, "user", "The new word is Wombat.") + &line(2502, "assistant", "Noted."),
    )
    .unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 2);
    assert_eq!(whole.load(Ordering::SeqCst), 0, "read whole again");
    // What each added, and at most the passage each joins.
    let most = 2 * 4 * link_memory_tool::semantic::CHUNK + 2 * turn.len();
    let read = grown.load(Ordering::SeqCst);
    assert!(read <= most, "{read} bytes read, over {most}");
    let found = |word: &str| -> Vec<String> {
        index
            .search(word, None, 10)
            .unwrap()
            .into_iter()
            .map(|hit| hit.path)
            .collect()
    };
    assert_eq!(found("Quokka"), ["chats/telegram-42/conversation.md"]);
    assert_eq!(found("Wombat"), ["chats/discord-7/conversation.md"]);
    assert_eq!(found("Aardvark"), ["chats/telegram-42/conversation.md"]);
    assert_eq!(found("Armadillo"), ["chats/discord-7/conversation.md"]);
    // The new turn's line, as the file has it.
    let hit = &index.search("Quokka", None, 1).unwrap()[0];
    let read = index
        .read("chats/telegram-42/conversation.md", hit.line, Some(1), None)
        .unwrap()
        .unwrap();
    assert_eq!(read.text, "The new word is Quokka.");
}
