//! A conversation's folder: its own files only, never through a link.

use super::*;

/// Where an agent's conversations are kept off this machine, nothing
/// of one is written to its disk: a turn asked to end, and an answer
/// of a crashed turn, are held in memory, and the tools it was
/// narrowed to are the store's alone. Its store unreachable, its tools
/// are an error, never every tool.
#[test]
fn a_conversation_kept_off_the_machine_leaves_nothing_on_its_disk() {
    use link_store_contract::storage::{Collection, Place, Places, StorageAt};
    let (root, agents) = world("off");
    let (main, other) = (Agent::new("main").unwrap(), Agent::new("other").unwrap());
    let places = Places(
        [(
            Collection::Conversations,
            Place {
                place: "nowhere".into(),
                overridden: true,
            },
        )]
        .into(),
    );
    let config = Config {
        endpoint_key: Some("key".to_string().into()),
        storage: Some(StorageAt {
            places,
            socket: Some(root.join("storage.sock")),
        }),
        ..Config::default()
    };
    let store = Files::new(agents).holding(&main, config);
    let thread = Thread::new("task:aaaaaaaaaaaa").unwrap();
    let another = Thread::new("telegram:42").unwrap();

    store.interrupt_turn(&main, &thread).unwrap();
    assert!(store.interrupted(&main, &thread));
    assert!(!store.interrupted(&main, &another));
    assert!(!store.interrupted(&other, &thread));

    store.set_recovered(&main, &thread, 3).unwrap();
    assert_eq!(store.recovered(&main), [(thread.clone(), 3)]);
    store.drop_recovered(&main, &thread, 2).unwrap();
    assert_eq!(store.recovered(&main), [(thread.clone(), 3)]);
    assert_eq!(store.take_recovered(&main), [(thread.clone(), 3)]);
    assert!(store.take_recovered(&main).is_empty());
    store.set_recovered(&main, &thread, 4).unwrap();
    store.drop_recovered(&main, &thread, 4).unwrap();
    assert!(store.recovered(&main).is_empty());

    // A store bridge that answers nothing: refused, not widened. (One
    // not there yet is waited for, as it starts beside the harness.)
    std::fs::create_dir_all(&root).unwrap();
    let bridge = std::os::unix::net::UnixListener::bind(root.join("storage.sock")).unwrap();
    std::thread::spawn(move || {
        for call in bridge.incoming() {
            drop(call);
        }
    });
    assert!(store.tools(&main, &thread).is_err());
    // Settings that can't be read: never guessed as on this machine.
    std::fs::create_dir_all(root.join("other/home")).unwrap();
    std::fs::write(root.join("other/home/config.json"), "not settings").unwrap();
    assert!(store.tools(&other, &thread).is_err());
    store.interrupt_turn(&other, &thread).unwrap();
    assert!(!root.join("other/files").exists());
    let tools = ["memory__search".to_string()];
    assert!(store.narrow_tools(&main, &thread, &tools).is_err());

    assert!(
        !root.join("main").exists(),
        "{:?}",
        std::fs::read_dir(&root).map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>())
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Nothing planted in a conversation's folder is followed, waited
/// on, or read past its size: a link, a pipe, a huge file.
#[test]
fn a_conversation_reads_only_its_own_files() {
    let (root, agents) = world("planted");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    store.append(&main, &thread, &said(1, "Hi"), &[]).unwrap();
    let folder = root.join("main/own/chats/telegram-42");
    let elsewhere = root.join("elsewhere.json");
    std::fs::write(&elsewhere, serde_json::to_vec(&said(1, "Hi")).unwrap()).unwrap();
    let log = folder.join(LOG);
    let kept = root.join("kept.jsonl");
    std::fs::rename(&log, &kept).unwrap();

    std::os::unix::fs::symlink(&kept, &log).unwrap();
    let err = store.read(&main, &thread).unwrap_err();
    assert!(format!("{err:#}").contains("a link"), "{err:#}");
    // Read on from a number, or written, too.
    let err = store.read_from(&main, &thread, 1).unwrap_err();
    assert!(format!("{err:#}").contains("a link"), "{err:#}");
    let err = store
        .append(&main, &thread, &said(2, "x"), &[])
        .unwrap_err();
    assert!(format!("{err:#}").contains("a link"), "{err:#}");
    assert_eq!(std::fs::read_to_string(&kept).unwrap().lines().count(), 1);
    std::fs::remove_file(&log).unwrap();

    fifo(&log);
    let err = store.read(&main, &thread).unwrap_err();
    assert!(format!("{err:#}").contains("not a regular file"), "{err:#}");
    let err = store
        .append(&main, &thread, &said(2, "x"), &[])
        .unwrap_err();
    assert!(format!("{err:#}").contains("not a regular file"), "{err:#}");
    std::fs::remove_file(&log).unwrap();

    let huge = std::fs::File::create(&log).unwrap();
    huge.set_len(EVENT_MAX + 2).unwrap();
    let err = store.read(&main, &thread).unwrap_err();
    assert!(format!("{err:#}").contains("larger than"), "{err:#}");
    let err = store
        .append(&main, &thread, &said(2, "x"), &[])
        .unwrap_err();
    assert!(format!("{err:#}").contains("larger than"), "{err:#}");
    std::fs::rename(&kept, &log).unwrap();
    let mut line = serde_json::to_vec(&said(1, "Hi")).unwrap();
    line.push(b'\n');
    std::fs::write(&log, line).unwrap();

    // A link for the copy is replaced, not written through.
    std::fs::remove_file(folder.join(MARKDOWN)).unwrap();
    std::os::unix::fs::symlink(&elsewhere, folder.join(MARKDOWN)).unwrap();
    let answer = Event {
        seq: 2,
        role: event::ASSISTANT.into(),
        text: "Hello".into(),
        ..Event::default()
    };
    store.append(&main, &thread, &answer, &[]).unwrap();
    assert!(
        !std::fs::symlink_metadata(folder.join(MARKDOWN))
            .unwrap()
            .is_symlink()
    );
    assert!(
        std::fs::read_to_string(folder.join(MARKDOWN))
            .unwrap()
            .contains("Hello")
    );
    let copied: Event = serde_json::from_slice(&std::fs::read(&elsewhere).unwrap()).unwrap();
    assert_eq!(copied.text, "Hi");
    let _ = std::fs::remove_dir_all(&root);
}

/// A conversation's folder that's a link is refused, and what it
/// points to is left as it was.
#[test]
fn a_linked_conversation_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let (root, agents) = world("linked");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    let conversations = root.join("main/own/chats");
    std::fs::create_dir_all(&conversations).unwrap();
    std::os::unix::fs::symlink(&target, conversations.join("telegram-42")).unwrap();
    let err = store
        .append(&main, &thread, &said(1, "Hi"), &[])
        .unwrap_err();
    assert!(err.to_string().contains("is a link"), "{err}");
    assert!(store.read(&main, &thread).is_err());
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    let mode = std::fs::metadata(&target).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    // The folder of every conversation, too.
    std::fs::remove_file(conversations.join("telegram-42")).unwrap();
    std::fs::remove_dir(&conversations).unwrap();
    std::os::unix::fs::symlink(&target, &conversations).unwrap();
    assert!(store.append(&main, &thread, &said(1, "Hi"), &[]).is_err());
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    // And the agent's own folder, opened by its path alone (the folder
    // it's in may be one a sandbox doesn't give): its last part a link.
    let own = root.join("main/own");
    std::fs::remove_dir_all(&own).unwrap();
    std::os::unix::fs::symlink(&target, &own).unwrap();
    let err = store
        .append(&main, &thread, &said(1, "Hi"), &[])
        .unwrap_err();
    assert!(err.to_string().contains("is a link"), "{err}");
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&root);
}

