//! Indexing a folder and searching it: a conversation from long ago and a
//! file dropped in both found, each saying which it is; changes seen at
//! the next refresh; nothing outside the folder reached.

use std::path::Path;

use link_memory_tool::{Filter, Index, Kind, Refreshed, parse_since};
use link_test_kit::TempDir;

/// A largest file a test's spec chooses, small enough to write past.
const SMALL: u64 = 1 << 20;

/// A fresh folder for test `name`.
fn folder(name: &str) -> TempDir {
    TempDir::new(&format!("index-{name}"))
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A folder with notes you dropped in and a conversation, laid out as
/// Link's harness writes it: its events, and its markdown copy.
fn memory(name: &str) -> TempDir {
    let root = folder(name);
    write(
        &root,
        "memory/notes/garden.md",
        "# The garden\n\nPlant the tomatoes in May, after the last frost.\n",
    );
    write(
        &root,
        "memory/notes/taxes.txt",
        "Receipts for the accountant go in the blue folder.\n",
    );
    write(
        &root,
        "chats/telegram-42/000001.json",
        r#"{"role":"user","text":"when do tomatoes go in?"}"#,
    );
    write(
        &root,
        "chats/telegram-42/conversation.md",
        "**You:** When should I plant the tomatoes?\n\n**Agent:** After the last frost, in May.\n",
    );
    root
}

#[test]
fn a_search_finds_conversations_and_files_and_says_which() {
    let root = memory("search");
    let mut index = Index::open(&root, None).unwrap();
    let refreshed = index.refresh().unwrap();
    // The events aren't indexed: the markdown copy is the conversation.
    assert_eq!(
        refreshed,
        Refreshed {
            indexed: 3,
            removed: 0,
            total: 3
        }
    );

    let hits = index.search("tomatoes", None, 10).unwrap();
    let mut found: Vec<(&str, Kind)> = hits.iter().map(|h| (h.path.as_str(), h.kind)).collect();
    found.sort();
    assert_eq!(
        found,
        [
            ("chats/telegram-42/conversation.md", Kind::Conversation),
            ("memory/notes/garden.md", Kind::File),
        ]
    );
    let garden = hits.iter().find(|h| h.kind == Kind::File).unwrap();
    assert_eq!(garden.title, "The garden");
    assert!(
        garden.snippet.contains("**tomatoes**"),
        "{}",
        garden.snippet
    );
    let chat = hits.iter().find(|h| h.kind == Kind::Conversation).unwrap();
    assert_eq!(chat.title, "telegram-42");

    // One kind only.
    let only = index
        .search("tomatoes", Some(Kind::Conversation), 10)
        .unwrap();
    assert_eq!(only.len(), 1);
    assert_eq!(only[0].kind, Kind::Conversation);

    // Stemmed: "planting" finds "plant"; any word counts, more rank higher.
    let hits = index.search("planting receipts", None, 10).unwrap();
    assert_eq!(hits.len(), 3);
    let hits = index.search("blue accountant folder", None, 10).unwrap();
    assert_eq!(hits[0].path, "memory/notes/taxes.txt");

    // FTS5's syntax is only words; no words is an error, not everything.
    assert!(index.search("tomatoes\" OR NEAR(", None, 10).is_ok());
    assert!(index.search("  *  ", None, 10).is_err());
}

#[test]
fn a_refresh_sees_what_changed_and_only_that() {
    let root = memory("refresh");
    let kept = folder("refresh-file");
    let file = kept.join("index.sqlite");
    let mut index = Index::open(&root, Some(&file)).unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 3);
    assert_eq!(index.refresh().unwrap().indexed, 0);
    drop(index);

    // Kept in a file: a new server reads nothing again.
    let mut index = Index::open(&root, Some(&file)).unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 0);

    write(
        &root,
        "memory/notes/garden.md",
        "# The garden\n\nThe roses need pruning in March, a longer note.\n",
    );
    std::fs::remove_file(root.join("memory/notes/taxes.txt")).unwrap();
    write(&root, "memory/ideas.md", "A shed for the bicycles.\n");
    let refreshed = index.refresh().unwrap();
    assert_eq!(
        refreshed,
        Refreshed {
            indexed: 2,
            removed: 1,
            total: 3
        }
    );
    let hits = index.search("tomatoes", None, 10).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].kind, Kind::Conversation);
    assert_eq!(index.search("roses", None, 10).unwrap().len(), 1);
    assert!(index.search("accountant", None, 10).unwrap().is_empty());
    assert_eq!(
        index.search("bicycles", None, 10).unwrap()[0].path,
        "memory/ideas.md"
    );
}

