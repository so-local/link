//! Link Harness: Link's own harness, the smallest one that shows the
//! value. A router: it answers a chat completions endpoint (Link's
//! connections send it the owner's messages), calls the model through
//! Link's proxy, and keeps the conversation as events in the agent's
//! files. It runs no code and holds no keys: its tools are MCP servers
//! outside it ([`tools`]), and their calls are events of the
//! conversation like any message. It keeps nothing between turns that
//! any process couldn't make again: only what each tool offers, and each
//! tool's session until it's idle.
//!
//! The seams, each with one implementation: the agent is always an
//! argument ([`turn::Harness::turn`]), and each request names it
//! (`Link-Agent`); everything it reads and writes (its settings, the
//! files its prompt is made of, its conversations) goes through
//! [`store::Store`], keyed
//! by agent; settings are read at each turn ([`config`]); every path
//! comes from the agent through one function ([`agent::Agents::paths`]);
//! the prompt comes from a file or Link's optional section assembly
//! ([`prompt`]);
//! and one turn at a time runs per conversation (a lock in the process).

pub mod acp;
pub mod agent;
pub mod commands;
pub mod compact;
pub mod config;
pub mod event;
pub mod fallback;
pub mod goal;
pub mod limits;
pub mod measures;
pub mod model;
pub mod prompt;
pub mod repository;
pub mod said;
pub mod server;
pub mod store;
pub mod tools;
pub mod turn;
pub mod window;

#[cfg(test)]
link_test_kit::program!(system);

/// `lnk-harness`'s command line, generated from its declarations: the
/// harness protocol's commands its contract declares (`actions.toml`),
/// whose shapes are the contract's (`link_harness_contract::declared`),
/// and its own `serve`.
pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The command this run is given.
    pub fn parse() -> Command {
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        parsed.command.command
    }

    /// `lnk-harness`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}
