//! A project's dot files that run outside the sandbox, under bubblewrap:
//! read only at the top of every folder a sandbox writes (`.git`,
//! `.envrc`, `.vscode`, `.idea`, the hooks folder git's settings name),
//! written only where `--files.write_dot_files` names the folder.

use std::path::PathBuf;
use std::process::Command;

use link_test_kit::TempDir;

/// A home, and a project in it that's a repository (made by hand: its
/// settings, a hook, a HEAD), with a hooks folder its settings name.
struct Project {
    root: TempDir,
    home: PathBuf,
    dir: PathBuf,
}

impl Project {
    fn new(test: &str) -> Self {
        let root = TempDir::new(&format!("dotfiles-{test}"));
        let home = root.join("home");
        let dir = home.join("app");
        std::fs::create_dir_all(dir.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(dir.join(".husky")).unwrap();
        std::fs::create_dir_all(home.join(".lnk")).unwrap();
        std::fs::write(
            dir.join(".git/config"),
            "[core]\n\trepositoryformatversion = 0\n\thooksPath = .husky/_\n",
        )
        .unwrap();
        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(dir.join(".git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
        Self { root, home, dir }
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn lnk(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"));
        command
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", "/usr/bin:/bin")
            .current_dir(&self.dir);
        command
    }

    /// Runs `script` in a sandbox writing the project (`flags` beside),
    /// and what it printed.
    fn run(&self, flags: &[&str], script: &str) -> String {
        let mut args = vec![
            "run",
            "--commands.toolchains",
            "shell",
            "--commands.toolchains",
            "git",
            "--files.write",
            ".",
        ];
        args.extend(flags);
        args.extend(["--", "/bin/bash", "-c", script]);
        let output = self.lnk(&args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

/// Each way to change what runs outside, and a plain file beside them:
/// one line per try that worked.
const TRIES: &str = r#"
echo x >> .git/config 2>/dev/null && echo config
echo x > .git/hooks/post-checkout 2>/dev/null && echo hook
echo x > .git/commondir 2>/dev/null && echo commondir
ln .git/config linked 2>/dev/null && echo x >> linked 2>/dev/null && echo linked
mv .git moved 2>/dev/null && echo moved && mv moved .git
echo x > .envrc 2>/dev/null && echo envrc
mkdir -p .vscode 2>/dev/null && echo x > .vscode/tasks.json 2>/dev/null && echo vscode
mkdir -p .idea 2>/dev/null && echo x > .idea/workspace.xml 2>/dev/null && echo idea
mkdir -p .husky/_ 2>/dev/null && echo x > .husky/_/pre-commit 2>/dev/null && echo husky-hook
echo x > .husky/pre-commit 2>/dev/null && echo husky-script
mv .husky .h2 2>/dev/null && echo husky-moved && mv .h2 .husky
echo x > file 2>/dev/null && echo file
true"#;

#[test]
fn a_projects_dot_files_are_read_only_unless_opened() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let project = Project::new("closed");
    let tried = project.run(&[], TRIES);
    // The project's own files, and a new .envrc, which direnv runs only
    // once it's allowed. husky's scripts beside its hooks folder, and the
    // folder above it, are kept too.
    assert_eq!(tried.lines().collect::<Vec<_>>(), ["envrc", "file"]);
    let config = std::fs::read_to_string(project.dir.join(".git/config")).unwrap();
    assert!(!config.contains('x'), "{config}");
    assert!(!project.dir.join(".git/commondir").exists());
    assert!(project.dir.join(".git/HEAD").is_file(), "its .git stayed");
    // What wasn't there was empty while it ran, and is gone after.
    for made in [".vscode", ".idea", ".husky/_"] {
        assert!(!project.dir.join(made).exists(), "{made} left behind");
    }
    assert!(project.dir.join(".husky").is_dir(), "the project's own");
    assert!(
        !project
            .home
            .join(".lnk/safe/sandboxes/placeholders.json")
            .exists()
    );
    // An .envrc that's there is read only.
    let tried = project.run(&[], "echo y >> .envrc 2>/dev/null && echo envrc; true");
    assert_eq!(tried, "");
    assert_eq!(
        std::fs::read_to_string(project.dir.join(".envrc")).unwrap(),
        "x\n"
    );

    // Opened for the folder, all of it is written.
    let opened = Project::new("opened");
    let tried = opened.run(&["--files.write_dot_files", "."], TRIES);
    for wrote in [
        "config",
        "hook",
        "commondir",
        "linked",
        "moved",
        "envrc",
        "vscode",
        "idea",
        "husky-hook",
        "husky-script",
        "file",
    ] {
        assert!(tried.lines().any(|l| l == wrote), "{wrote}: {tried}");
    }
}

/// A hooks folder below the top, in a folder of the project's own: the
/// hooks read only, and the folders above it kept from being renamed, but
/// what else is in them written.
#[test]
fn a_nested_hooks_folder_is_kept_and_the_folder_above_it_written() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let project = Project::new("nested");
    std::fs::create_dir_all(project.dir.join("scripts/hooks")).unwrap();
    std::fs::write(
        project.dir.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n\thooksPath = scripts/hooks\n",
    )
    .unwrap();
    let tried = project.run(
        &[],
        r#"
echo x > scripts/hooks/pre-commit 2>/dev/null && echo hook
mv scripts/hooks scripts/h2 2>/dev/null && echo hooks-moved
mv scripts s2 2>/dev/null && echo scripts-moved
echo x > scripts/build.sh 2>/dev/null && echo script
true"#,
    );
    assert_eq!(tried.lines().collect::<Vec<_>>(), ["script"]);
    assert!(!project.dir.join("scripts/hooks/pre-commit").exists());
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn git_reads_a_closed_repository_and_commits_in_an_opened_one() {
    if !link_test_kit::gate::bubblewrap()
        || !link_test_kit::gate::program("/usr/bin/git", &["--version"])
    {
        return;
    }
    let project = Project::new("repo");
    std::fs::remove_dir_all(project.dir.join(".git")).unwrap();
    let outside = |args: &[&str]| {
        let status = Command::new("/usr/bin/git")
            .args(["-c", "user.name=a", "-c", "user.email=a@b"])
            .args(args)
            .current_dir(&project.dir)
            .env("HOME", &project.home)
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}");
    };
    outside(&["init", "-q"]);
    std::fs::write(project.dir.join("a"), "a\n").unwrap();
    outside(&["add", "a"]);
    outside(&["commit", "-qm", "first"]);
    let tries = r#"
git status --short >/dev/null 2>&1 && echo status
git log --oneline >/dev/null 2>&1 && echo log
echo b > a
git -c user.name=a -c user.email=a@b commit -qam second >/dev/null 2>&1 && echo commit
true"#;
    let closed = project.run(&[], tries);
    assert_eq!(closed.lines().collect::<Vec<_>>(), ["status", "log"]);
    let opened = project.run(&["--files.write_dot_files", "."], tries);
    assert_eq!(
        opened.lines().collect::<Vec<_>>(),
        ["status", "log", "commit"]
    );
}

/// A box's clone stage, as the boxes' guide writes it: `git clone` into
/// the empty folder it writes finds the placeholder `.git` there and
/// stops, unless the stage names the folder in `write_dot_files`.
#[test]
fn a_clone_stage_clones_only_where_it_writes_dot_files() {
    if !link_test_kit::gate::bubblewrap()
        || !link_test_kit::gate::program("/usr/bin/git", &["--version"])
    {
        return;
    }
    let project = Project::new("clone");
    let source = project.root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    let git = |args: &[&str]| {
        let status = link_test_kit::env::command("/usr/bin/git", &project.home)
            .args(["-c", "user.name=a", "-c", "user.email=a@b"])
            .args(args)
            .current_dir(&source)
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(source.join("a"), "a\n").unwrap();
    git(&["add", "a"]);
    git(&["commit", "-qm", "first"]);
    std::fs::remove_dir_all(project.dir.join(".git")).unwrap();
    std::fs::remove_dir_all(project.dir.join(".husky")).unwrap();
    let read = source.display().to_string();
    let clone = format!("git clone -q {read} . 2>/dev/null && echo cloned; true");
    let closed = project.run(&["--files.read", &read], &clone);
    assert_eq!(closed, "", "cloned past the placeholder .git");
    assert!(!project.dir.join("a").exists());
    let opened = project.run(
        &["--files.read", &read, "--files.write_dot_files", "."],
        &clone,
    );
    assert_eq!(opened.trim(), "cloned");
    assert!(project.dir.join(".git/HEAD").is_file());
    assert_eq!(
        std::fs::read_to_string(project.dir.join("a")).unwrap(),
        "a\n"
    );
}

#[test]
fn a_project_without_a_repository_cant_get_one() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let project = Project::new("bare");
    std::fs::remove_dir_all(project.dir.join(".git")).unwrap();
    let tried = project.run(
        &[],
        "mkdir -p .git/hooks 2>/dev/null; echo x > .git/config 2>/dev/null && echo made; true",
    );
    assert_eq!(tried, "");
    // Its placeholder, an empty .git, went with the run.
    assert!(!project.dir.join(".git").exists());
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_placeholder_stays_while_another_run_holds_it() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let project = Project::new("held");
    let vscode = project.dir.join(".vscode");
    // A long run, which tries .vscode after a short one has ended.
    let long = project
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--commands.toolchains",
            "git",
            "--files.write",
            ".",
            "--",
            "/bin/bash",
            "-c",
            "sleep 3; { echo x > .vscode/tasks.json; } 2>/dev/null && echo wrote; true",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !vscode.is_dir() && start.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(vscode.is_dir(), "the long run's placeholder");
    assert_eq!(project.run(&[], "true"), "");
    assert!(vscode.is_dir(), "removed while the long run holds it");
    let out = long.wait_with_output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "", "it wrote .vscode");
    assert!(!vscode.exists(), "left after both");
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn what_a_killed_run_left_goes_with_the_next() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let project = Project::new("killed");
    let idea = project.dir.join(".idea");
    let mut killed = project
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--commands.toolchains",
            "git",
            "--files.write",
            ".",
            "--",
            "/bin/sleep",
            "30",
        ])
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !idea.is_dir() && start.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // Every process of the run, the one holding the placeholders too.
    let _ = Command::new("pkill")
        .args([
            "-KILL",
            "-f",
            &format!("placeholders --dir {}", project.home.display()),
        ])
        .status();
    let _ = killed.kill();
    let _ = killed.wait();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(idea.is_dir(), "a run killed outright can't remove its own");
    let list = project.home.join(".lnk/safe/sandboxes/placeholders.json");
    let before = std::fs::read_to_string(&list).unwrap_or_default();
    assert_eq!(project.run(&[], "true"), "");
    let after = std::fs::read_to_string(&list).unwrap_or_default();
    assert!(
        !idea.exists(),
        "the next run to end removes it: {before} then {after}"
    );
}

#[test]
fn a_link_where_a_dot_file_goes_is_refused() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let project = Project::new("link");
    let elsewhere = project.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, project.dir.join(".vscode")).unwrap();
    let output = project
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--commands.toolchains",
            "git",
            "--files.write",
            ".",
            "--",
            "/bin/true",
        ])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && err.contains("is a link"),
        "{err}"
    );
    // Opened, the link is the sandbox's to write through, as before.
    let output = project
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--commands.toolchains",
            "git",
            "--files.write",
            ".",
            "--files.write_dot_files",
            ".",
            "--",
            "/bin/true",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn check_tries_them_closed_and_opened() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let project = Project::new("check");
    let check = |args: &[&str]| {
        let output = project.lnk(args).output().unwrap();
        let out = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success(), "{out}");
        out
    };
    let closed = check(&["check", "--files.write", ".", "--dot-files-closed", "."]);
    assert!(closed.contains("~/app/.git"), "{closed}");
    let opened = check(&[
        "check",
        "--files.write",
        ".",
        "--files.write_dot_files",
        ".",
        "--dot-files-open",
        ".",
    ]);
    assert!(opened.contains("~/app/.envrc"), "{opened}");
    // Asked open where it's closed: not as asked.
    let output = project
        .lnk(&["check", "--files.write", ".", "--dot-files-open", "."])
        .output()
        .unwrap();
    assert!(!output.status.success());
    // The repository is as it was.
    assert!(project.dir.join(".git/config").is_file());
    let left: Vec<_> = std::fs::read_dir(&project.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("lnk-check"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// In an agent's Home, Link's own data folder, nothing is made where a
/// dot file isn't: the program sees no placeholder, may make one, and
/// what it made is gone when its run ends.
#[test]
fn in_an_agents_home_nothing_is_made_and_what_a_program_made_is_swept() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let project = Project::new("swept");
    let home = project.home.join("Link/Agents/main/home");
    std::fs::create_dir_all(&home).unwrap();
    let mut command = project.lnk(&[
        "run",
        "--commands.toolchains",
        "shell",
        "--commands.toolchains",
        "git",
        "--files.write",
        ".",
        "--",
        "/bin/bash",
        "-c",
        "ls -A; mkdir -p .git/hooks .vscode .idea && echo x > .git/hooks/pre-commit \
         && echo x > .envrc && echo made",
    ]);
    let output = command.current_dir(&home).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stdout.trim(), "made", "no placeholder seen inside");
    let left: Vec<_> = std::fs::read_dir(&home)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "swept when the run ended: {left:?}");
}
