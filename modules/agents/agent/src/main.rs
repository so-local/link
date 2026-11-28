//! `lnk-agent`: the harness module's commands. `lnk` runs it for `lnk
//! agent ...`.

use std::path::PathBuf;

use link_agent::{bridge, connections, interaction_mcp, moved, service, settings};
use link_format::WithinExt;

fn schedules_report(
    out: &mut link_format::Out,
    schedules: &link_harness_contract::asp::schedules::Schedules,
) {
    for job in &schedules.carried {
        out.block(link_format::Note(link_format::Sentence::adopt(&format!(
            "Schedule {} is carried by Link.",
            link_agent::schedules::words(job)
        ))));
    }
    for job in &schedules.not_carried {
        out.block(link_format::Report::warning(link_format::Sentence::adopt(
            &format!(
                "Schedule {:?} ({}) is not carried: {}.",
                job.name, job.job, job.reason
            ),
        )));
    }
}
#[cfg(test)]
link_test_kit::program!(unit);

mod action_routes;
mod answering;
mod confine;
mod connecting;
mod context_mcp;
mod conversations;
mod creating;
mod defined;
mod dispatch;
mod environment;
mod following;
mod granting;
mod inspecting;
mod launching;
mod listing;
mod live;
mod machine;
mod mcp_tool;
mod memory;
mod migrate;
mod model;
mod moving;
mod namespace_cli;
mod namespaces;
mod naming;
#[cfg(test)]
mod navigation;
mod permissions;
mod question_mcp;
mod questions_cli;
mod read_sources;
mod readout;
mod removing;
mod restoring;
mod run_file;
mod scheduler;
mod screens;
mod services;
mod shared;
mod shares;
mod slots;
mod spec_file;
mod state_export;
mod status;
mod stopping;
mod switching;
mod teams;
mod tools;
mod using;

/// `names` as a sentence: `Telegram and Slack`.
fn and_list<S: AsRef<str>>(names: &[S]) -> String {
    let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    let saved: Vec<fn() -> link_files::upgrade::Saved> = link_agent::SAVED
        .into_iter()
        .chain([
            moving::journal::saved as fn() -> _,
            teams::defining::saved,
            teams::moving::saved,
            environment::saved,
            machine::saved,
            shares::saved,
            defined::saved,
            shared::saved,
            memory::defining::saved,
            memory::recording::saved_record,
            memory::recording::saved_seen,
            link_interactions::saved_approvals,
            link_interactions::chains::saved,
            link_memory_tool_contract::saved_agents,
            link_scheduler_tool_contract::listed::saved,
            link_harness_contract::harness::saved_shared_agents,
        ])
        .collect();
    // The guest service, which this program runs, keeps its state on Linux.
    #[cfg(target_os = "linux")]
    let saved = [saved, vec![link_service::saved_guest_state as fn() -> _]].concat();
    link_core::saved::own(&saved);
    link_core::describe_component!(link_agent_spec::Spec, link_agent_spec::TeamFile);
    // `permission add` and `remove` take the sandbox spec's field flags.
    link_core::descriptor::uses(&[|| link_spec::Exported::of::<link_sandbox_contract::Spec>()]);
    // A person names an agent; every action takes its ID.
    link_actions::cli::names(settings::ids::KIND, naming::lookup);
    link_settings::output::run(|out| {
        let parsed = link_agent::cli::parse();
        let parsed = match parsed.command.command {
            link_agent::cli::Command::GuestService(given) => {
                link_service::guest_service_entry(given)?;
                return Ok(link_format::Done::shown());
            }
            _ => parsed,
        };
        if let link_agent::cli::Command::Answer(question) = &parsed.command.command {
            crate::answering::answer(question)?;
            return Ok(link_format::Done::shown());
        }
        let mut runtime = match memory_process(&parsed.command.command) {
            true => tokio::runtime::Builder::new_current_thread(),
            false => tokio::runtime::Builder::new_multi_thread(),
        };
        let runtime = runtime
            .enable_all()
            .build()
            .within("starting its runtime")?;
        runtime.block_on(dispatch::try_main(out, parsed))
    })
}

/// Whether `command` runs an agent's memory: `memory serve`, `connect` or
/// `service`.
fn memory_process(command: &link_agent::cli::Command) -> bool {
    use link_agent::cli::Command;
    matches!(
        command,
        Command::MemoryServe(_) | Command::MemoryConnect(_) | Command::MemoryService(_)
    )
}

/// A report as an `anyhow` error, for a caller that takes no report: its
/// headline and why, no more.
pub(crate) fn lossy<T>(result: Result<T, link_format::Report>) -> anyhow::Result<T> {
    result.map_err(|report| {
        let mut said = report.headline.to_string();
        if let Some(why) = &report.why {
            said.push(' ');
            said.push_str(&why.to_string());
        }
        anyhow::anyhow!(said)
    })
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => link_files::home_dir().join(rest),
        None => PathBuf::from(path),
    }
}

fn absolute(path: PathBuf) -> anyhow::Result<PathBuf> {
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    })
}

/// `~/Link` for display.
fn tilde(path: &std::path::Path) -> String {
    match path.strip_prefix(link_files::home_dir()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(unix)]
mod stops;

#[cfg(test)]
mod tests {
    use super::*;

    /// Link's settings in a folder of this run's, never this machine's
    /// `~/.lnk`, as the library's tests have them: held, one test
    /// that reads an agent's files at a time.
    pub(crate) fn own_config() -> std::sync::MutexGuard<'static, ()> {
        static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ONE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// An agent's memory runs on the current thread's runtime; any other
    /// command on one with a worker per core.
    #[test]
    fn memory_runs_without_a_worker_per_core() {
        use clap::Parser;
        let args = |line: &str| {
            link_agent::cli::Cli::try_parse_from(
                std::iter::once("lnk-agent").chain(line.split_whitespace()),
            )
            .unwrap()
            .command
        };
        assert!(memory_process(&args(
            "memory connect /d --agent-id a1b2c3d4e5f6"
        )));
        assert!(memory_process(&args(
            "-a agent:k7m2p9q4r8s3 memory service /d 0123"
        )));
        assert!(memory_process(&args("memory serve --model m -- x")));
        assert!(!memory_process(&args("memory start")));
        assert!(!memory_process(&args("start")));
    }
}
