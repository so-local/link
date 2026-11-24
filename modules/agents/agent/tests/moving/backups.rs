//! Backups and restores: what goes to a bucket, its layout, and an agent
//! coming back from one.

use super::*;

#[test]
fn a_backup_pushes_the_harness_state_with_the_files() {
    let world = World::new("backup");
    let here = &world.here;
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"fake\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(here).display()
        ),
    );
    write(&world.agent(here).join("memory/first"), "remembered");
    write(&world.agent(here).join("bin/fake"), "a Mac program");
    write(&world.agent(here).join(".env"), "KEY=here");
    write(&world.files(here).join("notes.txt"), "mine");
    let own = here.join("Link/Agents/main/agent");
    write(&own.join("instructions/instruction.md"), "Be brief.");
    write(&own.join("chats/link-main/events.jsonl"), "{}\n");

    ok(&world.lnk_agent(&["backup"]));
    let archive = world.files(here).join("Backups/fake.tar");
    let own_archive = world.files(here).join("Backups/Agent.tar");
    let pushed = std::fs::read_to_string(world.root.join("pushed")).unwrap();
    assert_eq!(
        pushed,
        format!(
            "storage push --replace {} {}\nstorage push --replace {}\n",
            archive.display(),
            own_archive.display(),
            world.files(here).display()
        )
    );
    // A bucket a first push makes is billed by its cloud, so `lnk storage
    // push` asks; `--yes` answers it ahead, for each push.
    std::fs::remove_file(world.root.join("pushed")).unwrap();
    ok(&world.lnk_agent(&["backup", "--yes"]));
    let pushed = std::fs::read_to_string(world.root.join("pushed")).unwrap();
    assert_eq!(
        pushed,
        format!(
            "storage push --replace --yes {} {}\nstorage push --replace --yes {}\n",
            archive.display(),
            own_archive.display(),
            world.files(here).display()
        )
    );
    // Its own folder, beside Home, carried whole.
    let own_names: Vec<String> = tar::Archive::new(std::fs::File::open(&own_archive).unwrap())
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().display().to_string())
        .collect();
    for name in [
        "instructions/instruction.md",
        "chats/link-main/events.jsonl",
    ] {
        assert!(
            own_names.iter().any(|n| n == name),
            "{name} in {own_names:?}"
        );
    }
    let mut names: Vec<String> = tar::Archive::new(std::fs::File::open(&archive).unwrap())
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().display().to_string())
        .collect();
    names.sort();
    assert_eq!(names, [".fake", ".fake/memory", ".fake/memory/first"]);
    let mode = std::os::unix::fs::PermissionsExt::mode(&archive.metadata().unwrap().permissions());
    assert_eq!(mode & 0o777, 0o600);
}

/// Backing up and restoring need the storage module: without it, each
/// installs it, saying so (no terminal to ask in), and goes on.
#[test]
fn a_backup_and_a_restore_install_the_bucket_module() {
    let world = World::new("backup-alone");
    an_agent_here(&world);
    write(&world.agent(&world.here).join("memory/first"), "remembered");
    let said = ok(&world.lnk_agent_alone(&["backup"]));
    assert!(
        said.contains("lnk agent backup needs the storage module (lnk-storage): installing it"),
        "{said}"
    );
    assert!(!said.contains("lnk module add storage`"), "{said}");
    assert_eq!(world.added(), "module add storage\n");
    assert!(world.root.join("pushed").exists());

    let archive = world.files(&world.here).join("Backups/fake.tar");
    std::fs::copy(&archive, world.root.join("backup.tar")).unwrap();
    std::fs::remove_file(world.alone.join("lnk-storage")).unwrap();
    let said = ok(&world.lnk_agent_alone(&["restore", "--yes", "--no-start"]));
    assert!(
        said.contains("lnk agent restore needs the storage module"),
        "{said}"
    );
    assert_eq!(world.added(), "module add storage\nmodule add storage\n");
    assert!(world.root.join("pulled").exists());

    // An agent that isn't here yet, the same.
    std::fs::remove_file(world.alone.join("lnk-storage")).unwrap();
    let said = world.lnk_agent_alone(&["restore", "other", "--yes", "--no-start"]);
    let said = String::from_utf8_lossy(&said.stderr);
    assert!(
        said.contains("lnk agent restore needs the storage module"),
        "{said}"
    );
    assert_eq!(world.added().lines().count(), 3);
}

/// A backup with no cloud to go to says what makes one, without a
/// terminal to ask which in, and pushes nothing.
#[test]
fn a_backup_with_no_cloud_says_what_makes_one() {
    let world = World::new("backup-no-cloud");
    an_agent_here(&world);
    std::fs::write(world.root.join("no-clouds"), "").unwrap();
    let out = world.lnk_agent(&["backup"]);
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("Backing up goes to a bucket in your clouds, and you have none yet.")
            && said.contains("lnk storage create <type>"),
        "{said}"
    );
    assert!(!world.root.join("pushed").exists());
}

