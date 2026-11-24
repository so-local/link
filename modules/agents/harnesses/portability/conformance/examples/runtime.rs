//! Controlled runtime observations, using built programs. Never builds a target.

#[cfg(target_os = "linux")]
#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[cfg(target_os = "linux")]
#[derive(clap::Subcommand)]
enum Command {
    Report {
        record: std::path::PathBuf,
        trace: std::path::PathBuf,
    },
    Run {
        bin: std::path::PathBuf,
        output: std::path::PathBuf,
        #[arg(value_parser = ["mini", "link"])]
        harness: String,
        #[arg(value_enum)]
        mode: Mode,
        source: String,
        #[arg(default_value_t = 1)]
        agents: usize,
        #[arg(value_parser = ["traced"])]
        traced: Option<String>,
    },
}
#[cfg(target_os = "linux")]
#[derive(clap::ValueEnum, Clone)]
enum Mode {
    Confined,
    Diagnostic,
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use link_harness_conformance::runtime;
    use link_measure_contract::client::record::{Executions, Phase};
    let Cli { command } = link_spec::entry::plain();
    if let Command::Report { record, trace } = command {
        let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(record)?)?;
        let phases: Vec<Phase> = serde_json::from_value(report["phases"].clone())?;
        let trace = std::fs::read_to_string(trace)?;
        report["executions"] =
            serde_json::json!(phases.windows(2).map(|pair| {
            let counts = Executions::in_window(&trace, pair[0].unix_ns, pair[1].unix_ns, &[]);
            serde_json::json!({"from": pair[0].name, "to": pair[1].name, "counts": counts})
        }).collect::<Vec<_>>());
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let Command::Run {
        bin,
        output,
        harness,
        mode,
        source,
        agents: count,
        traced,
    } = command
    else {
        unreachable!("report handled")
    };
    let reaper = link_test_kit::process::Subreaper::for_fixture()?;
    let mode = match mode {
        Mode::Confined => runtime::Mode::Confined,
        Mode::Diagnostic => runtime::Mode::Diagnostic,
    };
    link_fetch::install_crypto_provider();
    let mut artifacts = serde_json::Map::new();
    for entry in std::fs::read_dir(&bin)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name == "lnk" || name.starts_with("lnk-"))
            && !name.contains('.')
            && entry.file_type()?.is_file()
        {
            let file = std::fs::File::open(entry.path())?;
            artifacts.insert(
                name,
                serde_json::json!({"bytes": file.metadata()?.len(),
                "sha256": link_digest::sha256_of(file)?}),
            );
        }
    }
    let began = std::time::Instant::now();
    let mut report = runtime::run(&bin, &harness, mode, count).await;
    link_test_kit::wait::eventually(
        "the fixture's adopted children reaped",
        std::time::Duration::from_secs(10),
        || {
            reaper.reaped().and_then(|done| {
                if done {
                    Ok(())
                } else {
                    Err(std::io::Error::other("an adopted child is still running"))
                }
            })
        },
    );
    report["adopted_children_reaped"] = serde_json::json!(reaper.reaped()?);
    report["fixture_elapsed_ms"] = serde_json::json!(began.elapsed().as_secs_f64() * 1000.0);
    report["artifacts"] = artifacts.into();
    report["runner_platform"] = serde_json::json!(format!(
        "{}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    report["kernel"] = serde_json::json!(
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|s| s.trim().to_string())
    );
    report["source"] = serde_json::json!(source);
    report["profile"] = serde_json::json!(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    report["traced"] = serde_json::json!(traced.is_some());
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", report["outcome"]);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("runtime observation fixture currently requires Linux");
    std::process::exit(2);
}