#[test]
fn read_and_list_give_what_is_indexed() {
    let root = memory("read");
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();

    let chat = index
        .read("chats/telegram-42/conversation.md", 1, None, None)
        .unwrap()
        .unwrap();
    assert_eq!(chat.kind, Kind::Conversation);
    assert_eq!(chat.lines, 3);
    assert!(chat.text.starts_with("**You:** When should I plant"));
    let line = index
        .read("./memory/notes/garden.md", 3, Some(1), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        line.text,
        "Plant the tomatoes in May, after the last frost."
    );
    // Any number of lines, however large, is all that's left.
    let rest = index
        .read("memory/notes/garden.md", 3, Some(usize::MAX), None)
        .unwrap()
        .unwrap();
    assert!(rest.text.starts_with("Plant the tomatoes in May"));
    assert_eq!(rest.next, None);
    assert_eq!(
        index
            .read("memory/notes/garden.md", 99, None, None)
            .unwrap()
            .unwrap()
            .text,
        ""
    );

    let list = |kind: Option<Kind>, under: Option<&str>, limit| {
        let filter = Filter {
            kind,
            under: under.map(str::to_string),
            since: None,
        };
        let listing = index.list(&filter, limit).unwrap();
        assert!(listing.removed.is_none());
        (listing.documents, listing.total)
    };
    let (all, total) = list(None, None, 50);
    assert_eq!((all.len(), total), (3, 3));
    let (chats, total) = list(Some(Kind::Conversation), None, 50);
    assert_eq!(total, 1);
    assert_eq!(chats[0].title, "telegram-42");
    let (notes, total) = list(None, Some("memory/notes/"), 1);
    assert_eq!((notes.len(), total), (1, 2));
    // A folder's name, not a prefix of one: `note` isn't `notes`.
    assert_eq!(list(None, Some("memory/note"), 50).1, 0);
    assert_eq!(list(None, Some("memory/n_tes"), 50).1, 0);
}

/// A skill the agent runs (`skills/<name>/SKILL.md`) isn't memory: never
/// listed nor found, only read by its path, as its harness's prompt
/// says, from where it is: not through a link, nor out of `skills/`. The
/// same skill kept in `memory/` is memory, as any file is, by its path,
/// and listed by its name, not its file's.
#[test]
fn a_skill_is_memory_only_in_memory() {
    let root = memory("skill");
    let skill =
        "---\nname: sourdough\ndescription: Bake sourdough bread.\n---\n\n1. Feed the starter.\n";
    write(&root, "skills/sourdough/SKILL.md", skill);
    write(&root, "memory/skills/sourdough/SKILL.md", skill);
    let mut index = Index::open(&root, None).unwrap();
    assert_eq!(index.refresh().unwrap().total, 4);

    let read = index
        .read("skills/sourdough/SKILL.md", 3, Some(1), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            read.path.as_str(),
            read.kind,
            read.title.as_str(),
            read.lines,
            read.text.as_str(),
            read.next
        ),
        (
            "skills/sourdough/SKILL.md",
            Kind::File,
            "sourdough",
            6,
            "description: Bake sourdough bread.",
            None
        )
    );
    // Written there since the walk: read too, and still never indexed.
    write(&root, "skills/rye/SKILL.md", "Rye needs longer.\n");
    let rye = index
        .read("./skills/rye/SKILL.md", 1, None, None)
        .unwrap()
        .unwrap();
    assert_eq!(rye.text, "Rye needs longer.");
    // Not through a link, out of `skills/`, hidden, from the root, or
    // not text.
    let outside = folder("skill-outside");
    write(&outside, "secret.md", "The password is swordfish.\n");
    std::os::unix::fs::symlink(outside.join("secret.md"), root.join("skills/link.md")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("skills/linked")).unwrap();
    write(&root, "work/x.md", "swordfish\n");
    write(&root, "instructions/x.md", "swordfish\n");
    write(&root, "skills/.hidden/SKILL.md", "swordfish\n");
    write(&root, "skills/photo.png", "swordfish\n");
    for path in [
        "skills/link.md",
        "skills/linked/secret.md",
        "skills/../work/x.md",
        "skills/./../work/x.md",
        "skills//rye/SKILL.md",
        "skills/.hidden/SKILL.md",
        "skills/photo.png",
        "/skills/sourdough/SKILL.md",
        "work/x.md",
        "instructions/x.md",
        "skills",
        "skills/",
    ] {
        assert!(index.read(path, 1, None, None).unwrap().is_none(), "{path}");
    }
    assert_eq!(index.refresh().unwrap().total, 4);
    let kept = index
        .read("memory/skills/sourdough/SKILL.md", 1, None, None)
        .unwrap()
        .unwrap();
    assert_eq!(kept.kind, Kind::File);
    assert!(kept.text.contains("1. Feed the starter."), "{}", kept.text);

    let titles = |under: &str| -> Vec<String> {
        let filter = Filter {
            kind: None,
            under: Some(under.into()),
            since: None,
        };
        let listing = index.list(&filter, 50).unwrap();
        listing.documents.into_iter().map(|d| d.title).collect()
    };
    assert!(titles("skills").is_empty());
    assert_eq!(titles("memory/skills"), ["sourdough"]);
    assert!(index.search("rye", None, 10).unwrap().is_empty());
    let found: Vec<String> = index
        .search("starter", None, 10)
        .unwrap()
        .into_iter()
        .map(|h| h.path)
        .collect();
    assert_eq!(found, ["memory/skills/sourdough/SKILL.md"]);
}

