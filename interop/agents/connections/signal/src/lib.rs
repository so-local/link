//! `lnk-signal`: the Signal connection module, which `lnk agent connect
//! signal` runs (`link_connection_contract`), and which links the agent's
//! number and carries its messages for a harness that speaks no Signal
//! ([`bridge`]).

pub mod bridge;
mod format;
pub mod native;

use link_harness_contract::harness::State;

/// What of the connection's folder is the link (`lnk-signal state`): the
/// account and the number it's for. Not `signal-cli` and its Java, which
/// are built for this machine: another installs its own.
pub fn state() -> State {
    State {
        include: vec![native::DATA.into(), native::ACCOUNT.into()],
        exclude: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_account_moves_and_signal_cli_stays() {
        let state = super::state();
        assert_eq!(
            state.include,
            ["~/.signal-cli/data", "~/.signal-cli/account"]
        );
        assert!(
            state
                .include
                .iter()
                .all(|p| !p.contains(super::native::VERSION) && !p.contains("java"))
        );
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["include"][0], "~/.signal-cli/data");
    }
}

#[cfg(test)]
link_test_kit::program!(system);

/// `lnk-signal`'s command line, generated from its declarations: the
/// connection protocol's commands its contract declares (`actions.toml`),
/// whose shapes are the contract's (`link_connection_contract::declared`).
pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// What a run of `lnk-signal` is given: one of the connection
    /// protocol's commands, or one of its native program's (`cli`).
    pub enum Given {
        Connection(link_connection_contract::Operation),
        Native(crate::native::Native),
    }

    /// The command this run is given.
    pub fn given() -> Given {
        use crate::native::Native;
        use link_connection_contract::Operation;
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        Given::Connection(match parsed.command.command {
            Command::SignalConnect(given) => Operation::Connect {
                agent: given.agent,
                taken: given.taken,
            },
            Command::SignalLink(given) => Operation::Link { again: given.again },
            Command::Uninstall(given) => Operation::Uninstall(given),
            Command::CliInstall(given) => {
                return Given::Native(Native::Install { home: given.home });
            }
            Command::CliLink(given) => {
                return Given::Native(Native::Link {
                    home: given.home,
                    again: given.again,
                });
            }
            Command::CliState(_) => return Given::Native(Native::State),
            // Every other command is given nothing: the protocol's, by its
            // words.
            _ => Operation::of(&parsed.words).expect("a command of the connection protocol"),
        })
    }

    /// `lnk-signal`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}
