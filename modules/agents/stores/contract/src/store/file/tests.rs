use super::*;

/// The folder, reached through its contract alone, as callers do.
struct Here(Folder);

impl super::super::Store for Here {
    fn name(&self) -> &str {
        "file"
    }

    fn call(&self, request: &Request) -> anyhow::Result<Reply> {
        super::super::Store::call(&self.0, request)
    }
}

fn agent() -> super::super::Agent {
    super::super::Agent {
        id: "aaaaaaaaaaaa".into(),
        owner: "me@box".into(),
    }
}

fn here(name: &str) -> (link_test_kit::TempDir, Here) {
    let dir = link_test_kit::TempDir::new(name);
    let folder = Folder::at(&dir.join("agent"), "aaaaaaaaaaaa").unwrap();
    (dir, Here(folder))
}

/// A lease renewed a moment before a second turns isn't lapsed a
/// moment after it.
#[test]
fn a_lease_lapses_by_its_milliseconds_not_its_second() {
    let held = |renewed_ms: u64| Held {
        holder: "a".into(),
        thread: "t".into(),
        started: 100,
        renewed: 100,
        renewed_ms,
    };
    assert!(!lapsed(&held(100_999), 101_004, 1000));
    assert!(lapsed(&held(100_999), 101_999, 1000));
    // As it's written, then read back.
    let read: Held = serde_json::from_slice(&serde_json::to_vec(&held(100_999)).unwrap()).unwrap();
    assert_eq!(read.renewed_ms, 100_999);
}

#[test]
fn the_contract_holds_in_the_agents_folder() {
    let (_dir, here) = here("file-contract");
    super::super::check(&here, &agent());
}

#[test]
fn the_files_contract_holds_in_the_agents_folder() {
    let (_dir, here) = here("file-files");
    super::super::check_files(&here, &agent());
}

#[test]
fn the_jobs_contract_holds_in_the_agents_folder() {
    let (_dir, here) = here("file-jobs");
    super::super::check_jobs(&here, &agent());
}

#[test]
fn narrowing_holds_in_the_agents_folder() {
    let (_dir, here) = here("file-narrowing");
    super::super::check_narrowing(&here, &agent());
}

#[test]
fn a_conversation_is_a_log_and_its_copy_as_a_person_reads_them() {
    use super::super::Store as _;
    let (dir, here) = here("file-layout");
    let a = agent();
    let said = |seq: u64, role: &str, text: &str| serde_json::json!({"text": text, "role": role, "seq": seq, "at": "2026-10-12T09:00:00Z"});
    here.append(&a, "telegram:42", &said(1, "user", "Hi"))
        .unwrap();
    here.append(&a, "telegram:42", &said(2, "assistant", "Hello."))
        .unwrap();
    let folder = dir.join("agent/chats/telegram-42");
    let log = std::fs::read_to_string(folder.join(EVENTS)).unwrap();
    // Its number first, so a reader finds it at the line's head.
    assert!(log.lines().all(|l| l.starts_with("{\"seq\":")), "{log}");
    assert_eq!(log.lines().count(), 2);
    let md = std::fs::read_to_string(folder.join(MARKDOWN)).unwrap();
    assert!(md.starts_with("# telegram:42\n"), "{md}");
    assert!(md.contains("Hi") && md.contains("Hello."), "{md}");
    assert_eq!(
        here.threads(&a).unwrap(),
        [Kept {
            thread: "telegram:42".into(),
            last: 2
        }]
    );
    // A file written as a person writes it reads back as written.
    std::fs::create_dir_all(dir.join("agent/instructions")).unwrap();
    std::fs::write(dir.join("agent/instructions/instruction.md"), "Be brief.").unwrap();
    let (bytes, _) = here
        .file_read(&a, "instructions/instruction.md")
        .unwrap()
        .unwrap();
    assert_eq!(bytes, b"Be brief.");
}