/// What the agent works on (`work/`) is never memory, however well it
/// matches; the same words in `memory/` are found.
#[test]
fn a_file_in_work_is_never_found_one_in_memory_is() {
    let root = memory("work");
    write(
        &root,
        "work/draft.md",
        "# Draft\n\nThe quarterly lighthouse budget.\n",
    );
    write(
        &root,
        "memory/budget.md",
        "# Budget\n\nThe quarterly lighthouse budget.\n",
    );
    let mut index = Index::open(&root, None).unwrap();
    assert_eq!(index.refresh().unwrap().total, 4);
    let found: Vec<String> = index
        .search("lighthouse budget", None, 10)
        .unwrap()
        .into_iter()
        .map(|h| h.path)
        .collect();
    assert_eq!(found, ["memory/budget.md"]);
    assert!(
        index
            .read("work/draft.md", 1, None, None)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        index
            .list(
                &Filter {
                    under: Some("work".into()),
                    ..Filter::default()
                },
                50
            )
            .unwrap()
            .total,
        0
    );
}

#[test]
fn nothing_outside_the_folder_is_reached() {
    let root = memory("outside");
    let outside = folder("outside-secret");
    write(&outside, "secret.md", "The password is swordfish.\n");
    std::os::unix::fs::symlink(outside.join("secret.md"), root.join("memory/link.md")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("memory/linked")).unwrap();
    write(&root, "memory/.hidden/secret.md", "swordfish\n");
    write(&root, "memory/.env.md", "swordfish\n");
    write(&root, "memory/photo.png", "swordfish\n");
    write(
        &root,
        "memory/big.md",
        &"swordfish ".repeat(SMALL as usize / 10 + 1),
    );

    let mut index = Index::open(&root, None).unwrap();
    // Its largest file, as a spec may choose it.
    index
        .spend(link_memory_tool::opinions::Opinions {
            file: SMALL,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(index.refresh().unwrap().total, 3);
    assert!(index.search("swordfish", None, 10).unwrap().is_empty());
    for path in [
        "memory/link.md",
        "memory/linked/secret.md",
        "memory/.hidden/secret.md",
        "../lnk-memory-outside-secret-0/secret.md",
        "memory/../../lnk-memory-outside-secret-0/secret.md",
        "/etc/passwd",
        &outside.join("secret.md").display().to_string(),
    ] {
        assert!(index.read(path, 1, None, None).unwrap().is_none(), "{path}");
    }
}

/// A note or a folder swapped for a link while memory walks and reads:
/// the link is never followed, whenever the swap lands.
#[test]
fn a_link_swapped_in_meanwhile_is_never_followed() {
    let root = memory("swapped");
    let outside = folder("swapped-secret");
    write(&outside, "secret.md", "The password is swordfish.\n");
    std::fs::create_dir_all(root.join("memory/sub")).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let swapper = {
        let (root, outside, stop) = (root.to_path_buf(), outside.to_path_buf(), stop.clone());
        std::thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                n += 1;
                // A note, then a link to the secret, each put in place whole.
                std::fs::write(root.join("memory/.note"), format!("# Note {n}\nplain\n")).unwrap();
                std::fs::rename(root.join("memory/.note"), root.join("memory/notes.md")).unwrap();
                let _ = std::fs::remove_file(root.join("memory/.link"));
                std::os::unix::fs::symlink(outside.join("secret.md"), root.join("memory/.link"))
                    .unwrap();
                std::fs::rename(root.join("memory/.link"), root.join("memory/notes.md")).unwrap();
                // A folder, then a link to the folder outside.
                let _ = std::fs::remove_file(root.join("memory/.dir"));
                std::os::unix::fs::symlink(&outside, root.join("memory/.dir")).unwrap();
                let _ = std::fs::remove_file(root.join("memory/sub"));
                let _ = std::fs::remove_dir_all(root.join("memory/sub"));
                std::fs::create_dir(root.join("memory/sub")).unwrap();
                write(&root, "memory/sub/secret.md", &format!("plain {n}\n"));
                let _ = std::fs::remove_dir_all(root.join("memory/sub"));
                std::fs::rename(root.join("memory/.dir"), root.join("memory/sub")).unwrap();
            }
        })
    };
    let mut index = Index::open(&root, None).unwrap();
    let started = std::time::Instant::now();
    let mut leaked = None;
    while started.elapsed() < std::time::Duration::from_secs(3) {
        let _ = index.refresh();
        for path in ["memory/notes.md", "memory/sub/secret.md"] {
            if let Ok(Some(found)) = index.read(path, 1, None, None)
                && format!("{found:?}").contains("swordfish")
            {
                leaked = Some(path);
            }
        }
        if !index.search("swordfish", None, 10).unwrap().is_empty() {
            leaked = Some("search");
        }
        if leaked.is_some() {
            break;
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    swapper.join().unwrap();
    assert_eq!(leaked, None, "memory read through a link");
}

/// Sets `path`'s modification time, as a copy that keeps it would.
fn touch(root: &Path, path: &str, date: &str) {
    let secs = parse_since(date).unwrap() as u64;
    let file = std::fs::File::options()
        .write(true)
        .open(root.join(path))
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

/// A filter of what changed after `date`.
fn since(date: &str) -> Filter {
    Filter {
        since: Some(parse_since(date).unwrap()),
        ..Filter::default()
    }
}

#[test]
fn search_and_list_take_a_folder_and_a_time() {
    let root = memory("filters");
    for path in [
        "memory/notes/garden.md",
        "memory/notes/taxes.txt",
        "chats/telegram-42/conversation.md",
    ] {
        touch(&root, path, "2020-01-01");
    }
    write(
        &root,
        "memory/diary/may.md",
        "The tomatoes went in on May 3.\n",
    );
    touch(&root, "memory/diary/may.md", "2026-05-03");
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();

    let paths = |hits: Vec<link_memory_tool::Hit>| -> Vec<String> {
        let mut paths: Vec<String> = hits.into_iter().map(|h| h.path).collect();
        paths.sort();
        paths
    };
    // Under a folder: only what's in it.
    let under = |folder: &str| Filter {
        under: Some(folder.into()),
        ..Filter::default()
    };
    assert_eq!(
        paths(index.search("tomatoes", under("memory/notes"), 10).unwrap()),
        ["memory/notes/garden.md"]
    );
    assert_eq!(
        paths(
            index
                .search("tomatoes", under("memory/diary/"), 10)
                .unwrap()
        ),
        ["memory/diary/may.md"]
    );
    assert!(
        index
            .search("tomatoes", under("memory/note"), 10)
            .unwrap()
            .is_empty()
    );
    // Changed after a time: only what's newer.
    assert_eq!(
        paths(index.search("tomatoes", since("2026-01-01"), 10).unwrap()),
        ["memory/diary/may.md"]
    );
    assert_eq!(
        paths(index.search("tomatoes", since("2019-12-31"), 10).unwrap()).len(),
        3
    );
    assert_eq!(index.list(&since("2026-01-01"), 50).unwrap().total, 1);
    // Both, and a kind: what all three let through.
    let all = Filter {
        kind: Some(Kind::Conversation),
        under: Some("chats".into()),
        since: Some(parse_since("2019-12-31").unwrap()),
    };
    assert_eq!(
        paths(index.search("tomatoes", all, 10).unwrap()),
        ["chats/telegram-42/conversation.md"]
    );
}

#[test]
fn list_since_says_what_is_new_changed_and_removed() {
    let root = memory("since");
    for path in [
        "memory/notes/garden.md",
        "memory/notes/taxes.txt",
        "chats/telegram-42/conversation.md",
    ] {
        touch(&root, path, "2020-01-01");
    }
    let kept = folder("since-file");
    let file = kept.join("index.sqlite");
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    // What was there before memory isn't new, whenever memory first saw it.
    let before = index.list(&since("2021-01-01"), 50).unwrap();
    assert_eq!(before.total, 0);
    assert!(before.removed.unwrap().is_empty());

    // Copied in keeping an old time: new all the same. Edited: changed.
    // Deleted: removed.
    write(
        &root,
        "memory/notes/roses.md",
        "# Roses\n\nPrune in March.\n",
    );
    touch(&root, "memory/notes/roses.md", "2019-06-01");
    write(
        &root,
        "memory/notes/garden.md",
        "# The garden\n\nTomatoes in May.\n",
    );
    std::fs::remove_file(root.join("memory/notes/taxes.txt")).unwrap();
    drop(index);
    // Another session, over the same index.
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let listing = index.list(&since("2021-01-01"), 50).unwrap();
    let mut changes: Vec<(&str, Option<&str>)> = listing
        .documents
        .iter()
        .map(|e| (e.path.as_str(), e.change))
        .collect();
    changes.sort();
    assert_eq!(
        changes,
        [
            ("memory/notes/garden.md", Some("changed")),
            ("memory/notes/roses.md", Some("new"))
        ]
    );
    let removed = listing.removed.unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].path, "memory/notes/taxes.txt");
    assert_eq!(removed[0].kind, Kind::File);
    // Removed after a later time: nothing.
    let later = index.list(&since("2999-01-01"), 50).unwrap();
    assert!(later.removed.unwrap().is_empty());

    // Back again: new, and no longer removed.
    write(
        &root,
        "memory/notes/taxes.txt",
        "Receipts in the red folder now.\n",
    );
    index.refresh().unwrap();
    let listing = index.list(&since("2021-01-01"), 50).unwrap();
    assert!(listing.removed.unwrap().is_empty());
    assert!(
        listing
            .documents
            .iter()
            .any(|e| e.path == "memory/notes/taxes.txt" && e.change == Some("new"))
    );
}
#[test]
fn a_read_stops_at_a_size_and_says_where_to_carry_on() {
    let root = folder("tokens");
    let body: String = (1..=100)
        .map(|n| format!("Line {n} of the long note, about the garden.\n"))
        .collect();
    write(&root, "memory/long.md", &body);
    write(&root, "memory/one-line.txt", &"x".repeat(4000));
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();

    // Read in pieces, each within the size, until nothing's left: the
    // whole document, every line once.
    let (mut from, mut pieces, mut read) = (1, 0, Vec::new());
    loop {
        let piece = index
            .read("memory/long.md", from, None, Some(100))
            .unwrap()
            .unwrap();
        assert!(
            link_memory_tool::tokens(&piece.text) <= 100,
            "{}",
            piece.text
        );
        assert_eq!(piece.lines, 100);
        read.push(piece.text);
        pieces += 1;
        match piece.next {
            Some(next) => from = next,
            None => break,
        }
    }
    assert!(pieces > 5, "{pieces}");
    assert_eq!(read.join("\n"), body.trim_end());

    // `lines` still counts: stopped by it, there's no `next`.
    let some = index
        .read("memory/long.md", 1, Some(3), Some(1000))
        .unwrap()
        .unwrap();
    assert_eq!(some.text.lines().count(), 3);
    assert_eq!(some.next, None);
    // Whole, without a size.
    assert_eq!(
        index
            .read("memory/long.md", 1, None, None)
            .unwrap()
            .unwrap()
            .next,
        None
    );

    // A line longer than the size is cut, and the next line is next.
    let cut = index
        .read("memory/one-line.txt", 1, None, Some(50))
        .unwrap()
        .unwrap();
    assert_eq!(cut.text.len(), 200);
    assert!(cut.cut);
    assert_eq!(cut.next, None);
    assert!(
        !index
            .read("memory/long.md", 1, None, Some(100))
            .unwrap()
            .unwrap()
            .cut
    );
}

