//! A hosted-agent adapter backed by folders, without running any agents.
//! Read one request, log it, apply failure controls, dispatch, then reply.
//! [`host`] owns agent records and backups, [`moves`] implements the sealed
//! move protocol, and [`chat`] streams a deterministic answer.
//!
//! The account's `LNK_AGENT_PLACE_FIXTURE` names the fixture folder. A test can write
//! `fail-<command>` or `fail-<command>-<phase>` there to fail that operation
//! with the file's contents on stderr. `log` records `<command> <phase>`.
//! This fixture is never shipped and keeps plaintext state for inspection.

use std::{io::Write, path::PathBuf};

use anyhow::{Context, bail};
use link_agent_place::{self as cloud, Reply};

mod chat;
mod host;
mod moves;

use host::Host;

/// Its command line, generated from its declarations (`declared/`): the
/// agent place contract's commands of an account that hosts agents.
#[expect(
    dead_code,
    reason = "the generated handler and its dispatch: the fixture answers each command by its words"
)]
#[expect(
    clippy::enum_variant_names,
    reason = "every command of the agent place contract starts agent-, as the protocol names it"
)]
mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}

fn main() -> std::process::ExitCode {
    link_settings::output::exit(run())
}

fn run() -> anyhow::Result<()> {
    // The command by its words, as the protocol calls it (`agent-runs`).
    let parsed: link_spec::Parsed<cli::Cli> = cli::PROGRAM.parse();
    let command = parsed.words.join(" ");
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    let request = cloud::read_request_line(&mut input)?;
    let dir = request
        .account
        .env
        .get("LNK_AGENT_PLACE_FIXTURE")
        .map(|dir| PathBuf::from(dir.expose()))
        .context("the account names no folder (LNK_AGENT_PLACE_FIXTURE)")?;
    // Holding no team, it's an adapter without team commands, which a move
    // asks each time: not logged.
    if command == "agent-team" && !dir.join("teams.json").exists() {
        bail!("lnk-agent-place-fixture has no command \"agent-team\"");
    }
    let phase = request.phase.map(cloud::Phase::name).unwrap_or_default();
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("log"))?;
    writeln!(log, "{command} {phase}")?;
    for fail in [format!("fail-{command}"), format!("fail-{command}-{phase}")] {
        if let Ok(why) = std::fs::read_to_string(dir.join(&fail)) {
            // Written as it is, as an adapter may: lnk cleans it.
            let _ = std::io::stderr().write_all(why.as_bytes());
            bail!("the mini host was told to fail {command} {phase}");
        }
    }
    let host = Host { dir };
    let reply = match command.as_str() {
        "agent-runs" => Reply {
            runs_agents: Some(true),
            ..Reply::default()
        },
        // Told to, an answer that says nothing of its agents.
        "agent-list" if host.dir.join("silent-agent-list").exists() => Reply::default(),
        "agent-list" => host.list(request.owner.as_deref())?,
        "agent-create" => host.create(&request)?,
        "agent-connect" => host.connect(&request)?,
        "agent-in" => host.move_in(&request)?,
        "agent-out" => host.move_out(&request)?,
        "agent-receive" => host.take(&request, &mut input)?,
        "agent-send" => return host.give(&request),
        "agent-chat" => return host.chat(&request),
        "agent-left" => host.left(&request)?,
        "agent-abort" => host.abort(&request)?,
        "agent-backup" => host.backup(&request, false)?,
        "agent-restore" => host.backup(&request, true)?,
        "agent-team" => host.team(&request)?,
        other => bail!("lnk-agent-place-fixture has no command {other:?}"),
    };
    cloud::reply(&reply)
}
