//! The sandbox spec cases, `tests/specs/sandbox/*.toml`: each a spec as
//! a user writes it (`[spec]`), the flags beside it and the folders its
//! home holds (`[run]`), and what
//! must come of it (`[expect]`): the hosts it `reaches` and is `refused`,
//! the programs it `runs` and doesn't (`refused_commands`), the folders
//! whose dot files it writes and doesn't (`dot_files_open`,
//! `dot_files_closed`), tried from
//! inside a real sandbox by `lnk sandbox check --spec`, which also fails on
//! any of its own probes leaking; or what reading it says when it's
//! `invalid`. A new case is a new file.

use std::path::{Path, PathBuf};
use std::process::Command;

use link_test_kit::TempDir;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    #[serde(default)]
    spec: Option<toml::Table>,
    #[serde(default)]
    run: Run,
    expect: Expect,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Run {
    /// Flags beside `--spec`.
    #[serde(default)]
    flags: Vec<String>,
    /// Run as on a cloud's machine (`LNK_TEST_ON_CLOUD=1`).
    #[serde(default)]
    cloud: bool,
    /// Folders in its home before it runs (`~/app`), as a stage before
    /// it, or a setup outside any sandbox, left them.
    #[serde(default)]
    folders: Vec<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct Expect {
    /// What reading or checking it says, when it's refused before it runs.
    invalid: Option<String>,
    /// `host[:port]`s (443 if none) its proxy must take.
    #[serde(default)]
    reaches: Vec<String>,
    /// `host[:port]`s it must not reach.
    #[serde(default)]
    refused: Vec<String>,
    /// Programs it must run, by name on `PATH` or by path.
    #[serde(default)]
    runs: Vec<String>,
    /// Programs it must not.
    #[serde(default)]
    refused_commands: Vec<String>,
    /// Folders it writes, from the case's folder, whose dot files that
    /// run outside it must write too: each made a repository first.
    #[serde(default)]
    dot_files_open: Vec<String>,
    /// Folders it writes whose dot files it must not.
    #[serde(default)]
    dot_files_closed: Vec<String>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A folder of its own for each case, outside `/tmp`, which a spec may
/// neither read nor write: its home, `home`, and the spec's, `case`.
fn folder(name: &str) -> TempDir {
    let dir = TempDir::under(
        Path::new(env!("CARGO_TARGET_TMPDIR")),
        &format!("sandbox-spec-{name}"),
    );
    std::fs::create_dir_all(dir.join("home")).unwrap();
    std::fs::create_dir_all(dir.join("case")).unwrap();
    dir
}

/// `lnk-sandbox <args>` in `dir/case`, its home `dir/home`, with
/// nothing of this machine's settings.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn sandbox(dir: &Path, args: &[String], cloud: bool) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .args(args)
        .env_clear()
        .envs(cloud.then_some(("LNK_TEST_ON_CLOUD", "1")))
        .env("HOME", dir.join("home"))
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&dir.join("home")),
        )
        .env("PATH", "/usr/bin:/bin")
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&dir.join("home")))
        .current_dir(dir.join("case"))
        .output()
        .unwrap()
}

/// What's wrong with how `case` came out, if anything; None when it's
/// skipped (it needs the sandbox, which can't run here).
fn run(name: &str, case: &Case, sandboxed: bool) -> Option<String> {
    came_out(&folder(name), case, sandboxed)
}

