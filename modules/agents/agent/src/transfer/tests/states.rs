//! A harness's state and the files folder, backed up, restored and moved:
//! what's named comes, and never through a link.

use super::*;

#[test]
fn what_stays_is_kept_and_the_rest_replaced() {
    let dir = temp("remove");
    let root = dir.join(".fake");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::create_dir_all(root.join("memory")).unwrap();
    std::fs::write(root.join("bin/fake"), "program").unwrap();
    std::fs::write(root.join("memory/old"), "stale").unwrap();
    std::fs::write(root.join("config"), "old").unwrap();
    remove_paths(&dir, std::slice::from_ref(&root), &[root.join("bin")]).unwrap();
    assert!(root.join("bin/fake").exists());
    assert!(!root.join("memory").exists() && !root.join("config").exists());

    let from = dir.join("from");
    std::fs::create_dir_all(from.join(".fake/memory")).unwrap();
    std::fs::write(from.join(".fake/memory/new"), "fresh").unwrap();
    std::fs::write(from.join(".fake/config"), "new").unwrap();
    // A link where a folder comes is replaced, not followed.
    std::os::unix::fs::symlink("/tmp", dir.join("to-link")).unwrap();
    merge(&from, &dir).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("config")).unwrap(), "new");
    assert!(root.join("memory/new").exists() && root.join("bin/fake").exists());
}

/// A folder of the files folder swapped for a link (by a process of
/// the harness that outlived `stop`) is replaced, never written
/// through.
#[test]
fn merging_files_never_follows_a_link() {
    let dir = temp("merge-links");
    let (from, files, outside) = (dir.join("from"), dir.join("files"), dir.join("outside"));
    std::fs::create_dir_all(from.join("Notes")).unwrap();
    std::fs::write(from.join("Notes/today.md"), "came").unwrap();
    std::fs::create_dir_all(&files).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, files.join("Notes")).unwrap();
    merge(&from, &files).unwrap();
    assert!(
        std::fs::symlink_metadata(files.join("Notes"))
            .unwrap()
            .is_dir()
    );
    assert_eq!(
        std::fs::read_to_string(files.join("Notes/today.md")).unwrap(),
        "came"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

#[test]
fn a_backup_holds_the_state_but_not_the_install() {
    // Restores stage in HOME's agents folder, which the settings tests
    // point at a folder of the run's and remove: one at a time with them.
    let _home = crate::settings::tests::own_config();
    let dir = temp("archive");
    std::fs::create_dir_all(dir.join(".fake/bin")).unwrap();
    std::fs::create_dir_all(dir.join(".fake/memory")).unwrap();
    std::fs::write(dir.join(".fake/bin/fake"), "program").unwrap();
    std::fs::write(dir.join(".fake/.env"), "KEY=secret").unwrap();
    std::fs::write(dir.join(".fake/memory/fact"), "the sky is blue").unwrap();
    let state = State {
        include: vec!["~/.fake".into(), "~/.missing".into()],
        exclude: vec!["~/.fake/bin".into(), "~/.fake/.env".into()],
    };
    let mut out = Vec::new();
    archive_state(&mut out, &dir, &state).unwrap();
    let mut names: Vec<String> = tar::Archive::new(out.as_slice())
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().display().to_string())
        .collect();
    names.sort();
    assert_eq!(names, [".fake", ".fake/memory", ".fake/memory/fact"]);
}

/// A folder on the way to the state that the harness made a link (to
/// another agent's home) is never followed: not to read it into a
/// backup, nor to remove or write through it on a restore or a move.
#[test]
fn a_link_on_the_way_to_the_state_is_never_followed() {
    // Restores stage in HOME's agents folder, which the settings tests
    // point at a folder of the run's and remove: one at a time with them.
    let _home = crate::settings::tests::own_config();
    let dir = temp("linked");
    let (home, other) = (dir.join("home"), dir.join("other"));
    std::fs::create_dir_all(other.join("data")).unwrap();
    std::fs::write(other.join("data/keys"), "the other agent's").unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::os::unix::fs::symlink(&other, home.join(".signal-cli")).unwrap();
    let state = State {
        include: vec!["~/.signal-cli/data".into()],
        exclude: vec![],
    };

    let refused = |result: anyhow::Result<()>| {
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("is a link"), "{err}");
    };
    let mut out = Vec::new();
    refused(archive_state(&mut out, &home, &state));

    let staging = dir.join("staging");
    std::fs::create_dir_all(staging.join(".signal-cli/data")).unwrap();
    std::fs::write(staging.join(".signal-cli/data/keys"), "planted").unwrap();
    refused(move_state(&staging, &home, &state));

    let mut backup = Vec::new();
    archive_state(&mut backup, &staging, &state).unwrap();
    refused(replace_state(backup.as_slice(), &home, &state, Links::Keep));

    assert_eq!(
        std::fs::read_to_string(other.join("data/keys")).unwrap(),
        "the other agent's"
    );
}