#[test]
fn nothing_outside_the_agents_folder_is_reached_through_a_link() {
    use super::super::Store as _;
    let (dir, here) = here("file-links");
    let a = agent();
    let outside = dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("instruction.md"), "theirs").unwrap();
    // A collection's folder that's a link, a conversation's folder
    // that's a link, a file that's a link: none followed.
    std::os::unix::fs::symlink(&outside, dir.join("agent/instructions")).unwrap();
    assert!(here.file_read(&a, "instructions/instruction.md").is_err());
    assert!(
        here.file_write(&a, "instructions/instruction.md", b"x", 0)
            .is_err()
    );
    assert!(here.file_list(&a, "instructions/").is_err());
    std::fs::create_dir_all(dir.join("agent/chats")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("agent/chats/telegram-42")).unwrap();
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "x"});
    assert!(here.append(&a, "telegram:42", &event).is_err());
    assert!(here.tools_narrow(&a, "telegram:42", &[]).is_err());
    std::fs::create_dir_all(dir.join("agent/skills/s")).unwrap();
    std::os::unix::fs::symlink(
        outside.join("instruction.md"),
        dir.join("agent/skills/s/SKILL.md"),
    )
    .unwrap();
    assert!(here.file_read(&a, "skills/s/SKILL.md").is_err());
    assert_eq!(here.file_list(&a, "skills/").unwrap(), Vec::<String>::new());
    assert_eq!(
        std::fs::read_to_string(outside.join("instruction.md")).unwrap(),
        "theirs"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    // A thread that would name another folder.
    for odd in ["..", ".", "a/b", ".hidden"] {
        assert!(here.append(&a, odd, &event).is_err(), "{odd}");
    }
}

/// A conversation's log and copy that are hard links to a file
/// elsewhere (the same file under another name) are never written
/// through: an event added to such a log is refused, and the file
/// elsewhere is as it was.
#[test]
fn a_hard_link_is_never_written_through() {
    use super::super::Store as _;
    let (dir, here) = here("file-hard-links");
    let a = agent();
    let theirs = dir.join("theirs.txt");
    std::fs::write(&theirs, "theirs\n").unwrap();
    let conv = dir.join("agent/chats/telegram-42");
    std::fs::create_dir_all(&conv).unwrap();
    for name in [EVENTS, MARKDOWN] {
        let _ = std::fs::remove_file(conv.join(name));
        std::fs::hard_link(&theirs, conv.join(name)).unwrap();
        let event = serde_json::json!({"seq": 1, "role": "assistant", "text": "x"});
        let added = here.append(&a, "telegram:42", &event);
        // The log is added to in place: refused. The copy, when it's
        // not what was added to, is made anew aside and renamed in.
        if name == EVENTS {
            assert!(added.is_err());
        }
        assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "theirs\n");
        let _ = std::fs::remove_file(conv.join(name));
    }
}

#[test]
fn file_revisions_survive_reopening_and_observed_manual_edits() {
    use super::super::{Store as _, Written};
    let (dir, here) = here("file-revisions");
    let a = agent();
    let path = "instructions/instruction.md";
    let Written::Version(first) = here.file_write(&a, path, b"A", 0).unwrap() else {
        panic!("the new file was not written");
    };
    let reopened = Here(Folder::at(&dir.join("agent"), &a.id).unwrap());
    assert_eq!(reopened.file_read(&a, path).unwrap().unwrap().1, first);
    let mut seen = std::collections::BTreeSet::from([first]);
    for bytes in [b"B", b"A"] {
        std::fs::write(dir.join("agent").join(path), bytes).unwrap();
        let (body, version) = reopened.file_read(&a, path).unwrap().unwrap();
        assert_eq!(body, bytes);
        assert!(
            seen.insert(version),
            "manual edits must not revive a revision"
        );
        assert!(matches!(
            here.file_write(&a, path, b"stale", first).unwrap(),
            Written::Stale(_)
        ));
        assert!(!here.file_remove(&a, path, first).unwrap());
    }
    // A same-content replacement has a new inode even if a filesystem's
    // timestamp resolution cannot distinguish the two operations.
    let replacement = dir.join("replacement");
    std::fs::write(&replacement, b"A").unwrap();
    std::fs::rename(replacement, dir.join("agent").join(path)).unwrap();
    let (_, replaced) = reopened.file_read(&a, path).unwrap().unwrap();
    assert!(seen.insert(replaced));
    assert_eq!(here.file_read(&a, path).unwrap().unwrap().1, replaced);
}

