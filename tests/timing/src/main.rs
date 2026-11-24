//! Render a saved Cargo test run without executing any tests.

use link_test_timing::{Context, Metadata, PhaseMeasurements, Report};
use std::path::PathBuf;

#[derive(clap::Parser)]
#[command(about = "Render a saved Cargo test run without executing tests")]
struct Cli {
    #[arg(long)]
    log: PathBuf,
    #[arg(long)]
    metadata: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    status: Option<i32>,
    #[arg(long)]
    wall_seconds: Option<f64>,
    #[arg(long)]
    revision: Option<String>,
    #[arg(long)]
    machine: Option<String>,
    #[arg(long)]
    threads: Option<String>,
    #[arg(long)]
    baseline: Option<PathBuf>,
    #[arg(long)]
    filtered_skip_reason: Option<String>,
    #[arg(long)]
    phases: Option<PathBuf>,
    /// A workspace's flakes.toml, whose tests are sorted as known flakes;
    /// given once per workspace.
    #[arg(long)]
    flakes: Vec<PathBuf>,
    /// The base's saved report (its timings.json), whose failing tests are
    /// sorted as the base's.
    #[arg(long)]
    base: Option<PathBuf>,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let given: Cli = link_spec::entry::plain();
    let log = std::fs::read_to_string(given.log)?;
    let metadata: Metadata = serde_json::from_str(&std::fs::read_to_string(given.metadata)?)?;
    let output = given.output;
    let wall_seconds = given.wall_seconds;
    if wall_seconds.is_some_and(|s| !s.is_finite() || s < 0.0) {
        return Err("--wall-seconds must be finite and nonnegative".into());
    }
    if given
        .filtered_skip_reason
        .as_ref()
        .is_some_and(|reason| reason.trim().is_empty())
    {
        return Err("--filtered-skip-reason must explain the environment exclusions".into());
    }
    let context = Context {
        revision: given.revision,
        machine: given.machine,
        threads: given.threads,
        status: given.status,
        wall_seconds,
        filtered_skip_reason: given.filtered_skip_reason,
    };
    let baseline: Option<Report> = given
        .baseline
        .as_ref()
        .map(|path| -> Result<Report, Box<dyn std::error::Error>> {
            let report = Report::parse(&std::fs::read_to_string(path)?)?;
            if report.kind != link_test_timing::MeasurementKind::TestPrograms {
                return Err("the baseline must measure test programs".into());
            }
            Ok(report)
        })
        .transpose()?;
    let base: Option<Report> = given
        .base
        .as_ref()
        .map(|path| -> Result<Report, Box<dyn std::error::Error>> {
            Ok(Report::parse(&std::fs::read_to_string(path)?)?)
        })
        .transpose()?;
    let mut flakes = Vec::new();
    for path in &given.flakes {
        flakes.extend(link_test_timing::check::flakes(&std::fs::read_to_string(
            path,
        )?)?);
    }
    let mut report = link_test_timing::report(&log, &metadata, context);
    report.validation = given
        .phases
        .as_ref()
        .map(|path| -> Result<_, Box<dyn std::error::Error>> {
            Ok(PhaseMeasurements::parse(&std::fs::read_to_string(path)?)?)
        })
        .transpose()?;
    std::fs::create_dir_all(&output)?;
    std::fs::write(
        output.join("timings.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    std::fs::write(
        output.join("index.html"),
        link_test_timing::html(&report, baseline.as_ref()),
    )?;
    println!(
        "{:.2} measured program seconds across {} programs; complete: {}\n{}",
        report.measured_program_seconds,
        report.programs.len(),
        report.complete,
        output.join("index.html").display()
    );
    let signatures = link_test_timing::check::signatures();
    let sorted = link_test_timing::check::sort(&report, base.as_ref(), &flakes, &signatures);
    if !sorted.is_empty() {
        print!("{}", link_test_timing::check::text(&sorted, &signatures));
    }
    Ok(())
}