/// A folder of Home named as one of the agent's own is left out of a
/// move, its own folder's carried in its place, and said.
#[test]
fn a_home_folder_named_as_its_own_is_said_when_left_out() {
    let dir = temp("left-out");
    let folder = link_agent_contract::folder::Folder::new(&dir, None);
    std::fs::create_dir_all(folder.home.join("skills/x")).unwrap();
    std::fs::create_dir_all(folder.agent.join("skills/y")).unwrap();
    assert_eq!(left_out(&folder), ["skills"]);
}

/// The agent's own folder from a backup: what's in its collections
/// comes, and a backup holding a link anywhere (one the agent made in
/// Home, where the backup sits) is refused whole, nothing changed.
#[test]
fn its_own_folder_comes_back_without_a_link() {
    // Restores stage in HOME's agents folder, which the settings tests
    // point at a folder of the run's and remove: one at a time with them.
    let _home = crate::settings::tests::own_config();
    // Restores stage in HOME; their destination shares its filesystem.
    let dir = link_test_kit::TempDir::under(&link_files::home_dir(), "transfer-own");
    let own = dir.join("agent");
    std::fs::create_dir_all(own.join("skills")).unwrap();
    let state = State {
        include: vec!["~/instructions".into(), "~/skills".into()],
        exclude: vec![],
    };
    let entry = |tar: &mut tar::Builder<&mut Vec<u8>>, path: &str, data: &str| {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        tar.append_data(&mut header, path, data.as_bytes()).unwrap();
    };
    let mut backup = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut backup);
        entry(&mut tar, "instructions/instruction.md", "Be brief.");
        entry(&mut tar, "schedule/jobs/a.json", "{}");
        tar.finish().unwrap();
    }
    replace_state(backup.as_slice(), &own, &state, Links::Refuse).unwrap();
    assert_eq!(
        std::fs::read_to_string(own.join("instructions/instruction.md")).unwrap(),
        "Be brief."
    );
    assert!(!own.join("schedule").exists());

    let mut forged = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut forged);
        entry(&mut tar, "skills/a/SKILL.md", "a");
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        tar.append_link(&mut header, "skills/b", "/etc").unwrap();
        tar.finish().unwrap();
    }
    let err = format!(
        "{:#}",
        replace_state(forged.as_slice(), &own, &state, Links::Refuse).unwrap_err()
    );
    assert!(err.contains("holds a link"), "{err}");
    // A hard link: two names of one file, refused as well.
    let mut hard = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut hard);
        entry(&mut tar, "instructions/instruction.md", "Be brief.");
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Link);
        header.set_size(0);
        header.set_mode(0o600);
        tar.append_link(&mut header, "skills/c", "instructions/instruction.md")
            .unwrap();
        tar.finish().unwrap();
    }
    let err = format!(
        "{:#}",
        replace_state(hard.as_slice(), &own, &state, Links::Refuse).unwrap_err()
    );
    assert!(err.contains("holds a link"), "{err}");
    assert!(!own.join("skills/c").exists());
    assert!(!own.join("skills/a").exists() && !own.join("skills/b").exists());
    assert!(own.join("instructions/instruction.md").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Restores at once, each into its own home, each from its own
/// folder: none removes or takes over another's, and each home holds
/// its own backup.
#[test]
fn restores_at_once_keep_to_their_own() {
    // Restores stage in HOME's agents folder, which the settings tests
    // point at a folder of the run's and remove: one at a time with them.
    let _home = crate::settings::tests::own_config();
    let dir = link_test_kit::TempDir::under(&link_files::home_dir(), "transfer-at-once");
    let state = State {
        include: vec!["~/instructions".into()],
        exclude: vec![],
    };
    let restores: Vec<_> = (0..8)
        .map(|n| {
            let own = dir.join(format!("Agent-{n}"));
            let state = state.clone();
            std::thread::spawn(move || {
                let mut backup = Vec::new();
                {
                    let mut tar = tar::Builder::new(&mut backup);
                    let data = format!("Agent {n}.");
                    let mut header = tar::Header::new_gnu();
                    header.set_size(data.len() as u64);
                    header.set_mode(0o600);
                    header.set_cksum();
                    tar.append_data(&mut header, "instructions/instruction.md", data.as_bytes())
                        .unwrap();
                    tar.finish().unwrap();
                }
                replace_state(backup.as_slice(), &own, &state, Links::Refuse).map(|()| own)
            })
        })
        .collect();
    for (n, restore) in restores.into_iter().enumerate() {
        let own = restore.join().unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(own.join("instructions/instruction.md")).unwrap(),
            format!("Agent {n}.")
        );
    }
}

