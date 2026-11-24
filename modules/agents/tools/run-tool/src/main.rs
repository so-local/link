//! `lnk-run-tool`: Link run, an agent's programs, each call in a sandbox
//! of its own (`link_run_tool`):
//!
//! ```sh
//! lnk-run-tool serve --files <folder> --programs-path <PATH> --timeout <duration> \
//!   --output-bytes <bytes> --program <program>... [--var <NAME=VALUE>]... \
//!   [--state <dir> --way-in <socket> --as <agent id> [--keyring <dir>] --background <n> --background-timeout <duration> \
//!    --background-output <bytes> --background-keep <duration> --background-per-day <n> \
//!    --hops <n>] -- <sandbox>...
//! lnk-run-tool supervise <id> --state <dir>   # one background command, to its end
//! lnk-run-tool gate -- <command>...           # it, once its supervisor names its group
//! lnk-run-tool sweep --state <dir>            # the lost found, the ended told, the old removed
//! lnk-run-tool stop --state <dir>             # every one running stopped, lost: the agent stopped
//! lnk-run-tool remove --state <dir>           # every one stopped and removed
//! ```
//!
//! The timeout and the output kept are the agent's spec's (`[tool.run]`),
//! the programs its sandbox file's (`[commands]`), and the sandbox
//! `lnk sandbox run` with the flags that file makes, so none is this
//! program's own.

use std::time::Duration;

use anyhow::Context;
use link_format::{
    Done, Name, Note, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence,
};
use link_run_tool::cli::{self, Command, RunToolServeArgs};
use link_run_tool::{Background, Run, background, serve};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

fn run_of(serve: RunToolServeArgs) -> anyhow::Result<Run> {
    let duration = |given: &link_actions::value::Duration| -> anyhow::Result<Duration> {
        Some(given.get())
            .filter(|t| !t.is_zero())
            .with_context(|| format!("{given} isn't a duration longer than none"))
    };
    let context = serve
        .with_env
        .then_some(link_run_tool_contract::context::ENV)
        .map(|name| -> anyhow::Result<_> {
            let value =
                link_secretive::Secret::from_env(name).context("reading the run server context")?;
            let context: link_run_tool_contract::context::Context =
                serde_json::from_str(value.expose())
                    .context("reading the typed run server context")?;
            context.check()?;
            anyhow::ensure!(
                !serve
                    .var
                    .iter()
                    .any(|var| var.split_once('=').is_some_and(|(key, _)| key == name)),
                "run context cannot be a task variable"
            );
            Ok(std::sync::Arc::new(context))
        })
        .transpose()?;
    let env: Vec<(String, String)> = serve
        .var
        .iter()
        .map(|given| link_run_tool::variable(given))
        .collect::<anyhow::Result<_>>()?;
    if let Some(context) = &context {
        context.check_variables(&env)?;
    }
    // Background commands: all of their flags, or none (its check hook
    // holds them to `--state`).
    let background = match serve.state {
        None => None,
        Some(state) => {
            let missing = || anyhow::anyhow!(Usage);
            Some(Background {
                program: std::env::current_exe().context("finding this program")?,
                state,
                way_in: serve.way_in.ok_or_else(missing)?,
                caller: background::Caller {
                    agent: serve.r#as.ok_or_else(missing)?,
                    keyring: serve.keyring,
                },
                at_once: serve.background.ok_or_else(missing)?,
                timeout: duration(&serve.background_timeout.ok_or_else(missing)?)?,
                output: serve.background_output.ok_or_else(missing)?,
                keep: duration(&serve.background_keep.ok_or_else(missing)?)?,
                per_day: serve.background_per_day.ok_or_else(missing)?,
                hops: serve.hops.ok_or_else(missing)?,
            })
        }
    };
    // Both streams, kept to it, fit one message to the tools bridge.
    let most = link_run_tool_contract::limits().output_most;
    anyhow::ensure!(
        serve.output_bytes <= most,
        "--output-bytes {} is above {most}, the most an answer under the tools bridge's message \
         cap holds",
        serve.output_bytes
    );
    Ok(Run {
        context,
        files: serve.files,
        path: serve.programs_path,
        commands: serve.program,
        sandbox: serve.sandbox,
        env,
        timeout: duration(&serve.timeout)?,
        output: usize::try_from(serve.output_bytes).context("--output-bytes is too large")?,
        background,
        conversations: serve.conversations,
    })
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    if err.is::<Usage>() {
        return Report::usage("lnk-run-tool");
    }
    let program = Product::new("lnk-run-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(out: &mut Out) -> anyhow::Result<Done> {
    // Its warnings are log lines: it runs with no one to read them.
    link_format::daemon::logs();
    let ended: anyhow::Result<()> = match cli::parse().command.command {
        Command::RunToolServe(given) => {
            let run = run_of(given)?;
            if let Some(background) = &run.background {
                // What ended while nobody looked: marked, and told by a
                // sweep of its own, which may wait on the bridge.
                background::sweep_quietly(&background.state);
                if background::has_untold(&background.state) {
                    let sweep = link_run_tool_contract::cli::RunToolSweepArgs {
                        state: background.state.clone(),
                    }
                    .argv();
                    let _ = link_secretive::process::command(&background.program)
                        .args(sweep)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .map(|mut child| std::thread::spawn(move || child.wait()));
                }
            }
            serve(
                &run,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        Command::RunToolSupervise(cli::RunToolSuperviseArgs { id, state, with_fd }) => {
            let context = with_fd
                .map(|fd| i32::try_from(fd).context("--with-fd is a descriptor"))
                .transpose()?
                .map(link_sandbox_client::RunWith::read_fd)
                .transpose()?;
            background::supervise_with(&state, &id, context.as_ref())
        }
        Command::RunToolGate(cli::RunToolGateArgs { command }) => background::gate(&command),
        Command::RunToolSweep(cli::RunToolSweepArgs { state }) => {
            let swept = background::sweep(&state);
            for id in &swept.untold {
                out.block(Note(sentence!(
                    "The ending of background command {} couldn't be told yet.",
                    Name::new("command", id)
                )));
            }
            Ok(())
        }
        Command::RunToolStop(cli::RunToolStopArgs { state }) => {
            for line in background::stop_all(&state, "the agent stopped") {
                out.data(format!("{line}\n").as_bytes());
            }
            Ok(())
        }
        Command::RunToolDelete(cli::RunToolDeleteArgs { state }) => {
            for line in background::forget(&state) {
                out.data(format!("{line}\n").as_bytes());
            }
            Ok(())
        }
        // Nothing to install: this program is the tool.
        Command::RunToolInstall(_) => Ok(()),
        // A model's calls, made over MCP through serve: no command line
        // reads them.
        Command::RunToolCommand(_)
        | Command::BackgroundList(_)
        | Command::BackgroundShow(_)
        | Command::BackgroundStop(_) => Err(anyhow::anyhow!(
            "command, list, read and stop are a model's calls, made through serve"
        )),
    };
    ended?;
    Ok(Done::shown())
}

#[cfg(test)]
link_test_kit::program!(unit);
