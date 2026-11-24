//! The checks a workspace runs, as one command: `lnk-check full` runs
//! every check over the whole workspace, `lnk-check session` the quick
//! ones and the named packages' tests.
//!
//! What each runs is data, the workspace's `.cargo/checks.toml`: its
//! steps in order, each a command, the programs and paths it needs and
//! whether it is the test command. A step whose need is missing is
//! skipped, saying what it lacks. Every step runs, even after one fails,
//! so one failure never hides another. Each step's output is saved under
//! `target/check/`, the test command's as [Test Timings](crate) reads it,
//! and its report sorts each failure as the change's, a known flake's or
//! the machine's.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A workspace's `.cargo/checks.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checks {
    pub schema: u32,
    /// How many copies of one test `lnk-check copies` runs at once.
    pub copies: usize,
    /// The workspace's list of known flakes, from its root, whose tests
    /// the report sorts apart.
    pub flakes: Option<PathBuf>,
    /// Every check over the whole workspace.
    pub full: Vec<Step>,
    /// The quick checks, then the named packages'.
    pub session: Vec<Step>,
}

/// One command of a check.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Its name, which names its log.
    pub name: String,
    /// The command and its arguments. `{packages}` stands for `-p <name>`
    /// for each package named, and a step holding it runs only when one is.
    pub run: Vec<String>,
    /// The folder it runs in, from the workspace's root.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Programs it runs, found on `PATH`, else the step is skipped.
    #[serde(default)]
    pub programs: Vec<String>,
    /// Paths it reads, from the workspace's root, else the step is skipped.
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    /// Rust targets it builds for, installed in the toolchain's sysroot,
    /// else the step is skipped.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Whether it is the test command, whose output is saved for the
    /// timing report.
    #[serde(default)]
    pub tests: bool,
}

impl Checks {
    /// From a `checks.toml`'s text.
    pub fn parse(text: &str) -> Result<Checks, String> {
        let checks: Checks = toml::from_str(text).map_err(|e| format!("checks.toml: {e}"))?;
        if checks.schema != 1 {
            return Err(format!("checks.toml: unsupported schema {}", checks.schema));
        }
        Ok(checks)
    }
}

/// What a step came to.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    Passed,
    Failed(i32),
    /// Not run, and why.
    Skipped(String),
}

/// A step's command with its packages put in, or why it doesn't run.
pub fn command(step: &Step, packages: &[String]) -> Result<Vec<String>, String> {
    let mut command = Vec::new();
    for word in &step.run {
        if word == "{packages}" {
            if packages.is_empty() {
                return Err("no package named".into());
            }
            for package in packages {
                command.push("-p".to_string());
                command.push(package.clone());
            }
        } else {
            command.push(word.clone());
        }
    }
    Ok(command)
}

/// What `step` lacks here, under `root`, if anything: a program on
/// `path`, a path, or a target in the toolchain's `sysroot`.
pub fn lacks(
    step: &Step,
    root: &Path,
    path: Option<&std::ffi::OsStr>,
    sysroot: Option<&Path>,
) -> Option<String> {
    for target in &step.targets {
        let installed = sysroot.is_some_and(|sysroot| {
            sysroot
                .join("lib/rustlib")
                .join(target)
                .join("lib")
                .is_dir()
        });
        if !installed {
            return Some(format!("lacks the Rust target {target}"));
        }
    }
    for program in &step.programs {
        let found = path
            .map(|path| std::env::split_paths(path).any(|dir| dir.join(program).is_file()))
            .unwrap_or(false);
        if !found {
            return Some(format!("lacks {program}"));
        }
    }
    step.paths
        .iter()
        .find(|path| !root.join(path).exists())
        .map(|path| format!("lacks {}", path.display()))
}

/// Runs `steps` in `root`, printing a line for each as it ends, and saves
/// their output in `target/check/`. Answers each step's verdict and time.
pub fn run(root: &Path, steps: &[Step], packages: &[String]) -> Vec<(String, Verdict, f64)> {
    let logs = root.join("target/check");
    let _ = std::fs::create_dir_all(&logs);
    let path = std::env::var_os("PATH");
    let sysroot = program("rustc")
        .args(["--print", "sysroot"])
        .current_dir(root)
        .output()
        .ok()
        .map(|out| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()));
    let mut done = Vec::new();
    for step in steps {
        let words = match command(step, packages) {
            Ok(words) => words,
            Err(why) => {
                report(&step.name, &Verdict::Skipped(why.clone()), 0.0);
                done.push((step.name.clone(), Verdict::Skipped(why), 0.0));
                continue;
            }
        };
        if let Some(why) = lacks(step, root, path.as_deref(), sysroot.as_deref()) {
            report(&step.name, &Verdict::Skipped(why.clone()), 0.0);
            done.push((step.name.clone(), Verdict::Skipped(why), 0.0));
            continue;
        }
        let log = logs.join(format!("{}.log", step.name));
        let started = Instant::now();
        let verdict = match execute(root, step, &words, &log) {
            Ok(0) => Verdict::Passed,
            Ok(code) => Verdict::Failed(code),
            Err(error) => Verdict::Skipped(format!("couldn't start: {error}")),
        };
        let seconds = started.elapsed().as_secs_f64();
        report(&step.name, &verdict, seconds);
        if matches!(verdict, Verdict::Failed(_)) {
            println!("  {}", log.display());
            for line in tail(&log, 12) {
                println!("  | {line}");
            }
        }
        done.push((step.name.clone(), verdict, seconds));
    }
    done
}