/// A folder on the way swapped for a link (by a `run` call), the
/// conversation's or the folder of every conversation: a read or a
/// write after the swap is refused, and one through a folder held
/// before it stays in that folder, never reaching the link's target.
#[test]
fn a_folder_swapped_for_a_link_is_never_followed() {
    let (root, agents) = world("swapped");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    let target = root.join("target");
    let empty = |target: &Path| std::fs::read_dir(target).unwrap().count() == 0;
    let conversations = root.join("main/own/chats");
    for swapped in [conversations.join("telegram-42"), conversations.clone()] {
        let _ = std::fs::remove_dir_all(&conversations);
        std::fs::create_dir_all(&target).unwrap();
        store.append(&main, &thread, &said(1, "Hi"), &[]).unwrap();
        let held = store.folder(&main, &thread, false).unwrap().unwrap();
        let moved = root.join("moved");
        std::fs::rename(&swapped, &moved).unwrap();
        std::os::unix::fs::symlink(&target, &swapped).unwrap();
        // Held before: written where it was, never through the link.
        held.write(TOOLS, b"[]").unwrap();
        assert!(empty(&target));
        let inside = moved.join(match swapped == conversations {
            true => "telegram-42/tools.json",
            false => "tools.json",
        });
        assert_eq!(std::fs::read(inside).unwrap(), b"[]");
        // Opened after: refused, read or written.
        let err = store.read(&main, &thread).unwrap_err();
        assert!(err.to_string().contains("is a link"), "{err}");
        assert!(store.append(&main, &thread, &said(2, "x"), &[]).is_err());
        assert!(store.last_events(&main, &thread, 4).is_err());
        assert!(store.tools(&main, &thread).is_err());
        let tools = ["memory__search".to_string()];
        assert!(store.narrow_tools(&main, &thread, &tools).is_err());
        assert!(store.take_turn(&main, &thread, "a").is_err());
        assert!(store.interrupt_turn(&main, &thread).is_err());
        assert!(store.set_recovered(&main, &thread, 1).is_err());
        assert!(!store.turn_running(&main, &thread));
        assert!(!store.interrupted(&main, &thread));
        assert!(store.recovered(&main).is_empty());
        assert!(store.lapsed_turns(&main).is_empty());
        assert!(empty(&target));
        std::fs::remove_file(&swapped).unwrap();
        std::fs::remove_dir_all(&moved).unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// One agent's turn can't read or write another's conversation: the
/// same thread is two conversations, each in its agent's own folder,
/// and no thread names a path out of it.
#[test]
fn one_agent_cannot_reach_anothers_conversation() {
    let (root, agents) = world("apart");
    let store = Files::new(agents);
    let (main, other) = (Agent::new("main").unwrap(), Agent::new("other").unwrap());
    let thread = Thread::new("telegram:42").unwrap();
    store
        .append(&main, &thread, &said(1, "main's secret"), &[])
        .unwrap();
    assert!(store.read(&other, &thread).unwrap().is_empty());
    store
        .append(&other, &thread, &said(1, "other's"), &[])
        .unwrap();
    assert_eq!(store.read(&main, &thread).unwrap()[0].text, "main's secret");
    assert_eq!(store.read(&other, &thread).unwrap()[0].text, "other's");
    for sneaky in ["../../main/own/chats/telegram-42", "..", "a/b"] {
        assert!(Thread::new(sneaky).is_err(), "{sneaky}");
    }
    // An agent this process doesn't answer for has nothing.
    let stranger = Agent::new("stranger").unwrap();
    assert!(store.read(&stranger, &thread).is_err());
    assert!(
        store
            .append(&stranger, &thread, &said(1, "x"), &[])
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&root);
}
