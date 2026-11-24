//! `lnk-browser-tool`: Link browser, pages opened in a sandboxed
//! Chromium, read, clicked and seen (`link_browser_tool`), for any
//! harness to reach as a tool:
//!
//! ```sh
//! lnk-browser-tool serve --profile <folder> [--chromium <program>] [--floor owner|member] [--internet-host <host>]... [--private-host <host>]... -- <lnk> sandbox run
//! ```
//!
//! The browser runs in the sandbox after `--`, the browser's own flags
//! put after it; its profile is the agent's, kept where no harness
//! reaches.

use link_browser_tool::cli::{self, BrowserToolServeArgs, Command};
use link_browser_tool::{Browser, Config, mcp};
use link_connection_contract::access::Standing;
use link_format::{Done, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

fn config_of(given: BrowserToolServeArgs) -> Config {
    Config {
        profile: given.profile,
        chromium: given.chromium,
        internet_hosts: given.internet_host,
        private_hosts: given.private_host,
        floor: match given.floor {
            cli::BrowserToolServeArgsFloor::Owner => Standing::Owner,
            cli::BrowserToolServeArgsFloor::Member => Standing::Member,
        },
        // A browser runs only in a sandbox: its declaration requires one.
        sandbox: given.sandbox,
    }
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    if err.is::<Usage>() {
        return Report::usage("lnk-browser-tool");
    }
    let program = Product::new("lnk-browser-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(_out: &mut Out) -> anyhow::Result<Done> {
    let ended: anyhow::Result<()> = match cli::parse().command.command {
        Command::BrowserToolServe(given) => {
            let browser = Browser::new(config_of(given))?;
            mcp::serve(
                browser,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        // Nothing to install: this program is the tool.
        Command::BrowserToolInstall(_) => Ok(()),
        // A model's calls, made over MCP through serve: no command line
        // reads them.
        Command::BrowserToolOpen(_)
        | Command::BrowserToolRead(_)
        | Command::BrowserToolClick(_)
        | Command::BrowserToolType(_)
        | Command::BrowserToolSelect(_)
        | Command::BrowserToolScroll(_)
        | Command::BrowserToolScreenshot(_)
        | Command::BrowserToolDialog(_)
        | Command::BrowserToolKey(_)
        | Command::BrowserToolBack(_)
        | Command::BrowserToolHover(_)
        | Command::BrowserToolClose(_) => Err(anyhow::anyhow!(
            "the browser's calls are a model's, made through serve"
        )),
    };
    ended?;
    Ok(Done::shown())
}

#[cfg(test)]
link_test_kit::program!(unit);