#[test]
fn links_go_both_ways_inside_the_folder() {
    let root = folder("links");
    write(
        &root,
        "memory/notes/today.md",
        "# Today\n\nCall [Maya](people/maya.md) about [[Party Plans]].\n\
         Also [[Maya]], [the web](https://example.com) and [gone](gone.md).\n\
         [out](../../../etc/passwd)\n",
    );
    write(
        &root,
        "memory/notes/people/maya.md",
        "# Maya\n\nSee [[today]].\n",
    );
    write(
        &root,
        "memory/notes/Party Plans.md",
        "# Party\n\nSaturday.\n",
    );
    // A deeper document of the same name: the shorter path is the one.
    write(&root, "memory/archive/2020/Party Plans.md", "# Old party\n");
    write(
        &root,
        "chats/t1/conversation.md",
        "About [the plans](../../memory/notes/Party%20Plans.md).\n",
    );
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();

    let today = index.links("memory/notes/today.md").unwrap().unwrap();
    let to: Vec<(&str, usize)> = today.to.iter().map(|l| (l.path.as_str(), l.line)).collect();
    assert_eq!(
        to,
        [
            ("memory/notes/Party Plans.md", 3),
            ("memory/notes/people/maya.md", 3),
            ("memory/notes/people/maya.md", 4),
        ]
    );
    let missing: Vec<&str> = today.missing.iter().map(|m| m.target.as_str()).collect();
    assert_eq!(missing, ["memory/notes/gone.md"]);
    assert_eq!(today.from.len(), 1);
    assert_eq!(today.from[0].path, "memory/notes/people/maya.md");

    let plans = index.links("memory/notes/Party Plans.md").unwrap().unwrap();
    let from: Vec<(&str, &str)> = plans
        .from
        .iter()
        .map(|l| (l.path.as_str(), l.kind.as_str()))
        .collect();
    assert_eq!(
        from,
        [
            ("chats/t1/conversation.md", "conversation"),
            ("memory/notes/today.md", "file"),
        ]
    );
    // The deeper one isn't what `[[Party Plans]]` finds.
    assert!(
        index
            .links("memory/archive/2020/Party Plans.md")
            .unwrap()
            .unwrap()
            .from
            .is_empty()
    );

    // Written anew: its links anew. Removed: no longer linking.
    write(
        &root,
        "memory/notes/today.md",
        "# Today\n\nNothing linked.\n",
    );
    std::fs::remove_file(root.join("chats/t1/conversation.md")).unwrap();
    index.refresh().unwrap();
    assert!(
        index
            .links("memory/notes/today.md")
            .unwrap()
            .unwrap()
            .to
            .is_empty()
    );
    assert!(
        index
            .links("memory/notes/Party Plans.md")
            .unwrap()
            .unwrap()
            .from
            .is_empty()
    );
    assert!(index.links("memory/nothing.md").unwrap().is_none());
}

/// A hub of many links: the first 200 found and the first 200 missing,
/// a wiki name found whatever its letters.
#[test]
fn a_hub_gives_the_first_of_each() {
    let root = folder("hub");
    for n in 0..300 {
        write(
            &root,
            &format!("memory/notes/note-{n}.md"),
            &format!("# Note {n}\n"),
        );
    }
    write(&root, "memory/notes/Über.md", "# Über\n");
    let found: String = (0..300).map(|n| format!("[[note-{n}]]\n")).collect();
    let missing: String = (0..700).map(|n| format!("[[missing-{n}]]\n")).collect();
    write(
        &root,
        "memory/hub.md",
        &format!("[[über]]\n{found}{missing}"),
    );
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let hub = index.links("memory/hub.md").unwrap().unwrap();
    assert_eq!((hub.to.len(), hub.missing.len()), (200, 200));
    assert_eq!(hub.to[0].path, "memory/notes/Über.md");
    assert_eq!(hub.to[1].path, "memory/notes/note-0.md");
    assert_eq!(hub.missing[0].target, "missing-0");
    assert_eq!(hub.missing[199].target, "missing-199");
}