/// An agent whose machine is gone comes back from its bucket: the same
/// agent, its files and its harness's memory, asking for its key again.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn an_agent_comes_back_from_its_bucket() {
    let world = World::new("restore");
    let here = &world.here;
    // An outside harness's adapter, saying its state as a real one does.
    crate::common::write_adapter(
        &world.bin.join("lnk-heron"),
        r#"#!/bin/sh
case "$1" in
  state) echo '{"include":["~/.heron","~/.signal-cli/data"],"exclude":["~/.heron/heron-agent","~/.heron/bin","~/.heron/logs","~/.heron/cache","~/.heron/.env"]}' ;;
  *) exit 0 ;;
esac
"#,
    )
    .unwrap();
    // Its last backup: the harness's state, and what the harness, which
    // writes the files folder, put beside it: an install, keys, a file
    // out of its state. It lands as `heron.tar` and as `box.tar`, whose
    // name is no harness here (`heron` was added from outside Link).
    let mut archive = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut archive);
        for (path, data) in [
            (".heron/memories/first", "remembered"),
            (".heron/bin/heron", "planted"),
            (".heron/.env", "KEY=planted"),
            (".profile", "planted"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o600);
            header.set_cksum();
            tar.append_data(&mut header, path, data.as_bytes()).unwrap();
        }
        tar.finish().unwrap();
    }
    std::fs::write(world.root.join("backup.tar"), archive).unwrap();
    // And its own folder, as a backup made since it's beside Home carries
    // it, with a file out of its collections the harness put there.
    let mut own = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut own);
        for (path, data) in [
            ("instructions/instruction.md", "Be brief."),
            ("chats/link-main/events.jsonl", "{}\n"),
            (".profile", "planted"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o600);
            header.set_cksum();
            tar.append_data(&mut header, path, data.as_bytes()).unwrap();
        }
        tar.finish().unwrap();
    }
    std::fs::write(world.root.join("own.tar"), own).unwrap();

    let mut restore = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["restore", "main", "--no-start"])
        .env_clear()
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(here))
        .env("HOME", here)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(here))
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut restore.stdin.take().unwrap(), b"the saved password\n").unwrap();
    ok(&restore.wait_with_output().unwrap());
    let settings = std::fs::read_to_string(here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    let identity =
        std::fs::read_to_string(here.join(".lnk/safe/agents/main/identity.toml")).unwrap();
    assert!(
        identity.contains(&format!("id = \"{ID}\"")),
        "{settings} {identity}"
    );
    assert!(settings.contains("harness = \"heron\""), "{settings}");
    // No model yet: `start` asks for its key.
    assert!(!settings.contains("[model]"), "{settings}");
    let home = here.join(".lnk/data/agents/main/harnesses");
    assert_eq!(
        std::fs::read_to_string(home.join("heron/.heron/memories/first")).unwrap(),
        "remembered"
    );
    // Only its state: never an install, keys, or anything else.
    for planted in [".heron/bin", ".heron/.env", ".profile"] {
        assert!(!home.join("heron").join(planted).exists(), "{planted}");
    }
    assert!(!home.join("box").exists());
    assert_eq!(
        std::fs::read_to_string(world.files(here).join("notes.txt")).unwrap(),
        "pulled\n"
    );
    // Its own folder back, its collections alone.
    let own = here.join("Link/Agents/main/agent");
    assert_eq!(
        std::fs::read_to_string(own.join("instructions/instruction.md")).unwrap(),
        "Be brief."
    );
    assert!(own.join("chats/link-main/events.jsonl").is_file());
    assert!(!own.join(".profile").exists());
    // One bucket command did the bucket's part: finding, asking the
    // password (on the terminal it inherits), adopting, pulling.
    assert_eq!(
        std::fs::read_to_string(world.root.join("restored")).unwrap(),
        format!(
            "storage restore --owner agent:main --folder {}\n",
            world.files(here).display()
        )
    );
    assert_eq!(
        std::fs::read_to_string(world.root.join("restored-with")).unwrap(),
        "the saved password\n"
    );

    // Here now: restoring it again puts back its state, after asking.
    let again = world.lnk_agent(&["restore", "main", "--no-start"]);
    assert!(!again.status.success());
    let asked = crate::moving::text(&again);
    assert!(asked.contains("This replaces what"), "{asked}");
    assert!(asked.contains("--yes"), "{asked}");
    assert!(!world.root.join("pulled").exists());
}

