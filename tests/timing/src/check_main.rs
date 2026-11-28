//! `lnk-check`: a workspace's checks as one command, `full` or `session`.

use link_test_timing::checks::{self, Checks, Verdict};
use link_test_timing::{Context, Metadata, Phase, PhaseMeasurements};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(clap::Parser)]
#[command(about = "Run the workspace's checks, as its .cargo/checks.toml says")]
struct Cli {
    #[command(subcommand)]
    check: Check,
}

#[derive(clap::Subcommand)]
enum Check {
    /// Every check over the whole workspace: once per initiative, before
    /// it merges.
    Full,
    /// The quick checks, then the named packages' Clippy and tests.
    Session {
        /// A package the change touched; given once per package.
        #[arg(long = "package", short = 'p')]
        packages: Vec<String>,
    },
    /// One test run as many copies at once, to find a flake again or
    /// prove its fix.
    Copies {
        /// The test's package.
        #[arg(long, short = 'p')]
        package: String,
        /// The test's whole name, as libtest names it.
        test: String,
        /// How many copies, past checks.toml's `copies`.
        #[arg(long)]
        copies: Option<usize>,
    },
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let given: Cli = link_spec::entry::plain();
    let root = workspace()?;
    let checks = Checks::parse(&std::fs::read_to_string(root.join(".cargo/checks.toml"))?)?;
    let (name, steps, packages) = match given.check {
        Check::Full => ("full", &checks.full, Vec::new()),
        Check::Session { packages } => ("session", &checks.session, packages),
        Check::Copies {
            package,
            test,
            copies,
        } => return copies_of(&root, &package, &test, copies.unwrap_or(checks.copies)),
    };
    println!("{name} check of {}", root.display());
    let started = Instant::now();
    let done = checks::run(&root, steps, &packages);
    let elapsed = started.elapsed().as_secs_f64();
    let failed = done
        .iter()
        .filter(|(_, v, _)| matches!(v, Verdict::Failed(_)))
        .count();
    let skipped = done
        .iter()
        .filter(|(_, v, _)| matches!(v, Verdict::Skipped(_)))
        .count();
    if let Some((step, verdict, seconds)) = steps
        .iter()
        .zip(&done)
        .find(|(step, _)| step.tests)
        .map(|(step, (_, verdict, seconds))| (step, verdict, *seconds))
        .filter(|(_, verdict, _)| !matches!(verdict, Verdict::Skipped(_)))
    {
        let phases = PhaseMeasurements {
            schema: 1,
            elapsed_seconds: Some(elapsed),
            cache_state: None,
            phases: done
                .iter()
                .map(|(name, verdict, seconds)| Phase {
                    name: name.clone(),
                    wall_seconds: (!matches!(verdict, Verdict::Skipped(_))).then_some(*seconds),
                    status: match verdict {
                        Verdict::Passed => Some(0),
                        Verdict::Failed(code) => Some(*code),
                        Verdict::Skipped(_) => None,
                    },
                })
                .collect(),
        };
        let status = match verdict {
            Verdict::Failed(code) => *code,
            _ => 0,
        };
        timing(
            &root,
            &step.name,
            status,
            seconds,
            phases,
            checks.flakes.as_deref(),
        )?;
    }
    println!(
        "\n{name} check: {} passed, {failed} failed, {skipped} skipped, in {:.0}s",
        done.len() - failed - skipped,
        elapsed
    );
    Ok(failed == 0)
}

/// Runs `test` as `copies` copies at once and says how many failed.
fn copies_of(
    root: &Path,
    package: &str,
    test: &str,
    copies: usize,
) -> Result<bool, Box<dyn std::error::Error>> {
    let program = link_test_timing::copies::program(root, package, test)?;
    let logs = root
        .join("target/check/copies")
        .join(test.replace("::", "-"));
    let _ = std::fs::remove_dir_all(&logs);
    let how = match link_test_timing::copies::is_system(&program)? {
        true => "through its runner, as wide as the suite",
        false => "at once",
    };
    println!("{test}: {copies} copies, {how}");
    let done = link_test_timing::copies::run(&program, test, copies, &logs)?;
    println!("{} of {} copies failed", done.failed, done.ran);
    if let Some(first) = &done.first {
        println!("the first failure's output: {}", first.display());
    }
    Ok(done.failed == 0)
}

/// Renders the test step's saved log as Test Timings' report, and prints
/// whose each failure is.
fn timing(
    root: &Path,
    step: &str,
    status: i32,
    seconds: f64,
    phases: PhaseMeasurements,
    flakes: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let log = std::fs::read_to_string(root.join("target/check").join(format!("{step}.log")))?;
    let metadata = checks::program("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()?;
    let metadata: Metadata = serde_json::from_slice(&metadata.stdout)?;
    let revision = checks::program("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|rev| !rev.is_empty());
    let context = Context {
        revision,
        machine: None,
        threads: std::env::var("RUST_TEST_THREADS").ok(),
        status: Some(status),
        wall_seconds: Some(seconds),
        filtered_skip_reason: None,
    };
    let mut report = link_test_timing::report(&log, &metadata, context);
    report.validation = Some(phases);
    let output = root.join("target/check/report");
    std::fs::create_dir_all(&output)?;
    std::fs::write(
        output.join("timings.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    std::fs::write(
        output.join("index.html"),
        link_test_timing::html(&report, None),
    )?;
    println!("\ntimings: {}", output.join("index.html").display());
    let known = match flakes
        .map(|path| root.join(path))
        .filter(|path| path.exists())
    {
        Some(path) => link_test_timing::check::flakes(&std::fs::read_to_string(path)?)?,
        None => Vec::new(),
    };
    let signatures = link_test_timing::check::signatures();
    let sorted = link_test_timing::check::sort(&report, None, &known, &signatures);
    if !sorted.is_empty() {
        print!("{}", link_test_timing::check::text(&sorted, &signatures));
    }
    Ok(())
}

/// The root of the workspace this runs in, as Cargo finds it.
fn workspace() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let found = checks::program("cargo")
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .output()?;
    let manifest = PathBuf::from(String::from_utf8(found.stdout)?.trim());
    Ok(manifest
        .parent()
        .ok_or("cargo named no workspace")?
        .to_path_buf())
}
