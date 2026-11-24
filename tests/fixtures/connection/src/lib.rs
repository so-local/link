//! `lnk-connection-fixture`'s command line, generated from its
//! declarations (`declared/`): every command of the connection protocol,
//! whose shapes are the contract's (`link_connection_contract::declared`).

pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The connection protocol's command this run is given.
    pub fn operation() -> link_connection_contract::Operation {
        use link_connection_contract::Operation;
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        match parsed.command.command {
            Command::ConnectionFixtureConnect(given) => Operation::Connect {
                agent: given.agent,
                taken: given.taken,
            },
            Command::ConnectionFixtureLink(given) => Operation::Link { again: given.again },
            Command::Uninstall(given) => Operation::Uninstall(given),
            // Every other command is given nothing: the protocol's, by its
            // words.
            _ => Operation::of(&parsed.words).expect("a command of the connection protocol"),
        }
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