fn execute(root: &Path, step: &Step, words: &[String], log: &Path) -> std::io::Result<i32> {
    let out = std::fs::File::create(log)?;
    let err = out.try_clone()?;
    let dir = step
        .dir
        .as_ref()
        .map_or(root.to_path_buf(), |dir| root.join(dir));
    let status = program(&words[0])
        .args(&words[1..])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err)
        .status()?;
    Ok(status.code().unwrap_or(101))
}

/// A check's command, run as the person who typed `lnk-check` would run
/// it, with their whole environment.
#[expect(
    clippy::disallowed_methods,
    reason = "a check runs as the person typing it would, with their environment whole"
)]
pub fn program(name: &str) -> std::process::Command {
    std::process::Command::new(name)
}

fn report(name: &str, verdict: &Verdict, seconds: f64) {
    let said = match verdict {
        Verdict::Passed => format!("ok      {seconds:>7.1}s"),
        Verdict::Failed(code) => format!("FAILED  {seconds:>7.1}s  (exit {code})"),
        Verdict::Skipped(why) => format!("skipped           {why}"),
    };
    println!("{name:<14} {said}");
}

fn tail(log: &Path, lines: usize) -> Vec<String> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(run: &[&str]) -> Step {
        Step {
            name: "s".into(),
            run: run.iter().map(|w| w.to_string()).collect(),
            dir: None,
            programs: vec![],
            paths: vec![],
            targets: vec![],
            tests: false,
        }
    }

    #[test]
    fn packages_stand_where_the_step_says() {
        let given = ["link-agent".to_string(), "link-box".to_string()];
        assert_eq!(
            command(
                &step(&["cargo", "test", "{packages}", "--no-fail-fast"]),
                &given
            )
            .unwrap(),
            [
                "cargo",
                "test",
                "-p",
                "link-agent",
                "-p",
                "link-box",
                "--no-fail-fast"
            ]
        );
        assert_eq!(
            command(&step(&["cargo", "test", "{packages}"]), &[]).unwrap_err(),
            "no package named"
        );
        assert_eq!(
            command(&step(&["cargo", "fmt"]), &[]).unwrap(),
            ["cargo", "fmt"]
        );
    }

    #[test]
    fn a_step_lacking_a_program_or_path_says_which() {
        let root = link_test_kit::TempDir::new("checks");
        let mut needs = step(&["shellcheck"]);
        needs.programs = vec!["no-such-program-here".into()];
        assert_eq!(
            lacks(&needs, root.path(), Some("/usr/bin:/bin".as_ref()), None).as_deref(),
            Some("lacks no-such-program-here")
        );
        let mut reads = step(&["npm"]);
        reads.paths = vec!["web/node_modules".into()];
        assert_eq!(
            lacks(&reads, root.path(), None, None).as_deref(),
            Some("lacks web/node_modules")
        );
        std::fs::create_dir_all(root.path().join("web/node_modules")).unwrap();
        assert_eq!(lacks(&reads, root.path(), None, None), None);
        let mut builds = step(&["cargo"]);
        builds.targets = vec!["x86_64-unknown-linux-musl".into()];
        assert_eq!(
            lacks(&builds, root.path(), None, Some(root.path())).as_deref(),
            Some("lacks the Rust target x86_64-unknown-linux-musl")
        );
    }

    /// The workspace's own checks read, and its full check runs the four
    /// commands over the whole workspace, the test command among them.
    #[test]
    fn the_workspaces_checks_read() {
        let text = include_str!("../../../.cargo/checks.toml");
        let checks = Checks::parse(text).unwrap();
        let names: Vec<&str> = checks.full.iter().map(|s| s.name.as_str()).collect();
        for name in ["fmt", "clippy", "build", "test"] {
            assert!(names.contains(&name), "{names:?}");
        }
        assert_eq!(checks.full.iter().filter(|s| s.tests).count(), 1);
        assert_eq!(checks.session.iter().filter(|s| s.tests).count(), 1);
    }
}
