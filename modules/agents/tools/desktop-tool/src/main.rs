//! `lnk-desktop-tool`: Link desktop, a virtual desktop an agent sees and
//! works (`link_desktop_tool`), for any harness to reach as a
//! tool:
//!
//! ```sh
//! lnk-desktop-tool serve --display <n> --xauthority <file> [--xvfb <program>] -- <lnk> sandbox run
//! lnk-desktop-tool cookie create <file> --display <n>   # the display's cookie, made once
//! ```

use link_desktop_tool::cli::{self, Command};
use link_desktop_tool::{Config, Desktop, auth, mcp};
use link_format::{Done, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence};

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
        return Report::usage("lnk-desktop-tool");
    }
    let program = Product::new("lnk-desktop-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(_out: &mut Out) -> anyhow::Result<Done> {
    let ended: anyhow::Result<()> = match cli::parse().command.command {
        Command::DesktopToolServe(given) => {
            let config = Config {
                display: given.display,
                auth: given.xauthority,
                xvfb: given.xvfb,
                sandbox: given.sandbox,
            };
            mcp::serve(
                Desktop::new(config),
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        Command::CookieCreate(given) => auth::make(&given.file, given.display),
        // Nothing to install: this program is the tool.
        Command::DesktopToolInstall(_) => Ok(()),
        // A model's calls, made over MCP through serve: no command line
        // reads them.
        Command::DesktopToolScreenshot(_)
        | Command::DesktopToolClick(_)
        | Command::DesktopToolType(_)
        | Command::DesktopToolKey(_)
        | Command::DesktopToolScroll(_)
        | Command::DesktopToolDrag(_) => Err(anyhow::anyhow!(
            "the desktop's calls are a model's, made through serve"
        )),
    };
    ended?;
    Ok(Done::shown())
}

#[cfg(test)]
link_test_kit::program!(unit);