/// A restore's folder past its age is one a restore cut short left:
/// removed, as a link is never followed. A younger one, a restore's
/// running now, stays, and so does anything not a restore's.
#[test]
fn a_restore_cut_short_is_removed_once_old() {
    let dir = temp("stale-restores");
    let hour = std::time::Duration::from_secs(60 * 60);
    let aged = |name: &str| {
        let path = dir.join(name);
        std::fs::create_dir_all(path.join("inside")).unwrap();
        std::fs::write(path.join("inside/file"), "left").unwrap();
        let at = std::time::SystemTime::now() - 2 * hour;
        std::fs::File::open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
        path
    };
    let old = aged("restoring-old");
    let other = aged("someones-own");
    let fresh = dir.join("restoring-fresh");
    std::fs::create_dir(&fresh).unwrap();
    let elsewhere = aged("elsewhere");
    std::os::unix::fs::symlink(&elsewhere, dir.join("restoring-link")).unwrap();

    remove_stale_restores(&Dir::open(&dir).unwrap(), hour);
    assert!(!old.exists(), "an old restore's folder stayed");
    assert!(fresh.is_dir() && other.is_dir());
    assert!(elsewhere.join("inside/file").is_file());
}

/// What a stream holds: each entry's path, its link's target if any,
/// and its bytes.
fn listed(tar: &[u8]) -> Vec<(String, Option<String>, Vec<u8>)> {
    tar::Archive::new(tar)
        .entries()
        .unwrap()
        .map(|e| {
            let mut e = e.unwrap();
            let path = e.path().unwrap().display().to_string();
            let link = e.link_name().unwrap().map(|l| l.display().to_string());
            let mut data = Vec::new();
            e.read_to_end(&mut data).unwrap();
            (path, link, data)
        })
        .collect()
}

/// A process of the harness that outlived `stop` swaps a folder on the
/// way to the state for a link after the check of every path, or a
/// file for one after its folder was listed: the walk, from folder
/// handles, refuses the one and carries the other as a link. Nothing
/// outside is read.
#[test]
fn a_link_swapped_in_after_the_check_is_never_read_through() {
    // Restores stage in HOME's agents folder, which the settings tests
    // point at a folder of the run's and remove: one at a time with them.
    let _home = crate::settings::tests::own_config();
    let dir = temp("swapped");
    let (home, outside) = (dir.join("home"), dir.join("outside"));
    std::fs::create_dir_all(outside.join("data")).unwrap();
    std::fs::write(outside.join("id_ed25519"), "PRIVATE KEY").unwrap();
    std::fs::write(outside.join("data/keys"), "PRIVATE KEY").unwrap();
    std::fs::create_dir_all(home.join(".fake/memory")).unwrap();
    std::fs::write(home.join(".fake/memory/fact"), "the sky is blue").unwrap();
    let state = State {
        include: vec!["~/.fake/memory".into()],
        exclude: vec![],
    };
    // The check passes, then the folder on the way becomes a link.
    let includes = in_home(&home, &state.include).unwrap();
    std::fs::rename(home.join(".fake"), dir.join("moved")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join(".fake")).unwrap();
    std::fs::create_dir_all(outside.join("memory")).unwrap();
    std::fs::write(outside.join("memory/fact"), "PRIVATE KEY").unwrap();
    let mut tar = tar::Builder::new(Vec::new());
    let err = add_paths(&mut tar, &home, &includes, Path::new(""), &[], &Out::none()).unwrap_err();
    assert!(format!("{err:#}").contains("is a link"), "{err:#}");
    std::fs::remove_file(home.join(".fake")).unwrap();
    std::fs::rename(dir.join("moved"), home.join(".fake")).unwrap();

    // The folder is open (listed), then its file becomes a link.
    let memory = Dir::open(&home.join(".fake/memory")).unwrap();
    std::fs::remove_file(home.join(".fake/memory/fact")).unwrap();
    std::os::unix::fs::symlink(outside.join("id_ed25519"), home.join(".fake/memory/fact")).unwrap();
    let mut tar = tar::Builder::new(Vec::new());
    tar.follow_symlinks(false);
    add(
        &mut tar,
        &memory,
        OsStr::new("fact"),
        &home.join(".fake/memory/fact"),
        Path::new(".fake/memory/fact"),
        &[],
        &Out::none(),
    )
    .unwrap();
    let entries = listed(&tar.into_inner().unwrap());
    assert_eq!(entries.len(), 1);
    let (path, link, data) = &entries[0];
    assert_eq!(path, ".fake/memory/fact");
    assert_eq!(
        link.as_deref(),
        Some(outside.join("id_ed25519").to_str().unwrap())
    );
    assert!(data.is_empty());

    // And a whole backup of it: the link goes as a link.
    let mut out = Vec::new();
    archive_state(&mut out, &home, &state).unwrap();
    assert!(
        listed(&out)
            .iter()
            .all(|(_, _, data)| !data.windows(11).any(|w| w == b"PRIVATE KEY"))
    );
}

