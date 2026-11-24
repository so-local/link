//! The log: its events numbered and in order, one file, read from the end.

use super::*;

/// Events are numbered one after another: a planted line out of
/// order is refused, as is a number past the next, the largest, or 0.
#[test]
fn events_are_numbered_one_after_another() {
    let (root, agents) = world("numbers");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    store.append(&main, &thread, &said(1, "Hi"), &[]).unwrap();
    let folder = root.join("main/own/chats/telegram-42");
    let log = std::fs::read(folder.join(LOG)).unwrap();
    let mut planted = log.clone();
    planted.extend(serde_json::to_vec(&said(u64::MAX, "planted")).unwrap());
    planted.push(b'\n');
    std::fs::write(folder.join(LOG), &planted).unwrap();
    let err = store.read(&main, &thread).unwrap_err();
    assert!(format!("{err:#}").contains("out of order"), "{err:#}");
    assert!(store.append(&main, &thread, &said(2, "next"), &[]).is_err());
    std::fs::write(folder.join(LOG), &log).unwrap();
    let err = store
        .append(&main, &thread, &said(3, "past"), &[])
        .unwrap_err();
    assert!(err.to_string().contains("2 is the next number"), "{err}");
    let err = store
        .append(&main, &thread, &said(SEQ_MAX + 1, "past"), &[])
        .unwrap_err();
    assert!(err.to_string().contains("at most"), "{err}");
    assert!(store.append(&main, &thread, &said(0, "zero"), &[]).is_err());
    let events = store.read(&main, &thread).unwrap();
    assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [1]);
    let _ = std::fs::remove_dir_all(&root);
}

