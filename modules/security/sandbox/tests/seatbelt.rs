//! The sandbox's rules, run for real: Seatbelt on macOS (this file), and
//! `lnk sandbox run` and `check` under bubblewrap on Linux (`run.rs`).

use std::path::Path;

use link_test_kit::TempDir;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The Seatbelt profile: a harness writes its files folder but can't read
/// anything else in the home folder, such as `~/.ssh`.
#[test]
fn the_seatbelt_profile_keeps_the_harness_out_of_ssh_keys() {
    use link_sandbox_confine::macos::seatbelt::profile as seatbelt_profile;
    use link_sandbox_confine::{Rules, resolve};

    let root = TempDir::new("seatbelt");
    let home = root.join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join("Documents")).unwrap();
    std::fs::write(home.join(".ssh/id_test"), "SECRET KEY").unwrap();
    std::fs::write(home.join("Documents/taxes.txt"), "PRIVATE").unwrap();
    let files = home.join("Link/Agents/main/home");
    std::fs::create_dir_all(&files).unwrap();
    // A folder under /tmp the harness creates itself, as a harness may its
    // lock folder: allowed through /tmp's real path, /private/tmp.
    let locks = std::path::PathBuf::from(format!("/tmp/lnk-test-locks-{}", std::process::id()));
    let mut policy = Rules::default();
    policy.common.write = vec![resolve(&files), resolve(&locks)];
    policy.common.home = resolve(&home);
    let profile = root.join("p.sb");
    std::fs::write(&profile, seatbelt_profile(&policy.common, &policy.macos)).unwrap();
    let files = files.canonicalize().unwrap();
    let home = home.canonicalize().unwrap();

    let run = |script: String| {
        link_test_kit::env::command("/usr/bin/sandbox-exec", &home)
            .arg("-f")
            .arg(&profile)
            .args(["/bin/sh", "-c", &script])
            .output()
            .unwrap()
    };
    let wrote = run(format!("echo working > '{}/out'", files.display()));
    assert!(
        wrote.status.success(),
        "{}",
        String::from_utf8_lossy(&wrote.stderr)
    );
    assert_eq!(read(&files.join("out")), "working\n");
    let locked = run(format!(
        "mkdir -p '{0}' && touch '{0}/a.lock'",
        locks.display()
    ));
    assert!(
        locked.status.success(),
        "{}",
        String::from_utf8_lossy(&locked.stderr)
    );
    let _ = std::fs::remove_dir_all(&locks);
    for secret in [".ssh/id_test", "Documents/taxes.txt"] {
        let stolen = run(format!("cat '{}/{secret}'", home.display()));
        assert!(
            !stolen.status.success(),
            "the sandbox let the harness read ~/{secret}"
        );
        assert!(stolen.stdout.is_empty());
    }
}

/// A project's dot files that run outside: read only at the top of a
/// folder it writes, there or not, in any case of their letters, and not
/// through a hard link; the rest of the project written.
#[test]
fn a_projects_dot_files_are_read_only() {
    use link_sandbox_client::{Confinement, Net, Programs};
    use link_sandbox_confine::macos::seatbelt::profile as seatbelt_profile;
    use link_sandbox_confine::{Rules, resolve};

    let root = TempDir::new("seatbelt-dots");
    let app = root.join("app");
    std::fs::create_dir_all(app.join(".git/hooks")).unwrap();
    std::fs::write(app.join(".git/config"), "[core]\n").unwrap();
    let app = resolve(&app);
    let rules = Rules::of(&Confinement {
        name: "dots".into(),
        write: vec![app.clone()],
        write_dot_files: Vec::new(),
        read: Vec::new(),
        read_home: false,
        hide: Vec::new(),
        tmp_prefix: None,
        terminal: false,
        commands: Programs::Any,
        developer_tools: false,
        builds: Vec::new(),
        net: Net::Closed,
    })
    .unwrap();
    let profile = root.join("p.sb");
    let mut rules = rules;
    rules.common.home = resolve(&root);
    std::fs::write(&profile, seatbelt_profile(&rules.common, &rules.macos)).unwrap();
    let run = |script: &str| {
        link_test_kit::env::command("/usr/bin/sandbox-exec", &root)
            .arg("-f")
            .arg(&profile)
            .args(["/bin/sh", "-c", script])
            .current_dir(&app)
            .output()
            .unwrap()
    };
    for (what, script) in [
        ("its settings", "echo x >> .git/config"),
        ("a hook", "echo x > .git/hooks/pre-commit"),
        ("a file git reads", "echo x > .git/commondir"),
        ("its .git, moved aside", "mv .git moved"),
        (
            "its settings, through a hard link",
            "ln .git/config linked && echo x >> linked",
        ),
        (".envrc", "echo x > .envrc"),
        (".envrc, in another case", "echo x > .ENVRC"),
        (".vscode", "mkdir .vscode"),
        (".vscode, in another case", "mkdir .VSCode"),
        (".idea", "mkdir .idea"),
    ] {
        let out = run(script);
        assert!(!out.status.success(), "the sandbox wrote {what}");
    }
    assert!(!read(&app.join(".git/config")).contains('x'));
    assert!(app.join(".git/config").is_file());
    let wrote = run("echo x > file");
    assert!(
        wrote.status.success(),
        "{}",
        String::from_utf8_lossy(&wrote.stderr)
    );
}