fn came_out(dir: &Path, case: &Case, sandboxed: bool) -> Option<String> {
    let dir = dir.to_path_buf();
    for folder in &case.run.folders {
        let inside = folder
            .strip_prefix("~/")
            .filter(|f| !f.split('/').any(|part| part == ".."))
            .unwrap_or_else(|| panic!("{folder}: a folder in its home is ~/<folder>"));
        std::fs::create_dir_all(dir.join("home").join(inside)).unwrap();
    }
    let text = match &case.spec {
        Some(spec) => toml::to_string(spec).unwrap(),
        None => String::new(),
    };
    std::fs::write(dir.join("case/spec.toml"), text).unwrap();
    let mut args: Vec<String> = ["check", "--spec", "spec.toml"].map(String::from).to_vec();
    args.extend(case.run.flags.iter().cloned());
    let expect = &case.expect;
    for (flag, values) in [
        ("--reaches", &expect.reaches),
        ("--refuses", &expect.refused),
        ("--runs", &expect.runs),
        ("--refuses-program", &expect.refused_commands),
        ("--dot-files-open", &expect.dot_files_open),
        ("--dot-files-closed", &expect.dot_files_closed),
    ] {
        for value in values {
            args.extend([flag.to_string(), value.clone()]);
        }
    }
    // A project for each, a repository with its settings and hooks.
    for folder in expect.dot_files_open.iter().chain(&expect.dot_files_closed) {
        let project = dir.join("case").join(folder);
        std::fs::create_dir_all(project.join(".git/hooks")).unwrap();
        std::fs::write(project.join(".git/config"), "[core]\n").unwrap();
    }
    if let Some(want) = &expect.invalid {
        // Refused before anything runs: no sandbox needed.
        let output = sandbox(&dir, &args, case.run.cloud);
        let err = String::from_utf8_lossy(&output.stderr);
        return match (output.status.success(), err.contains(want.as_str())) {
            (false, true) => None,
            _ => Some(format!("expected it refused with {want:?}, got: {err}")),
        };
    }
    if !sandboxed {
        return None;
    }
    // Ask the same spec resolver that the check uses, including flags
    // and OS overrides. Only executable restrictions require Landlock.
    // Invalid specs above always run, even without sandbox capabilities.
    #[cfg(target_os = "linux")]
    {
        let mut spec_args: Vec<String> = ["spec", "show", "--spec", "spec.toml"]
            .map(String::from)
            .to_vec();
        spec_args.extend(case.run.flags.iter().cloned());
        let resolved = sandbox(&dir, &spec_args, case.run.cloud);
        if !resolved.status.success() {
            return Some(format!(
                "resolving the effective spec: {}",
                String::from_utf8_lossy(&resolved.stderr)
            ));
        }
        let spec: link_sandbox_client::Spec =
            link_spec::parse_spec(&String::from_utf8_lossy(&resolved.stdout)).unwrap();
        let policy = link_sandbox_client::Confinement::of(
            &spec,
            link_sandbox_client::At {
                name: "case",
                dir: &dir.join("case"),
                home: &dir.join("home"),
                path: None,
                way: link_sandbox_client::Way::Proxy(Default::default()),
            },
        )
        .unwrap();
        if matches!(policy.commands, link_sandbox_client::Programs::Only(_)) && !crate::landlock() {
            return None;
        }
    }
    let output = sandbox(&dir, &args, case.run.cloud);
    let out = String::from_utf8_lossy(&output.stdout);
    let tried = expect.reaches.len()
        + expect.refused.len()
        + expect.runs.len()
        + expect.refused_commands.len()
        + expect.dot_files_open.len()
        + expect.dot_files_closed.len();
    let shown = ["reach ", "run ", "/.git"].iter().any(|s| out.contains(s));
    match output.status.success() {
        true if tried > 0 && !shown => Some(format!("its expectations weren't tried:\n{out}")),
        true => None,
        false => Some(format!(
            "a probe leaked, or what it should reach didn't:\n{out}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )),
    }
}

#[test]
fn every_sandbox_spec_case_comes_out_as_it_says() {
    if !link_test_kit::out_of_tmp(Path::new(env!("CARGO_TARGET_TMPDIR"))) {
        return;
    }
    let dir = root().join("tests/specs/sandbox");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    assert!(files.len() >= 10, "the cases in {}", dir.display());
    #[cfg(target_os = "linux")]
    let sandboxed = link_test_kit::gate::bubblewrap();
    #[cfg(not(target_os = "linux"))]
    let sandboxed = false;
    let cases: Vec<(String, Case)> = files
        .iter()
        .map(|file| {
            let name = file.file_stem().unwrap().to_string_lossy().to_string();
            let case = toml::from_str(&std::fs::read_to_string(file).unwrap())
                .unwrap_or_else(|err| panic!("{name}: not a case: {err}"));
            (name, case)
        })
        .collect();
    // Eight at a time, most of it waiting: each check is its own sandboxes,
    // home and wire.
    let mut failed = Vec::new();
    for chunk in cases.chunks(8) {
        std::thread::scope(|scope| {
            let running: Vec<_> = chunk
                .iter()
                .map(|(name, case)| (name, scope.spawn(|| run(name, case, sandboxed))))
                .collect();
            for (name, handle) in running {
                if let Some(why) = handle.join().unwrap() {
                    failed.push(format!("{name}: {why}"));
                }
            }
        });
    }
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

/// Every sandbox spec the docs show is one Link reads: each `toml` block
/// of the sandbox's pages and its spec's, and each a box's setup stage has on
/// the boxes' pages and theirs.
#[test]
fn every_sandbox_spec_in_the_docs_is_read() {
    let pages = |folder: &str| -> Vec<PathBuf> {
        std::fs::read_dir(root().join(folder))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect()
    };
    let pages: Vec<(PathBuf, bool)> = pages("modules/security/sandbox/docs/tooling/sandbox")
        .into_iter()
        .chain([root().join("modules/security/sandbox/docs/config/specs/sandbox.md")])
        .map(|p| (p, false))
        .chain(
            pages("modules/compute/box/docs/services/boxes")
                .into_iter()
                .chain([root().join("modules/compute/box/docs/config/specs/box.md")])
                .map(|p| (p, true)),
        )
        .collect();
    let dir = folder("docs");
    let mut read = 0;
    let mut stages = 0;
    for (page, boxes) in pages {
        let text = std::fs::read_to_string(&page).unwrap();
        let mut block: Option<String> = None;
        for line in text.lines() {
            match (&mut block, line.trim_start()) {
                (None, "```toml") => block = Some(String::new()),
                // A box spec: the boxes' test reads it.
                (Some(spec), "```") if boxes && !is_sandbox_spec(spec) => block = None,
                (Some(spec), "```") => {
                    stages += usize::from(boxes);
                    std::fs::write(dir.join("case/spec.toml"), &*spec).unwrap();
                    let output = sandbox(
                        &dir,
                        &[
                            "spec".into(),
                            "show".into(),
                            "--spec".into(),
                            "spec.toml".into(),
                        ],
                        false,
                    );
                    assert!(
                        output.status.success(),
                        "{}: {}\n{spec}",
                        page.display(),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    read += 1;
                    block = None;
                }
                (Some(spec), _) => {
                    spec.push_str(line);
                    spec.push('\n');
                }
                (None, _) => {}
            }
        }
    }
    assert!(read >= 2, "the docs' sandbox specs");
    assert!(stages >= 1, "the boxes' setup stages");
}

/// Whether `text` is a sandbox spec: a table with one of its sections.
fn is_sandbox_spec(text: &str) -> bool {
    const SECTIONS: &[&str] = &[
        "files",
        "internet",
        "lan",
        "localhost",
        "tools",
        "browser",
        "commands",
    ];
    toml::from_str::<toml::Table>(text).is_ok_and(|t| SECTIONS.iter().any(|s| t.contains_key(*s)))
}
