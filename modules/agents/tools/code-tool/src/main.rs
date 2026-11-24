//! `lnk-code-tool`: Link code, a project's language servers asked for
//! diagnostics, definitions and references (`link_code_tool`):
//!
//! ```sh
//! lnk-code-tool serve --files <folder> --programs-path <PATH> --timeout <duration> \
//!   --servers <json> [--unavailable <language>=<why>]... [--var NAME=VALUE]... -- <sandbox>...
//! ```
//!
//! The servers are the agent's spec's (`[tool.code.servers]`), those it
//! may not start its sandbox file's (`[commands]`) or this machine's, and
//! the sandbox `lnk sandbox run` with the flags that file makes, so none
//! is this program's own.

use std::collections::BTreeMap;

use anyhow::Context;
use link_code_tool::cli::{self, CodeToolServeArgs, Command};
use link_code_tool::{Code, Config, serve};
use link_format::{Done, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence};

fn main() -> std::process::ExitCode {
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

fn config_of(serve: CodeToolServeArgs) -> anyhow::Result<Config> {
    let timeout = Some(serve.timeout.get())
        .filter(|t| !t.is_zero())
        .context("--timeout is longer than none")?;
    let servers =
        serde_json::from_str::<BTreeMap<String, link_agent_spec::CodeServer>>(&serve.servers)
            .context("--servers isn't [tool.code.servers] as JSON")?;
    let mut unavailable = BTreeMap::new();
    for given in &serve.unavailable {
        let (language, why) = given
            .split_once('=')
            .with_context(|| format!("--unavailable {given:?} is <language>=<why>"))?;
        unavailable.insert(language.to_string(), why.to_string());
    }
    let env = serve
        .var
        .iter()
        .map(|given| {
            given
                .split_once('=')
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .with_context(|| format!("--var {given:?} is NAME=VALUE"))
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(Config {
        env,
        files: serve.files,
        path: serve.programs_path,
        // A server runs only in a sandbox: its declaration requires one.
        sandbox: serve.sandbox,
        servers,
        unavailable,
        timeout,
    })
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    if err.is::<Usage>() {
        return Report::usage("lnk-code-tool");
    }
    let program = Product::new("lnk-code-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(_out: &mut Out) -> anyhow::Result<Done> {
    let ended: anyhow::Result<()> = match cli::parse().command.command {
        Command::CodeToolServe(given) => {
            let code = Code::new(config_of(given)?)?;
            serve(
                &code,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        // Nothing to install: this program is the tool.
        Command::CodeToolInstall(_) => Ok(()),
        // A model's calls, made over MCP through serve: no command line
        // reads them.
        Command::CodeToolDiagnostics(_)
        | Command::CodeToolDefinition(_)
        | Command::CodeToolReferences(_) => Err(anyhow::anyhow!(
            "diagnostics, definition and references are a model's calls, made through serve"
        )),
    };
    ended?;
    Ok(Done::shown())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use link_code_tool::cli::Cli;

    #[test]
    fn serve_without_a_sandbox_runs_nothing() {
        let parse = |rest: &[&str]| {
            Cli::try_parse_from(
                [
                    "lnk-code-tool",
                    "serve",
                    "--files",
                    "/tmp",
                    "--programs-path",
                    "/usr/bin",
                    "--timeout",
                    "1m",
                    "--servers",
                    "{}",
                ]
                .iter()
                .chain(rest),
            )
            .map(|cli| match cli.command {
                Command::CodeToolServe(serve) => serve,
                _ => unreachable!(),
            })
        };
        assert!(parse(&[]).is_err());
        assert!(parse(&["--"]).is_err());
        let config =
            config_of(parse(&["--unavailable", "go=missing", "--", "lnk"]).unwrap()).unwrap();
        assert_eq!(config.unavailable["go"], "missing");
        assert_eq!(config.sandbox, ["lnk"]);
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