#[test]
fn a_reserved_revision_is_not_reused_after_an_interrupted_write() {
    use super::super::{Store as _, Written};
    let (dir, here) = here("file-revision-interrupted");
    let a = agent();
    let path = "instructions/instruction.md";
    let Written::Version(first) = here.file_write(&a, path, b"A", 0).unwrap() else {
        panic!("the new file was not written");
    };
    // The process stopped after reserving and replacing the bytes but
    // before recording the new fingerprint. Recovery must spend a new
    // revision rather than revive either one.
    let reserved = {
        let versions = here.0.revisions().unwrap();
        let _locked = versions.lock(libc::LOCK_EX).unwrap();
        let reserved = revisions::reserve(&versions).unwrap();
        let (folder, name) = here.0.place_of(path, false).unwrap();
        folder.write(&name, b"B").unwrap();
        reserved
    };
    let reopened = Here(Folder::at(&dir.join("agent"), &a.id).unwrap());
    let (body, recovered) = reopened.file_read(&a, path).unwrap().unwrap();
    assert_eq!(body, b"B");
    assert!(recovered > reserved && reserved > first);
    assert!(matches!(
        reopened.file_write(&a, path, b"stale", first).unwrap(),
        Written::Stale(_)
    ));
    assert!(matches!(
        reopened.file_write(&a, path, b"stale", reserved).unwrap(),
        Written::Stale(_)
    ));
}

#[test]
fn exhausted_or_missing_revision_state_refuses_before_mutating_bytes() {
    use super::super::{Store as _, Written};
    let (dir, here) = here("file-revision-refusal");
    let a = agent();
    let path = "instructions/instruction.md";
    let Written::Version(first) = here.file_write(&a, path, b"A", 0).unwrap() else {
        panic!("the new file was not written");
    };
    let counter = dir.join("agent").join(state_paths()[0].join("next"));
    let mut clock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&counter).unwrap()).unwrap();
    clock["reserved"] = u64::MAX.into();
    std::fs::write(&counter, serde_json::to_vec(&clock).unwrap()).unwrap();
    assert!(here.file_write(&a, path, b"B", first).is_err());
    assert!(here.file_append(&a, path, b"B").is_err());
    assert_eq!(std::fs::read(dir.join("agent").join(path)).unwrap(), b"A");
    std::fs::remove_file(counter).unwrap();
    assert!(here.file_read(&a, path).is_err());
    assert!(here.file_write(&a, path, b"B", first).is_err());
    assert!(here.file_append(&a, path, b"B").is_err());
    assert_eq!(std::fs::read(dir.join("agent").join(path)).unwrap(), b"A");
}

#[test]
fn linked_revision_state_is_refused_before_writing_data() {
    use super::super::Store as _;
    let (dir, here) = here("file-revision-link");
    let outside = dir.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let revision_state = dir.join("agent").join(&state_paths()[0]);
    std::fs::create_dir_all(revision_state.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, revision_state).unwrap();
    assert!(
        here.file_write(&agent(), "instructions/instruction.md", b"A", 0)
            .is_err()
    );
    assert!(!dir.join("agent/instructions/instruction.md").exists());
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn newer_revision_metadata_is_refused_without_mutating_data_or_history() {
    use super::super::{Store as _, Written};
    let (dir, here) = here("file-revision-schema");
    let a = agent();
    let path = "instructions/instruction.md";
    let Written::Version(first) = here.file_write(&a, path, b"A", 0).unwrap() else {
        panic!("the new file was not written");
    };
    let state = dir.join("agent").join(&state_paths()[0]);
    for target in [state.join("next"), state.join("files").join(path)] {
        let original = std::fs::read(&target).unwrap();
        let mut future: serde_json::Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(future["schema"], 1);
        future["schema"] = 2.into();
        let future = serde_json::to_vec(&future).unwrap();
        std::fs::write(&target, &future).unwrap();
        for error in [
            here.file_read(&a, path).unwrap_err(),
            here.file_write(&a, path, b"B", first).unwrap_err(),
            here.file_append(&a, path, b"B").unwrap_err(),
            here.file_remove(&a, path, first).unwrap_err(),
        ] {
            assert!(format!("{error:#}").contains("lnk upgrade"), "{error:#}");
        }
        assert_eq!(std::fs::read(dir.join("agent").join(path)).unwrap(), b"A");
        assert_eq!(std::fs::read(&target).unwrap(), future);
        std::fs::write(target, original).unwrap();
    }
    // A removed file still has history. Recreating it must not overwrite
    // a newer history record merely because its data is absent.
    assert!(here.file_remove(&a, path, first).unwrap());
    let record = state.join("files").join(path);
    let mut future: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    future["schema"] = 2.into();
    let future = serde_json::to_vec(&future).unwrap();
    std::fs::write(&record, &future).unwrap();
    for error in [
        here.file_write(&a, path, b"B", 0).unwrap_err(),
        here.file_append(&a, path, b"B").unwrap_err(),
    ] {
        assert!(format!("{error:#}").contains("lnk upgrade"), "{error:#}");
    }
    assert!(!dir.join("agent").join(path).exists());
    assert_eq!(std::fs::read(record).unwrap(), future);
}
