//! `lnk-web-tool`: Link web, a page fetched as markdown and a web search
//! (`link_web_tool`), for any harness to reach as a tool:
//!
//! ```sh
//! lnk-web-tool serve [--private-host <host>]...
//! ```
//!
//! Its search API's key is in its environment (`BRAVE_API_KEY` or
//! `TAVILY_API_KEY`, `lnk sandbox tool add --env`), never the harness's;
//! without one it offers only `fetch`. `--private-host` names a host on
//! this machine or its networks it may fetch, which it otherwise never
//! does.

use link_format::{Done, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence};
use link_web_tool::cli::{self, Command};
use link_web_tool::{Search, Web, mcp};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    if err.is::<Usage>() {
        return Report::usage("lnk-web-tool");
    }
    let program = Product::new("lnk-web-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(_out: &mut Out) -> anyhow::Result<Done> {
    match cli::parse().command.command {
        Command::WebToolServe(given) => {
            link_fetch::install_crypto_provider();
            let web = Web {
                search: Search::from_env()?,
                private_hosts: given.private_host,
                ..Web::default()
            };
            mcp::serve(
                web,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )?;
        }
        // Nothing to install: this program is the tool.
        Command::WebToolInstall(_) => {}
        // A model's calls, made over MCP through serve: no command line
        // reads them.
        Command::WebToolFetch(_) | Command::WebToolSearch(_) => {
            anyhow::bail!("fetch and search are a model's calls, made through serve")
        }
    }
    Ok(Done::shown())
}

#[cfg(test)]
link_test_kit::program!(unit);
