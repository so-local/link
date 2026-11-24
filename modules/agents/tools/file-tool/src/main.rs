//! `lnk-file-tool`: Link file, an agent's files, its calls in a sandbox
//! of their own (`link_file_tool`):
//!
//! ```sh
//! lnk-file-tool serve [--no-find] [--no-diffs] [--team-bridge <team>... --socket <path> --as <agent id> --account <account> [--keyring <folder>]] -- <lnk> sandbox run <flags> -- lnk-file-tool apply --files <folder> --agent-folder <folder> [--whole | --write <folder>...]
//! lnk-file-tool bridge --socket <path> --store <path> --as <agent id> --account <account> [--keyring <folder>] [--write <folder>]... [--no-find] [--no-diffs]
//! lnk-file-tool apply --files <folder> --agent-folder <folder> [--whole] [--write <folder>]... [--team-folder <name>=<folder> [--team-write|--team-read <name>/<folder>]...]...
//! ```
//!
//! The sandbox and the folders written are the agent's sandbox file's
//! (`[files]`), so none is this program's own.

use std::path::PathBuf;

use anyhow::{Context, bail};
use link_file_tool::cli::{self, Command};
use link_file_tool::{
    BridgedTeams, Bridges, Files, Offer, Team, apply, serve, serve_bridged, serve_teamed,
};
use link_format::{Done, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

fn files_of(given: cli::FileToolApplyArgs) -> anyhow::Result<Files> {
    let mut teams = Vec::<Team>::new();
    for arg in &given.team_folder {
        let (name, folder) = arg
            .split_once('=')
            .context("--team-folder is <name>=<folder>")?;
        link_machine::machine::check_name(name)?;
        teams.push(Team {
            name: name.to_string(),
            root: PathBuf::from(folder),
            write: Vec::new(),
            read: Vec::new(),
        });
    }
    for (flag, args, write) in [
        ("--team-write", &given.team_write, true),
        ("--team-read", &given.team_read, false),
    ] {
        for arg in args {
            let (name, folder) = arg
                .split_once('/')
                .with_context(|| format!("{flag} is <team>/<folder>"))?;
            let team = teams
                .iter_mut()
                .find(|t| t.name == name)
                .with_context(|| format!("{flag} {arg}: no --team-folder {name}"))?;
            match write {
                true => team.write.push(folder.to_string()),
                false => team.read.push(folder.to_string()),
            }
        }
    }
    Ok(Files {
        folder: link_agent_contract::folder::Folder {
            home: given.files,
            agent: given.agent_folder,
        },
        // The whole files folder comes as `--whole`: a bare `.` reads as a
        // path, which `lnk sandbox tool add` refuses in a server's command.
        write: given
            .write
            .into_iter()
            .chain(given.whole.then(|| ".".to_string()))
            .collect(),
        teams,
    })
}

/// The files bridge, as agent `agent` of `account` bound in the keyring
/// at `keys` (this machine's, if none).
fn client_of(
    socket: PathBuf,
    agent: &str,
    account: &str,
    keys: Option<&std::path::Path>,
) -> anyhow::Result<link_store_contract::files::Client> {
    let keyring = match keys {
        Some(dir) => link_sandbox_binding::Keyring::at(dir),
        None => link_sandbox_binding::Keyring::here(),
    };
    // Its key while it's bound, and only as the account named.
    let binding = keyring
        .bound(agent)
        .filter(|b| b.identity.owner == account)
        .with_context(|| format!("agent {} isn't bound here", link_format::clean(agent)))?;
    Ok(link_store_contract::files::Client { socket, binding })
}

/// `serve`'s teams whose folders it reaches through the files bridge
/// (`--team-bridge <team>`, with the bridge's `--socket`, `--as`,
/// `--account` and `--keyring` as `bridge` takes them), none without one.
fn teamed_of(given: &cli::FileToolServeArgs) -> anyhow::Result<Option<BridgedTeams>> {
    if given.team_bridge.is_empty() {
        return Ok(None);
    }
    for team in &given.team_bridge {
        link_store_contract::files::TeamRef::parse(team)
            .ok()
            .filter(|t| t.owner.is_none())
            .with_context(|| format!("--team-bridge {team}: name a team of yours"))?;
    }
    // Its check hook holds the bridge's flags to `--team-bridge`.
    let (Some(socket), Some(agent), Some(account)) = (&given.socket, &given.r#as, &given.account)
    else {
        bail!(Usage);
    };
    let bridge = client_of(socket.clone(), agent, account, given.keyring.as_deref())?;
    Ok(Some(BridgedTeams {
        bridge,
        teams: given.team_bridge.clone(),
    }))
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    if err.is::<Usage>() {
        return Report::usage("lnk-file-tool");
    }
    let program = Product::new("lnk-file-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(_out: &mut Out) -> anyhow::Result<Done> {
    let ended: anyhow::Result<()> = match cli::parse().command.command {
        Command::FileToolServe(given) => {
            let mut offer = Offer::leaving(given.no_find, given.no_diffs);
            offer.write = given.write.clone();
            if given.whole {
                offer.write.push(".".into());
            }
            offer.team_write = given.team_write.clone();
            offer.team_read = given.team_read.clone();
            offer.team_bridge = given.team_bridge.clone();
            match teamed_of(&given)? {
                Some(teams) => serve_teamed(
                    &given.command,
                    &teams,
                    &offer,
                    std::io::stdin().lock(),
                    link_format::streams::protocol(),
                ),
                None => serve(
                    &given.command,
                    &offer,
                    std::io::stdin().lock(),
                    link_format::streams::protocol(),
                ),
            }
        }
        Command::FileToolBridge(given) => {
            let bridge = client_of(
                given.socket,
                &given.r#as,
                &given.account,
                given.keyring.as_deref(),
            )?;
            let offer = Offer {
                write: given.write.clone(),
                every_team: true,
                ..Offer::leaving(given.no_find, given.no_diffs)
            };
            // The storage bridge, its calls proved with the same key.
            let store = link_store_contract::store::Bridged {
                socket: given.store,
                binding: link_secretive::Duplicate::duplicate(&bridge.binding),
            };
            let bridges = Bridges {
                files: &bridge,
                store: Some(&store),
            };
            serve_bridged(
                &bridges,
                &given.write,
                &offer,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        Command::FileToolApply(given) => {
            let files = files_of(given)?;
            apply(
                &files,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        // Nothing to install: this program is the tool.
        Command::FileToolInstall(_) => Ok(()),
        // A model's calls, made over MCP through serve or bridge: no
        // command line reads them.
        Command::FileToolRead(_)
        | Command::FileToolWrite(_)
        | Command::FileToolEdit(_)
        | Command::FileToolGlob(_)
        | Command::FileToolGrep(_) => Err(anyhow::anyhow!(
            "read, write, edit, glob and grep are a model's calls, made through serve"
        )),
    };
    ended?;
    Ok(Done::shown())
}

#[cfg(test)]
link_test_kit::program!(unit);