/// A process of the harness that outlived `stop` swaps a folder for a
/// link while a restore runs: after the check of every path, or after
/// the folder was opened. The removal and the move, from folder
/// handles, refuse a link on the way, remove or replace one in a state
/// path's place, and act in the folder they opened: nothing outside is
/// removed or written.
#[test]
fn a_restore_never_follows_a_link_swapped_in() {
    let dir = temp("restore-swapped");
    let (home, outside, staging) = (dir.join("home"), dir.join("outside"), dir.join("staging"));
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    std::fs::create_dir_all(outside.join("memory")).unwrap();
    std::fs::write(outside.join("memory/keys"), "PRIVATE KEY").unwrap();
    std::fs::create_dir_all(home.join(".fake/memory")).unwrap();
    std::fs::write(home.join(".fake/memory/fact"), "old").unwrap();
    std::fs::create_dir_all(staging.join(".fake/memory")).unwrap();
    std::fs::write(staging.join(".fake/memory/fact"), "restored").unwrap();
    let deep = State {
        include: vec!["~/.fake/memory".into()],
        exclude: vec![],
    };
    let untouched = || {
        assert_eq!(read(&outside.join("memory/keys")), "PRIVATE KEY");
        let names: Vec<_> = std::fs::read_dir(outside.join("memory"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["keys"]);
    };

    // The checks pass, then the folder on the way becomes a link.
    let includes = in_home(&home, &deep.include).unwrap();
    let from = in_home(&staging, &deep.include).unwrap();
    std::fs::rename(home.join(".fake"), dir.join("aside")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join(".fake")).unwrap();
    let err = remove_paths(&home, &includes, &[]).unwrap_err();
    assert!(format!("{err:#}").contains("is a link"), "{err:#}");
    let err = move_paths(&staging, &home, &from, &[]).unwrap_err();
    assert!(format!("{err:#}").contains("is a link"), "{err:#}");
    untouched();
    std::fs::remove_file(home.join(".fake")).unwrap();
    std::fs::rename(dir.join("aside"), home.join(".fake")).unwrap();

    // The folder is open, then swapped for a link: what's removed is
    // in the folder that was opened.
    let fake = Dir::open(&home.join(".fake")).unwrap();
    std::fs::rename(home.join(".fake"), dir.join("aside")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join(".fake")).unwrap();
    remove_at(&fake, OsStr::new("memory"), &home.join(".fake/memory"), &[]).unwrap();
    assert!(!dir.join("aside/memory").exists());
    untouched();
    std::fs::remove_file(home.join(".fake")).unwrap();
    std::fs::rename(dir.join("aside"), home.join(".fake")).unwrap();

    // A link in a state path's place is removed, and replaced by what
    // comes, never followed.
    std::os::unix::fs::symlink(outside.join("memory"), home.join(".fake/memory")).unwrap();
    let whole = State {
        include: vec!["~/.fake".into()],
        exclude: vec!["~/.fake/bin".into()],
    };
    std::fs::create_dir_all(home.join(".fake/bin")).unwrap();
    std::fs::write(home.join(".fake/bin/fake"), "program").unwrap();
    let excluded = in_home(&home, &whole.exclude).unwrap();
    remove_paths(&home, &in_home(&home, &whole.include).unwrap(), &excluded).unwrap();
    assert!(home.join(".fake/memory").symlink_metadata().is_err());
    untouched();
    std::os::unix::fs::symlink(outside.join("memory"), home.join(".fake/memory")).unwrap();
    move_state(&staging, &home, &whole).unwrap();
    assert!(
        home.join(".fake/memory")
            .symlink_metadata()
            .unwrap()
            .is_dir()
    );
    assert_eq!(read(&home.join(".fake/memory/fact")), "restored");
    assert_eq!(read(&home.join(".fake/bin/fake")), "program");
    untouched();
}

/// A stream's home holds more than the state: an install, keys, a
/// program for later. Only what this machine's adapter names as the
/// state comes; its install and keys here stay.
#[test]
fn only_the_state_named_here_comes() {
    let dir = temp("put-state");
    let (home, staging) = (dir.join("home"), dir.join("staging"));
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    std::fs::create_dir_all(home.join(".fake/bin")).unwrap();
    std::fs::create_dir_all(home.join(".fake/memory")).unwrap();
    std::fs::write(home.join(".fake/bin/fake"), "program").unwrap();
    std::fs::write(home.join(".fake/.env"), "KEY=mine").unwrap();
    std::fs::write(home.join(".fake/memory/stale"), "old").unwrap();
    for (path, text) in [
        (".fake/memory/fact", "the sky is blue"),
        (".fake/bin/fake", "planted"),
        (".fake/.env", "KEY=planted"),
        (".lnk/exec/bin/lnk-fake", "planted"),
        (".profile", "planted"),
    ] {
        let path = staging.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let here = State {
        include: vec!["~/.fake".into()],
        exclude: vec!["~/.fake/bin".into(), "~/.fake/.env".into()],
    };
    put_state(&staging, &home, &here).unwrap();
    assert_eq!(read(&home.join(".fake/memory/fact")), "the sky is blue");
    assert!(!home.join(".fake/memory/stale").exists());
    assert_eq!(read(&home.join(".fake/bin/fake")), "program");
    assert_eq!(read(&home.join(".fake/.env")), "KEY=mine");
    assert!(!home.join(".lnk").exists() && !home.join(".profile").exists());
}

#[test]
fn a_state_moves_without_its_install() {
    let dir = temp("move");
    let (from, to) = (dir.join("from"), dir.join("to"));
    std::fs::create_dir_all(from.join(".fake/bin")).unwrap();
    std::fs::create_dir_all(from.join(".fake/memory")).unwrap();
    std::fs::write(from.join(".fake/bin/fake"), "program").unwrap();
    std::fs::write(from.join(".fake/.env"), "KEY=secret").unwrap();
    std::fs::write(from.join(".fake/memory/fact"), "the sky is blue").unwrap();
    std::os::unix::fs::symlink("memory/fact", from.join(".fake/latest")).unwrap();
    std::fs::write(from.join("other"), "not the agent").unwrap();
    std::fs::create_dir_all(to.join(".fake/memory")).unwrap();
    std::fs::write(to.join(".fake/memory/stale"), "old").unwrap();
    let state = State {
        include: vec!["~/.fake".into(), "~/.missing".into()],
        exclude: vec!["~/.fake/bin".into(), "~/.fake/.env".into()],
    };
    move_state(&from, &to, &state).unwrap();
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    assert_eq!(read(&to.join(".fake/memory/fact")), "the sky is blue");
    assert_eq!(
        std::fs::read_link(to.join(".fake/latest")).unwrap(),
        Path::new("memory/fact")
    );
    // Folders merge: a move cut short and run again finishes it.
    assert_eq!(read(&to.join(".fake/memory/stale")), "old");
    // The install and its keys stay where they were, and nothing else
    // comes.
    assert!(!to.join(".fake/bin").exists() && !to.join(".fake/.env").exists());
    assert_eq!(read(&from.join(".fake/bin/fake")), "program");
    assert_eq!(read(&from.join(".fake/.env")), "KEY=secret");
    assert!(!from.join(".fake/memory").exists());
    assert!(!to.join("other").exists());
}