/// A conversation is one file, however long, a line an event: a
/// small event takes a line's bytes, not a file's block.
#[test]
fn a_conversation_is_one_file() {
    let (root, agents) = world("one-file");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    for seq in 1..=100 {
        store.append(&main, &thread, &said(seq, "ok"), &[]).unwrap();
    }
    let folder = root.join("main/own/chats/telegram-42");
    let mut names: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, [MARKDOWN, LOG]);
    let log = std::fs::read_to_string(folder.join(LOG)).unwrap();
    assert_eq!(log.lines().count(), 100);
    assert!(log.starts_with(r#"{"seq":1,"#), "{log}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A line cut short at the log's end, by a writer that stopped midway,
/// isn't an event: it's left out, and taken off by the next writer.
#[test]
fn a_line_cut_short_is_no_event() {
    let (root, agents) = world("cut");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    store.append(&main, &thread, &said(1, "Hi"), &[]).unwrap();
    let path = root.join("main/own/chats/telegram-42").join(LOG);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(br#"{"seq":2,"role":"us"#).unwrap();
    assert_eq!(store.read(&main, &thread).unwrap().len(), 1);
    store
        .append(&main, &thread, &said(2, "again"), &[])
        .unwrap();
    let events = store.read(&main, &thread).unwrap();
    assert_eq!(
        events.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
        ["Hi", "again"]
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// A read from a number reads the log from that line on, found from
/// the end back, not from its first: a line at its head that isn't an
/// event fails a whole read, and not one of the end.
#[test]
fn a_read_from_a_number_reads_only_from_there() {
    let (root, agents) = world("tail");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    // Lines of many lengths, so line ends and heads fall across the
    // chunks it reads back in.
    for seq in 1..=400 {
        let text = "x".repeat((seq as usize * 977) % 3000);
        store
            .append(&main, &thread, &said(seq, &text), &[])
            .unwrap();
    }
    let whole = store.read(&main, &thread).unwrap();
    for from in [2, 3, 63, 64, 65, 199, 300, 399, 400] {
        let events = store.read_from(&main, &thread, from).unwrap();
        assert_eq!(events, whole[from as usize - 1..], "{from}");
    }
    assert!(store.read_from(&main, &thread, 401).unwrap().is_empty());
    // A line cut short at the end is no event yet, wherever it's cut.
    let log = root.join("main/own/chats/telegram-42").join(LOG);
    let kept = std::fs::read(&log).unwrap();
    for cut in [r#"{"se"#, r#"{"seq":401,"ro"#] {
        std::fs::write(&log, [&kept[..], cut.as_bytes()].concat()).unwrap();
        assert_eq!(store.read_from(&main, &thread, 400).unwrap(), whole[399..]);
        assert!(store.read_from(&main, &thread, 401).unwrap().is_empty());
    }
    // Its head spoiled: only a read that reaches it fails.
    let mut spoiled = b"not an event\n".to_vec();
    spoiled.extend_from_slice(&kept);
    std::fs::write(&log, spoiled).unwrap();
    assert!(store.read(&main, &thread).is_err());
    assert_eq!(store.read_from(&main, &thread, 390).unwrap(), whole[389..]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_conversation_is_its_events_in_order() {
    let (root, agents) = world("order");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    assert!(store.read(&main, &thread).unwrap().is_empty());
    for seq in 1..=11 {
        store
            .append(&main, &thread, &said(seq, &format!("m{seq}")), &[])
            .unwrap();
    }
    let events = store.read(&main, &thread).unwrap();
    let texts: Vec<_> = events.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(texts.first(), Some(&"m1"));
    assert_eq!(texts.last(), Some(&"m11"));
    assert_eq!(texts.len(), 11);
    let folder = root.join("main/own/chats/telegram-42");
    let log = std::fs::read_to_string(folder.join(LOG)).unwrap();
    assert_eq!(log.lines().count(), 11);
    // Never rewritten: a number taken is refused, and the event kept.
    let err = store
        .append(&main, &thread, &said(3, "again"), &[])
        .unwrap_err();
    assert!(err.to_string().contains("another turn came first"), "{err}");
    assert_eq!(store.read(&main, &thread).unwrap()[2].text, "m3");
    // The copy for people follows the events, once they're answered.
    let md = std::fs::read_to_string(folder.join(MARKDOWN)).unwrap();
    assert!(!md.contains("m11"), "{md}");
    let answer = Event {
        seq: 12,
        role: event::ASSISTANT.into(),
        text: "a12".into(),
        ..Event::default()
    };
    store.append(&main, &thread, &answer, &[]).unwrap();
    let md = std::fs::read_to_string(folder.join(MARKDOWN)).unwrap();
    assert!(md.starts_with("# telegram:42\n"), "{md}");
    assert!(md.contains("\n\nm11\n"), "{md}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A store keeps what it read of a log, so the next read parses only the
/// lines added since; a log put in its place, or written again where it
/// is, is read again whole. A line starts with its number, evidence and
/// all, so a read from a number finds its line from the end back.
#[test]
fn a_read_parses_only_what_was_added_since() {
    use link_harness_wire::evidence::{CallRef, Captured, Evidence, McpDispatch, Record};
    let (root, agents) = world("added");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    for seq in 1..=50 {
        let text = "x".repeat(seq as usize * 37 % 500);
        store
            .append(&main, &thread, &said(seq, &text), &[])
            .unwrap();
    }
    let log = root.join("main/own/chats/telegram-42").join(LOG);
    let length = || std::fs::metadata(&log).unwrap().len();
    let parsed = |read: &dyn Fn() -> Vec<Event>| {
        let at = store.parsed();
        let events = read();
        (events, store.parsed() - at)
    };
    let (events, read) = parsed(&|| store.read(&main, &thread).unwrap());
    assert_eq!((events.len(), read), (50, length()));
    // Read again, from any number it holds: nothing parsed.
    let (again, read) = parsed(&|| store.read(&main, &thread).unwrap());
    assert_eq!((&again, read), (&events, 0));
    let (tail, read) = parsed(&|| store.read_from(&main, &thread, 40).unwrap());
    assert_eq!((&tail[..], read), (&events[39..], 0));
    // One more, with evidence: only its line parsed.
    let marker = Event {
        seq: 51,
        role: event::EVIDENCE.into(),
        evidence: Some(
            Evidence::new(Record::Dispatched {
                call: CallRef {
                    proposal: 1,
                    ordinal: 0,
                },
                offset_ns: 0,
                request: McpDispatch {
                    server: "fixture".into(),
                    operation: "read".into(),
                    arguments: Captured::Retained {
                        value: serde_json::json!({}),
                    },
                },
            })
            .into(),
        ),
        ..Event::default()
    };
    let before = length();
    store.append(&main, &thread, &marker, &events).unwrap();
    let line = std::fs::read(&log).unwrap()[before as usize..].to_vec();
    assert!(line.starts_with(br#"{"seq":51,"#), "{line:?}");
    let (newer, read) = parsed(&|| store.read_from(&main, &thread, 45).unwrap());
    assert_eq!(newer.last(), Some(&marker));
    assert_eq!((newer.len(), read), (7, line.len() as u64));
    // Another log put in its place, then one as long written again where
    // it is: each read whole, as it is now.
    for text in ["y", "z"] {
        let other: Vec<Event> = (1..=51).map(|seq| said(seq, text)).collect();
        let bytes: Vec<u8> = other
            .iter()
            .flat_map(|e| link_files::saved::line(e).unwrap())
            .collect();
        if text == "y" {
            let aside = log.with_extension("new");
            std::fs::write(&aside, &bytes).unwrap();
            std::fs::rename(&aside, &log).unwrap();
        } else {
            std::fs::write(&log, &bytes).unwrap();
        }
        assert_eq!(store.read(&main, &thread).unwrap(), other, "{text}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A message its source sends again is found by its delivery or its
/// sender, the first that said it, reading only the lines added since
/// the last time it was asked; a log put in its place is read again.
#[test]
fn a_repeated_message_is_found_reading_only_what_was_added() {
    use link_interactions::TransportEventId;
    let (root, agents) = world("repeats");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("task-1").unwrap();
    let delivered = |seq: u64, id: &str| Event {
        delivery: Some(TransportEventId::new(id).unwrap()),
        from: Some(format!("task:{}", seq % 3)),
        ..said(seq, &format!("message {seq}"))
    };
    for seq in 1..=200 {
        store
            .append(&main, &thread, &delivered(seq, &format!("d-{seq}")), &[])
            .unwrap();
    }
    let delivery = |id: &str| Repeat::Delivery(TransportEventId::new(id).unwrap());
    let found = store
        .first_of(&main, &thread, &delivery("d-7"))
        .unwrap()
        .unwrap();
    assert_eq!((found.seq, found.text.as_str()), (7, "message 7"));
    let from = store
        .first_of(&main, &thread, &Repeat::From("task:2".into()))
        .unwrap();
    assert_eq!(from.map(|e| e.seq), Some(2));
    assert!(
        store
            .first_of(&main, &thread, &delivery("d-999"))
            .unwrap()
            .is_none()
    );
    let read = store.firsts.read();
    for seq in 201..=210 {
        store
            .append(&main, &thread, &delivered(seq, &format!("d-{seq}")), &[])
            .unwrap();
    }
    let found = store
        .first_of(&main, &thread, &delivery("d-205"))
        .unwrap()
        .unwrap();
    assert_eq!(found.seq, 205);
    let log = root.join("main/own/chats/task-1").join(LOG);
    let added = std::fs::read(&log).unwrap().len() as u64 - read;
    assert_eq!(
        store.firsts.read() - read,
        added,
        "it read more than the lines added"
    );
    // A log begun again in its place is read again.
    std::fs::remove_file(&log).unwrap();
    store
        .append(&main, &thread, &delivered(1, "d-new"), &[])
        .unwrap();
    assert!(
        store
            .first_of(&main, &thread, &delivery("d-7"))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .first_of(&main, &thread, &delivery("d-new"))
            .unwrap()
            .map(|e| e.seq),
        Some(1)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// What a store keeps of a log it read follows the latest window mark,
/// not the log's length: the events before the first a turn reads by it
/// are let go.
#[test]
fn what_a_store_keeps_of_a_log_follows_its_window() {
    let (root, agents) = world("kept-window");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    for seq in 1..=1000 {
        let mut event = said(seq, "words");
        if seq % 2 == 0 {
            event.role = event::ASSISTANT.into();
            event.window = Some(crate::window::Mark {
                seq: seq.saturating_sub(40).max(1),
                answered: seq,
                ..Default::default()
            });
        }
        store.append(&main, &thread, &event, &[]).unwrap();
    }
    // Read from the start, as a process does before it knows a mark.
    assert_eq!(store.read(&main, &thread).unwrap().len(), 1000);
    let held = store.logs.held();
    assert!(held <= 41, "it keeps {held} events of 1000, its window 41");
    // Reading on from the window takes nothing from the disk again.
    let parsed = store.parsed();
    assert_eq!(store.read_from(&main, &thread, 960).unwrap().len(), 41);
    assert_eq!(store.parsed(), parsed);
    let _ = std::fs::remove_dir_all(&root);
}