/// An agent here goes back to what its harness knew at its last backup:
/// its state replaced, its install, keys and files as they are.
#[test]
fn an_agent_here_goes_back_to_its_backup() {
    let world = World::new("rollback");
    let here = &world.here;
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"fake\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(here).display()
        ),
    );
    write(&world.agent(here).join("memory/first"), "remembered");
    write(&world.agent(here).join("bin/fake"), "a Mac program");
    write(&world.agent(here).join(".env"), "KEY=here");
    write(&world.files(here).join("notes.txt"), "mine");
    let own = here.join("Link/Agents/main/agent");
    write(&own.join("chats/link-main/events.jsonl"), "before\n");
    ok(&world.lnk_agent(&["backup"]));
    let archive = world.files(here).join("Backups/fake.tar");
    let own_archive = world.files(here).join("Backups/Agent.tar");
    std::fs::copy(&archive, world.root.join("backup.tar")).unwrap();
    std::fs::copy(&own_archive, world.root.join("own.tar")).unwrap();
    // Since then: a memory changed, one learned, a conversation went on
    // and one began, and the backup here lost.
    write(&world.agent(here).join("memory/first"), "garbled");
    write(&world.agent(here).join("memory/second"), "learned since");
    write(
        &own.join("chats/link-main/events.jsonl"),
        "before\nsaid since\n",
    );
    write(&own.join("chats/link-later/events.jsonl"), "said since\n");
    std::fs::remove_file(&archive).unwrap();
    std::fs::remove_file(&own_archive).unwrap();

    // A restore that starts nothing reads no spec, so it's refused one
    // before anything is pulled.
    for flagged in [&["--spec", "given.toml"][..], &["--turns.calls", "3"]] {
        let args = [&["restore", "--yes", "--no-start"][..], flagged].concat();
        let refused = world.lnk_agent(&args);
        assert_eq!(refused.status.code(), Some(2), "{refused:?}");
        assert!(!world.root.join("pulled").exists());
    }
    let restored = ok(&world.lnk_agent(&["restore", "--yes", "--no-start"]));
    assert!(restored.contains("knows what it knew"), "{restored}");
    assert_eq!(
        std::fs::read_to_string(world.root.join("pulled")).unwrap(),
        format!(
            "storage pull --replace {} {}\n",
            archive.display(),
            own_archive.display()
        )
    );
    // Its conversations went back too: what was said since is gone.
    assert_eq!(
        std::fs::read_to_string(own.join("chats/link-main/events.jsonl")).unwrap(),
        "before\n"
    );
    assert!(!own.join("chats/link-later").exists());
    let agent = world.agent(here);
    assert_eq!(
        std::fs::read_to_string(agent.join("memory/first")).unwrap(),
        "remembered"
    );
    assert!(!agent.join("memory/second").exists());
    assert_eq!(
        std::fs::read_to_string(agent.join("bin/fake")).unwrap(),
        "a Mac program"
    );
    assert_eq!(
        std::fs::read_to_string(agent.join(".env")).unwrap(),
        "KEY=here"
    );
    assert_eq!(
        std::fs::read_to_string(world.files(here).join("notes.txt")).unwrap(),
        "mine"
    );
    let staged = std::fs::read_dir(link_files::layout::Hidden::Restoring.in_home(here))
        .unwrap()
        .any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("restoring-")
        });
    assert!(!staged, "a restore's staging folder is left");

    // --id is for an agent that isn't here.
    let wrong = world.lnk_agent(&["restore", "main", "--id", ID, "--yes"]);
    assert!(!wrong.status.success());
}

/// A restore of an agent running in the background stops it before its
/// backup is pulled, as storage refuses a running owner's folder, then
/// replaces its state.
#[cfg(target_os = "linux")]
#[test]
fn a_running_agent_is_stopped_before_its_backup_is_pulled() {
    let world = World::new("rollback-running");
    let here = &world.here;
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"fake\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(here).display()
        ),
    );
    write(&world.agent(here).join("memory/first"), "remembered");
    ok(&world.lnk_agent(&["backup"]));
    let archive = world.files(here).join("Backups/fake.tar");
    std::fs::copy(&archive, world.root.join("backup.tar")).unwrap();
    write(&world.agent(here).join("memory/first"), "garbled");
    // Its service runs, as systemd says, until it's stopped.
    write(&world.root.join("running"), "");
    executable(
        &world.bin.join("systemctl"),
        &format!(
            "#!/bin/sh\ncase \"$*\" in\n  *is-active*) [ ! -e '{root}/stopped' ] ;;\n  \
             *disable*--now*) : > '{root}/stopped' ;;\n  *) exit 0 ;;\nesac\n",
            root = world.root.display()
        ),
    );
    let mut restore = world.lnk_agent_command(&["restore", "--yes", "--no-start"]);
    restore.env("LNK_SERVICES", "1");
    let restored = ok(&restore.output().unwrap());
    assert!(restored.contains("Stopping your agent"), "{restored}");
    assert!(world.root.join("stopped").exists());
    assert_eq!(
        std::fs::read_to_string(world.agent(here).join("memory/first")).unwrap(),
        "remembered"
    );
}
